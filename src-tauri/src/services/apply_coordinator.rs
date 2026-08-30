use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify, RwLock, broadcast};
use uuid::Uuid;

use crate::{
    adapters::{AdapterPaths, AdapterWritePlan, HostEnvironment},
    domain::{
        AppSettings, ApplyItemState, ApplyPreview, ApplyPreviewFile, ApplyPreviewItem,
        ApplyRunItem, ApplyRunSnapshot, CliId, ConfigurationTarget, FieldChange, ProviderData,
        ProviderProfile, RestorePreview,
    },
    error::{AppError, AppResult},
    filesystem::{
        atomic_replace::{atomic_replace, canonicalize_allow_missing, resolve_target},
        digest::{bytes_digest, file_digest},
    },
    persistence::repository::Repository,
    process::process_probe::is_cli_running,
    services::{
        backup::{BackupRecord, BackupService},
        cli_manager::AdapterRegistry,
        discovery::discover_executable,
        redaction::Redactor,
    },
};

const PREVIEW_TTL: ChronoDuration = ChronoDuration::minutes(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProgressEvent {
    pub run_id: Uuid,
    pub cli_id: CliId,
    pub state: ApplyItemState,
    pub message: Option<String>,
}

#[derive(Clone)]
struct PreparedItem {
    target: ConfigurationTarget,
    provider: ProviderProfile,
    paths: AdapterPaths,
    executable: PathBuf,
    plan: AdapterWritePlan,
}

impl std::fmt::Debug for PreparedItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedItem")
            .field("cli_id", &self.target.cli_id())
            .field("paths", &self.paths)
            .field("file_count", &self.plan.files.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct PreparedPreview {
    public: ApplyPreview,
    settings_revision: i64,
    items: HashMap<CliId, PreparedItem>,
}

#[derive(Debug)]
struct ActiveRun {
    snapshot: RwLock<ApplyRunSnapshot>,
    cancel: AtomicBool,
    finished: Notify,
}

#[derive(Debug, Clone)]
pub struct ApplyCoordinator {
    repository: Repository,
    registry: AdapterRegistry,
    backup: BackupService,
    redactor: Redactor,
    previews: Arc<RwLock<HashMap<Uuid, PreparedPreview>>>,
    runs: Arc<RwLock<HashMap<Uuid, Arc<ActiveRun>>>>,
    restore_previews: Arc<RwLock<HashMap<Uuid, RestorePreview>>>,
    write_operation: Arc<Mutex<()>>,
    events: broadcast::Sender<ApplyProgressEvent>,
}

impl ApplyCoordinator {
    pub fn new(
        repository: Repository,
        registry: AdapterRegistry,
        backup: BackupService,
        redactor: Redactor,
    ) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            repository,
            registry,
            backup,
            redactor,
            previews: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            restore_previews: Arc::new(RwLock::new(HashMap::new())),
            write_operation: Arc::new(Mutex::new(())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApplyProgressEvent> {
        self.events.subscribe()
    }

    pub fn try_mutation_guard(&self) -> AppResult<tokio::sync::OwnedMutexGuard<()>> {
        self.write_operation
            .clone()
            .try_lock_owned()
            .map_err(|_| AppError::Blocked("another apply or restore operation is active".into()))
    }

    pub async fn preview(
        &self,
        configuration_id: Uuid,
        expected_revision: i64,
        settings: &AppSettings,
    ) -> AppResult<ApplyPreview> {
        self.preview_targets(configuration_id, expected_revision, settings, None)
            .await
    }

    pub async fn preview_target(
        &self,
        configuration_id: Uuid,
        expected_revision: i64,
        settings: &AppSettings,
        target: ConfigurationTarget,
    ) -> AppResult<ApplyPreview> {
        self.preview_targets(configuration_id, expected_revision, settings, Some(target))
            .await
    }

    async fn preview_targets(
        &self,
        configuration_id: Uuid,
        expected_revision: i64,
        settings: &AppSettings,
        target_override: Option<ConfigurationTarget>,
    ) -> AppResult<ApplyPreview> {
        self.evict_expired().await;
        let configuration = self.repository.get_configuration(configuration_id).await?;
        if configuration.revision != expected_revision {
            return Err(AppError::Conflict(
                "configuration changed before preview".into(),
            ));
        }
        let environment = HostEnvironment::capture()?;
        let targets = target_override
            .map(|target| vec![target])
            .unwrap_or_else(|| configuration.targets.clone());
        let mut public_items = Vec::with_capacity(targets.len());
        let mut prepared_items = HashMap::new();
        for target in &targets {
            let cli_id = target.cli_id();
            let provider = self.repository.get_provider(target.provider_id()).await?;
            let adapter = self.registry.get(cli_id);
            let manual = settings
                .manual_locations
                .iter()
                .find(|item| item.cli_id == cli_id);
            let paths = adapter.resolve_paths(
                &environment,
                manual.and_then(|item| item.config_directory.clone()),
            );
            let protocol = target_protocol(target, &provider);
            let executable = match discover_executable(
                cli_id,
                &environment,
                manual.and_then(|item| item.executable_path.as_deref()),
            )
            .await
            {
                Ok(executable) => executable,
                Err(error) => {
                    public_items.push(ApplyPreviewItem {
                        cli_id,
                        state: ApplyItemState::Failed,
                        path: Some(paths.config_file),
                        provider_name: provider.name,
                        protocol,
                        model: target.model().into(),
                        changes: Vec::new(),
                        files: Vec::new(),
                        warning: Some(self.redactor.sanitize(error.to_string())),
                    });
                    continue;
                }
            };
            let Some(executable) = executable else {
                public_items.push(ApplyPreviewItem {
                    cli_id,
                    state: ApplyItemState::NotInstalled,
                    path: Some(paths.config_file),
                    provider_name: provider.name,
                    protocol,
                    model: target.model().into(),
                    changes: Vec::new(),
                    files: Vec::new(),
                    warning: Some("CLI is not installed; this target will be skipped".into()),
                });
                continue;
            };
            if matches!(target, ConfigurationTarget::Oauth { .. }) {
                let running_warning = match is_cli_running(&executable.path, cli_id.command()) {
                    Some(false) => None,
                    Some(true) => Some("target CLI is running; OAuth switching is blocked"),
                    None => Some(
                        "cannot reliably determine whether the target CLI is running; OAuth switching is blocked",
                    ),
                };
                if let Some(warning) = running_warning {
                    public_items.push(ApplyPreviewItem {
                        cli_id,
                        state: ApplyItemState::RunningBlocked,
                        path: Some(paths.config_file),
                        provider_name: provider.name,
                        protocol,
                        model: target.model().into(),
                        changes: Vec::new(),
                        files: Vec::new(),
                        warning: Some(warning.into()),
                    });
                    continue;
                }
            }
            let plan = match adapter.plan_write(&paths, target, &provider).await {
                Ok(plan) => plan,
                Err(AppError::Validation(message) | AppError::Unsupported(message)) => {
                    public_items.push(ApplyPreviewItem {
                        cli_id,
                        state: ApplyItemState::Incompatible,
                        path: Some(paths.config_file),
                        provider_name: provider.name,
                        protocol,
                        model: target.model().into(),
                        changes: Vec::new(),
                        files: Vec::new(),
                        warning: Some(self.redactor.sanitize(message)),
                    });
                    continue;
                }
                Err(error) => {
                    public_items.push(ApplyPreviewItem {
                        cli_id,
                        state: ApplyItemState::Failed,
                        path: Some(paths.config_file),
                        provider_name: provider.name,
                        protocol,
                        model: target.model().into(),
                        changes: Vec::new(),
                        files: Vec::new(),
                        warning: Some(self.redactor.sanitize(error.to_string())),
                    });
                    continue;
                }
            };
            let unchanged = match all_files_unchanged(&plan).await {
                Ok(unchanged) => unchanged,
                Err(error) => {
                    public_items.push(ApplyPreviewItem {
                        cli_id,
                        state: ApplyItemState::Failed,
                        path: plan.files.first().map(|file| file.path.clone()),
                        provider_name: provider.name,
                        protocol,
                        model: target.model().into(),
                        changes: Vec::new(),
                        files: Vec::new(),
                        warning: Some(self.redactor.sanitize(error.to_string())),
                    });
                    continue;
                }
            };
            let current = adapter.read_current(&paths, &environment).await.ok();
            let changes = vec![
                FieldChange {
                    field: "provider".into(),
                    before: current
                        .as_ref()
                        .and_then(|current| current.current.provider_name.clone()),
                    after: Some(provider.name.clone()),
                },
                FieldChange {
                    field: "protocol".into(),
                    before: current
                        .as_ref()
                        .and_then(|current| current.current.protocol)
                        .map(|value| value.to_string()),
                    after: protocol.map(|value| value.to_string()),
                },
                FieldChange {
                    field: "model".into(),
                    before: current
                        .as_ref()
                        .and_then(|current| current.current.model.clone()),
                    after: Some(target.model().into()),
                },
            ]
            .into_iter()
            .filter(|change| change.before != change.after)
            .collect();
            let files = preview_files(&plan).await;
            public_items.push(ApplyPreviewItem {
                cli_id,
                state: if unchanged {
                    ApplyItemState::Unchanged
                } else {
                    ApplyItemState::Waiting
                },
                path: plan.files.first().map(|file| file.path.clone()),
                provider_name: provider.name.clone(),
                protocol,
                model: target.model().into(),
                changes,
                files,
                warning: plan
                    .warning
                    .clone()
                    .map(|value| self.redactor.sanitize(value)),
            });
            if !unchanged || matches!(target, ConfigurationTarget::Oauth { .. }) {
                prepared_items.insert(
                    cli_id,
                    PreparedItem {
                        target: target.clone(),
                        provider,
                        paths,
                        executable: executable.path,
                        plan,
                    },
                );
            }
        }
        let created_at = Utc::now();
        let preview = ApplyPreview {
            id: Uuid::new_v4(),
            configuration_id,
            configuration_revision: configuration.revision,
            created_at,
            expires_at: created_at + PREVIEW_TTL,
            items: public_items,
        };
        self.previews.write().await.insert(
            preview.id,
            PreparedPreview {
                public: preview.clone(),
                settings_revision: settings.revision,
                items: prepared_items,
            },
        );
        Ok(preview)
    }

    pub async fn start(&self, preview_id: Uuid) -> AppResult<ApplyRunSnapshot> {
        let write_guard = self.try_mutation_guard()?;
        self.start_with_guard(preview_id, write_guard).await
    }

    pub async fn start_with_guard(
        &self,
        preview_id: Uuid,
        write_guard: tokio::sync::OwnedMutexGuard<()>,
    ) -> AppResult<ApplyRunSnapshot> {
        let prepared = self
            .previews
            .write()
            .await
            .remove(&preview_id)
            .ok_or_else(|| {
                AppError::Conflict("apply preview expired or was already used".into())
            })?;
        if prepared.public.expires_at < Utc::now() {
            return Err(AppError::Conflict("apply preview expired".into()));
        }
        let configuration = self
            .repository
            .get_configuration(prepared.public.configuration_id)
            .await?;
        if configuration.revision != prepared.public.configuration_revision {
            return Err(AppError::Conflict(
                "configuration changed after preview".into(),
            ));
        }
        if self.repository.get_settings().await?.revision != prepared.settings_revision {
            return Err(AppError::Conflict(
                "settings changed after the apply preview".into(),
            ));
        }
        let run_id = Uuid::new_v4();
        let snapshot = ApplyRunSnapshot {
            id: run_id,
            preview_id,
            configuration_id: prepared.public.configuration_id,
            started_at: Utc::now(),
            finished_at: None,
            cancel_requested: false,
            items: prepared
                .public
                .items
                .iter()
                .map(|item| ApplyRunItem {
                    cli_id: item.cli_id,
                    state: item.state,
                    message: item.warning.clone(),
                })
                .collect(),
        };
        let active = Arc::new(ActiveRun {
            snapshot: RwLock::new(snapshot.clone()),
            cancel: AtomicBool::new(false),
            finished: Notify::new(),
        });
        self.runs.write().await.insert(run_id, active.clone());
        let coordinator = self.clone();
        tokio::spawn(async move {
            let _write_guard = write_guard;
            coordinator.execute(prepared, active).await;
        });
        Ok(snapshot)
    }

    async fn execute(&self, prepared: PreparedPreview, active: Arc<ActiveRun>) {
        for public in &prepared.public.items {
            let Some(item) = prepared.items.get(&public.cli_id) else {
                continue;
            };
            if active.cancel.load(Ordering::SeqCst) {
                self.set_item(
                    &active,
                    public.cli_id,
                    ApplyItemState::Cancelled,
                    Some("Cancelled before this CLI started".into()),
                )
                .await;
                continue;
            }
            self.set_item(&active, public.cli_id, ApplyItemState::Writing, None)
                .await;
            let result = self
                .execute_item(
                    prepared.public.configuration_id,
                    prepared.public.configuration_revision,
                    item,
                )
                .await;
            match result {
                Ok(unverified) => {
                    self.set_item(
                        &active,
                        public.cli_id,
                        if public.state == ApplyItemState::Unchanged {
                            ApplyItemState::Unchanged
                        } else if unverified {
                            ApplyItemState::SuccessUnverified
                        } else {
                            ApplyItemState::Success
                        },
                        unverified.then(|| "Written, but OAuth content cannot be verified".into()),
                    )
                    .await;
                }
                Err(AppError::Conflict(message)) => {
                    self.set_item(
                        &active,
                        public.cli_id,
                        ApplyItemState::Conflict,
                        Some(message),
                    )
                    .await;
                }
                Err(AppError::Blocked(message)) => {
                    self.set_item(
                        &active,
                        public.cli_id,
                        ApplyItemState::RunningBlocked,
                        Some(message),
                    )
                    .await;
                }
                Err(error) => {
                    self.set_item(
                        &active,
                        public.cli_id,
                        ApplyItemState::Failed,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
        }
        let public_snapshot = active.snapshot.read().await.clone();
        let successful = run_fully_successful(&public_snapshot.items);
        let summary = self.redactor.sanitize(
            serde_json::to_string(&public_snapshot.items).unwrap_or_else(|_| "[]".into()),
        );
        if let Err(error) = self
            .repository
            .record_apply_result(
                public_snapshot.configuration_id,
                public_snapshot.id,
                if successful { "success" } else { "partial" },
                &summary,
                successful,
            )
            .await
        {
            tracing::error!(
                run_id = %public_snapshot.id,
                error = %self.redactor.sanitize(error.to_string()),
                "failed to persist apply summary"
            );
        }
        let mut snapshot = active.snapshot.write().await;
        snapshot.finished_at = Some(Utc::now());
        let run_id = snapshot.id;
        drop(snapshot);
        active.finished.notify_waiters();
        self.runs.write().await.retain(|id, _| *id == run_id);
    }

    async fn execute_item(
        &self,
        configuration_id: Uuid,
        expected_revision: i64,
        item: &PreparedItem,
    ) -> AppResult<bool> {
        let current_configuration = self.repository.get_configuration(configuration_id).await?;
        if current_configuration.revision != expected_revision {
            return Err(AppError::Conflict(
                "configuration revision changed after preview".into(),
            ));
        }
        let current_provider = self.repository.get_provider(item.provider.id).await?;
        if current_provider.revision != item.provider.revision {
            return Err(AppError::Conflict(
                "provider changed after the apply preview".into(),
            ));
        }
        if matches!(item.target, ConfigurationTarget::Oauth { .. }) {
            match is_cli_running(&item.executable, item.target.cli_id().command()) {
                Some(false) => {}
                Some(true) => {
                    return Err(AppError::Blocked(
                        "target CLI is running; OAuth switching is blocked".into(),
                    ));
                }
                None => {
                    return Err(AppError::Blocked(
                        "cannot reliably determine whether the target CLI is running".into(),
                    ));
                }
            }
        }
        let mut changes = Vec::new();
        for file in &item.plan.files {
            let current_digest = file_digest(&file.path).await?;
            if current_digest != file.source_digest {
                return Err(AppError::Conflict(format!(
                    "{} changed after preview",
                    file.path.display()
                )));
            }
            if current_digest != Some(bytes_digest(&file.target_content)) {
                let target = resolve_target(&file.path, &file.allowed_root).await?;
                changes.push((file, target));
            }
        }
        if changes.is_empty() {
            self.upsert_oauth_binding(item).await?;
            return Ok(false);
        }
        let mut backups = Vec::<(BackupRecord, PathBuf)>::new();
        for (file, target) in &changes {
            let backup = self
                .backup
                .create(
                    item.target.cli_id(),
                    target,
                    Some(configuration_id),
                    file.contains_credentials,
                    file.source_digest.as_deref(),
                )
                .await?;
            backups.push((backup, file.allowed_root.clone()));
        }
        let mut written = 0usize;
        for (file, target) in &changes {
            if let Err(error) = atomic_replace(target, &file.target_content).await {
                self.rollback_written(&backups, written).await;
                return Err(error);
            }
            written += 1;
        }
        let verified = self
            .registry
            .get(item.target.cli_id())
            .verify_applied(&item.paths, &item.target, &item.provider)
            .await;
        if !matches!(verified, Ok(true)) {
            self.rollback_written(&backups, written).await;
            return match verified {
                Ok(false) => Err(AppError::Conflict("write verification failed".into())),
                Err(error) => Err(error),
                Ok(true) => unreachable!(),
            };
        }
        if let Err(error) = self.upsert_oauth_binding(item).await {
            self.rollback_written(&backups, written).await;
            return Err(error);
        }
        Ok(item.plan.files.iter().any(|file| file.opaque_content))
    }

    async fn upsert_oauth_binding(&self, item: &PreparedItem) -> AppResult<()> {
        let ConfigurationTarget::Oauth { provider_id, .. } = item.target else {
            return Ok(());
        };
        let (provider_digest, account_identity) = match &item.provider.data {
            ProviderData::Oauth(oauth) => (oauth.digest.clone(), oauth.account_id.as_deref()),
            _ => unreachable!("OAuth target was validated against an OAuth provider"),
        };
        let native_digest = if let Some(auth_file) = item.paths.auth_file.as_ref() {
            file_digest(auth_file).await?.unwrap_or(provider_digest)
        } else {
            provider_digest
        };
        self.repository
            .upsert_active_oauth_binding(
                item.target.cli_id(),
                provider_id,
                &native_digest,
                account_identity,
            )
            .await
    }

    async fn rollback_written(&self, backups: &[(BackupRecord, PathBuf)], written: usize) {
        for (backup, allowed_root) in backups.iter().take(written).rev() {
            let _ = self.backup.rollback(backup, allowed_root).await;
        }
    }

    async fn set_item(
        &self,
        active: &ActiveRun,
        cli_id: CliId,
        state: ApplyItemState,
        message: Option<String>,
    ) {
        let message = message.map(|message| self.redactor.sanitize(message));
        let mut snapshot = active.snapshot.write().await;
        if let Some(item) = snapshot.items.iter_mut().find(|item| item.cli_id == cli_id) {
            item.state = state;
            item.message = message.clone();
        }
        let _ = self.events.send(ApplyProgressEvent {
            run_id: snapshot.id,
            cli_id,
            state,
            message,
        });
    }

    pub async fn cancel(&self, run_id: Uuid) -> AppResult<()> {
        let run = self
            .runs
            .read()
            .await
            .get(&run_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("apply run {run_id}")))?;
        run.cancel.store(true, Ordering::SeqCst);
        run.snapshot.write().await.cancel_requested = true;
        Ok(())
    }

    pub async fn snapshot(&self, run_id: Uuid) -> AppResult<ApplyRunSnapshot> {
        let run = self
            .runs
            .read()
            .await
            .get(&run_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("apply run {run_id}")))?;
        let snapshot = run.snapshot.read().await.clone();
        Ok(snapshot)
    }

    pub async fn latest_snapshot(&self) -> Option<ApplyRunSnapshot> {
        let runs = self.runs.read().await.values().cloned().collect::<Vec<_>>();
        let mut snapshots = Vec::with_capacity(runs.len());
        for run in runs {
            snapshots.push(run.snapshot.read().await.clone());
        }
        snapshots.into_iter().max_by_key(|run| run.started_at)
    }

    pub async fn has_active_runs(&self) -> bool {
        self.write_operation.try_lock().is_err()
    }

    pub async fn retry_preview(
        &self,
        run_id: Uuid,
        settings: &AppSettings,
    ) -> AppResult<ApplyPreview> {
        let snapshot = self.snapshot(run_id).await?;
        let retry_cli_ids = snapshot
            .items
            .iter()
            .filter(|item| is_retryable(item.state))
            .map(|item| item.cli_id)
            .collect::<std::collections::HashSet<_>>();
        if retry_cli_ids.is_empty() {
            return Err(AppError::Validation(
                "apply run has no failed or conflicted items to retry".into(),
            ));
        }
        let configuration = self
            .repository
            .get_configuration(snapshot.configuration_id)
            .await?;
        let mut public = self
            .preview(configuration.id, configuration.revision, settings)
            .await?;
        let mut previews = self.previews.write().await;
        let prepared = previews
            .get_mut(&public.id)
            .ok_or_else(|| AppError::NotFound(format!("apply preview {}", public.id)))?;
        prepared
            .items
            .retain(|cli_id, _| retry_cli_ids.contains(cli_id));
        prepared
            .public
            .items
            .retain(|item| retry_cli_ids.contains(&item.cli_id));
        public.items = prepared.public.items.clone();
        Ok(public)
    }

    pub async fn preview_restore(&self, backup_id: Uuid) -> AppResult<RestorePreview> {
        let backup = self.backup.get(backup_id).await?;
        let preview = RestorePreview {
            id: Uuid::new_v4(),
            backup_id,
            cli_id: backup.metadata.cli_id,
            target_path: backup.metadata.original_path.clone(),
            current_digest: file_digest(&backup.metadata.original_path).await?,
            restores_tombstone: !backup.metadata.originally_existed,
            contains_credentials: backup.metadata.contains_credentials,
            expires_at: Utc::now() + PREVIEW_TTL,
        };
        self.restore_previews
            .write()
            .await
            .insert(preview.id, preview.clone());
        Ok(preview)
    }

    pub async fn restore(&self, preview_id: Uuid, settings: &AppSettings) -> AppResult<()> {
        let _write_guard = self.try_mutation_guard()?;
        let preview = self
            .restore_previews
            .write()
            .await
            .remove(&preview_id)
            .ok_or_else(|| {
                AppError::Conflict("restore preview expired or was already used".into())
            })?;
        if preview.expires_at < Utc::now() {
            return Err(AppError::Conflict("restore preview expired".into()));
        }
        let environment = HostEnvironment::capture()?;
        let adapter = self.registry.get(preview.cli_id);
        let manual = settings
            .manual_locations
            .iter()
            .find(|item| item.cli_id == preview.cli_id)
            .and_then(|item| item.config_directory.clone());
        let paths = adapter.resolve_paths(&environment, manual);
        let canonical_target = canonicalize_allow_missing(&preview.target_path).await?;
        let canonical_config = canonicalize_allow_missing(&paths.config_directory).await?;
        let allowed_root = if canonical_target.starts_with(&canonical_config) {
            paths.config_directory
        } else if let Some(auth) = paths.auth_file
            && canonical_target == canonicalize_allow_missing(&auth).await?
        {
            auth.parent().expect("checked parent").to_path_buf()
        } else {
            return Err(AppError::Blocked(
                "backup target is outside the current approved CLI directories".into(),
            ));
        };
        self.backup
            .restore(preview.backup_id, &allowed_root, preview.current_digest)
            .await
    }

    pub async fn shutdown(&self) {
        let runs = self.runs.read().await.values().cloned().collect::<Vec<_>>();
        for run in &runs {
            run.cancel.store(true, Ordering::SeqCst);
        }
        for run in runs {
            if run.snapshot.read().await.finished_at.is_none() {
                let _ =
                    tokio::time::timeout(Duration::from_secs(15), run.finished.notified()).await;
            }
        }
        let _ = tokio::time::timeout(
            Duration::from_secs(15),
            self.write_operation.clone().lock_owned(),
        )
        .await;
    }

    async fn evict_expired(&self) {
        let now = Utc::now();
        self.previews
            .write()
            .await
            .retain(|_, preview| preview.public.expires_at >= now);
        self.restore_previews
            .write()
            .await
            .retain(|_, preview| preview.expires_at >= now);
    }
}

async fn all_files_unchanged(plan: &AdapterWritePlan) -> AppResult<bool> {
    for file in &plan.files {
        if file_digest(&file.path).await? != Some(bytes_digest(&file.target_content)) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn preview_files(plan: &AdapterWritePlan) -> Vec<ApplyPreviewFile> {
    let mut files = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let source_content = match tokio::fs::read(&file.path).await {
            Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => None,
        };
        files.push(ApplyPreviewFile {
            path: file.path.clone(),
            existed: source_content.is_some(),
            source_content,
            target_content: String::from_utf8_lossy(&file.target_content).into_owned(),
        });
    }
    files
}

fn target_protocol(
    target: &ConfigurationTarget,
    provider: &ProviderProfile,
) -> Option<crate::domain::CliProtocol> {
    match (target, &provider.data) {
        (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => api
            .connections
            .iter()
            .find(|connection| connection.id == *connection_id)
            .map(|connection| connection.protocol),
        _ => None,
    }
}

fn is_retryable(state: ApplyItemState) -> bool {
    matches!(
        state,
        ApplyItemState::Failed | ApplyItemState::Conflict | ApplyItemState::RunningBlocked
    )
}

fn run_fully_successful(items: &[ApplyRunItem]) -> bool {
    items.iter().all(|item| {
        matches!(
            item.state,
            ApplyItemState::Success | ApplyItemState::Unchanged | ApplyItemState::NotInstalled
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::{AdapterWritePlan, FileWritePlan},
        domain::{
            ApiProviderData, CliProtocol, ConnectionAuthType, OAuthKind, OAuthProviderData,
            ProviderConnection, SavedConfiguration, VerificationInfo,
        },
        filesystem::private_paths::PrivatePaths,
    };
    use url::Url;

    async fn fixture() -> (
        tempfile::TempDir,
        PrivatePaths,
        Repository,
        ApplyCoordinator,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone())
            .await
            .unwrap();
        let coordinator = ApplyCoordinator::new(
            repository.clone(),
            AdapterRegistry::default(),
            BackupService::new(repository.clone(), paths.clone()),
            redactor,
        );
        (temp, paths, repository, coordinator)
    }

    fn api_provider() -> ProviderProfile {
        let now = Utc::now();
        ProviderProfile {
            id: Uuid::new_v4(),
            name: "Fixture API".into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "responses-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse("https://example.test/v1").unwrap(),
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: "fixture-secret".into(),
                    default_model: "model-a".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        }
    }

    fn configuration(targets: Vec<ConfigurationTarget>) -> SavedConfiguration {
        let now = Utc::now();
        SavedConfiguration {
            id: Uuid::new_v4(),
            name: format!("Fixture {}", Uuid::new_v4()),
            creation_order: 0,
            revision: 1,
            targets,
            last_applied_at: None,
            last_apply_summary: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn api_target(cli_id: CliId, provider: &ProviderProfile) -> ConfigurationTarget {
        let ProviderData::Api(api) = &provider.data else {
            unreachable!();
        };
        ConfigurationTarget::Api {
            cli_id,
            provider_id: provider.id,
            connection_id: api.connections[0].id,
            model: "model-a".into(),
        }
    }

    fn public_item(cli_id: CliId, state: ApplyItemState) -> ApplyPreviewItem {
        ApplyPreviewItem {
            cli_id,
            state,
            path: None,
            provider_name: "Fixture".into(),
            protocol: Some(CliProtocol::OpenaiResponses),
            model: "model-a".into(),
            changes: Vec::new(),
            files: Vec::new(),
            warning: None,
        }
    }

    fn prepared_item(
        target: ConfigurationTarget,
        provider: ProviderProfile,
        plan: AdapterWritePlan,
    ) -> PreparedItem {
        PreparedItem {
            target,
            provider,
            paths: AdapterPaths {
                config_directory: PathBuf::new(),
                config_file: PathBuf::new(),
                auth_file: None,
            },
            executable: PathBuf::new(),
            plan,
        }
    }

    fn active_run(
        id: Uuid,
        preview_id: Uuid,
        configuration_id: Uuid,
        items: Vec<ApplyRunItem>,
    ) -> Arc<ActiveRun> {
        Arc::new(ActiveRun {
            snapshot: RwLock::new(ApplyRunSnapshot {
                id,
                preview_id,
                configuration_id,
                started_at: Utc::now(),
                finished_at: None,
                cancel_requested: false,
                items,
            }),
            cancel: AtomicBool::new(false),
            finished: Notify::new(),
        })
    }

    #[tokio::test]
    async fn unchanged_plan_is_detected_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("config");
        tokio::fs::write(&file, b"same").await.unwrap();
        let plan = AdapterWritePlan {
            cli_id: CliId::Codex,
            files: vec![FileWritePlan {
                path: file.clone(),
                allowed_root: temp.path().to_path_buf(),
                source_digest: file_digest(&file).await.unwrap(),
                target_content: b"same".to_vec(),
                contains_credentials: false,
                opaque_content: false,
            }],
            warning: None,
        };
        assert!(all_files_unchanged(&plan).await.unwrap());
    }

    #[tokio::test]
    async fn preview_file_snapshot_includes_source_and_target_content() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("existing.toml");
        let missing = temp.path().join("new.toml");
        tokio::fs::write(&existing, b"model = \"old\"\n")
            .await
            .unwrap();
        let plan = AdapterWritePlan {
            cli_id: CliId::Codex,
            files: vec![
                FileWritePlan {
                    path: existing.clone(),
                    allowed_root: temp.path().to_path_buf(),
                    source_digest: file_digest(&existing).await.unwrap(),
                    target_content: b"model = \"new\"\n".to_vec(),
                    contains_credentials: false,
                    opaque_content: false,
                },
                FileWritePlan {
                    path: missing,
                    allowed_root: temp.path().to_path_buf(),
                    source_digest: Some("stale-digest".into()),
                    target_content: b"model = \"new\"\n".to_vec(),
                    contains_credentials: false,
                    opaque_content: false,
                },
            ],
            warning: None,
        };

        let files = preview_files(&plan).await;

        assert_eq!(
            files[0].source_content.as_deref(),
            Some("model = \"old\"\n")
        );
        assert!(files[0].existed);
        assert_eq!(files[0].target_content, "model = \"new\"\n");
        assert_eq!(files[1].source_content, None);
        assert!(!files[1].existed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn single_target_preview_only_includes_requested_cli() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, _paths, repository, coordinator) = fixture().await;
        let provider = api_provider();
        repository.insert_provider(&provider, None).await.unwrap();
        let configuration = configuration(vec![
            api_target(CliId::Codex, &provider),
            api_target(CliId::Opencode, &provider),
        ]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();

        let executable = temp.path().join("codex-fixture");
        tokio::fs::write(&executable, b"#!/bin/sh\necho codex 1.0\n")
            .await
            .unwrap();
        tokio::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        let config_directory = temp.path().join("codex-config");
        tokio::fs::create_dir_all(&config_directory).await.unwrap();
        let mut settings = repository.get_settings().await.unwrap();
        for location in &mut settings.manual_locations {
            if location.cli_id == CliId::Codex {
                location.executable_path = Some(executable.clone());
                location.config_directory = Some(config_directory.clone());
            }
        }

        let preview = coordinator
            .preview_target(
                configuration.id,
                configuration.revision,
                &settings,
                api_target(CliId::Codex, &provider),
            )
            .await
            .unwrap();

        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.items[0].cli_id, CliId::Codex);
    }

    #[tokio::test]
    async fn background_run_retains_the_mutation_guard_until_finished() {
        let (_temp, _paths, repository, coordinator) = fixture().await;
        let configuration = configuration(Vec::new());
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();
        let settings = repository.get_settings().await.unwrap();
        let preview_id = Uuid::new_v4();
        let now = Utc::now();
        coordinator.previews.write().await.insert(
            preview_id,
            PreparedPreview {
                public: ApplyPreview {
                    id: preview_id,
                    configuration_id: configuration.id,
                    configuration_revision: configuration.revision,
                    created_at: now,
                    expires_at: now + PREVIEW_TTL,
                    items: Vec::new(),
                },
                settings_revision: settings.revision,
                items: HashMap::new(),
            },
        );

        let guard = coordinator.try_mutation_guard().unwrap();
        let run = coordinator
            .start_with_guard(preview_id, guard)
            .await
            .unwrap();
        assert!(coordinator.try_mutation_guard().is_err());

        for _ in 0..100 {
            if coordinator
                .snapshot(run.id)
                .await
                .unwrap()
                .finished_at
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            coordinator
                .snapshot(run.id)
                .await
                .unwrap()
                .finished_at
                .is_some()
        );
        assert!(coordinator.try_mutation_guard().is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovery_error_for_one_cli_does_not_abort_the_rest_of_the_preview() {
        use std::os::unix::fs::PermissionsExt;

        let (temp, _paths, repository, coordinator) = fixture().await;
        let mut provider = api_provider();
        let anthropic_connection = ProviderConnection {
            id: Uuid::new_v4(),
            template_endpoint_id: None,
            credential_slot_id: "anthropic-key".into(),
            protocol: CliProtocol::AnthropicMessages,
            endpoint: Url::parse("https://anthropic.example.test/v1").unwrap(),
            auth_type: ConnectionAuthType::Bearer,
            api_key: "anthropic-fixture-secret".into(),
            default_model: "claude-model".into(),
            verification: VerificationInfo::default(),
        };
        let ProviderData::Api(api) = &mut provider.data else {
            unreachable!();
        };
        api.connections.push(anthropic_connection.clone());
        repository.insert_provider(&provider, None).await.unwrap();
        let configuration = configuration(vec![
            ConfigurationTarget::Api {
                cli_id: CliId::ClaudeCode,
                provider_id: provider.id,
                connection_id: anthropic_connection.id,
                model: "claude-model".into(),
            },
            api_target(CliId::Codex, &provider),
        ]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();
        let executable = temp.path().join("codex-fixture");
        tokio::fs::write(&executable, b"#!/bin/sh\necho codex 1.0\n")
            .await
            .unwrap();
        tokio::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        let mut settings = repository.get_settings().await.unwrap();
        for location in &mut settings.manual_locations {
            match location.cli_id {
                CliId::ClaudeCode => {
                    location.executable_path = Some(temp.path().join("missing-claude"));
                    location.config_directory = Some(temp.path().join("claude-config"));
                }
                CliId::Codex => {
                    location.executable_path = Some(executable.clone());
                    location.config_directory = Some(temp.path().join("codex-config"));
                }
                CliId::Opencode => {}
            }
        }

        let preview = coordinator
            .preview(configuration.id, configuration.revision, &settings)
            .await
            .unwrap();

        assert_eq!(preview.items.len(), 2);
        assert_eq!(preview.items[0].cli_id, CliId::ClaudeCode);
        assert_eq!(preview.items[0].state, ApplyItemState::Failed);
        assert_eq!(preview.items[1].cli_id, CliId::Codex);
        assert_eq!(preview.items[1].state, ApplyItemState::Waiting);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn second_file_write_failure_rolls_back_the_first_file() {
        let (temp, _paths, repository, coordinator) = fixture().await;
        let provider = api_provider();
        repository.insert_provider(&provider, None).await.unwrap();
        let target = api_target(CliId::Codex, &provider);
        let configuration = configuration(vec![target.clone()]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();

        let config_root = temp.path().join("config");
        tokio::fs::create_dir_all(&config_root).await.unwrap();
        let first = config_root.join("first.toml");
        tokio::fs::write(&first, b"original").await.unwrap();
        let read_only_target = PathBuf::from("/proc/version");
        let plan = AdapterWritePlan {
            cli_id: CliId::Codex,
            files: vec![
                FileWritePlan {
                    path: first.clone(),
                    allowed_root: config_root,
                    source_digest: file_digest(&first).await.unwrap(),
                    target_content: b"changed".to_vec(),
                    contains_credentials: false,
                    opaque_content: false,
                },
                FileWritePlan {
                    path: read_only_target.clone(),
                    allowed_root: PathBuf::from("/proc"),
                    source_digest: file_digest(&read_only_target).await.unwrap(),
                    target_content: b"cannot replace procfs".to_vec(),
                    contains_credentials: false,
                    opaque_content: false,
                },
            ],
            warning: None,
        };
        let item = prepared_item(target, provider, plan);

        let error = coordinator
            .execute_item(configuration.id, configuration.revision, &item)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("temporary file creation"));
        assert_eq!(tokio::fs::read(&first).await.unwrap(), b"original");
    }

    #[tokio::test]
    async fn one_failed_cli_does_not_stop_the_next_cli_and_completed_runs_are_pruned() {
        let (_temp, _paths, repository, coordinator) = fixture().await;
        let provider = api_provider();
        repository.insert_provider(&provider, None).await.unwrap();
        let codex = api_target(CliId::Codex, &provider);
        let opencode = api_target(CliId::Opencode, &provider);
        let configuration = configuration(vec![codex.clone(), opencode.clone()]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();
        let empty_plan = |cli_id| AdapterWritePlan {
            cli_id,
            files: Vec::new(),
            warning: None,
        };
        let mut stale_provider = provider.clone();
        stale_provider.revision = 0;
        let preview_id = Uuid::new_v4();
        let prepared = PreparedPreview {
            public: ApplyPreview {
                id: preview_id,
                configuration_id: configuration.id,
                configuration_revision: configuration.revision,
                created_at: Utc::now(),
                expires_at: Utc::now() + PREVIEW_TTL,
                items: vec![
                    public_item(CliId::Codex, ApplyItemState::Waiting),
                    public_item(CliId::Opencode, ApplyItemState::Waiting),
                ],
            },
            settings_revision: 1,
            items: HashMap::from([
                (
                    CliId::Codex,
                    prepared_item(codex, stale_provider, empty_plan(CliId::Codex)),
                ),
                (
                    CliId::Opencode,
                    prepared_item(opencode, provider, empty_plan(CliId::Opencode)),
                ),
            ]),
        };
        let run_id = Uuid::new_v4();
        let active = active_run(
            run_id,
            preview_id,
            configuration.id,
            vec![
                ApplyRunItem {
                    cli_id: CliId::Codex,
                    state: ApplyItemState::Waiting,
                    message: None,
                },
                ApplyRunItem {
                    cli_id: CliId::Opencode,
                    state: ApplyItemState::Waiting,
                    message: None,
                },
            ],
        );
        let stale_id = Uuid::new_v4();
        coordinator.runs.write().await.insert(
            stale_id,
            active_run(stale_id, preview_id, configuration.id, Vec::new()),
        );
        coordinator
            .runs
            .write()
            .await
            .insert(run_id, active.clone());

        coordinator.execute(prepared, active.clone()).await;

        let snapshot = active.snapshot.read().await.clone();
        assert_eq!(snapshot.items[0].state, ApplyItemState::Conflict);
        assert_eq!(snapshot.items[1].state, ApplyItemState::Success);
        assert!(snapshot.finished_at.is_some());
        assert_eq!(
            coordinator
                .runs
                .read()
                .await
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![run_id]
        );
        let persisted: String =
            sqlx::query_scalar("SELECT run_id FROM latest_apply_runs WHERE configuration_id = ?")
                .bind(configuration.id.to_string())
                .fetch_one(repository.pool())
                .await
                .unwrap();
        assert_eq!(persisted, run_id.to_string());
    }

    #[tokio::test]
    async fn cancellation_preserves_completed_items_and_cancels_only_unstarted_items() {
        let (_temp, _paths, repository, coordinator) = fixture().await;
        let provider = api_provider();
        repository.insert_provider(&provider, None).await.unwrap();
        let target = api_target(CliId::Codex, &provider);
        let configuration = configuration(vec![target.clone()]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();
        let preview_id = Uuid::new_v4();
        let prepared = PreparedPreview {
            public: ApplyPreview {
                id: preview_id,
                configuration_id: configuration.id,
                configuration_revision: configuration.revision,
                created_at: Utc::now(),
                expires_at: Utc::now() + PREVIEW_TTL,
                items: vec![
                    public_item(CliId::ClaudeCode, ApplyItemState::Success),
                    public_item(CliId::Codex, ApplyItemState::Waiting),
                ],
            },
            settings_revision: 1,
            items: HashMap::from([(
                CliId::Codex,
                prepared_item(
                    target,
                    provider,
                    AdapterWritePlan {
                        cli_id: CliId::Codex,
                        files: Vec::new(),
                        warning: None,
                    },
                ),
            )]),
        };
        let run_id = Uuid::new_v4();
        let active = active_run(
            run_id,
            preview_id,
            configuration.id,
            vec![
                ApplyRunItem {
                    cli_id: CliId::ClaudeCode,
                    state: ApplyItemState::Success,
                    message: None,
                },
                ApplyRunItem {
                    cli_id: CliId::Codex,
                    state: ApplyItemState::Waiting,
                    message: None,
                },
            ],
        );
        coordinator
            .runs
            .write()
            .await
            .insert(run_id, active.clone());
        coordinator.cancel(run_id).await.unwrap();

        coordinator.execute(prepared, active.clone()).await;

        let snapshot = active.snapshot.read().await.clone();
        assert!(snapshot.cancel_requested);
        assert_eq!(snapshot.items[0].state, ApplyItemState::Success);
        assert_eq!(snapshot.items[1].state, ApplyItemState::Cancelled);
    }

    #[tokio::test]
    async fn unchanged_oauth_files_still_create_the_active_binding() {
        let (temp, paths, repository, coordinator) = fixture().await;
        let id = Uuid::new_v4();
        let raw = br#"{"claudeAiOauth":{"accountUuid":"account-a","accessToken":"token-a"}}"#;
        let profile_directory = paths.auth_profile_dir(id).await.unwrap();
        tokio::fs::write(profile_directory.join("auth.txt"), raw)
            .await
            .unwrap();
        let now = Utc::now();
        let provider = ProviderProfile {
            id,
            name: "Claude OAuth".into(),
            template_id: Some("anthropic-auth".into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Anthropic,
                account_id: Some("account-a".into()),
                account_label: None,
                raw_content: String::from_utf8(raw.to_vec()).unwrap(),
                digest: bytes_digest(raw),
                manually_modified: false,
                verification: VerificationInfo::default(),
            }),
        };
        let relative = PathBuf::from("auth").join(id.to_string()).join("auth.txt");
        repository
            .insert_provider(&provider, Some(&relative))
            .await
            .unwrap();
        let target = ConfigurationTarget::Oauth {
            cli_id: CliId::ClaudeCode,
            provider_id: id,
            model: "model-a".into(),
        };
        let configuration = configuration(vec![target.clone()]);
        repository
            .insert_configuration(&configuration)
            .await
            .unwrap();
        let native_root = temp.path().join("native");
        tokio::fs::create_dir_all(&native_root).await.unwrap();
        let native_auth = native_root.join("auth.json");
        tokio::fs::write(&native_auth, raw).await.unwrap();
        let executable = temp.path().join("unused-codex-executable");
        tokio::fs::write(&executable, b"unused").await.unwrap();
        let item = PreparedItem {
            target,
            provider,
            paths: AdapterPaths {
                config_directory: native_root.clone(),
                config_file: native_root.join("config.toml"),
                auth_file: Some(native_auth.clone()),
            },
            executable,
            plan: AdapterWritePlan {
                cli_id: CliId::ClaudeCode,
                files: vec![FileWritePlan {
                    path: native_auth.clone(),
                    allowed_root: native_root,
                    source_digest: file_digest(&native_auth).await.unwrap(),
                    target_content: raw.to_vec(),
                    contains_credentials: true,
                    opaque_content: false,
                }],
                warning: None,
            },
        };

        assert!(
            !coordinator
                .execute_item(configuration.id, configuration.revision, &item)
                .await
                .unwrap()
        );
        let binding = repository
            .get_active_oauth_binding(CliId::ClaudeCode)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(binding.provider_id, id);
        assert_eq!(binding.native_digest, bytes_digest(raw));
    }

    #[test]
    fn retry_is_limited_to_failed_conflicted_or_running_blocked_items() {
        assert!(is_retryable(ApplyItemState::Failed));
        assert!(is_retryable(ApplyItemState::Conflict));
        assert!(is_retryable(ApplyItemState::RunningBlocked));
        assert!(!is_retryable(ApplyItemState::Cancelled));
        assert!(!is_retryable(ApplyItemState::Success));
        assert!(!is_retryable(ApplyItemState::NotInstalled));
    }

    #[test]
    fn unverified_or_incompatible_items_never_count_as_fully_successful() {
        let item = |state| ApplyRunItem {
            cli_id: CliId::Codex,
            state,
            message: None,
        };
        assert!(run_fully_successful(&[
            item(ApplyItemState::Success),
            item(ApplyItemState::Unchanged),
            item(ApplyItemState::NotInstalled),
        ]));
        assert!(!run_fully_successful(&[item(
            ApplyItemState::SuccessUnverified
        )]));
        assert!(!run_fully_successful(&[item(ApplyItemState::Incompatible)]));
    }
}
