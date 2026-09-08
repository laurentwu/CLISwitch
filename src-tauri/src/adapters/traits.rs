use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CliId, ConfigurationTarget, CurrentCliConfiguration, OAuthKind, ProviderConnection,
        ProviderProfile,
    },
    error::{AppError, AppResult},
    filesystem::{atomic_replace::canonicalize_allow_missing, digest::bytes_digest},
};

#[derive(Debug, Clone)]
pub struct HostEnvironment {
    pub home: PathBuf,
    pub variables: BTreeMap<String, String>,
    pub present_variables: HashSet<String>,
    pub os: String,
}

impl HostEnvironment {
    pub fn capture() -> AppResult<Self> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .ok_or_else(|| {
                crate::error::AppError::Validation("home directory is not available".into())
            })?;
        if !home.is_absolute() {
            return Err(crate::error::AppError::Validation(
                "home directory must be an absolute path".into(),
            ));
        }
        const VALUE_APPROVED: &[&str] = &[
            "PATH",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "APPDATA",
            "LOCALAPPDATA",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_DIR",
            "QWEN_HOME",
        ];
        let variables = VALUE_APPROVED
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| ((*name).to_string(), value))
            })
            .collect();
        let present_variables = std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .filter(|name| valid_environment_name(name))
            .collect();
        Ok(Self {
            home,
            variables,
            present_variables,
            os: std::env::consts::OS.to_string(),
        })
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.variables.get(key).map(String::as_str)
    }

    pub fn absolute_path(&self, key: &str) -> Option<PathBuf> {
        self.value(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }

    pub fn is_present(&self, key: &str) -> bool {
        if self.os == "windows" {
            self.variables
                .keys()
                .any(|name| name.eq_ignore_ascii_case(key))
                || self
                    .present_variables
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(key))
        } else {
            self.variables.contains_key(key) || self.present_variables.contains(key)
        }
    }
}

fn valid_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterMetadata {
    pub cli_id: CliId,
    pub display_name: String,
    pub command: String,
    pub schema_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct AdapterPaths {
    pub config_directory: PathBuf,
    pub config_file: PathBuf,
    pub auth_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AdapterApiCandidate {
    pub source_provider_id: String,
    pub suggested_name: String,
    pub template_id: Option<String>,
    pub connection: ProviderConnection,
    pub available_models: Vec<String>,
    pub default_model: Option<String>,
    pub is_current: bool,
    pub model_routed: bool,
}

#[derive(Debug, Clone)]
pub struct AdapterReadResult {
    pub current: CurrentCliConfiguration,
    pub unmanaged_api_candidates: Vec<AdapterApiCandidate>,
    pub scan_status_hint: Option<crate::domain::ScanStatus>,
}

#[derive(Clone)]
pub struct FileWritePlan {
    pub path: PathBuf,
    pub allowed_root: PathBuf,
    pub source_content: Option<Vec<u8>>,
    pub source_digest: Option<String>,
    pub target_content: Vec<u8>,
    pub contains_credentials: bool,
    pub opaque_content: bool,
}

impl std::fmt::Debug for FileWritePlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWritePlan")
            .field("path", &self.path)
            .field("allowed_root", &self.allowed_root)
            .field("source_exists", &self.source_content.is_some())
            .field("source_digest", &self.source_digest)
            .field("target_size", &self.target_content.len())
            .field("contains_credentials", &self.contains_credentials)
            .field("opaque_content", &self.opaque_content)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct AdapterWritePlan {
    pub cli_id: CliId,
    pub files: Vec<FileWritePlan>,
    pub warning: Option<String>,
}

#[async_trait]
pub trait CliAdapter: Send + Sync {
    fn metadata(&self) -> AdapterMetadata;
    fn resolve_paths(&self, environment: &HostEnvironment, manual: Option<PathBuf>)
    -> AdapterPaths;
    async fn read_current(
        &self,
        paths: &AdapterPaths,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterReadResult>;
    async fn plan_write(
        &self,
        paths: &AdapterPaths,
        target: &ConfigurationTarget,
        provider: &ProviderProfile,
        environment: &HostEnvironment,
    ) -> AppResult<AdapterWritePlan>;
    async fn verify_applied(&self, plan: &AdapterWritePlan) -> AppResult<bool> {
        for file in &plan.files {
            let current = match read_file_snapshot(&file.path, &file.allowed_root).await? {
                (Some(current), _) => current,
                (None, _) => return Ok(false),
            };
            if bytes_digest(&current) != bytes_digest(&file.target_content) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn oauth_kind(&self) -> Option<OAuthKind>;
    fn validate_imported_auth(&self, bytes: &[u8]) -> AppResult<Option<String>>;
    fn fixed_oauth_command(
        &self,
        executable: PathBuf,
        isolated_home: PathBuf,
    ) -> AppResult<FixedOAuthCommand>;
}

#[derive(Debug, Clone)]
pub struct FixedOAuthCommand {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub artifact: PathBuf,
}

pub async fn read_optional(path: &std::path::Path, default: &str) -> AppResult<String> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default.to_string()),
        Err(error) => Err(error.into()),
    }
}

/// Reads a write-plan source exactly once after a non-mutating containment and file-type check.
/// The returned digest always belongs to the returned bytes.
pub async fn read_file_snapshot(
    path: &Path,
    allowed_root: &Path,
) -> AppResult<(Option<Vec<u8>>, Option<String>)> {
    let resolved_root = canonicalize_allow_missing(allowed_root).await?;
    let candidate = canonicalize_allow_missing(path).await?;
    if !candidate.starts_with(&resolved_root) {
        return Err(AppError::Blocked(
            "resolved configuration path is outside the approved directory".into(),
        ));
    }
    let bytes = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let resolved = tokio::fs::canonicalize(path).await.map_err(|error| {
                AppError::Blocked(format!("cannot resolve configuration symlink: {error}"))
            })?;
            if !resolved.starts_with(&resolved_root) {
                return Err(AppError::Blocked(
                    "resolved configuration path is outside the approved directory".into(),
                ));
            }
            if !tokio::fs::metadata(&resolved).await?.is_file() {
                return Err(AppError::Blocked(
                    "symlink target is not a regular file".into(),
                ));
            }
            Some(tokio::fs::read(resolved).await?)
        }
        Ok(metadata) if metadata.is_file() => Some(tokio::fs::read(path).await?),
        Ok(_) => return Err(AppError::Blocked("target is not a regular file".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let digest = bytes.as_deref().map(bytes_digest);
    Ok((bytes, digest))
}

pub fn namespaced_provider_id(provider_id: uuid::Uuid) -> String {
    format!("cliswitch_{}", provider_id.simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_presence_uses_windows_case_rules_only_on_windows() {
        let mut environment = HostEnvironment {
            home: PathBuf::from("/fixture"),
            variables: BTreeMap::from([("APPDATA".into(), "fixture".into())]),
            present_variables: HashSet::from(["openai_api_key".into()]),
            os: "windows".into(),
        };

        assert!(environment.is_present("appdata"));
        assert!(environment.is_present("OPENAI_API_KEY"));

        environment.os = "linux".into();
        assert!(!environment.is_present("appdata"));
        assert!(!environment.is_present("OPENAI_API_KEY"));
        assert!(environment.is_present("APPDATA"));
        assert!(environment.is_present("openai_api_key"));
    }

    #[tokio::test]
    async fn source_snapshot_is_non_mutating_and_hashes_the_same_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("missing-config");
        let path = allowed.join("nested").join("config.json");
        let (content, digest) = read_file_snapshot(&path, &allowed).await.unwrap();
        assert_eq!(content, None);
        assert_eq!(digest, None);
        assert!(!allowed.exists());

        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"fixture").await.unwrap();
        let (content, digest) = read_file_snapshot(&path, &allowed).await.unwrap();
        assert_eq!(content.as_deref(), Some(b"fixture".as_slice()));
        assert_eq!(digest, Some(bytes_digest(b"fixture")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn source_snapshot_rejects_a_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("config");
        tokio::fs::create_dir_all(&allowed).await.unwrap();
        let outside = temp.path().join("outside.json");
        tokio::fs::write(&outside, b"fixture").await.unwrap();
        let link = allowed.join("config.json");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let error = read_file_snapshot(&link, &allowed).await.unwrap_err();
        assert!(matches!(error, AppError::Blocked(_)));
    }
}
