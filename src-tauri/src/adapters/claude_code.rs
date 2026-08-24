use std::{collections::BTreeMap, path::PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan, CliAdapter,
        FileWritePlan, FixedOAuthCommand, HostEnvironment, read_optional,
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
pub struct ClaudeCodeAdapter;

#[async_trait]
impl CliAdapter for ClaudeCodeAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::ClaudeCode,
            display_name: "Claude Code".into(),
            command: "claude".into(),
            schema_fingerprint: "stable-2026-08:settings.model+env/.credentials.json".into(),
        }
    }

    fn resolve_paths(
        &self,
        environment: &HostEnvironment,
        manual: Option<PathBuf>,
    ) -> AdapterPaths {
        let directory = manual
            .or_else(|| environment.absolute_path("CLAUDE_CONFIG_DIR"))
            .unwrap_or_else(|| environment.home.join(".claude"));
        AdapterPaths {
            config_file: directory.join("settings.json"),
            auth_file: Some(directory.join(".credentials.json")),
            config_directory: directory,
        }
    }

    async fn read_current(
        &self,
        paths: &AdapterPaths,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterReadResult> {
        let text = read_optional(&paths.config_file, "{}\n").await?;
        let digest = file_digest(&paths.config_file).await?;
        let value = parse_jsonc_value(&text)?;
        let object = value
            .as_object()
            .ok_or_else(|| AppError::Unsupported("Claude settings root is not an object".into()))?;
        let env = object.get("env").and_then(Value::as_object);
        if object.contains_key("env") && env.is_none() {
            return Err(AppError::Unsupported(
                "Claude settings env field is not an object".into(),
            ));
        }
        let model = object
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| env.and_then(|env| env.get("ANTHROPIC_MODEL")?.as_str()))
            .map(str::to_string);
        let endpoint = env
            .and_then(|env| env.get("ANTHROPIC_BASE_URL")?.as_str())
            .map(str::to_string);
        let api_key = env
            .and_then(|env| env.get("ANTHROPIC_API_KEY")?.as_str())
            .map(|value| (ConnectionAuthType::ApiKey, value.to_string()))
            .or_else(|| {
                env.and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN")?.as_str())
                    .map(|value| (ConnectionAuthType::Bearer, value.to_string()))
            });
        let oauth_token = env
            .and_then(|env| env.get("CLAUDE_CODE_OAUTH_TOKEN")?.as_str())
            .map(str::to_string);
        let auth_file_exists = paths
            .auth_file
            .as_ref()
            .map(|path| path.exists())
            .unwrap_or(false);
        let externally_overridden = [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ]
        .iter()
        .any(|key| environment.is_present(key));
        let candidate = match (&endpoint, &api_key, &model) {
            (Some(endpoint), Some((auth_type, key)), Some(model)) => Some(ProviderConnection {
                id: Uuid::new_v4(),
                protocol: CliProtocol::AnthropicMessages,
                endpoint: Url::parse(endpoint)?,
                auth_type: *auth_type,
                api_key: key.clone(),
                default_model: model.clone(),
                verification: VerificationInfo::default(),
            }),
            _ => None,
        };
        let auth_kind = if oauth_token.is_some() {
            Some("oauth".into())
        } else if api_key.is_some() {
            Some("api".into())
        } else if auth_file_exists {
            Some("oauth".into())
        } else {
            None
        };
        let mut sources = vec![SourceFileSnapshot {
            source_id: "claude-settings".into(),
            display_path: paths.config_file.clone(),
            digest,
        }];
        if let Some(auth_file) = paths.auth_file.as_ref()
            && auth_file.exists()
        {
            sources.push(SourceFileSnapshot {
                source_id: "claude-auth".into(),
                display_path: auth_file.clone(),
                digest: file_digest(auth_file).await?,
            });
        }
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: endpoint.clone(),
                protocol: endpoint.as_ref().map(|_| CliProtocol::AnthropicMessages),
                auth_kind,
                model,
                managed_provider_id: None,
                sources,
                externally_overridden,
                diagnostics: if externally_overridden {
                    vec!["A process environment variable overrides the user setting".into()]
                } else {
                    Vec::new()
                },
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
        let source = read_optional(&paths.config_file, "{}\n").await?;
        let model = target.model();
        let mut patches = vec![JsonPatch::SetString {
            path: vec!["model".into()],
            value: model.to_string(),
        }];
        let mut files = Vec::new();
        let mut warning = None;
        match (target, &provider.data) {
            (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => {
                let connection = api
                    .connections
                    .iter()
                    .find(|connection| connection.id == *connection_id)
                    .ok_or_else(|| AppError::Validation("connection does not exist".into()))?;
                if connection.protocol != CliProtocol::AnthropicMessages {
                    return Err(AppError::Validation(
                        "Claude Code only accepts Anthropic Messages".into(),
                    ));
                }
                patches.extend([
                    JsonPatch::SetString {
                        path: vec!["env".into(), "ANTHROPIC_BASE_URL".into()],
                        value: connection.endpoint.to_string(),
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "ANTHROPIC_MODEL".into()],
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "CLAUDE_CODE_OAUTH_TOKEN".into()],
                    },
                ]);
                match connection.auth_type {
                    ConnectionAuthType::ApiKey => {
                        patches.push(JsonPatch::SetString {
                            path: vec!["env".into(), "ANTHROPIC_API_KEY".into()],
                            value: connection.api_key.clone(),
                        });
                        patches.push(JsonPatch::Remove {
                            path: vec!["env".into(), "ANTHROPIC_AUTH_TOKEN".into()],
                        });
                    }
                    ConnectionAuthType::Bearer => {
                        patches.push(JsonPatch::SetString {
                            path: vec!["env".into(), "ANTHROPIC_AUTH_TOKEN".into()],
                            value: connection.api_key.clone(),
                        });
                        patches.push(JsonPatch::Remove {
                            path: vec!["env".into(), "ANTHROPIC_API_KEY".into()],
                        });
                    }
                }
            }
            (ConfigurationTarget::Oauth { .. }, ProviderData::Oauth(oauth))
                if oauth.oauth_kind == OAuthKind::Anthropic =>
            {
                patches.extend([
                    JsonPatch::Remove {
                        path: vec!["env".into(), "ANTHROPIC_BASE_URL".into()],
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "ANTHROPIC_API_KEY".into()],
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "ANTHROPIC_AUTH_TOKEN".into()],
                    },
                ]);
                if cfg!(target_os = "macos") {
                    patches.push(JsonPatch::SetString {
                        path: vec!["env".into(), "CLAUDE_CODE_OAUTH_TOKEN".into()],
                        value: oauth.raw_content.clone(),
                    });
                } else {
                    patches.push(JsonPatch::Remove {
                        path: vec!["env".into(), "CLAUDE_CODE_OAUTH_TOKEN".into()],
                    });
                    let auth_file = paths.auth_file.clone().ok_or_else(|| {
                        AppError::Unsupported("Claude auth file is unavailable".into())
                    })?;
                    files.push(FileWritePlan {
                        source_digest: file_digest(&auth_file).await?,
                        path: auth_file,
                        allowed_root: paths.config_directory.clone(),
                        target_content: oauth.raw_content.as_bytes().to_vec(),
                        contains_credentials: true,
                        opaque_content: oauth.manually_modified,
                    });
                }
                if oauth.manually_modified {
                    warning = Some(
                        "OAuth content was edited manually and will be written without schema validation"
                            .into(),
                    );
                }
            }
            _ => {
                return Err(AppError::Validation(
                    "Claude target does not match the provider type".into(),
                ));
            }
        }
        let target_content = patch_jsonc(&source, &patches)?.into_bytes();
        files.insert(
            0,
            FileWritePlan {
                source_digest: file_digest(&paths.config_file).await?,
                path: paths.config_file.clone(),
                allowed_root: paths.config_directory.clone(),
                target_content,
                contains_credentials: true,
                opaque_content: false,
            },
        );
        Ok(AdapterWritePlan {
            cli_id: CliId::ClaudeCode,
            files,
            warning,
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
        Some(OAuthKind::Anthropic)
    }

    fn validate_imported_auth(&self, bytes: &[u8]) -> AppResult<Option<String>> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|_| AppError::Validation("unrecognized Claude auth JSON".into()))?;
        if !value.is_object() {
            return Err(AppError::Validation(
                "Claude auth must be a JSON object".into(),
            ));
        }
        let account = value
            .pointer("/claudeAiOauth/accountUuid")
            .and_then(Value::as_str)
            .or_else(|| value.get("accountUuid").and_then(Value::as_str))
            .map(str::to_string);
        let has_auth = value
            .pointer("/claudeAiOauth/accessToken")
            .and_then(Value::as_str)
            .is_some_and(|token| !token.trim().is_empty());
        if !has_auth {
            return Err(AppError::Validation(
                "Claude auth does not contain a recognized token field".into(),
            ));
        }
        Ok(account)
    }

    fn fixed_oauth_command(
        &self,
        executable: PathBuf,
        isolated_home: PathBuf,
    ) -> AppResult<FixedOAuthCommand> {
        let mut environment = BTreeMap::new();
        environment.insert(
            "CLAUDE_CONFIG_DIR".into(),
            isolated_home.to_string_lossy().to_string(),
        );
        Ok(FixedOAuthCommand {
            executable,
            args: if cfg!(target_os = "macos") {
                vec!["setup-token".into()]
            } else {
                vec!["auth".into(), "login".into()]
            },
            environment,
            artifact: isolated_home.join(".credentials.json"),
        })
    }
}
