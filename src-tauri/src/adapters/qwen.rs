use std::{collections::HashSet, path::PathBuf};

use async_trait::async_trait;
use jsonc_parser::{
    ParseOptions,
    cst::{CstNode, CstRootNode},
};
use serde_json::{Map, Value};
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterApiCandidate, AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan,
        CliAdapter, FileWritePlan, FixedOAuthCommand, HostEnvironment, read_file_snapshot,
    },
    catalog::{ProviderCatalog, resolve_catalog_endpoint, runtime_catalog},
    config_templates::{
        QwenTemplateBindings, RenderedManagedConfig, TemplateBindings, TemplateSelection,
        render_managed_config, resolve_templates,
    },
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, ScanStatus,
        SourceFileSnapshot, VerificationInfo,
    },
    error::{AppError, AppResult},
    services::config_writer::{JsonPatch, parse_jsonc_value, patch_jsonc},
};

const QWEN_SCHEMA_FINGERPRINT: &str =
    "stable-v0.23:templated-modelProviders+providerProtocol+file-env+model.baseUrl";
const DEFAULT_OPENAI_ENV_KEY: &str = "OPENAI_API_KEY";

#[derive(Debug, Default)]
pub struct QwenAdapter;

struct QwenRoute {
    group: String,
    index: usize,
    model: String,
    raw_base_url: String,
    endpoint: Url,
    env_key: String,
    credential: Option<String>,
    special_only: bool,
    extra_auth: bool,
}

struct QwenAnalysis {
    routes: Vec<QwenRoute>,
    selected_model: Option<String>,
    selected_route: Option<usize>,
    selected_openai: bool,
    externally_overridden: bool,
    diagnostics: Vec<String>,
    scan_status_hint: Option<ScanStatus>,
}

struct CandidateGroup {
    endpoint: Url,
    credential: String,
    route_indices: Vec<usize>,
    models: Vec<String>,
}

fn ensure_absolute(paths: &AdapterPaths) -> AppResult<()> {
    if paths.config_directory.is_absolute() && paths.config_file.is_absolute() {
        Ok(())
    } else {
        Err(AppError::Blocked("QWEN_RELATIVE_HOME".into()))
    }
}

fn qwen_home_path(value: &str, home: &std::path::Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home.join(relative);
    }
    PathBuf::from(value)
}

fn snapshot_text<'a>(source: &'a Option<Vec<u8>>, default: &'a str) -> AppResult<&'a str> {
    match source {
        Some(source) => std::str::from_utf8(source)
            .map_err(|_| AppError::Serialization("QWEN_CONFIG_NOT_UTF8".into())),
        None => Ok(default),
    }
}

fn parse_qwen_jsonc(text: &str) -> AppResult<Value> {
    let effective = if text.trim().is_empty() { "{}\n" } else { text };
    let root = CstRootNode::parse(effective, &ParseOptions::default())
        .map_err(|_| AppError::Serialization("QWEN_MALFORMED_JSONC".into()))?;
    let object = root
        .object_value()
        .ok_or_else(|| AppError::Unsupported("QWEN_ROOT_NOT_OBJECT".into()))?;
    ensure_unique_object_properties(&object)?;
    parse_jsonc_value(effective)
}

fn ensure_unique_object_properties(object: &jsonc_parser::cst::CstObject) -> AppResult<()> {
    let mut names = HashSet::new();
    for property in object.properties() {
        let name = property
            .name()
            .ok_or_else(|| AppError::Serialization("QWEN_INVALID_PROPERTY_NAME".into()))?
            .decoded_value()
            .map_err(|_| AppError::Serialization("QWEN_INVALID_PROPERTY_NAME".into()))?;
        if !names.insert(name) {
            return Err(AppError::Unsupported("QWEN_DUPLICATE_PROPERTY".into()));
        }
        if let Some(value) = property.value() {
            ensure_unique_nested_properties(&value)?;
        }
    }
    Ok(())
}

fn ensure_unique_nested_properties(node: &CstNode) -> AppResult<()> {
    if let Some(object) = node.as_object() {
        ensure_unique_object_properties(&object)?;
    } else if let Some(array) = node.as_array() {
        for element in array.elements() {
            ensure_unique_nested_properties(&element)?;
        }
    }
    Ok(())
}

fn object_field<'a>(
    root: &'a Map<String, Value>,
    key: &str,
) -> AppResult<Option<&'a Map<String, Value>>> {
    root.get(key)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| AppError::Unsupported(object_error_code(key).into()))
        })
        .transpose()
}

fn object_error_code(key: &str) -> &'static str {
    match key {
        "modelProviders" => "QWEN_MODEL_PROVIDERS_NOT_OBJECT",
        "providerProtocol" => "QWEN_PROVIDER_PROTOCOL_NOT_OBJECT",
        "env" => "QWEN_ENV_NOT_OBJECT",
        "model" => "QWEN_MODEL_NOT_OBJECT",
        "security" => "QWEN_SECURITY_NOT_OBJECT",
        _ => "QWEN_MANAGED_FIELD_NOT_OBJECT",
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn valid_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn contains_qwen_env_reference(value: &str) -> bool {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'$' {
            continue;
        }
        let Some(next) = bytes.get(index + 1) else {
            continue;
        };
        if next.is_ascii_alphanumeric() || *next == b'_' {
            return true;
        }
        if *next == b'{'
            && bytes[index + 2..]
                .iter()
                .position(|candidate| *candidate == b'}')
                .is_some_and(|closing| closing > 0)
        {
            return true;
        }
    }
    false
}

enum FileCredential {
    Missing,
    ExternalReference,
    Literal(String),
}

fn file_credential(env: Option<&Map<String, Value>>, env_key: &str) -> FileCredential {
    let Some(value) = nonempty_string(env.and_then(|env| env.get(env_key))) else {
        return FileCredential::Missing;
    };
    if contains_qwen_env_reference(value) {
        return FileCredential::ExternalReference;
    }
    if value.contains("${") || value.contains("$(") || value.contains('`') {
        return FileCredential::Missing;
    }
    FileCredential::Literal(value.to_string())
}

fn is_special_only(model: &Map<String, Value>) -> bool {
    ["imageOnly", "voiceOnly", "visionOnly", "fastOnly"]
        .iter()
        .any(|key| model.get(*key).and_then(Value::as_bool) == Some(true))
}

fn has_extra_authentication(model: &Map<String, Value>) -> bool {
    model.values().any(contains_authentication_header)
}

fn contains_authentication_header(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "authorization" | "api-key" | "x-api-key" | "proxy-authorization"
            ) || contains_authentication_header(value)
        }),
        Value::Array(values) => values.iter().any(contains_authentication_header),
        _ => false,
    }
}

fn effective_protocol<'a>(
    group: &str,
    protocols: Option<&'a Map<String, Value>>,
) -> AppResult<Option<&'a str>> {
    if let Some(value) = protocols.and_then(|protocols| protocols.get(group)) {
        return value
            .as_str()
            .map(Some)
            .ok_or_else(|| AppError::Unsupported("QWEN_PROVIDER_PROTOCOL_NOT_STRING".into()));
    }
    Ok((group == "openai").then_some("openai"))
}

fn analyze_qwen(value: &Value, environment: &HostEnvironment) -> AppResult<QwenAnalysis> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Unsupported("QWEN_ROOT_NOT_OBJECT".into()))?;
    if let Some(version) = root.get("$version")
        && version.as_u64() != Some(4)
    {
        return Err(AppError::Unsupported("QWEN_UNSUPPORTED_VERSION".into()));
    }
    let providers = object_field(root, "modelProviders")?;
    let protocols = object_field(root, "providerProtocol")?;
    let env = object_field(root, "env")?;
    let model_selection = object_field(root, "model")?;
    let security = object_field(root, "security")?;
    let auth = security
        .and_then(|security| security.get("auth"))
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| AppError::Unsupported("QWEN_SECURITY_AUTH_NOT_OBJECT".into()))
        })
        .transpose()?;
    if let Some(auth) = auth {
        for (key, code) in [
            ("selectedType", "QWEN_SELECTED_TYPE_NOT_STRING"),
            ("enforcedType", "QWEN_ENFORCED_TYPE_NOT_STRING"),
        ] {
            if auth.get(key).is_some_and(|value| !value.is_string()) {
                return Err(AppError::Unsupported(code.into()));
            }
        }
        if auth
            .get("useExternal")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(AppError::Unsupported(
                "QWEN_USE_EXTERNAL_NOT_BOOLEAN".into(),
            ));
        }
    }

    let mut routes = Vec::new();
    let mut diagnostics = Vec::new();
    if let Some(providers) = providers {
        for (group, entries) in providers {
            if entries.as_object().is_some_and(|legacy| {
                legacy.contains_key("protocol") || legacy.contains_key("models")
            }) {
                return Err(AppError::Unsupported("QWEN_LEGACY_PROVIDER_SCHEMA".into()));
            }
            if effective_protocol(group, protocols)? != Some("openai") {
                diagnostics.push("QWEN_UNSUPPORTED_PROTOCOL".into());
                continue;
            }
            let entries = entries
                .as_array()
                .ok_or_else(|| AppError::Unsupported("QWEN_OPENAI_PROVIDER_NOT_ARRAY".into()))?;
            for (index, entry) in entries.iter().enumerate() {
                let entry = entry
                    .as_object()
                    .ok_or_else(|| AppError::Unsupported("QWEN_OPENAI_MODEL_NOT_OBJECT".into()))?;
                let model = match entry.get("id") {
                    Some(value) if !value.is_string() => {
                        return Err(AppError::Unsupported("QWEN_MODEL_ID_NOT_STRING".into()));
                    }
                    value => match nonempty_string(value) {
                        Some(model) => model,
                        None => {
                            diagnostics.push("QWEN_MISSING_MODEL_ID".into());
                            continue;
                        }
                    },
                };
                let raw_base_url = match entry.get("baseUrl") {
                    Some(value) if !value.is_string() => {
                        return Err(AppError::Unsupported("QWEN_BASE_URL_NOT_STRING".into()));
                    }
                    value => match nonempty_string(value) {
                        Some(base_url) => base_url,
                        None => {
                            diagnostics.push("QWEN_MISSING_BASE_URL".into());
                            continue;
                        }
                    },
                };
                let endpoint = match resolve_catalog_endpoint(raw_base_url) {
                    Ok(endpoint) => endpoint,
                    Err(_) => {
                        diagnostics.push("QWEN_INVALID_BASE_URL".into());
                        continue;
                    }
                };
                let env_key = match entry.get("envKey") {
                    Some(value) => nonempty_string(Some(value))
                        .filter(|name| valid_env_name(name))
                        .ok_or_else(|| AppError::Unsupported("QWEN_INVALID_ENV_KEY".into()))?,
                    None => DEFAULT_OPENAI_ENV_KEY,
                };
                if env
                    .and_then(|env| env.get(env_key))
                    .is_some_and(|value| !value.is_string())
                {
                    return Err(AppError::Unsupported(
                        "QWEN_ENV_CREDENTIAL_NOT_STRING".into(),
                    ));
                }
                let credential = match file_credential(env, env_key) {
                    FileCredential::Missing => {
                        diagnostics.push("QWEN_MISSING_FILE_CREDENTIAL".into());
                        None
                    }
                    FileCredential::ExternalReference => {
                        diagnostics.push("QWEN_EXTERNAL_CREDENTIAL_REFERENCE".into());
                        None
                    }
                    FileCredential::Literal(value) => Some(value),
                };
                let special_only = is_special_only(entry);
                if special_only {
                    diagnostics.push("QWEN_SPECIAL_ONLY_MODEL".into());
                }
                routes.push(QwenRoute {
                    group: group.clone(),
                    index,
                    model: model.into(),
                    raw_base_url: raw_base_url.into(),
                    endpoint,
                    env_key: env_key.into(),
                    credential,
                    special_only,
                    extra_auth: has_extra_authentication(entry),
                });
            }
        }
    }
    if routes.iter().enumerate().any(|(index, route)| {
        routes.iter().skip(index + 1).any(|other| {
            !route.special_only
                && !other.special_only
                && route.model == other.model
                && route_key(&route.endpoint) == route_key(&other.endpoint)
        })
    }) {
        diagnostics.push("QWEN_AMBIGUOUS_ROUTE".into());
    }

    let selected_type = auth.and_then(|auth| nonempty_string(auth.get("selectedType")));
    let selected_openai = selected_type == Some("openai");
    let selected_model = model_selection.and_then(|model| nonempty_string(model.get("name")));
    let selected_base_url = model_selection.and_then(|model| nonempty_string(model.get("baseUrl")));
    let mut selected_route = None;
    if selected_openai {
        if let Some(selected_model) = selected_model {
            let matches = routes
                .iter()
                .enumerate()
                .filter(|(_, route)| {
                    !route.special_only
                        && route.model == selected_model
                        && selected_base_url
                            .map(|base_url| route.raw_base_url == base_url)
                            .unwrap_or(true)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [index]
                    if routes
                        .iter()
                        .filter(|route| {
                            !route.special_only
                                && route.model == selected_model
                                && route_key(&route.endpoint) == route_key(&routes[*index].endpoint)
                        })
                        .count()
                        == 1 =>
                {
                    selected_route = Some(*index)
                }
                [] => diagnostics.push("QWEN_SELECTED_ROUTE_NOT_FOUND".into()),
                _ => diagnostics.push("QWEN_AMBIGUOUS_ROUTE".into()),
            }
        } else {
            diagnostics.push("QWEN_MISSING_SELECTION".into());
        }
    } else if selected_type.is_some() {
        diagnostics.push("QWEN_UNSUPPORTED_AUTH".into());
    } else if !routes.is_empty() {
        diagnostics.push("QWEN_MISSING_SELECTION".into());
    }
    if routes.is_empty()
        && auth.is_some_and(|auth| auth.contains_key("apiKey") || auth.contains_key("baseUrl"))
    {
        diagnostics.push("QWEN_LEGACY_RUNTIME_CONFIG".into());
    }

    let externally_overridden = selected_route
        .and_then(|index| routes.get(index))
        .is_some_and(|route| environment.is_present(&route.env_key));
    if externally_overridden {
        diagnostics.push("QWEN_EXTERNAL_OVERRIDE".into());
    }
    diagnostics.sort();
    diagnostics.dedup();
    let has_complete_candidates = routes
        .iter()
        .any(|route| !route.special_only && route.credential.is_some());
    let current_complete = selected_route
        .and_then(|index| routes.get(index))
        .is_some_and(|route| route.credential.is_some());
    let scan_status_hint = if !current_complete
        && (has_complete_candidates
            || !routes.is_empty()
            || selected_type.is_some()
            || selected_model.is_some())
    {
        Some(ScanStatus::PartiallyDetected)
    } else {
        None
    };
    Ok(QwenAnalysis {
        routes,
        selected_model: selected_model.map(str::to_string),
        selected_route,
        selected_openai,
        externally_overridden,
        diagnostics,
        scan_status_hint,
    })
}

fn route_key(endpoint: &Url) -> String {
    endpoint.as_str().trim_end_matches('/').to_string()
}

fn template_identity(group: &str, endpoint: &Url) -> AppResult<(Option<String>, String)> {
    let catalog = runtime_catalog()?;
    let matched = catalog.dynamic_provider_info(group).filter(|info| {
        info.selectable
            && info.endpoints.iter().any(|candidate| {
                candidate.selectable
                    && candidate.protocol == Some(CliProtocol::OpenaiChat)
                    && candidate
                        .endpoint
                        .as_ref()
                        .is_some_and(|candidate| route_key(candidate) == route_key(endpoint))
            })
    });
    Ok(match matched {
        Some(info) => (Some(info.id.clone()), info.name.clone()),
        None => (None, format!("Qwen Code API ({group})")),
    })
}

fn candidate_groups(analysis: &QwenAnalysis) -> Vec<CandidateGroup> {
    let mut groups: Vec<CandidateGroup> = Vec::new();
    for (route_index, route) in analysis.routes.iter().enumerate() {
        let Some(credential) = route.credential.as_ref().filter(|_| !route.special_only) else {
            continue;
        };
        if analysis
            .routes
            .iter()
            .filter(|candidate| {
                !candidate.special_only
                    && candidate.model == route.model
                    && route_key(&candidate.endpoint) == route_key(&route.endpoint)
            })
            .count()
            > 1
        {
            continue;
        }
        if let Some(group) = groups.iter_mut().find(|group| {
            route_key(&group.endpoint) == route_key(&route.endpoint)
                && group.credential == *credential
        }) {
            group.route_indices.push(route_index);
            if !group.models.contains(&route.model) {
                group.models.push(route.model.clone());
            }
        } else {
            groups.push(CandidateGroup {
                endpoint: route.endpoint.clone(),
                credential: credential.clone(),
                route_indices: vec![route_index],
                models: vec![route.model.clone()],
            });
        }
    }
    groups
}

fn validate_connection_identity(
    catalog: &ProviderCatalog,
    provider: &ProviderProfile,
    connection: &ProviderConnection,
) -> AppResult<()> {
    match (
        provider.template_id.as_deref(),
        connection.template_endpoint_id.as_deref(),
    ) {
        (Some(template_id), Some(endpoint_id)) => catalog
            .api_relation(CliId::Qwen, template_id, endpoint_id)
            .filter(|_| connection.protocol == CliProtocol::OpenaiChat)
            .map(|_| ())
            .ok_or_else(|| AppError::Validation("QWEN_INCOMPATIBLE_CONNECTION".into())),
        (Some(template_id), None)
            if catalog
                .dynamic_provider_info(template_id)
                .is_some_and(|info| info.supported_clis.contains(&CliId::Qwen)) =>
        {
            Ok(())
        }
        (None, None) if connection.protocol == CliProtocol::OpenaiChat => Ok(()),
        _ => Err(AppError::Validation("QWEN_INCOMPATIBLE_CONNECTION".into())),
    }
}

fn check_write_policy(root: &Map<String, Value>) -> AppResult<()> {
    let Some(security) = root.get("security") else {
        return Ok(());
    };
    let security = security
        .as_object()
        .ok_or_else(|| AppError::Unsupported("QWEN_SECURITY_NOT_OBJECT".into()))?;
    let Some(auth) = security.get("auth") else {
        return Ok(());
    };
    let auth = auth
        .as_object()
        .ok_or_else(|| AppError::Unsupported("QWEN_SECURITY_AUTH_NOT_OBJECT".into()))?;
    if auth.get("useExternal").and_then(Value::as_bool) == Some(true) {
        return Err(AppError::Unsupported("QWEN_EXTERNAL_AUTH_POLICY".into()));
    }
    if let Some(enforced) = nonempty_string(auth.get("enforcedType"))
        && enforced != "openai"
    {
        return Err(AppError::Unsupported("QWEN_ENFORCED_AUTH_CONFLICT".into()));
    }
    Ok(())
}

fn semantic_settings_valid(bytes: &[u8]) -> AppResult<bool> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| AppError::Serialization(error.to_string()))?;
    let value = parse_qwen_jsonc(text)?;
    let environment = HostEnvironment {
        home: PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" }),
        variables: Default::default(),
        present_variables: Default::default(),
        os: std::env::consts::OS.into(),
    };
    let analysis = analyze_qwen(&value, &environment)?;
    let Some(route) = analysis
        .selected_route
        .and_then(|index| analysis.routes.get(index))
    else {
        return Ok(false);
    };
    Ok(analysis.selected_openai && route.credential.is_some() && !route.special_only)
}

#[async_trait]
impl CliAdapter for QwenAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::Qwen,
            display_name: "Qwen Code".into(),
            command: "qwen".into(),
            schema_fingerprint: QWEN_SCHEMA_FINGERPRINT.into(),
        }
    }

    fn resolve_paths(
        &self,
        environment: &HostEnvironment,
        manual: Option<PathBuf>,
    ) -> AdapterPaths {
        let directory = manual
            .or_else(|| {
                environment
                    .value("QWEN_HOME")
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| qwen_home_path(value, &environment.home))
            })
            .unwrap_or_else(|| environment.home.join(".qwen"));
        AdapterPaths {
            config_file: directory.join("settings.json"),
            auth_file: None,
            config_directory: directory,
        }
    }

    async fn read_current(
        &self,
        paths: &AdapterPaths,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterReadResult> {
        ensure_absolute(paths)?;
        let (source, digest) =
            read_file_snapshot(&paths.config_file, &paths.config_directory).await?;
        let text = snapshot_text(&source, "{}\n")?;
        let value = parse_qwen_jsonc(text)?;
        let analysis = analyze_qwen(&value, environment)?;
        let groups = candidate_groups(&analysis);
        let mut candidates = Vec::with_capacity(groups.len());
        for group in groups {
            let first_index = *group.route_indices.first().expect("candidate route");
            let first = &analysis.routes[first_index];
            let (template_id, suggested_name) = template_identity(&first.group, &group.endpoint)?;
            let is_current = analysis
                .selected_route
                .is_some_and(|selected| group.route_indices.contains(&selected))
                && !analysis.externally_overridden;
            let default_model = if is_current {
                analysis.selected_model.clone()
            } else {
                group.models.first().cloned()
            };
            candidates.push(AdapterApiCandidate {
                source_provider_id: template_id.clone().unwrap_or_else(|| first.group.clone()),
                suggested_name,
                template_id,
                connection: ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiChat,
                    endpoint: group.endpoint,
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: group.credential,
                    default_model: default_model.clone().unwrap_or_default(),
                    verification: VerificationInfo::default(),
                },
                available_models: group.models,
                default_model,
                is_current,
                model_routed: false,
            });
        }
        let selected = analysis
            .selected_route
            .and_then(|index| analysis.routes.get(index));
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: selected.map(|route| route.group.clone()),
                protocol: analysis.selected_openai.then_some(CliProtocol::OpenaiChat),
                auth_kind: analysis.selected_openai.then(|| "api".into()),
                model: analysis.selected_model,
                managed_provider_id: None,
                managed_connection_id: None,
                sources: vec![SourceFileSnapshot {
                    source_id: "qwen-settings".into(),
                    display_path: paths.config_file.clone(),
                    digest,
                }],
                externally_overridden: analysis.externally_overridden,
                diagnostics: analysis.diagnostics,
            },
            unmanaged_api_candidates: candidates,
            scan_status_hint: analysis.scan_status_hint,
        })
    }

    async fn plan_write(
        &self,
        paths: &AdapterPaths,
        target: &ConfigurationTarget,
        provider: &ProviderProfile,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterWritePlan> {
        ensure_absolute(paths)?;
        let (connection_id, connection) = match (target, &provider.data) {
            (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => {
                let connection = api
                    .connections
                    .iter()
                    .find(|connection| connection.id == *connection_id)
                    .ok_or_else(|| AppError::Validation("connection does not exist".into()))?;
                (*connection_id, connection)
            }
            _ => return Err(AppError::Validation("QWEN_INCOMPATIBLE_CONNECTION".into())),
        };
        if connection.protocol != CliProtocol::OpenaiChat
            || connection.auth_type != ConnectionAuthType::Bearer
        {
            return Err(AppError::Validation("QWEN_INCOMPATIBLE_CONNECTION".into()));
        }
        if contains_qwen_env_reference(&connection.api_key) {
            return Err(AppError::Unsupported(
                "QWEN_EXTERNAL_CREDENTIAL_REFERENCE".into(),
            ));
        }
        let catalog = runtime_catalog()?;
        validate_connection_identity(&catalog, provider, connection)?;
        let group_id = format!("cliswitch_qwen_{}", connection_id.simple());
        let env_key = format!(
            "CLISWITCH_QWEN_KEY_{}",
            connection_id.simple().to_string().to_ascii_uppercase()
        );
        if environment.is_present(&env_key) {
            return Err(AppError::Unsupported("QWEN_TARGET_ENV_OVERRIDE".into()));
        }
        let (source, source_digest) =
            read_file_snapshot(&paths.config_file, &paths.config_directory).await?;
        let source_text = snapshot_text(&source, "{}\n")?;
        let value = parse_qwen_jsonc(source_text)?;
        let root = value
            .as_object()
            .ok_or_else(|| AppError::Unsupported("QWEN_ROOT_NOT_OBJECT".into()))?;
        check_write_policy(root)?;
        let analysis = analyze_qwen(&value, environment)?;
        let target_endpoint = connection.endpoint.as_str();
        let target_route_key = target_endpoint.trim_end_matches('/');
        if analysis.routes.iter().any(|route| {
            route.special_only
                && route.model == target.model()
                && route_key(&route.endpoint) == target_route_key
        }) {
            return Err(AppError::Unsupported("QWEN_SPECIAL_ONLY_MODEL".into()));
        }
        let matches = analysis
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| {
                !route.special_only
                    && route.model == target.model()
                    && route_key(&route.endpoint) == target_route_key
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(AppError::Unsupported("QWEN_AMBIGUOUS_ROUTE".into()));
        }
        if matches.first().is_some_and(|(_, route)| route.extra_auth) {
            return Err(AppError::Unsupported("QWEN_EXTRA_AUTH_SETTINGS".into()));
        }
        let templates = resolve_templates(&TemplateSelection {
            cli_id: CliId::Qwen,
            template_id: provider.template_id.as_deref(),
            protocol: connection.protocol,
            model: target.model(),
        })?;
        let rendered = render_managed_config(
            &templates,
            &TemplateBindings {
                provider_id: &group_id,
                provider_name: &provider.name,
                endpoint: target_endpoint,
                auth_type: connection.auth_type,
                api_key: &connection.api_key,
                model: target.model(),
                model_catalog_path: None,
                qwen: Some(QwenTemplateBindings {
                    group_id: &group_id,
                    env_key: &env_key,
                }),
            },
        )?;
        let RenderedManagedConfig::Qwen(rendered) = rendered else {
            return Err(AppError::Serialization(
                "resolved a non-Qwen config template".into(),
            ));
        };
        let mut patches = Vec::new();
        if let Some((_, route)) = matches.first() {
            for (field, value) in [
                ("id", rendered.model.as_str()),
                ("name", rendered.model.as_str()),
                ("baseUrl", rendered.endpoint.as_str()),
                ("envKey", rendered.env_key.as_str()),
            ] {
                patches.push(JsonPatch::SetArrayObjectString {
                    array_path: vec!["modelProviders".into(), route.group.clone()],
                    index: route.index,
                    object_path: vec![field.into()],
                    value: value.into(),
                });
            }
        } else {
            if let Some(providers) = root.get("modelProviders") {
                let providers = providers.as_object().ok_or_else(|| {
                    AppError::Unsupported("QWEN_MODEL_PROVIDERS_NOT_OBJECT".into())
                })?;
                if let Some(existing) = providers.get(&rendered.group_id) {
                    if effective_protocol(
                        &rendered.group_id,
                        root.get("providerProtocol").and_then(Value::as_object),
                    )? != Some("openai")
                    {
                        return Err(AppError::Unsupported("QWEN_GROUP_PROTOCOL_CONFLICT".into()));
                    }
                    if !existing.is_array() {
                        return Err(AppError::Unsupported("QWEN_GROUP_TYPE_CONFLICT".into()));
                    }
                }
            }
            patches.push(JsonPatch::AppendArrayObject {
                path: vec!["modelProviders".into(), rendered.group_id.clone()],
                value: rendered.model_entry,
            });
            patches.push(JsonPatch::SetString {
                path: vec!["providerProtocol".into(), rendered.group_id.clone()],
                value: rendered.protocol,
            });
        }
        patches.extend([
            JsonPatch::SetString {
                path: vec!["env".into(), rendered.env_key.clone()],
                value: rendered.api_key,
            },
            JsonPatch::SetString {
                path: vec!["security".into(), "auth".into(), "selectedType".into()],
                value: "openai".into(),
            },
            JsonPatch::SetString {
                path: vec!["model".into(), "name".into()],
                value: rendered.model,
            },
            JsonPatch::SetString {
                path: vec!["model".into(), "baseUrl".into()],
                value: rendered.endpoint,
            },
        ]);
        let target_content = patch_jsonc(source_text, &patches)?.into_bytes();
        if !semantic_settings_valid(&target_content)? {
            return Err(AppError::Serialization("QWEN_RENDER_VERIFY_FAILED".into()));
        }
        Ok(AdapterWritePlan {
            cli_id: CliId::Qwen,
            files: vec![FileWritePlan {
                path: paths.config_file.clone(),
                allowed_root: paths.config_directory.clone(),
                source_content: source,
                source_digest,
                target_content,
                contains_credentials: true,
                opaque_content: false,
            }],
            warning: Some("QWEN_RESTART_REQUIRED".into()),
        })
    }

    async fn verify_applied(&self, plan: &AdapterWritePlan) -> AppResult<bool> {
        for file in &plan.files {
            let (Some(current), _) = read_file_snapshot(&file.path, &file.allowed_root).await?
            else {
                return Ok(false);
            };
            if current != file.target_content || !semantic_settings_valid(&current)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn oauth_kind(&self) -> Option<OAuthKind> {
        None
    }

    fn validate_imported_auth(&self, _bytes: &[u8]) -> AppResult<Option<String>> {
        Err(AppError::Unsupported("QWEN_OAUTH_UNSUPPORTED".into()))
    }

    fn fixed_oauth_command(
        &self,
        _executable: PathBuf,
        _isolated_home: PathBuf,
    ) -> AppResult<FixedOAuthCommand> {
        Err(AppError::Unsupported("QWEN_OAUTH_UNSUPPORTED".into()))
    }
}
