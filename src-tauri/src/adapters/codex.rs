use std::{collections::BTreeMap, path::PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterApiCandidate, AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan,
        CliAdapter, FileWritePlan, FixedOAuthCommand, HostEnvironment, namespaced_provider_id,
        read_optional,
    },
    catalog::runtime_catalog,
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, SourceFileSnapshot,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    services::config_writer::{parse_toml, patch_codex_api_toml, patch_codex_oauth_toml},
};

#[derive(Debug, Default)]
pub struct CodexAdapter;

#[async_trait]
impl CliAdapter for CodexAdapter {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            cli_id: CliId::Codex,
            display_name: "Codex CLI".into(),
            command: "codex".into(),
            schema_fingerprint:
                "stable-2026-08:model_providers.responses+experimental_bearer_token/file-auth"
                    .into(),
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
        let managed_provider_id = provider_id
            .strip_prefix("cliswitch_")
            .and_then(|value| Uuid::parse_str(value).ok());
        let dynamic_info = if let (Some(endpoint), Some(provider_info)) =
            (endpoint.as_deref(), runtime_catalog()?.provider_info)
        {
            let parsed = Url::parse(endpoint)?;
            provider_info.into_iter().find(|info| {
                info.selectable
                    && info.protocol == Some(CliProtocol::OpenaiResponses)
                    && (info.id == provider_id
                        || info.endpoint.as_ref().is_some_and(|candidate| {
                            candidate.as_str().trim_end_matches('/')
                                == parsed.as_str().trim_end_matches('/')
                        }))
            })
        } else {
            None
        };
        let candidate = match (&endpoint, &key, &model) {
            (Some(endpoint), Some(key), Some(model)) => Some((
                dynamic_info.clone(),
                ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse(endpoint)?,
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: key.clone(),
                    default_model: model.clone(),
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
        let unmanaged_api_candidates = candidate
            .into_iter()
            .map(|(dynamic_info, connection)| {
                let mut available_models = dynamic_info
                    .as_ref()
                    .map(|info| {
                        info.models
                            .iter()
                            .filter(|model| model.selectable)
                            .map(|model| model.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !available_models.contains(&connection.default_model) {
                    available_models.push(connection.default_model.clone());
                }
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
                    default_model: Some(connection.default_model.clone()),
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
                diagnostics: forced_login
                    .map(|method| vec![format!("forced_login_method is set to {method}")])
                    .unwrap_or_default(),
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
        let source = read_optional(&paths.config_file, "").await?;
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
                patch_codex_api_toml(
                    &source,
                    &namespaced_provider_id(provider.id),
                    &provider.name,
                    connection.endpoint.as_str(),
                    &connection.api_key,
                    target.model(),
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
                files.push(FileWritePlan {
                    source_digest: file_digest(&auth_file).await?,
                    path: auth_file,
                    allowed_root: paths.config_directory.clone(),
                    target_content: oauth.raw_content.as_bytes().to_vec(),
                    contains_credentials: true,
                    opaque_content: oauth.manually_modified,
                });
                patch_codex_oauth_toml(&source, target.model())?
            }
            _ => {
                return Err(AppError::Validation(
                    "Codex target does not match the provider type".into(),
                ));
            }
        };
        files.insert(
            0,
            FileWritePlan {
                source_digest: file_digest(&paths.config_file).await?,
                path: paths.config_file.clone(),
                allowed_root: paths.config_directory.clone(),
                target_content: target_toml.into_bytes(),
                contains_credentials: matches!(target, ConfigurationTarget::Api { .. }),
                opaque_content: false,
            },
        );
        Ok(AdapterWritePlan {
            cli_id: CliId::Codex,
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
