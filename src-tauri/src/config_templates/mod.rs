use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    domain::{CliId, CliProtocol, ConnectionAuthType},
    error::{AppError, AppResult},
    services::config_writer::{parse_jsonc_value, parse_toml},
};

pub const UPSTREAM_REPOSITORY: &str = "https://github.com/laurentwu/CLIAdapter";
pub const UPSTREAM_COMMIT: &str = "7ea4dcc5e874d76a14e54a8e15f4fec7b8c5522d";
pub const OPENCODE_UPSTREAM_COMMIT: &str = "25ce581516599103b2455cab6770f021aa5f2f91";
pub const TEMPLATE_SCHEMA_VERSION: u32 = 2;
pub const PROVIDER_TEMPLATE_IDS: [&str; 7] = [
    "deepseek",
    "zhipuai",
    "zhipuai-coding-plan",
    "zai",
    "zai-coding-plan",
    "opencode",
    "opencode-go",
];
pub const DEEPSEEK_EXACT_MODEL_IDS: [&str; 3] = [
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "deepseek-v4-flash-vision-exp",
];

pub const CLAUDE_MANAGED_ENV_FIELDS: [&str; 14] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_EFFORT_LEVEL",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "API_TIMEOUT_MS",
];

const MANIFEST: &str = include_str!("../../catalog/config-templates/manifest.json");

struct Resource {
    path: &'static str,
    bytes: &'static [u8],
}

macro_rules! resource {
    ($path:literal) => {
        Resource {
            path: $path,
            bytes: include_bytes!(concat!("../../catalog/config-templates/", $path)),
        }
    };
}

// This explicit compile-time table is the runtime trust boundary. Paths can never come from a
// manifest supplied by a user and no template directory is traversed at runtime.
const RESOURCES: &[Resource] = &[
    resource!("claude/deepseek/provider.json"),
    resource!("claude/deepseek/settings.json"),
    resource!("claude/opencode-go/provider.json"),
    resource!("claude/opencode-go/settings.json"),
    resource!("claude/opencode/provider.json"),
    resource!("claude/opencode/settings.json"),
    resource!("claude/settings.json"),
    resource!("claude/zai-coding-plan/provider.json"),
    resource!("claude/zai-coding-plan/settings.json"),
    resource!("claude/zai/provider.json"),
    resource!("claude/zai/settings.json"),
    resource!("claude/zhipuai-coding-plan/provider.json"),
    resource!("claude/zhipuai-coding-plan/settings.json"),
    resource!("claude/zhipuai/provider.json"),
    resource!("claude/zhipuai/settings.json"),
    resource!("codex/config.toml"),
    resource!("codex/deepseek/config.toml"),
    resource!("codex/deepseek/deepseek-v4-flash/models.json"),
    resource!("codex/deepseek/deepseek-v4-flash-vision-exp/models.json"),
    resource!("codex/deepseek/deepseek-v4-pro/models.json"),
    resource!("codex/deepseek/models.json"),
    resource!("codex/deepseek/provider.json"),
    resource!("codex/models.json"),
    resource!("codex/opencode/config.toml"),
    resource!("codex/opencode-go/config.toml"),
    resource!("codex/opencode-go/models.json"),
    resource!("codex/opencode-go/provider.json"),
    resource!("codex/opencode/models.json"),
    resource!("codex/opencode/provider.json"),
    resource!("codex/zai-coding-plan/config.toml"),
    resource!("codex/zai-coding-plan/models.json"),
    resource!("codex/zai-coding-plan/provider.json"),
    resource!("codex/zai/config.toml"),
    resource!("codex/zai/models.json"),
    resource!("codex/zai/provider.json"),
    resource!("codex/zhipuai-coding-plan/config.toml"),
    resource!("codex/zhipuai-coding-plan/models.json"),
    resource!("codex/zhipuai-coding-plan/provider.json"),
    resource!("codex/zhipuai/config.toml"),
    resource!("codex/zhipuai/models.json"),
    resource!("codex/zhipuai/provider.json"),
    resource!("LICENSE"),
    resource!("opencode/auth.json"),
    resource!("opencode/deepseek/auth.json"),
    resource!("opencode/deepseek/opencode.jsonc"),
    resource!("opencode/deepseek/provider.json"),
    resource!("opencode/opencode-go/auth.json"),
    resource!("opencode/opencode-go/opencode.jsonc"),
    resource!("opencode/opencode-go/provider.json"),
    resource!("opencode/opencode.jsonc"),
    resource!("opencode/opencode/auth.json"),
    resource!("opencode/opencode/opencode.jsonc"),
    resource!("opencode/opencode/provider.json"),
    resource!("opencode/zai-coding-plan/auth.json"),
    resource!("opencode/zai-coding-plan/opencode.jsonc"),
    resource!("opencode/zai-coding-plan/provider.json"),
    resource!("opencode/zai/auth.json"),
    resource!("opencode/zai/opencode.jsonc"),
    resource!("opencode/zai/provider.json"),
    resource!("opencode/zhipuai-coding-plan/auth.json"),
    resource!("opencode/zhipuai-coding-plan/opencode.jsonc"),
    resource!("opencode/zhipuai-coding-plan/provider.json"),
    resource!("opencode/zhipuai/auth.json"),
    resource!("opencode/zhipuai/opencode.jsonc"),
    resource!("opencode/zhipuai/provider.json"),
    resource!("qwen/deepseek/provider.json"),
    resource!("qwen/deepseek/settings.json"),
    resource!("qwen/opencode-go/provider.json"),
    resource!("qwen/opencode-go/settings.json"),
    resource!("qwen/opencode/provider.json"),
    resource!("qwen/opencode/settings.json"),
    resource!("qwen/settings.json"),
    resource!("qwen/zai-coding-plan/provider.json"),
    resource!("qwen/zai-coding-plan/settings.json"),
    resource!("qwen/zai/provider.json"),
    resource!("qwen/zai/settings.json"),
    resource!("qwen/zhipuai-coding-plan/provider.json"),
    resource!("qwen/zhipuai-coding-plan/settings.json"),
    resource!("qwen/zhipuai/provider.json"),
    resource!("qwen/zhipuai/settings.json"),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    upstream_repository: String,
    upstream_commit: String,
    resources: Vec<ManifestResource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestResource {
    path: String,
    sha256: String,
    role: String,
    cli: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    protocol: Option<String>,
    #[serde(default)]
    upstream_commit: Option<String>,
    #[serde(default)]
    upstream_path: Option<String>,
}

static VALIDATED: OnceCell<Result<(), String>> = OnceCell::new();

pub fn validate_bundled_templates() -> AppResult<()> {
    VALIDATED
        .get_or_init(validate_bundled_templates_inner)
        .clone()
        .map_err(AppError::Serialization)
}

fn validate_bundled_templates_inner() -> Result<(), String> {
    let manifest: Manifest = serde_json::from_str(MANIFEST)
        .map_err(|error| format!("invalid config-template manifest: {error}"))?;
    if manifest.schema_version != TEMPLATE_SCHEMA_VERSION
        || manifest.upstream_repository != UPSTREAM_REPOSITORY
        || manifest.upstream_commit != UPSTREAM_COMMIT
    {
        return Err("config-template manifest identity does not match the compiled version".into());
    }
    let resources = RESOURCES
        .iter()
        .map(|resource| (resource.path, resource.bytes))
        .collect::<HashMap<_, _>>();
    if resources.len() != RESOURCES.len() || manifest.resources.len() != RESOURCES.len() {
        return Err("config-template manifest and compiled resource table differ".into());
    }
    let mut paths = HashSet::new();
    let mut provider_ids = HashSet::new();
    let mut exact_models = HashSet::new();
    for entry in &manifest.resources {
        if !paths.insert(entry.path.as_str()) {
            return Err(format!("duplicate config-template resource {}", entry.path));
        }
        let bytes = resources
            .get(entry.path.as_str())
            .ok_or_else(|| format!("manifest resource {} is not compiled", entry.path))?;
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual != entry.sha256 {
            return Err(format!(
                "config-template digest mismatch for {}",
                entry.path
            ));
        }
        validate_manifest_metadata(entry)?;
        validate_manifest_source(entry)?;
        validate_resource(entry, bytes)?;
        if entry.role == "provider-identity" {
            provider_ids.insert((
                entry.cli.as_deref().unwrap_or_default(),
                entry.provider_id.as_deref().unwrap_or_default(),
            ));
        }
        if let Some(model_id) = entry.model_id.as_deref() {
            exact_models.insert(model_id);
        }
    }
    let mut expected_paths = HashSet::from([
        "LICENSE",
        "claude/settings.json",
        "codex/config.toml",
        "codex/models.json",
        "opencode/auth.json",
        "opencode/opencode.jsonc",
        "qwen/settings.json",
    ]);
    for provider in PROVIDER_TEMPLATE_IDS {
        expected_paths.extend([
            claude_provider_path(provider),
            claude_settings_path(provider),
            codex_provider_path(provider),
            codex_config_path(provider),
            codex_models_path(provider),
            opencode_provider_path(provider),
            opencode_config_path(provider),
            opencode_auth_path(provider),
            qwen_provider_path(provider),
            qwen_settings_path(provider),
        ]);
    }
    for model in DEEPSEEK_EXACT_MODEL_IDS {
        expected_paths.insert(deepseek_exact_model_path(model));
    }
    if paths != expected_paths {
        return Err("the config-template resource set is incomplete or contains extras".into());
    }
    for cli in ["claude-code", "codex", "opencode", "qwen"] {
        for provider in PROVIDER_TEMPLATE_IDS {
            if !provider_ids.contains(&(cli, provider)) {
                return Err(format!(
                    "missing {cli}/{provider} provider identity template"
                ));
            }
        }
    }
    if exact_models != DEEPSEEK_EXACT_MODEL_IDS.into_iter().collect() {
        return Err("the DeepSeek exact-model template set is incomplete".into());
    }
    Ok(())
}

fn validate_manifest_metadata(entry: &ManifestResource) -> Result<(), String> {
    let (role, cli, provider_id, model_id, protocol) = expected_resource_metadata(&entry.path)
        .ok_or_else(|| format!("unknown config-template resource {}", entry.path))?;
    if entry.role != role
        || entry.cli.as_deref() != cli
        || entry.provider_id.as_deref() != provider_id
        || entry.model_id.as_deref() != model_id
        || entry.protocol.as_deref() != protocol
    {
        return Err(format!(
            "config-template metadata does not match its path for {}",
            entry.path
        ));
    }
    Ok(())
}

type ExpectedResourceMetadata = (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

/// Binds every resource to exactly one of the two compiled upstream sources. OpenCode resources
/// must name the newer commit and its `cli/` source path; everything else must use the default
/// source and may not override it. Arbitrary sources are never accepted.
fn validate_manifest_source(entry: &ManifestResource) -> Result<(), String> {
    let is_opencode = entry.path.starts_with("opencode/");
    match (&entry.upstream_commit, &entry.upstream_path) {
        (Some(commit), Some(path)) => {
            if !is_opencode {
                return Err(format!(
                    "config-template source override is not allowed for {}",
                    entry.path
                ));
            }
            if commit != OPENCODE_UPSTREAM_COMMIT || path != &format!("cli/{}", entry.path) {
                return Err(format!(
                    "config-template source for {} is not the compiled OpenCode source",
                    entry.path
                ));
            }
            Ok(())
        }
        (Some(_), None) | (None, Some(_)) => Err(format!(
            "config-template source for {} is incomplete",
            entry.path
        )),
        (None, None) if is_opencode => Err(format!(
            "OpenCode config-template {} does not declare its fixed source",
            entry.path
        )),
        (None, None) => Ok(()),
    }
}

fn expected_resource_metadata(path: &str) -> Option<ExpectedResourceMetadata> {
    if path == "LICENSE" {
        return Some(("license", None, None, None, None));
    }
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["claude", "settings.json"] => Some((
            "claude-settings",
            Some("claude-code"),
            None,
            None,
            Some("anthropic-messages"),
        )),
        [
            "claude",
            provider,
            file @ ("provider.json" | "settings.json"),
        ] => {
            let provider = known_template_id(Some(provider))?;
            Some((
                if *file == "provider.json" {
                    "provider-identity"
                } else {
                    "claude-settings"
                },
                Some("claude-code"),
                Some(provider),
                None,
                Some("anthropic-messages"),
            ))
        }
        ["codex", file @ ("config.toml" | "models.json")] => Some((
            if *file == "config.toml" {
                "codex-config"
            } else {
                "codex-model-catalog"
            },
            Some("codex"),
            None,
            None,
            Some("openai-responses"),
        )),
        [
            "codex",
            provider,
            file @ ("provider.json" | "config.toml" | "models.json"),
        ] => {
            let provider = known_template_id(Some(provider))?;
            let role = match *file {
                "provider.json" => "provider-identity",
                "config.toml" => "codex-config",
                "models.json" => "codex-model-catalog",
                _ => unreachable!(),
            };
            Some((
                role,
                Some("codex"),
                Some(provider),
                None,
                Some("openai-responses"),
            ))
        }
        ["codex", "deepseek", model, "models.json"] => {
            let model = known_exact_model_id(model)?;
            Some((
                "codex-model-catalog",
                Some("codex"),
                Some("deepseek"),
                Some(model),
                Some("openai-responses"),
            ))
        }
        ["opencode", "opencode.jsonc"] => Some((
            "opencode-config",
            Some("opencode"),
            None,
            None,
            Some("openai-compatible"),
        )),
        ["opencode", "auth.json"] => Some((
            "opencode-auth",
            Some("opencode"),
            None,
            None,
            Some("openai-compatible"),
        )),
        [
            "opencode",
            provider,
            file @ ("provider.json" | "opencode.jsonc" | "auth.json"),
        ] => {
            let provider = known_template_id(Some(provider))?;
            let role = match *file {
                "provider.json" => "provider-identity",
                "opencode.jsonc" => "opencode-config",
                "auth.json" => "opencode-auth",
                _ => unreachable!(),
            };
            Some((
                role,
                Some("opencode"),
                Some(provider),
                None,
                Some("openai-compatible"),
            ))
        }
        ["qwen", "settings.json"] => Some((
            "qwen-config",
            Some("qwen"),
            None,
            None,
            Some("openai-compatible"),
        )),
        ["qwen", provider, file @ ("provider.json" | "settings.json")] => {
            let provider = known_template_id(Some(provider))?;
            Some((
                if *file == "provider.json" {
                    "provider-identity"
                } else {
                    "qwen-config"
                },
                Some("qwen"),
                Some(provider),
                None,
                Some("openai-compatible"),
            ))
        }
        _ => None,
    }
}

fn validate_resource(entry: &ManifestResource, bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("template {} is not UTF-8: {error}", entry.path))?;
    match entry.role.as_str() {
        "license" => Ok(()),
        "provider-identity" => {
            let value: Value = serde_json::from_str(text)
                .map_err(|error| format!("invalid provider identity {}: {error}", entry.path))?;
            validate_provider_identity(
                &value,
                &entry.path,
                entry.provider_id.as_deref(),
                entry.cli.as_deref(),
            )
            .map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "claude-settings" => {
            let value: Value = serde_json::from_str(text)
                .map_err(|error| format!("invalid JSON template {}: {error}", entry.path))?;
            validate_claude_template_shape(&value, &entry.path)
                .map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "codex-model-catalog" => {
            let value: Value = serde_json::from_str(text)
                .map_err(|error| format!("invalid JSON template {}: {error}", entry.path))?;
            validate_codex_model_catalog_shape(&value, &entry.path)
                .map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "opencode-config" => {
            // The upstream OpenCode templates are JSONC and may carry guidance comments. Digests
            // still cover the untouched original bytes.
            let value = parse_jsonc_value(text)
                .map_err(|error| format!("invalid JSONC template {}: {error}", entry.path))?;
            validate_opencode_config_shape(&value, &entry.path, entry.provider_id.as_deref())
                .map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "opencode-auth" => {
            let value = parse_jsonc_value(text)
                .map_err(|error| format!("invalid JSONC template {}: {error}", entry.path))?;
            validate_opencode_auth_shape(&value, &entry.path, entry.provider_id.as_deref())
                .map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "qwen-config" => {
            let value: Value = serde_json::from_str(text)
                .map_err(|error| format!("invalid JSON template {}: {error}", entry.path))?;
            validate_qwen_template_shape(&value, &entry.path).map_err(|error| error.to_string())?;
            validate_template_placeholders(&value, &entry.path)
        }
        "codex-config" => {
            let document = parse_toml(text).map_err(|error| error.to_string())?;
            validate_codex_config_shape(&document, &entry.path)
                .map_err(|error| error.to_string())?;
            for (_, item) in document.iter() {
                validate_toml_item_placeholders(item, &entry.path)?;
            }
            for key in document.as_table().iter().map(|(key, _)| key) {
                validate_placeholder_string(key, &entry.path)?;
            }
            Ok(())
        }
        _ => unreachable!(),
    }
}

fn validate_provider_identity(
    value: &Value,
    path: &str,
    expected_provider_id: Option<&str>,
    cli: Option<&str>,
) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &["id", "name", "env", "protocol", "base_url", "docs"],
        path,
    )?;
    for key in ["id", "name", "protocol", "base_url", "docs"] {
        required_string(
            root.get(key)
                .ok_or_else(|| AppError::Serialization(format!("{path} has no {key}")))?,
        )?;
    }
    let env = root
        .get("env")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Serialization(format!("{path} env is not an array")))?;
    if env.is_empty() || env.iter().any(|value| value.as_str().is_none()) {
        return Err(AppError::Serialization(format!(
            "{path} env must contain strings"
        )));
    }
    if root.get("id").and_then(Value::as_str) != expected_provider_id {
        return Err(AppError::Serialization(format!(
            "provider identity mismatch in {path}"
        )));
    }
    let expected_protocol = match cli {
        Some("claude-code") => "anthropic-messages",
        Some("codex") => "responses",
        Some("opencode") => "openai-compatible",
        Some("qwen") => "openai-compatible",
        _ => {
            return Err(AppError::Serialization(format!(
                "provider identity has an invalid CLI in {path}"
            )));
        }
    };
    if root.get("protocol").and_then(Value::as_str) != Some(expected_protocol) {
        return Err(AppError::Serialization(format!(
            "provider identity protocol mismatch in {path}"
        )));
    }
    Ok(())
}

const CLAUDE_MODEL_ENV_FIELDS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
];

fn validate_claude_template_shape(value: &Value, path: &str) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &["$schema", "model", "env"],
        path,
    )?;
    if root.get("$schema").and_then(Value::as_str)
        != Some("https://json.schemastore.org/claude-code-settings.json")
        || root.get("model").and_then(Value::as_str) != Some("<model-id>")
    {
        return Err(AppError::Serialization(format!(
            "{path} has an invalid schema or primary model field"
        )));
    }
    let env = root
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization(format!("{path} env is not an object")))?;
    reject_unknown_keys(
        env.keys().map(String::as_str),
        &CLAUDE_MANAGED_ENV_FIELDS,
        path,
    )?;
    for (key, value) in env {
        let value = required_string(value)?;
        if CLAUDE_MODEL_ENV_FIELDS.contains(&key.as_str())
            && value != "<model-id>"
            && value != "<model-id>[1m]"
        {
            return Err(AppError::Serialization(format!(
                "{path} model field {key} has an unsupported template value"
            )));
        }
    }
    Ok(())
}

fn validate_qwen_template_shape(value: &Value, path: &str) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &["modelProviders", "providerProtocol", "env"],
        path,
    )?;
    let providers = root
        .get("modelProviders")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Serialization(format!("{path} modelProviders is not an object"))
        })?;
    let protocols = root
        .get("providerProtocol")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Serialization(format!("{path} providerProtocol is not an object"))
        })?;
    let env = root
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization(format!("{path} env is not an object")))?;
    if providers.len() != 1 || protocols.len() != 1 || env.len() != 1 {
        return Err(AppError::Serialization(format!(
            "{path} must contain one provider, protocol, and environment binding"
        )));
    }
    let (provider_id, models) = providers.iter().next().expect("checked length");
    if protocols.get(provider_id).and_then(Value::as_str) != Some("openai") {
        return Err(AppError::Serialization(format!(
            "{path} must map its provider to the openai protocol"
        )));
    }
    let models = models
        .as_array()
        .filter(|models| models.len() == 1)
        .ok_or_else(|| AppError::Serialization(format!("{path} must contain one model")))?;
    let model = models[0]
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} model is not an object")))?;
    reject_unknown_keys(
        model.keys().map(String::as_str),
        &["id", "name", "envKey", "baseUrl"],
        path,
    )?;
    for key in ["id", "name", "envKey", "baseUrl"] {
        required_string(
            model
                .get(key)
                .ok_or_else(|| AppError::Serialization(format!("{path} model has no {key}")))?,
        )?;
    }
    if env.values().any(|value| value.as_str().is_none()) {
        return Err(AppError::Serialization(format!(
            "{path} environment credential must be a string"
        )));
    }
    Ok(())
}

fn validate_codex_config_shape(document: &toml_edit::DocumentMut, path: &str) -> AppResult<()> {
    reject_unknown_keys(
        document.as_table().iter().map(|(key, _)| key),
        &[
            "model",
            "model_provider",
            "model_reasoning_effort",
            "model_catalog_json",
            "preferred_auth_method",
            "forced_login_method",
            "model_providers",
        ],
        path,
    )?;
    for key in [
        "model",
        "model_provider",
        "model_reasoning_effort",
        "model_catalog_json",
    ] {
        toml_string(document, key, path)?;
    }
    let reasoning = toml_string(document, "model_reasoning_effort", path)?;
    if !["high", "max"].contains(&reasoning.as_str()) {
        return Err(AppError::Serialization(format!(
            "{path} has an unsupported model_reasoning_effort"
        )));
    }
    if let Some(value) = optional_toml_string(document, "preferred_auth_method", path)?
        && value != "apikey"
    {
        return Err(AppError::Serialization(format!(
            "{path} has an unsupported preferred_auth_method"
        )));
    }
    if let Some(value) = optional_toml_string(document, "forced_login_method", path)?
        && value != "api"
    {
        return Err(AppError::Serialization(format!(
            "{path} has an unsupported forced_login_method"
        )));
    }
    let providers = document
        .get("model_providers")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(|| AppError::Serialization(format!("{path} has no model_providers table")))?;
    if providers.len() != 1 {
        return Err(AppError::Serialization(format!(
            "{path} must contain one provider table"
        )));
    }
    let provider = providers
        .iter()
        .next()
        .and_then(|(_, item)| item.as_table())
        .ok_or_else(|| AppError::Serialization(format!("{path} provider entry is not a table")))?;
    reject_unknown_keys(
        provider.iter().map(|(key, _)| key),
        &[
            "name",
            "base_url",
            "wire_api",
            "experimental_bearer_token",
            "env_key",
        ],
        path,
    )?;
    for key in ["name", "base_url", "wire_api"] {
        if provider
            .get(key)
            .and_then(toml_edit::Item::as_str)
            .is_none()
        {
            return Err(AppError::Serialization(format!(
                "{path} provider field {key} must be a string"
            )));
        }
    }
    if provider.get("wire_api").and_then(toml_edit::Item::as_str) != Some("responses") {
        return Err(AppError::Serialization(format!(
            "{path} has an invalid wire_api"
        )));
    }
    let mut credential_fields = 0;
    for key in ["experimental_bearer_token", "env_key"] {
        if let Some(value) = provider.get(key) {
            if value.as_str().is_none() {
                return Err(AppError::Serialization(format!(
                    "{path} provider field {key} must be a string"
                )));
            }
            credential_fields += 1;
        }
    }
    if credential_fields != 1 {
        return Err(AppError::Serialization(format!(
            "{path} must contain exactly one provider credential field"
        )));
    }
    Ok(())
}

fn validate_codex_model_catalog_shape(value: &Value, path: &str) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    reject_unknown_keys(root.keys().map(String::as_str), &["models"], path)?;
    let models = root
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Serialization(format!("{path} models is not an array")))?;
    if models.len() != 1 {
        return Err(AppError::Serialization(format!(
            "{path} must contain exactly one model"
        )));
    }
    let model = models[0]
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} model is not an object")))?;
    for (key, value) in model {
        let valid = match key.as_str() {
            "slug"
            | "display_name"
            | "description"
            | "default_reasoning_level"
            | "shell_type"
            | "visibility"
            | "base_instructions"
            | "default_reasoning_summary"
            | "apply_patch_tool_type"
            | "default_verbosity"
            | "web_search_tool_type"
            | "multi_agent_version"
            | "comp_hash"
            | "reasoning_summary_format"
            | "minimal_client_version" => value.is_string(),
            "supported_in_api"
            | "supports_reasoning_summaries"
            | "support_verbosity"
            | "supports_parallel_tool_calls"
            | "prefer_websockets"
            | "supports_image_detail_original"
            | "use_responses_lite"
            | "include_skills_usage_instructions"
            | "supports_search_tool" => value.is_boolean(),
            "priority"
            | "context_window"
            | "max_context_window"
            | "effective_context_window_percent" => value.as_u64().is_some(),
            "supported_reasoning_levels" => validate_reasoning_levels(value, path)?,
            "truncation_policy" => validate_truncation_policy(value, path)?,
            "experimental_supported_tools" | "input_modalities" => value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_string)),
            "tool_mode"
            | "auto_review_model_override"
            | "auto_compact_token_limit"
            | "default_service_tier"
            | "availability_nux"
            | "upgrade" => value.is_null(),
            _ => {
                return Err(AppError::Serialization(format!(
                    "unknown managed template field {key} in {path}"
                )));
            }
        };
        if !valid {
            return Err(AppError::Serialization(format!(
                "{path} model field {key} has an invalid type"
            )));
        }
    }
    for required in [
        "slug",
        "display_name",
        "description",
        "default_reasoning_level",
        "supported_reasoning_levels",
        "truncation_policy",
        "context_window",
        "max_context_window",
        "input_modalities",
    ] {
        if !model.contains_key(required) {
            return Err(AppError::Serialization(format!(
                "{path} model has no {required}"
            )));
        }
    }
    Ok(())
}

fn validate_reasoning_levels(value: &Value, path: &str) -> AppResult<bool> {
    let Some(levels) = value.as_array() else {
        return Ok(false);
    };
    if levels.is_empty() {
        return Ok(false);
    }
    for level in levels {
        let Some(level) = level.as_object() else {
            return Ok(false);
        };
        reject_unknown_keys(
            level.keys().map(String::as_str),
            &["effort", "description"],
            path,
        )?;
        if level.get("effort").and_then(Value::as_str).is_none()
            || level.get("description").and_then(Value::as_str).is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_truncation_policy(value: &Value, path: &str) -> AppResult<bool> {
    let Some(policy) = value.as_object() else {
        return Ok(false);
    };
    reject_unknown_keys(policy.keys().map(String::as_str), &["mode", "limit"], path)?;
    Ok(policy.get("mode").and_then(Value::as_str).is_some()
        && policy.get("limit").and_then(Value::as_u64).is_some())
}

fn validate_opencode_config_shape(
    value: &Value,
    path: &str,
    provider_id: Option<&str>,
) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    let expected_schema = Some("https://opencode.ai/config.json");
    if root.get("$schema").and_then(Value::as_str) != expected_schema {
        return Err(AppError::Serialization(format!(
            "{path} has an invalid schema",
        )));
    }
    if let Some(provider_id) = provider_id {
        reject_unknown_keys(root.keys().map(String::as_str), &["$schema", "model"], path)?;
        let expected_model = format!("{provider_id}/<model-id>");
        if root.get("model").and_then(Value::as_str) != Some(expected_model.as_str()) {
            return Err(AppError::Serialization(format!(
                "{path} has an invalid native model reference",
            )));
        }
        return Ok(());
    }
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &["$schema", "model", "provider"],
        path,
    )?;
    if root.get("model").and_then(Value::as_str) != Some("<provider-id>/<model-id>") {
        return Err(AppError::Serialization(
            "generic OpenCode template has an invalid model reference".into(),
        ));
    }
    let providers = root
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization("generic OpenCode provider is invalid".into()))?;
    if providers.len() != 1 || !providers.contains_key("<provider-id>") {
        return Err(AppError::Serialization(
            "generic OpenCode template must contain one placeholder provider".into(),
        ));
    }
    let provider = providers["<provider-id>"]
        .as_object()
        .ok_or_else(|| AppError::Serialization("generic OpenCode provider is invalid".into()))?;
    reject_unknown_keys(
        provider.keys().map(String::as_str),
        &["npm", "name", "options", "models"],
        path,
    )?;
    if provider.get("npm").and_then(Value::as_str) != Some("<npm-package>")
        || provider.get("name").and_then(Value::as_str) != Some("<provider-name>")
    {
        return Err(AppError::Serialization(
            "generic OpenCode provider bindings are invalid".into(),
        ));
    }
    let options = provider
        .get("options")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization("generic OpenCode options are invalid".into()))?;
    // Credentials live in auth.json only; the reviewed generic template must not inline a key.
    reject_unknown_keys(options.keys().map(String::as_str), &["baseURL"], path)?;
    if options.get("baseURL").and_then(Value::as_str) != Some("<base-url>") {
        return Err(AppError::Serialization(
            "generic OpenCode option bindings are invalid".into(),
        ));
    }
    let models = provider
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization("generic OpenCode models are invalid".into()))?;
    if models.len() != 1 || !models.contains_key("<model-id>") {
        return Err(AppError::Serialization(
            "generic OpenCode template must contain one placeholder model".into(),
        ));
    }
    let model = models["<model-id>"]
        .as_object()
        .ok_or_else(|| AppError::Serialization("generic OpenCode model is invalid".into()))?;
    reject_unknown_keys(
        model.keys().map(String::as_str),
        &["name", "reasoning"],
        path,
    )?;
    if model.get("name").and_then(Value::as_str) != Some("<model-name>")
        || model.get("reasoning").and_then(Value::as_bool) != Some(true)
    {
        return Err(AppError::Serialization(
            "generic OpenCode model bindings are invalid".into(),
        ));
    }
    Ok(())
}

fn validate_opencode_auth_shape(
    value: &Value,
    path: &str,
    provider_id: Option<&str>,
) -> AppResult<()> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    let expected_key = provider_id.unwrap_or("<provider-id>");
    if root.len() != 1 || !root.contains_key(expected_key) {
        return Err(AppError::Serialization(format!(
            "{path} must contain its provider auth entry",
        )));
    }
    let auth = root[expected_key]
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} auth entry is not an object")))?;
    reject_unknown_keys(auth.keys().map(String::as_str), &["type", "key"], path)?;
    if auth.get("type").and_then(Value::as_str) != Some("api")
        || auth.get("key").and_then(Value::as_str) != Some("<your-api-key>")
    {
        return Err(AppError::Serialization(format!(
            "{path} has invalid auth bindings",
        )));
    }
    Ok(())
}

fn validate_toml_item_placeholders(item: &toml_edit::Item, path: &str) -> Result<(), String> {
    match item {
        toml_edit::Item::Value(value) => validate_toml_value_placeholders(value, path),
        toml_edit::Item::Table(table) => {
            for (key, item) in table.iter() {
                validate_placeholder_string(key, path)?;
                validate_toml_item_placeholders(item, path)?;
            }
            Ok(())
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                for (key, item) in table.iter() {
                    validate_placeholder_string(key, path)?;
                    validate_toml_item_placeholders(item, path)?;
                }
            }
            Ok(())
        }
        toml_edit::Item::None => Ok(()),
    }
}

fn validate_toml_value_placeholders(value: &toml_edit::Value, path: &str) -> Result<(), String> {
    match value {
        toml_edit::Value::String(value) => validate_placeholder_string(value.value(), path),
        toml_edit::Value::Array(values) => {
            for value in values.iter() {
                validate_toml_value_placeholders(value, path)?;
            }
            Ok(())
        }
        toml_edit::Value::InlineTable(table) => {
            for (key, value) in table.iter() {
                validate_placeholder_string(key, path)?;
                validate_toml_value_placeholders(value, path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_template_placeholders(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::String(value) => validate_placeholder_string(value, path),
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| validate_template_placeholders(value, path)),
        Value::Object(values) => values.iter().try_for_each(|(key, value)| {
            validate_placeholder_string(key, path)?;
            validate_template_placeholders(value, path)
        }),
        _ => Ok(()),
    }
}

const PLACEHOLDERS: [&str; 9] = [
    "<model-id>",
    "<model-name>",
    "<model-description>",
    "<provider-id>",
    "<provider-key>",
    "<provider-name>",
    "<base-url>",
    "<your-api-key>",
    "<npm-package>",
];

fn validate_placeholder_string(value: &str, path: &str) -> Result<(), String> {
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let candidate = &rest[start..];
        let Some(end) = candidate.find('>') else {
            return Err(format!("unterminated placeholder in {path}"));
        };
        let marker = &candidate[..=end];
        if !PLACEHOLDERS.contains(&marker) {
            return Err(format!("unknown placeholder {marker} in {path}"));
        }
        rest = &candidate[end + 1..];
    }
    Ok(())
}

pub struct TemplateSelection<'a> {
    pub cli_id: CliId,
    pub template_id: Option<&'a str>,
    pub protocol: CliProtocol,
    pub model: &'a str,
}

pub struct TemplateBindings<'a> {
    pub provider_id: &'a str,
    pub provider_name: &'a str,
    pub endpoint: &'a str,
    pub auth_type: ConnectionAuthType,
    pub api_key: &'a str,
    pub model: &'a str,
    pub model_catalog_path: Option<&'a Path>,
    pub qwen: Option<QwenTemplateBindings<'a>>,
}

#[derive(Clone, Copy)]
pub struct QwenTemplateBindings<'a> {
    pub group_id: &'a str,
    pub env_key: &'a str,
}

pub struct ResolvedTemplates {
    cli_id: CliId,
    protocol: CliProtocol,
    config_path: &'static str,
    model_catalog_path: Option<&'static str>,
    auth_path: Option<&'static str>,
}

pub struct ClaudeManagedConfig {
    pub schema: Option<String>,
    pub model: String,
    pub env: BTreeMap<String, String>,
}

pub struct CodexManagedConfig {
    pub model: String,
    pub provider_id: String,
    pub provider_name: String,
    pub endpoint: String,
    pub api_key: String,
    pub reasoning_effort: String,
    pub model_catalog_path: String,
    pub preferred_auth_method: Option<String>,
    pub forced_login_method: Option<String>,
    pub model_entry: Value,
}

pub struct OpenCodeManagedConfig {
    pub schema: String,
    pub model_reference: String,
    pub provider_id: String,
    pub api_key: String,
    pub kind: OpenCodeConfigKind,
}

/// Only the Generic mode defines a provider block: npm, display name, endpoint, and the selected
/// model entry. Native output delegates transport and model catalog to OpenCode itself.
pub enum OpenCodeConfigKind {
    Native,
    Generic {
        provider_name: String,
        endpoint: String,
        npm_package: String,
        model_name: String,
        reasoning: bool,
    },
}

/// Fixed connection contract extracted from a validated provider-native template. It never
/// depends on the runtime provider catalog.
#[derive(Debug, Clone)]
pub struct OpenCodeNativeContract {
    pub template_id: &'static str,
    pub native_provider_id: String,
    pub protocol: CliProtocol,
    pub endpoint: Url,
    pub auth_type: ConnectionAuthType,
}

pub enum OpenCodeWriteMode {
    Native,
    Generic,
}

/// Returns the fixed native contract when `template_id` is exactly one of the seven bundled
/// provider templates; unknown or missing IDs return `None`. Broken templates stay an error.
pub fn opencode_native_contract(
    template_id: Option<&str>,
) -> AppResult<Option<OpenCodeNativeContract>> {
    validate_bundled_templates()?;
    let Some(template_id) = known_template_id(template_id) else {
        return Ok(None);
    };
    let path = opencode_provider_path(template_id);
    let value: Value = serde_json::from_str(resource_text(path)?).map_err(|error| {
        AppError::Serialization(format!("invalid provider identity {path}: {error}"))
    })?;
    validate_provider_identity(&value, path, Some(template_id), Some("opencode"))?;
    let native_provider_id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Serialization(format!("{path} has no provider id")))?;
    let protocol = match value.get("protocol").and_then(Value::as_str) {
        Some("openai-compatible") => CliProtocol::OpenaiChat,
        Some(other) => {
            return Err(AppError::Serialization(format!(
                "{path} declares unsupported native protocol {other}"
            )));
        }
        None => {
            return Err(AppError::Serialization(format!("{path} has no protocol")));
        }
    };
    let endpoint = Url::parse(
        value
            .get("base_url")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Serialization(format!("{path} has no base_url")))?,
    )
    .map_err(|error| AppError::Serialization(format!("{path} has an invalid base_url: {error}")))?;
    if endpoint.scheme() != "https" {
        return Err(AppError::Serialization(format!(
            "{path} base_url must use HTTPS"
        )));
    }
    Ok(Some(OpenCodeNativeContract {
        template_id,
        native_provider_id: native_provider_id.to_string(),
        protocol,
        endpoint,
        // This version only bundles `openai-compatible` native templates.
        auth_type: ConnectionAuthType::Bearer,
    }))
}

/// Chooses Native versus Generic output for OpenCode from the saved template identity and the
/// saved connection. A known provider keeps its native slot only when the connection matches the
/// fixed contract exactly; any other legal connection uses the generic namespaced template.
pub fn opencode_write_mode(
    template_id: Option<&str>,
    protocol: CliProtocol,
    endpoint: &Url,
    auth_type: ConnectionAuthType,
) -> AppResult<OpenCodeWriteMode> {
    // Every render path needs a fixed package mapping; unsupported protocols must not fall
    // through to a guessed template.
    npm_package_for_protocol(protocol)?;
    let contract = opencode_native_contract(template_id)?;
    Ok(match contract {
        Some(contract)
            if contract.protocol == protocol
                && contract.endpoint == *endpoint
                && contract.auth_type == auth_type =>
        {
            OpenCodeWriteMode::Native
        }
        Some(_) | None => OpenCodeWriteMode::Generic,
    })
}

pub struct QwenManagedConfig {
    pub group_id: String,
    pub protocol: String,
    pub model: String,
    pub endpoint: String,
    pub env_key: String,
    pub api_key: String,
    pub model_entry: Value,
}

pub enum RenderedManagedConfig {
    Claude(ClaudeManagedConfig),
    Codex(CodexManagedConfig),
    OpenCode(OpenCodeManagedConfig),
    Qwen(QwenManagedConfig),
}

pub fn resolve_templates(selection: &TemplateSelection<'_>) -> AppResult<ResolvedTemplates> {
    validate_bundled_templates()?;
    if selection.model.trim().is_empty() {
        return Err(AppError::Validation(
            "template model cannot be empty".into(),
        ));
    }
    match selection.cli_id {
        CliId::ClaudeCode if selection.protocol != CliProtocol::AnthropicMessages => Err(
            AppError::Validation("Claude Code templates require Anthropic Messages".into()),
        ),
        CliId::Codex if selection.protocol != CliProtocol::OpenaiResponses => Err(
            AppError::Validation("Codex templates require the Responses protocol".into()),
        ),
        CliId::Qwen if selection.protocol != CliProtocol::OpenaiChat => Err(AppError::Validation(
            "Qwen Code templates require OpenAI Chat Completions".into(),
        )),
        CliId::ClaudeCode => {
            let template_id = known_template_id(selection.template_id);
            let config_path = template_id
                .map(claude_settings_path)
                .unwrap_or("claude/settings.json");
            Ok(ResolvedTemplates {
                cli_id: selection.cli_id,
                protocol: selection.protocol,
                config_path,
                model_catalog_path: None,
                auth_path: None,
            })
        }
        CliId::Codex => {
            let template_id = known_template_id(selection.template_id);
            let config_path = template_id
                .map(codex_config_path)
                .unwrap_or("codex/config.toml");
            let model_catalog_path = if template_id == Some("deepseek")
                && DEEPSEEK_EXACT_MODEL_IDS.contains(&selection.model)
            {
                Some(deepseek_exact_model_path(selection.model))
            } else {
                Some(
                    template_id
                        .map(codex_models_path)
                        .unwrap_or("codex/models.json"),
                )
            };
            Ok(ResolvedTemplates {
                cli_id: selection.cli_id,
                protocol: selection.protocol,
                config_path,
                model_catalog_path,
                auth_path: None,
            })
        }
        CliId::Opencode => {
            let template_id = known_template_id(selection.template_id);
            let (config_path, auth_path) = match template_id {
                Some(template_id) => (
                    opencode_config_path(template_id),
                    opencode_auth_path(template_id),
                ),
                None => (OPENCODE_GENERIC_CONFIG_PATH, OPENCODE_GENERIC_AUTH_PATH),
            };
            Ok(ResolvedTemplates {
                cli_id: selection.cli_id,
                protocol: selection.protocol,
                config_path,
                model_catalog_path: None,
                auth_path: Some(auth_path),
            })
        }
        CliId::Qwen => {
            let template_id = known_template_id(selection.template_id);
            Ok(ResolvedTemplates {
                cli_id: selection.cli_id,
                protocol: selection.protocol,
                config_path: template_id
                    .map(qwen_settings_path)
                    .unwrap_or("qwen/settings.json"),
                model_catalog_path: None,
                auth_path: None,
            })
        }
    }
}

pub fn render_managed_config(
    templates: &ResolvedTemplates,
    bindings: &TemplateBindings<'_>,
) -> AppResult<RenderedManagedConfig> {
    if bindings.model.trim().is_empty() || bindings.provider_id.trim().is_empty() {
        return Err(AppError::Validation(
            "template provider and model bindings cannot be empty".into(),
        ));
    }
    match templates.cli_id {
        CliId::ClaudeCode => render_claude(templates, bindings).map(RenderedManagedConfig::Claude),
        CliId::Codex => render_codex(templates, bindings).map(RenderedManagedConfig::Codex),
        CliId::Opencode => {
            render_opencode(templates, bindings).map(RenderedManagedConfig::OpenCode)
        }
        CliId::Qwen => render_qwen(templates, bindings).map(RenderedManagedConfig::Qwen),
    }
}

fn render_qwen(
    templates: &ResolvedTemplates,
    bindings: &TemplateBindings<'_>,
) -> AppResult<QwenManagedConfig> {
    let template: Value =
        serde_json::from_str(resource_text(templates.config_path)?).map_err(|error| {
            AppError::Serialization(format!(
                "invalid JSON template {}: {error}",
                templates.config_path
            ))
        })?;
    validate_qwen_template_shape(&template, templates.config_path)?;
    validate_template_placeholders(&template, templates.config_path)
        .map_err(AppError::Serialization)?;
    let qwen = bindings
        .qwen
        .ok_or_else(|| AppError::Validation("Qwen template bindings are unavailable".into()))?;
    if qwen.group_id.trim().is_empty() || !valid_qwen_env_key(qwen.env_key) {
        return Err(AppError::Validation(
            "Qwen group and environment bindings are invalid".into(),
        ));
    }
    let model_entry = serde_json::json!({
        "id": bindings.model,
        "name": bindings.model,
        "envKey": qwen.env_key,
        "baseUrl": bindings.endpoint,
    });
    Ok(QwenManagedConfig {
        group_id: qwen.group_id.into(),
        protocol: "openai".into(),
        model: bindings.model.into(),
        endpoint: bindings.endpoint.into(),
        env_key: qwen.env_key.into(),
        api_key: bindings.api_key.into(),
        model_entry,
    })
}

fn valid_qwen_env_key(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_uppercase())
        && characters.all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

fn render_claude(
    templates: &ResolvedTemplates,
    bindings: &TemplateBindings<'_>,
) -> AppResult<ClaudeManagedConfig> {
    let value = render_json_resource(templates.config_path, templates.protocol, bindings)?;
    let root = value.as_object().ok_or_else(|| {
        AppError::Serialization(format!("{} root is not an object", templates.config_path))
    })?;
    let allowed_root = ["$schema", "model", "env"];
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &allowed_root,
        templates.config_path,
    )?;
    let schema = root
        .get("$schema")
        .map(required_string)
        .transpose()?
        .map(str::to_string);
    let model = required_string(root.get("model").ok_or_else(|| {
        AppError::Serialization(format!("{} has no model", templates.config_path))
    })?)?
    .to_string();
    let env = root.get("env").and_then(Value::as_object).ok_or_else(|| {
        AppError::Serialization(format!("{} env is not an object", templates.config_path))
    })?;
    reject_unknown_keys(
        env.keys().map(String::as_str),
        &CLAUDE_MANAGED_ENV_FIELDS,
        templates.config_path,
    )?;
    let mut env = env
        .iter()
        .map(|(key, value)| Ok((key.clone(), required_string(value)?.to_string())))
        .collect::<AppResult<BTreeMap<_, _>>>()?;
    env.insert("ANTHROPIC_BASE_URL".into(), bindings.endpoint.into());
    env.remove("ANTHROPIC_API_KEY");
    env.remove("ANTHROPIC_AUTH_TOKEN");
    match bindings.auth_type {
        ConnectionAuthType::ApiKey => {
            env.insert("ANTHROPIC_API_KEY".into(), bindings.api_key.into());
        }
        ConnectionAuthType::Bearer => {
            env.insert("ANTHROPIC_AUTH_TOKEN".into(), bindings.api_key.into());
        }
    }
    env.remove("CLAUDE_CODE_OAUTH_TOKEN");
    Ok(ClaudeManagedConfig { schema, model, env })
}

fn render_codex(
    templates: &ResolvedTemplates,
    bindings: &TemplateBindings<'_>,
) -> AppResult<CodexManagedConfig> {
    let text = resource_text(templates.config_path)?;
    let document = parse_toml(text)?;
    let allowed = [
        "model",
        "model_provider",
        "model_reasoning_effort",
        "model_catalog_json",
        "preferred_auth_method",
        "forced_login_method",
        "model_providers",
    ];
    reject_unknown_keys(
        document.as_table().iter().map(|(key, _)| key),
        &allowed,
        templates.config_path,
    )?;
    for key in ["model", "model_provider", "model_catalog_json"] {
        toml_string(&document, key, templates.config_path)?;
    }
    let reasoning_effort = toml_string(&document, "model_reasoning_effort", templates.config_path)?;
    let preferred_auth_method =
        optional_toml_string(&document, "preferred_auth_method", templates.config_path)?;
    let forced_login_method =
        optional_toml_string(&document, "forced_login_method", templates.config_path)?;
    let providers = document
        .get("model_providers")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(|| {
            AppError::Serialization(format!(
                "{} has no model_providers table",
                templates.config_path
            ))
        })?;
    if providers.len() != 1 {
        return Err(AppError::Serialization(format!(
            "{} must contain one provider table",
            templates.config_path
        )));
    }
    let provider = providers
        .iter()
        .next()
        .and_then(|(_, item)| item.as_table())
        .ok_or_else(|| {
            AppError::Serialization(format!(
                "{} provider entry is not a table",
                templates.config_path
            ))
        })?;
    reject_unknown_keys(
        provider.iter().map(|(key, _)| key),
        &[
            "name",
            "base_url",
            "wire_api",
            "experimental_bearer_token",
            "env_key",
        ],
        templates.config_path,
    )?;
    if provider.get("wire_api").and_then(toml_edit::Item::as_str) != Some("responses") {
        return Err(AppError::Serialization(format!(
            "{} has an invalid wire_api",
            templates.config_path
        )));
    }
    for key in ["name", "base_url"] {
        if provider
            .get(key)
            .and_then(toml_edit::Item::as_str)
            .is_none()
        {
            return Err(AppError::Serialization(format!(
                "{} provider field {key} must be a string",
                templates.config_path
            )));
        }
    }
    for key in ["experimental_bearer_token", "env_key"] {
        if provider
            .get(key)
            .is_some_and(|item| item.as_str().is_none())
        {
            return Err(AppError::Serialization(format!(
                "{} provider field {key} must be a string",
                templates.config_path
            )));
        }
    }
    let catalog_output_path = bindings.model_catalog_path.ok_or_else(|| {
        AppError::Validation("Codex model catalog output path is unavailable".into())
    })?;
    if !catalog_output_path.is_absolute() {
        return Err(AppError::Validation(
            "Codex model catalog path must be absolute".into(),
        ));
    }
    let catalog_output_path = catalog_output_path.to_str().ok_or_else(|| {
        AppError::Validation("Codex model catalog path is not valid UTF-8".into())
    })?;
    let models_path = templates
        .model_catalog_path
        .ok_or_else(|| AppError::Serialization("Codex model template is unavailable".into()))?;
    let rendered = render_json_resource(models_path, templates.protocol, bindings)?;
    let root = rendered
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{models_path} root is not an object")))?;
    reject_unknown_keys(root.keys().map(String::as_str), &["models"], models_path)?;
    let models = root
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Serialization(format!("{models_path} models is not an array")))?;
    if models.len() != 1 || !models[0].is_object() {
        return Err(AppError::Serialization(format!(
            "{models_path} must contain exactly one model"
        )));
    }
    let mut model_entry = models[0].clone();
    model_entry
        .as_object_mut()
        .expect("checked object")
        .insert("slug".into(), Value::String(bindings.model.into()));
    Ok(CodexManagedConfig {
        model: bindings.model.into(),
        provider_id: bindings.provider_id.into(),
        provider_name: bindings.provider_name.into(),
        endpoint: bindings.endpoint.into(),
        api_key: bindings.api_key.into(),
        reasoning_effort,
        model_catalog_path: catalog_output_path.into(),
        preferred_auth_method,
        forced_login_method,
        model_entry,
    })
}

const OPENCODE_GENERIC_CONFIG_PATH: &str = "opencode/opencode.jsonc";
const OPENCODE_GENERIC_AUTH_PATH: &str = "opencode/auth.json";

fn render_opencode(
    templates: &ResolvedTemplates,
    bindings: &TemplateBindings<'_>,
) -> AppResult<OpenCodeManagedConfig> {
    let native = templates.config_path != OPENCODE_GENERIC_CONFIG_PATH;
    let auth_path = templates
        .auth_path
        .ok_or_else(|| AppError::Serialization("OpenCode auth template is unavailable".into()))?;
    if native {
        // Native output keeps the provider-native model reference and the same-named auth slot.
        // The namespaced instance identity is intentionally not written in this mode.
        let selected = render_jsonc_resource(templates.config_path, templates.protocol, bindings)?;
        let root = selected.as_object().ok_or_else(|| {
            AppError::Serialization(format!("{} root is not an object", templates.config_path))
        })?;
        reject_unknown_keys(
            root.keys().map(String::as_str),
            &["$schema", "model"],
            templates.config_path,
        )?;
        let schema = root
            .get("$schema")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Serialization(format!("{} has no schema", templates.config_path))
            })?
            .to_string();
        let model_reference = root
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Serialization(format!("{} has no model", templates.config_path))
            })?
            .to_string();
        let provider_id = model_reference
            .split_once('/')
            .map(|(provider_id, _)| provider_id.to_string())
            .filter(|provider_id| !provider_id.is_empty())
            .ok_or_else(|| {
                AppError::Serialization(format!(
                    "{} has an invalid native model reference",
                    templates.config_path
                ))
            })?;
        let (auth_provider_id, api_key) =
            render_opencode_auth_template(auth_path, templates.protocol, bindings)?;
        if auth_provider_id != provider_id {
            return Err(AppError::Serialization(format!(
                "{auth_path} does not authenticate native provider {provider_id}"
            )));
        }
        return Ok(OpenCodeManagedConfig {
            schema,
            model_reference,
            provider_id: provider_id.to_string(),
            api_key,
            kind: OpenCodeConfigKind::Native,
        });
    }
    let generic =
        render_jsonc_resource(OPENCODE_GENERIC_CONFIG_PATH, templates.protocol, bindings)?;
    let root = generic.as_object().ok_or_else(|| {
        AppError::Serialization("generic OpenCode template root is not an object".into())
    })?;
    reject_unknown_keys(
        root.keys().map(String::as_str),
        &["$schema", "model", "provider"],
        OPENCODE_GENERIC_CONFIG_PATH,
    )?;
    let schema = root
        .get("$schema")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Serialization("generic OpenCode template has no schema".into()))?;
    let provider = root
        .get("provider")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(bindings.provider_id))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Serialization("generic OpenCode provider rendering failed".into())
        })?;
    reject_unknown_keys(
        provider.keys().map(String::as_str),
        &["npm", "name", "options", "models"],
        OPENCODE_GENERIC_CONFIG_PATH,
    )?;
    let options = provider
        .get("options")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization("generic OpenCode options are invalid".into()))?;
    reject_unknown_keys(
        options.keys().map(String::as_str),
        &["baseURL"],
        OPENCODE_GENERIC_CONFIG_PATH,
    )?;
    let model = provider
        .get("models")
        .and_then(Value::as_object)
        .and_then(|models| models.get(bindings.model))
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Serialization("generic OpenCode model rendering failed".into()))?;
    reject_unknown_keys(
        model.keys().map(String::as_str),
        &["name", "reasoning"],
        OPENCODE_GENERIC_CONFIG_PATH,
    )?;
    let npm_package = provider
        .get("npm")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Serialization("generic OpenCode package is invalid".into()))?;
    let (auth_provider_id, api_key) =
        render_opencode_auth_template(auth_path, templates.protocol, bindings)?;
    if auth_provider_id != bindings.provider_id {
        return Err(AppError::Serialization(
            "generic OpenCode auth template does not match the rendered provider".into(),
        ));
    }
    Ok(OpenCodeManagedConfig {
        schema: schema.into(),
        model_reference: format!("{}/{}", bindings.provider_id, bindings.model),
        provider_id: bindings.provider_id.into(),
        api_key,
        kind: OpenCodeConfigKind::Generic {
            provider_name: bindings.provider_name.into(),
            endpoint: bindings.endpoint.into(),
            npm_package: npm_package.into(),
            model_name: model
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::Serialization("generic OpenCode model name is invalid".into())
                })?
                .into(),
            reasoning: model
                .get("reasoning")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    AppError::Serialization("generic OpenCode reasoning flag is invalid".into())
                })?,
        },
    })
}

/// Renders the selected `auth.json` template and returns its single rendered root key together
/// with the rendered API key. Both modes must actually read their selected template.
fn render_opencode_auth_template(
    path: &str,
    protocol: CliProtocol,
    bindings: &TemplateBindings<'_>,
) -> AppResult<(String, String)> {
    let auth = render_jsonc_resource(path, protocol, bindings)?;
    let auth = auth
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} root is not an object")))?;
    if auth.len() != 1 {
        return Err(AppError::Serialization(format!(
            "{path} must contain one auth entry"
        )));
    }
    let (provider_id, entry) = auth.iter().next().expect("checked length");
    let entry = entry
        .as_object()
        .ok_or_else(|| AppError::Serialization(format!("{path} auth entry is not an object")))?;
    reject_unknown_keys(entry.keys().map(String::as_str), &["type", "key"], path)?;
    if entry.get("type").and_then(Value::as_str) != Some("api") {
        return Err(AppError::Serialization(format!(
            "{path} has an invalid auth type"
        )));
    }
    let api_key = entry
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Serialization(format!("{path} has no API key")))?
        .to_string();
    if api_key != bindings.api_key {
        return Err(AppError::Serialization(format!(
            "{path} has an inconsistent API key binding"
        )));
    }
    Ok((provider_id.clone(), api_key))
}

pub fn npm_package_for_protocol(protocol: CliProtocol) -> AppResult<String> {
    let package = crate::catalog::runtime_catalog()?
        .protocol_package(CliId::Opencode, protocol)
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::Serialization(format!(
                "OpenCode has no fixed package mapping for {protocol}"
            ))
        })?;
    if ![
        "@ai-sdk/anthropic",
        "@ai-sdk/openai",
        "@ai-sdk/openai-compatible",
    ]
    .contains(&package.as_str())
    {
        return Err(AppError::Serialization(format!(
            "OpenCode package {package} is outside the template allowlist"
        )));
    }
    Ok(package)
}

fn render_json_resource(
    path: &str,
    protocol: CliProtocol,
    bindings: &TemplateBindings<'_>,
) -> AppResult<Value> {
    let mut value: Value = serde_json::from_str(resource_text(path)?).map_err(|error| {
        AppError::Serialization(format!("invalid JSON template {path}: {error}"))
    })?;
    render_json_value(&mut value, protocol, bindings, path)?;
    Ok(value)
}

fn render_jsonc_resource(
    path: &str,
    protocol: CliProtocol,
    bindings: &TemplateBindings<'_>,
) -> AppResult<Value> {
    let mut value: Value = parse_jsonc_value(resource_text(path)?).map_err(|error| {
        AppError::Serialization(format!("invalid JSONC template {path}: {error}"))
    })?;
    render_json_value(&mut value, protocol, bindings, path)?;
    Ok(value)
}

fn render_json_value(
    value: &mut Value,
    protocol: CliProtocol,
    bindings: &TemplateBindings<'_>,
    path: &str,
) -> AppResult<()> {
    match value {
        Value::String(value) => {
            *value = render_string(value, protocol, bindings, path)?;
        }
        Value::Array(values) => {
            for value in values {
                render_json_value(value, protocol, bindings, path)?;
            }
        }
        Value::Object(values) => {
            let old = std::mem::take(values);
            for (key, mut value) in old {
                let key = render_string(&key, protocol, bindings, path)?;
                render_json_value(&mut value, protocol, bindings, path)?;
                if values.insert(key.clone(), value).is_some() {
                    return Err(AppError::Serialization(format!(
                        "template key collision at {key} in {path}"
                    )));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn render_string(
    input: &str,
    protocol: CliProtocol,
    bindings: &TemplateBindings<'_>,
    path: &str,
) -> AppResult<String> {
    validate_placeholder_string(input, path).map_err(AppError::Serialization)?;
    let mut package = None;
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = input[cursor..].find('<') {
        let start = cursor + relative;
        output.push_str(&input[cursor..start]);
        let end = input[start..].find('>').expect("placeholder validation") + start;
        let marker = &input[start..=end];
        let replacement = match marker {
            "<model-id>" => bindings.model,
            "<model-name>" | "<model-description>" => bindings.model,
            "<provider-id>" | "<provider-key>" => bindings.provider_id,
            "<provider-name>" => bindings.provider_name,
            "<base-url>" => bindings.endpoint,
            "<your-api-key>" => bindings.api_key,
            "<npm-package>" => package
                .get_or_insert(npm_package_for_protocol(protocol)?)
                .as_str(),
            _ => unreachable!("placeholder validation"),
        };
        output.push_str(replacement);
        cursor = end + 1;
        if marker == "<model-id>"
            && input[cursor..].starts_with("[1m]")
            && bindings.model.ends_with("[1m]")
        {
            cursor += "[1m]".len();
        }
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

fn resource_text(path: &str) -> AppResult<&'static str> {
    let resource = RESOURCES
        .iter()
        .find(|resource| resource.path == path)
        .ok_or_else(|| {
            AppError::Serialization(format!(
                "compiled config-template resource {path} is unavailable"
            ))
        })?;
    std::str::from_utf8(resource.bytes)
        .map_err(|error| AppError::Serialization(format!("template {path} is not UTF-8: {error}")))
}

fn known_template_id(template_id: Option<&str>) -> Option<&'static str> {
    PROVIDER_TEMPLATE_IDS
        .into_iter()
        .find(|candidate| Some(*candidate) == template_id)
}

fn known_exact_model_id(model_id: &str) -> Option<&'static str> {
    DEEPSEEK_EXACT_MODEL_IDS
        .into_iter()
        .find(|candidate| *candidate == model_id)
}

fn claude_provider_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "claude/deepseek/provider.json",
        "zhipuai" => "claude/zhipuai/provider.json",
        "zhipuai-coding-plan" => "claude/zhipuai-coding-plan/provider.json",
        "zai" => "claude/zai/provider.json",
        "zai-coding-plan" => "claude/zai-coding-plan/provider.json",
        "opencode" => "claude/opencode/provider.json",
        "opencode-go" => "claude/opencode-go/provider.json",
        _ => unreachable!("known provider template"),
    }
}

fn claude_settings_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "claude/deepseek/settings.json",
        "zhipuai" => "claude/zhipuai/settings.json",
        "zhipuai-coding-plan" => "claude/zhipuai-coding-plan/settings.json",
        "zai" => "claude/zai/settings.json",
        "zai-coding-plan" => "claude/zai-coding-plan/settings.json",
        "opencode" => "claude/opencode/settings.json",
        "opencode-go" => "claude/opencode-go/settings.json",
        _ => unreachable!("known provider template"),
    }
}

fn codex_config_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "codex/deepseek/config.toml",
        "zhipuai" => "codex/zhipuai/config.toml",
        "zhipuai-coding-plan" => "codex/zhipuai-coding-plan/config.toml",
        "zai" => "codex/zai/config.toml",
        "zai-coding-plan" => "codex/zai-coding-plan/config.toml",
        "opencode" => "codex/opencode/config.toml",
        "opencode-go" => "codex/opencode-go/config.toml",
        _ => unreachable!("known provider template"),
    }
}

fn codex_provider_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "codex/deepseek/provider.json",
        "zhipuai" => "codex/zhipuai/provider.json",
        "zhipuai-coding-plan" => "codex/zhipuai-coding-plan/provider.json",
        "zai" => "codex/zai/provider.json",
        "zai-coding-plan" => "codex/zai-coding-plan/provider.json",
        "opencode" => "codex/opencode/provider.json",
        "opencode-go" => "codex/opencode-go/provider.json",
        _ => unreachable!("known provider template"),
    }
}

fn codex_models_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "codex/deepseek/models.json",
        "zhipuai" => "codex/zhipuai/models.json",
        "zhipuai-coding-plan" => "codex/zhipuai-coding-plan/models.json",
        "zai" => "codex/zai/models.json",
        "zai-coding-plan" => "codex/zai-coding-plan/models.json",
        "opencode" => "codex/opencode/models.json",
        "opencode-go" => "codex/opencode-go/models.json",
        _ => unreachable!("known provider template"),
    }
}

fn deepseek_exact_model_path(model: &str) -> &'static str {
    match model {
        "deepseek-v4-flash" => "codex/deepseek/deepseek-v4-flash/models.json",
        "deepseek-v4-pro" => "codex/deepseek/deepseek-v4-pro/models.json",
        "deepseek-v4-flash-vision-exp" => "codex/deepseek/deepseek-v4-flash-vision-exp/models.json",
        _ => unreachable!("known DeepSeek exact model"),
    }
}

fn opencode_config_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "opencode/deepseek/opencode.jsonc",
        "zhipuai" => "opencode/zhipuai/opencode.jsonc",
        "zhipuai-coding-plan" => "opencode/zhipuai-coding-plan/opencode.jsonc",
        "zai" => "opencode/zai/opencode.jsonc",
        "zai-coding-plan" => "opencode/zai-coding-plan/opencode.jsonc",
        "opencode" => "opencode/opencode/opencode.jsonc",
        "opencode-go" => "opencode/opencode-go/opencode.jsonc",
        _ => unreachable!("known provider template"),
    }
}

fn opencode_provider_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "opencode/deepseek/provider.json",
        "zhipuai" => "opencode/zhipuai/provider.json",
        "zhipuai-coding-plan" => "opencode/zhipuai-coding-plan/provider.json",
        "zai" => "opencode/zai/provider.json",
        "zai-coding-plan" => "opencode/zai-coding-plan/provider.json",
        "opencode" => "opencode/opencode/provider.json",
        "opencode-go" => "opencode/opencode-go/provider.json",
        _ => unreachable!("known provider template"),
    }
}

fn opencode_auth_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "opencode/deepseek/auth.json",
        "zhipuai" => "opencode/zhipuai/auth.json",
        "zhipuai-coding-plan" => "opencode/zhipuai-coding-plan/auth.json",
        "zai" => "opencode/zai/auth.json",
        "zai-coding-plan" => "opencode/zai-coding-plan/auth.json",
        "opencode" => "opencode/opencode/auth.json",
        "opencode-go" => "opencode/opencode-go/auth.json",
        _ => unreachable!("known provider template"),
    }
}

fn qwen_provider_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "qwen/deepseek/provider.json",
        "zhipuai" => "qwen/zhipuai/provider.json",
        "zhipuai-coding-plan" => "qwen/zhipuai-coding-plan/provider.json",
        "zai" => "qwen/zai/provider.json",
        "zai-coding-plan" => "qwen/zai-coding-plan/provider.json",
        "opencode" => "qwen/opencode/provider.json",
        "opencode-go" => "qwen/opencode-go/provider.json",
        _ => unreachable!("known provider template"),
    }
}

fn qwen_settings_path(template_id: &str) -> &'static str {
    match template_id {
        "deepseek" => "qwen/deepseek/settings.json",
        "zhipuai" => "qwen/zhipuai/settings.json",
        "zhipuai-coding-plan" => "qwen/zhipuai-coding-plan/settings.json",
        "zai" => "qwen/zai/settings.json",
        "zai-coding-plan" => "qwen/zai-coding-plan/settings.json",
        "opencode" => "qwen/opencode/settings.json",
        "opencode-go" => "qwen/opencode-go/settings.json",
        _ => unreachable!("known provider template"),
    }
}

fn reject_unknown_keys<'a>(
    actual: impl Iterator<Item = &'a str>,
    allowed: &[&str],
    path: &str,
) -> AppResult<()> {
    for key in actual {
        if !allowed.contains(&key) {
            return Err(AppError::Serialization(format!(
                "unknown managed template field {key} in {path}"
            )));
        }
    }
    Ok(())
}

fn required_string(value: &Value) -> AppResult<&str> {
    value.as_str().ok_or_else(|| {
        AppError::Serialization("managed JSON template field must be a string".into())
    })
}

fn toml_string(document: &toml_edit::DocumentMut, key: &str, path: &str) -> AppResult<String> {
    document
        .get(key)
        .and_then(toml_edit::Item::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::Serialization(format!("{path} field {key} must be a string")))
}

fn optional_toml_string(
    document: &toml_edit::DocumentMut,
    key: &str,
    path: &str,
) -> AppResult<Option<String>> {
    document
        .get(key)
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                AppError::Serialization(format!("{path} field {key} must be a string"))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings<'a>(model: &'a str, catalog: &'a Path) -> TemplateBindings<'a> {
        TemplateBindings {
            provider_id: "cliswitch_1234",
            provider_name: "Provider \"雪\"",
            endpoint: "https://example.test/a b",
            auth_type: ConnectionAuthType::Bearer,
            api_key: "fixture-<model-id>-key",
            model,
            model_catalog_path: Some(catalog),
            qwen: None,
        }
    }

    fn manifest_entry(path: &str) -> ManifestResource {
        serde_json::from_str::<Manifest>(MANIFEST)
            .unwrap()
            .resources
            .into_iter()
            .find(|entry| entry.path == path)
            .unwrap()
    }

    #[test]
    fn bundled_resources_are_complete_and_match_the_manifest() {
        validate_bundled_templates().unwrap();
        assert_eq!(RESOURCES.len(), 80);
        for (path, expected) in [
            (
                "claude/settings.json",
                "e2690758e6f97d847369f0bb925fd4221467c9b6a1faab8ab2f53c90e7022d90",
            ),
            (
                "codex/config.toml",
                "983f2914a92d05bbf4276b010959f5786c1285a6611920471b5b6c0cc604caac",
            ),
            (
                "codex/models.json",
                "0ab179be55dc6611f7010f1ff84739545c2e73e61ed48b5ec3c270066fc37c3f",
            ),
            (
                "opencode/opencode.jsonc",
                "ba2ef9231226420839bb4e58b2139c2a1047e28bb17b6b6f1321f5867a28e9d5",
            ),
            (
                "opencode/auth.json",
                "4e3774157e7aa55addbb78c1909c7be9090c55d0a563549cbe3090a04741a6fb",
            ),
            (
                "qwen/settings.json",
                "012c170be01b332e3114330430cdff89b88b7a1b9df6bf86ee27de1255681b19",
            ),
        ] {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(resource_text(path).unwrap().as_bytes())
                ),
                expected
            );
        }
    }

    #[test]
    fn opencode_sources_are_bound_to_the_fixed_newer_commit_and_everything_else_to_the_default() {
        validate_bundled_templates().unwrap();
        let manifest: Manifest = serde_json::from_str(MANIFEST).unwrap();
        let mut opencode = 0;
        for entry in &manifest.resources {
            if entry.path.starts_with("opencode/") {
                opencode += 1;
                assert_eq!(
                    entry.upstream_commit.as_deref(),
                    Some(OPENCODE_UPSTREAM_COMMIT)
                );
                assert_eq!(
                    entry.upstream_path.as_deref(),
                    Some(format!("cli/{}", entry.path).as_str())
                );
            } else {
                assert_eq!(entry.upstream_commit, None);
                assert_eq!(entry.upstream_path, None);
            }
        }
        assert_eq!(opencode, 23);
    }

    #[test]
    fn manifest_source_overrides_outside_the_compiled_boundary_are_rejected() {
        let mut opencode = manifest_entry("opencode/auth.json");
        assert!(validate_manifest_source(&opencode).is_ok());
        opencode.upstream_commit = Some(OPENCODE_UPSTREAM_COMMIT.into());
        opencode.upstream_path = Some("cli/other/auth.json".into());
        assert!(validate_manifest_source(&opencode).is_err());
        opencode.upstream_path = None;
        assert!(validate_manifest_source(&opencode).is_err());

        let mut other = manifest_entry("codex/config.toml");
        assert!(validate_manifest_source(&other).is_ok());
        other.upstream_commit = Some(OPENCODE_UPSTREAM_COMMIT.into());
        other.upstream_path = Some("cli/opencode/auth.json".into());
        assert!(validate_manifest_source(&other).is_err());
    }

    #[test]
    fn commented_generic_opencode_template_parses_and_rejects_unknown_fields() {
        let text = resource_text(OPENCODE_GENERIC_CONFIG_PATH).unwrap();
        assert!(text.contains("//"));
        let value = parse_jsonc_value(text).unwrap();
        validate_opencode_config_shape(&value, OPENCODE_GENERIC_CONFIG_PATH, None).unwrap();
        let mut invalid = value.clone();
        invalid["provider"]["<provider-id>"]["options"]["apiKey"] =
            Value::String("<your-api-key>".into());
        assert!(
            validate_opencode_config_shape(&invalid, OPENCODE_GENERIC_CONFIG_PATH, None).is_err()
        );
    }

    #[test]
    fn generic_opencode_auth_template_only_accepts_the_placeholder_slot() {
        let value = parse_jsonc_value(resource_text(OPENCODE_GENERIC_AUTH_PATH).unwrap()).unwrap();
        validate_opencode_auth_shape(&value, OPENCODE_GENERIC_AUTH_PATH, None).unwrap();
        let mut wrong_key = value.clone();
        let placeholder_entry = wrong_key["<provider-id>"].clone();
        wrong_key
            .as_object_mut()
            .unwrap()
            .insert("deepseek".into(), placeholder_entry);
        assert!(
            validate_opencode_auth_shape(&wrong_key, OPENCODE_GENERIC_AUTH_PATH, None).is_err()
        );
    }

    #[test]
    fn unknown_or_unterminated_placeholders_are_rejected() {
        assert!(validate_placeholder_string("<future-field>", "fixture").is_err());
        assert!(validate_placeholder_string("<model-id", "fixture").is_err());
    }

    #[test]
    fn unknown_managed_fields_are_rejected_at_every_template_boundary() {
        let mut provider: Value =
            serde_json::from_str(resource_text("claude/deepseek/provider.json").unwrap()).unwrap();
        provider
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(
            validate_provider_identity(
                &provider,
                "claude/deepseek/provider.json",
                Some("deepseek"),
                Some("claude-code")
            )
            .is_err()
        );

        let mut claude: Value =
            serde_json::from_str(resource_text("claude/settings.json").unwrap()).unwrap();
        claude["env"]
            .as_object_mut()
            .unwrap()
            .insert("FUTURE_FIELD".into(), Value::String("value".into()));
        assert!(validate_claude_template_shape(&claude, "fixture").is_err());

        let codex = parse_toml(&format!(
            "future = true\n{}",
            resource_text("codex/config.toml").unwrap()
        ))
        .unwrap();
        assert!(validate_codex_config_shape(&codex, "fixture").is_err());

        let mut models: Value =
            serde_json::from_str(resource_text("codex/models.json").unwrap()).unwrap();
        models["models"][0]
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(validate_codex_model_catalog_shape(&models, "fixture").is_err());

        let mut opencode: Value =
            parse_jsonc_value(resource_text(OPENCODE_GENERIC_CONFIG_PATH).unwrap()).unwrap();
        opencode["provider"]["<provider-id>"]["models"]["<model-id>"]
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(
            validate_opencode_config_shape(&opencode, OPENCODE_GENERIC_CONFIG_PATH, None).is_err()
        );

        let mut native_config: Value = parse_jsonc_value(
            resource_text("opencode/zhipuai-coding-plan/opencode.jsonc").unwrap(),
        )
        .unwrap();
        native_config
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(
            validate_opencode_config_shape(
                &native_config,
                "opencode/zhipuai-coding-plan/opencode.jsonc",
                Some("zhipuai-coding-plan")
            )
            .is_err()
        );

        let mut auth: Value =
            serde_json::from_str(resource_text("opencode/deepseek/auth.json").unwrap()).unwrap();
        auth["deepseek"]
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(
            validate_opencode_auth_shape(&auth, "opencode/deepseek/auth.json", Some("deepseek"))
                .is_err()
        );

        let mut qwen: Value =
            serde_json::from_str(resource_text("qwen/settings.json").unwrap()).unwrap();
        qwen["modelProviders"]["<provider-id>"][0]
            .as_object_mut()
            .unwrap()
            .insert("future".into(), Value::Bool(true));
        assert!(validate_qwen_template_shape(&qwen, "fixture").is_err());

        let mut qwen_protocol: Value =
            serde_json::from_str(resource_text("qwen/settings.json").unwrap()).unwrap();
        qwen_protocol["providerProtocol"]["<provider-id>"] = Value::String("future".into());
        assert!(validate_qwen_template_shape(&qwen_protocol, "fixture").is_err());
    }

    #[test]
    fn manifest_metadata_is_bound_to_the_allowlisted_resource_path() {
        let mut entry = manifest_entry("codex/models.json");
        entry.role = "codex-config".into();
        assert!(validate_manifest_metadata(&entry).is_err());
    }

    #[test]
    fn json_template_keys_values_and_arrays_are_rendered_once() {
        let catalog = Path::new("/tmp/models.json");
        let provider_id = "provider/with.dot 雪";
        let model = "model/<provider-id>/\"quoted\"";
        let binding = TemplateBindings {
            provider_id,
            provider_name: "Provider 雪",
            endpoint: "https://example.test/a b",
            auth_type: ConnectionAuthType::Bearer,
            api_key: "fixture-<model-id>-key",
            model,
            model_catalog_path: Some(catalog),
            qwen: None,
        };
        let mut value = serde_json::json!({
            "<provider-id>": ["<model-id>", "<model-description>", "<your-api-key>"]
        });
        render_json_value(&mut value, CliProtocol::OpenaiChat, &binding, "fixture").unwrap();
        assert_eq!(value[provider_id][0], model);
        assert_eq!(value[provider_id][1], model);
        assert_eq!(value[provider_id][2], "fixture-<model-id>-key");
    }

    #[test]
    fn deepseek_suffix_is_not_duplicated() {
        let catalog = Path::new("/tmp/models.json");
        let templates = resolve_templates(&TemplateSelection {
            cli_id: CliId::ClaudeCode,
            template_id: Some("deepseek"),
            protocol: CliProtocol::AnthropicMessages,
            model: "deepseek-chat[1m]",
        })
        .unwrap();
        let RenderedManagedConfig::Claude(rendered) =
            render_managed_config(&templates, &bindings("deepseek-chat[1m]", catalog)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(rendered.env["ANTHROPIC_MODEL"], "deepseek-chat[1m]");
        assert_eq!(
            rendered.env["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "deepseek-chat[1m]"
        );
    }

    #[test]
    fn user_data_is_substituted_once() {
        let catalog = Path::new("/tmp/models.json");
        let templates = resolve_templates(&TemplateSelection {
            cli_id: CliId::Opencode,
            template_id: None,
            protocol: CliProtocol::OpenaiChat,
            model: "model/<provider-id> \"雪\"",
        })
        .unwrap();
        let RenderedManagedConfig::OpenCode(rendered) =
            render_managed_config(&templates, &bindings("model/<provider-id> \"雪\"", catalog))
                .unwrap()
        else {
            unreachable!()
        };
        let OpenCodeConfigKind::Generic { model_name, .. } = rendered.kind else {
            unreachable!();
        };
        assert_eq!(model_name, "model/<provider-id> \"雪\"");
    }

    #[test]
    fn every_native_template_renders_its_native_identity() {
        let catalog = Path::new("/tmp/models.json");
        for template_id in PROVIDER_TEMPLATE_IDS {
            let templates = resolve_templates(&TemplateSelection {
                cli_id: CliId::Opencode,
                template_id: Some(template_id),
                protocol: CliProtocol::OpenaiChat,
                model: "fixture-model",
            })
            .unwrap();
            assert_ne!(templates.config_path, OPENCODE_GENERIC_CONFIG_PATH);
            assert_eq!(templates.auth_path, Some(opencode_auth_path(template_id)));
            let RenderedManagedConfig::OpenCode(rendered) =
                render_managed_config(&templates, &bindings("fixture-model", catalog)).unwrap()
            else {
                unreachable!();
            };
            assert!(matches!(rendered.kind, OpenCodeConfigKind::Native));
            assert_eq!(rendered.provider_id, template_id);
            assert_eq!(
                rendered.model_reference,
                format!("{template_id}/fixture-model")
            );
            assert_eq!(rendered.api_key, "fixture-<model-id>-key");
        }
    }

    #[test]
    fn native_contracts_pin_the_fixed_connection_and_unknown_ids_have_none() {
        for template_id in PROVIDER_TEMPLATE_IDS {
            let contract = opencode_native_contract(Some(template_id))
                .unwrap()
                .unwrap();
            assert_eq!(contract.template_id, template_id);
            assert_eq!(contract.native_provider_id, template_id);
            assert_eq!(contract.protocol, CliProtocol::OpenaiChat);
            assert_eq!(contract.auth_type, ConnectionAuthType::Bearer);
        }
        assert!(opencode_native_contract(None).unwrap().is_none());
        assert!(
            opencode_native_contract(Some("glm-coding-plan"))
                .unwrap()
                .is_none()
        );
        assert!(
            opencode_native_contract(Some("future-provider"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn write_mode_follows_the_saved_connection_exactly() {
        let contract = opencode_native_contract(Some("zhipuai-coding-plan"))
            .unwrap()
            .unwrap();
        assert!(matches!(
            opencode_write_mode(
                Some("zhipuai-coding-plan"),
                contract.protocol,
                &contract.endpoint,
                contract.auth_type,
            )
            .unwrap(),
            OpenCodeWriteMode::Native
        ));
        // Root-path spellings that Url itself normalizes stay native.
        let deepseek = opencode_native_contract(Some("deepseek")).unwrap().unwrap();
        let root_without_slash = Url::parse("https://api.deepseek.com").unwrap();
        assert_eq!(root_without_slash, deepseek.endpoint);
        assert!(matches!(
            opencode_write_mode(
                Some("deepseek"),
                deepseek.protocol,
                &root_without_slash,
                deepseek.auth_type,
            )
            .unwrap(),
            OpenCodeWriteMode::Native
        ));
        // A non-root path differing only by its trailing slash is a different destination.
        let trailing_slash = Url::parse(&format!("{}/", contract.endpoint)).unwrap();
        assert_ne!(trailing_slash, contract.endpoint);
        assert!(matches!(
            opencode_write_mode(
                Some("zhipuai-coding-plan"),
                contract.protocol,
                &trailing_slash,
                contract.auth_type
            )
            .unwrap(),
            OpenCodeWriteMode::Generic
        ));
        assert!(matches!(
            opencode_write_mode(
                Some("zhipuai-coding-plan"),
                CliProtocol::OpenaiResponses,
                &contract.endpoint,
                contract.auth_type
            )
            .unwrap(),
            OpenCodeWriteMode::Generic
        ));
        assert!(matches!(
            opencode_write_mode(
                Some("zhipuai-coding-plan"),
                contract.protocol,
                &contract.endpoint,
                ConnectionAuthType::ApiKey
            )
            .unwrap(),
            OpenCodeWriteMode::Generic
        ));
        assert!(matches!(
            opencode_write_mode(
                None,
                contract.protocol,
                &contract.endpoint,
                contract.auth_type
            )
            .unwrap(),
            OpenCodeWriteMode::Generic
        ));
        assert!(matches!(
            opencode_write_mode(
                Some("glm-coding-plan"),
                contract.protocol,
                &contract.endpoint,
                contract.auth_type
            )
            .unwrap(),
            OpenCodeWriteMode::Generic
        ));
    }

    #[test]
    fn exact_deepseek_models_keep_their_reviewed_metadata() {
        let catalog = std::env::temp_dir().join("cliswitch-test-models.json");
        for (model, display_name, priority, modalities) in [
            (
                "deepseek-v4-flash",
                "DeepSeek-V4-Flash",
                1,
                serde_json::json!(["text"]),
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek-V4-Pro",
                2,
                serde_json::json!(["text"]),
            ),
            (
                "deepseek-v4-flash-vision-exp",
                "DeepSeek-V4-Flash-Vision",
                3,
                serde_json::json!(["text", "image"]),
            ),
        ] {
            let templates = resolve_templates(&TemplateSelection {
                cli_id: CliId::Codex,
                template_id: Some("deepseek"),
                protocol: CliProtocol::OpenaiResponses,
                model,
            })
            .unwrap();
            let RenderedManagedConfig::Codex(rendered) =
                render_managed_config(&templates, &bindings(model, &catalog)).unwrap()
            else {
                unreachable!()
            };
            assert_eq!(rendered.model_entry["slug"], model);
            assert_eq!(rendered.model_entry["display_name"], display_name);
            assert_eq!(rendered.model_entry["priority"], priority);
            assert_eq!(rendered.model_entry["input_modalities"], modalities);
        }
    }

    #[test]
    fn unknown_provider_uses_generic_template() {
        let catalog = std::env::temp_dir().join("cliswitch-test-models.json");
        let templates = resolve_templates(&TemplateSelection {
            cli_id: CliId::Codex,
            template_id: Some("future-provider"),
            protocol: CliProtocol::OpenaiResponses,
            model: "manual-model",
        })
        .unwrap();
        let RenderedManagedConfig::Codex(rendered) =
            render_managed_config(&templates, &bindings("manual-model", &catalog)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(rendered.reasoning_effort, "high");
        assert_eq!(rendered.model_entry["slug"], "manual-model");
    }
}
