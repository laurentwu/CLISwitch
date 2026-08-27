//! Local models.dev snapshot management.
//!
//! The upstream document is treated as data only. It is validated before it is made active and
//! arbitrary provider npm values are never executed by CLISwitch. A checked-in snapshot is always
//! available as an offline fallback; a downloaded snapshot is an optional, private cache.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};

use crate::{
    catalog::{CatalogProviderInfo, MODELS_DEV_URL, ModelsDevCatalog, ProviderCatalog},
    error::{AppError, AppResult},
    filesystem::private_paths::{set_private_directory_permissions, set_private_file_permissions},
    filesystem::{atomic_replace::atomic_replace, atomic_replace::resolve_target},
};

const CACHE_FILE_NAME: &str = "models.dev.json";
const META_FILE_NAME: &str = "models.dev.meta.json";
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_METADATA_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMetadata {
    pub source: CatalogSource,
    pub fetched_at: Option<DateTime<Utc>>,
    pub etag: Option<String>,
    pub digest: String,
    pub provider_count: usize,
    pub model_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogSource {
    Bundled,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    pub source: CatalogSource,
    pub cache_path: PathBuf,
    pub metadata_path: PathBuf,
    pub fetched_at: Option<DateTime<Utc>>,
    pub etag: Option<String>,
    pub digest: String,
    pub provider_count: usize,
    pub model_count: usize,
    pub last_error: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone)]
struct CacheState {
    catalog: ProviderCatalog,
    metadata: CatalogMetadata,
}

#[derive(Debug, Clone)]
pub struct CatalogCacheService {
    root: PathBuf,
    cache_path: PathBuf,
    metadata_path: PathBuf,
    client: reqwest::Client,
    state: Arc<RwLock<CacheState>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl CatalogCacheService {
    pub fn new(root: PathBuf) -> AppResult<Self> {
        match std::fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::Blocked(
                    "models.dev cache directory must not be a symlink".into(),
                ));
            }
            Ok(_) => {
                return Err(AppError::Blocked(
                    "models.dev cache path is not a directory".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let cache_path = root.join(CACHE_FILE_NAME);
        let metadata_path = root.join(META_FILE_NAME);
        let bundled_bytes = include_bytes!("../../catalog/models.dev.json");
        let bundled = ModelsDevCatalog::from_api_json(bundled_bytes)?;
        // Validate the generated compatibility projection before accepting either the bundled
        // snapshot or a downloaded one. This catches broken provider IDs/relations at the same
        // boundary where a refresh would otherwise make them active.
        let bundled_catalog = ProviderCatalog::from_models_dev(bundled.clone())?;
        let bundled_digest = digest_bytes(bundled_bytes);
        let bundled_metadata = serde_json::from_slice::<CatalogMetadata>(include_bytes!(
            "../../catalog/models.dev.meta.json"
        ))
        .ok()
        .filter(|metadata| metadata.digest == bundled_digest);

        let local_candidate = read_bounded_regular_file(&cache_path, MAX_BODY).and_then(|bytes| {
            let catalog = ModelsDevCatalog::from_api_json(&bytes).ok()?;
            let provider_catalog = ProviderCatalog::from_models_dev(catalog.clone()).ok()?;
            Some((catalog, provider_catalog, digest_bytes(&bytes)))
        });
        let (models_dev, catalog, source, cache_digest, metadata_from_disk) = match local_candidate
        {
            Some((models_dev, catalog, digest)) => (
                models_dev,
                catalog,
                CatalogSource::Local,
                Some(digest),
                read_metadata(&metadata_path).ok(),
            ),
            None => (bundled, bundled_catalog, CatalogSource::Bundled, None, None),
        };
        let metadata_digest = cache_digest
            .clone()
            .unwrap_or_else(|| bundled_digest.clone());
        // Sidecar timestamps/ETags are meaningful only when they describe the exact bytes which
        // won the startup race. A copied or truncated sidecar is ignored wholesale rather than
        // leaking a stale fetch time or sending an unrelated conditional request upstream.
        let trusted_sidecar = if source == CatalogSource::Local {
            metadata_from_disk.filter(|metadata| metadata.digest == metadata_digest)
        } else {
            bundled_metadata
        };
        let disk_metadata = trusted_sidecar.unwrap_or_else(|| CatalogMetadata {
            source,
            fetched_at: None,
            etag: None,
            digest: metadata_digest.clone(),
            provider_count: models_dev.provider_count(),
            model_count: models_dev.model_count(),
            last_error: None,
        });
        let metadata = CatalogMetadata {
            source,
            fetched_at: disk_metadata.fetched_at,
            // An ETag is valid only for the exact bytes it was received with. If the sidecar is
            // stale or was copied independently, discard it rather than sending a misleading
            // conditional request.
            etag: (source == CatalogSource::Local)
                .then_some(disk_metadata.etag)
                .flatten(),
            digest: metadata_digest,
            provider_count: models_dev.provider_count(),
            model_count: models_dev.model_count(),
            last_error: disk_metadata.last_error,
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("CLISwitch/0.1")
            .build()
            .map_err(|error| AppError::Network(error.to_string()))?;
        Ok(Self {
            root,
            cache_path,
            metadata_path,
            client,
            state: Arc::new(RwLock::new(CacheState { catalog, metadata })),
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn catalog(&self) -> AppResult<ProviderCatalog> {
        let state = self.state.read().await;
        Ok(state.catalog.clone())
    }

    pub async fn providers(&self) -> AppResult<Vec<CatalogProviderInfo>> {
        let state = self.state.read().await;
        state.catalog.provider_info.clone().ok_or_else(|| {
            AppError::Serialization("active models.dev catalog has no provider information".into())
        })
    }

    pub async fn models(
        &self,
        provider_id: &str,
    ) -> AppResult<Vec<crate::catalog::CatalogModelInfo>> {
        let state = self.state.read().await;
        let info = state
            .catalog
            .dynamic_provider_info(provider_id)
            .ok_or_else(|| AppError::NotFound(format!("catalog provider {provider_id}")))?;
        Ok(info.models.clone())
    }

    pub async fn status(&self) -> CatalogStatus {
        let state = self.state.read().await;
        CatalogStatus {
            source: state.metadata.source,
            cache_path: self.cache_path.clone(),
            metadata_path: self.metadata_path.clone(),
            fetched_at: state.metadata.fetched_at,
            etag: state.metadata.etag.clone(),
            digest: state.metadata.digest.clone(),
            provider_count: state.metadata.provider_count,
            model_count: state.metadata.model_count,
            last_error: state.metadata.last_error.clone(),
            update_available: false,
        }
    }

    pub async fn refresh(&self) -> AppResult<CatalogStatus> {
        self.refresh_from(MODELS_DEV_URL).await
    }

    async fn refresh_from(&self, source_url: &str) -> AppResult<CatalogStatus> {
        let _refresh_guard = self.refresh_lock.lock().await;
        let previous = self.state.read().await.metadata.clone();
        let response = match self
            .refresh_request(source_url, previous.etag.as_deref())
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return self.record_error(error.to_string()).await,
        };
        if let Some(status) = self
            .process_response_head(response.status(), response.content_length())
            .await?
        {
            return Ok(status);
        }
        let response_etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .map(str::to_owned);
        let bytes = match bounded_body(response).await {
            Ok(bytes) => bytes,
            Err(error) => return self.record_error(error.to_string()).await,
        };
        self.activate_download(&bytes, response_etag).await
    }

    fn refresh_request(&self, source_url: &str, etag: Option<&str>) -> reqwest::RequestBuilder {
        let request = self
            .client
            .get(source_url)
            .header(header::ACCEPT, "application/json");
        match etag {
            Some(etag) => request.header(header::IF_NONE_MATCH, etag),
            None => request,
        }
    }

    async fn process_response_head(
        &self,
        status: StatusCode,
        content_length: Option<u64>,
    ) -> AppResult<Option<CatalogStatus>> {
        if status == StatusCode::NOT_MODIFIED {
            // A successful conditional request also confirms that any previous transient error
            // is no longer current. Keep the existing bytes/ETag, but clear that diagnostic.
            self.state.write().await.metadata.last_error = None;
            return Ok(Some(self.status().await));
        }
        if status.is_redirection() {
            return self
                .record_error("models.dev returned a redirect".into())
                .await;
        }
        if !status.is_success() {
            return self
                .record_error(format!("models.dev returned HTTP {}", status.as_u16()))
                .await;
        }
        if content_length.is_some_and(|length| length > MAX_BODY as u64) {
            return self
                .record_error("models.dev response is too large".into())
                .await;
        }
        Ok(None)
    }

    async fn activate_download(
        &self,
        bytes: &[u8],
        response_etag: Option<String>,
    ) -> AppResult<CatalogStatus> {
        if bytes.len() > MAX_BODY {
            return self
                .record_error("models.dev response is too large".into())
                .await;
        }
        let models_dev = match ModelsDevCatalog::from_api_json(bytes) {
            Ok(catalog) => catalog,
            Err(error) => return self.record_error(error.to_string()).await,
        };
        let catalog = match ProviderCatalog::from_models_dev(models_dev.clone()) {
            Ok(catalog) => catalog,
            Err(error) => return self.record_error(error.to_string()).await,
        };
        let digest = digest_bytes(bytes);
        let fetched_at = Utc::now();
        if let Err(error) = self
            .write_cache(
                bytes,
                &CatalogMetadata {
                    source: CatalogSource::Local,
                    fetched_at: Some(fetched_at),
                    etag: response_etag.clone(),
                    digest: digest.clone(),
                    provider_count: models_dev.provider_count(),
                    model_count: models_dev.model_count(),
                    last_error: None,
                },
            )
            .await
        {
            return self.record_error(error.to_string()).await;
        }
        let metadata = CatalogMetadata {
            source: CatalogSource::Local,
            fetched_at: Some(fetched_at),
            etag: response_etag,
            digest,
            provider_count: models_dev.provider_count(),
            model_count: models_dev.model_count(),
            last_error: None,
        };
        *self.state.write().await = CacheState { catalog, metadata };
        Ok(self.status().await)
    }

    async fn record_error<T>(&self, message: String) -> AppResult<T> {
        let mut state = self.state.write().await;
        state.metadata.last_error = Some(message.clone());
        Err(AppError::Network(message))
    }

    async fn write_cache(&self, bytes: &[u8], metadata: &CatalogMetadata) -> AppResult<()> {
        ensure_cache_root(&self.root).await?;
        tokio::fs::create_dir_all(&self.root).await?;
        set_private_directory_permissions(&self.root).await?;
        ensure_cache_target(&self.cache_path).await?;
        let metadata_bytes = serde_json::to_vec_pretty(metadata)?;
        if metadata_bytes.len() > MAX_METADATA_BODY {
            return Err(AppError::Serialization(
                "models.dev metadata is too large".into(),
            ));
        }
        // Check both destinations before replacing either one. Each replacement is atomic; this
        // preflight also prevents a substituted metadata symlink from leaving a half-updated pair.
        ensure_cache_target(&self.metadata_path).await?;
        let target = resolve_target(&self.cache_path, &self.root).await?;
        atomic_replace(&target, bytes).await?;
        set_private_file_permissions(&self.cache_path).await?;
        let metadata_target = resolve_target(&self.metadata_path, &self.root).await?;
        atomic_replace(&metadata_target, &metadata_bytes).await?;
        set_private_file_permissions(&self.metadata_path).await?;
        Ok(())
    }
}

fn read_metadata(path: &Path) -> AppResult<CatalogMetadata> {
    let bytes = read_bounded_regular_file(path, MAX_METADATA_BODY).ok_or_else(|| {
        AppError::Serialization("models.dev metadata is missing or not a regular file".into())
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Reads a bounded regular file without following a symlink. Cache files are application-owned
/// state, so a malformed, oversized, or substituted path is treated as an absent cache and the
/// bundled snapshot remains the safe fallback.
fn read_bounded_regular_file(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

async fn ensure_cache_root(root: &Path) -> AppResult<()> {
    match tokio::fs::symlink_metadata(root).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::Blocked(
            "models.dev cache directory must not be a symlink".into(),
        )),
        Ok(_) => Err(AppError::Blocked(
            "models.dev cache path is not a directory".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn ensure_cache_target(path: &Path) -> AppResult<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::Blocked(
            "models.dev cache file must not be a symlink".into(),
        )),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(AppError::Blocked(
            "models.dev cache target is not a regular file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

async fn bounded_body(response: reqwest::Response) -> AppResult<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::Network(error.to_string()))?;
        append_bounded(&mut output, &chunk, MAX_BODY)?;
    }
    Ok(output)
}

fn append_bounded(output: &mut Vec<u8>, chunk: &[u8], limit: usize) -> AppResult<()> {
    if output
        .len()
        .checked_add(chunk.len())
        .is_none_or(|length| length > limit)
    {
        return Err(AppError::Network("models.dev response is too large".into()));
    }
    output.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn snapshot_bytes(provider_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            (provider_id): {
                "env": ["FIXTURE_API_KEY"],
                "npm": "@ai-sdk/openai-compatible",
                "api": format!("https://{provider_id}.example/v1"),
                "name": format!("{provider_id} provider"),
                "models": {
                    "fixture-model": { "name": "Fixture model" }
                }
            }
        }))
        .unwrap()
    }

    fn write_local_snapshot(
        root: &Path,
        bytes: &[u8],
        etag: Option<&str>,
        last_error: Option<&str>,
    ) {
        let catalog = ModelsDevCatalog::from_api_json(bytes).unwrap();
        std::fs::write(root.join(CACHE_FILE_NAME), bytes).unwrap();
        std::fs::write(
            root.join(META_FILE_NAME),
            serde_json::to_vec_pretty(&CatalogMetadata {
                source: CatalogSource::Local,
                fetched_at: Some(Utc::now()),
                etag: etag.map(str::to_owned),
                digest: digest_bytes(bytes),
                provider_count: catalog.provider_count(),
                model_count: catalog.model_count(),
                last_error: last_error.map(str::to_owned),
            })
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn bundled_snapshot_is_valid_and_contains_core_providers() {
        let catalog = ModelsDevCatalog::bundled().unwrap();
        assert!(catalog.provider("openai").is_some());
        assert!(catalog.provider("anthropic").is_some());
        assert!(catalog.provider_count() >= 5);
    }

    #[tokio::test]
    async fn invalid_local_cache_falls_back_to_bundled_snapshot() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(CACHE_FILE_NAME), b"not-json").unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        let status = service.status().await;
        assert_eq!(status.source, CatalogSource::Bundled);
        assert!(
            service
                .providers()
                .await
                .unwrap()
                .iter()
                .any(|p| p.id == "openai")
        );
    }

    #[tokio::test]
    async fn oversized_local_cache_falls_back_to_bundled_snapshot() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join(CACHE_FILE_NAME);
        let bytes = vec![b' '; MAX_BODY + 1];
        std::fs::write(file, bytes).unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        assert_eq!(service.status().await.source, CatalogSource::Bundled);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_local_cache_is_not_followed() {
        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join(CACHE_FILE_NAME);
        std::fs::write(&target, include_bytes!("../../catalog/models.dev.json")).unwrap();
        std::os::unix::fs::symlink(&target, temp.path().join(CACHE_FILE_NAME)).unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        assert_eq!(service.status().await.source, CatalogSource::Bundled);
    }

    #[tokio::test]
    async fn refresh_validates_writes_and_activates_one_consistent_snapshot() {
        let temp = TempDir::new().unwrap();
        let bytes = snapshot_bytes("refreshed");
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();

        let status = service
            .activate_download(&bytes, Some("\"refreshed-etag\"".into()))
            .await
            .unwrap();

        assert_eq!(status.source, CatalogSource::Local);
        assert_eq!(status.etag.as_deref(), Some("\"refreshed-etag\""));
        assert_eq!(status.digest, digest_bytes(&bytes));
        assert_eq!(std::fs::read(&service.cache_path).unwrap(), bytes);
        let metadata = read_metadata(&service.metadata_path).unwrap();
        assert_eq!(metadata.digest, status.digest);
        assert_eq!(metadata.etag, status.etag);
        assert_eq!(metadata.provider_count, status.provider_count);
        assert_eq!(metadata.model_count, status.model_count);
        assert!(
            service
                .catalog()
                .await
                .unwrap()
                .dynamic_provider_info("refreshed")
                .is_some()
        );
    }

    #[tokio::test]
    async fn refresh_uses_etag_and_304_keeps_the_active_snapshot() {
        let temp = TempDir::new().unwrap();
        let bytes = snapshot_bytes("cached");
        write_local_snapshot(
            temp.path(),
            &bytes,
            Some("\"cached-etag\""),
            Some("previous transient error"),
        );
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        let previous = service.status().await;
        let request = service
            .refresh_request("https://models.dev/api.json", previous.etag.as_deref())
            .build()
            .unwrap();
        assert_eq!(
            request
                .headers()
                .get(header::IF_NONE_MATCH)
                .and_then(|value| value.to_str().ok()),
            Some("\"cached-etag\"")
        );

        let status = service
            .process_response_head(StatusCode::NOT_MODIFIED, None)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(status.source, CatalogSource::Local);
        assert_eq!(status.digest, previous.digest);
        assert_eq!(status.etag, previous.etag);
        assert_eq!(status.fetched_at, previous.fetched_at);
        assert!(status.last_error.is_none());
        assert_eq!(std::fs::read(&service.cache_path).unwrap(), bytes);
    }

    #[tokio::test]
    async fn refresh_rejects_redirects_without_changing_state_or_disk() {
        let temp = TempDir::new().unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        let previous = service.status().await;

        let error = service
            .process_response_head(StatusCode::FOUND, None)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("redirect"));
        let status = service.status().await;
        assert_eq!(status.digest, previous.digest);
        assert!(
            status
                .last_error
                .as_deref()
                .is_some_and(|value| value.contains("redirect"))
        );
        assert!(!service.cache_path.exists());
        assert!(!service.metadata_path.exists());
    }

    #[tokio::test]
    async fn refresh_rejects_an_oversized_response_without_changing_state_or_disk() {
        let temp = TempDir::new().unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        let previous = service.status().await;

        let error = service
            .process_response_head(StatusCode::OK, Some(MAX_BODY as u64 + 1))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("too large"));
        assert_eq!(service.status().await.digest, previous.digest);
        assert!(!service.cache_path.exists());
        assert!(!service.metadata_path.exists());
    }

    #[test]
    fn streamed_body_limit_applies_without_a_content_length_header() {
        let mut output = b"abc".to_vec();
        append_bounded(&mut output, b"d", 4).unwrap();
        let error = append_bounded(&mut output, b"e", 4).unwrap_err();
        assert!(error.to_string().contains("too large"));
        assert_eq!(output, b"abcd");
    }

    #[tokio::test]
    async fn refresh_validation_failure_preserves_the_previous_cache_and_catalog() {
        let temp = TempDir::new().unwrap();
        let previous_bytes = snapshot_bytes("previous");
        write_local_snapshot(temp.path(), &previous_bytes, Some("\"previous\""), None);
        let previous_metadata = std::fs::read(temp.path().join(META_FILE_NAME)).unwrap();
        let service = CatalogCacheService::new(temp.path().to_path_buf()).unwrap();
        let previous_status = service.status().await;

        let error = service
            .activate_download(b"{}", Some("\"invalid\"".into()))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("contains no providers"));
        assert_eq!(service.status().await.digest, previous_status.digest);
        assert_eq!(std::fs::read(&service.cache_path).unwrap(), previous_bytes);
        assert_eq!(
            std::fs::read(&service.metadata_path).unwrap(),
            previous_metadata
        );
        let catalog = service.catalog().await.unwrap();
        assert!(catalog.dynamic_provider_info("previous").is_some());
    }

    #[test]
    fn unknown_model_status_is_preserved_as_a_disabled_hint() {
        let value = serde_json::json!({
            "demo": {
                "env": ["DEMO_API_KEY"],
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://demo.example/v1",
                "name": "Demo",
                "models": {
                    "future-model": { "name": "Future", "status": "preview" }
                }
            }
        });
        let models = ModelsDevCatalog::from_api_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let info = models.provider_info().pop().unwrap();
        assert_eq!(info.id, "demo");
        assert!(!info.models[0].selectable);
        assert_eq!(info.models[0].status.as_deref(), Some("preview"));
    }

    #[test]
    fn provider_without_an_environment_name_is_visible_but_not_selectable() {
        let value = serde_json::json!({
            "demo": {
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://demo.example/v1",
                "name": "Demo",
                "models": { "model": { "name": "Model" } }
            }
        });
        let models = ModelsDevCatalog::from_api_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let info = models.provider_info().pop().unwrap();
        assert_eq!(info.id, "demo");
        assert!(!info.selectable);
        assert!(
            info.disabled_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("environment"))
        );
    }

    #[test]
    fn unsafe_upstream_endpoint_is_not_exposed_to_the_renderer() {
        let value = serde_json::json!({
            "demo": {
                "env": ["DEMO_API_KEY"],
                "npm": "@ai-sdk/openai-compatible",
                "api": "https://user:secret@demo.example/v1",
                "name": "Demo",
                "models": { "model": { "name": "Model" } }
            }
        });
        let models = ModelsDevCatalog::from_api_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let info = models.provider_info().pop().unwrap();
        assert!(info.api.is_none());
        assert!(info.endpoint.is_none());
        assert!(!info.selectable);
    }

    #[test]
    fn endpoint_policy_rejects_non_loopback_http() {
        assert!(crate::catalog::resolve_catalog_endpoint("http://example.test/v1").is_err());
        assert!(crate::catalog::resolve_catalog_endpoint("http://127.0.0.1:11434/v1").is_ok());
    }
}
