use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterApiCandidate, AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan,
        CliAdapter, FileWritePlan, FixedOAuthCommand, HostEnvironment, namespaced_provider_id,
        read_optional,
    },
    catalog::{
        ProviderCatalog, ProviderModelTemplate, fixed_adapter_protocol, legacy_catalog,
        runtime_catalog,
    },
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, SourceFileSnapshot,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    services::{
        config_writer::{JsonPatch, parse_jsonc_value, patch_jsonc},
        minimax::{classify_credential, recognize_anthropic_endpoint},
    },
};

#[derive(Debug, Default)]
pub struct OpenCodeAdapter;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSelection {
    provider_id: String,
    model_id: String,
}

enum ConfiguredModelSelection {
    None,
    Unique(ModelSelection),
    Ambiguous,
}

fn parse_model_reference(reference: &str) -> Option<ModelSelection> {
    let (provider_id, model_id) = reference.split_once('/')?;
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ModelSelection {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn parse_last_used_model(state: &Value) -> Result<Option<ModelSelection>, &'static str> {
    let Some(recent) = state.get("recent") else {
        return Ok(None);
    };
    let recent = recent
        .as_array()
        .ok_or("OpenCode model state field recent is not an array")?;
    let Some(entry) = recent.first() else {
        return Ok(None);
    };
    let provider_id = entry
        .get("providerID")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("OpenCode's most recent model has no valid providerID")?;
    let model_id = entry
        .get("modelID")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("OpenCode's most recent model has no valid modelID")?;
    Ok(Some(ModelSelection {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    }))
}

fn configured_model_selection(root: &serde_json::Map<String, Value>) -> ConfiguredModelSelection {
    let Some(providers) = root.get("provider").and_then(Value::as_object) else {
        return ConfiguredModelSelection::None;
    };
    let mut selected = None;
    for (provider_id, provider) in providers {
        let Some(models) = provider
            .as_object()
            .and_then(|provider| provider.get("models"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for model_id in models.keys() {
            if provider_id.is_empty() || model_id.is_empty() {
                continue;
            }
            if selected.is_some() {
                return ConfiguredModelSelection::Ambiguous;
            }
            selected = Some(ModelSelection {
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
            });
        }
    }
    selected
        .map(ConfiguredModelSelection::Unique)
        .unwrap_or(ConfiguredModelSelection::None)
}

fn model_state_path(environment: &HostEnvironment) -> PathBuf {
    environment
        .absolute_path("XDG_STATE_HOME")
        .unwrap_or_else(|| environment.home.join(".local").join("state"))
        .join("opencode")
        .join("model.json")
}

async fn read_last_used_model(
    path: &Path,
) -> (Option<ModelSelection>, Option<String>, Option<String>) {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, None, None);
        }
        Err(error) => {
            return (
                None,
                None,
                Some(format!("Unable to read OpenCode model state: {error}")),
            );
        }
    };
    let digest = Some(bytes_digest(&bytes));
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => {
            return (
                None,
                digest,
                Some(format!("OpenCode model state is not valid UTF-8: {error}")),
            );
        }
    };
    let state = match parse_jsonc_value(text) {
        Ok(state) => state,
        Err(error) => {
            return (
                None,
                digest,
                Some(format!("Unable to parse OpenCode model state: {error}")),
            );
        }
    };
    match parse_last_used_model(&state) {
        Ok(model) => (model, digest, None),
        Err(error) => (None, digest, Some(error.into())),
    }
}

#[derive(Debug, Clone)]
struct ResolvedProviderMetadata {
    display_name: String,
    template_id: Option<String>,
    template_endpoint_id: Option<String>,
    credential_slot_id: Option<String>,
    protocol: Option<CliProtocol>,
    auth_type: Option<ConnectionAuthType>,
    endpoint: Option<String>,
    explicit_npm: Option<String>,
    model_routed: bool,
    unsupported_model_package: Option<String>,
}

fn catalog_for_provider<'a>(
    runtime: &'a ProviderCatalog,
    legacy: &'a ProviderCatalog,
    provider_id: &str,
) -> &'a ProviderCatalog {
    if runtime.dynamic_provider_info(provider_id).is_some()
        || runtime.api_template(provider_id).is_some()
        || runtime
            .native_api_relation(CliId::Opencode, provider_id)
            .is_some()
    {
        runtime
    } else if legacy.api_template(provider_id).is_some()
        || legacy
            .native_api_relation(CliId::Opencode, provider_id)
            .is_some()
    {
        legacy
    } else {
        runtime
    }
}

fn configured_provider<'a>(
    root: &'a serde_json::Map<String, Value>,
    provider_id: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    root.get("provider")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(provider_id))
        .and_then(Value::as_object)
}

fn resolve_provider_metadata(
    provider_id: &str,
    provider: Option<&serde_json::Map<String, Value>>,
    model_id: Option<&str>,
) -> AppResult<ResolvedProviderMetadata> {
    let runtime = runtime_catalog()?;
    let legacy = legacy_catalog()?;
    let catalog = catalog_for_provider(&runtime, legacy, provider_id);
    let dynamic_info = runtime.dynamic_provider_info(provider_id);
    let native_relation = catalog.native_api_relation(CliId::Opencode, provider_id);
    let template_id = dynamic_info
        .map(|_| provider_id)
        .or_else(|| native_relation.map(|relation| relation.provider_template_id.as_str()));
    let template = template_id.and_then(|id| catalog.api_template(id));
    let explicit_npm = provider
        .and_then(|provider| provider.get("npm"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let explicit_base_url = provider
        .and_then(|provider| provider.get("options"))
        .and_then(Value::as_object)
        .and_then(|options| options.get("baseURL"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());

    let model_route = template
        .filter(|template| template.model_routing)
        .and_then(|template| {
            model_id.and_then(|id| catalog.model_routed_endpoint(&template.id, id))
        })
        .and_then(|endpoint| {
            template_id.and_then(|id| catalog.api_relation(CliId::Opencode, id, &endpoint.id))
        });
    let package_relation = explicit_npm.as_deref().and_then(|package| {
        template_id.and_then(|id| {
            let package_protocol = catalog
                .package_protocol(CliId::Opencode, package)
                .or_else(|| fixed_adapter_protocol(package));
            catalog.api_relations(CliId::Opencode, id).find(|relation| {
                if relation.provider_package.as_deref() == Some(package) {
                    return true;
                }
                package_protocol.is_some_and(|protocol| {
                    catalog
                        .api_template(&relation.provider_template_id)
                        .and_then(|template| {
                            template
                                .endpoints
                                .iter()
                                .find(|endpoint| endpoint.id == relation.endpoint_id)
                        })
                        .is_some_and(|endpoint| endpoint.protocol == protocol)
                })
            })
        })
    });
    let is_model_routed_template = template.is_some_and(|template| template.model_routing);
    let relation = if explicit_npm.is_some() {
        package_relation
    } else if is_model_routed_template {
        // A Base URL override changes only the destination. Zen and Go still
        // derive their protocol/package from the selected model.
        model_route
    } else {
        native_relation
    };
    let relation_endpoint = relation.and_then(|relation| {
        catalog
            .api_template(&relation.provider_template_id)?
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == relation.endpoint_id)
    });
    let protocol = explicit_npm
        .as_deref()
        .and_then(|package| catalog.package_protocol(CliId::Opencode, package))
        .or_else(|| {
            explicit_npm
                .as_deref()
                .and_then(fixed_adapter_protocol)
                .filter(|_| dynamic_info.is_some())
        })
        .or_else(|| relation_endpoint.map(|endpoint| endpoint.protocol));
    let endpoint = explicit_base_url
        .clone()
        .or_else(|| relation_endpoint.map(|endpoint| endpoint.base_url.to_string()));
    let display_name = provider
        .and_then(|provider| provider.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            template_id
                .and_then(|id| catalog.api_template(id))
                .map(|template| template.name.clone())
        })
        .unwrap_or_else(|| provider_id.to_string());
    let auth_type = protocol.map(|protocol| {
        relation
            .zip(relation_endpoint)
            .and_then(|(relation, endpoint)| {
                endpoint
                    .auth_options
                    .iter()
                    .find(|option| option.id == relation.auth_option_id)
                    .map(|option| option.auth_type)
            })
            .unwrap_or_else(|| default_auth_type(protocol))
    });
    let dynamic_package_matches =
        dynamic_info.is_none() || explicit_npm.is_none() || package_relation.is_some();
    let resolved_template_id = if !dynamic_package_matches {
        None
    } else if dynamic_info.is_some() && relation.is_some() {
        template_id.map(str::to_string)
    } else if is_model_routed_template && (explicit_npm.is_some() || explicit_base_url.is_some()) {
        None
    } else if explicit_npm.is_some()
        && !is_model_routed_template
        && relation.is_some_and(|relation| {
            native_relation.is_some_and(|native| {
                native.provider_template_id == relation.provider_template_id
                    && native.endpoint_id == relation.endpoint_id
            })
        })
    {
        template_id.map(str::to_string)
    } else if explicit_npm.is_some() && !is_model_routed_template {
        None
    } else {
        template_id.map(str::to_string)
    };
    let resolved_template = resolved_template_id
        .as_deref()
        .and_then(|id| catalog.api_template(id));
    let model_routed = template.is_some_and(|template| template.model_routing)
        && explicit_npm.is_none()
        && explicit_base_url.is_none();
    Ok(ResolvedProviderMetadata {
        display_name,
        template_id: resolved_template_id.clone(),
        template_endpoint_id: resolved_template_id
            .as_ref()
            .and_then(|_| relation.map(|relation| relation.endpoint_id.clone())),
        credential_slot_id: resolved_template_id
            .as_ref()
            .and_then(|_| relation_endpoint.map(|endpoint| endpoint.credential_slot_id.clone()))
            .or_else(|| {
                resolved_template
                    .and_then(|template| template.endpoints.first())
                    .map(|endpoint| endpoint.credential_slot_id.clone())
            }),
        protocol,
        auth_type,
        endpoint,
        explicit_npm,
        model_routed,
        unsupported_model_package: model_routed
            .then_some(template)
            .flatten()
            .and_then(|template| {
                model_id.and_then(|id| {
                    template
                        .unsupported_models
                        .iter()
                        .find(|model| model.id == id)
                })
            })
            .map(|model| model.provider_package.clone()),
    })
}

const fn default_auth_type(protocol: CliProtocol) -> ConnectionAuthType {
    match protocol {
        CliProtocol::AnthropicMessages => ConnectionAuthType::ApiKey,
        CliProtocol::OpenaiChat | CliProtocol::OpenaiResponses => ConnectionAuthType::Bearer,
    }
}

fn opencode_models(
    provider: &ProviderProfile,
    connection: &ProviderConnection,
    selected_model: &str,
) -> Value {
    let runtime = runtime_catalog().ok();
    let legacy = legacy_catalog().ok();
    let suggestions = provider
        .template_id
        .as_deref()
        .zip(connection.template_endpoint_id.as_deref())
        .and_then(|(template_id, endpoint_id)| {
            let runtime = runtime.as_ref()?;
            let catalog = legacy
                .as_ref()
                .map(|legacy| catalog_for_provider(runtime, legacy, template_id))
                .unwrap_or(runtime);
            catalog
                .api_template(template_id)?
                .endpoints
                .iter()
                .find(|endpoint| endpoint.id == endpoint_id)
                .map(|endpoint| endpoint.models.clone())
        })
        .unwrap_or_default();
    let mut models = serde_json::Map::new();
    for model in &suggestions {
        models.insert(model.id.clone(), opencode_model_value(model));
    }
    models
        .entry(selected_model.to_string())
        .or_insert_with(|| json!({ "name": selected_model }));
    Value::Object(models)
}

fn opencode_model_value(model: &ProviderModelTemplate) -> Value {
    let mut value =
        serde_json::Map::from_iter([("name".to_string(), Value::String(model.name.clone()))]);
    if model.context.is_some() || model.output.is_some() {
        let mut limit = serde_json::Map::new();
        if let Some(context) = model.context {
            limit.insert("context".into(), Value::from(context));
        }
        if let Some(output) = model.output {
            limit.insert("output".into(), Value::from(output));
        }
        value.insert("limit".into(), Value::Object(limit));
    }
    Value::Object(value)
}

fn provider_models(
    provider: Option<&serde_json::Map<String, Value>>,
    current: Option<&ModelSelection>,
    provider_id: &str,
) -> Vec<String> {
    let mut models = Vec::new();
    if let Some(current) = current
        && current.provider_id == provider_id
    {
        models.push(current.model_id.clone());
    }
    if let Some(configured_models) = provider
        .and_then(|provider| provider.get("models"))
        .and_then(Value::as_object)
    {
        for model in configured_models.keys() {
            if !model.is_empty() && !models.contains(model) {
                models.push(model.clone());
            }
        }
    }
    models
}

fn model_routed_model_is_supported(template_id: &str, model_id: &str) -> AppResult<bool> {
    let catalog = runtime_catalog()?;
    if catalog
        .model_routed_endpoint(template_id, model_id)
        .is_some()
    {
        return Ok(true);
    }
    Ok(legacy_catalog()?
        .model_routed_endpoint(template_id, model_id)
        .is_some())
}

#[async_trait]
impl CliAdapter for OpenCodeAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::Opencode,
            display_name: "OpenCode".into(),
            command: "opencode".into(),
            schema_fingerprint:
                "stable-v1:provider/npm/options/models+auth.type-api+state.model.recent".into(),
        }
    }

    fn resolve_paths(
        &self,
        environment: &HostEnvironment,
        manual: Option<PathBuf>,
    ) -> AdapterPaths {
        let (config_directory, explicit_config) = if let Some(manual) = manual {
            (manual, None)
        } else if let Some(config) = environment.absolute_path("OPENCODE_CONFIG") {
            let directory = config
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| environment.home.join(".config").join("opencode"));
            (directory, Some(config))
        } else {
            let directory = environment
                .absolute_path("OPENCODE_CONFIG_DIR")
                .unwrap_or_else(|| {
                    environment
                        .absolute_path("XDG_CONFIG_HOME")
                        .unwrap_or_else(|| environment.home.join(".config"))
                        .join("opencode")
                });
            (directory, None)
        };
        let config_file = explicit_config.unwrap_or_else(|| {
            let jsonc = config_directory.join("opencode.jsonc");
            if jsonc.exists() {
                jsonc
            } else {
                config_directory.join("opencode.json")
            }
        });
        let data_directory = environment
            .absolute_path("XDG_DATA_HOME")
            .unwrap_or_else(|| environment.home.join(".local").join("share"))
            .join("opencode");
        AdapterPaths {
            config_directory,
            config_file,
            auth_file: Some(data_directory.join("auth.json")),
        }
    }

    async fn read_current(
        &self,
        paths: &AdapterPaths,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterReadResult> {
        let config_text = read_optional(&paths.config_file, "{}\n").await?;
        let config = parse_jsonc_value(&config_text)?;
        let root = config
            .as_object()
            .ok_or_else(|| AppError::Unsupported("OpenCode config root is not an object".into()))?;
        if root.contains_key("providers") {
            return Err(AppError::Unsupported(
                "OpenCode v2 beta providers schema is outside the 0.1 compatibility baseline"
                    .into(),
            ));
        }
        let mut diagnostics = Vec::new();
        let state_file = model_state_path(environment);
        let mut state_digest = None;
        let explicit_model = root.get("model");
        let selection = match explicit_model {
            Some(Value::String(reference)) => match parse_model_reference(reference) {
                Some(selection) => Some(selection),
                None => {
                    diagnostics.push(
                        "OpenCode model must use the provider/model format; last-used state was not used because model is explicitly configured"
                            .into(),
                    );
                    None
                }
            },
            Some(_) => {
                diagnostics.push(
                    "OpenCode model must be a provider/model string; last-used state was not used because model is explicitly configured"
                        .into(),
                );
                None
            }
            None => {
                let (last_used, digest, diagnostic) = read_last_used_model(&state_file).await;
                state_digest = digest;
                if let Some(diagnostic) = diagnostic {
                    diagnostics.push(diagnostic);
                }
                if last_used.is_some() {
                    last_used
                } else {
                    match configured_model_selection(root) {
                        ConfiguredModelSelection::Unique(selection) => Some(selection),
                        ConfiguredModelSelection::Ambiguous => {
                            diagnostics.push(
                                "OpenCode has multiple configured models and no explicit or valid last-used model; the active model cannot be inferred"
                                    .into(),
                            );
                            None
                        }
                        ConfiguredModelSelection::None => None,
                    }
                }
            }
        };
        let provider_id = selection
            .as_ref()
            .map(|selection| selection.provider_id.clone());
        let model = selection
            .as_ref()
            .map(|selection| selection.model_id.clone());
        let provider = provider_id
            .as_deref()
            .and_then(|id| configured_provider(root, id));
        let current_metadata = provider_id
            .as_deref()
            .map(|id| resolve_provider_metadata(id, provider, model.as_deref()))
            .transpose()?;
        if let Some(metadata) = &current_metadata
            && let Some(model) = model.as_deref()
        {
            let diagnostic_provider_id = provider_id.as_deref().unwrap_or("unknown");
            if let Some(package) = metadata.unsupported_model_package.as_deref() {
                diagnostics.push(format!(
                    "OpenCode provider {diagnostic_provider_id} model {model} requires unsupported provider package {package}; choose a model supported by CLISwitch"
                ));
            } else if metadata.model_routed && metadata.protocol.is_none() {
                diagnostics.push(format!(
                    "OpenCode provider {diagnostic_provider_id} model {model} has no supported model route; choose a model from the provider catalog"
                ));
            }
        }
        if let (Some(provider_id), Some(metadata)) = (&provider_id, &current_metadata)
            && provider_id.starts_with("cliswitch_")
            && metadata.explicit_npm.is_some()
            && metadata.protocol.is_none()
        {
            return Err(AppError::Unsupported(format!(
                "unsupported OpenCode provider package: {}",
                metadata.explicit_npm.as_deref().unwrap_or_default()
            )));
        }
        let protocol = current_metadata
            .as_ref()
            .and_then(|metadata| metadata.protocol);
        let auth_file = paths
            .auth_file
            .as_ref()
            .ok_or_else(|| AppError::Unsupported("OpenCode auth path is unavailable".into()))?;
        let auth_text = read_optional(auth_file, "{}\n").await?;
        let auth = parse_jsonc_value(&auth_text)?;
        let empty_auth_root = serde_json::Map::new();
        let auth_root = match auth.as_object() {
            Some(auth_root) => auth_root,
            None => {
                diagnostics.push(
                    "OpenCode auth root is not an object; provider credentials were ignored".into(),
                );
                &empty_auth_root
            }
        };
        let current_auth_kind = provider_id.as_ref().and_then(|id| {
            auth_root
                .get(id)
                .and_then(Value::as_object)
                .and_then(|entry| entry.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let managed_provider_id = provider_id
            .as_deref()
            .and_then(|id| id.strip_prefix("cliswitch_"))
            .and_then(|id| Uuid::parse_str(id).ok());
        let mut unmanaged_api_candidates = Vec::new();
        for (auth_provider_id, auth_value) in auth_root {
            let Some(auth_entry) = auth_value.as_object() else {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} has an invalid auth entry and cannot be saved"
                ));
                continue;
            };
            match auth_entry.get("type").and_then(Value::as_str) {
                Some("oauth") => {
                    diagnostics.push(format!(
                        "OpenCode provider {auth_provider_id} uses OAuth; OpenCode OAuth providers are recognized but cannot be saved in this version"
                    ));
                    continue;
                }
                Some("api") => {}
                Some(auth_type) => {
                    diagnostics.push(format!(
                        "OpenCode provider {auth_provider_id} uses unsupported auth type {auth_type} and cannot be saved"
                    ));
                    continue;
                }
                None => {
                    diagnostics.push(format!(
                        "OpenCode provider {auth_provider_id} has no auth type and cannot be saved"
                    ));
                    continue;
                }
            }
            let Some(key) = auth_entry
                .get("key")
                .and_then(Value::as_str)
                .filter(|key| !key.trim().is_empty())
            else {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} has no API key and cannot be saved"
                ));
                continue;
            };
            let configured = configured_provider(root, auth_provider_id);
            let metadata = resolve_provider_metadata(
                auth_provider_id,
                configured,
                selection
                    .as_ref()
                    .filter(|selection| selection.provider_id == *auth_provider_id)
                    .map(|selection| selection.model_id.as_str()),
            )?;
            let mut models = provider_models(configured, selection.as_ref(), auth_provider_id);
            let catalog = runtime_catalog()?;
            let routed_template = metadata
                .template_id
                .as_deref()
                .and_then(|id| catalog.api_template(id))
                .filter(|template| template.model_routing && metadata.model_routed);
            if let Some(template) = routed_template {
                // A model-routed provider may use a different transport for every model.
                // Keep only catalog models here; unknown IDs cannot be assigned safely.
                models.retain(|model| catalog.model_routed_endpoint(&template.id, model).is_some());
                for model in template
                    .endpoints
                    .iter()
                    .flat_map(|endpoint| endpoint.models.iter())
                {
                    if !models.contains(&model.id) {
                        models.push(model.id.clone());
                    }
                }
            } else if let Some((template_id, endpoint_id)) = metadata
                .template_id
                .as_deref()
                .zip(metadata.template_endpoint_id.as_deref())
                && let Some(endpoint) = catalog.api_template(template_id).and_then(|template| {
                    template
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.id == endpoint_id)
                })
            {
                for model in &endpoint.models {
                    if !models.contains(&model.id) {
                        models.push(model.id.clone());
                    }
                }
            }
            let mut missing = Vec::new();
            if metadata.protocol.is_none() && !metadata.model_routed {
                missing.push("a supported npm package or provider relation");
            }
            if metadata.endpoint.is_none() && !metadata.model_routed {
                missing.push("options.baseURL or a default endpoint relation");
            }
            if models.is_empty() {
                missing.push("a configured or current model");
            }
            if !missing.is_empty() {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} was recognized but cannot be saved without {}",
                    missing.join(", ")
                ));
                continue;
            }
            let fallback_endpoint = metadata
                .template_id
                .as_deref()
                .and_then(|id| catalog.api_template(id))
                .and_then(|template| template.endpoints.first());
            let protocol = metadata
                .protocol
                .or_else(|| fallback_endpoint.map(|endpoint| endpoint.protocol));
            let endpoint_url = metadata
                .endpoint
                .clone()
                .or_else(|| fallback_endpoint.map(|endpoint| endpoint.base_url.to_string()));
            let auth_type = metadata
                .auth_type
                .or_else(|| fallback_endpoint.and_then(|endpoint| endpoint.default_auth_type()));
            let Some(protocol) = protocol else {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} has no supported protocol; choose a supported model or configure npm explicitly"
                ));
                continue;
            };
            let Some(endpoint_url) = endpoint_url else {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} has no endpoint; configure options.baseURL"
                ));
                continue;
            };
            let endpoint = match Url::parse(&endpoint_url) {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    diagnostics.push(format!(
                        "OpenCode provider {auth_provider_id} has an invalid endpoint and cannot be saved: {error}"
                    ));
                    continue;
                }
            };
            let auth_type = recognize_anthropic_endpoint(&endpoint)
                .map(|_| classify_credential(key).auth_type())
                .or(auth_type)
                .unwrap_or_else(|| default_auth_type(protocol));
            let selected_model = selection
                .as_ref()
                .filter(|selection| selection.provider_id == *auth_provider_id)
                .map(|selection| selection.model_id.as_str());
            let selected_model_is_supported = metadata
                .template_id
                .as_deref()
                .zip(selected_model)
                .map(|(template_id, model)| {
                    model_routed_model_is_supported(template_id, model).unwrap_or(false)
                })
                .unwrap_or(false);
            let default_model = if metadata.model_routed {
                selected_model
                    .filter(|_| selected_model_is_supported)
                    .map(str::to_string)
            } else {
                models.first().cloned()
            };
            let connection = ProviderConnection {
                id: Uuid::new_v4(),
                template_endpoint_id: if metadata.model_routed && !selected_model_is_supported {
                    None
                } else {
                    metadata.template_endpoint_id.clone()
                },
                credential_slot_id: metadata
                    .credential_slot_id
                    .clone()
                    .unwrap_or_else(|| "api-key".into()),
                protocol,
                endpoint,
                auth_type,
                api_key: key.to_string(),
                default_model: default_model.clone().unwrap_or_default(),
                verification: VerificationInfo::default(),
            };
            if !metadata.model_routed
                && let Err(error) = connection.validate()
            {
                diagnostics.push(format!(
                    "OpenCode provider {auth_provider_id} cannot be saved: {error}"
                ));
                continue;
            }
            unmanaged_api_candidates.push(AdapterApiCandidate {
                source_provider_id: auth_provider_id.clone(),
                suggested_name: metadata.display_name,
                template_id: metadata.template_id,
                connection,
                available_models: models,
                default_model,
                is_current: provider_id.as_deref() == Some(auth_provider_id),
                model_routed: metadata.model_routed,
            });
        }
        let mut sources = vec![
            SourceFileSnapshot {
                source_id: "opencode-config".into(),
                display_path: paths.config_file.clone(),
                digest: file_digest(&paths.config_file).await?,
            },
            SourceFileSnapshot {
                source_id: "opencode-auth".into(),
                display_path: auth_file.clone(),
                digest: file_digest(auth_file).await?,
            },
        ];
        if explicit_model.is_none() {
            sources.push(SourceFileSnapshot {
                source_id: "opencode-model-state".into(),
                display_path: state_file,
                digest: state_digest,
            });
        }
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: provider_id,
                protocol,
                auth_kind: current_auth_kind,
                model,
                managed_provider_id,
                sources,
                externally_overridden: false,
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
        let (connection_id, api) = match (target, &provider.data) {
            (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => {
                (*connection_id, api)
            }
            _ => {
                return Err(AppError::Validation(
                    "OpenCode only accepts endpoint + key providers".into(),
                ));
            }
        };
        let connection = api
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .ok_or_else(|| AppError::Validation("connection does not exist".into()))?;
        let runtime = runtime_catalog()?;
        let legacy = legacy_catalog()?;
        let catalog = catalog_for_provider(
            &runtime,
            legacy,
            provider.template_id.as_deref().unwrap_or_default(),
        );
        if let (Some(template_id), Some(endpoint_id)) = (
            provider.template_id.as_deref(),
            connection.template_endpoint_id.as_deref(),
        ) && catalog
            .api_template(template_id)
            .is_some_and(|template| template.model_routing)
        {
            let model = target.model().trim();
            let routed_endpoint = catalog
                .model_routed_endpoint(template_id, model)
                .ok_or_else(|| {
                    AppError::Validation(format!(
                        "model {model} has no route in provider template {template_id}"
                    ))
                })?;
            if routed_endpoint.id != endpoint_id {
                return Err(AppError::Validation(format!(
                    "model {model} routes to endpoint {}, not {endpoint_id}",
                    routed_endpoint.id
                )));
            }
        }
        let relation_package = provider
            .template_id
            .as_deref()
            .zip(connection.template_endpoint_id.as_deref())
            .and_then(|(template_id, endpoint_id)| {
                catalog
                    .api_relations(CliId::Opencode, template_id)
                    .find(|relation| relation.endpoint_id == endpoint_id)
                    .and_then(|relation| relation.provider_package.as_deref())
            });
        let package = relation_package
            .or_else(|| catalog.protocol_package(CliId::Opencode, connection.protocol))
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "OpenCode has no provider package for {}",
                    connection.protocol
                ))
            })?;
        let provider_id = namespaced_provider_id(provider.id);
        let config_source = read_optional(&paths.config_file, "{}\n").await?;
        let parsed = parse_jsonc_value(&config_source)?;
        if parsed.get("providers").is_some() {
            return Err(AppError::Unsupported(
                "refusing to write the OpenCode v2 beta schema".into(),
            ));
        }
        let provider_value = json!({
            "npm": package,
            "name": provider.name,
            "options": { "baseURL": connection.endpoint.as_str() },
            "models": opencode_models(provider, connection, target.model())
        });
        let target_config = patch_jsonc(
            &config_source,
            &[
                JsonPatch::SetValue {
                    path: vec!["provider".into(), provider_id.clone()],
                    value: provider_value,
                },
                JsonPatch::SetString {
                    path: vec!["model".into()],
                    value: format!("{provider_id}/{}", target.model()),
                },
            ],
        )?;
        let auth_file = paths
            .auth_file
            .clone()
            .ok_or_else(|| AppError::Unsupported("OpenCode auth path is unavailable".into()))?;
        let auth_root = auth_file
            .parent()
            .ok_or_else(|| AppError::Validation("OpenCode auth path has no parent".into()))?
            .to_path_buf();
        let auth_source = read_optional(&auth_file, "{}\n").await?;
        let target_auth = patch_jsonc(
            &auth_source,
            &[JsonPatch::SetValue {
                path: vec![provider_id],
                value: json!({ "type": "api", "key": connection.api_key }),
            }],
        )?;
        Ok(AdapterWritePlan {
            cli_id: CliId::Opencode,
            files: vec![
                FileWritePlan {
                    path: paths.config_file.clone(),
                    allowed_root: paths.config_directory.clone(),
                    source_digest: file_digest(&paths.config_file).await?,
                    target_content: target_config.into_bytes(),
                    contains_credentials: false,
                    opaque_content: false,
                },
                FileWritePlan {
                    path: auth_file.clone(),
                    allowed_root: auth_root,
                    source_digest: file_digest(&auth_file).await?,
                    target_content: target_auth.into_bytes(),
                    contains_credentials: true,
                    opaque_content: false,
                },
            ],
            warning: None,
        })
    }

    async fn verify_applied(
        &self,
        paths: &AdapterPaths,
        target: &ConfigurationTarget,
        provider: &ProviderProfile,
    ) -> AppResult<bool> {
        let plan = self.plan_write(paths, target, provider).await?;
        for file in plan.files {
            let current = tokio::fs::read(&file.path).await?;
            if bytes_digest(&current) != bytes_digest(&file.target_content) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn oauth_kind(&self) -> Option<OAuthKind> {
        None
    }

    fn validate_imported_auth(&self, _bytes: &[u8]) -> AppResult<Option<String>> {
        Err(AppError::Unsupported(
            "OpenCode OAuth is not supported in 0.1".into(),
        ))
    }

    fn fixed_oauth_command(
        &self,
        _executable: PathBuf,
        _isolated_home: PathBuf,
    ) -> AppResult<FixedOAuthCommand> {
        Err(AppError::Unsupported(
            "OpenCode OAuth is not supported in 0.1".into(),
        ))
    }
}
