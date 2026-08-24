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
