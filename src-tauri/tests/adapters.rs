use std::{collections::BTreeMap, path::Path};

use chrono::Utc;
use cliswitch_lib::{
    adapters::{ClaudeCodeAdapter, CliAdapter, CodexAdapter, HostEnvironment, OpenCodeAdapter},
    domain::{
        ApiProviderData, CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType,
        ProviderConnection, ProviderData, ProviderProfile, VerificationInfo,
    },
};
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

fn environment(home: &Path) -> HostEnvironment {
    HostEnvironment {
        home: home.to_path_buf(),
        variables: BTreeMap::new(),
        present_variables: Default::default(),
        os: std::env::consts::OS.into(),
    }
}

fn provider(protocol: CliProtocol) -> (ProviderProfile, Uuid) {
    let now = Utc::now();
    let connection_id = Uuid::new_v4();
    (
        ProviderProfile {
            id: Uuid::new_v4(),
            name: "Fixture provider".into(),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                coding_plan: true,
                coding_plan_name: Some("Fixture Plan".into()),
                connections: vec![ProviderConnection {
                    id: connection_id,
                    protocol,
                    endpoint: Url::parse("https://gateway.invalid/v1").unwrap(),
                    auth_type: if protocol == CliProtocol::AnthropicMessages {
                        ConnectionAuthType::ApiKey
                    } else {
                        ConnectionAuthType::Bearer
                    },
                    api_key: "fixture-new-key-not-real".into(),
                    default_model: "fixture-model".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        },
        connection_id,
    )
}

async fn write_fixture(path: &Path, content: &str) {
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, content).await.unwrap();
}

#[tokio::test]
async fn claude_patch_preserves_comments_order_and_unmanaged_fields() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    let source = include_str!("fixtures/claude/settings.json");
    write_fixture(&paths.config_file, source).await;
    let (provider, connection_id) = provider(CliProtocol::AnthropicMessages);
    let target = ConfigurationTarget::Api {
        cli_id: CliId::ClaudeCode,
        provider_id: provider.id,
        connection_id,
        model: "fixture-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let output = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
    assert!(output.contains("// Scrubbed stable Claude Code settings fixture."));
    assert!(output.contains("\"UNRELATED_VALUE\": \"keep-me\""));
    assert!(output.contains("\"unknown\": { \"ordered\": true }"));
    assert!(output.contains("fixture-new-key-not-real"));
    assert!(!output.contains("fixture-old-key"));
    assert!(output.find("permissions").unwrap() < output.find("unknown").unwrap());
}

#[tokio::test]
async fn claude_explicit_api_settings_take_priority_over_a_stale_oauth_file() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        include_str!("fixtures/claude/settings.json"),
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        include_str!("fixtures/claude/credentials.json"),
    )
    .await;
    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert_eq!(current.current.auth_kind.as_deref(), Some("api"));
    assert_eq!(
        current.current.protocol,
        Some(CliProtocol::AnthropicMessages)
    );
}

#[tokio::test]
async fn codex_writes_responses_file_mapping_and_preserves_unmanaged_toml() {
    let temp = TempDir::new().unwrap();
    let adapter = CodexAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(
        &paths.config_file,
        include_str!("fixtures/codex/config.toml"),
    )
    .await;
    let (provider, connection_id) = provider(CliProtocol::OpenaiResponses);
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Codex,
        provider_id: provider.id,
        connection_id,
        model: "fixture-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let output = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
    assert!(output.contains("# Scrubbed stable Codex CLI fixture."));
    assert!(output.contains("[profiles.keep_me]"));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(output.contains("experimental_bearer_token = \"fixture-new-key-not-real\""));
    assert!(!output.contains("env_key"));
}

#[tokio::test]
async fn opencode_stable_schema_maps_each_protocol_to_the_correct_package() {
    for (protocol, package) in [
        (CliProtocol::OpenaiChat, "@ai-sdk/openai-compatible"),
        (CliProtocol::OpenaiResponses, "@ai-sdk/openai"),
        (CliProtocol::AnthropicMessages, "@ai-sdk/anthropic"),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = OpenCodeAdapter;
        let paths = adapter.resolve_paths(&environment(temp.path()), None);
        write_fixture(
            &paths.config_file,
            include_str!("fixtures/opencode/opencode.jsonc"),
        )
        .await;
        write_fixture(
            paths.auth_file.as_ref().unwrap(),
            include_str!("fixtures/opencode/auth.json"),
        )
        .await;
        let (provider, connection_id) = provider(protocol);
        let target = ConfigurationTarget::Api {
            cli_id: CliId::Opencode,
            provider_id: provider.id,
            connection_id,
            model: "fixture-model".into(),
        };
        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        let config = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
        let auth = String::from_utf8(plan.files[1].target_content.clone()).unwrap();
        assert!(config.contains("// Scrubbed OpenCode stable-v1 fixture"));
        assert!(config.contains("\"unknown\": [1, 2, 3]"));
        assert!(config.contains(package));
        assert!(config.contains("\"provider\""));
        assert!(!config.contains("\"providers\""));
        assert!(auth.contains("\"type\": \"api\""));
        assert!(auth.contains("fixture-new-key-not-real"));
    }
}

#[tokio::test]
async fn opencode_reads_the_last_used_model_when_no_default_is_configured() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let mut host = environment(temp.path());
    let state_home = temp.path().join("custom-state");
    host.variables.insert(
        "XDG_STATE_HOME".into(),
        state_home.to_string_lossy().into_owned(),
    );
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "provider": {
            "zhipuai-coding-plan": {
              "models": {
                "glm-5.3": { "name": "GLM-5.3" }
              }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{
          "zhipuai-coding-plan": {
            "type": "api",
            "key": "fixture-existing-key"
          }
        }"#,
    )
    .await;
    let state_file = state_home.join("opencode").join("model.json");
    write_fixture(
        &state_file,
        r#"{
          "recent": [
            { "providerID": "zhipuai-coding-plan", "modelID": "glm-5.3" },
            { "providerID": "zhipuai-coding-plan", "modelID": "glm-5.2" }
          ]
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("zhipuai-coding-plan")
    );
    assert_eq!(current.current.model.as_deref(), Some("glm-5.3"));
    assert_eq!(current.current.auth_kind.as_deref(), Some("api"));
    assert_eq!(current.current.protocol, None);
    assert!(current.unmanaged_candidate.is_none());
    assert!(current.current.diagnostics.is_empty());
    let state_source = current
        .current
        .sources
        .iter()
        .find(|source| source.source_id == "opencode-model-state")
        .unwrap();
    assert_eq!(state_source.display_path, state_file);
    assert!(state_source.digest.is_some());
}

#[tokio::test]
async fn opencode_explicit_default_model_takes_priority_over_last_used_state() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        include_str!("fixtures/opencode/opencode.jsonc"),
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        include_str!("fixtures/opencode/auth.json"),
    )
    .await;
    write_fixture(
        &temp
            .path()
            .join(".local")
            .join("state")
            .join("opencode")
            .join("model.json"),
        r#"{
          "recent": [
            { "providerID": "other-provider", "modelID": "other-model" }
          ]
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("user_provider")
    );
    assert_eq!(current.current.model.as_deref(), Some("existing-model"));
    assert!(
        current
            .current
            .sources
            .iter()
            .all(|source| source.source_id != "opencode-model-state")
    );
}

#[tokio::test]
async fn opencode_invalid_explicit_model_does_not_fall_back_to_last_used_state() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "missing-provider-separator",
          "provider": {
            "only-provider": {
              "models": { "only-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        &temp
            .path()
            .join(".local")
            .join("state")
            .join("opencode")
            .join("model.json"),
        r#"{
          "recent": [
            { "providerID": "state-provider", "modelID": "state-model" }
          ]
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(current.current.provider_name, None);
    assert_eq!(current.current.model, None);
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("provider/model format"))
    );
    assert!(
        current
            .current
            .sources
            .iter()
            .all(|source| source.source_id != "opencode-model-state")
    );
}

#[tokio::test]
async fn opencode_falls_back_to_one_unambiguous_configured_model() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "provider": {
            "only-provider": {
              "models": {
                "only-model": { "name": "Only model" }
              }
            }
          }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("only-provider")
    );
    assert_eq!(current.current.model.as_deref(), Some("only-model"));
    assert!(current.current.diagnostics.is_empty());
    let state_source = current
        .current
        .sources
        .iter()
        .find(|source| source.source_id == "opencode-model-state")
        .unwrap();
    assert!(state_source.digest.is_none());
}

#[tokio::test]
async fn opencode_invalid_recent_entry_falls_back_to_the_unique_configured_model() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "provider": {
            "only-provider": {
              "models": { "only-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        &temp
            .path()
            .join(".local")
            .join("state")
            .join("opencode")
            .join("model.json"),
        r#"{
          "recent": [
            { "providerID": "incomplete-provider" }
          ]
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("only-provider")
    );
    assert_eq!(current.current.model.as_deref(), Some("only-model"));
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("no valid modelID"))
    );
    assert!(
        current
            .current
            .sources
            .iter()
            .find(|source| source.source_id == "opencode-model-state")
            .is_some_and(|source| source.digest.is_some())
    );
}

#[tokio::test]
async fn opencode_ignores_invalid_model_state_and_uses_the_unique_configured_model() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "provider": {
            "only-provider": {
              "models": { "only-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        &temp
            .path()
            .join(".local")
            .join("state")
            .join("opencode")
            .join("model.json"),
        "{ invalid json",
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("only-provider")
    );
    assert_eq!(current.current.model.as_deref(), Some("only-model"));
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("parse OpenCode model state"))
    );
}

#[tokio::test]
async fn opencode_does_not_guess_between_ambiguous_configured_models() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "provider": {
            "one-provider": {
              "models": {
                "first-model": {},
                "second-model": {}
              }
            }
          }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(current.current.provider_name, None);
    assert_eq!(current.current.model, None);
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("multiple configured models"))
    );
}

#[test]
fn oauth_fixtures_are_recognized_locally_with_stable_account_identity() {
    assert_eq!(
        ClaudeCodeAdapter
            .validate_imported_auth(include_bytes!("fixtures/claude/credentials.json"))
            .unwrap()
            .as_deref(),
        Some("fixture-claude-account")
    );
    assert_eq!(
        CodexAdapter
            .validate_imported_auth(include_bytes!("fixtures/codex/auth.json"))
            .unwrap()
            .as_deref(),
        Some("fixture-codex-account")
    );
}

#[test]
fn claude_oauth_import_requires_the_real_access_token_field() {
    let adapter = ClaudeCodeAdapter;
    assert!(
        adapter
            .validate_imported_auth(br#"{"note":"token pending for oauth"}"#)
            .is_err()
    );
    assert!(
        adapter
            .validate_imported_auth(br#"{"claudeAiOauth":{"accessToken":""}}"#)
            .is_err()
    );
}

#[tokio::test]
async fn opencode_v2_beta_schema_is_explicitly_rejected() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{ \"providers\": {} }").await;
    let error = adapter
        .read_current(&paths, &environment(temp.path()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("v2 beta"));
}
