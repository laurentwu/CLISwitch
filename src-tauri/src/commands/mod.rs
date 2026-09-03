use std::{path::PathBuf, sync::atomic::Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use url::Url;
use uuid::Uuid;

use crate::{
    app_state::{AppSnapshot, AppState, StartupState, StartupStatus},
    catalog::{ProviderCatalog, install_runtime_catalog},
    domain::{
        ApiProviderData, AppSettings, BackupMetadata, CliId, CliProtocol, ConfigurationTarget,
        ConnectionAuthType, OAuthKind, ProviderConnection, ProviderData, ProviderProfile,
        PublicProvider, RestorePreview, SavedConfiguration, ScanSnapshot, VerificationInfo,
        validate_ui_zoom_percent,
    },
    error::{AppError, AppResult},
    services::{
        apply_coordinator::ApplyProgressEvent,
        catalog_cache::CatalogStatus,
        cli_manager::UnmanagedCandidateSaveRequest,
        minimax::normalize_provider_credential_kind,
        model_catalog::{ReleaseCheck, check_github_release as check_release},
        oauth::{OAuthSessionSnapshot, validate_oauth_browser_url},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDraft {
    pub id: Option<Uuid>,
    pub template_endpoint_id: Option<String>,
    pub credential_slot_id: String,
    pub protocol: CliProtocol,
    pub endpoint: Url,
    pub auth_type: ConnectionAuthType,
    pub api_key: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProviderDraft {
    pub name: String,
    pub template_id: Option<String>,
    pub connections: Vec<ConnectionDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConfigurationRequest {
    pub name: String,
    pub targets: Vec<ConfigurationTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseState {
    pub frontend_dirty: bool,
    pub oauth_active: bool,
    pub apply_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgressEvent {
    scan_id: Uuid,
    phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateChangedEvent {
    scope: &'static str,
}

#[tauri::command]
pub fn get_startup_status(state: State<'_, StartupState>) -> StartupStatus {
    state.0.clone()
}

#[tauri::command]
pub fn open_startup_data_directory(
    app: AppHandle,
    state: State<'_, StartupState>,
) -> AppResult<()> {
    app.opener()
        .open_path(
            state.0.app_data_directory.to_string_lossy().to_string(),
            None::<String>,
        )
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    state.snapshot().await
}

/// Returns the current provider cache status without exposing provider secrets.
#[tauri::command]
pub async fn get_catalog_status(state: State<'_, AppState>) -> AppResult<CatalogStatus> {
    Ok(state.catalog_cache.status().await)
}

/// Lists all upstream providers, including entries which are intentionally disabled by the
/// backend compatibility/security policy.
#[tauri::command]
pub async fn list_catalog_providers(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::catalog::CatalogProviderInfo>> {
    state.catalog_cache.providers().await
}

/// Downloads and validates a new CLIAdapter snapshot. The old snapshot remains active if any
/// network, size, schema, or atomic-write step fails.
#[tauri::command]
pub async fn update_catalog(state: State<'_, AppState>) -> AppResult<CatalogStatus> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let status = state.catalog_cache.refresh().await?;
    install_runtime_catalog(state.catalog_cache.catalog().await?);
    Ok(status)
}

#[tauri::command]
pub async fn scan_clis(app: AppHandle, state: State<'_, AppState>) -> AppResult<ScanSnapshot> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let scan_id = Uuid::new_v4();
    let _ = app.emit(
        "cliswitch://scan-progress",
        ScanProgressEvent {
            scan_id,
            phase: "starting",
        },
    );
    let settings = state.repository.get_settings().await?;
    if let Err(error) = state.oauth.refresh_active_bindings(&settings).await {
        tracing::warn!(
            operation = "oauth-refresh-before-scan",
            error = %state.redactor.sanitize(error.to_string()),
            "OAuth refresh failed; continuing with an independent CLI scan"
        );
        let _ = app.emit(
            "cliswitch://scan-progress",
            ScanProgressEvent {
                scan_id,
                phase: "refresh-warning",
            },
        );
    }
    let snapshot = state.cli_manager.scan(&settings).await;
    let _ = app.emit(
        "cliswitch://scan-progress",
        ScanProgressEvent {
            scan_id,
            phase: "complete",
        },
    );
    let _ = app.emit(
        "cliswitch://state-changed",
        StateChangedEvent { scope: "scan" },
    );
    Ok(snapshot)
}

#[tauri::command]
pub async fn select_cli_executable(
    app: AppHandle,
    state: State<'_, AppState>,
    cli_id: CliId,
) -> AppResult<Option<AppSettings>> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let selected = app.dialog().file().blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let path = tokio::fs::canonicalize(path).await?;
    if !tokio::fs::metadata(&path).await?.is_file() {
        return Err(AppError::Validation(
            "selected executable is not a file".into(),
        ));
    }
    let mut settings = state.repository.get_settings().await?;
    let revision = settings.revision;
    let location = settings
        .manual_locations
        .iter_mut()
        .find(|location| location.cli_id == cli_id)
        .expect("all supported CLI locations exist");
    location.executable_path = Some(path);
    state
        .repository
        .update_settings(&settings, revision)
        .await?;
    settings.revision += 1;
    Ok(Some(settings))
}

#[tauri::command]
pub async fn select_cli_config_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    cli_id: CliId,
) -> AppResult<Option<AppSettings>> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let selected = app.dialog().file().blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let path = tokio::fs::canonicalize(path).await?;
    if !tokio::fs::metadata(&path).await?.is_dir() {
        return Err(AppError::Validation(
            "selected configuration path is not a directory".into(),
        ));
    }
    let mut settings = state.repository.get_settings().await?;
    let revision = settings.revision;
    let location = settings
        .manual_locations
        .iter_mut()
        .find(|location| location.cli_id == cli_id)
        .expect("all supported CLI locations exist");
    location.config_directory = Some(path);
    state
        .repository
        .update_settings(&settings, revision)
        .await?;
    settings.revision += 1;
    Ok(Some(settings))
}

#[tauri::command]
pub async fn save_unmanaged_candidate_provider(
    state: State<'_, AppState>,
    snapshot_id: Uuid,
    candidate_id: Uuid,
    name: String,
    default_model: Option<String>,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    let provider = state
        .cli_manager
        .save_unmanaged_candidate(
            &state.oauth,
            &settings,
            UnmanagedCandidateSaveRequest {
                snapshot_id,
                candidate_id,
                name,
                default_model,
            },
        )
        .await?;
    let catalog = state.catalog_cache.catalog().await?;
    Ok(public_with_catalog(&provider, Vec::new(), &catalog))
}

#[tauri::command]
pub async fn list_providers(state: State<'_, AppState>) -> AppResult<Vec<PublicProvider>> {
    let catalog = state.catalog_cache.catalog().await?;
    Ok(state
        .repository
        .list_providers()
        .await?
        .into_iter()
        .map(|provider| decorate_public_provider(provider, &catalog))
        .collect())
}

#[tauri::command]
pub async fn get_provider(
    state: State<'_, AppState>,
    provider_id: Uuid,
) -> AppResult<PublicProvider> {
    let catalog = state.catalog_cache.catalog().await?;
    state
        .repository
        .list_providers()
        .await?
        .into_iter()
        .map(|provider| decorate_public_provider(provider, &catalog))
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| AppError::NotFound(format!("provider {provider_id}")))
}

#[tauri::command]
pub async fn get_provider_secret_detail(
    state: State<'_, AppState>,
    provider_id: Uuid,
) -> AppResult<ProviderProfile> {
    state.repository.get_provider(provider_id).await
}

#[tauri::command]
pub async fn create_provider(
    state: State<'_, AppState>,
    draft: ApiProviderDraft,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    if !settings.plaintext_risk_accepted {
        return Err(AppError::Blocked(
            "plaintext credential risk must be accepted before saving a secret".into(),
        ));
    }
    let catalog = state.catalog_cache.catalog().await?;
    let detach_catalog_identity = validate_api_provider_draft(&catalog, &draft, None)?;
    debug_assert!(!detach_catalog_identity);
    let now = Utc::now();
    let provider = provider_from_draft(Uuid::new_v4(), 1, now, now, draft);
    state.repository.insert_provider(&provider, None).await?;
    Ok(public_with_catalog(&provider, Vec::new(), &catalog))
}

#[tauri::command]
pub async fn create_oauth_provider(
    state: State<'_, AppState>,
    kind: OAuthKind,
    name: String,
    raw_content: String,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    let provider = state
        .oauth
        .create_from_raw(kind, name, raw_content, &settings)
        .await?;
    Ok(provider.public(Vec::new()))
}

#[tauri::command]
pub async fn update_provider(
    state: State<'_, AppState>,
    provider_id: Uuid,
    expected_revision: i64,
    draft: ApiProviderDraft,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let previous = state.repository.get_provider(provider_id).await?;
    if !matches!(previous.data, ProviderData::Api(_)) {
        return Err(AppError::Validation(
            "provider is not endpoint + key".into(),
        ));
    }
    let catalog = state.catalog_cache.catalog().await?;
    let detach_catalog_identity = validate_api_provider_draft(&catalog, &draft, Some(&previous))?;
    let mut provider = provider_from_draft(
        provider_id,
        expected_revision,
        previous.created_at,
        Utc::now(),
        draft,
    );
    if detach_catalog_identity {
        detach_api_catalog_identity(&mut provider);
    }
    if let (ProviderData::Api(previous_api), ProviderData::Api(updated_api)) =
        (&previous.data, &mut provider.data)
    {
        for connection in &mut updated_api.connections {
            let Some(previous_connection) = previous_api
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id)
            else {
                continue;
            };
            if previous_connection.protocol == connection.protocol
                && (detach_catalog_identity
                    || previous_connection.template_endpoint_id == connection.template_endpoint_id)
                && previous_connection.credential_slot_id == connection.credential_slot_id
                && previous_connection.endpoint == connection.endpoint
                && previous_connection.auth_type == connection.auth_type
                && previous_connection.api_key == connection.api_key
                && previous_connection.default_model.trim() == connection.default_model.trim()
            {
                connection.verification = previous_connection.verification.clone();
            }
        }
    }
    state
        .repository
        .update_provider(&provider, expected_revision, None)
        .await?;
    let mut public = public_with_catalog(&provider, Vec::new(), &catalog);
    public.revision = expected_revision + 1;
    Ok(public)
}

#[tauri::command]
pub async fn update_oauth_provider(
    state: State<'_, AppState>,
    provider_id: Uuid,
    expected_revision: i64,
    name: String,
    raw_content: String,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let provider = state
        .oauth
        .update_provider(provider_id, expected_revision, name, raw_content)
        .await?;
    Ok(provider.public(Vec::new()))
}

#[tauri::command]
pub async fn rename_oauth_provider(
    state: State<'_, AppState>,
    provider_id: Uuid,
    expected_revision: i64,
    name: String,
) -> AppResult<PublicProvider> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let mut provider = state.repository.get_provider(provider_id).await?;
    if !matches!(provider.data, ProviderData::Oauth(_)) {
        return Err(AppError::Validation("provider is not OAuth".into()));
    }
    provider.name = name;
    let relative = PathBuf::from("auth")
        .join(provider_id.to_string())
        .join("auth.txt");
    state
        .repository
        .update_provider(&provider, expected_revision, Some(&relative))
        .await?;
    let mut public = provider.public(Vec::new());
    public.revision = expected_revision + 1;
    Ok(public)
}

#[tauri::command]
pub async fn update_oauth_raw_content(
    state: State<'_, AppState>,
    provider_id: Uuid,
    expected_revision: i64,
    raw_content: String,
) -> AppResult<ProviderProfile> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    state
        .oauth
        .update_raw_content(provider_id, expected_revision, raw_content)
        .await
}

#[tauri::command]
pub async fn delete_provider(
    state: State<'_, AppState>,
    provider_id: Uuid,
    expected_revision: i64,
) -> AppResult<()> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let provider = state.repository.get_provider(provider_id).await?;
    state
        .repository
        .delete_provider(provider_id, expected_revision)
        .await?;
    if matches!(provider.data, ProviderData::Oauth(_)) {
        let directory = state.paths.auth.join(provider_id.to_string());
        if directory.parent() == Some(state.paths.auth.as_path()) {
            match tokio::fs::remove_dir_all(directory).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    provider_id: Uuid,
    connection_id: Uuid,
) -> AppResult<Vec<String>> {
    let provider = state.repository.get_provider(provider_id).await?;
    let connection = find_connection(&provider, connection_id)?;
    state.models.list_models(connection).await
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    provider_id: Uuid,
    connection_id: Uuid,
) -> AppResult<()> {
    let provider = state.repository.get_provider(provider_id).await?;
    let expected_revision = provider.revision;
    let connection = find_connection(&provider, connection_id)?.clone();
    match state.models.test_connection(&connection).await {
        Ok(()) => {
            state
                .repository
                .set_connection_verification(
                    provider_id,
                    connection_id,
                    expected_revision,
                    &VerificationInfo {
                        status: crate::domain::VerificationStatus::Valid,
                        verified_at: Some(Utc::now()),
                        error: None,
                    },
                )
                .await?;
            Ok(())
        }
        Err(error) => {
            let message = state.redactor.sanitize(error.to_string());
            state
                .repository
                .set_connection_verification(
                    provider_id,
                    connection_id,
                    expected_revision,
                    &VerificationInfo {
                        status: crate::domain::VerificationStatus::Invalid,
                        verified_at: Some(Utc::now()),
                        error: Some(message.clone()),
                    },
                )
                .await?;
            Err(AppError::Network(message))
        }
    }
}

#[tauri::command]
pub async fn start_oauth_login(
    state: State<'_, AppState>,
    kind: OAuthKind,
    name: String,
    replace_provider_id: Option<Uuid>,
    device_auth: bool,
) -> AppResult<OAuthSessionSnapshot> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    state
        .oauth
        .start_login(kind, name, &settings, replace_provider_id, device_auth)
        .await
}

#[tauri::command]
pub async fn cancel_oauth_login(state: State<'_, AppState>, session_id: Uuid) -> AppResult<()> {
    state.oauth.cancel(session_id).await
}

#[tauri::command]
pub async fn send_oauth_input(
    state: State<'_, AppState>,
    session_id: Uuid,
    input: String,
) -> AppResult<()> {
    state.oauth.send_input(session_id, input).await
}

#[tauri::command]
pub async fn get_oauth_snapshot(
    state: State<'_, AppState>,
    session_id: Uuid,
) -> AppResult<OAuthSessionSnapshot> {
    state.oauth.get_snapshot(session_id).await
}

#[tauri::command]
pub fn open_oauth_browser_url(app: AppHandle, kind: OAuthKind, url: Url) -> AppResult<()> {
    validate_oauth_browser_url(kind, &url)?;
    app.opener()
        .open_url(url.to_string(), None::<String>)
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

#[tauri::command]
pub async fn import_oauth(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: OAuthKind,
    name: String,
    replace_provider_id: Option<Uuid>,
) -> AppResult<Option<PublicProvider>> {
    let selected = app
        .dialog()
        .file()
        .add_filter("OAuth auth", &["json", "txt"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    let provider = state
        .oauth
        .import_from_path(kind, name, &path, &settings, replace_provider_id)
        .await?;
    Ok(Some(provider.public(Vec::new())))
}

#[tauri::command]
pub async fn list_configurations(state: State<'_, AppState>) -> AppResult<Vec<SavedConfiguration>> {
    state.repository.list_configurations().await
}

#[tauri::command]
pub async fn create_configuration(
    state: State<'_, AppState>,
    request: CreateConfigurationRequest,
) -> AppResult<SavedConfiguration> {
    let now = Utc::now();
    let mut configuration = SavedConfiguration {
        id: Uuid::new_v4(),
        name: request.name,
        creation_order: 0,
        revision: 1,
        targets: request.targets,
        last_applied_at: None,
        last_apply_summary: None,
        created_at: now,
        updated_at: now,
    };
    configuration.creation_order = state
        .repository
        .insert_configuration(&configuration)
        .await?;
    Ok(configuration)
}

#[tauri::command]
pub async fn update_configuration(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
    request: CreateConfigurationRequest,
) -> AppResult<SavedConfiguration> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let previous = state.repository.get_configuration(configuration_id).await?;
    let mut configuration = SavedConfiguration {
        id: configuration_id,
        name: request.name,
        creation_order: previous.creation_order,
        revision: expected_revision,
        targets: request.targets,
        last_applied_at: previous.last_applied_at,
        last_apply_summary: previous.last_apply_summary,
        created_at: previous.created_at,
        updated_at: Utc::now(),
    };
    state
        .repository
        .update_configuration(&configuration, expected_revision)
        .await?;
    configuration.revision += 1;
    Ok(configuration)
}

#[tauri::command]
pub async fn duplicate_configuration(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    name: String,
) -> AppResult<SavedConfiguration> {
    let source = state.repository.get_configuration(configuration_id).await?;
    create_configuration(
        state,
        CreateConfigurationRequest {
            name,
            targets: source.targets,
        },
    )
    .await
}

#[tauri::command]
pub async fn rename_configuration(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
    name: String,
) -> AppResult<()> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    state
        .repository
        .rename_configuration(configuration_id, &name, expected_revision)
        .await
}

#[tauri::command]
pub async fn delete_configuration(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
) -> AppResult<()> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    state
        .repository
        .delete_configuration(configuration_id, expected_revision)
        .await
}

fn supports_current_api_connection(
    catalog: &ProviderCatalog,
    cli_id: CliId,
    template_id: Option<&str>,
    template_endpoint_id: Option<&str>,
    protocol: CliProtocol,
) -> bool {
    match (template_id, template_endpoint_id) {
        (Some(template_id), Some(endpoint_id)) => {
            catalog.supports_api_endpoint(cli_id, template_id, endpoint_id)
        }
        (None, None) => catalog.supports_protocol(cli_id, protocol),
        (Some(_template_id), None) => {
            // Older catalog profiles stored a resolved connection without an endpoint ID. Keep
            // that historical snapshot usable without pretending it belongs to a current route.
            catalog.supports_protocol(cli_id, protocol)
        }
        _ => false,
    }
}

#[tauri::command]
pub async fn save_current_as_configuration(
    state: State<'_, AppState>,
    name: String,
) -> AppResult<SavedConfiguration> {
    let catalog = state.catalog_cache.catalog().await?;
    let snapshot = state
        .cli_manager
        .latest_snapshot()
        .await
        .ok_or_else(|| AppError::NotFound("scan current configuration first".into()))?;
    let mut targets = Vec::new();
    for item in snapshot.items {
        let Some(current) = item.current else {
            continue;
        };
        let (Some(provider_id), Some(model)) = (current.managed_provider_id, current.model) else {
            continue;
        };
        let provider = state.repository.get_provider(provider_id).await?;
        match &provider.data {
            ProviderData::Api(api) => {
                if let Some(connection) = api.connections.iter().find(|connection| {
                    current.protocol == Some(connection.protocol)
                        && supports_current_api_connection(
                            &catalog,
                            item.cli_id,
                            provider.template_id.as_deref(),
                            connection.template_endpoint_id.as_deref(),
                            connection.protocol,
                        )
                }) {
                    targets.push(ConfigurationTarget::Api {
                        cli_id: item.cli_id,
                        provider_id,
                        connection_id: connection.id,
                        model,
                    });
                }
            }
            ProviderData::Oauth(_)
                if provider.template_id.as_deref().is_some_and(|template_id| {
                    catalog.supports_auth_template(item.cli_id, template_id)
                }) =>
            {
                targets.push(ConfigurationTarget::Oauth {
                    cli_id: item.cli_id,
                    provider_id,
                    model,
                });
            }
            _ => {}
        }
    }
    if targets.is_empty() {
        return Err(AppError::Blocked(
            "current scan has no managed targets; save unmanaged providers first".into(),
        ));
    }
    create_configuration(state, CreateConfigurationRequest { name, targets }).await
}

#[tauri::command]
pub async fn preview_apply(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
) -> AppResult<crate::domain::ApplyPreview> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    if let Err(error) = state.oauth.refresh_active_bindings(&settings).await {
        tracing::warn!(
            operation = "oauth-refresh-before-preview",
            error = %state.redactor.sanitize(error.to_string()),
            "OAuth refresh failed; continuing with an independent apply preview"
        );
    }
    state.cli_manager.scan(&settings).await;
    state
        .apply
        .preview(configuration_id, expected_revision, &settings)
        .await
}

#[tauri::command]
pub async fn preview_cli_apply(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
    target: ConfigurationTarget,
) -> AppResult<crate::domain::ApplyPreview> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    if let Err(error) = state.oauth.refresh_active_bindings(&settings).await {
        tracing::warn!(
            operation = "oauth-refresh-before-cli-preview",
            error = %state.redactor.sanitize(error.to_string()),
            "OAuth refresh failed; continuing with an independent CLI preview"
        );
    }
    state.cli_manager.scan(&settings).await;
    state
        .apply
        .preview_target(configuration_id, expected_revision, &settings, target)
        .await
}

#[tauri::command]
pub async fn apply_configuration(
    state: State<'_, AppState>,
    configuration_id: Uuid,
    expected_revision: i64,
) -> AppResult<crate::domain::ApplyRunSnapshot> {
    ensure_apply_allowed(state.oauth.has_active_sessions().await)?;
    let mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    if let Err(error) = state.oauth.refresh_active_bindings(&settings).await {
        tracing::warn!(
            operation = "oauth-refresh-before-apply",
            error = %state.redactor.sanitize(error.to_string()),
            "OAuth refresh failed; continuing with an independent apply"
        );
    }
    state.cli_manager.scan(&settings).await;
    let preview = state
        .apply
        .preview(configuration_id, expected_revision, &settings)
        .await?;
    state
        .apply
        .start_with_guard(preview.id, mutation_guard)
        .await
}

#[tauri::command]
pub async fn start_apply(
    state: State<'_, AppState>,
    preview_id: Uuid,
) -> AppResult<crate::domain::ApplyRunSnapshot> {
    ensure_apply_allowed(state.oauth.has_active_sessions().await)?;
    state.apply.start(preview_id).await
}

fn ensure_apply_allowed(oauth_active: bool) -> AppResult<()> {
    if oauth_active {
        return Err(AppError::Blocked("an OAuth login session is active".into()));
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_apply(state: State<'_, AppState>, run_id: Uuid) -> AppResult<()> {
    state.apply.cancel(run_id).await
}

#[tauri::command]
pub async fn retry_apply_items(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> AppResult<crate::domain::ApplyPreview> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    let settings = state.repository.get_settings().await?;
    if let Err(error) = state.oauth.refresh_active_bindings(&settings).await {
        tracing::warn!(
            operation = "oauth-refresh-before-retry",
            error = %state.redactor.sanitize(error.to_string()),
            "OAuth refresh failed; continuing with an independent retry preview"
        );
    }
    state.cli_manager.scan(&settings).await;
    state.apply.retry_preview(run_id, &settings).await
}

#[tauri::command]
pub async fn get_apply_snapshot(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> AppResult<crate::domain::ApplyRunSnapshot> {
    state.apply.snapshot(run_id).await
}

#[tauri::command]
pub async fn list_backups(
    state: State<'_, AppState>,
    cli_id: Option<CliId>,
) -> AppResult<Vec<BackupMetadata>> {
    state.backup.list(cli_id).await
}

#[tauri::command]
pub async fn preview_restore(
    state: State<'_, AppState>,
    backup_id: Uuid,
) -> AppResult<RestorePreview> {
    state.apply.preview_restore(backup_id).await
}

#[tauri::command]
pub async fn restore_backup(
    state: State<'_, AppState>,
    preview_id: Uuid,
) -> AppResult<ScanSnapshot> {
    let settings = state.repository.get_settings().await?;
    state.apply.restore(preview_id, &settings).await?;
    Ok(state.cli_manager.scan(&settings).await)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.repository.get_settings().await
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
    expected_revision: i64,
) -> AppResult<AppSettings> {
    let _mutation_guard = state.apply.try_mutation_guard()?;
    state
        .repository
        .update_settings(&settings, expected_revision)
        .await?;
    let mut updated = settings;
    updated.revision = expected_revision + 1;
    Ok(updated)
}

#[tauri::command]
pub fn set_ui_zoom(app: AppHandle, ui_zoom_percent: u16) -> AppResult<()> {
    validate_ui_zoom_percent(ui_zoom_percent)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::NotFound("main window".into()))?;
    window
        .set_zoom(f64::from(ui_zoom_percent) / 100.0)
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

#[tauri::command]
pub async fn open_app_data_directory(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    app.opener()
        .open_path(
            state.paths.root.to_string_lossy().to_string(),
            None::<String>,
        )
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}

#[tauri::command]
pub async fn check_github_release(app: AppHandle) -> AppResult<ReleaseCheck> {
    let release = check_release(env!("CARGO_PKG_VERSION")).await?;
    if release.update_available {
        app.opener()
            .open_url(release.release_url.to_string(), None::<String>)
            .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))?;
    }
    Ok(release)
}

#[tauri::command]
pub async fn set_frontend_dirty(state: State<'_, AppState>, dirty: bool) -> AppResult<()> {
    state.frontend_dirty.store(dirty, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn get_close_state(state: State<'_, AppState>) -> AppResult<CloseState> {
    Ok(CloseState {
        frontend_dirty: state.frontend_dirty.load(Ordering::SeqCst),
        oauth_active: state.oauth.has_active_sessions().await,
        apply_active: state.apply.has_active_runs().await,
    })
}

#[tauri::command]
pub async fn shutdown_app(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    state.apply.shutdown().await;
    state.oauth.shutdown().await;
    state.safe_to_exit.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

fn decorate_public_provider(
    mut public: PublicProvider,
    catalog: &ProviderCatalog,
) -> PublicProvider {
    if let Some(template_id) = public.template_id.as_deref()
        && let Some(info) = catalog.dynamic_provider_info(template_id)
    {
        public.template_name = Some(info.name.clone());
        public.template_mode = Some("api".into());
        public.template_category = Some("cli-adapter".into());
    }
    public
}

fn public_with_catalog(
    provider: &ProviderProfile,
    referenced_by: Vec<String>,
    catalog: &ProviderCatalog,
) -> PublicProvider {
    decorate_public_provider(provider.public(referenced_by), catalog)
}

fn validate_api_provider_draft(
    catalog: &ProviderCatalog,
    draft: &ApiProviderDraft,
    previous: Option<&ProviderProfile>,
) -> AppResult<bool> {
    let Some(template_id) = draft.template_id.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(false);
    };
    let info = catalog.dynamic_provider_info(template_id);
    let template = catalog.api_template(template_id);
    let matches_current_contract = info.is_some_and(|info| info.selectable)
        && template.is_some_and(|template| draft_matches_template_contract(template, draft));
    let updates_same_saved_template = previous
        .and_then(|provider| provider.template_id.as_deref())
        .is_some_and(|previous_template_id| previous_template_id == template_id);
    let previous_matches_current_contract = previous
        .zip(template)
        .is_some_and(|(provider, template)| provider_matches_template_contract(template, provider));
    if updates_same_saved_template
        && !matches_current_contract
        && !previous_matches_current_contract
    {
        // A saved provider is a resolved snapshot and can outlive or predate its catalog
        // contract. On the next explicit edit, retain its connections but detach the stale
        // catalog identity so it cannot bypass validation while masquerading as a current
        // CLIAdapter template.
        for connection in &draft.connections {
            crate::catalog::resolve_catalog_endpoint(connection.endpoint.as_str())
                .map_err(AppError::Validation)?;
        }
        return Ok(true);
    }
    let Some(info) = info else {
        if template.is_none() {
            return Err(AppError::Validation(format!(
                "unknown API provider template {template_id}"
            )));
        }
        // Legacy templates retain their detailed validation in ProviderProfile::validate.
        return Ok(false);
    };
    if !info.selectable {
        return Err(AppError::Validation(format!(
            "provider {template_id} is unavailable: {}",
            info.disabled_reason
                .as_deref()
                .unwrap_or("unsupported provider")
        )));
    }
    let template = template.ok_or_else(|| {
        AppError::Validation(format!("provider {template_id} has no generated template"))
    })?;
    if draft.connections.len() != template.endpoints.len() {
        return Err(AppError::Validation(format!(
            "CLIAdapter provider {template_id} requires all declared connections"
        )));
    }
    let mut endpoint_ids = std::collections::HashSet::new();
    for connection in &draft.connections {
        let endpoint_id = connection
            .template_endpoint_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "CLIAdapter provider {template_id} requires endpoint IDs"
                ))
            })?;
        if !endpoint_ids.insert(endpoint_id) {
            return Err(AppError::Validation(format!(
                "CLIAdapter provider {template_id} repeats endpoint {endpoint_id}"
            )));
        }
        let expected = template
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == endpoint_id)
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "CLIAdapter provider {template_id} has unknown endpoint {endpoint_id}"
                ))
            })?;
        if connection.protocol != expected.protocol
            || connection.credential_slot_id != expected.credential_slot_id
            || !expected
                .auth_options
                .iter()
                .any(|option| option.auth_type == connection.auth_type)
        {
            return Err(AppError::Validation(format!(
                "connection does not match CLIAdapter provider {template_id} endpoint {endpoint_id}"
            )));
        }
        // Users may intentionally override a template destination, but never its security scheme
        // or embedded credentials.
        crate::catalog::resolve_catalog_endpoint(connection.endpoint.as_str())
            .map_err(AppError::Validation)?;
    }
    Ok(false)
}

fn provider_matches_template_contract(
    template: &crate::catalog::ApiProviderTemplate,
    provider: &ProviderProfile,
) -> bool {
    let ProviderData::Api(api) = &provider.data else {
        return false;
    };
    if api.connections.len() != template.endpoints.len() {
        return false;
    }
    let mut endpoint_ids = std::collections::HashSet::new();
    api.connections.iter().all(|connection| {
        let Some(endpoint_id) = connection
            .template_endpoint_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return false;
        };
        endpoint_ids.insert(endpoint_id)
            && template
                .endpoints
                .iter()
                .find(|endpoint| endpoint.id == endpoint_id)
                .is_some_and(|endpoint| {
                    connection.protocol == endpoint.protocol
                        && connection.credential_slot_id == endpoint.credential_slot_id
                        && endpoint
                            .auth_options
                            .iter()
                            .any(|option| option.auth_type == connection.auth_type)
                })
    })
}

fn draft_matches_template_contract(
    template: &crate::catalog::ApiProviderTemplate,
    draft: &ApiProviderDraft,
) -> bool {
    if draft.connections.len() != template.endpoints.len() {
        return false;
    }
    let mut endpoint_ids = std::collections::HashSet::new();
    draft.connections.iter().all(|connection| {
        let Some(endpoint_id) = connection
            .template_endpoint_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return false;
        };
        endpoint_ids.insert(endpoint_id)
            && template
                .endpoints
                .iter()
                .find(|endpoint| endpoint.id == endpoint_id)
                .is_some_and(|endpoint| {
                    connection.protocol == endpoint.protocol
                        && connection.credential_slot_id == endpoint.credential_slot_id
                        && endpoint
                            .auth_options
                            .iter()
                            .any(|option| option.auth_type == connection.auth_type)
                })
    })
}

fn provider_from_draft(
    id: Uuid,
    revision: i64,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    draft: ApiProviderDraft,
) -> ProviderProfile {
    let mut provider = ProviderProfile {
        id,
        name: draft.name,
        template_id: draft
            .template_id
            .filter(|template_id| !template_id.trim().is_empty()),
        revision,
        created_at,
        updated_at,
        data: ProviderData::Api(ApiProviderData {
            connections: draft
                .connections
                .into_iter()
                .map(|connection| ProviderConnection {
                    id: connection.id.unwrap_or_else(Uuid::new_v4),
                    template_endpoint_id: connection
                        .template_endpoint_id
                        .filter(|id| !id.trim().is_empty()),
                    credential_slot_id: connection.credential_slot_id,
                    protocol: connection.protocol,
                    endpoint: connection.endpoint,
                    auth_type: connection.auth_type,
                    api_key: connection.api_key,
                    default_model: connection.default_model,
                    verification: VerificationInfo::default(),
                })
                .collect(),
        }),
    };
    normalize_provider_credential_kind(&mut provider);
    provider
}

fn detach_api_catalog_identity(provider: &mut ProviderProfile) {
    provider.template_id = None;
    if let ProviderData::Api(api) = &mut provider.data {
        for connection in &mut api.connections {
            connection.template_endpoint_id = None;
        }
    }
}

fn find_connection(
    provider: &ProviderProfile,
    connection_id: Uuid,
) -> AppResult<&ProviderConnection> {
    match &provider.data {
        ProviderData::Api(api) => api
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .ok_or_else(|| AppError::NotFound(format!("connection {connection_id}"))),
        _ => Err(AppError::Validation(
            "provider is not endpoint + key".into(),
        )),
    }
}

#[macro_export]
macro_rules! cliswitch_invoke_handler {
    () => {
        tauri::generate_handler![
            $crate::commands::get_startup_status,
            $crate::commands::open_startup_data_directory,
            $crate::commands::get_app_snapshot,
            $crate::commands::get_catalog_status,
            $crate::commands::list_catalog_providers,
            $crate::commands::update_catalog,
            $crate::commands::scan_clis,
            $crate::commands::select_cli_executable,
            $crate::commands::select_cli_config_directory,
            $crate::commands::save_unmanaged_candidate_provider,
            $crate::commands::list_providers,
            $crate::commands::get_provider,
            $crate::commands::get_provider_secret_detail,
            $crate::commands::create_provider,
            $crate::commands::create_oauth_provider,
            $crate::commands::update_provider,
            $crate::commands::update_oauth_provider,
            $crate::commands::rename_oauth_provider,
            $crate::commands::update_oauth_raw_content,
            $crate::commands::delete_provider,
            $crate::commands::list_models,
            $crate::commands::test_connection,
            $crate::commands::start_oauth_login,
            $crate::commands::cancel_oauth_login,
            $crate::commands::send_oauth_input,
            $crate::commands::get_oauth_snapshot,
            $crate::commands::open_oauth_browser_url,
            $crate::commands::import_oauth,
            $crate::commands::list_configurations,
            $crate::commands::create_configuration,
            $crate::commands::update_configuration,
            $crate::commands::duplicate_configuration,
            $crate::commands::rename_configuration,
            $crate::commands::delete_configuration,
            $crate::commands::save_current_as_configuration,
            $crate::commands::preview_apply,
            $crate::commands::preview_cli_apply,
            $crate::commands::apply_configuration,
            $crate::commands::start_apply,
            $crate::commands::cancel_apply,
            $crate::commands::retry_apply_items,
            $crate::commands::get_apply_snapshot,
            $crate::commands::list_backups,
            $crate::commands::preview_restore,
            $crate::commands::restore_backup,
            $crate::commands::get_settings,
            $crate::commands::update_settings,
            $crate::commands::set_ui_zoom,
            $crate::commands::open_app_data_directory,
            $crate::commands::check_github_release,
            $crate::commands::set_frontend_dirty,
            $crate::commands::get_close_state,
            $crate::commands::shutdown_app
        ]
    };
}

#[allow(dead_code)]
fn _event_type_assertion(event: ApplyProgressEvent) -> ApplyProgressEvent {
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimax_draft(
        template_id: &str,
        key: &str,
        auth_type: ConnectionAuthType,
        connection_id: Uuid,
    ) -> ApiProviderDraft {
        ApiProviderDraft {
            name: "MiniMax fixture".into(),
            template_id: Some(template_id.into()),
            connections: vec![ConnectionDraft {
                id: Some(connection_id),
                template_endpoint_id: Some("anthropic".into()),
                credential_slot_id: "api-key".into(),
                protocol: CliProtocol::AnthropicMessages,
                endpoint: Url::parse("https://api.minimax.io/anthropic/v1").unwrap(),
                auth_type,
                api_key: key.into(),
                default_model: "MiniMax-M2.7".into(),
            }],
        }
    }

    fn one_endpoint_catalog(provider_id: &str) -> ProviderCatalog {
        let value = serde_json::json!([{
            "id": provider_id,
            "name": "Fixture provider",
            "env": ["FIXTURE_API_KEY"],
            "endpoints": [{
                "protocol": "responses",
                "url": "https://fixture.example/v1"
            }]
        }]);
        let source =
            crate::catalog::CliAdapterCatalog::from_json(&serde_json::to_vec(&value).unwrap())
                .unwrap();
        ProviderCatalog::from_cli_adapter(source).unwrap()
    }

    fn response_draft(template_id: &str, endpoint_id: Option<&str>) -> ApiProviderDraft {
        ApiProviderDraft {
            name: "Fixture provider".into(),
            template_id: Some(template_id.into()),
            connections: vec![ConnectionDraft {
                id: Some(Uuid::new_v4()),
                template_endpoint_id: endpoint_id.map(str::to_string),
                credential_slot_id: "api-key".into(),
                protocol: CliProtocol::OpenaiResponses,
                endpoint: Url::parse("https://fixture.example/v1").unwrap(),
                auth_type: ConnectionAuthType::Bearer,
                api_key: "fixture-key".into(),
                default_model: "fixture-model".into(),
            }],
        }
    }

    #[test]
    fn command_provider_drafts_normalize_minimax_kind_before_persistence() {
        let now = Utc::now();
        let token_connection_id = Uuid::new_v4();
        let token_provider = provider_from_draft(
            Uuid::new_v4(),
            1,
            now,
            now,
            minimax_draft(
                "minimax-api",
                "sk-cp-command-fixture",
                ConnectionAuthType::ApiKey,
                token_connection_id,
            ),
        );
        assert_eq!(
            token_provider.template_id.as_deref(),
            Some("minimax-coding-plan")
        );
        let ProviderData::Api(token_api) = &token_provider.data else {
            panic!("command draft should produce an API provider");
        };
        assert_eq!(token_api.connections[0].id, token_connection_id);
        assert_eq!(
            token_api.connections[0].auth_type,
            ConnectionAuthType::Bearer
        );

        let api_provider = provider_from_draft(
            Uuid::new_v4(),
            2,
            now,
            now,
            minimax_draft(
                "minimax-coding-plan",
                "sk-api-command-fixture",
                ConnectionAuthType::Bearer,
                Uuid::new_v4(),
            ),
        );
        assert_eq!(api_provider.template_id.as_deref(), Some("minimax-api"));
        let ProviderData::Api(api) = &api_provider.data else {
            panic!("command draft should produce an API provider");
        };
        assert_eq!(api.connections[0].auth_type, ConnectionAuthType::ApiKey);
    }

    #[test]
    fn current_cli_adapter_routes_require_endpoint_ids_but_old_snapshots_remain_usable() {
        let value = serde_json::json!([{
            "id": "demo",
            "name": "Demo",
            "env": ["DEMO_API_KEY"],
            "endpoints": [{ "protocol": "responses", "url": "https://demo.example/v1" }]
        }]);
        let source =
            crate::catalog::CliAdapterCatalog::from_json(&serde_json::to_vec(&value).unwrap())
                .unwrap();
        let catalog = ProviderCatalog::from_cli_adapter(source).unwrap();

        assert!(supports_current_api_connection(
            &catalog,
            CliId::Codex,
            Some("demo"),
            Some("responses"),
            CliProtocol::OpenaiResponses,
        ));
        assert!(!supports_current_api_connection(
            &catalog,
            CliId::ClaudeCode,
            Some("demo"),
            Some("responses"),
            CliProtocol::OpenaiResponses,
        ));
        // Historical catalog providers had no endpoint identity. Preserve their already-saved
        // connection as long as the target CLI supports its protocol.
        assert!(supports_current_api_connection(
            &catalog,
            CliId::Opencode,
            Some("demo"),
            None,
            CliProtocol::OpenaiChat,
        ));
    }

    #[test]
    fn current_template_update_cannot_bypass_endpoint_contract() {
        let catalog = one_endpoint_catalog("demo");
        let now = Utc::now();
        let previous = provider_from_draft(
            Uuid::new_v4(),
            1,
            now,
            now,
            response_draft("demo", Some("responses")),
        );
        let mut malformed = response_draft("demo", None);
        malformed.connections[0].protocol = CliProtocol::OpenaiChat;

        let error = validate_api_provider_draft(&catalog, &malformed, Some(&previous))
            .unwrap_err()
            .to_string();
        assert!(error.contains("requires endpoint IDs"));
    }

    #[test]
    fn removed_or_changed_saved_template_detaches_to_resolved_custom_connections() {
        let catalog = one_endpoint_catalog("current-provider");
        let now = Utc::now();
        let removed_draft = response_draft("removed-provider", Some("responses"));
        let removed = provider_from_draft(Uuid::new_v4(), 1, now, now, removed_draft.clone());
        assert!(validate_api_provider_draft(&catalog, &removed_draft, Some(&removed)).unwrap());

        let current_catalog = one_endpoint_catalog("changed-provider");
        let mut changed_draft = response_draft("changed-provider", Some("responses"));
        changed_draft.connections.push(ConnectionDraft {
            id: Some(Uuid::new_v4()),
            template_endpoint_id: Some("anthropic-messages".into()),
            credential_slot_id: "api-key".into(),
            protocol: CliProtocol::AnthropicMessages,
            endpoint: Url::parse("https://fixture.example/anthropic").unwrap(),
            auth_type: ConnectionAuthType::ApiKey,
            api_key: "fixture-key".into(),
            default_model: "fixture-model".into(),
        });
        let changed = provider_from_draft(Uuid::new_v4(), 1, now, now, changed_draft.clone());
        assert!(
            validate_api_provider_draft(&current_catalog, &changed_draft, Some(&changed)).unwrap()
        );

        let mut detached = provider_from_draft(
            changed.id,
            changed.revision,
            changed.created_at,
            changed.updated_at,
            changed_draft,
        );
        detach_api_catalog_identity(&mut detached);
        assert_eq!(detached.template_id, None);
        assert!(detached.validate().is_ok());
    }

    #[test]
    fn applying_is_blocked_while_oauth_session_is_active() {
        assert!(ensure_apply_allowed(false).is_ok());
        assert!(matches!(
            ensure_apply_allowed(true),
            Err(AppError::Blocked(message)) if message == "an OAuth login session is active"
        ));
    }
}
