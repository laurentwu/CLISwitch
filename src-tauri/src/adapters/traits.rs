use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CliId, ConfigurationTarget, CurrentCliConfiguration, OAuthKind, ProviderConnection,
        ProviderProfile,
    },
    error::AppResult,
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
        ];
        const PRESENCE_APPROVED: &[&str] = &[
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ];
        let variables = VALUE_APPROVED
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| ((*name).to_string(), value))
            })
            .collect();
        let present_variables = PRESENCE_APPROVED
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .map(|name| (*name).to_string())
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
        self.variables.contains_key(key) || self.present_variables.contains(key)
    }
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
pub struct AdapterReadResult {
    pub current: CurrentCliConfiguration,
    pub unmanaged_candidate: Option<ProviderConnection>,
}

#[derive(Debug, Clone)]
pub struct FileWritePlan {
    pub path: PathBuf,
    pub allowed_root: PathBuf,
    pub source_digest: Option<String>,
    pub target_content: Vec<u8>,
    pub contains_credentials: bool,
    pub opaque_content: bool,
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
    ) -> AppResult<AdapterWritePlan>;
    async fn verify_applied(
        &self,
        paths: &AdapterPaths,
        target: &ConfigurationTarget,
        provider: &ProviderProfile,
    ) -> AppResult<bool>;
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

pub fn namespaced_provider_id(provider_id: uuid::Uuid) -> String {
    format!("cliswitch_{}", provider_id.simple())
}
