use std::{collections::BTreeMap, path::PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterApiCandidate, AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan,
        CliAdapter, FileWritePlan, FixedOAuthCommand, HostEnvironment, namespaced_provider_id,
        read_file_snapshot, read_optional,
    },
    catalog::runtime_catalog,
    config_templates::{
        RenderedManagedConfig, TemplateBindings, TemplateSelection, render_managed_config,
        resolve_templates,
    },
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, SourceFileSnapshot,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::file_digest,
    services::config_writer::{
        CodexApiTemplatePatch, JsonPatch, parse_jsonc_value, parse_toml,
        patch_codex_api_toml_from_template, patch_codex_oauth_toml, patch_jsonc,
    },
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

fn managed_model_catalog_path(
    paths: &AdapterPaths,
    provider_id: Uuid,
    connection_id: Uuid,
) -> PathBuf {
    paths
        .config_directory
        .join("cliswitch-models")
        .join(format!(
            "{}-{}.json",
            provider_id.simple(),
            connection_id.simple()
        ))
}

fn recognized_managed_model_catalog(paths: &AdapterPaths, value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.parent()? != paths.config_directory.join("cliswitch-models") {
        return None;
    }
    let file_name = path.file_name()?.to_str()?.strip_suffix(".json")?;
    let (provider, connection) = file_name.split_once('-')?;
    if provider.len() != 32 || connection.len() != 32 {
        return None;
    }
    Uuid::parse_str(provider).ok()?;
    Uuid::parse_str(connection).ok()?;
    Some(path)
}

fn snapshot_text<'a>(source: &'a Option<Vec<u8>>, default: &'a str) -> AppResult<&'a str> {
    match source {
        Some(source) => std::str::from_utf8(source).map_err(|error| {
            AppError::Serialization(format!("configuration is not UTF-8: {error}"))
        }),
        None => Ok(default),
    }
}

#[async_trait]
impl CliAdapter for CodexAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::Codex,
            display_name: "Codex CLI".into(),
            command: "codex".into(),
            schema_fingerprint: "stable-2026-09:templated-responses+model-catalog/file-auth".into(),
        }
    }

    fn resolve_paths(
        &self,
        environment: &HostEnvironment,
        manual: Option<PathBuf>,
    ) -> AdapterPaths {
        let directory = manual
            .or_else(|| environment.absolute_path("CODEX_HOME"))
            .unwrap_or_else(|| environment.home.join(".codex"));
        AdapterPaths {
            config_file: directory.join("config.toml"),
            auth_file: Some(directory.join("auth.json")),
            config_directory: directory,
        }
    }

    async fn read_current(
        &self,
        paths: &AdapterPaths,
        _environment: &HostEnvironment,
    ) -> AppResult<AdapterReadResult> {
        let text = read_optional(&paths.config_file, "").await?;
        let document = parse_toml(&text)?;
        let model = document
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let provider_id = document
            .get("model_provider")
            .and_then(|value| value.as_str())
            .unwrap_or("openai")
            .to_string();
        let provider_table = document
            .get("model_providers")
            .and_then(|value| value.as_table())
            .and_then(|providers| providers.get(&provider_id))
            .and_then(|value| value.as_table());
        let endpoint = provider_table
            .and_then(|provider| provider.get("base_url"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let key = provider_table
            .and_then(|provider| provider.get("experimental_bearer_token"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let wire_api = provider_table
            .and_then(|provider| provider.get("wire_api"))
            .and_then(|value| value.as_str());
        if let Some(wire_api) = wire_api
            && wire_api != "responses"
        {
            return Err(AppError::Unsupported(format!(
                "unsupported Codex wire_api: {wire_api}"
            )));
        }
        let forced_login = document
            .get("forced_login_method")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let model_catalog_json = document
            .get("model_catalog_json")
            .and_then(|value| value.as_str())
            .and_then(|value| recognized_managed_model_catalog(paths, value));
        let managed_provider_id = provider_id
            .strip_prefix("cliswitch_")
            .and_then(|value| Uuid::parse_str(value).ok());
        let dynamic_info = if let (Some(endpoint), Some(provider_info)) =
            (endpoint.as_deref(), runtime_catalog()?.provider_info)
        {
            let parsed = Url::parse(endpoint)?;
            let supports_endpoint = |info: &&crate::catalog::CatalogProviderInfo| {
                info.selectable
                    && info.endpoints.iter().any(|candidate| {
                        candidate.selectable
                            && candidate.protocol == Some(CliProtocol::OpenaiResponses)
                            && candidate.endpoint.as_ref().is_some_and(|candidate| {
                                candidate.as_str().trim_end_matches('/')
                                    == parsed.as_str().trim_end_matches('/')
                            })
                    })
            };
            provider_info
                .iter()
                .find(|info| info.id == provider_id && supports_endpoint(info))
                .cloned()
                .or_else(|| {
                    let mut matches = provider_info.iter().filter(supports_endpoint);
                    let matched = matches.next()?.clone();
                    matches.next().is_none().then_some(matched)
                })
        } else {
            None
        };
        let candidate = match (&endpoint, &key) {
            (Some(endpoint), Some(key)) => Some((
                dynamic_info.clone(),
                ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: dynamic_info.as_ref().map(|_| "responses".to_string()),
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse(endpoint)?,
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: key.clone(),
                    default_model: model.clone().unwrap_or_default(),
                    verification: VerificationInfo::default(),
                },
            )),
            _ => None,
        };
        let auth_file_exists = paths
            .auth_file
            .as_ref()
            .map(|path| path.exists())
            .unwrap_or(false);
        let mut sources = vec![SourceFileSnapshot {
            source_id: "codex-config".into(),
            display_path: paths.config_file.clone(),
            digest: file_digest(&paths.config_file).await?,
        }];
        if let Some(auth_file) = paths.auth_file.as_ref()
            && auth_file.exists()
        {
            sources.push(SourceFileSnapshot {
                source_id: "codex-auth".into(),
                display_path: auth_file.clone(),
                digest: file_digest(auth_file).await?,
            });
        }
        let mut diagnostics = forced_login
            .as_ref()
            .map(|method| vec![format!("forced_login_method is set to {method}")])
            .unwrap_or_default();
        if let Some(model_catalog) = model_catalog_json {
            match read_file_snapshot(&model_catalog, &paths.config_directory).await {
                Ok((Some(bytes), digest)) => {
                    match std::str::from_utf8(&bytes)
                        .map_err(|error| error.to_string())
                        .and_then(|text| parse_jsonc_value(text).map_err(|error| error.to_string()))
                    {
                        Ok(value)
                            if value
                                .get("models")
                                .and_then(Value::as_array)
                                .is_some() => {}
                        Ok(_) => diagnostics.push(
                            "CLISwitch Codex model catalog does not contain a models array; fix or restore it before applying"
                                .into(),
                        ),
                        Err(error) => diagnostics.push(format!(
                            "Unable to parse the CLISwitch Codex model catalog: {error}"
                        )),
                    }
                    sources.push(SourceFileSnapshot {
                        source_id: "codex-model-catalog".into(),
                        display_path: model_catalog,
                        digest,
                    });
                }
                Ok((None, digest)) => {
                    diagnostics.push(
                        "The configured CLISwitch Codex model catalog is missing; reapply to create it"
                            .into(),
                    );
                    sources.push(SourceFileSnapshot {
                        source_id: "codex-model-catalog".into(),
                        display_path: model_catalog,
                        digest,
                    });
                }
                Err(error) => diagnostics.push(format!(
                    "Unable to inspect the CLISwitch Codex model catalog: {error}"
                )),
            }
        }
        let unmanaged_api_candidates = candidate
            .into_iter()
            .map(|(dynamic_info, connection)| {
                let available_models = model.iter().cloned().collect();
                AdapterApiCandidate {
                    source_provider_id: dynamic_info
                        .as_ref()
                        .map(|info| info.id.clone())
                        .unwrap_or_else(|| provider_id.clone()),
                    suggested_name: dynamic_info
                        .as_ref()
                        .map(|info| info.name.clone())
                        .unwrap_or_else(|| provider_id.clone()),
                    template_id: dynamic_info.map(|info| info.id),
                    available_models,
                    default_model: model.clone(),
                    is_current: true,
                    model_routed: false,
                    connection,
                }
            })
            .collect();
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: Some(provider_id),
                protocol: endpoint.as_ref().map(|_| CliProtocol::OpenaiResponses),
                auth_kind: if key.is_some() {
                    Some("api".into())
                } else if auth_file_exists {
                    Some("oauth".into())
                } else {
                    None
                },
                model,
                managed_provider_id,
                sources,
                externally_overridden: forced_login.is_some(),
                diagnostics,
            },
            unmanaged_api_candidates,
        })
    }

    async fn plan_write(
        &self,
        paths: &AdapterPaths,
        target: &ConfigurationTarget,
        provider: &ProviderProfile,
    ) -> AppResult<AdapterWritePlan> {
        let (config_source, config_digest) =
            read_file_snapshot(&paths.config_file, &paths.config_directory).await?;
        let source = snapshot_text(&config_source, "")?;
        let mut files = Vec::new();
        let warning;
        let target_toml = match (target, &provider.data) {
            (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => {
                let connection = api
                    .connections
                    .iter()
                    .find(|connection| connection.id == *connection_id)
                    .ok_or_else(|| AppError::Validation("connection does not exist".into()))?;
                if connection.protocol != CliProtocol::OpenaiResponses {
                    return Err(AppError::Validation(
                        "Codex only accepts the Responses wire API".into(),
                    ));
                }
                warning = Some(
                    "Codex stores this key in the documented but discouraged experimental_bearer_token field"
                        .into(),
                );
                let provider_id = namespaced_provider_id(provider.id);
                let model_catalog = managed_model_catalog_path(paths, provider.id, connection.id);
                let templates = resolve_templates(&TemplateSelection {
                    cli_id: CliId::Codex,
                    template_id: provider.template_id.as_deref(),
                    protocol: connection.protocol,
                    model: target.model(),
                })?;
                let rendered = render_managed_config(
                    &templates,
                    &TemplateBindings {
                        provider_id: &provider_id,
                        provider_name: &provider.name,
                        endpoint: connection.endpoint.as_str(),
                        auth_type: connection.auth_type,
                        api_key: &connection.api_key,
                        model: target.model(),
                        model_catalog_path: Some(&model_catalog),
                    },
                )?;
                let RenderedManagedConfig::Codex(rendered) = rendered else {
                    return Err(AppError::Serialization(
                        "resolved a non-Codex config template".into(),
                    ));
                };
                let (models_source, models_digest) =
                    read_file_snapshot(&model_catalog, &paths.config_directory).await?;
                let models_text = snapshot_text(&models_source, "{}\n")?;
                let models_value = parse_jsonc_value(models_text)?;
                let root = models_value.as_object().ok_or_else(|| {
                    AppError::Unsupported("Codex model catalog root must be an object".into())
                })?;
                if let Some(models) = root.get("models")
                    && !models.is_array()
                {
                    return Err(AppError::Unsupported(
                        "Codex model catalog models field must be an array".into(),
                    ));
                }
                let target_models = patch_jsonc(
                    models_text,
                    &[JsonPatch::SetValue {
                        path: vec!["models".into()],
                        value: Value::Array(vec![rendered.model_entry.clone()]),
                    }],
                )?;
                files.push(FileWritePlan {
                    path: model_catalog,
                    allowed_root: paths.config_directory.clone(),
                    source_content: models_source,
                    source_digest: models_digest,
                    target_content: target_models.into_bytes(),
                    contains_credentials: false,
                    opaque_content: false,
                });
                patch_codex_api_toml_from_template(
                    source,
                    &CodexApiTemplatePatch {
                        provider_id: &rendered.provider_id,
                        provider_name: &rendered.provider_name,
                        base_url: &rendered.endpoint,
                        api_key: &rendered.api_key,
                        model: &rendered.model,
                        model_reasoning_effort: &rendered.reasoning_effort,
                        model_catalog_json: Some(&rendered.model_catalog_path),
                        preferred_auth_method: rendered.preferred_auth_method.as_deref(),
                        forced_login_method: rendered.forced_login_method.as_deref(),
                    },
                )?
            }
            (ConfigurationTarget::Oauth { .. }, ProviderData::Oauth(oauth))
                if oauth.oauth_kind == OAuthKind::Codex =>
            {
                warning = oauth.manually_modified.then(|| {
                    "OAuth content was edited manually and will be written without schema validation"
                        .into()
                });
                let auth_file = paths.auth_file.clone().ok_or_else(|| {
                    AppError::Unsupported("Codex auth file is unavailable".into())
                })?;
                let (auth_source, auth_digest) =
                    read_file_snapshot(&auth_file, &paths.config_directory).await?;
                files.push(FileWritePlan {
                    source_content: auth_source,
                    source_digest: auth_digest,
                    path: auth_file,
                    allowed_root: paths.config_directory.clone(),
                    target_content: oauth.raw_content.as_bytes().to_vec(),
                    contains_credentials: true,
                    opaque_content: oauth.manually_modified,
                });
                patch_codex_oauth_toml(source, target.model())?
            }
            _ => {
                return Err(AppError::Validation(
                    "Codex target does not match the provider type".into(),
                ));
            }
        };
        let config_plan = FileWritePlan {
            source_content: config_source,
            source_digest: config_digest,
            path: paths.config_file.clone(),
            allowed_root: paths.config_directory.clone(),
            target_content: target_toml.into_bytes(),
            contains_credentials: matches!(target, ConfigurationTarget::Api { .. }),
            opaque_content: false,
        };
        if matches!(target, ConfigurationTarget::Api { .. }) {
            // The referenced catalog must become visible before config.toml starts pointing to it.
            files.push(config_plan);
        } else {
            files.insert(0, config_plan);
        }
        Ok(AdapterWritePlan {
            cli_id: CliId::Codex,
            files,
            warning,
        })
    }

    fn oauth_kind(&self) -> Option<OAuthKind> {
        Some(OAuthKind::Codex)
    }

    fn validate_imported_auth(&self, bytes: &[u8]) -> AppResult<Option<String>> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|_| AppError::Validation("unrecognized Codex auth JSON".into()))?;
        let object = value
            .as_object()
            .ok_or_else(|| AppError::Validation("Codex auth must be a JSON object".into()))?;
        if !object.contains_key("tokens") && !object.contains_key("OPENAI_API_KEY") {
            return Err(AppError::Validation(
                "Codex auth does not contain tokens or OPENAI_API_KEY".into(),
            ));
        }
        Ok(value
            .pointer("/tokens/account_id")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    fn fixed_oauth_command(
        &self,
        executable: PathBuf,
        isolated_home: PathBuf,
    ) -> AppResult<FixedOAuthCommand> {
        let mut environment = BTreeMap::new();
        environment.insert(
            "CODEX_HOME".into(),
            isolated_home.to_string_lossy().to_string(),
        );
        Ok(FixedOAuthCommand {
            executable,
            args: vec!["login".into()],
            environment,
            artifact: isolated_home.join("auth.json"),
        })
    }
}
