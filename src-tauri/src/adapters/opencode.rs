use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan, CliAdapter,
        FileWritePlan, FixedOAuthCommand, HostEnvironment, namespaced_provider_id, read_optional,
    },
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, SourceFileSnapshot,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    services::config_writer::{JsonPatch, parse_jsonc_value, patch_jsonc},
};

#[derive(Debug, Default)]
pub struct OpenCodeAdapter;

#[async_trait]
impl CliAdapter for OpenCodeAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::Opencode,
            display_name: "OpenCode".into(),
            command: "opencode".into(),
            schema_fingerprint: "stable-v1:provider/npm/options/models+auth.type-api".into(),
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
        _environment: &HostEnvironment,
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
        let default_model = root
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string);
        let provider_id = default_model.as_deref().and_then(|model| {
            model
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
        });
        let model = default_model
            .as_deref()
            .and_then(|model| model.split_once('/').map(|(_, model)| model.to_string()));
        let provider = provider_id.as_ref().and_then(|id| {
            root.get("provider")
                .and_then(Value::as_object)
                .and_then(|providers| providers.get(id))
                .and_then(Value::as_object)
        });
        let endpoint = provider
            .and_then(|provider| provider.get("options"))
            .and_then(Value::as_object)
            .and_then(|options| options.get("baseURL"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let npm = provider
            .and_then(|provider| provider.get("npm"))
            .and_then(Value::as_str);
        let protocol = match npm {
            Some("@ai-sdk/openai-compatible") => Some(CliProtocol::OpenaiChat),
            Some("@ai-sdk/openai") => Some(CliProtocol::OpenaiResponses),
            Some("@ai-sdk/anthropic") => Some(CliProtocol::AnthropicMessages),
            Some(value)
                if provider_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("cliswitch_")) =>
            {
                return Err(AppError::Unsupported(format!(
                    "unsupported OpenCode provider package: {value}"
                )));
            }
            _ => None,
        };
        let auth_file = paths
            .auth_file
            .as_ref()
            .ok_or_else(|| AppError::Unsupported("OpenCode auth path is unavailable".into()))?;
        let auth_text = read_optional(auth_file, "{}\n").await?;
        let auth = parse_jsonc_value(&auth_text)?;
        let key = provider_id.as_ref().and_then(|id| {
            auth.get(id)
                .and_then(Value::as_object)
                .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("api"))
                .and_then(|entry| entry.get("key"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let managed_provider_id = provider_id
            .as_deref()
            .and_then(|id| id.strip_prefix("cliswitch_"))
            .and_then(|id| Uuid::parse_str(id).ok());
        let candidate = match (&endpoint, &key, &model, protocol) {
            (Some(endpoint), Some(key), Some(model), Some(protocol)) => Some(ProviderConnection {
                id: Uuid::new_v4(),
                protocol,
                endpoint: Url::parse(endpoint)?,
                auth_type: if protocol == CliProtocol::AnthropicMessages {
                    ConnectionAuthType::ApiKey
                } else {
                    ConnectionAuthType::Bearer
                },
                api_key: key.clone(),
                default_model: model.clone(),
                verification: VerificationInfo::default(),
            }),
            _ => None,
        };
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: provider_id,
                protocol,
                auth_kind: key.as_ref().map(|_| "api".into()),
                model,
                managed_provider_id,
                sources: vec![
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
                ],
                externally_overridden: false,
                diagnostics: Vec::new(),
            },
            unmanaged_candidate: candidate,
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
        let package = match connection.protocol {
            CliProtocol::OpenaiChat => "@ai-sdk/openai-compatible",
            CliProtocol::OpenaiResponses => "@ai-sdk/openai",
            CliProtocol::AnthropicMessages => "@ai-sdk/anthropic",
        };
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
            "models": {
                target.model(): { "name": target.model() }
            }
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
