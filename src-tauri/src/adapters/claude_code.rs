use std::{collections::BTreeMap, path::PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::{
    adapters::traits::{
        AdapterApiCandidate, AdapterMetadata, AdapterPaths, AdapterReadResult, AdapterWritePlan,
        CliAdapter, FileWritePlan, FixedOAuthCommand, HostEnvironment, read_optional,
    },
    catalog::{CatalogProviderInfo, legacy_catalog, runtime_catalog},
    domain::{
        CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, CurrentCliConfiguration,
        OAuthKind, ProviderConnection, ProviderData, ProviderProfile, SourceFileSnapshot,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    services::{
        config_writer::{JsonPatch, parse_jsonc_value, patch_jsonc},
        minimax::{
            ANTHROPIC_ENDPOINT_ID, classify_credential, recognize_anthropic_endpoint, template_id,
        },
    },
};

#[derive(Debug, Default)]
pub struct ClaudeCodeAdapter;

fn same_endpoint(left: &Url, right: &Url) -> bool {
    left.scheme().eq_ignore_ascii_case(right.scheme())
        && left.host_str().map(str::to_ascii_lowercase)
            == right.host_str().map(str::to_ascii_lowercase)
        && left.port_or_known_default() == right.port_or_known_default()
        && left.path().trim_end_matches('/') == right.path().trim_end_matches('/')
        && left.query() == right.query()
        && left.fragment() == right.fragment()
}

fn same_anthropic_endpoint(left: &Url, right: &Url) -> bool {
    if same_endpoint(left, right) {
        return true;
    }
    let left_path = left.path().trim_end_matches('/');
    let right_path = right.path().trim_end_matches('/');
    fn without_v1(path: &str) -> &str {
        path.strip_suffix("/v1").unwrap_or(path)
    }
    left.scheme().eq_ignore_ascii_case(right.scheme())
        && left.host_str().map(str::to_ascii_lowercase)
            == right.host_str().map(str::to_ascii_lowercase)
        && left.port_or_known_default() == right.port_or_known_default()
        && without_v1(left_path) == without_v1(right_path)
        && left.query() == right.query()
        && left.fragment() == right.fragment()
}

fn dynamic_anthropic_provider<'a>(
    catalog: &'a crate::catalog::ProviderCatalog,
    endpoint: &Url,
    api_key: &str,
) -> Option<&'a CatalogProviderInfo> {
    let providers = catalog.provider_info.as_ref()?;
    let mut matches = providers
        .iter()
        .filter(|info| {
            info.selectable
                && info.endpoints.iter().any(|candidate| {
                    candidate.selectable
                        && candidate.protocol == Some(CliProtocol::AnthropicMessages)
                        && candidate
                            .endpoint
                            .as_ref()
                            .is_some_and(|candidate| same_anthropic_endpoint(candidate, endpoint))
                })
        })
        .collect::<Vec<_>>();
    if matches.len() <= 1 {
        return matches.pop();
    }
    // CLIAdapter can publish separate API and coding-plan identities at the same endpoint. The
    // stable token-plan prefix is the only credential signal we use to disambiguate them; if a
    // future snapshot remains ambiguous, leave the provider unmanaged instead of guessing.
    let token_plan = api_key.starts_with("sk-cp-");
    let preferred = matches
        .iter()
        .copied()
        .filter(|info| info.id.ends_with("-coding-plan") == token_plan)
        .collect::<Vec<_>>();
    if preferred.len() == 1 {
        preferred.into_iter().next()
    } else {
        None
    }
}

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
            .map(str::to_string);
        let auth_token = env
            .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN")?.as_str())
            .map(str::to_string);
        if api_key.is_some() && auth_token.is_some() {
            return Err(AppError::Unsupported(
                "Claude settings contain both ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN; remove one credential before importing"
                    .into(),
            ));
        }
        if environment.is_present("ANTHROPIC_API_KEY")
            && environment.is_present("ANTHROPIC_AUTH_TOKEN")
        {
            return Err(AppError::Unsupported(
                "The process environment contains both ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN; remove one credential override before scanning"
                    .into(),
            ));
        }
        let credential = api_key
            .map(|value| (ConnectionAuthType::ApiKey, value))
            .or_else(|| auth_token.map(|value| (ConnectionAuthType::Bearer, value)));
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
        let mut recognized_provider_name = None;
        let candidate = match (&endpoint, &credential) {
            (Some(endpoint), Some((configured_auth_type, key))) => {
                let parsed_endpoint = Url::parse(endpoint)?;
                let catalog = runtime_catalog()?;
                // A CLIAdapter provider is identified by a declared Anthropic endpoint.
                let dynamic_info = dynamic_anthropic_provider(&catalog, &parsed_endpoint, key);
                if let Some(info) = dynamic_info {
                    recognized_provider_name = Some(info.name.clone());
                    Some(AdapterApiCandidate {
                        source_provider_id: info.id.clone(),
                        suggested_name: info.name.clone(),
                        template_id: Some(info.id.clone()),
                        available_models: model.iter().cloned().collect(),
                        default_model: model.clone(),
                        is_current: true,
                        model_routed: false,
                        connection: ProviderConnection {
                            id: Uuid::new_v4(),
                            template_endpoint_id: Some("anthropic-messages".into()),
                            credential_slot_id: "api-key".into(),
                            protocol: CliProtocol::AnthropicMessages,
                            endpoint: parsed_endpoint,
                            auth_type: *configured_auth_type,
                            api_key: key.clone(),
                            default_model: model.clone().unwrap_or_default(),
                            verification: VerificationInfo::default(),
                        },
                    })
                } else if let Some(region) = recognize_anthropic_endpoint(&parsed_endpoint) {
                    // Keep the narrowly-scoped MiniMax import recognition for legacy snapshots;
                    // it is not used by the CLIAdapter runtime catalog.
                    let credential_kind = classify_credential(key);
                    let template_id = template_id(region, credential_kind);
                    let legacy = legacy_catalog()?;
                    let template = legacy.api_template(template_id).ok_or_else(|| {
                        AppError::Serialization(format!(
                            "MiniMax provider template {template_id} is unavailable"
                        ))
                    })?;
                    let template_endpoint = template
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.id == ANTHROPIC_ENDPOINT_ID)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "MiniMax provider template {template_id} has no Anthropic endpoint"
                            ))
                        })?;
                    recognized_provider_name = Some(template.name.clone());
                    Some(AdapterApiCandidate {
                        source_provider_id: template_id.into(),
                        suggested_name: template.name.clone(),
                        template_id: Some(template_id.into()),
                        available_models: model.iter().cloned().collect(),
                        default_model: model.clone(),
                        is_current: true,
                        model_routed: false,
                        connection: ProviderConnection {
                            id: Uuid::new_v4(),
                            template_endpoint_id: Some(ANTHROPIC_ENDPOINT_ID.into()),
                            credential_slot_id: template_endpoint.credential_slot_id.clone(),
                            protocol: CliProtocol::AnthropicMessages,
                            endpoint: parsed_endpoint,
                            auth_type: *configured_auth_type,
                            api_key: key.clone(),
                            default_model: model.clone().unwrap_or_default(),
                            verification: VerificationInfo::default(),
                        },
                    })
                } else {
                    Some(AdapterApiCandidate {
                        source_provider_id: "claude-code".into(),
                        suggested_name: "Claude Code API".into(),
                        template_id: None,
                        available_models: model.iter().cloned().collect(),
                        default_model: model.clone(),
                        is_current: true,
                        model_routed: false,
                        connection: ProviderConnection {
                            id: Uuid::new_v4(),
                            template_endpoint_id: None,
                            credential_slot_id: "api-key".into(),
                            protocol: CliProtocol::AnthropicMessages,
                            endpoint: parsed_endpoint,
                            auth_type: *configured_auth_type,
                            api_key: key.clone(),
                            default_model: model.clone().unwrap_or_default(),
                            verification: VerificationInfo::default(),
                        },
                    })
                }
            }
            _ => None,
        };
        let auth_kind = if oauth_token.is_some() {
            Some("oauth".into())
        } else if credential.is_some() {
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
        let unmanaged_api_candidates = candidate.into_iter().collect();
        Ok(AdapterReadResult {
            current: CurrentCliConfiguration {
                provider_name: recognized_provider_name.or_else(|| endpoint.clone()),
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
            unmanaged_api_candidates,
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
                let active_catalog = runtime_catalog()?;
                let (effective_endpoint, effective_auth_type) = match (
                    provider.template_id.as_deref(),
                    connection.template_endpoint_id.as_deref(),
                ) {
                    (Some(template_id), Some(endpoint_id)) => {
                        let is_runtime_template =
                            active_catalog.api_template(template_id).is_some();
                        let catalog = if is_runtime_template {
                            &active_catalog
                        } else {
                            legacy_catalog()?
                        };
                        let relation = catalog
                            .api_relation(CliId::ClaudeCode, template_id, endpoint_id)
                            .ok_or_else(|| {
                                AppError::Validation(format!(
                                    "Claude Code has no relation for template {template_id} endpoint {endpoint_id}"
                                ))
                            })?;
                        let auth_type = catalog.relation_auth_type(relation).ok_or_else(|| {
                            AppError::Serialization(format!(
                                "Claude Code relation {} has no valid auth option",
                                relation.id
                            ))
                        })?;
                        let auth_type = if is_runtime_template {
                            catalog
                                .api_template(template_id)
                                .and_then(|template| {
                                    template
                                        .endpoints
                                        .iter()
                                        .find(|endpoint| endpoint.id == endpoint_id)
                                })
                                .filter(|endpoint| {
                                    endpoint
                                        .auth_options
                                        .iter()
                                        .any(|option| option.auth_type == connection.auth_type)
                                })
                                .map_or(auth_type, |_| connection.auth_type)
                        } else {
                            // Legacy MiniMax identities encode API versus coding-plan auth in the
                            // relation itself, even though their endpoint schema permits both.
                            auth_type
                        };
                        (
                            relation.base_url.as_ref().unwrap_or(&connection.endpoint),
                            auth_type,
                        )
                    }
                    (Some(template_id), None)
                        if active_catalog
                            .dynamic_provider_info(template_id)
                            .is_some_and(|info| {
                                info.selectable
                                    && info.endpoints.iter().any(|endpoint| {
                                        endpoint.selectable
                                            && endpoint.protocol
                                                == Some(CliProtocol::AnthropicMessages)
                                    })
                            }) =>
                    {
                        (&connection.endpoint, connection.auth_type)
                    }
                    (None, None) => (&connection.endpoint, connection.auth_type),
                    _ => {
                        return Err(AppError::Validation(
                            "Claude provider template identity is incomplete".into(),
                        ));
                    }
                };
                patches.extend([
                    JsonPatch::SetString {
                        path: vec!["env".into(), "ANTHROPIC_BASE_URL".into()],
                        value: effective_endpoint.to_string(),
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "ANTHROPIC_MODEL".into()],
                    },
                    JsonPatch::Remove {
                        path: vec!["env".into(), "CLAUDE_CODE_OAUTH_TOKEN".into()],
                    },
                ]);
                match effective_auth_type {
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
