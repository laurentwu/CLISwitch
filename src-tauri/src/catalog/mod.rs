use std::collections::{BTreeMap, HashSet};

use jsonc_parser::{ParseOptions, parse_to_serde_value};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use url::Url;

use crate::{
    domain::{CliId, CliProtocol, ConnectionAuthType, OAuthKind},
    error::{AppError, AppResult},
};

const CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliCatalogFile {
    schema_version: u32,
    clis: Vec<CatalogCli>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTemplateCatalogFile {
    schema_version: u32,
    provider_templates: Vec<ProviderTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelationCatalogFile {
    schema_version: u32,
    relations: Vec<CliProviderRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalog {
    pub schema_version: u32,
    pub clis: Vec<CatalogCli>,
    pub provider_templates: Vec<ProviderTemplate>,
    pub relations: Vec<CliProviderRelation>,
    /// Backend-resolved compatibility information for every upstream provider. Keeping this
    /// separate from the raw snapshot lets the UI explain why a provider is disabled without
    /// ever trusting arbitrary npm packages or endpoint values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_info: Option<Vec<CatalogProviderInfo>>,
}

pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// A normalized, lossless-enough representation of the fields CLISwitch consumes from
/// models.dev. Unknown upstream fields are retained in `extra` so a cache refresh does not turn
/// into an accidental schema downgrade when models.dev adds metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevCatalog {
    pub providers: Vec<ModelsDevProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevProvider {
    /// The provider map key is the stable identity. Older snapshots did not always repeat it in
    /// the object, so normalization fills an omitted value from that key below.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub env: Vec<String>,
    pub npm: String,
    #[serde(default)]
    pub api: Option<String>,
    pub name: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub models: BTreeMap<String, ModelsDevModel>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevModel {
    /// The model map key is the stable identity; see [`ModelsDevCatalog::from_api_json`].
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub provider: Option<ModelsDevModelProvider>,
    #[serde(default)]
    pub limit: Option<ModelsDevModelLimit>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevModelProvider {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub body: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDevModelLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModelInfo {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
    pub selectable: bool,
    pub disabled_reason: Option<String>,
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProviderInfo {
    pub id: String,
    pub name: String,
    pub npm: String,
    pub env: Vec<String>,
    pub api: Option<String>,
    pub doc: String,
    pub protocol: Option<CliProtocol>,
    pub auth_type: Option<ConnectionAuthType>,
    pub endpoint: Option<Url>,
    pub selectable: bool,
    pub disabled_reason: Option<String>,
    pub supported_clis: Vec<CliId>,
    pub models: Vec<CatalogModelInfo>,
}

#[derive(Debug, Clone, Copy)]
struct AdapterRule {
    protocol: CliProtocol,
    auth_type: ConnectionAuthType,
    default_endpoint: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCli {
    pub id: CliId,
    pub name: String,
    pub protocols: Vec<CliProtocol>,
    pub auth_modes: Vec<CatalogAuthMode>,
    pub protocol_adapters: Vec<CliProtocolAdapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAuthMode {
    pub id: String,
    pub oauth_kind: OAuthKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliProtocolAdapter {
    pub protocol: CliProtocol,
    pub provider_package: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum ProviderTemplate {
    Api(ApiProviderTemplate),
    Auth(AuthProviderTemplate),
}

impl ProviderTemplate {
    pub fn id(&self) -> &str {
        match self {
            Self::Api(template) => &template.id,
            Self::Auth(template) => &template.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Api(template) => &template.name,
            Self::Auth(template) => &template.name,
        }
    }

    pub const fn mode(&self) -> &'static str {
        match self {
            Self::Api(_) => "api",
            Self::Auth(_) => "auth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProviderTemplate {
    pub id: String,
    pub name: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub model_routing: bool,
    pub credential_slots: Vec<CredentialSlotTemplate>,
    pub endpoints: Vec<ProviderEndpointTemplate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_models: Vec<UnsupportedProviderModelTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProviderTemplate {
    pub id: String,
    pub name: String,
    pub auth_kind: OAuthKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSlotTemplate {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEndpointTemplate {
    pub id: String,
    pub name: String,
    pub protocol: CliProtocol,
    pub base_url: Url,
    pub credential_slot_id: String,
    pub auth_options: Vec<EndpointAuthOption>,
    pub default_auth_option_id: String,
    pub models: Vec<ProviderModelTemplate>,
}

impl ProviderEndpointTemplate {
    pub fn default_auth_type(&self) -> Option<ConnectionAuthType> {
        self.auth_options
            .iter()
            .find(|option| option.id == self.default_auth_option_id)
            .map(|option| option.auth_type)
    }

    pub fn default_model(&self) -> Option<&str> {
        self.models
            .iter()
            .find(|model| model.default)
            .or_else(|| self.models.first())
            .map(|model| model.id.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointAuthOption {
    pub id: String,
    pub auth_type: ConnectionAuthType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub default: bool,
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedProviderModelTemplate {
    pub id: String,
    pub name: String,
    pub provider_package: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum CliProviderRelation {
    Api(ApiCliProviderRelation),
    Auth(AuthCliProviderRelation),
}

impl CliProviderRelation {
    pub fn id(&self) -> &str {
        match self {
            Self::Api(relation) => &relation.id,
            Self::Auth(relation) => &relation.id,
        }
    }

    pub const fn cli_id(&self) -> CliId {
        match self {
            Self::Api(relation) => relation.cli_id,
            Self::Auth(relation) => relation.cli_id,
        }
    }

    pub fn provider_template_id(&self) -> &str {
        match self {
            Self::Api(relation) => &relation.provider_template_id,
            Self::Auth(relation) => &relation.provider_template_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCliProviderRelation {
    pub id: String,
    pub cli_id: CliId,
    pub provider_template_id: String,
    pub endpoint_id: String,
    pub auth_option_id: String,
    #[serde(default)]
    pub base_url: Option<Url>,
    #[serde(default)]
    pub provider_package: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub native_provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCliProviderRelation {
    pub id: String,
    pub cli_id: CliId,
    pub provider_template_id: String,
    pub auth_mode_id: String,
}

impl ModelsDevCatalog {
    pub fn from_api_json(bytes: &[u8]) -> AppResult<Self> {
        let mut providers = serde_json::from_slice::<BTreeMap<String, ModelsDevProvider>>(bytes)?;
        if providers.is_empty() {
            return invalid("models.dev catalog contains no providers".into());
        }
        let mut normalized = Vec::with_capacity(providers.len());
        for (map_id, provider) in providers.iter_mut() {
            if provider.id.trim().is_empty() {
                provider.id = map_id.clone();
            }
            if provider.id != *map_id {
                return invalid(format!(
                    "models.dev provider key {map_id} does not match id {}",
                    provider.id
                ));
            }
            ensure_source_identifier("models.dev provider ID", &provider.id)?;
            ensure_nonempty("models.dev provider name", &provider.name)?;
            ensure_nonempty("models.dev provider npm", &provider.npm)?;
            for environment_name in &provider.env {
                ensure_nonempty("models.dev environment name", environment_name)?;
            }
            ensure_unique_nonempty_strings(
                "models.dev environment name",
                provider.env.iter().map(String::as_str),
            )?;
            let mut model_ids = HashSet::new();
            for (map_model_id, model) in &mut provider.models {
                if model.id.trim().is_empty() {
                    model.id = map_model_id.clone();
                }
                if model.id != *map_model_id {
                    return invalid(format!(
                        "models.dev provider {} model key {map_model_id} does not match id {}",
                        provider.id, model.id
                    ));
                }
                ensure_source_identifier("models.dev model ID", &model.id)?;
                ensure_nonempty("models.dev model name", &model.name)?;
                if !model_ids.insert(model.id.as_str()) {
                    return invalid(format!(
                        "models.dev provider {} repeats model {}",
                        provider.id, model.id
                    ));
                }
                // Status values are upstream-extensible. Unknown values remain in the snapshot
                // and are exposed as disabled hints by `resolve_model`, so one new status cannot
                // invalidate an otherwise usable downloaded database.
            }
            normalized.push(provider.clone());
        }
        normalized.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Self {
            providers: normalized,
        })
    }

    pub fn bundled() -> AppResult<Self> {
        Self::from_api_json(include_bytes!("../../catalog/models.dev.json"))
    }

    pub fn provider(&self, provider_id: &str) -> Option<&ModelsDevProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id == provider_id)
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    pub fn model_count(&self) -> usize {
        self.providers
            .iter()
            .map(|provider| provider.models.len())
            .sum()
    }

    pub fn provider_info(&self) -> Vec<CatalogProviderInfo> {
        self.providers.iter().map(resolve_provider).collect()
    }
}

fn adapter_rule(npm: &str) -> Option<AdapterRule> {
    match npm {
        "@ai-sdk/openai" => Some(AdapterRule {
            protocol: CliProtocol::OpenaiResponses,
            auth_type: ConnectionAuthType::Bearer,
            default_endpoint: Some("https://api.openai.com/v1"),
        }),
        "@ai-sdk/anthropic" => Some(AdapterRule {
            protocol: CliProtocol::AnthropicMessages,
            auth_type: ConnectionAuthType::ApiKey,
            default_endpoint: Some("https://api.anthropic.com"),
        }),
        "@ai-sdk/openai-compatible" | "@openrouter/ai-sdk-provider" => Some(AdapterRule {
            protocol: CliProtocol::OpenaiChat,
            auth_type: ConnectionAuthType::Bearer,
            default_endpoint: None,
        }),
        _ => None,
    }
}

/// Returns the protocol represented by one of the built-in models.dev adapters. The upstream
/// package names are data only; callers use this helper to recognize an entry and then select a
/// fixed, compiled-in transport implementation.
pub(crate) fn fixed_adapter_protocol(npm: &str) -> Option<CliProtocol> {
    adapter_rule(npm).map(|rule| rule.protocol)
}

fn supported_clis(protocol: CliProtocol) -> Vec<CliId> {
    match protocol {
        CliProtocol::AnthropicMessages => vec![CliId::ClaudeCode, CliId::Opencode],
        CliProtocol::OpenaiResponses => vec![CliId::Codex, CliId::Opencode],
        CliProtocol::OpenaiChat => vec![CliId::Opencode],
    }
}

fn resolve_provider(provider: &ModelsDevProvider) -> CatalogProviderInfo {
    let rule = adapter_rule(&provider.npm);
    let endpoint_result = rule.and_then(|rule| {
        provider
            .api
            .as_deref()
            .filter(|api| !api.trim().is_empty())
            .or(rule.default_endpoint)
            .map(resolve_catalog_endpoint)
    });
    let (endpoint, endpoint_error) = match endpoint_result {
        Some(Ok(endpoint)) => (Some(endpoint), None),
        Some(Err(error)) => (None, Some(error)),
        None => (
            None,
            rule.map(|_| "models.dev does not provide a usable API endpoint".to_string()),
        ),
    };
    let disabled_reason = match rule {
        None => Some(format!("unsupported provider package {}", provider.npm)),
        Some(_) if provider.env.is_empty() => {
            Some("models.dev provider has no credential environment name".into())
        }
        Some(_) if provider.env.len() != 1 => Some(format!(
            "models.dev provider requires {} credential environment names; CLISwitch supports one API key",
            provider.env.len()
        )),
        Some(_) => endpoint_error,
    };
    let mut models = provider
        .models
        .values()
        .map(|model| resolve_model(provider, rule, model))
        .collect::<Vec<_>>();
    models.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    let supported_clis = if disabled_reason.is_none() {
        rule.map_or_else(Vec::new, |rule| supported_clis(rule.protocol))
    } else {
        Vec::new()
    };
    CatalogProviderInfo {
        id: provider.id.clone(),
        name: provider.name.clone(),
        npm: provider.npm.clone(),
        env: provider.env.clone(),
        // Do not echo an unsafe or unresolved upstream URL to the renderer. The raw value is
        // still used to produce a disabled reason, but only a URL which passes the same endpoint
        // policy as a saved connection is exposed as metadata.
        api: provider
            .api
            .as_deref()
            .filter(|api| resolve_catalog_endpoint(api).is_ok())
            .map(str::to_string),
        doc: provider.doc.clone(),
        protocol: rule.map(|rule| rule.protocol),
        auth_type: rule.map(|rule| rule.auth_type),
        endpoint,
        selectable: disabled_reason.is_none(),
        disabled_reason,
        supported_clis,
        models,
    }
}

fn resolve_model(
    provider: &ModelsDevProvider,
    rule: Option<AdapterRule>,
    model: &ModelsDevModel,
) -> CatalogModelInfo {
    let mut disabled_reason = match model.status.as_deref() {
        Some("deprecated") => Some("model is deprecated".to_string()),
        Some("alpha" | "beta") | None => None,
        Some(_) => Some("model has an unknown status".to_string()),
    };
    if disabled_reason.is_none()
        && let (Some(rule), Some(override_provider)) = (rule, model.provider.as_ref())
    {
        if override_provider.npm.as_deref().is_some_and(|npm| {
            adapter_rule(npm).map(|candidate| candidate.protocol) != Some(rule.protocol)
        }) {
            disabled_reason = Some("model requires a different provider adapter".into());
        } else if override_provider.shape.as_deref().is_some_and(|shape| {
            matches!(
                (shape, rule.protocol),
                ("responses", CliProtocol::OpenaiChat)
                    | ("completions", CliProtocol::OpenaiResponses)
            )
        }) {
            disabled_reason = Some("model requires a different wire protocol".into());
        } else if override_provider.api.is_some()
            || override_provider
                .body
                .as_ref()
                .is_some_and(|body| !body.is_empty())
            || override_provider
                .headers
                .as_ref()
                .is_some_and(|headers| !headers.is_empty())
        {
            // models.dev can describe per-model endpoint/body/header overrides. CLISwitch keeps
            // routing at provider scope, so these entries remain visible as hints but cannot be
            // selected until a provider-level adapter can represent the override safely.
            disabled_reason = Some("model has a provider-specific request override".into());
        }
    }
    if disabled_reason.is_none() && adapter_rule(&provider.npm).is_none() {
        disabled_reason = Some("provider adapter is unsupported".into());
    }
    CatalogModelInfo {
        id: model.id.clone(),
        name: model.name.clone(),
        status: model.status.clone(),
        selectable: disabled_reason.is_none(),
        disabled_reason,
        context: model.limit.as_ref().and_then(|limit| limit.context),
        output: model.limit.as_ref().and_then(|limit| limit.output),
    }
}

pub(crate) fn resolve_catalog_endpoint(raw: &str) -> Result<Url, String> {
    if raw.contains("${") {
        return Err("API endpoint contains an unresolved environment placeholder".into());
    }
    let endpoint = Url::parse(raw).map_err(|_| "API endpoint is not a valid URL".to_string())?;
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err("API endpoint contains embedded credentials".into());
    }
    let host = endpoint
        .host_str()
        .ok_or_else(|| "API endpoint has no host".to_string())?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback) {
        return Err("API endpoint must use HTTPS (HTTP is allowed only for loopback)".into());
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err("API endpoint must not contain a query or fragment".into());
    }
    Ok(endpoint)
}

impl ProviderCatalog {
    /// Loads the bundled models.dev snapshot and builds the runtime catalog used by the app.
    ///
    /// The compatibility catalog is intentionally derived from the upstream snapshot at load
    /// time; callers which need the pre-migration fixtures must use [`Self::load_legacy`].
    pub fn load_embedded() -> AppResult<Self> {
        Self::load_runtime_embedded()
    }

    /// Loads the retired hand-maintained catalog. It remains available only for migration and
    /// regression fixtures while persisted API profiles move to models.dev identities.
    pub fn load_legacy() -> AppResult<Self> {
        let cli_file: CliCatalogFile =
            parse_catalog_file("clis.jsonc", include_str!("../../catalog/clis.jsonc"))?;
        let template_file: ProviderTemplateCatalogFile = parse_catalog_file(
            "provider-templates.jsonc",
            include_str!("../../catalog/provider-templates.jsonc"),
        )?;
        let relation_file: RelationCatalogFile = parse_catalog_file(
            "cli-provider-relations.jsonc",
            include_str!("../../catalog/cli-provider-relations.jsonc"),
        )?;
        for (name, version) in [
            ("clis.jsonc", cli_file.schema_version),
            ("provider-templates.jsonc", template_file.schema_version),
            ("cli-provider-relations.jsonc", relation_file.schema_version),
        ] {
            if version != CATALOG_SCHEMA_VERSION {
                return Err(AppError::Serialization(format!(
                    "unsupported {name} schema version {version}"
                )));
            }
        }
        let catalog = Self {
            schema_version: CATALOG_SCHEMA_VERSION,
            clis: cli_file.clis,
            provider_templates: template_file.provider_templates,
            relations: relation_file.relations,
            provider_info: None,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    /// Builds the runtime catalog from a models.dev snapshot. The legacy template fields are
    /// populated only for providers which pass the fixed adapter policy; the raw upstream data
    /// and resolved disabled reasons are exposed through `provider_info`; the raw snapshot stays
    /// backend-only so a full upstream document is not duplicated in every IPC snapshot.
    pub fn from_models_dev(models: ModelsDevCatalog) -> AppResult<Self> {
        let cli_file: CliCatalogFile =
            parse_catalog_file("clis.jsonc", include_str!("../../catalog/clis.jsonc"))?;
        let mut provider_info = models.provider_info();
        let mut provider_templates = Vec::new();
        let mut relations = Vec::new();
        let mut relation_ids = HashSet::new();
        for info in &provider_info {
            // Disabled upstream entries remain visible through `provider_info`, but must not
            // become selectable templates. In particular, a known adapter without any
            // credential environment name has a protocol and endpoint yet cannot safely be
            // materialized into a provider instance.
            if !info.selectable {
                continue;
            }
            let (Some(protocol), Some(endpoint)) = (info.protocol, info.endpoint.clone()) else {
                continue;
            };
            let template = ApiProviderTemplate {
                id: info.id.clone(),
                name: info.name.clone(),
                category: "models.dev".into(),
                model_routing: false,
                credential_slots: vec![CredentialSlotTemplate {
                    id: "api-key".into(),
                    name: "API Key".into(),
                }],
                endpoints: vec![ProviderEndpointTemplate {
                    id: "default".into(),
                    name: protocol.to_string(),
                    protocol,
                    base_url: endpoint,
                    credential_slot_id: "api-key".into(),
                    auth_options: if protocol == CliProtocol::AnthropicMessages {
                        vec![
                            EndpointAuthOption {
                                id: "api-key".into(),
                                auth_type: ConnectionAuthType::ApiKey,
                            },
                            EndpointAuthOption {
                                id: "bearer".into(),
                                auth_type: ConnectionAuthType::Bearer,
                            },
                        ]
                    } else {
                        vec![EndpointAuthOption {
                            id: "bearer".into(),
                            auth_type: ConnectionAuthType::Bearer,
                        }]
                    },
                    default_auth_option_id: if protocol == CliProtocol::AnthropicMessages {
                        "api-key".into()
                    } else {
                        "bearer".into()
                    },
                    // Model metadata is carried once in `provider_info`. Keeping generated
                    // endpoint templates model-free avoids duplicating a several-megabyte
                    // upstream snapshot in every IPC app snapshot.
                    models: Vec::new(),
                }],
                unsupported_models: Vec::new(),
            };
            provider_templates.push(ProviderTemplate::Api(template));
            for cli_id in supported_clis(protocol) {
                let relation_base =
                    format!("models-dev-{}-{}", sanitize_identifier(&info.id), cli_id);
                let mut relation_id = relation_base.clone();
                let mut suffix = 2;
                while !relation_ids.insert(relation_id.clone()) {
                    relation_id = format!("{relation_base}-{suffix}");
                    suffix += 1;
                }
                relations.push(CliProviderRelation::Api(ApiCliProviderRelation {
                    id: relation_id,
                    cli_id,
                    provider_template_id: info.id.clone(),
                    endpoint_id: "default".into(),
                    auth_option_id: if protocol == CliProtocol::AnthropicMessages {
                        "api-key".into()
                    } else {
                        "bearer".into()
                    },
                    base_url: None,
                    // The package is retained only after the fixed adapter allowlist has
                    // resolved it. Adapters may use it as a declarative OpenCode package hint;
                    // no upstream package is ever loaded or executed.
                    provider_package: Some(info.npm.clone()),
                    default: true,
                    native_provider_ids: vec![info.id.clone()],
                }));
            }
        }
        // OAuth is deliberately not sourced from models.dev. These two fixed modes are part of
        // the CLI support contract and have no provider package in the upstream database.
        provider_templates.extend([
            ProviderTemplate::Auth(AuthProviderTemplate {
                id: "anthropic-auth".into(),
                name: "Anthropic Account".into(),
                auth_kind: OAuthKind::Anthropic,
            }),
            ProviderTemplate::Auth(AuthProviderTemplate {
                id: "codex-auth".into(),
                name: "Codex Account".into(),
                auth_kind: OAuthKind::Codex,
            }),
        ]);
        relations.extend([
            CliProviderRelation::Auth(AuthCliProviderRelation {
                id: "models-dev-anthropic-auth".into(),
                cli_id: CliId::ClaudeCode,
                provider_template_id: "anthropic-auth".into(),
                auth_mode_id: "anthropic-oauth".into(),
            }),
            CliProviderRelation::Auth(AuthCliProviderRelation {
                id: "models-dev-codex-auth".into(),
                cli_id: CliId::Codex,
                provider_template_id: "codex-auth".into(),
                auth_mode_id: "codex-oauth".into(),
            }),
        ]);
        // A provider may have a valid adapter but an unrepresentable credential shape or no
        // selectable model. Keep its information in the UI; generated endpoint templates are
        // omitted for every disabled entry.
        provider_info.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        let catalog = Self {
            schema_version: CATALOG_SCHEMA_VERSION,
            clis: cli_file.clis,
            provider_templates,
            relations,
            provider_info: Some(provider_info),
        };
        catalog.validate_runtime()?;
        Ok(catalog)
    }

    pub fn load_runtime_embedded() -> AppResult<Self> {
        Self::from_models_dev(ModelsDevCatalog::bundled()?)
    }

    pub fn dynamic_provider_info(&self, provider_id: &str) -> Option<&CatalogProviderInfo> {
        self.provider_info
            .as_ref()?
            .iter()
            .find(|provider| provider.id == provider_id)
    }

    pub fn dynamic_models(&self, provider_id: &str) -> Vec<CatalogModelInfo> {
        self.dynamic_provider_info(provider_id)
            .map(|provider| provider.models.clone())
            .unwrap_or_default()
    }

    fn validate_runtime(&self) -> AppResult<()> {
        // Validate the fixed CLI contract and all generated references, but do not require every
        // upstream provider to have a template: unsupported providers are intentionally visible
        // as disabled entries.
        validate_cli_definitions(&self.clis)?;

        let mut template_ids = HashSet::new();
        for template in &self.provider_templates {
            let id = template.id();
            if !template_ids.insert(id) {
                return invalid(format!("duplicate provider template {id}"));
            }
            match template {
                ProviderTemplate::Api(template) => {
                    ensure_source_identifier("models.dev provider template", &template.id)?;
                    validate_api_template(template)?;
                }
                ProviderTemplate::Auth(template) => {
                    ensure_identifier("auth provider template", &template.id)?;
                    ensure_nonempty("auth provider template name", &template.name)?;
                }
            }
        }

        let mut relation_ids = HashSet::new();
        let mut native_ids = HashSet::new();
        let mut routes = HashSet::new();
        for relation in &self.relations {
            ensure_identifier("CLI provider relation", relation.id())?;
            if !relation_ids.insert(relation.id()) {
                return invalid(format!("duplicate relation {}", relation.id()));
            }
            let cli = self.cli(relation.cli_id()).ok_or_else(|| {
                AppError::Serialization(format!(
                    "relation {} references unknown CLI {}",
                    relation.id(),
                    relation.cli_id()
                ))
            })?;
            match relation {
                CliProviderRelation::Api(relation) => {
                    if !routes.insert((
                        relation.cli_id,
                        relation.provider_template_id.as_str(),
                        relation.endpoint_id.as_str(),
                    )) {
                        return invalid(format!(
                            "duplicate CLI/template/endpoint relation {}",
                            relation.id
                        ));
                    }
                    let template = self
                        .api_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing API template",
                                relation.id
                            ))
                        })?;
                    let endpoint = template
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.id == relation.endpoint_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing endpoint",
                                relation.id
                            ))
                        })?;
                    if !cli.protocols.contains(&endpoint.protocol) {
                        return invalid(format!(
                            "relation {} connects {} to unsupported protocol {}",
                            relation.id, cli.id, endpoint.protocol
                        ));
                    }
                    if !endpoint
                        .auth_options
                        .iter()
                        .any(|option| option.id == relation.auth_option_id)
                    {
                        return invalid(format!(
                            "relation {} references a missing auth option",
                            relation.id
                        ));
                    }
                    if let Some(package) = relation.provider_package.as_deref() {
                        ensure_nonempty("relation provider package", package)?;
                        if let Some(protocol) = fixed_adapter_protocol(package)
                            && protocol != endpoint.protocol
                        {
                            return invalid(format!(
                                "relation {} provider package does not match endpoint protocol",
                                relation.id
                            ));
                        }
                        if let Some(protocol) = self.package_protocol(cli.id, package)
                            && protocol != endpoint.protocol
                        {
                            return invalid(format!(
                                "relation {} provider package does not match CLI protocol",
                                relation.id
                            ));
                        }
                    }
                    for native_id in &relation.native_provider_ids {
                        ensure_source_identifier("native provider ID", native_id)?;
                        if !native_ids.insert((relation.cli_id, native_id.as_str())) {
                            return invalid(format!(
                                "CLI {} repeats native provider ID {native_id}",
                                relation.cli_id
                            ));
                        }
                    }
                }
                CliProviderRelation::Auth(relation) => {
                    let template = self
                        .auth_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing auth template",
                                relation.id
                            ))
                        })?;
                    if !cli.auth_modes.iter().any(|mode| {
                        mode.id == relation.auth_mode_id && mode.oauth_kind == template.auth_kind
                    }) {
                        return invalid(format!(
                            "relation {} references an incompatible CLI auth mode",
                            relation.id
                        ));
                    }
                }
            }
        }
        for template in &self.provider_templates {
            if !self
                .relations
                .iter()
                .any(|relation| relation.provider_template_id() == template.id())
            {
                return invalid(format!(
                    "provider template {} has no CLI relation",
                    template.id()
                ));
            }
        }

        let Some(provider_info) = self.provider_info.as_ref() else {
            return Ok(());
        };
        let mut info_ids = HashSet::new();
        let dynamic_template_ids = self
            .provider_templates
            .iter()
            .filter_map(|template| match template {
                ProviderTemplate::Api(template) if template.category == "models.dev" => {
                    Some(template.id.as_str())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        for info in provider_info {
            ensure_source_identifier("models.dev provider ID", &info.id)?;
            ensure_nonempty("models.dev provider name", &info.name)?;
            ensure_nonempty("models.dev provider npm", &info.npm)?;
            ensure_unique_nonempty_strings(
                "models.dev environment name",
                info.env.iter().map(String::as_str),
            )?;
            if !info_ids.insert(info.id.as_str()) {
                return invalid(format!("duplicate models.dev provider {}", info.id));
            }
            let mut model_ids = HashSet::new();
            for model in &info.models {
                ensure_source_identifier("models.dev model ID", &model.id)?;
                ensure_nonempty("models.dev model name", &model.name)?;
                if !model_ids.insert(model.id.as_str()) {
                    return invalid(format!(
                        "models.dev provider {} repeats model {}",
                        info.id, model.id
                    ));
                }
                if model.selectable && model.disabled_reason.is_some() {
                    return invalid(format!(
                        "selectable models.dev model {} has a disabled reason",
                        model.id
                    ));
                }
                if !model.selectable && model.disabled_reason.is_none() {
                    return invalid(format!(
                        "disabled models.dev model {} has no reason",
                        model.id
                    ));
                }
            }
            let template = self.api_template(&info.id);
            if info.selectable {
                let protocol = info.protocol.ok_or_else(|| {
                    AppError::Serialization(format!(
                        "selectable models.dev provider {} has no protocol",
                        info.id
                    ))
                })?;
                let endpoint = info.endpoint.as_ref().ok_or_else(|| {
                    AppError::Serialization(format!(
                        "selectable models.dev provider {} has no endpoint",
                        info.id
                    ))
                })?;
                let auth_type = info.auth_type.ok_or_else(|| {
                    AppError::Serialization(format!(
                        "selectable models.dev provider {} has no auth type",
                        info.id
                    ))
                })?;
                if !dynamic_template_ids.contains(info.id.as_str()) {
                    return invalid(format!(
                        "selectable models.dev provider {} has no generated template",
                        info.id
                    ));
                }
                if info.supported_clis != supported_clis(protocol) {
                    return invalid(format!(
                        "models.dev provider {} has an invalid CLI compatibility list",
                        info.id
                    ));
                }
                let Some(template) = template else {
                    return invalid(format!(
                        "selectable models.dev provider {} has no generated template",
                        info.id
                    ));
                };
                let generated_endpoint = template.endpoints.first().ok_or_else(|| {
                    AppError::Serialization(format!(
                        "models.dev provider {} has no generated endpoint",
                        info.id
                    ))
                })?;
                if generated_endpoint.protocol != protocol
                    || generated_endpoint.base_url != *endpoint
                    || generated_endpoint
                        .default_auth_type()
                        .is_none_or(|value| value != auth_type)
                {
                    return invalid(format!(
                        "generated template for models.dev provider {} disagrees with metadata",
                        info.id
                    ));
                }
            } else if template.is_some() {
                return invalid(format!(
                    "disabled models.dev provider {} has a generated template",
                    info.id
                ));
            } else if !info.supported_clis.is_empty() {
                return invalid(format!(
                    "disabled models.dev provider {} has supported CLIs",
                    info.id
                ));
            }
        }
        for template_id in dynamic_template_ids {
            if !info_ids.contains(template_id) {
                return invalid(format!(
                    "generated models.dev template {template_id} has no provider metadata"
                ));
            }
        }
        Ok(())
    }

    pub fn cli(&self, cli_id: CliId) -> Option<&CatalogCli> {
        self.clis.iter().find(|cli| cli.id == cli_id)
    }

    pub fn template(&self, template_id: &str) -> Option<&ProviderTemplate> {
        self.provider_templates
            .iter()
            .find(|template| template.id() == template_id)
    }

    pub fn api_template(&self, template_id: &str) -> Option<&ApiProviderTemplate> {
        match self.template(template_id) {
            Some(ProviderTemplate::Api(template)) => Some(template),
            _ => None,
        }
    }

    pub fn auth_template(&self, template_id: &str) -> Option<&AuthProviderTemplate> {
        match self.template(template_id) {
            Some(ProviderTemplate::Auth(template)) => Some(template),
            _ => None,
        }
    }

    pub fn auth_template_for_kind(&self, kind: OAuthKind) -> Option<&AuthProviderTemplate> {
        self.provider_templates
            .iter()
            .find_map(|template| match template {
                ProviderTemplate::Auth(template) if template.auth_kind == kind => Some(template),
                _ => None,
            })
    }

    pub fn api_relations(
        &self,
        cli_id: CliId,
        template_id: &str,
    ) -> impl Iterator<Item = &ApiCliProviderRelation> {
        self.relations
            .iter()
            .filter_map(move |relation| match relation {
                CliProviderRelation::Api(relation)
                    if relation.cli_id == cli_id
                        && relation.provider_template_id == template_id =>
                {
                    Some(relation)
                }
                _ => None,
            })
    }

    pub fn api_relation(
        &self,
        cli_id: CliId,
        template_id: &str,
        endpoint_id: &str,
    ) -> Option<&ApiCliProviderRelation> {
        self.api_relations(cli_id, template_id)
            .find(|relation| relation.endpoint_id == endpoint_id)
    }

    pub fn relation_auth_type(
        &self,
        relation: &ApiCliProviderRelation,
    ) -> Option<ConnectionAuthType> {
        self.api_template(&relation.provider_template_id)?
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == relation.endpoint_id)?
            .auth_options
            .iter()
            .find(|option| option.id == relation.auth_option_id)
            .map(|option| option.auth_type)
    }

    pub fn supports_api_endpoint(
        &self,
        cli_id: CliId,
        template_id: &str,
        endpoint_id: &str,
    ) -> bool {
        self.api_relations(cli_id, template_id)
            .any(|relation| relation.endpoint_id == endpoint_id)
    }

    pub fn supports_auth_template(&self, cli_id: CliId, template_id: &str) -> bool {
        self.relations.iter().any(|relation| {
            matches!(
                relation,
                CliProviderRelation::Auth(relation)
                    if relation.cli_id == cli_id
                        && relation.provider_template_id == template_id
            )
        })
    }

    pub fn native_api_relation(
        &self,
        cli_id: CliId,
        native_provider_id: &str,
    ) -> Option<&ApiCliProviderRelation> {
        self.relations.iter().find_map(|relation| match relation {
            CliProviderRelation::Api(relation)
                if relation.cli_id == cli_id
                    && relation
                        .native_provider_ids
                        .iter()
                        .any(|id| id == native_provider_id) =>
            {
                Some(relation)
            }
            _ => None,
        })
    }

    pub fn model_routed_endpoint(
        &self,
        template_id: &str,
        model_id: &str,
    ) -> Option<&ProviderEndpointTemplate> {
        let template = self.api_template(template_id)?;
        if !template.model_routing {
            return None;
        }
        let mut matches = template
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.models.iter().any(|model| model.id == model_id));
        let endpoint = matches.next()?;
        if matches.next().is_some() {
            None
        } else {
            Some(endpoint)
        }
    }

    pub fn unsupported_model(
        &self,
        template_id: &str,
        model_id: &str,
    ) -> Option<&UnsupportedProviderModelTemplate> {
        self.api_template(template_id)?
            .unsupported_models
            .iter()
            .find(|model| model.id == model_id)
    }

    pub fn protocol_package(&self, cli_id: CliId, protocol: CliProtocol) -> Option<&str> {
        self.cli(cli_id)?
            .protocol_adapters
            .iter()
            .find(|adapter| adapter.protocol == protocol)
            .map(|adapter| adapter.provider_package.as_str())
    }

    pub fn package_protocol(&self, cli_id: CliId, provider_package: &str) -> Option<CliProtocol> {
        self.cli(cli_id)?
            .protocol_adapters
            .iter()
            .find(|adapter| adapter.provider_package == provider_package)
            .map(|adapter| adapter.protocol)
    }

    pub fn supports_protocol(&self, cli_id: CliId, protocol: CliProtocol) -> bool {
        self.cli(cli_id)
            .is_some_and(|cli| cli.protocols.contains(&protocol))
    }

    fn validate(&self) -> AppResult<()> {
        let mut cli_ids = HashSet::new();
        for cli in &self.clis {
            if !cli_ids.insert(cli.id) {
                return invalid(format!("duplicate CLI {}", cli.id));
            }
            ensure_nonempty("CLI name", &cli.name)?;
            ensure_unique_strings(
                "CLI auth mode",
                cli.auth_modes.iter().map(|mode| mode.id.as_str()),
            )?;
            let mut protocols = HashSet::new();
            for protocol in &cli.protocols {
                if !protocols.insert(*protocol) {
                    return invalid(format!("CLI {} repeats protocol {protocol}", cli.id));
                }
            }
            let mut adapters = HashSet::new();
            for adapter in &cli.protocol_adapters {
                if !protocols.contains(&adapter.protocol) {
                    return invalid(format!(
                        "CLI {} has an adapter for unsupported protocol {}",
                        cli.id, adapter.protocol
                    ));
                }
                if !adapters.insert(adapter.protocol) {
                    return invalid(format!(
                        "CLI {} repeats the adapter for {}",
                        cli.id, adapter.protocol
                    ));
                }
                ensure_nonempty("provider package", &adapter.provider_package)?;
            }
        }
        for required in CliId::ALL {
            if !cli_ids.contains(&required) {
                return invalid(format!("catalog omits CLI {required}"));
            }
        }

        let mut template_ids = HashSet::new();
        for template in &self.provider_templates {
            ensure_identifier("provider template", template.id())?;
            ensure_nonempty("provider template name", template.name())?;
            if !template_ids.insert(template.id()) {
                return invalid(format!("duplicate provider template {}", template.id()));
            }
            match template {
                ProviderTemplate::Api(template) => validate_api_template(template)?,
                ProviderTemplate::Auth(template) => {
                    if self
                        .provider_templates
                        .iter()
                        .filter_map(|candidate| match candidate {
                            ProviderTemplate::Auth(candidate)
                                if candidate.auth_kind == template.auth_kind =>
                            {
                                Some(candidate)
                            }
                            _ => None,
                        })
                        .count()
                        != 1
                    {
                        return invalid(format!(
                            "auth kind {} must have exactly one template",
                            template.auth_kind
                        ));
                    }
                }
            }
        }

        let mut relation_ids = HashSet::new();
        let mut native_ids = HashSet::new();
        let mut api_routes = HashSet::new();
        let mut default_api_routes = HashSet::new();
        let mut auth_routes = HashSet::new();
        for relation in &self.relations {
            ensure_identifier("CLI provider relation", relation.id())?;
            if !relation_ids.insert(relation.id()) {
                return invalid(format!("duplicate relation {}", relation.id()));
            }
            let cli = self.cli(relation.cli_id()).ok_or_else(|| {
                AppError::Serialization(format!(
                    "relation {} references unknown CLI {}",
                    relation.id(),
                    relation.cli_id()
                ))
            })?;
            match relation {
                CliProviderRelation::Api(relation) => {
                    if !api_routes.insert((
                        relation.cli_id,
                        relation.provider_template_id.as_str(),
                        relation.endpoint_id.as_str(),
                    )) {
                        return invalid(format!(
                            "duplicate CLI/template/endpoint relation {}",
                            relation.id
                        ));
                    }
                    if relation.default
                        && !default_api_routes
                            .insert((relation.cli_id, relation.provider_template_id.as_str()))
                    {
                        return invalid(format!(
                            "CLI {} template {} has multiple default endpoints",
                            relation.cli_id, relation.provider_template_id
                        ));
                    }
                    let template = self
                        .api_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing API template",
                                relation.id
                            ))
                        })?;
                    let endpoint = template
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.id == relation.endpoint_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing endpoint",
                                relation.id
                            ))
                        })?;
                    if !cli.protocols.contains(&endpoint.protocol) {
                        return invalid(format!(
                            "relation {} connects {} to unsupported protocol {}",
                            relation.id, cli.id, endpoint.protocol
                        ));
                    }
                    if !endpoint
                        .auth_options
                        .iter()
                        .any(|option| option.id == relation.auth_option_id)
                    {
                        return invalid(format!(
                            "relation {} references a missing auth option",
                            relation.id
                        ));
                    }
                    if relation.base_url.as_ref().is_some_and(|base_url| {
                        !matches!(base_url.scheme(), "http" | "https")
                            || base_url.host_str().is_none()
                            || !base_url.username().is_empty()
                            || base_url.password().is_some()
                            || base_url.query().is_some()
                            || base_url.fragment().is_some()
                    }) {
                        return invalid(format!(
                            "relation {} has an invalid base URL override",
                            relation.id
                        ));
                    }
                    if let Some(provider_package) = relation.provider_package.as_deref() {
                        ensure_nonempty("relation provider package", provider_package)?;
                        if let Some(protocol) = self.package_protocol(cli.id, provider_package)
                            && protocol != endpoint.protocol
                        {
                            return invalid(format!(
                                "relation {} provider package does not match endpoint protocol",
                                relation.id
                            ));
                        }
                    }
                    for native_id in &relation.native_provider_ids {
                        ensure_nonempty("native provider ID", native_id)?;
                        if !native_ids.insert((relation.cli_id, native_id.as_str())) {
                            return invalid(format!(
                                "CLI {} repeats native provider ID {native_id}",
                                relation.cli_id
                            ));
                        }
                    }
                }
                CliProviderRelation::Auth(relation) => {
                    if !auth_routes
                        .insert((relation.cli_id, relation.provider_template_id.as_str()))
                    {
                        return invalid(format!(
                            "duplicate CLI/auth-template relation {}",
                            relation.id
                        ));
                    }
                    let template = self
                        .auth_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing auth template",
                                relation.id
                            ))
                        })?;
                    if !cli.auth_modes.iter().any(|mode| {
                        mode.id == relation.auth_mode_id && mode.oauth_kind == template.auth_kind
                    }) {
                        return invalid(format!(
                            "relation {} references an incompatible CLI auth mode",
                            relation.id
                        ));
                    }
                }
            }
        }
        for template in &self.provider_templates {
            if !self
                .relations
                .iter()
                .any(|relation| relation.provider_template_id() == template.id())
            {
                return invalid(format!(
                    "provider template {} has no CLI relation",
                    template.id()
                ));
            }
        }
        Ok(())
    }
}

fn validate_api_template(template: &ApiProviderTemplate) -> AppResult<()> {
    ensure_nonempty("provider category", &template.category)?;
    if template.credential_slots.is_empty() {
        return invalid(format!(
            "API template {} has no credential slots",
            template.id
        ));
    }
    if template.endpoints.is_empty() {
        return invalid(format!("API template {} has no endpoints", template.id));
    }
    ensure_unique_strings(
        "credential slot",
        template
            .credential_slots
            .iter()
            .map(|slot| slot.id.as_str()),
    )?;
    for slot in &template.credential_slots {
        ensure_nonempty("credential slot name", &slot.name)?;
    }
    let slot_ids = template
        .credential_slots
        .iter()
        .map(|slot| slot.id.as_str())
        .collect::<HashSet<_>>();
    let mut endpoint_ids = HashSet::new();
    let mut supported_model_ids = HashSet::new();
    for endpoint in &template.endpoints {
        ensure_identifier("endpoint", &endpoint.id)?;
        ensure_nonempty("endpoint name", &endpoint.name)?;
        if !endpoint_ids.insert(endpoint.id.as_str()) {
            return invalid(format!(
                "template {} repeats endpoint {}",
                template.id, endpoint.id
            ));
        }
        if !matches!(endpoint.base_url.scheme(), "http" | "https")
            || endpoint.base_url.host_str().is_none()
            || !endpoint.base_url.username().is_empty()
            || endpoint.base_url.password().is_some()
        {
            return invalid(format!(
                "template {} endpoint {} has an invalid URL",
                template.id, endpoint.id
            ));
        }
        if !slot_ids.contains(endpoint.credential_slot_id.as_str()) {
            return invalid(format!(
                "template {} endpoint {} references a missing credential slot",
                template.id, endpoint.id
            ));
        }
        if endpoint.auth_options.is_empty() || endpoint.default_auth_type().is_none() {
            return invalid(format!(
                "template {} endpoint {} has no valid default auth option",
                template.id, endpoint.id
            ));
        }
        ensure_unique_strings(
            "endpoint auth option",
            endpoint
                .auth_options
                .iter()
                .map(|option| option.id.as_str()),
        )?;
        ensure_unique_nonempty_strings(
            "endpoint model",
            endpoint.models.iter().map(|model| model.id.as_str()),
        )?;
        if endpoint.models.iter().filter(|model| model.default).count() > 1 {
            return invalid(format!(
                "template {} endpoint {} has multiple default models",
                template.id, endpoint.id
            ));
        }
        for model in &endpoint.models {
            ensure_nonempty("model ID", &model.id)?;
            ensure_nonempty("model name", &model.name)?;
            if model.context == Some(0) || model.output == Some(0) {
                return invalid(format!(
                    "template {} endpoint {} has a zero model limit",
                    template.id, endpoint.id
                ));
            }
            if template.model_routing && !supported_model_ids.insert(model.id.as_str()) {
                return invalid(format!(
                    "model-routed template {} repeats model {} across endpoints",
                    template.id, model.id
                ));
            }
        }
    }
    let mut unsupported_model_ids = HashSet::new();
    for model in &template.unsupported_models {
        ensure_nonempty("unsupported model ID", &model.id)?;
        ensure_nonempty("unsupported model name", &model.name)?;
        ensure_nonempty("unsupported model package", &model.provider_package)?;
        if !unsupported_model_ids.insert(model.id.as_str()) {
            return invalid(format!(
                "template {} repeats unsupported model {}",
                template.id, model.id
            ));
        }
        if supported_model_ids.contains(model.id.as_str()) {
            return invalid(format!(
                "template {} marks supported model {} as unsupported",
                template.id, model.id
            ));
        }
    }
    if template.model_routing
        && template
            .endpoints
            .iter()
            .any(|endpoint| endpoint.models.is_empty())
    {
        return invalid(format!(
            "model-routed template {} has an endpoint without model routes",
            template.id
        ));
    }
    Ok(())
}

fn validate_cli_definitions(clis: &[CatalogCli]) -> AppResult<()> {
    let mut cli_ids = HashSet::new();
    for cli in clis {
        if !cli_ids.insert(cli.id) {
            return invalid(format!("duplicate CLI {}", cli.id));
        }
        ensure_nonempty("CLI name", &cli.name)?;
        ensure_unique_strings(
            "CLI auth mode",
            cli.auth_modes.iter().map(|mode| mode.id.as_str()),
        )?;
        let mut protocols = HashSet::new();
        for protocol in &cli.protocols {
            if !protocols.insert(*protocol) {
                return invalid(format!("CLI {} repeats protocol {protocol}", cli.id));
            }
        }
        let mut adapters = HashSet::new();
        for adapter in &cli.protocol_adapters {
            if !protocols.contains(&adapter.protocol) {
                return invalid(format!(
                    "CLI {} has an adapter for unsupported protocol {}",
                    cli.id, adapter.protocol
                ));
            }
            if !adapters.insert(adapter.protocol) {
                return invalid(format!(
                    "CLI {} repeats the adapter for {}",
                    cli.id, adapter.protocol
                ));
            }
            ensure_nonempty("provider package", &adapter.provider_package)?;
        }
    }
    for required in CliId::ALL {
        if !cli_ids.contains(&required) {
            return invalid(format!("catalog omits CLI {required}"));
        }
    }
    Ok(())
}

fn parse_catalog_file<T: DeserializeOwned>(name: &str, text: &str) -> AppResult<T> {
    let value = parse_to_serde_value(text, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(format!("invalid {name}: {error}")))?;
    serde_json::from_value(value)
        .map_err(|error| AppError::Serialization(format!("invalid {name}: {error}")))
}

fn ensure_unique_strings<'a>(kind: &str, values: impl Iterator<Item = &'a str>) -> AppResult<()> {
    let mut seen = HashSet::new();
    for value in values {
        ensure_identifier(kind, value)?;
        if !seen.insert(value) {
            return invalid(format!("duplicate {kind} {value}"));
        }
    }
    Ok(())
}

fn ensure_unique_nonempty_strings<'a>(
    kind: &str,
    values: impl Iterator<Item = &'a str>,
) -> AppResult<()> {
    let mut seen = HashSet::new();
    for value in values {
        ensure_nonempty(kind, value)?;
        if !seen.insert(value) {
            return invalid(format!("duplicate {kind} {value}"));
        }
    }
    Ok(())
}

fn ensure_identifier(kind: &str, value: &str) -> AppResult<()> {
    ensure_nonempty(kind, value)?;
    if !value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) {
        return invalid(format!("{kind} {value} is not a kebab-case identifier"));
    }
    Ok(())
}

/// Upstream provider and model IDs are persisted verbatim and are not required to be kebab-case
/// (for example, models.dev currently contains `wafer.ai`). They still must be bounded, printable
/// keys so they can safely cross the IPC boundary and be used as OpenCode object keys.
fn ensure_source_identifier(kind: &str, value: &str) -> AppResult<()> {
    ensure_nonempty(kind, value)?;
    if value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return invalid(format!("{kind} {value} contains unsafe characters"));
    }
    Ok(())
}

fn sanitize_identifier(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
            output.push(character);
        } else if character.is_ascii_uppercase() {
            output.push(character.to_ascii_lowercase());
        } else {
            output.push('-');
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        "provider".into()
    } else {
        output
    }
}

fn ensure_nonempty(kind: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return invalid(format!("{kind} must not be empty"));
    }
    Ok(())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn invalid<T>(message: String) -> AppResult<T> {
    Err(AppError::Serialization(message))
}

static EMBEDDED_CATALOG: Lazy<Result<ProviderCatalog, String>> =
    Lazy::new(|| ProviderCatalog::load_embedded().map_err(|error| error.to_string()));

static LEGACY_CATALOG: Lazy<Result<ProviderCatalog, String>> =
    Lazy::new(|| ProviderCatalog::load_legacy().map_err(|error| error.to_string()));

// The active catalog is installed by AppState after the private cache has been loaded. Keeping
// this handle behind a narrow backend-only API lets adapters resolve the same snapshot that the
// UI selected, while unit tests and read-only library users continue to fall back to the bundled
// compatibility catalog. No renderer input can mutate this value directly.
static ACTIVE_CATALOG: Lazy<std::sync::RwLock<Option<ProviderCatalog>>> =
    Lazy::new(|| std::sync::RwLock::new(None));

pub fn embedded_catalog() -> AppResult<&'static ProviderCatalog> {
    EMBEDDED_CATALOG
        .as_ref()
        .map_err(|error| AppError::Serialization(error.clone()))
}

/// Returns the retired static catalog for migration/import fixtures. New runtime provider
/// selection must use [`embedded_catalog`] or [`runtime_catalog`] instead.
pub fn legacy_catalog() -> AppResult<&'static ProviderCatalog> {
    LEGACY_CATALOG
        .as_ref()
        .map_err(|error| AppError::Serialization(error.clone()))
}

pub fn install_runtime_catalog(catalog: ProviderCatalog) {
    if let Ok(mut active) = ACTIVE_CATALOG.write() {
        *active = Some(catalog);
    }
}

pub fn runtime_catalog() -> AppResult<ProviderCatalog> {
    if let Ok(active) = ACTIVE_CATALOG.read()
        && let Some(catalog) = active.as_ref()
    {
        return Ok(catalog.clone());
    }
    Ok(embedded_catalog()?.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_well_formed() {
        ProviderCatalog::load_embedded().unwrap();
    }

    #[test]
    fn models_dev_bundle_is_full_and_resolves_core_adapters() {
        let models = ModelsDevCatalog::bundled().unwrap();
        assert!(models.provider_count() >= 200);
        assert!(models.model_count() >= 7_000);
        let catalog = ProviderCatalog::from_models_dev(models).unwrap();
        let openai = catalog.dynamic_provider_info("openai").unwrap();
        assert_eq!(openai.protocol, Some(CliProtocol::OpenaiResponses));
        assert!(openai.selectable);
        assert!(openai.supported_clis.contains(&CliId::Codex));
        assert!(catalog.api_template("openai").is_some());
    }

    #[test]
    fn models_dev_model_overrides_are_hints_not_routes() {
        let models = ModelsDevCatalog::bundled().unwrap();
        let opencode = models.provider("opencode").unwrap();
        let info = models
            .provider_info()
            .into_iter()
            .find(|provider| provider.id == opencode.id)
            .unwrap();
        // The bundled upstream record describes models which need Responses or Anthropic
        // adapters. OpenCode's provider-level Chat adapter cannot silently route those models.
        assert!(
            info.models
                .iter()
                .any(|model| !model.selectable && model.disabled_reason.is_some())
        );
    }

    #[test]
    fn disabled_models_dev_provider_stays_visible_without_a_generated_template() {
        let value = serde_json::json!({
            "demo": {
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://demo.example/v1",
                "name": "Demo",
                "models": { "model": { "name": "Model" } }
            },
            "openai": {
                "env": ["OPENAI_API_KEY"],
                "npm": "@ai-sdk/openai",
                "api": "https://api.openai.com/v1",
                "name": "OpenAI",
                "models": { "gpt-test": { "name": "GPT Test" } }
            }
        });
        let models = ModelsDevCatalog::from_api_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let catalog = ProviderCatalog::from_models_dev(models).unwrap();
        let demo = catalog.dynamic_provider_info("demo").unwrap();
        assert!(!demo.selectable);
        assert!(catalog.api_template("demo").is_none());
        assert!(catalog.api_template("openai").is_some());
    }

    #[test]
    fn multi_environment_provider_is_visible_but_not_selectable() {
        let value = serde_json::json!({
            "demo": {
                "env": ["DEMO_ACCOUNT", "DEMO_API_KEY"],
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://demo.example/v1",
                "name": "Demo",
                "models": { "model": { "name": "Model" } }
            }
        });
        let models = ModelsDevCatalog::from_api_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let catalog = ProviderCatalog::from_models_dev(models).unwrap();
        let demo = catalog.dynamic_provider_info("demo").unwrap();
        assert!(!demo.selectable);
        assert!(
            demo.disabled_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("one API key"))
        );
        assert!(catalog.api_template("demo").is_none());
    }

    #[test]
    fn catalog_serializes_as_frontend_discriminated_unions() {
        let value = serde_json::to_value(ProviderCatalog::load_embedded().unwrap()).unwrap();
        let template = &value["providerTemplates"][0];
        assert_eq!(template["mode"], "api");
        assert!(template.get("id").is_some());
        assert!(
            value["providerInfo"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        let auth_relation = value["relations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|relation| relation["mode"] == "auth")
            .unwrap();
        assert!(auth_relation.get("authModeId").is_some());
    }

    #[test]
    fn glm_coding_plan_has_three_protocol_endpoints_and_one_shared_slot() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        let template = catalog.api_template("glm-coding-plan").unwrap();
        assert_eq!(template.endpoints.len(), 3);
        assert_eq!(template.credential_slots.len(), 1);
        assert!(
            template
                .endpoints
                .iter()
                .all(|endpoint| endpoint.credential_slot_id == "api-key")
        );
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::AnthropicMessages
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/anthropic"
        }));
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::OpenaiChat
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/coding/paas/v4"
        }));
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::OpenaiResponses
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/v1"
        }));
    }

    #[test]
    fn opencode_glm_routes_are_explicit_and_have_no_silent_default() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        let relations = catalog
            .api_relations(CliId::Opencode, "glm-coding-plan")
            .collect::<Vec<_>>();
        assert_eq!(relations.len(), 3);
        assert!(relations.iter().all(|relation| !relation.default));
        assert_eq!(
            catalog
                .native_api_relation(CliId::Opencode, "zhipuai-coding-plan")
                .map(|relation| relation.endpoint_id.as_str()),
            Some("openai-chat")
        );
    }

    #[test]
    fn opencode_packages_come_from_the_cli_catalog() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        assert_eq!(
            catalog.protocol_package(CliId::Opencode, CliProtocol::OpenaiChat),
            Some("@ai-sdk/openai-compatible")
        );
        assert_eq!(
            catalog.protocol_package(CliId::Opencode, CliProtocol::OpenaiResponses),
            Some("@ai-sdk/openai")
        );
        assert_eq!(
            catalog
                .native_api_relation(CliId::Opencode, "openrouter")
                .and_then(|relation| relation.provider_package.as_deref()),
            Some("@openrouter/ai-sdk-provider")
        );
    }

    #[test]
    fn opencode_zen_and_go_route_models_to_documented_transports() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        for (template_id, native_id, base_url) in [
            ("opencode-zen", "opencode", "https://opencode.ai/zen/v1"),
            (
                "opencode-go",
                "opencode-go",
                "https://opencode.ai/zen/go/v1",
            ),
        ] {
            let template = catalog.api_template(template_id).unwrap();
            assert!(template.model_routing);
            assert_eq!(template.credential_slots.len(), 1);
            assert_eq!(template.endpoints.len(), 3);
            assert!(
                template
                    .endpoints
                    .iter()
                    .all(|endpoint| endpoint.base_url.as_str() == base_url)
            );
            assert!(
                catalog
                    .native_api_relation(CliId::Opencode, native_id)
                    .is_some()
            );
            assert_eq!(
                catalog
                    .native_api_relation(CliId::Opencode, native_id)
                    .map(|relation| relation.endpoint_id.as_str()),
                Some("chat")
            );
        }

        assert_eq!(
            catalog
                .model_routed_endpoint("opencode-zen", "gpt-5.6-sol")
                .map(|endpoint| endpoint.protocol),
            Some(CliProtocol::OpenaiResponses)
        );
        assert_eq!(
            catalog
                .model_routed_endpoint("opencode-zen", "claude-opus-4-6")
                .map(|endpoint| endpoint.protocol),
            Some(CliProtocol::AnthropicMessages)
        );
        assert_eq!(
            catalog
                .model_routed_endpoint("opencode-go", "glm-5.3")
                .map(|endpoint| endpoint.protocol),
            Some(CliProtocol::OpenaiChat)
        );
        assert_eq!(
            catalog
                .unsupported_model("opencode-zen", "gemini-3.7-flash")
                .map(|model| model.provider_package.as_str()),
            Some("@ai-sdk/google")
        );
    }

    #[test]
    fn claude_minimax_relations_separate_api_and_token_plan_transport() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        for (template_id, auth_type, base_url) in [
            (
                "minimax-api",
                ConnectionAuthType::ApiKey,
                "https://api.minimax.io/anthropic",
            ),
            (
                "minimax-cn-api",
                ConnectionAuthType::ApiKey,
                "https://api.minimaxi.com/anthropic",
            ),
            (
                "minimax-coding-plan",
                ConnectionAuthType::Bearer,
                "https://api.minimax.io/anthropic",
            ),
            (
                "minimax-cn-coding-plan",
                ConnectionAuthType::Bearer,
                "https://api.minimaxi.com/anthropic",
            ),
        ] {
            let relation = catalog
                .api_relation(CliId::ClaudeCode, template_id, "anthropic")
                .unwrap();
            assert_eq!(catalog.relation_auth_type(relation), Some(auth_type));
            assert_eq!(relation.base_url.as_ref().map(Url::as_str), Some(base_url));
        }

        assert_eq!(
            catalog
                .api_relation(CliId::Opencode, "minimax-coding-plan", "anthropic")
                .and_then(|relation| relation.base_url.as_ref()),
            None
        );
    }

    #[test]
    fn all_previous_opencode_native_provider_ids_are_in_the_relation_catalog() {
        let catalog = ProviderCatalog::load_legacy().unwrap();
        for provider_id in [
            "openai",
            "anthropic",
            "openrouter",
            "opencode",
            "opencode-go",
            "zhipuai-coding-plan",
            "zai-coding-plan",
            "minimax-coding-plan",
            "minimax-cn-coding-plan",
            "alibaba-coding-plan",
            "alibaba-coding-plan-cn",
            "tencent-coding-plan",
            "kimi-for-coding",
            "umans-ai-coding-plan",
            "kuae-cloud-coding-plan",
        ] {
            assert!(
                catalog
                    .native_api_relation(CliId::Opencode, provider_id)
                    .is_some(),
                "missing native provider relation for {provider_id}"
            );
        }
    }
}
