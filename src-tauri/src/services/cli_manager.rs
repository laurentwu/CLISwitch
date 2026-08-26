use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    adapters::{ClaudeCodeAdapter, CliAdapter, CodexAdapter, HostEnvironment, OpenCodeAdapter},
    catalog::{ApiProviderTemplate, embedded_catalog},
    domain::{
        ApiProviderData, AppSettings, CliId, CurrentCliConfiguration, DetectedCli,
        DetectedProviderCandidate, OAuthKind, ProviderData, ProviderProfile, ScanSnapshot,
        ScanStatus, SourceFileSnapshot, UnmanagedCandidate, UnmanagedCandidateData,
        VerificationInfo,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    persistence::repository::Repository,
    services::{
        discovery::discover_executable,
        minimax::normalize_provider_credential_kind,
        oauth::{OAuthService, read_oauth_auth_file},
        redaction::Redactor,
    },
};

const CANDIDATE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct AdapterRegistry {
    adapters: Arc<HashMap<CliId, Arc<dyn CliAdapter>>>,
}

impl std::fmt::Debug for AdapterRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterRegistry")
            .field("cli_ids", &self.adapters.keys())
            .finish()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        let adapters: HashMap<CliId, Arc<dyn CliAdapter>> = [
            (
                CliId::ClaudeCode,
                Arc::new(ClaudeCodeAdapter) as Arc<dyn CliAdapter>,
            ),
            (CliId::Codex, Arc::new(CodexAdapter) as Arc<dyn CliAdapter>),
            (
                CliId::Opencode,
                Arc::new(OpenCodeAdapter) as Arc<dyn CliAdapter>,
            ),
        ]
        .into_iter()
        .collect();
        Self {
            adapters: Arc::new(adapters),
        }
    }
}

impl AdapterRegistry {
    pub fn get(&self, cli_id: CliId) -> Arc<dyn CliAdapter> {
        self.adapters
            .get(&cli_id)
            .expect("all supported adapters are registered")
            .clone()
    }
}

#[derive(Debug, Clone)]
struct CandidateEntry {
    candidate: UnmanagedCandidate,
    created_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct UnmanagedCandidateSaveRequest {
    pub snapshot_id: Uuid,
    pub candidate_id: Uuid,
    pub name: String,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CliManager {
    registry: AdapterRegistry,
    repository: Repository,
    candidates: Arc<RwLock<HashMap<Uuid, CandidateEntry>>>,
    latest_snapshot: Arc<RwLock<Option<ScanSnapshot>>>,
    redactor: Redactor,
    #[cfg(test)]
    environment_override: Option<HostEnvironment>,
}

impl CliManager {
    pub fn new(registry: AdapterRegistry, repository: Repository, redactor: Redactor) -> Self {
        Self {
            registry,
            repository,
            candidates: Arc::new(RwLock::new(HashMap::new())),
            latest_snapshot: Arc::new(RwLock::new(None)),
            redactor,
            #[cfg(test)]
            environment_override: None,
        }
    }

    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    pub async fn latest_snapshot(&self) -> Option<ScanSnapshot> {
        self.latest_snapshot.read().await.clone()
    }

    fn capture_environment(&self) -> AppResult<HostEnvironment> {
        #[cfg(test)]
        if let Some(environment) = &self.environment_override {
            return Ok(environment.clone());
        }
        HostEnvironment::capture()
    }

    pub async fn scan(&self, settings: &AppSettings) -> ScanSnapshot {
        let snapshot_id = Uuid::new_v4();
        let environment = match self.capture_environment() {
            Ok(environment) => environment,
            Err(error) => {
                let snapshot = ScanSnapshot {
                    id: snapshot_id,
                    generated_at: Utc::now(),
                    items: CliId::ALL
                        .into_iter()
                        .map(|cli_id| DetectedCli {
                            cli_id,
                            label: cli_id.label().into(),
                            status: ScanStatus::Unreadable,
                            executable_path: None,
                            config_directory: PathBuf::new(),
                            version: None,
                            source: "environment".into(),
                            current: Some(CurrentCliConfiguration {
                                provider_name: None,
                                protocol: None,
                                auth_kind: None,
                                model: None,
                                managed_provider_id: None,
                                sources: Vec::new(),
                                externally_overridden: false,
                                diagnostics: vec![self.redactor.sanitize(error.to_string())],
                            }),
                            provider_candidates: Vec::new(),
                        })
                        .collect(),
                };
                *self.latest_snapshot.write().await = Some(snapshot.clone());
                return snapshot;
            }
        };
        self.evict_expired_candidates().await;
        let mut items = Vec::with_capacity(3);
        for cli_id in CliId::ALL {
            let adapter = self.registry.get(cli_id);
            let location = settings
                .manual_locations
                .iter()
                .find(|item| item.cli_id == cli_id);
            let paths = adapter.resolve_paths(
                &environment,
                location.and_then(|item| item.config_directory.clone()),
            );
            let discovered = discover_executable(
                cli_id,
                &environment,
                location.and_then(|item| item.executable_path.as_deref()),
            )
            .await;
            let executable = match discovered {
                Ok(value) => value,
                Err(error) => {
                    items.push(DetectedCli {
                        cli_id,
                        label: cli_id.label().into(),
                        status: ScanStatus::Unreadable,
                        executable_path: None,
                        config_directory: paths.config_directory,
                        version: None,
                        source: "manual override".into(),
                        current: Some(error_current(&self.redactor, error)),
                        provider_candidates: Vec::new(),
                    });
                    continue;
                }
            };
            let Some(executable) = executable else {
                items.push(DetectedCli {
                    cli_id,
                    label: cli_id.label().into(),
                    status: ScanStatus::NotInstalled,
                    executable_path: None,
                    config_directory: paths.config_directory,
                    version: None,
                    source: "not found".into(),
                    current: None,
                    provider_candidates: Vec::new(),
                });
                continue;
            };
            match adapter.read_current(&paths, &environment).await {
                Ok(mut read) => {
                    let mut oauth_unmanaged = false;
                    let mut unmatched_api_candidates = Vec::new();
                    for candidate in std::mem::take(&mut read.unmanaged_api_candidates) {
                        if let Some(provider_id) =
                            self.match_saved_connection(&candidate.connection).await
                        {
                            if candidate.is_current {
                                read.current.managed_provider_id = Some(provider_id);
                                read.current.provider_name = self
                                    .repository
                                    .get_provider(provider_id)
                                    .await
                                    .ok()
                                    .map(|provider| provider.name);
                            }
                        } else {
                            unmatched_api_candidates.push(candidate);
                        }
                    }
                    if read.current.auth_kind.as_deref() == Some("oauth")
                        && cli_id != CliId::Opencode
                    {
                        match self.repository.get_active_oauth_binding(cli_id).await {
                            Ok(Some(binding)) => {
                                let identity_matches =
                                    if cfg!(target_os = "macos") && cli_id == CliId::ClaudeCode {
                                        true
                                    } else if let Some(auth_file) = paths.auth_file.as_ref() {
                                        match tokio::fs::read(auth_file).await {
                                            Ok(bytes) => {
                                                let digest_matches =
                                                    crate::filesystem::digest::bytes_digest(&bytes)
                                                        == binding.native_digest;
                                                let account_matches = adapter
                                                    .validate_imported_auth(&bytes)
                                                    .ok()
                                                    .flatten()
                                                    .as_deref()
                                                    .zip(binding.account_identity.as_deref())
                                                    .is_some_and(|(current, expected)| {
                                                        current == expected
                                                    });
                                                digest_matches || account_matches
                                            }
                                            Err(_) => false,
                                        }
                                    } else {
                                        false
                                    };
                                if identity_matches {
                                    read.current.managed_provider_id = Some(binding.provider_id);
                                    read.current.provider_name = self
                                        .repository
                                        .get_provider(binding.provider_id)
                                        .await
                                        .ok()
                                        .map(|provider| provider.name);
                                } else {
                                    oauth_unmanaged = true;
                                    read.current.diagnostics.push(
                                        "Native OAuth identity differs from the active CLISwitch profile; the saved profile was not overwritten"
                                            .into(),
                                    );
                                }
                            }
                            Ok(None) => oauth_unmanaged = true,
                            Err(error) => {
                                oauth_unmanaged = true;
                                read.current
                                    .diagnostics
                                    .push(self.redactor.sanitize(error.to_string()));
                            }
                        }
                    }
                    let oauth_candidate = if oauth_unmanaged
                        && cli_id == CliId::Codex
                        && !read.current.externally_overridden
                    {
                        match paths.auth_file.as_deref() {
                            Some(auth_file) => match read_oauth_auth_file(auth_file).await {
                                Ok(bytes) => match adapter.validate_imported_auth(&bytes) {
                                    Ok(_) => Some(UnmanagedCandidateData::Oauth {
                                        kind: OAuthKind::Codex,
                                        auth_file: auth_file.to_path_buf(),
                                        digest: bytes_digest(&bytes),
                                    }),
                                    Err(error) => {
                                        read.current.diagnostics.push(self.redactor.sanitize(
                                            format!(
                                                "Codex OAuth auth file cannot be saved: {error}"
                                            ),
                                        ));
                                        None
                                    }
                                },
                                Err(error) => {
                                    read.current
                                        .diagnostics
                                        .push(self.redactor.sanitize(format!(
                                            "Codex OAuth auth file cannot be saved: {error}"
                                        )));
                                    None
                                }
                            },
                            None => None,
                        }
                    } else {
                        None
                    };
                    let status = if read.current.externally_overridden {
                        ScanStatus::ExternallyOverridden
                    } else if oauth_unmanaged || !unmatched_api_candidates.is_empty() {
                        ScanStatus::Unmanaged
                    } else if read.current.model.is_some()
                        || read.current.provider_name.is_some()
                        || read.current.auth_kind.is_some()
                    {
                        ScanStatus::Detected
                    } else {
                        ScanStatus::Installed
                    };
                    let mut candidate_data = unmatched_api_candidates
                        .into_iter()
                        .map(|candidate| {
                            (
                                candidate.source_provider_id,
                                candidate.suggested_name,
                                UnmanagedCandidateData::Api {
                                    template_id: candidate.template_id,
                                    connection: Box::new(candidate.connection),
                                    available_models: candidate.available_models,
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    if let Some(data) = oauth_candidate {
                        candidate_data.push(("codex".into(), "Codex OAuth".into(), data));
                    }
                    let mut provider_candidates = Vec::with_capacity(candidate_data.len());
                    for (source_provider_id, suggested_name, data) in candidate_data {
                        if let UnmanagedCandidateData::Api { connection, .. } = &data {
                            self.redactor.register(&connection.api_key);
                        }
                        let (protocol, endpoint, auth_type, available_models, default_model) =
                            match &data {
                                UnmanagedCandidateData::Api {
                                    connection,
                                    available_models,
                                    ..
                                } => (
                                    Some(connection.protocol),
                                    Some(connection.endpoint.clone()),
                                    Some(connection.auth_type),
                                    available_models.clone(),
                                    Some(connection.default_model.clone()),
                                ),
                                UnmanagedCandidateData::Oauth { .. } => {
                                    (None, None, None, Vec::new(), None)
                                }
                            };
                        let candidate_id = Uuid::new_v4();
                        let candidate_template_id = match &data {
                            UnmanagedCandidateData::Api { template_id, .. } => template_id.clone(),
                            UnmanagedCandidateData::Oauth { .. } => None,
                        };
                        let source_digests = match &data {
                            UnmanagedCandidateData::Api { .. } => read
                                .current
                                .sources
                                .iter()
                                .map(|source| (source.display_path.clone(), source.digest.clone()))
                                .collect(),
                            UnmanagedCandidateData::Oauth {
                                auth_file, digest, ..
                            } => [(auth_file.clone(), Some(digest.clone()))]
                                .into_iter()
                                .collect(),
                        };
                        self.candidates.write().await.insert(
                            candidate_id,
                            CandidateEntry {
                                candidate: UnmanagedCandidate {
                                    id: candidate_id,
                                    snapshot_id,
                                    cli_id,
                                    source_provider_id: source_provider_id.clone(),
                                    suggested_name: suggested_name.clone(),
                                    source_digests,
                                    data,
                                },
                                created_at: std::time::Instant::now(),
                            },
                        );
                        provider_candidates.push(DetectedProviderCandidate {
                            id: candidate_id,
                            source_provider_id,
                            suggested_name,
                            template_id: candidate_template_id,
                            protocol,
                            endpoint,
                            auth_type,
                            available_models,
                            default_model,
                        });
                    }
                    items.push(DetectedCli {
                        cli_id,
                        label: cli_id.label().into(),
                        status,
                        executable_path: Some(executable.path),
                        config_directory: paths.config_directory,
                        version: executable.version,
                        source: executable.source,
                        current: Some(read.current),
                        provider_candidates,
                    });
                }
                Err(error) => {
                    let status = match &error {
                        AppError::Serialization(_) => ScanStatus::InvalidConfig,
                        AppError::Unsupported(_) => ScanStatus::PartiallyDetected,
                        _ => ScanStatus::Unreadable,
                    };
                    items.push(DetectedCli {
                        cli_id,
                        label: cli_id.label().into(),
                        status,
                        executable_path: Some(executable.path),
                        config_directory: paths.config_directory.clone(),
                        version: executable.version,
                        source: executable.source,
                        current: Some(CurrentCliConfiguration {
                            provider_name: None,
                            protocol: None,
                            auth_kind: None,
                            model: None,
                            managed_provider_id: None,
                            sources: vec![SourceFileSnapshot {
                                source_id: format!("{cli_id}-config"),
                                display_path: paths.config_file,
                                digest: None,
                            }],
                            externally_overridden: false,
                            diagnostics: vec![self.redactor.sanitize(error.to_string())],
                        }),
                        provider_candidates: Vec::new(),
                    });
                }
            }
        }
        let snapshot = ScanSnapshot {
            id: snapshot_id,
            generated_at: Utc::now(),
            items,
        };
        *self.latest_snapshot.write().await = Some(snapshot.clone());
        snapshot
    }

    pub async fn save_unmanaged_candidate(
        &self,
        oauth: &OAuthService,
        settings: &AppSettings,
        request: UnmanagedCandidateSaveRequest,
    ) -> AppResult<ProviderProfile> {
        let UnmanagedCandidateSaveRequest {
            snapshot_id,
            candidate_id,
            name,
            default_model,
        } = request;
        if !settings.plaintext_risk_accepted {
            return Err(AppError::Blocked(
                "plaintext credential risk must be accepted before saving a secret".into(),
            ));
        }
        let entry = self
            .candidates
            .read()
            .await
            .get(&candidate_id)
            .cloned()
            .ok_or_else(|| AppError::Conflict("candidate expired; scan again".into()))?;
        if entry.created_at.elapsed() > CANDIDATE_TTL
            || entry.candidate.snapshot_id != snapshot_id
            || entry.candidate.id != candidate_id
        {
            return Err(AppError::Conflict(
                "candidate snapshot is no longer valid".into(),
            ));
        }
        for (path, expected) in &entry.candidate.source_digests {
            if &file_digest(path).await? != expected {
                return Err(AppError::Conflict(
                    "candidate source changed after the scan".into(),
                ));
            }
        }
        let provider = match &entry.candidate.data {
            UnmanagedCandidateData::Api {
                template_id,
                connection,
                available_models: _,
            } => {
                let selected_model = default_model
                    .as_deref()
                    .unwrap_or(&connection.default_model)
                    .trim();
                if selected_model.is_empty() {
                    return Err(AppError::Validation("selected model is required".into()));
                }
                let connections = match template_id.as_deref() {
                    Some(template_id) => {
                        materialize_template_connections(template_id, connection, selected_model)?
                    }
                    None => {
                        let mut connection = connection.as_ref().clone();
                        connection.template_endpoint_id = None;
                        connection.default_model = selected_model.to_string();
                        vec![connection]
                    }
                };
                let now = Utc::now();
                let mut provider = ProviderProfile {
                    id: Uuid::new_v4(),
                    name,
                    template_id: template_id.clone(),
                    revision: 1,
                    created_at: now,
                    updated_at: now,
                    data: ProviderData::Api(ApiProviderData { connections }),
                };
                normalize_provider_credential_kind(&mut provider);
                self.repository.insert_provider(&provider, None).await?;
                provider
            }
            UnmanagedCandidateData::Oauth {
                kind,
                auth_file,
                digest,
            } => {
                oauth
                    .save_active_auth_file(*kind, name, auth_file, digest, settings)
                    .await?
            }
        };
        self.candidates.write().await.remove(&candidate_id);
        Ok(provider)
    }

    async fn evict_expired_candidates(&self) {
        self.candidates
            .write()
            .await
            .retain(|_, entry| entry.created_at.elapsed() <= CANDIDATE_TTL);
    }

    async fn match_saved_connection(
        &self,
        candidate: &crate::domain::ProviderConnection,
    ) -> Option<Uuid> {
        for public in self.repository.list_providers().await.ok()? {
            let provider = self.repository.get_provider(public.id).await.ok()?;
            if let ProviderData::Api(api) = provider.data
                && api.connections.iter().any(|connection| {
                    connection.protocol == candidate.protocol
                        && connection.endpoint == candidate.endpoint
                        && connection.auth_type == candidate.auth_type
                        && connection.api_key == candidate.api_key
                })
            {
                return Some(provider.id);
            }
        }
        None
    }
}

fn materialize_template_connections(
    template_id: &str,
    detected: &crate::domain::ProviderConnection,
    selected_model: &str,
) -> AppResult<Vec<crate::domain::ProviderConnection>> {
    let template = embedded_catalog()?
        .api_template(template_id)
        .ok_or_else(|| AppError::Validation(format!("unknown provider template {template_id}")))?;
    materialize_template_connections_from_template(template_id, template, detected, selected_model)
}

fn materialize_template_connections_from_template(
    template_id: &str,
    template: &ApiProviderTemplate,
    detected: &crate::domain::ProviderConnection,
    selected_model: &str,
) -> AppResult<Vec<crate::domain::ProviderConnection>> {
    let detected_endpoint_id = detected.template_endpoint_id.as_deref().ok_or_else(|| {
        AppError::Validation("detected template provider has no endpoint identity".into())
    })?;
    let detected_endpoint = template
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == detected_endpoint_id)
        .ok_or_else(|| {
            AppError::Validation(format!(
                "template {template_id} has no endpoint {detected_endpoint_id}"
            ))
        })?;
    if detected.credential_slot_id != detected_endpoint.credential_slot_id {
        return Err(AppError::Validation(
            "detected credential slot does not match the provider template".into(),
        ));
    }

    template
        .endpoints
        .iter()
        .map(|endpoint| {
            if endpoint.credential_slot_id != detected.credential_slot_id {
                return Err(AppError::Validation(format!(
                    "saving template {template_id} requires another credential slot"
                )));
            }
            let is_detected = endpoint.id == detected_endpoint_id;
            let model = if is_detected
                || endpoint.models.is_empty()
                || endpoint
                    .models
                    .iter()
                    .any(|model| model.id == selected_model)
            {
                selected_model.to_string()
            } else {
                endpoint
                    .default_model()
                    .unwrap_or(selected_model)
                    .to_string()
            };
            Ok(crate::domain::ProviderConnection {
                id: if is_detected {
                    detected.id
                } else {
                    Uuid::new_v4()
                },
                template_endpoint_id: Some(endpoint.id.clone()),
                credential_slot_id: endpoint.credential_slot_id.clone(),
                protocol: endpoint.protocol,
                endpoint: if is_detected {
                    detected.endpoint.clone()
                } else {
                    endpoint.base_url.clone()
                },
                auth_type: if is_detected {
                    detected.auth_type
                } else {
                    endpoint.default_auth_type().ok_or_else(|| {
                        AppError::Validation(format!(
                            "template endpoint {} has no default auth mode",
                            endpoint.id
                        ))
                    })?
                },
                api_key: detected.api_key.clone(),
                default_model: model,
                verification: if is_detected {
                    detected.verification.clone()
                } else {
                    VerificationInfo::default()
                },
            })
        })
        .collect()
}

fn error_current(redactor: &Redactor, error: AppError) -> CurrentCliConfiguration {
    CurrentCliConfiguration {
        provider_name: None,
        protocol: None,
        auth_kind: None,
        model: None,
        managed_provider_id: None,
        sources: Vec::new(),
        externally_overridden: false,
        diagnostics: vec![redactor.sanitize(error.to_string())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            CliProtocol, ConnectionAuthType, ProviderConnection, VerificationInfo,
            VerificationStatus,
        },
        filesystem::private_paths::PrivatePaths,
        services::oauth::OAuthService,
    };

    struct CodexOAuthFixture {
        _temp: tempfile::TempDir,
        manager: CliManager,
        oauth: OAuthService,
        repository: Repository,
        settings: AppSettings,
        auth_file: PathBuf,
    }

    struct OpenCodeScanFixture {
        _temp: tempfile::TempDir,
        manager: CliManager,
        oauth: OAuthService,
        repository: Repository,
        settings: AppSettings,
        config_file: PathBuf,
        auth_file: PathBuf,
        managed_provider_id: Uuid,
        managed_source_provider_id: String,
    }

    #[test]
    fn glm_import_materializes_all_endpoints_with_one_shared_secret_and_manual_model() {
        let detected = ProviderConnection {
            id: Uuid::new_v4(),
            template_endpoint_id: Some("openai-chat".into()),
            credential_slot_id: "api-key".into(),
            protocol: CliProtocol::OpenaiChat,
            endpoint: url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap(),
            auth_type: ConnectionAuthType::Bearer,
            api_key: "shared-fixture-secret".into(),
            default_model: "glm-detected".into(),
            verification: VerificationInfo::default(),
        };

        let connections = materialize_template_connections(
            "glm-coding-plan",
            &detected,
            "manually-entered-model",
        )
        .unwrap();

        assert_eq!(connections.len(), 3);
        assert!(
            connections
                .iter()
                .all(|connection| connection.credential_slot_id == "api-key")
        );
        assert!(
            connections
                .iter()
                .all(|connection| connection.api_key == "shared-fixture-secret")
        );
        assert_eq!(
            connections
                .iter()
                .find(|connection| connection.template_endpoint_id.as_deref() == Some("openai-chat"))
                .map(|connection| connection.default_model.as_str()),
            Some("manually-entered-model")
        );
        for endpoint_id in ["anthropic", "openai-responses"] {
            assert_eq!(
                connections
                    .iter()
                    .find(|connection| {
                        connection.template_endpoint_id.as_deref() == Some(endpoint_id)
                    })
                    .map(|connection| connection.default_model.as_str()),
                Some("glm-4.7")
            );
        }
        assert_eq!(
            connections
                .iter()
                .filter_map(|connection| connection.template_endpoint_id.as_deref())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn template_import_rejects_an_endpoint_that_requires_another_credential_slot() {
        let mut template = embedded_catalog()
            .unwrap()
            .api_template("glm-coding-plan")
            .unwrap()
            .clone();
        template
            .credential_slots
            .push(crate::catalog::CredentialSlotTemplate {
                id: "secondary-key".into(),
                name: "Secondary API Key".into(),
            });
        template.endpoints[0].credential_slot_id = "secondary-key".into();
        let detected = ProviderConnection {
            id: Uuid::new_v4(),
            template_endpoint_id: Some("openai-chat".into()),
            credential_slot_id: "api-key".into(),
            protocol: CliProtocol::OpenaiChat,
            endpoint: url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap(),
            auth_type: ConnectionAuthType::Bearer,
            api_key: "shared-fixture-secret".into(),
            default_model: "glm-detected".into(),
            verification: VerificationInfo::default(),
        };

        let error = materialize_template_connections_from_template(
            "glm-coding-plan",
            &template,
            &detected,
            "glm-4.7",
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("requires another credential slot")
        );
    }

    async fn write_json_fixture(path: &std::path::Path, value: &serde_json::Value) {
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(path, serde_json::to_vec_pretty(value).unwrap())
            .await
            .unwrap();
    }

    async fn opencode_scan_fixture() -> OpenCodeScanFixture {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let registry = AdapterRegistry::default();
        let mut manager = CliManager::new(registry.clone(), repository.clone(), redactor.clone());
        manager.environment_override = Some(HostEnvironment {
            home: temp.path().to_path_buf(),
            variables: Default::default(),
            present_variables: Default::default(),
            os: std::env::consts::OS.into(),
        });
        let oauth = OAuthService::new(repository.clone(), paths, registry, redactor);

        let executable = temp.path().join("fixture-opencode");
        tokio::fs::write(&executable, b"#!/bin/sh\necho opencode 1.0\n")
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        let config_directory = temp.path().join("opencode");
        let config_file = config_directory.join("opencode.json");
        let auth_file = temp
            .path()
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json");
        let mut settings = AppSettings {
            plaintext_risk_accepted: true,
            ..AppSettings::default()
        };
        for location in &mut settings.manual_locations {
            location.executable_path = Some(temp.path().join("not-installed"));
            location.config_directory =
                Some(temp.path().join(format!("{}-config", location.cli_id)));
        }
        let opencode_location = settings
            .manual_locations
            .iter_mut()
            .find(|location| location.cli_id == CliId::Opencode)
            .unwrap();
        opencode_location.executable_path = Some(executable);
        opencode_location.config_directory = Some(config_directory);

        let now = Utc::now();
        let managed_provider_id = Uuid::new_v4();
        let managed_source_provider_id = format!("cliswitch_{managed_provider_id}");
        let managed_provider = ProviderProfile {
            id: managed_provider_id,
            name: "Managed OpenCode provider".into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiChat,
                    endpoint: url::Url::parse("https://managed.invalid/v1").unwrap(),
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: "managed-secret-key".into(),
                    default_model: "managed-model".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        };
        repository
            .insert_provider(&managed_provider, None)
            .await
            .unwrap();

        let mut providers = serde_json::Map::new();
        providers.insert(
            managed_source_provider_id.clone(),
            serde_json::json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "OpenCode managed source",
                "options": { "baseURL": "https://managed.invalid/v1" },
                "models": { "managed-model": {} }
            }),
        );
        let mut auth = serde_json::Map::new();
        auth.insert(
            managed_source_provider_id.clone(),
            serde_json::json!({ "type": "api", "key": "managed-secret-key" }),
        );
        write_json_fixture(
            &config_file,
            &serde_json::json!({
                "model": format!("{managed_source_provider_id}/managed-model"),
                "provider": providers
            }),
        )
        .await;
        write_json_fixture(&auth_file, &serde_json::Value::Object(auth)).await;

        OpenCodeScanFixture {
            _temp: temp,
            manager,
            oauth,
            repository,
            settings,
            config_file,
            auth_file,
            managed_provider_id,
            managed_source_provider_id,
        }
    }

    async fn codex_oauth_fixture(auth_content: &[u8]) -> CodexOAuthFixture {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let registry = AdapterRegistry::default();
        let manager = CliManager::new(registry.clone(), repository.clone(), redactor.clone());
        let oauth = OAuthService::new(repository.clone(), paths, registry, redactor);

        let executable = temp.path().join("fixture-cli");
        tokio::fs::write(&executable, b"#!/bin/sh\necho fixture-cli 1.0\n")
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        let codex_directory = temp.path().join("codex");
        tokio::fs::create_dir_all(&codex_directory).await.unwrap();
        tokio::fs::write(
            codex_directory.join("config.toml"),
            b"model = \"gpt-fixture\"\n",
        )
        .await
        .unwrap();
        let auth_file = codex_directory.join("auth.json");
        tokio::fs::write(&auth_file, auth_content).await.unwrap();

        let mut settings = AppSettings {
            plaintext_risk_accepted: true,
            ..AppSettings::default()
        };
        for location in &mut settings.manual_locations {
            location.executable_path = Some(temp.path().join("not-installed"));
            location.config_directory = Some(temp.path().join(location.cli_id.to_string()));
        }
        let codex_location = settings
            .manual_locations
            .iter_mut()
            .find(|location| location.cli_id == CliId::Codex)
            .unwrap();
        codex_location.executable_path = Some(executable);
        codex_location.config_directory = Some(codex_directory);

        CodexOAuthFixture {
            _temp: temp,
            manager,
            oauth,
            repository,
            settings,
            auth_file,
        }
    }

    fn codex_item(snapshot: &ScanSnapshot) -> &DetectedCli {
        snapshot
            .items
            .iter()
            .find(|item| item.cli_id == CliId::Codex)
            .unwrap()
    }

    fn opencode_item(snapshot: &ScanSnapshot) -> &DetectedCli {
        snapshot
            .items
            .iter()
            .find(|item| item.cli_id == CliId::Opencode)
            .unwrap()
    }

    #[tokio::test]
    async fn opencode_scan_reconciles_saved_unmanaged_and_oauth_providers() {
        let fixture = opencode_scan_fixture().await;

        let managed_scan = fixture.manager.scan(&fixture.settings).await;
        let managed = opencode_item(&managed_scan);
        assert_eq!(managed.status, ScanStatus::Detected);
        assert!(managed.provider_candidates.is_empty());
        let managed_current = managed.current.as_ref().unwrap();
        assert_eq!(
            managed_current.managed_provider_id,
            Some(fixture.managed_provider_id)
        );
        assert_eq!(
            managed_current.provider_name.as_deref(),
            Some("Managed OpenCode provider")
        );
        assert_eq!(managed_current.auth_kind.as_deref(), Some("api"));

        let mut providers = serde_json::Map::new();
        providers.insert(
            fixture.managed_source_provider_id.clone(),
            serde_json::json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "OpenCode managed source",
                "options": { "baseURL": "https://managed.invalid/v1" },
                "models": { "managed-model": {} }
            }),
        );
        providers.insert(
            "other-provider".into(),
            serde_json::json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "Other provider",
                "options": { "baseURL": "https://other.invalid/v1" },
                "models": { "other-model": {} }
            }),
        );
        let mut auth = serde_json::Map::new();
        auth.insert(
            fixture.managed_source_provider_id.clone(),
            serde_json::json!({ "type": "api", "key": "managed-secret-key" }),
        );
        auth.insert(
            "other-provider".into(),
            serde_json::json!({ "type": "api", "key": "other-secret-key" }),
        );
        write_json_fixture(
            &fixture.config_file,
            &serde_json::json!({
                "model": format!(
                    "{}/managed-model",
                    fixture.managed_source_provider_id
                ),
                "provider": providers
            }),
        )
        .await;
        write_json_fixture(&fixture.auth_file, &serde_json::Value::Object(auth)).await;

        let unmanaged_scan = fixture.manager.scan(&fixture.settings).await;
        let unmanaged = opencode_item(&unmanaged_scan);
        assert_eq!(unmanaged.status, ScanStatus::Unmanaged);
        assert_eq!(
            unmanaged
                .current
                .as_ref()
                .and_then(|current| current.managed_provider_id),
            Some(fixture.managed_provider_id)
        );
        assert_eq!(unmanaged.provider_candidates.len(), 1);
        let candidate = &unmanaged.provider_candidates[0];
        assert_eq!(candidate.source_provider_id, "other-provider");
        assert_eq!(candidate.default_model.as_deref(), Some("other-model"));

        let saved = fixture
            .manager
            .save_unmanaged_candidate(
                &fixture.oauth,
                &fixture.settings,
                UnmanagedCandidateSaveRequest {
                    snapshot_id: unmanaged_scan.id,
                    candidate_id: candidate.id,
                    name: "Saved other provider".into(),
                    default_model: None,
                },
            )
            .await
            .unwrap();
        let ProviderData::Api(saved_api) = saved.data else {
            panic!("OpenCode API candidate should save as an API provider");
        };
        assert_eq!(saved_api.connections[0].default_model, "other-model");

        let reconciled_scan = fixture.manager.scan(&fixture.settings).await;
        let reconciled = opencode_item(&reconciled_scan);
        assert_eq!(reconciled.status, ScanStatus::Detected);
        assert!(reconciled.provider_candidates.is_empty());
        assert_eq!(
            reconciled
                .current
                .as_ref()
                .and_then(|current| current.managed_provider_id),
            Some(fixture.managed_provider_id)
        );
        assert_eq!(fixture.repository.list_providers().await.unwrap().len(), 2);

        write_json_fixture(
            &fixture.config_file,
            &serde_json::json!({
                "model": "openai/oauth-model",
                "provider": {
                    "openai": { "models": { "oauth-model": {} } }
                }
            }),
        )
        .await;
        write_json_fixture(
            &fixture.auth_file,
            &serde_json::json!({
                "openai": {
                    "type": "oauth",
                    "access": "oauth-access-token",
                    "refresh": "oauth-refresh-token"
                }
            }),
        )
        .await;

        let oauth_scan = fixture.manager.scan(&fixture.settings).await;
        let opencode_oauth = opencode_item(&oauth_scan);
        assert_eq!(opencode_oauth.status, ScanStatus::Detected);
        assert!(opencode_oauth.provider_candidates.is_empty());
        let oauth_current = opencode_oauth.current.as_ref().unwrap();
        assert_eq!(oauth_current.auth_kind.as_deref(), Some("oauth"));
        assert_eq!(oauth_current.managed_provider_id, None);
        assert!(
            oauth_current
                .diagnostics
                .iter()
                .any(|message| message
                    .contains("OAuth providers are recognized but cannot be saved"))
        );
    }

    #[tokio::test]
    async fn opencode_minimax_token_plan_save_and_apply_reconcile_on_rescan() {
        let fixture = opencode_scan_fixture().await;
        write_json_fixture(
            &fixture.config_file,
            &serde_json::json!({
                "model": "minimax-coding-plan/MiniMax-M2.7",
                "provider": {
                    "minimax-coding-plan": {
                        "npm": "@ai-sdk/anthropic",
                        "name": "MiniMax Token Plan",
                        "options": { "baseURL": "https://api.minimax.io/anthropic/v1" },
                        "models": { "MiniMax-M2.7": {} }
                    }
                }
            }),
        )
        .await;
        write_json_fixture(
            &fixture.auth_file,
            &serde_json::json!({
                "minimax-coding-plan": {
                    "type": "api",
                    "key": "sk-cp-opencode-fixture"
                }
            }),
        )
        .await;

        let imported_scan = fixture.manager.scan(&fixture.settings).await;
        let imported = opencode_item(&imported_scan);
        assert_eq!(imported.status, ScanStatus::Unmanaged);
        assert_eq!(imported.provider_candidates.len(), 1);
        let candidate = &imported.provider_candidates[0];
        assert_eq!(
            candidate.template_id.as_deref(),
            Some("minimax-coding-plan")
        );
        assert_eq!(candidate.auth_type, Some(ConnectionAuthType::Bearer));

        let saved = fixture
            .manager
            .save_unmanaged_candidate(
                &fixture.oauth,
                &fixture.settings,
                UnmanagedCandidateSaveRequest {
                    snapshot_id: imported_scan.id,
                    candidate_id: candidate.id,
                    name: "Saved MiniMax Token Plan".into(),
                    default_model: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(saved.template_id.as_deref(), Some("minimax-coding-plan"));
        let ProviderData::Api(saved_api) = &saved.data else {
            panic!("MiniMax candidate should save as an API provider");
        };
        assert_eq!(
            saved_api.connections[0].auth_type,
            ConnectionAuthType::Bearer
        );

        let namespaced_id = format!("cliswitch_{}", saved.id);
        write_json_fixture(
            &fixture.config_file,
            &serde_json::json!({
                "model": format!("{namespaced_id}/MiniMax-M2.7"),
                "provider": {
                    (namespaced_id.clone()): {
                        "npm": "@ai-sdk/anthropic",
                        "name": "Saved MiniMax Token Plan",
                        "options": { "baseURL": "https://api.minimax.io/anthropic/v1" },
                        "models": { "MiniMax-M2.7": {} }
                    }
                }
            }),
        )
        .await;
        write_json_fixture(
            &fixture.auth_file,
            &serde_json::json!({
                (namespaced_id): {
                    "type": "api",
                    "key": "sk-cp-opencode-fixture"
                }
            }),
        )
        .await;

        let applied_scan = fixture.manager.scan(&fixture.settings).await;
        let applied = opencode_item(&applied_scan);
        assert_eq!(applied.status, ScanStatus::Detected);
        assert!(applied.provider_candidates.is_empty());
        let current = applied.current.as_ref().unwrap();
        assert_eq!(current.managed_provider_id, Some(saved.id));
        assert_eq!(
            current.provider_name.as_deref(),
            Some("Saved MiniMax Token Plan")
        );
    }

    #[tokio::test]
    async fn unmanaged_codex_oauth_can_be_saved_and_bound_without_changing_auth_file() {
        let auth_content =
            br#"{"tokens":{"account_id":"fixture-account","access_token":"fixture-token"}}"#;
        let fixture = codex_oauth_fixture(auth_content).await;
        let previous_content =
            br#"{"tokens":{"account_id":"previous-account","access_token":"previous-token"}}"#;
        let previous = fixture
            .oauth
            .import_bytes(
                OAuthKind::Codex,
                "Previous Codex OAuth".into(),
                previous_content.to_vec(),
                None,
            )
            .await
            .unwrap();
        fixture
            .repository
            .upsert_active_oauth_binding(
                CliId::Codex,
                previous.id,
                &bytes_digest(previous_content),
                Some("previous-account"),
            )
            .await
            .unwrap();
        let first_scan = fixture.manager.scan(&fixture.settings).await;
        let first_codex = codex_item(&first_scan);
        assert_eq!(first_codex.status, ScanStatus::Unmanaged);
        assert_eq!(
            first_codex
                .current
                .as_ref()
                .and_then(|current| current.auth_kind.as_deref()),
            Some("oauth")
        );
        let candidate_id = first_codex
            .provider_candidates
            .first()
            .map(|candidate| candidate.id)
            .expect("valid unmanaged Codex OAuth should be savable");

        let provider = fixture
            .manager
            .save_unmanaged_candidate(
                &fixture.oauth,
                &fixture.settings,
                UnmanagedCandidateSaveRequest {
                    snapshot_id: first_scan.id,
                    candidate_id,
                    name: "Codex OAuth".into(),
                    default_model: None,
                },
            )
            .await
            .unwrap();
        let ProviderData::Oauth(saved_oauth) = &provider.data else {
            panic!("saved candidate should be OAuth");
        };
        assert_eq!(saved_oauth.oauth_kind, OAuthKind::Codex);
        assert_eq!(saved_oauth.account_id.as_deref(), Some("fixture-account"));
        assert_eq!(
            saved_oauth.verification.status,
            VerificationStatus::NotOnlineVerified
        );
        assert!(!saved_oauth.manually_modified);
        assert_eq!(
            tokio::fs::read(&fixture.auth_file).await.unwrap(),
            auth_content
        );

        let binding = fixture
            .repository
            .get_active_oauth_binding(CliId::Codex)
            .await
            .unwrap()
            .expect("saving the active auth file should create a binding");
        assert_eq!(binding.provider_id, provider.id);
        assert_ne!(binding.provider_id, previous.id);
        assert_eq!(binding.native_digest, bytes_digest(auth_content));
        assert_eq!(binding.account_identity.as_deref(), Some("fixture-account"));
        assert_eq!(fixture.repository.list_providers().await.unwrap().len(), 2);

        let second_scan = fixture.manager.scan(&fixture.settings).await;
        let second_codex = codex_item(&second_scan);
        assert_eq!(second_codex.status, ScanStatus::Detected);
        assert!(second_codex.provider_candidates.is_empty());
        assert_eq!(
            second_codex
                .current
                .as_ref()
                .and_then(|current| current.managed_provider_id),
            Some(provider.id)
        );
    }

    #[tokio::test]
    async fn changed_codex_auth_file_invalidates_the_scanned_candidate() {
        let fixture = codex_oauth_fixture(
            br#"{"tokens":{"account_id":"fixture-account","access_token":"old-token"}}"#,
        )
        .await;
        let scan = fixture.manager.scan(&fixture.settings).await;
        let candidate_id = codex_item(&scan).provider_candidates[0].id;
        tokio::fs::write(
            &fixture.auth_file,
            br#"{"tokens":{"account_id":"fixture-account","access_token":"new-token"}}"#,
        )
        .await
        .unwrap();

        assert!(matches!(
            fixture
                .manager
                .save_unmanaged_candidate(
                    &fixture.oauth,
                    &fixture.settings,
                    UnmanagedCandidateSaveRequest {
                        snapshot_id: scan.id,
                        candidate_id,
                        name: "Codex OAuth".into(),
                        default_model: None,
                    },
                )
                .await,
            Err(AppError::Conflict(_))
        ));
        assert!(
            fixture
                .repository
                .list_providers()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn invalid_codex_auth_is_reported_but_not_exposed_as_a_savable_candidate() {
        let fixture = codex_oauth_fixture(br#"{"unexpected":true}"#).await;
        let scan = fixture.manager.scan(&fixture.settings).await;
        let codex = codex_item(&scan);
        assert_eq!(codex.status, ScanStatus::Unmanaged);
        assert!(codex.provider_candidates.is_empty());
        assert!(
            codex
                .current
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .any(|message| message.contains("cannot be saved"))
        );
    }

    #[tokio::test]
    async fn detected_api_provider_accepts_a_manually_entered_model() {
        let fixture = codex_oauth_fixture(br#"{}"#).await;
        let snapshot_id = Uuid::new_v4();
        let candidate_id = Uuid::new_v4();
        fixture.manager.candidates.write().await.insert(
            candidate_id,
            CandidateEntry {
                candidate: UnmanagedCandidate {
                    id: candidate_id,
                    snapshot_id,
                    cli_id: CliId::Opencode,
                    source_provider_id: "fixture-provider".into(),
                    suggested_name: "Fixture provider".into(),
                    source_digests: Default::default(),
                    data: UnmanagedCandidateData::Api {
                        template_id: None,
                        connection: Box::new(crate::domain::ProviderConnection {
                            id: Uuid::new_v4(),
                            template_endpoint_id: None,
                            credential_slot_id: "api-key".into(),
                            protocol: crate::domain::CliProtocol::AnthropicMessages,
                            endpoint: url::Url::parse("https://fixture.invalid/v1").unwrap(),
                            auth_type: crate::domain::ConnectionAuthType::ApiKey,
                            api_key: "fixture-secret-key".into(),
                            default_model: "model-a".into(),
                            verification: VerificationInfo::default(),
                        }),
                        available_models: vec!["model-a".into(), "model-b".into()],
                    },
                },
                created_at: std::time::Instant::now(),
            },
        );

        let saved = fixture
            .manager
            .save_unmanaged_candidate(
                &fixture.oauth,
                &fixture.settings,
                UnmanagedCandidateSaveRequest {
                    snapshot_id,
                    candidate_id,
                    name: "Fixture provider".into(),
                    default_model: Some("unadvertised-model".into()),
                },
            )
            .await
            .unwrap();
        let saved_id = saved.id;
        let ProviderData::Api(api) = saved.data else {
            panic!("detected API candidate should save as an API provider");
        };
        assert_eq!(api.connections[0].default_model, "unadvertised-model");
        assert_eq!(
            fixture
                .manager
                .match_saved_connection(&api.connections[0])
                .await,
            Some(saved_id)
        );
        let mut different_auth_type = api.connections[0].clone();
        different_auth_type.auth_type = ConnectionAuthType::Bearer;
        assert_eq!(
            fixture
                .manager
                .match_saved_connection(&different_auth_type)
                .await,
            None
        );
    }
}
