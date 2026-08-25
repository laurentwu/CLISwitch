use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool},
};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::{ProviderCatalog, embedded_catalog},
    domain::{
        AppSettings, ApplyRunSnapshot, ConfigurationMatchStatus, PublicProvider,
        SavedConfiguration, ScanSnapshot, calculate_configuration_match,
    },
    error::AppResult,
    filesystem::private_paths::PrivatePaths,
    persistence::repository::Repository,
    services::{
        apply_coordinator::ApplyCoordinator,
        backup::BackupService,
        cli_manager::{AdapterRegistry, CliManager},
        model_catalog::ModelCatalogService,
        oauth::OAuthService,
        redaction::Redactor,
    },
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    pub ready: bool,
    pub code: Option<String>,
    pub message: Option<String>,
    pub app_data_directory: std::path::PathBuf,
}

#[derive(Debug)]
pub struct StartupState(pub StartupStatus);

impl StartupState {
    pub fn ready(app_data_directory: std::path::PathBuf) -> Self {
        Self(StartupStatus {
            ready: true,
            code: None,
            message: None,
            app_data_directory,
        })
    }

    pub fn diagnostic(
        app_data_directory: std::path::PathBuf,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self(StartupStatus {
            ready: false,
            code: Some(code.into()),
            message: Some(message.into()),
            app_data_directory,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub providers: Vec<PublicProvider>,
    pub configurations: Vec<SavedConfiguration>,
    pub current: Option<ScanSnapshot>,
    pub latest_apply: Option<ApplyRunSnapshot>,
    pub configuration_statuses: BTreeMap<uuid::Uuid, ConfigurationMatchStatus>,
    pub app_data_directory: std::path::PathBuf,
    pub backup_bytes: u64,
    pub app_version: String,
    pub catalog: ProviderCatalog,
}

#[derive(Debug)]
pub struct AppState {
    pub paths: PrivatePaths,
    pub repository: Repository,
    pub cli_manager: CliManager,
    pub backup: BackupService,
    pub oauth: OAuthService,
    pub apply: ApplyCoordinator,
    pub models: ModelCatalogService,
    pub catalog: ProviderCatalog,
    pub redactor: Redactor,
    pub safe_to_exit: Arc<AtomicBool>,
    pub frontend_dirty: Arc<AtomicBool>,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl AppState {
    pub async fn initialize(root: std::path::PathBuf) -> AppResult<Self> {
        let catalog = embedded_catalog()?.clone();
        let paths = PrivatePaths::from_root(root);
        paths.ensure().await?;
        let log_guard = initialize_logging(&paths);
        let redactor = Redactor::default();
        let repository = Repository::open(&paths.database, redactor.clone()).await?;
        let registry = AdapterRegistry::default();
        let cli_manager = CliManager::new(registry.clone(), repository.clone(), redactor.clone());
        let backup = BackupService::new(repository.clone(), paths.clone());
        let oauth = OAuthService::new(
            repository.clone(),
            paths.clone(),
            registry.clone(),
            redactor.clone(),
        );
        oauth.cleanup_stale_sessions().await?;
        let apply = ApplyCoordinator::new(
            repository.clone(),
            registry,
            backup.clone(),
            redactor.clone(),
        );
        let settings = repository.get_settings().await?;
        if settings.scan_on_startup {
            if let Err(error) = oauth.refresh_active_bindings(&settings).await {
                tracing::warn!(
                    operation = "oauth-refresh-before-scan",
                    error = %redactor.sanitize(error.to_string()),
                    "OAuth refresh failed; continuing with an independent CLI scan"
                );
            }
            cli_manager.scan(&settings).await;
        }
        Ok(Self {
            paths,
            repository,
            cli_manager,
            backup,
            oauth,
            apply,
            models: ModelCatalogService::new()?,
            catalog,
            redactor,
            safe_to_exit: Arc::new(AtomicBool::new(false)),
            frontend_dirty: Arc::new(AtomicBool::new(false)),
            _log_guard: log_guard,
        })
    }

    pub async fn snapshot(&self) -> AppResult<AppSnapshot> {
        let providers = self.repository.list_providers().await?;
        let configurations = self.repository.list_configurations().await?;
        let current = self.cli_manager.latest_snapshot().await;
        let mut profiles = Vec::with_capacity(providers.len());
        for provider in &providers {
            profiles.push(self.repository.get_provider(provider.id).await?);
        }
        let configuration_statuses = configurations
            .iter()
            .map(|configuration| {
                let status = current
                    .as_ref()
                    .map(|scan| calculate_configuration_match(configuration, scan, &profiles))
                    .unwrap_or(ConfigurationMatchStatus::UnableToVerify);
                (configuration.id, status)
            })
            .collect();
        Ok(AppSnapshot {
            settings: self.repository.get_settings().await?,
            providers,
            configurations,
            current,
            latest_apply: self.apply.latest_snapshot().await,
            configuration_statuses,
            app_data_directory: self.paths.root.clone(),
            backup_bytes: directory_size(&self.paths.backups).await?,
            app_version: env!("CARGO_PKG_VERSION").into(),
            catalog: self.catalog.clone(),
        })
    }
}

async fn directory_size(root: &std::path::Path) -> AppResult<u64> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn initialize_logging(paths: &PrivatePaths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

    let appender = tracing_appender::rolling::daily(&paths.logs, "cliswitch.jsonl");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("info,sqlx=warn"))
        .with(tracing_subscriber::fmt::layer().json().with_writer(writer));
    subscriber.try_init().ok().map(|_| guard)
}
