use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    adapters::{ClaudeCodeAdapter, CliAdapter, CodexAdapter, HostEnvironment, OpenCodeAdapter},
    domain::{
        ApiProviderData, AppSettings, CliId, CurrentCliConfiguration, DetectedCli, OAuthKind,
        ProviderData, ProviderProfile, ScanSnapshot, ScanStatus, SourceFileSnapshot,
        UnmanagedCandidate, UnmanagedCandidateData,
    },
    error::{AppError, AppResult},
    filesystem::digest::{bytes_digest, file_digest},
    persistence::repository::Repository,
    services::{
        discovery::discover_executable,
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
    pub coding_plan: bool,
    pub coding_plan_name: Option<String>,
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
                    let candidate_data = read
                        .unmanaged_candidate
                        .map(UnmanagedCandidateData::Api)
                        .or(oauth_candidate);
                    let candidate_id = if let Some(data) = candidate_data {
                        if let UnmanagedCandidateData::Api(connection) = &data {
                            self.redactor.register(&connection.api_key);
                        }
                        let candidate_id = Uuid::new_v4();
                        let source_digests = match &data {
                            UnmanagedCandidateData::Api(_) => read
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
                                    source_digests,
                                    data,
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
        oauth: &OAuthService,
        settings: &AppSettings,
        request: UnmanagedCandidateSaveRequest,
    ) -> AppResult<ProviderProfile> {
        let UnmanagedCandidateSaveRequest {
            snapshot_id,
            candidate_id,
            name,
            coding_plan,
            coding_plan_name,
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
            UnmanagedCandidateData::Api(connection) => {
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
                        connections: vec![connection.clone()],
                    }),
                };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::VerificationStatus, filesystem::private_paths::PrivatePaths,
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
            .candidate_id
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
                    coding_plan: false,
                    coding_plan_name: None,
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
        assert_eq!(second_codex.candidate_id, None);
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
        let candidate_id = codex_item(&scan).candidate_id.unwrap();
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
                        coding_plan: false,
                        coding_plan_name: None,
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
        assert_eq!(codex.candidate_id, None);
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
}
