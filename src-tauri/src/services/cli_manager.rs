use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    adapters::{ClaudeCodeAdapter, CliAdapter, CodexAdapter, HostEnvironment, OpenCodeAdapter},
    domain::{
        ApiProviderData, AppSettings, CliId, CurrentCliConfiguration, DetectedCli, ProviderData,
        ProviderProfile, ScanSnapshot, ScanStatus, SourceFileSnapshot, UnmanagedCandidate,
    },
    error::{AppError, AppResult},
    filesystem::digest::file_digest,
    persistence::repository::Repository,
    services::{discovery::discover_executable, redaction::Redactor},
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
pub struct CliManager {
    registry: AdapterRegistry,
    repository: Repository,
    candidates: Arc<RwLock<HashMap<Uuid, CandidateEntry>>>,
    latest_snapshot: Arc<RwLock<Option<ScanSnapshot>>>,
    redactor: Redactor,
}

impl CliManager {
    pub fn new(registry: AdapterRegistry, repository: Repository, redactor: Redactor) -> Self {
        Self {
            registry,
            repository,
            candidates: Arc::new(RwLock::new(HashMap::new())),
            latest_snapshot: Arc::new(RwLock::new(None)),
            redactor,
        }
    }

    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    pub async fn latest_snapshot(&self) -> Option<ScanSnapshot> {
        self.latest_snapshot.read().await.clone()
    }

    pub async fn scan(&self, settings: &AppSettings) -> ScanSnapshot {
        let snapshot_id = Uuid::new_v4();
        let environment = match HostEnvironment::capture() {
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
                            candidate_id: None,
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
                        candidate_id: None,
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
                    candidate_id: None,
                });
                continue;
            };
            match adapter.read_current(&paths, &environment).await {
                Ok(mut read) => {
                    let mut oauth_unmanaged = false;
                    if let Some(candidate) = &read.unmanaged_candidate
                        && let Some(provider_id) = self.match_saved_connection(candidate).await
                    {
                        read.current.managed_provider_id = Some(provider_id);
                        read.current.provider_name = self
                            .repository
                            .get_provider(provider_id)
                            .await
                            .ok()
                            .map(|provider| provider.name);
                        read.unmanaged_candidate = None;
                    }
                    if read.current.auth_kind.as_deref() == Some("oauth") {
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
                    let status = if read.current.externally_overridden {
                        ScanStatus::ExternallyOverridden
                    } else if oauth_unmanaged
                        || (read.unmanaged_candidate.is_some()
                            && read.current.managed_provider_id.is_none())
                    {
                        ScanStatus::Unmanaged
                    } else if read.current.model.is_some()
                        || read.current.provider_name.is_some()
                        || read.current.auth_kind.is_some()
                    {
                        ScanStatus::Detected
                    } else {
                        ScanStatus::Installed
                    };
                    let candidate_id = if let Some(connection) = read.unmanaged_candidate {
                        self.redactor.register(&connection.api_key);
                        let candidate_id = Uuid::new_v4();
                        let source_digests = read
                            .current
                            .sources
                            .iter()
                            .map(|source| (source.display_path.clone(), source.digest.clone()))
                            .collect();
                        self.candidates.write().await.insert(
                            candidate_id,
                            CandidateEntry {
                                candidate: UnmanagedCandidate {
                                    id: candidate_id,
                                    snapshot_id,
                                    cli_id,
                                    source_digests,
                                    connection,
                                },
                                created_at: std::time::Instant::now(),
                            },
                        );
                        Some(candidate_id)
                    } else {
                        None
                    };
                    items.push(DetectedCli {
                        cli_id,
                        label: cli_id.label().into(),
                        status,
                        executable_path: Some(executable.path),
                        config_directory: paths.config_directory,
                        version: executable.version,
                        source: executable.source,
                        current: Some(read.current),
                        candidate_id,
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
                        candidate_id: None,
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
        settings: &AppSettings,
        snapshot_id: Uuid,
        candidate_id: Uuid,
        name: String,
        coding_plan: bool,
        coding_plan_name: Option<String>,
    ) -> AppResult<ProviderProfile> {
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
        let now = Utc::now();
        let provider = ProviderProfile {
            id: Uuid::new_v4(),
            name,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                coding_plan,
                coding_plan_name,
                connections: vec![entry.candidate.connection],
            }),
        };
        self.repository.insert_provider(&provider, None).await?;
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
                        && connection.api_key == candidate.api_key
                })
            {
                return Some(provider.id);
            }
        }
        None
    }
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
