use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliId {
    ClaudeCode,
    Codex,
    Opencode,
}

impl CliId {
    pub const ALL: [Self; 3] = [Self::ClaudeCode, Self::Codex, Self::Opencode];

    pub const fn command(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Opencode => "OpenCode",
        }
    }
}

impl fmt::Display for CliId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
        })
    }
}

impl FromStr for CliId {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::Opencode),
            _ => Err(AppError::Validation(format!("unknown CLI id: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliProtocol {
    OpenaiChat,
    OpenaiResponses,
    AnthropicMessages,
}

impl fmt::Display for CliProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenaiChat => "openai-chat",
            Self::OpenaiResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
        })
    }
}

impl FromStr for CliProtocol {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai-chat" => Ok(Self::OpenaiChat),
            "openai-responses" => Ok(Self::OpenaiResponses),
            "anthropic-messages" => Ok(Self::AnthropicMessages),
            _ => Err(AppError::Validation(format!("unknown protocol: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionAuthType {
    ApiKey,
    Bearer,
}

impl fmt::Display for ConnectionAuthType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ApiKey => "api-key",
            Self::Bearer => "bearer",
        })
    }
}

impl FromStr for ConnectionAuthType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "api-key" => Ok(Self::ApiKey),
            "bearer" => Ok(Self::Bearer),
            _ => Err(AppError::Validation(format!("unknown auth type: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OAuthKind {
    Anthropic,
    Codex,
}

impl OAuthKind {
    pub const fn target_cli(self) -> CliId {
        match self {
            Self::Anthropic => CliId::ClaudeCode,
            Self::Codex => CliId::Codex,
        }
    }
}

impl fmt::Display for OAuthKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Anthropic => "anthropic",
            Self::Codex => "codex",
        })
    }
}

impl FromStr for OAuthKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "anthropic" => Ok(Self::Anthropic),
            "codex" => Ok(Self::Codex),
            _ => Err(AppError::Validation(format!("unknown OAuth kind: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationStatus {
    NeverTested,
    Valid,
    Invalid,
    NotOnlineVerified,
    UserModifiedUnverified,
}

impl fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NeverTested => "never-tested",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::NotOnlineVerified => "not-online-verified",
            Self::UserModifiedUnverified => "user-modified-unverified",
        })
    }
}

impl FromStr for VerificationStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "never-tested" => Ok(Self::NeverTested),
            "valid" => Ok(Self::Valid),
            "invalid" => Ok(Self::Invalid),
            "not-online-verified" => Ok(Self::NotOnlineVerified),
            "user-modified-unverified" => Ok(Self::UserModifiedUnverified),
            _ => Err(AppError::Validation(format!(
                "unknown verification status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationInfo {
    pub status: VerificationStatus,
    pub verified_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

impl Default for VerificationInfo {
    fn default() -> Self {
        Self {
            status: VerificationStatus::NeverTested,
            verified_at: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnection {
    pub id: Uuid,
    pub template_endpoint_id: Option<String>,
    pub credential_slot_id: String,
    pub protocol: CliProtocol,
    pub endpoint: Url,
    pub auth_type: ConnectionAuthType,
    pub api_key: String,
    pub default_model: String,
    pub verification: VerificationInfo,
}

impl ProviderConnection {
    pub fn validate(&self) -> AppResult<()> {
        if self.credential_slot_id.trim().is_empty() {
            return Err(AppError::Validation(
                "credential slot ID is required".into(),
            ));
        }
        if !matches!(self.endpoint.scheme(), "http" | "https") {
            return Err(AppError::Validation(
                "endpoint must use HTTP or HTTPS".into(),
            ));
        }
        if !self.endpoint.username().is_empty() || self.endpoint.password().is_some() {
            return Err(AppError::Validation(
                "endpoint must not contain embedded credentials".into(),
            ));
        }
        if self.endpoint.host_str().is_none() {
            return Err(AppError::Validation("endpoint must include a host".into()));
        }
        if self.protocol != CliProtocol::AnthropicMessages
            && self.auth_type != ConnectionAuthType::Bearer
        {
            return Err(AppError::Validation(
                "OpenAI-compatible protocols require bearer authentication".into(),
            ));
        }
        if self.api_key.trim().is_empty() {
            return Err(AppError::Validation("API key is required".into()));
        }
        if self.default_model.trim().is_empty() {
            return Err(AppError::Validation("default model is required".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProviderData {
    pub connections: Vec<ProviderConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProviderData {
    pub oauth_kind: OAuthKind,
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub raw_content: String,
    pub digest: String,
    pub manually_modified: bool,
    pub verification: VerificationInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "profileType", rename_all = "kebab-case")]
pub enum ProviderData {
    Api(ApiProviderData),
    Oauth(OAuthProviderData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: Uuid,
    pub name: String,
    pub template_id: Option<String>,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub data: ProviderData,
}

impl ProviderProfile {
    pub fn validate(&self) -> AppResult<()> {
        normalize_name(&self.name)?;
        match &self.data {
            ProviderData::Api(api) => {
                if api.connections.is_empty() {
                    return Err(AppError::Validation(
                        "API provider requires at least one connection".into(),
                    ));
                }
                let mut endpoint_ids = std::collections::HashSet::new();
                let mut slot_secrets = std::collections::HashMap::new();
                for connection in &api.connections {
                    connection.validate()?;
                    if !endpoint_ids.insert(connection.id) {
                        return Err(AppError::Validation(
                            "a provider cannot repeat an endpoint ID".into(),
                        ));
                    }
                    if let Some(previous) = slot_secrets.insert(
                        connection.credential_slot_id.as_str(),
                        connection.api_key.as_str(),
                    ) && previous != connection.api_key.as_str()
                    {
                        return Err(AppError::Validation(format!(
                            "connections sharing credential slot {} must use the same secret",
                            connection.credential_slot_id
                        )));
                    }
                }
                if let Some(template_id) = self.template_id.as_deref() {
                    let catalog = crate::catalog::embedded_catalog()?;
                    let template = catalog.api_template(template_id).ok_or_else(|| {
                        AppError::Validation(format!("unknown API provider template {template_id}"))
                    })?;
                    if api.connections.len() != template.endpoints.len() {
                        return Err(AppError::Validation(format!(
                            "provider template {template_id} requires all configured endpoints"
                        )));
                    }
                    let mut template_endpoint_ids = std::collections::HashSet::new();
                    for connection in &api.connections {
                        let endpoint_id =
                            connection.template_endpoint_id.as_deref().ok_or_else(|| {
                                AppError::Validation(
                                    "template provider endpoint is missing its template ID".into(),
                                )
                            })?;
                        if !template_endpoint_ids.insert(endpoint_id) {
                            return Err(AppError::Validation(format!(
                                "provider repeats template endpoint {endpoint_id}"
                            )));
                        }
                        let endpoint = template
                            .endpoints
                            .iter()
                            .find(|endpoint| endpoint.id == endpoint_id)
                            .ok_or_else(|| {
                                AppError::Validation(format!(
                                    "template {template_id} has no endpoint {endpoint_id}"
                                ))
                            })?;
                        if connection.protocol != endpoint.protocol
                            || connection.credential_slot_id != endpoint.credential_slot_id
                            || !endpoint
                                .auth_options
                                .iter()
                                .any(|option| option.auth_type == connection.auth_type)
                        {
                            return Err(AppError::Validation(format!(
                                "endpoint {endpoint_id} does not match template {template_id}"
                            )));
                        }
                    }
                } else if api
                    .connections
                    .iter()
                    .any(|connection| connection.template_endpoint_id.is_some())
                {
                    return Err(AppError::Validation(
                        "custom providers cannot reference template endpoints".into(),
                    ));
                }
            }
            // OAuth payloads are validated by OAuthService at every write boundary. Domain
            // validation also keeps a saved template and OAuth kind from drifting apart.
            ProviderData::Oauth(oauth) => {
                if let Some(template_id) = self.template_id.as_deref() {
                    let template = crate::catalog::embedded_catalog()?
                        .auth_template(template_id)
                        .ok_or_else(|| {
                            AppError::Validation(format!(
                                "unknown auth provider template {template_id}"
                            ))
                        })?;
                    if template.auth_kind != oauth.oauth_kind {
                        return Err(AppError::Validation(format!(
                            "auth template {template_id} does not match the saved auth kind"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn public(&self, referenced_by: Vec<String>) -> PublicProvider {
        let (kind, oauth_kind, oauth_account_label, connections, verification_status) =
            match &self.data {
                ProviderData::Api(api) => (
                    "api".to_string(),
                    None,
                    None,
                    api.connections
                        .iter()
                        .map(|connection| PublicProviderConnection {
                            id: connection.id,
                            template_endpoint_id: connection.template_endpoint_id.clone(),
                            credential_slot_id: connection.credential_slot_id.clone(),
                            protocol: connection.protocol,
                            endpoint: connection.endpoint.clone(),
                            auth_type: connection.auth_type,
                            default_model: connection.default_model.clone(),
                            verification: connection.verification.clone(),
                        })
                        .collect(),
                    None,
                ),
                ProviderData::Oauth(oauth) => (
                    "oauth".to_string(),
                    Some(oauth.oauth_kind),
                    oauth
                        .account_label
                        .clone()
                        .or_else(|| oauth.account_id.clone()),
                    Vec::new(),
                    Some(oauth.verification.status),
                ),
            };
        let template = self
            .template_id
            .as_deref()
            .and_then(|id| crate::catalog::embedded_catalog().ok()?.template(id));
        PublicProvider {
            id: self.id,
            name: self.name.clone(),
            kind,
            template_id: self.template_id.clone(),
            template_name: template.map(|template| template.name().to_string()),
            template_mode: template.map(|template| template.mode().to_string()),
            template_category: template.and_then(|template| match template {
                crate::catalog::ProviderTemplate::Api(template) => Some(template.category.clone()),
                crate::catalog::ProviderTemplate::Auth(_) => None,
            }),
            oauth_kind,
            oauth_account_label,
            connections,
            verification_status,
            referenced_by,
            revision: self.revision,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProviderConnection {
    pub id: Uuid,
    pub template_endpoint_id: Option<String>,
    pub credential_slot_id: String,
    pub protocol: CliProtocol,
    pub endpoint: Url,
    pub auth_type: ConnectionAuthType,
    pub default_model: String,
    pub verification: VerificationInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProvider {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub template_id: Option<String>,
    pub template_name: Option<String>,
    pub template_mode: Option<String>,
    pub template_category: Option<String>,
    pub oauth_kind: Option<OAuthKind>,
    pub oauth_account_label: Option<String>,
    pub connections: Vec<PublicProviderConnection>,
    pub verification_status: Option<VerificationStatus>,
    pub referenced_by: Vec<String>,
    pub revision: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "targetType", rename_all = "kebab-case")]
pub enum ConfigurationTarget {
    Api {
        #[serde(rename = "cliId")]
        cli_id: CliId,
        #[serde(rename = "providerId")]
        provider_id: Uuid,
        #[serde(rename = "connectionId")]
        connection_id: Uuid,
        model: String,
    },
    Oauth {
        #[serde(rename = "cliId")]
        cli_id: CliId,
        #[serde(rename = "providerId")]
        provider_id: Uuid,
        model: String,
    },
}

impl ConfigurationTarget {
    pub const fn cli_id(&self) -> CliId {
        match self {
            Self::Api { cli_id, .. } | Self::Oauth { cli_id, .. } => *cli_id,
        }
    }

    pub const fn provider_id(&self) -> Uuid {
        match self {
            Self::Api { provider_id, .. } | Self::Oauth { provider_id, .. } => *provider_id,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Api { model, .. } | Self::Oauth { model, .. } => model,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConfiguration {
    pub id: Uuid,
    pub name: String,
    pub creation_order: i64,
    pub revision: i64,
    pub targets: Vec<ConfigurationTarget>,
    pub last_applied_at: Option<DateTime<Utc>>,
    pub last_apply_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SavedConfiguration {
    pub fn validate(&self) -> AppResult<()> {
        normalize_name(&self.name)?;
        let mut cli_ids = std::collections::HashSet::new();
        for target in &self.targets {
            if !cli_ids.insert(target.cli_id()) {
                return Err(AppError::Validation("a CLI can appear only once".into()));
            }
            if target.model().trim().is_empty() {
                return Err(AppError::Validation("target model is required".into()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanStatus {
    NotInstalled,
    Installed,
    Detected,
    PartiallyDetected,
    Unmanaged,
    ExternallyOverridden,
    Unreadable,
    InvalidConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFileSnapshot {
    pub source_id: String,
    pub display_path: PathBuf,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCliConfiguration {
    pub provider_name: Option<String>,
    pub protocol: Option<CliProtocol>,
    pub auth_kind: Option<String>,
    pub model: Option<String>,
    pub managed_provider_id: Option<Uuid>,
    pub sources: Vec<SourceFileSnapshot>,
    pub externally_overridden: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProviderCandidate {
    pub id: Uuid,
    pub source_provider_id: String,
    pub suggested_name: String,
    pub template_id: Option<String>,
    pub protocol: Option<CliProtocol>,
    pub endpoint: Option<Url>,
    pub auth_type: Option<ConnectionAuthType>,
    pub available_models: Vec<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedCli {
    pub cli_id: CliId,
    pub label: String,
    pub status: ScanStatus,
    pub executable_path: Option<PathBuf>,
    pub config_directory: PathBuf,
    pub version: Option<String>,
    pub source: String,
    pub current: Option<CurrentCliConfiguration>,
    pub provider_candidates: Vec<DetectedProviderCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSnapshot {
    pub id: Uuid,
    pub generated_at: DateTime<Utc>,
    pub items: Vec<DetectedCli>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOAuthBinding {
    pub cli_id: CliId,
    pub provider_id: Uuid,
    pub native_digest: String,
    pub account_identity: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UnmanagedCandidate {
    pub id: Uuid,
    pub snapshot_id: Uuid,
    pub cli_id: CliId,
    pub source_provider_id: String,
    pub suggested_name: String,
    pub source_digests: BTreeMap<PathBuf, Option<String>>,
    pub data: UnmanagedCandidateData,
}

#[derive(Debug, Clone)]
pub enum UnmanagedCandidateData {
    Api {
        template_id: Option<String>,
        connection: Box<ProviderConnection>,
        available_models: Vec<String>,
    },
    Oauth {
        kind: OAuthKind,
        auth_file: PathBuf,
        digest: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigurationMatchStatus {
    Applied,
    PartiallyApplied,
    NotApplied,
    UnableToVerify,
    NoApplicableCli,
}

pub fn calculate_configuration_match(
    configuration: &SavedConfiguration,
    scan: &ScanSnapshot,
    providers: &[ProviderProfile],
) -> ConfigurationMatchStatus {
    let applicable = configuration
        .targets
        .iter()
        .filter_map(|target| {
            scan.items
                .iter()
                .find(|item| item.cli_id == target.cli_id())
                .filter(|item| item.status != ScanStatus::NotInstalled)
                .map(|item| (target, item))
        })
        .collect::<Vec<_>>();
    if applicable.is_empty() {
        return ConfigurationMatchStatus::NoApplicableCli;
    }

    let mut matches = 0usize;
    let mut unable = false;
    for (target, detected) in &applicable {
        if matches!(
            detected.status,
            ScanStatus::PartiallyDetected
                | ScanStatus::ExternallyOverridden
                | ScanStatus::Unreadable
                | ScanStatus::InvalidConfig
        ) {
            unable = true;
            continue;
        }
        let Some(current) = detected.current.as_ref() else {
            unable = true;
            continue;
        };
        let Some(provider) = providers
            .iter()
            .find(|provider| provider.id == target.provider_id())
        else {
            unable = true;
            continue;
        };
        let target_matches = match (target, &provider.data) {
            (ConfigurationTarget::Api { connection_id, .. }, ProviderData::Api(api)) => api
                .connections
                .iter()
                .find(|connection| connection.id == *connection_id)
                .is_some_and(|connection| {
                    current.managed_provider_id == Some(provider.id)
                        && current.protocol == Some(connection.protocol)
                        && current.model.as_deref() == Some(target.model())
                }),
            (ConfigurationTarget::Oauth { .. }, ProviderData::Oauth(oauth)) => {
                if oauth.manually_modified {
                    unable = true;
                    false
                } else {
                    current.managed_provider_id == Some(provider.id)
                        && current.auth_kind.as_deref() == Some("oauth")
                        && current.model.as_deref() == Some(target.model())
                }
            }
            _ => {
                unable = true;
                false
            }
        };
        matches += usize::from(target_matches);
    }
    if unable {
        return ConfigurationMatchStatus::UnableToVerify;
    }
    if last_apply_has_failures(configuration.last_apply_summary.as_deref()) {
        return ConfigurationMatchStatus::PartiallyApplied;
    }
    if matches == applicable.len() {
        ConfigurationMatchStatus::Applied
    } else if matches == 0 {
        ConfigurationMatchStatus::NotApplied
    } else {
        ConfigurationMatchStatus::PartiallyApplied
    }
}

fn last_apply_has_failures(summary: Option<&str>) -> bool {
    let Some(summary) = summary else { return false };
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("state")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|state| {
                        matches!(
                            state,
                            "failed"
                                | "conflict"
                                | "running-blocked"
                                | "cancelled"
                                | "incompatible"
                                | "success-unverified"
                        )
                    })
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyItemState {
    Waiting,
    Writing,
    Success,
    SuccessUnverified,
    Unchanged,
    NotInstalled,
    Incompatible,
    Conflict,
    RunningBlocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreviewItem {
    pub cli_id: CliId,
    pub state: ApplyItemState,
    pub path: Option<PathBuf>,
    pub provider_name: String,
    pub protocol: Option<CliProtocol>,
    pub model: String,
    pub changes: Vec<FieldChange>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreview {
    pub id: Uuid,
    pub configuration_id: Uuid,
    pub configuration_revision: i64,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub items: Vec<ApplyPreviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRunItem {
    pub cli_id: CliId,
    pub state: ApplyItemState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRunSnapshot {
    pub id: Uuid,
    pub preview_id: Uuid,
    pub configuration_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
    pub items: Vec<ApplyRunItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupMetadata {
    pub id: Uuid,
    pub cli_id: CliId,
    pub source_file_id: String,
    pub original_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub configuration_id: Option<Uuid>,
    pub original_digest: Option<String>,
    pub permissions: Option<u32>,
    pub originally_existed: bool,
    pub contains_credentials: bool,
    pub relative_backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub id: Uuid,
    pub backup_id: Uuid,
    pub cli_id: CliId,
    pub target_path: PathBuf,
    pub current_digest: Option<String>,
    pub restores_tombstone: bool,
    pub contains_credentials: bool,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualCliLocation {
    pub cli_id: CliId,
    pub executable_path: Option<PathBuf>,
    pub config_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLanguage {
    ZhCn,
    En,
}

impl fmt::Display for AppLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        })
    }
}

impl FromStr for AppLanguage {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "zh-CN" => Ok(Self::ZhCn),
            "en" => Ok(Self::En),
            _ => Err(AppError::Validation(format!("unknown language: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppTheme {
    Light,
    Dark,
    System,
}

impl fmt::Display for AppTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        })
    }
}

impl FromStr for AppTheme {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            "system" => Ok(Self::System),
            _ => Err(AppError::Validation(format!("unknown theme: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: AppLanguage,
    pub theme: AppTheme,
    pub scan_on_startup: bool,
    pub plaintext_risk_accepted: bool,
    pub revision: i64,
    pub manual_locations: Vec<ManualCliLocation>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: AppLanguage::ZhCn,
            theme: AppTheme::System,
            scan_on_startup: true,
            plaintext_risk_accepted: false,
            revision: 1,
            manual_locations: CliId::ALL
                .into_iter()
                .map(|cli_id| ManualCliLocation {
                    cli_id,
                    executable_path: None,
                    config_directory: None,
                })
                .collect(),
        }
    }
}

pub fn normalize_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    let length = trimmed.chars().count();
    if !(1..=64).contains(&length) {
        return Err(AppError::Validation(
            "name must contain between 1 and 64 Unicode characters".into(),
        ));
    }
    Ok(trimmed.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_provider_from_template(template_id: &str) -> ProviderProfile {
        let template = crate::catalog::embedded_catalog()
            .unwrap()
            .api_template(template_id)
            .unwrap();
        let now = Utc::now();
        ProviderProfile {
            id: Uuid::new_v4(),
            name: format!("{template_id} fixture"),
            template_id: Some(template_id.into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: template
                    .endpoints
                    .iter()
                    .map(|endpoint| ProviderConnection {
                        id: Uuid::new_v4(),
                        template_endpoint_id: Some(endpoint.id.clone()),
                        credential_slot_id: endpoint.credential_slot_id.clone(),
                        protocol: endpoint.protocol,
                        endpoint: endpoint.base_url.clone(),
                        auth_type: endpoint.default_auth_type().unwrap(),
                        api_key: "shared-fixture-secret".into(),
                        default_model: endpoint.default_model().unwrap_or("manual-model").into(),
                        verification: VerificationInfo::default(),
                    })
                    .collect(),
            }),
        }
    }

    fn assert_validation_contains(profile: &ProviderProfile, expected: &str) {
        let error = profile.validate().unwrap_err().to_string();
        assert!(
            error.contains(expected),
            "expected validation error containing {expected:?}, got {error:?}"
        );
    }

    #[test]
    fn names_are_trimmed_and_case_insensitive() {
        assert_eq!(normalize_name("  工作配置 ").unwrap(), "工作配置");
        assert_eq!(
            normalize_name("Dev").unwrap(),
            normalize_name("dev").unwrap()
        );
        assert!(normalize_name(" ").is_err());
        assert!(normalize_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn oauth_target_is_not_user_selectable() {
        assert_eq!(OAuthKind::Anthropic.target_cli(), CliId::ClaudeCode);
        assert_eq!(OAuthKind::Codex.target_cli(), CliId::Codex);
    }

    #[test]
    fn api_template_validation_rejects_unknown_and_incomplete_templates() {
        let mut unknown = api_provider_from_template("glm-coding-plan");
        unknown.template_id = Some("unknown-api-template".into());
        assert_validation_contains(&unknown, "unknown API provider template");

        let mut incomplete = api_provider_from_template("glm-coding-plan");
        let ProviderData::Api(api) = &mut incomplete.data else {
            unreachable!()
        };
        api.connections.pop();
        assert_validation_contains(&incomplete, "requires all configured endpoints");
    }

    #[test]
    fn api_template_validation_rejects_endpoint_contract_mismatches() {
        let mut wrong_protocol = api_provider_from_template("glm-coding-plan");
        let ProviderData::Api(api) = &mut wrong_protocol.data else {
            unreachable!()
        };
        api.connections[0].protocol = CliProtocol::OpenaiChat;
        assert_validation_contains(&wrong_protocol, "does not match template");

        let mut wrong_slot = api_provider_from_template("glm-coding-plan");
        let ProviderData::Api(api) = &mut wrong_slot.data else {
            unreachable!()
        };
        api.connections[0].credential_slot_id = "another-key".into();
        assert_validation_contains(&wrong_slot, "does not match template");

        let mut wrong_auth = api_provider_from_template("anthropic-api");
        let ProviderData::Api(api) = &mut wrong_auth.data else {
            unreachable!()
        };
        api.connections[0].auth_type = ConnectionAuthType::Bearer;
        assert_validation_contains(&wrong_auth, "does not match template");
    }

    #[test]
    fn api_template_validation_rejects_detached_endpoints_and_divergent_slot_secrets() {
        let mut custom = api_provider_from_template("glm-coding-plan");
        custom.template_id = None;
        assert_validation_contains(
            &custom,
            "custom providers cannot reference template endpoints",
        );

        let mut divergent_secrets = api_provider_from_template("glm-coding-plan");
        let ProviderData::Api(api) = &mut divergent_secrets.data else {
            unreachable!()
        };
        api.connections[1].api_key = "different-secret".into();
        assert_validation_contains(&divergent_secrets, "must use the same secret");
    }

    #[test]
    fn auth_template_validation_rejects_a_different_oauth_kind() {
        let now = Utc::now();
        let profile = ProviderProfile {
            id: Uuid::new_v4(),
            name: "Mismatched OAuth".into(),
            template_id: Some("codex-auth".into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Anthropic,
                account_id: None,
                account_label: None,
                raw_content: "fixture".into(),
                digest: "sha256:fixture".into(),
                manually_modified: false,
                verification: VerificationInfo::default(),
            }),
        };
        assert_validation_contains(&profile, "does not match the saved auth kind");
    }

    #[test]
    fn public_provider_never_serializes_secret() {
        let now = Utc::now();
        let profile = ProviderProfile {
            id: Uuid::new_v4(),
            name: "private".into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse("https://example.com/v1").unwrap(),
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: "super-secret".into(),
                    default_model: "model".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        };
        let serialized = serde_json::to_string(&profile.public(Vec::new())).unwrap();
        assert!(!serialized.contains("super-secret"));
        assert!(!serialized.contains("apiKey"));
    }

    #[test]
    fn saved_oauth_profile_allows_arbitrary_raw_content() {
        let now = Utc::now();
        let profile = ProviderProfile {
            id: Uuid::new_v4(),
            name: "manually edited".into(),
            template_id: Some("codex-auth".into()),
            revision: 2,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Codex,
                account_id: None,
                account_label: None,
                raw_content: String::new(),
                digest: "sha256:fixture".into(),
                manually_modified: true,
                verification: VerificationInfo {
                    status: VerificationStatus::UserModifiedUnverified,
                    verified_at: None,
                    error: None,
                },
            }),
        };
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn configuration_match_has_exactly_the_five_product_states() {
        let now = Utc::now();
        let provider_id = Uuid::new_v4();
        let connection_id = Uuid::new_v4();
        let provider = ProviderProfile {
            id: provider_id,
            name: "shared".into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: connection_id,
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse("https://example.com/v1").unwrap(),
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: "fixture-secret".into(),
                    default_model: "model-a".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        };
        let configuration = SavedConfiguration {
            id: Uuid::new_v4(),
            name: "two targets".into(),
            creation_order: 1,
            revision: 1,
            targets: vec![CliId::Codex, CliId::Opencode]
                .into_iter()
                .map(|cli_id| ConfigurationTarget::Api {
                    cli_id,
                    provider_id,
                    connection_id,
                    model: "model-a".into(),
                })
                .collect(),
            last_applied_at: None,
            last_apply_summary: None,
            created_at: now,
            updated_at: now,
        };
        let current = || CurrentCliConfiguration {
            provider_name: Some("shared".into()),
            protocol: Some(CliProtocol::OpenaiResponses),
            auth_kind: Some("api".into()),
            model: Some("model-a".into()),
            managed_provider_id: Some(provider_id),
            sources: Vec::new(),
            externally_overridden: false,
            diagnostics: Vec::new(),
        };
        let mut scan = ScanSnapshot {
            id: Uuid::new_v4(),
            generated_at: now,
            items: vec![CliId::Codex, CliId::Opencode]
                .into_iter()
                .map(|cli_id| DetectedCli {
                    cli_id,
                    label: cli_id.label().into(),
                    status: ScanStatus::Detected,
                    executable_path: Some(cli_id.command().into()),
                    config_directory: "/fixture".into(),
                    version: Some("fixture".into()),
                    source: "fixture".into(),
                    current: Some(current()),
                    provider_candidates: Vec::new(),
                })
                .collect(),
        };
        assert_eq!(
            calculate_configuration_match(&configuration, &scan, std::slice::from_ref(&provider)),
            ConfigurationMatchStatus::Applied
        );
        scan.items[0].current.as_mut().unwrap().model = Some("different".into());
        assert_eq!(
            calculate_configuration_match(&configuration, &scan, std::slice::from_ref(&provider)),
            ConfigurationMatchStatus::PartiallyApplied
        );
        scan.items[1].current.as_mut().unwrap().model = Some("different".into());
        assert_eq!(
            calculate_configuration_match(&configuration, &scan, std::slice::from_ref(&provider)),
            ConfigurationMatchStatus::NotApplied
        );
        scan.items[0].status = ScanStatus::InvalidConfig;
        assert_eq!(
            calculate_configuration_match(&configuration, &scan, std::slice::from_ref(&provider)),
            ConfigurationMatchStatus::UnableToVerify
        );
        for item in &mut scan.items {
            item.status = ScanStatus::NotInstalled;
        }
        assert_eq!(
            calculate_configuration_match(&configuration, &scan, &[provider]),
            ConfigurationMatchStatus::NoApplicableCli
        );
    }
}
