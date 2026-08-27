use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use crate::{
    adapters::HostEnvironment,
    catalog::runtime_catalog,
    domain::{
        ActiveOAuthBinding, AppSettings, OAuthKind, OAuthProviderData, ProviderData,
        ProviderProfile, VerificationInfo, VerificationStatus,
    },
    error::{AppError, AppResult},
    filesystem::{
        atomic_replace::{atomic_replace, resolve_target},
        digest::bytes_digest,
        private_paths::{
            PrivatePaths, set_private_directory_permissions, set_private_file_permissions,
        },
    },
    persistence::repository::Repository,
    process::{
        fixed_command::isolated_environment,
        pty::{PtyControl, SpawnedPty, spawn_fixed_pty},
    },
    services::{cli_manager::AdapterRegistry, discovery::discover_executable, redaction::Redactor},
};

const MAX_AUTH_BYTES: u64 = 1024 * 1024;
const SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn validate_oauth_browser_url(kind: OAuthKind, url: &url::Url) -> AppResult<()> {
    if url.scheme() != "https" {
        return Err(AppError::Blocked("OAuth browser URL must use HTTPS".into()));
    }
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::Blocked("OAuth browser URL has no host".into()))?;
    let roots: &[&str] = match kind {
        OAuthKind::Anthropic => &["claude.ai", "anthropic.com"],
        OAuthKind::Codex => &["openai.com", "chatgpt.com"],
    };
    if !roots
        .iter()
        .any(|root| host == *root || host.ends_with(&format!(".{root}")))
    {
        return Err(AppError::Blocked(
            "OAuth browser URL is outside the expected vendor domains".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OAuthStage {
    Starting,
    WaitingForBrowser,
    WaitingForConfirmation,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthSessionSnapshot {
    pub id: Uuid,
    pub kind: OAuthKind,
    pub stage: OAuthStage,
    pub message: String,
    pub provider_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProgressEvent {
    pub session_id: Uuid,
    pub stage: OAuthStage,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnershipMarker {
    application: String,
    session_id: Uuid,
    owner_pid: u32,
    child_pid: Option<u32>,
    child_start_time: Option<u64>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct OAuthSessionEntry {
    snapshot: OAuthSessionSnapshot,
    control: Option<PtyControl>,
}

struct OAuthPayload {
    kind: OAuthKind,
    name: String,
    raw_content: String,
    account_id: Option<String>,
    manually_modified: bool,
    status: VerificationStatus,
    replace_provider_id: Option<Uuid>,
    active_binding: Option<OAuthBindingPayload>,
}

struct OAuthBindingPayload {
    cli_id: crate::domain::CliId,
    native_digest: String,
    account_identity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthService {
    repository: Repository,
    paths: PrivatePaths,
    registry: AdapterRegistry,
    redactor: Redactor,
    sessions: Arc<RwLock<HashMap<Uuid, OAuthSessionEntry>>>,
    events: broadcast::Sender<OAuthProgressEvent>,
}

impl OAuthService {
    pub fn new(
        repository: Repository,
        paths: PrivatePaths,
        registry: AdapterRegistry,
        redactor: Redactor,
    ) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            repository,
            paths,
            registry,
            redactor,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OAuthProgressEvent> {
        self.events.subscribe()
    }

    pub async fn get_snapshot(&self, id: Uuid) -> AppResult<OAuthSessionSnapshot> {
        self.sessions
            .read()
            .await
            .get(&id)
            .map(|entry| entry.snapshot.clone())
            .ok_or_else(|| AppError::NotFound(format!("OAuth session {id}")))
    }

    pub async fn has_active_sessions(&self) -> bool {
        self.sessions
            .read()
            .await
            .values()
            .any(|entry| entry.control.is_some())
    }

    /// Refreshes an OAuth profile only when the active binding's stable account identity still
    /// matches the native CLI artifact. A digest change without an identity match is treated as
    /// an external login and deliberately leaves the saved profile untouched.
    pub async fn refresh_active_bindings(&self, settings: &AppSettings) -> AppResult<()> {
        let environment = HostEnvironment::capture()?;
        for binding in self.repository.list_active_oauth_bindings().await? {
            if let Err(error) = self
                .refresh_active_binding(settings, &environment, &binding)
                .await
            {
                tracing::warn!(
                    cli_id = %binding.cli_id,
                    provider_id = %binding.provider_id,
                    error = %self.redactor.sanitize(error.to_string()),
                    "failed to refresh one active OAuth binding; continuing"
                );
            }
        }
        Ok(())
    }

    async fn refresh_active_binding(
        &self,
        settings: &AppSettings,
        environment: &HostEnvironment,
        binding: &ActiveOAuthBinding,
    ) -> AppResult<()> {
        if cfg!(target_os = "macos") && binding.cli_id == crate::domain::CliId::ClaudeCode {
            return Ok(());
        }
        let adapter = self.registry.get(binding.cli_id);
        let manual = settings
            .manual_locations
            .iter()
            .find(|location| location.cli_id == binding.cli_id);
        let paths = adapter.resolve_paths(
            environment,
            manual.and_then(|location| location.config_directory.clone()),
        );
        let Some(auth_file) = paths.auth_file else {
            return Ok(());
        };
        let metadata = match tokio::fs::metadata(&auth_file).await {
            Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_AUTH_BYTES => metadata,
            _ => return Ok(()),
        };
        if metadata.len() > MAX_AUTH_BYTES {
            return Ok(());
        }
        let bytes = tokio::fs::read(&auth_file).await?;
        let digest = bytes_digest(&bytes);
        if digest == binding.native_digest {
            return Ok(());
        }
        let account = match adapter.validate_imported_auth(&bytes) {
            Ok(account) => account,
            Err(_) => return Ok(()),
        };
        let identity_matches = binding
            .account_identity
            .as_deref()
            .is_some_and(|expected| account.as_deref() == Some(expected));
        if !identity_matches {
            return Ok(());
        }
        let raw_content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(_) => return Ok(()),
        };
        let mut provider = self.repository.get_provider(binding.provider_id).await?;
        let expected_revision = provider.revision;
        let oauth = match &mut provider.data {
            ProviderData::Oauth(oauth) if oauth.oauth_kind.target_cli() == binding.cli_id => oauth,
            _ => return Ok(()),
        };
        self.redactor.register(&raw_content);
        oauth.raw_content = raw_content;
        oauth.digest = digest.clone();
        oauth.account_id = account.clone();
        oauth.manually_modified = false;
        oauth.verification = VerificationInfo {
            status: VerificationStatus::Valid,
            verified_at: Some(Utc::now()),
            error: None,
        };
        let relative = oauth_relative_path(provider.id);
        self.write_and_update_provider(&provider, expected_revision, &relative)
            .await?;
        self.repository
            .upsert_active_oauth_binding(
                binding.cli_id,
                binding.provider_id,
                &digest,
                account.as_deref(),
            )
            .await
    }

    pub async fn import_from_path(
        &self,
        kind: OAuthKind,
        name: String,
        path: &Path,
        settings: &AppSettings,
        replace_provider_id: Option<Uuid>,
    ) -> AppResult<ProviderProfile> {
        require_plaintext_ack(settings)?;
        let metadata = tokio::fs::symlink_metadata(path).await?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_AUTH_BYTES {
            return Err(AppError::Validation(
                "OAuth import must be a regular file no larger than 1 MiB".into(),
            ));
        }
        let bytes = tokio::fs::read(path).await?;
        self.import_bytes(kind, name, bytes, replace_provider_id)
            .await
    }

    pub async fn import_bytes(
        &self,
        kind: OAuthKind,
        name: String,
        bytes: Vec<u8>,
        replace_provider_id: Option<Uuid>,
    ) -> AppResult<ProviderProfile> {
        if bytes.len() as u64 > MAX_AUTH_BYTES {
            return Err(AppError::Validation("OAuth content exceeds 1 MiB".into()));
        }
        let adapter = self.registry.get(kind.target_cli());
        let account_id = adapter.validate_imported_auth(&bytes)?;
        let raw_content = String::from_utf8(bytes)
            .map_err(|_| AppError::Validation("OAuth auth content must be UTF-8 text".into()))?;
        let provider = self
            .save_oauth_payload(OAuthPayload {
                kind,
                name,
                raw_content,
                account_id,
                manually_modified: false,
                status: VerificationStatus::NotOnlineVerified,
                replace_provider_id,
                active_binding: None,
            })
            .await?;
        Ok(provider)
    }

    pub async fn create_from_raw(
        &self,
        kind: OAuthKind,
        name: String,
        raw_content: String,
        settings: &AppSettings,
    ) -> AppResult<ProviderProfile> {
        require_plaintext_ack(settings)?;
        self.import_bytes(kind, name, raw_content.into_bytes(), None)
            .await
    }

    pub async fn save_active_auth_file(
        &self,
        kind: OAuthKind,
        name: String,
        path: &Path,
        expected_digest: &str,
        settings: &AppSettings,
    ) -> AppResult<ProviderProfile> {
        require_plaintext_ack(settings)?;
        if kind != OAuthKind::Codex {
            return Err(AppError::Unsupported(
                "saving detected OAuth auth files is supported only for Codex".into(),
            ));
        }
        let bytes = read_oauth_auth_file(path).await?;
        let digest = bytes_digest(&bytes);
        if digest != expected_digest {
            return Err(AppError::Conflict(
                "OAuth auth file changed after the scan".into(),
            ));
        }
        let account_id = self
            .registry
            .get(kind.target_cli())
            .validate_imported_auth(&bytes)?;
        let raw_content = String::from_utf8(bytes)
            .map_err(|_| AppError::Validation("OAuth auth content must be UTF-8 text".into()))?;
        self.redactor.register(&raw_content);
        let binding = OAuthBindingPayload {
            cli_id: kind.target_cli(),
            native_digest: digest.clone(),
            account_identity: account_id.clone(),
        };
        if let Some(mut provider) = self
            .find_matching_oauth_provider(kind, &digest, account_id.as_deref(), None)
            .await?
        {
            let expected_revision = provider.revision;
            let now = Utc::now();
            let ProviderData::Oauth(oauth) = &mut provider.data else {
                unreachable!("OAuth match returned a non-OAuth provider");
            };
            oauth.raw_content = raw_content;
            oauth.digest = digest;
            oauth.account_id = account_id;
            oauth.manually_modified = false;
            oauth.verification = VerificationInfo {
                status: VerificationStatus::NotOnlineVerified,
                verified_at: None,
                error: None,
            };
            provider.updated_at = now;
            let relative = oauth_relative_path(provider.id);
            self.write_and_update_active_provider(
                &provider,
                expected_revision,
                &relative,
                &binding,
            )
            .await?;
            provider.revision = expected_revision + 1;
            return Ok(provider);
        }
        self.save_oauth_payload(OAuthPayload {
            kind,
            name,
            raw_content,
            account_id: account_id.clone(),
            manually_modified: false,
            status: VerificationStatus::NotOnlineVerified,
            replace_provider_id: None,
            active_binding: Some(binding),
        })
        .await
    }

    pub async fn update_raw_content(
        &self,
        provider_id: Uuid,
        expected_revision: i64,
        raw_content: String,
    ) -> AppResult<ProviderProfile> {
        let name = self.repository.get_provider(provider_id).await?.name;
        self.update_provider(provider_id, expected_revision, name, raw_content)
            .await
    }

    pub async fn update_provider(
        &self,
        provider_id: Uuid,
        expected_revision: i64,
        name: String,
        raw_content: String,
    ) -> AppResult<ProviderProfile> {
        let mut provider = self.repository.get_provider(provider_id).await?;
        if provider.revision != expected_revision {
            return Err(AppError::Conflict("OAuth provider was changed".into()));
        }
        let (kind, previous_account_id, previous_raw_content) = match &provider.data {
            ProviderData::Oauth(oauth) => (
                oauth.oauth_kind,
                oauth.account_id.clone(),
                oauth.raw_content.clone(),
            ),
            _ => return Err(AppError::Validation("provider is not OAuth".into())),
        };
        let normalized_name = name.trim().to_string();
        if raw_content == previous_raw_content {
            if normalized_name == provider.name.trim() {
                return Ok(provider);
            }
            provider.name = normalized_name;
            provider.updated_at = Utc::now();
            let relative = oauth_relative_path(provider.id);
            self.repository
                .update_provider(&provider, expected_revision, Some(&relative))
                .await?;
            provider.revision += 1;
            return Ok(provider);
        }
        if raw_content.len() as u64 > MAX_AUTH_BYTES {
            return Err(AppError::Validation("OAuth content exceeds 1 MiB".into()));
        }
        let account_id = self
            .registry
            .get(kind.target_cli())
            .validate_imported_auth(raw_content.as_bytes())?;
        let digest = bytes_digest(raw_content.as_bytes());
        if let Some(existing) = self
            .find_matching_oauth_provider(kind, &digest, account_id.as_deref(), Some(provider_id))
            .await?
        {
            return Err(AppError::Conflict(format!(
                "OAuth credentials already exist as provider '{}'; update that provider instead",
                existing.name
            )));
        }
        self.redactor.register(&raw_content);
        provider.name = normalized_name;
        provider.updated_at = Utc::now();
        let oauth = match &mut provider.data {
            ProviderData::Oauth(oauth) => oauth,
            _ => unreachable!("OAuth provider changed kind while being updated"),
        };
        oauth.raw_content = raw_content;
        oauth.digest = digest;
        if previous_account_id != account_id {
            oauth.account_label = None;
        }
        oauth.account_id = account_id;
        oauth.manually_modified = false;
        oauth.verification = VerificationInfo {
            status: VerificationStatus::NotOnlineVerified,
            verified_at: None,
            error: None,
        };
        let relative = oauth_relative_path(provider.id);
        self.write_and_update_provider(&provider, expected_revision, &relative)
            .await?;
        provider.revision += 1;
        Ok(provider)
    }

    pub async fn start_login(
        &self,
        kind: OAuthKind,
        name: String,
        settings: &AppSettings,
        replace_provider_id: Option<Uuid>,
        device_auth: bool,
    ) -> AppResult<OAuthSessionSnapshot> {
        require_plaintext_ack(settings)?;
        let environment = HostEnvironment::capture()?;
        let cli_id = kind.target_cli();
        let manual = settings
            .manual_locations
            .iter()
            .find(|item| item.cli_id == cli_id)
            .and_then(|item| item.executable_path.as_deref());
        let executable = discover_executable(cli_id, &environment, manual)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("{} is not installed", cli_id.label())))?;
        let adapter = self.registry.get(cli_id);
        let session_id = Uuid::new_v4();
        let session_directory = self.paths.oauth_tmp.join(session_id.to_string());
        tokio::fs::create_dir_all(&session_directory).await?;
        set_private_directory_permissions(&session_directory).await?;
        let started_at = Utc::now();
        let mut marker = OwnershipMarker {
            application: "io.github.laurentwu.cliswitch".into(),
            session_id,
            owner_pid: std::process::id(),
            child_pid: None,
            child_start_time: None,
            created_at: started_at,
        };
        if let Err(error) = write_marker(&session_directory, &marker).await {
            let _ = tokio::fs::remove_dir_all(&session_directory).await;
            return Err(error);
        }
        let setup = async {
            let mut fixed =
                adapter.fixed_oauth_command(executable.path, session_directory.clone())?;
            if kind == OAuthKind::Codex && device_auth {
                fixed.args = vec!["login".into(), "--device-auth".into()];
            }
            if kind == OAuthKind::Codex {
                let config = session_directory.join("config.toml");
                tokio::fs::write(&config, b"cli_auth_credentials_store = \"file\"\n").await?;
                set_private_file_permissions(&config).await?;
            }
            let environment = isolated_environment(&session_directory, &fixed.environment);
            let spawned = spawn_fixed_pty(
                fixed.executable.clone(),
                fixed.args.clone(),
                environment,
                session_directory.clone(),
                SESSION_TIMEOUT,
            )?;
            AppResult::Ok((fixed, spawned))
        }
        .await;
        let (fixed, spawned) = match setup {
            Ok(setup) => setup,
            Err(error) => {
                self.cleanup_owned_current_session(&session_directory, session_id)
                    .await;
                return Err(error);
            }
        };
        for _ in 0..20 {
            if let Some(pid) = spawned.control.process_id() {
                marker.child_pid = Some(pid);
                marker.child_start_time = process_start_time(pid);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if marker.child_pid.is_none() || marker.child_start_time.is_none() {
            self.abort_untracked_login(kind, started_at, session_directory, session_id, spawned)
                .await;
            return Err(AppError::Blocked(
                "OAuth child process identity was unavailable".into(),
            ));
        }
        if let Err(error) = write_marker(&session_directory, &marker).await {
            self.abort_untracked_login(kind, started_at, session_directory, session_id, spawned)
                .await;
            return Err(error);
        }
        let snapshot = OAuthSessionSnapshot {
            id: session_id,
            kind,
            stage: OAuthStage::Starting,
            message: "Official CLI login started".into(),
            provider_id: None,
            started_at,
            finished_at: None,
        };
        self.sessions.write().await.insert(
            session_id,
            OAuthSessionEntry {
                snapshot: snapshot.clone(),
                control: Some(spawned.control.clone()),
            },
        );
        self.emit(&snapshot);
        let service = self.clone();
        tokio::spawn(async move {
            service
                .drive_login(
                    session_id,
                    kind,
                    name,
                    replace_provider_id,
                    session_directory,
                    fixed.artifact,
                    spawned.output,
                    spawned.completion,
                )
                .await;
        });
        Ok(snapshot)
    }

    async fn abort_untracked_login(
        &self,
        kind: OAuthKind,
        started_at: DateTime<Utc>,
        session_directory: PathBuf,
        session_id: Uuid,
        mut spawned: SpawnedPty,
    ) {
        spawned.control.cancel();
        self.sessions.write().await.insert(
            session_id,
            OAuthSessionEntry {
                snapshot: OAuthSessionSnapshot {
                    id: session_id,
                    kind,
                    stage: OAuthStage::Failed,
                    message: "OAuth login startup failed; cancelling child process".into(),
                    provider_id: None,
                    started_at,
                    finished_at: None,
                },
                control: Some(spawned.control.clone()),
            },
        );
        let service = self.clone();
        tokio::spawn(async move {
            while spawned.output.recv().await.is_some() {}
            let _ = spawned.completion.await;
            service
                .cleanup_owned_current_session(&session_directory, session_id)
                .await;
            service.sessions.write().await.remove(&session_id);
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn drive_login(
        &self,
        session_id: Uuid,
        kind: OAuthKind,
        name: String,
        replace_provider_id: Option<Uuid>,
        session_directory: PathBuf,
        artifact: PathBuf,
        mut output: tokio::sync::mpsc::Receiver<String>,
        completion: tokio::sync::oneshot::Receiver<AppResult<i32>>,
    ) {
        let mut captured = String::new();
        while let Some(chunk) = output.recv().await {
            if captured.len() < MAX_AUTH_BYTES as usize {
                captured.push_str(utf8_prefix(
                    &chunk,
                    MAX_AUTH_BYTES as usize - captured.len(),
                ));
            }
            let lowercase = chunk.to_ascii_lowercase();
            let stage = if lowercase.contains("browser") || lowercase.contains("http") {
                OAuthStage::WaitingForBrowser
            } else {
                OAuthStage::WaitingForConfirmation
            };
            self.update_session(
                session_id,
                stage,
                self.redactor.sanitize(strip_control_codes(&chunk)),
                None,
                false,
            )
            .await;
        }
        let completion = completion
            .await
            .unwrap_or_else(|_| Err(AppError::Io(std::io::Error::other("OAuth worker stopped"))));
        match completion {
            Ok(0) => {
                let is_setup_token = kind == OAuthKind::Anthropic && cfg!(target_os = "macos");
                let raw = if artifact.exists() {
                    read_login_artifact(&artifact).await
                } else if is_setup_token {
                    extract_setup_token(&captured).ok_or_else(|| {
                        AppError::Validation("setup-token output was not recognized".into())
                    })
                } else {
                    Err(AppError::NotFound("OAuth artifact was not created".into()))
                };
                match raw {
                    Ok(raw_content) => {
                        let account = if is_setup_token {
                            Ok(None)
                        } else {
                            self.registry
                                .get(kind.target_cli())
                                .validate_imported_auth(raw_content.as_bytes())
                        };
                        match account {
                            Ok(account_id) => {
                                match self
                                    .save_oauth_payload(OAuthPayload {
                                        kind,
                                        name,
                                        raw_content,
                                        account_id,
                                        manually_modified: false,
                                        status: VerificationStatus::Valid,
                                        replace_provider_id,
                                        active_binding: None,
                                    })
                                    .await
                                {
                                    Ok(provider) => {
                                        self.update_session(
                                            session_id,
                                            OAuthStage::Success,
                                            "Official CLI login completed".into(),
                                            Some(provider.id),
                                            true,
                                        )
                                        .await;
                                    }
                                    Err(error) => {
                                        self.fail_session(session_id, error).await;
                                    }
                                }
                            }
                            Err(error) => self.fail_session(session_id, error).await,
                        }
                    }
                    Err(error) => self.fail_session(session_id, error).await,
                }
            }
            Ok(code) => {
                self.fail_session(
                    session_id,
                    AppError::Blocked(format!("official CLI exited with status {code}")),
                )
                .await;
            }
            Err(AppError::Cancelled) => {
                self.update_session(
                    session_id,
                    OAuthStage::Cancelled,
                    "OAuth login cancelled".into(),
                    None,
                    true,
                )
                .await;
            }
            Err(error) => self.fail_session(session_id, error).await,
        }
        self.cleanup_owned_current_session(&session_directory, session_id)
            .await;
    }

    pub async fn send_input(&self, id: Uuid, input: String) -> AppResult<()> {
        let control = self
            .sessions
            .read()
            .await
            .get(&id)
            .and_then(|entry| entry.control.clone())
            .ok_or_else(|| AppError::NotFound("active OAuth session not found".into()))?;
        control.send_line(input).await
    }

    pub async fn cancel(&self, id: Uuid) -> AppResult<()> {
        let control = self
            .sessions
            .read()
            .await
            .get(&id)
            .and_then(|entry| entry.control.clone())
            .ok_or_else(|| AppError::NotFound("active OAuth session not found".into()))?;
        control.cancel();
        Ok(())
    }

    pub async fn shutdown(&self) {
        let controls = self
            .sessions
            .read()
            .await
            .values()
            .filter_map(|entry| entry.control.clone())
            .collect::<Vec<_>>();
        for control in controls {
            control.cancel();
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .sessions
                .read()
                .await
                .values()
                .all(|entry| entry.control.is_none())
                || tokio::time::Instant::now() >= deadline
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn cleanup_stale_sessions(&self) -> AppResult<()> {
        let root = tokio::fs::canonicalize(&self.paths.oauth_tmp).await?;
        let mut entries = tokio::fs::read_dir(&root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let child = match tokio::fs::canonicalize(entry.path()).await {
                Ok(child) if child.parent() == Some(root.as_path()) => child,
                _ => continue,
            };
            let marker = match read_marker(&child).await {
                Ok(marker)
                    if marker.application == "io.github.laurentwu.cliswitch"
                        && child.file_name().and_then(|name| name.to_str())
                            == Some(marker.session_id.to_string().as_str()) =>
                {
                    marker
                }
                _ => continue,
            };
            let Some(child_pid) = marker.child_pid else {
                // A crash could have occurred between process creation and the marker update.
                // Without a recorded child identity, preserving the directory is safer.
                continue;
            };
            match process_identity_alive(child_pid, marker.child_start_time) {
                Some(false) => {}
                Some(true) | None => continue,
            }
            tokio::fs::remove_dir_all(&child).await?;
        }
        Ok(())
    }

    async fn save_oauth_payload(&self, payload: OAuthPayload) -> AppResult<ProviderProfile> {
        let OAuthPayload {
            kind,
            name,
            raw_content,
            account_id,
            manually_modified,
            status,
            replace_provider_id,
            active_binding,
        } = payload;
        if replace_provider_id.is_some() && active_binding.is_some() {
            return Err(AppError::Validation(
                "replacing an OAuth provider cannot create an active binding".into(),
            ));
        }
        self.redactor.register(&raw_content);
        let now = Utc::now();
        let digest = bytes_digest(raw_content.as_bytes());
        if let Some(existing) = self
            .find_matching_oauth_provider(kind, &digest, account_id.as_deref(), replace_provider_id)
            .await?
        {
            return Err(AppError::Conflict(format!(
                "OAuth credentials already exist as provider '{}'; update that provider instead",
                existing.name
            )));
        }
        let id = replace_provider_id.unwrap_or_else(Uuid::new_v4);
        let expected_revision = if let Some(id) = replace_provider_id {
            let previous = self.repository.get_provider(id).await?;
            match &previous.data {
                ProviderData::Oauth(oauth) if oauth.oauth_kind == kind => previous.revision,
                _ => {
                    return Err(AppError::Validation(
                        "replacement provider has a different OAuth kind".into(),
                    ));
                }
            }
        } else {
            0
        };
        let mut provider = ProviderProfile {
            id,
            name,
            template_id: runtime_catalog()?
                .auth_template_for_kind(kind)
                .map(|template| template.id.clone()),
            revision: expected_revision.max(1),
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: kind,
                account_id,
                account_label: None,
                digest,
                raw_content,
                manually_modified,
                verification: VerificationInfo {
                    status,
                    verified_at: (status == VerificationStatus::Valid).then_some(now),
                    error: None,
                },
            }),
        };
        let relative = oauth_relative_path(id);
        if replace_provider_id.is_some() {
            self.write_and_update_provider(&provider, expected_revision, &relative)
                .await?;
            provider.revision = expected_revision + 1;
        } else {
            let profile_directory = self.paths.auth_profile_dir(id).await?;
            let path = profile_directory.join("auth.txt");
            let target = resolve_target(&path, &profile_directory).await?;
            atomic_replace(&target, oauth_raw(&provider)?.as_bytes()).await?;
            let inserted = if let Some(binding) = active_binding {
                self.repository
                    .insert_provider_with_active_oauth_binding(
                        &provider,
                        &relative,
                        binding.cli_id,
                        &binding.native_digest,
                        binding.account_identity.as_deref(),
                    )
                    .await
            } else {
                self.repository
                    .insert_provider(&provider, Some(&relative))
                    .await
            };
            if let Err(error) = inserted {
                let _ = tokio::fs::remove_dir_all(profile_directory).await;
                return Err(error);
            }
        }
        Ok(provider)
    }

    async fn find_matching_oauth_provider(
        &self,
        kind: OAuthKind,
        digest: &str,
        account_id: Option<&str>,
        exclude_provider_id: Option<Uuid>,
    ) -> AppResult<Option<ProviderProfile>> {
        for existing in self.repository.list_providers().await? {
            if Some(existing.id) == exclude_provider_id || existing.oauth_kind != Some(kind) {
                continue;
            }
            let profile = self.repository.get_provider(existing.id).await?;
            let ProviderData::Oauth(existing_oauth) = &profile.data else {
                continue;
            };
            let same_account = account_id
                .zip(existing_oauth.account_id.as_deref())
                .is_some_and(|(current, saved)| current == saved);
            if existing_oauth.digest == digest || same_account {
                return Ok(Some(profile));
            }
        }
        Ok(None)
    }

    async fn write_and_update_provider(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        relative: &Path,
    ) -> AppResult<()> {
        self.write_and_update_provider_transaction(provider, expected_revision, relative, None)
            .await
    }

    async fn write_and_update_active_provider(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        relative: &Path,
        binding: &OAuthBindingPayload,
    ) -> AppResult<()> {
        self.write_and_update_provider_transaction(
            provider,
            expected_revision,
            relative,
            Some(binding),
        )
        .await
    }

    async fn write_and_update_provider_transaction(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        relative: &Path,
        binding: Option<&OAuthBindingPayload>,
    ) -> AppResult<()> {
        let profile_directory = self.paths.auth_profile_dir(provider.id).await?;
        let path = profile_directory.join("auth.txt");
        let previous = tokio::fs::read(&path).await.ok();
        let target = resolve_target(&path, &profile_directory).await?;
        atomic_replace(&target, oauth_raw(provider)?.as_bytes()).await?;
        let updated = if let Some(binding) = binding {
            self.repository
                .update_provider_with_active_oauth_binding(
                    provider,
                    expected_revision,
                    relative,
                    binding.cli_id,
                    &binding.native_digest,
                    binding.account_identity.as_deref(),
                )
                .await
        } else {
            self.repository
                .update_provider(provider, expected_revision, Some(relative))
                .await
        };
        if let Err(error) = updated {
            if let Some(previous) = previous {
                let _ = atomic_replace(&target, &previous).await;
            }
            return Err(error);
        }
        Ok(())
    }

    async fn update_session(
        &self,
        id: Uuid,
        stage: OAuthStage,
        message: String,
        provider_id: Option<Uuid>,
        finished: bool,
    ) {
        let message = self.redactor.sanitize(message);
        let mut sessions = self.sessions.write().await;
        if let Some(entry) = sessions.get_mut(&id) {
            entry.snapshot.stage = stage;
            entry.snapshot.message = message;
            entry.snapshot.provider_id = provider_id;
            if finished {
                entry.snapshot.finished_at = Some(Utc::now());
                entry.control = None;
            }
            self.emit(&entry.snapshot);
        }
    }

    async fn fail_session(&self, id: Uuid, error: AppError) {
        self.update_session(
            id,
            OAuthStage::Failed,
            self.redactor.sanitize(error.to_string()),
            None,
            true,
        )
        .await;
    }

    fn emit(&self, snapshot: &OAuthSessionSnapshot) {
        let _ = self.events.send(OAuthProgressEvent {
            session_id: snapshot.id,
            stage: snapshot.stage,
            message: self.redactor.sanitize(&snapshot.message),
        });
    }

    async fn cleanup_owned_current_session(&self, path: &Path, id: Uuid) {
        let Ok(root) = tokio::fs::canonicalize(&self.paths.oauth_tmp).await else {
            return;
        };
        let Ok(path) = tokio::fs::canonicalize(path).await else {
            return;
        };
        let Ok(marker) = read_marker(&path).await else {
            return;
        };
        if path.parent() == Some(root.as_path())
            && marker.session_id == id
            && marker.owner_pid == std::process::id()
        {
            let _ = tokio::fs::remove_dir_all(path).await;
        }
    }
}

fn require_plaintext_ack(settings: &AppSettings) -> AppResult<()> {
    if settings.plaintext_risk_accepted {
        Ok(())
    } else {
        Err(AppError::Blocked(
            "plaintext credential risk must be accepted before saving a secret".into(),
        ))
    }
}

fn oauth_relative_path(id: Uuid) -> PathBuf {
    PathBuf::from("auth").join(id.to_string()).join("auth.txt")
}

fn oauth_raw(provider: &ProviderProfile) -> AppResult<&str> {
    match &provider.data {
        ProviderData::Oauth(oauth) => Ok(&oauth.raw_content),
        _ => Err(AppError::Validation("provider is not OAuth".into())),
    }
}

pub(crate) async fn read_oauth_auth_file(path: &Path) -> AppResult<Vec<u8>> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_AUTH_BYTES {
        return Err(AppError::Validation(
            "OAuth auth file must be a regular file no larger than 1 MiB".into(),
        ));
    }
    let bytes = tokio::fs::read(path).await?;
    if bytes.len() as u64 > MAX_AUTH_BYTES {
        return Err(AppError::Validation("OAuth auth file exceeds 1 MiB".into()));
    }
    Ok(bytes)
}

async fn write_marker(directory: &Path, marker: &OwnershipMarker) -> AppResult<()> {
    let path = directory.join(".cliswitch-owner.json");
    let target = resolve_target(&path, directory).await?;
    atomic_replace(&target, &serde_json::to_vec(marker)?).await
}

async fn read_marker(directory: &Path) -> AppResult<OwnershipMarker> {
    let bytes = tokio::fs::read(directory.join(".cliswitch-owner.json")).await?;
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn process_start_time(pid: u32) -> Option<u64> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    system
        .process(Pid::from_u32(pid))
        .map(|process| process.start_time())
}

fn process_identity_alive(pid: u32, start_time: Option<u64>) -> Option<bool> {
    let expected = start_time?;
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    Some(
        system
            .process(Pid::from_u32(pid))
            .is_some_and(|process| expected == process.start_time()),
    )
}

fn strip_control_codes(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            *character == '\n'
                || *character == '\r'
                || *character == '\t'
                || !character.is_control()
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(500)
        .collect()
}

fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

async fn read_login_artifact(path: &Path) -> AppResult<String> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_AUTH_BYTES {
        return Err(AppError::Validation(
            "OAuth artifact must be a regular UTF-8 file no larger than 1 MiB".into(),
        ));
    }
    let bytes = tokio::fs::read(path).await?;
    if bytes.len() as u64 > MAX_AUTH_BYTES {
        return Err(AppError::Validation("OAuth artifact exceeds 1 MiB".into()));
    }
    String::from_utf8(bytes)
        .map_err(|_| AppError::Validation("OAuth artifact must be UTF-8 text".into()))
}

fn extract_setup_token(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .filter_map(|value| {
            let value = value.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && !"-_".contains(character)
            });
            (value.len() > 20 && value.starts_with("sk-ant-")).then(|| value.to_string())
        })
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{persistence::repository::Repository, services::cli_manager::AdapterRegistry};

    #[test]
    fn control_output_is_bounded_and_cleaned() {
        let output = strip_control_codes("hello\u{0} world\n");
        assert_eq!(output, "hello world");
    }

    #[test]
    fn setup_token_extraction_is_local_only() {
        assert_eq!(
            extract_setup_token("token: sk-ant-abcdefghijklmnopqrstuvwxyz"),
            Some("sk-ant-abcdefghijklmnopqrstuvwxyz".into())
        );
        assert_eq!(
            extract_setup_token(
                "ignore sk-other-secret-value first sk-ant-old-old-old-old then sk-ant-new-new-new-new"
            ),
            Some("sk-ant-new-new-new-new".into())
        );
        assert_eq!(extract_setup_token("sk-generic-generic-generic"), None);
    }

    #[test]
    fn bounded_oauth_capture_never_splits_utf8() {
        assert_eq!(utf8_prefix("配置", 4), "配");
        assert_eq!(utf8_prefix("配置", 2), "");
        assert_eq!(utf8_prefix("abc", 3), "abc");
    }

    #[test]
    fn oauth_browser_urls_are_https_and_vendor_scoped() {
        assert!(
            validate_oauth_browser_url(
                OAuthKind::Codex,
                &url::Url::parse("https://auth.openai.com/codex/device").unwrap()
            )
            .is_ok()
        );
        assert!(
            validate_oauth_browser_url(
                OAuthKind::Anthropic,
                &url::Url::parse("https://claude.ai/oauth/authorize").unwrap()
            )
            .is_ok()
        );
        assert!(
            validate_oauth_browser_url(
                OAuthKind::Codex,
                &url::Url::parse("https://openai.com.attacker.test/login").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_oauth_browser_url(
                OAuthKind::Anthropic,
                &url::Url::parse("http://claude.ai/oauth/authorize").unwrap()
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn duplicate_oauth_account_or_digest_requires_updating_existing_profile() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(repository, paths, AdapterRegistry::default(), redactor);
        let payload =
            br#"{"tokens":{"account_id":"account-a","access_token":"fixture-token"}}"#.to_vec();
        service
            .import_bytes(OAuthKind::Codex, "First".into(), payload.clone(), None)
            .await
            .unwrap();
        assert!(matches!(
            service
                .import_bytes(OAuthKind::Codex, "Second".into(), payload, None)
                .await,
            Err(AppError::Conflict(_))
        ));
        assert!(matches!(
            service
                .import_bytes(
                    OAuthKind::Codex,
                    "Same account".into(),
                    br#"{"tokens":{"account_id":"account-a","access_token":"different-token"}}"#
                        .to_vec(),
                    None,
                )
                .await,
            Err(AppError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn saving_active_auth_reuses_and_refreshes_a_matching_provider() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths,
            AdapterRegistry::default(),
            redactor,
        );
        let original =
            br#"{"tokens":{"account_id":"account-a","access_token":"old-token"}}"#.to_vec();
        let existing = service
            .import_bytes(OAuthKind::Codex, "Existing".into(), original, None)
            .await
            .unwrap();
        let refreshed =
            br#"{"tokens":{"account_id":"account-a","access_token":"new-token"}}"#.to_vec();
        let digest = bytes_digest(&refreshed);
        let native_auth = temp.path().join("auth.json");
        tokio::fs::write(&native_auth, &refreshed).await.unwrap();
        let settings = AppSettings {
            plaintext_risk_accepted: true,
            ..AppSettings::default()
        };

        let saved = service
            .save_active_auth_file(
                OAuthKind::Codex,
                "Ignored duplicate name".into(),
                &native_auth,
                &digest,
                &settings,
            )
            .await
            .unwrap();
        assert_eq!(saved.id, existing.id);
        assert_eq!(saved.name, "Existing");
        assert_eq!(saved.revision, existing.revision + 1);
        let ProviderData::Oauth(saved_oauth) = &saved.data else {
            unreachable!();
        };
        assert_eq!(saved_oauth.raw_content.as_bytes(), refreshed);
        assert_eq!(saved_oauth.digest, digest);
        assert_eq!(
            saved_oauth.verification.status,
            VerificationStatus::NotOnlineVerified
        );

        let saved_again = service
            .save_active_auth_file(
                OAuthKind::Codex,
                "Another ignored name".into(),
                &native_auth,
                &digest,
                &settings,
            )
            .await
            .unwrap();
        assert_eq!(saved_again.id, existing.id);
        assert_eq!(repository.list_providers().await.unwrap().len(), 1);
        let binding = repository
            .get_active_oauth_binding(crate::domain::CliId::Codex)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(binding.provider_id, existing.id);
        assert_eq!(binding.native_digest, digest);
        assert_eq!(binding.account_identity.as_deref(), Some("account-a"));
    }

    #[tokio::test]
    async fn saving_active_auth_enforces_plaintext_ack_and_codex_scope() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths,
            AdapterRegistry::default(),
            redactor,
        );
        let content = br#"{"tokens":{"account_id":"account-a","access_token":"fixture-token"}}"#;
        let digest = bytes_digest(content);
        let native_auth = temp.path().join("auth.json");
        tokio::fs::write(&native_auth, content).await.unwrap();

        assert!(matches!(
            service
                .save_active_auth_file(
                    OAuthKind::Codex,
                    "Blocked".into(),
                    &native_auth,
                    &digest,
                    &AppSettings::default(),
                )
                .await,
            Err(AppError::Blocked(_))
        ));
        let accepted = AppSettings {
            plaintext_risk_accepted: true,
            ..AppSettings::default()
        };
        assert!(matches!(
            service
                .save_active_auth_file(
                    OAuthKind::Anthropic,
                    "Unsupported".into(),
                    &native_auth,
                    &digest,
                    &accepted,
                )
                .await,
            Err(AppError::Unsupported(_))
        ));
        assert!(repository.list_providers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn replacement_and_strict_editor_update_exclude_the_current_profile() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths,
            AdapterRegistry::default(),
            redactor.clone(),
        );
        let first = service
            .import_bytes(
                OAuthKind::Codex,
                "Personal".into(),
                br#"{"tokens":{"account_id":"account-a","access_token":"old-token"}}"#.to_vec(),
                None,
            )
            .await
            .unwrap();
        let replaced = service
            .import_bytes(
                OAuthKind::Codex,
                "Personal".into(),
                br#"{"tokens":{"account_id":"account-a","access_token":"new-token"}}"#.to_vec(),
                Some(first.id),
            )
            .await
            .unwrap();
        assert_eq!(replaced.revision, 2);

        let raw = r#"{"tokens":{"account_id":"account-a","access_token":"editor-token"}}"#;
        let edited = service
            .update_provider(first.id, replaced.revision, "Renamed".into(), raw.into())
            .await
            .unwrap();
        assert_eq!(edited.revision, 3);
        assert_eq!(edited.name, "Renamed");
        let reloaded = repository.get_provider(first.id).await.unwrap();
        let ProviderData::Oauth(oauth) = reloaded.data else {
            unreachable!();
        };
        assert_eq!(oauth.raw_content, raw);
        assert!(!oauth.manually_modified);
        assert_eq!(
            oauth.verification.status,
            VerificationStatus::NotOnlineVerified
        );
        assert!(!redactor.sanitize(raw).contains("editor-token"));
    }

    #[tokio::test]
    async fn direct_raw_creation_requires_ack_and_validates_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths.clone(),
            AdapterRegistry::default(),
            redactor,
        );
        let raw = r#"{"tokens":{"account_id":"account-a","access_token":"fixture-token"}}"#;

        assert!(matches!(
            service
                .create_from_raw(
                    OAuthKind::Codex,
                    "Blocked".into(),
                    raw.into(),
                    &AppSettings::default(),
                )
                .await,
            Err(AppError::Blocked(_))
        ));
        let accepted = AppSettings {
            plaintext_risk_accepted: true,
            ..AppSettings::default()
        };
        assert!(matches!(
            service
                .create_from_raw(
                    OAuthKind::Codex,
                    "Invalid".into(),
                    "not-json".into(),
                    &accepted,
                )
                .await,
            Err(AppError::Validation(_))
        ));
        assert!(repository.list_providers().await.unwrap().is_empty());
        assert!(
            tokio::fs::read_dir(&paths.auth)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );

        let created = service
            .create_from_raw(OAuthKind::Codex, "Created".into(), raw.into(), &accepted)
            .await
            .unwrap();
        assert_eq!(created.name, "Created");
        assert_eq!(repository.list_providers().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invalid_duplicate_or_name_conflicting_update_keeps_file_and_profile_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths.clone(),
            AdapterRegistry::default(),
            redactor,
        );
        let first_raw = r#"{"tokens":{"account_id":"account-a","access_token":"first-token"}}"#;
        let second_raw = r#"{"tokens":{"account_id":"account-b","access_token":"second-token"}}"#;
        let first = service
            .import_bytes(
                OAuthKind::Codex,
                "First".into(),
                first_raw.as_bytes().to_vec(),
                None,
            )
            .await
            .unwrap();
        let second = service
            .import_bytes(
                OAuthKind::Codex,
                "Second".into(),
                second_raw.as_bytes().to_vec(),
                None,
            )
            .await
            .unwrap();
        let second_path = paths.auth.join(second.id.to_string()).join("auth.txt");
        let original_file = tokio::fs::read(&second_path).await.unwrap();

        assert!(matches!(
            service
                .update_provider(
                    second.id,
                    second.revision,
                    "Invalid".into(),
                    "not-json".into()
                )
                .await,
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            service
                .update_provider(
                    second.id,
                    second.revision,
                    "Duplicate".into(),
                    first_raw.into(),
                )
                .await,
            Err(AppError::Conflict(_))
        ));
        let changed_second_raw =
            r#"{"tokens":{"account_id":"account-b","access_token":"changed-token"}}"#;
        assert!(matches!(
            service
                .update_provider(
                    second.id,
                    second.revision,
                    first.name.clone(),
                    changed_second_raw.into(),
                )
                .await,
            Err(AppError::Conflict(_))
        ));

        assert_eq!(tokio::fs::read(&second_path).await.unwrap(), original_file);
        let unchanged = repository.get_provider(second.id).await.unwrap();
        assert_eq!(unchanged.name, "Second");
        assert_eq!(unchanged.revision, second.revision);
        let ProviderData::Oauth(oauth) = unchanged.data else {
            unreachable!();
        };
        assert_eq!(oauth.raw_content, second_raw);
    }

    #[tokio::test]
    async fn renaming_legacy_unvalidated_oauth_keeps_content_and_verification_state() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let service = OAuthService::new(
            repository.clone(),
            paths.clone(),
            AdapterRegistry::default(),
            redactor,
        );
        let legacy_raw = "legacy UTF-8 content\nnot a recognized auth document";
        let legacy_id = Uuid::new_v4();
        let now = Utc::now();
        let legacy = ProviderProfile {
            id: legacy_id,
            name: "Legacy OAuth".into(),
            template_id: Some("codex-auth".into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Codex,
                account_id: None,
                account_label: None,
                digest: bytes_digest(legacy_raw.as_bytes()),
                raw_content: legacy_raw.into(),
                manually_modified: true,
                verification: VerificationInfo {
                    status: VerificationStatus::UserModifiedUnverified,
                    verified_at: None,
                    error: None,
                },
            }),
        };
        let relative = oauth_relative_path(legacy_id);
        let auth_path = paths.root.join(&relative);
        tokio::fs::create_dir_all(auth_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&auth_path, legacy_raw).await.unwrap();
        repository
            .insert_provider(&legacy, Some(&relative))
            .await
            .unwrap();
        let original_file = tokio::fs::read(&auth_path).await.unwrap();

        let renamed = service
            .update_provider(
                legacy_id,
                1,
                "Renamed legacy OAuth".into(),
                legacy_raw.into(),
            )
            .await
            .unwrap();
        assert_eq!(renamed.revision, 2);
        assert_eq!(renamed.name, "Renamed legacy OAuth");
        let ProviderData::Oauth(renamed_oauth) = renamed.data else {
            unreachable!();
        };
        assert!(renamed_oauth.manually_modified);
        assert_eq!(
            renamed_oauth.verification.status,
            VerificationStatus::UserModifiedUnverified
        );
        assert_eq!(tokio::fs::read(&auth_path).await.unwrap(), original_file);

        let unchanged = service
            .update_provider(
                legacy_id,
                2,
                "Renamed legacy OAuth".into(),
                legacy_raw.into(),
            )
            .await
            .unwrap();
        assert_eq!(unchanged.revision, 2);
    }
}
