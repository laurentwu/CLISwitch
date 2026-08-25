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
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: connection_id,
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
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
async fn opencode_materializes_the_explicitly_selected_glm_endpoint() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{}\n").await;
    write_fixture(paths.auth_file.as_ref().unwrap(), "{}\n").await;
    let now = Utc::now();
    let connections = [
        (
            "anthropic",
            CliProtocol::AnthropicMessages,
            "https://open.bigmodel.cn/api/anthropic",
        ),
        (
            "openai-chat",
            CliProtocol::OpenaiChat,
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ),
        (
            "openai-responses",
            CliProtocol::OpenaiResponses,
            "https://open.bigmodel.cn/api/v1",
        ),
    ]
    .into_iter()
    .map(|(endpoint_id, protocol, endpoint)| ProviderConnection {
        id: Uuid::new_v4(),
        template_endpoint_id: Some(endpoint_id.into()),
        credential_slot_id: "api-key".into(),
        protocol,
        endpoint: Url::parse(endpoint).unwrap(),
        auth_type: ConnectionAuthType::Bearer,
        api_key: "shared-glm-key".into(),
        default_model: "glm-4.7".into(),
        verification: VerificationInfo::default(),
    })
    .collect::<Vec<_>>();
    let responses_id = connections
        .iter()
        .find(|connection| connection.template_endpoint_id.as_deref() == Some("openai-responses"))
        .unwrap()
        .id;
    let provider = ProviderProfile {
        id: Uuid::new_v4(),
        name: "GLM Coding Plan".into(),
        template_id: Some("glm-coding-plan".into()),
        revision: 1,
        created_at: now,
        updated_at: now,
        data: ProviderData::Api(ApiProviderData { connections }),
    };
    provider.validate().unwrap();
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: provider.id,
        connection_id: responses_id,
        model: "manual-glm-model".into(),
    };

    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let config = String::from_utf8(plan.files[0].target_content.clone()).unwrap();

    assert!(config.contains("@ai-sdk/openai"));
    assert!(config.contains("https://open.bigmodel.cn/api/v1"));
    assert!(!config.contains("https://open.bigmodel.cn/api/coding/paas/v4"));
    assert!(config.contains("manual-glm-model"));
    assert!(config.contains("glm-4.7"));
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
    assert_eq!(current.current.protocol, Some(CliProtocol::OpenaiChat));
    assert_eq!(current.unmanaged_api_candidates.len(), 1);
    let candidate = &current.unmanaged_api_candidates[0];
    assert_eq!(candidate.source_provider_id, "zhipuai-coding-plan");
    assert_eq!(candidate.suggested_name, "GLM Coding Plan");
    assert_eq!(candidate.template_id.as_deref(), Some("glm-coding-plan"));
    assert_eq!(
        candidate.connection.template_endpoint_id.as_deref(),
        Some("openai-chat")
    );
    assert_eq!(candidate.connection.credential_slot_id, "api-key");
    assert_eq!(candidate.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(candidate.connection.auth_type, ConnectionAuthType::Bearer);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    assert_eq!(candidate.available_models[0], "glm-5.3");
    assert!(candidate.available_models.contains(&"glm-4.7".into()));
    assert!(candidate.is_current);
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

#[tokio::test]
async fn opencode_recognizes_every_savable_api_provider_in_auth_json() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "first-provider/current-model",
          "provider": {
            "first-provider": {
              "npm": "@ai-sdk/openai-compatible",
              "name": "First provider",
              "options": { "baseURL": "https://first.invalid/v1" },
              "models": {
                "current-model": {},
                "other-model": {}
              }
            },
            "second-provider": {
              "npm": "@ai-sdk/anthropic",
              "name": "Second provider",
              "options": { "baseURL": "https://second.invalid" },
              "models": { "second-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{
          "first-provider": { "type": "api", "key": "fixture-first-key" },
          "second-provider": { "type": "api", "key": "fixture-second-key" },
          "oauth-provider": {
            "type": "oauth",
            "access": "fixture-oauth-access",
            "refresh": "fixture-oauth-refresh"
          },
          "incomplete-provider": { "type": "api", "key": "fixture-incomplete-key" }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(current.unmanaged_api_candidates.len(), 2);
    let first = current
        .unmanaged_api_candidates
        .iter()
        .find(|candidate| candidate.source_provider_id == "first-provider")
        .unwrap();
    assert_eq!(first.suggested_name, "First provider");
    assert_eq!(first.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(first.available_models, vec!["current-model", "other-model"]);
    assert!(first.is_current);
    let second = current
        .unmanaged_api_candidates
        .iter()
        .find(|candidate| candidate.source_provider_id == "second-provider")
        .unwrap();
    assert_eq!(second.connection.protocol, CliProtocol::AnthropicMessages);
    assert_eq!(second.connection.auth_type, ConnectionAuthType::ApiKey);
    assert_eq!(second.available_models, vec!["second-model"]);
    assert!(!second.is_current);
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("oauth-provider") && message.contains("OAuth"))
    );
    assert!(
        current.current.diagnostics.iter().any(
            |message| message.contains("incomplete-provider") && message.contains("recognized")
        )
    );
    assert!(current.current.diagnostics.iter().all(|message| {
        !message.contains("fixture-first-key")
            && !message.contains("fixture-second-key")
            && !message.contains("fixture-oauth-access")
    }));
}

#[tokio::test]
async fn opencode_explicit_provider_fields_override_the_relation_defaults() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "zhipuai-coding-plan/fixture-model",
          "provider": {
            "zhipuai-coding-plan": {
              "name": "Private Zhipu gateway",
              "npm": "@ai-sdk/anthropic",
              "options": { "baseURL": "https://private.invalid/anthropic" },
              "models": { "fixture-model": {} }
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
            "key": "fixture-private-key"
          }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    let candidate = &current.unmanaged_api_candidates[0];

    assert_eq!(candidate.suggested_name, "Private Zhipu gateway");
    assert_eq!(candidate.template_id, None);
    assert_eq!(candidate.connection.template_endpoint_id, None);
    assert_eq!(
        candidate.connection.protocol,
        CliProtocol::AnthropicMessages
    );
    assert_eq!(candidate.connection.auth_type, ConnectionAuthType::ApiKey);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://private.invalid/anthropic"
    );
}

#[tokio::test]
async fn opencode_recognizes_a_relation_specific_native_provider_package() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "openrouter/fixture-model",
          "provider": {
            "openrouter": {
              "npm": "@openrouter/ai-sdk-provider",
              "models": { "fixture-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{ "openrouter": { "type": "api", "key": "fixture-openrouter-key" } }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    let candidate = &current.unmanaged_api_candidates[0];

    assert_eq!(candidate.template_id.as_deref(), Some("openrouter-api"));
    assert_eq!(candidate.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://openrouter.ai/api/v1"
    );
}

#[tokio::test]
async fn opencode_non_object_auth_root_keeps_the_configuration_readable() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "fixture-provider/fixture-model",
          "provider": {
            "fixture-provider": {
              "npm": "@ai-sdk/openai-compatible",
              "options": { "baseURL": "https://fixture.invalid/v1" },
              "models": { "fixture-model": {} }
            }
          }
        }"#,
    )
    .await;
    write_fixture(paths.auth_file.as_ref().unwrap(), "[]").await;

    let current = adapter.read_current(&paths, &host).await.unwrap();

    assert_eq!(
        current.current.provider_name.as_deref(),
        Some("fixture-provider")
    );
    assert_eq!(current.current.model.as_deref(), Some("fixture-model"));
    assert_eq!(current.current.protocol, Some(CliProtocol::OpenaiChat));
    assert_eq!(current.current.auth_kind, None);
    assert!(current.unmanaged_api_candidates.is_empty());
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| message.contains("auth root is not an object"))
    );
}

#[test]
fn opencode_auth_path_uses_the_documented_home_location_and_xdg_override() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let default_host = environment(temp.path());
    let default_paths = adapter.resolve_paths(&default_host, None);
    assert_eq!(
        default_paths.auth_file.as_deref(),
        Some(
            temp.path()
                .join(".local")
                .join("share")
                .join("opencode")
                .join("auth.json")
                .as_path()
        )
    );

    let mut xdg_host = environment(temp.path());
    let xdg_data = temp.path().join("xdg-data");
    xdg_host.variables.insert(
        "XDG_DATA_HOME".into(),
        xdg_data.to_string_lossy().into_owned(),
    );
    let xdg_paths = adapter.resolve_paths(&xdg_host, None);
    assert_eq!(
        xdg_paths.auth_file.as_deref(),
        Some(xdg_data.join("opencode").join("auth.json").as_path())
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
