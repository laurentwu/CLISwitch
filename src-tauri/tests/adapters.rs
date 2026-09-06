use std::{collections::BTreeMap, path::Path};

use chrono::Utc;
use cliswitch_lib::{
    adapters::{
        AdapterWritePlan, ClaudeCodeAdapter, CliAdapter, CodexAdapter, HostEnvironment,
        OpenCodeAdapter, namespaced_provider_id,
    },
    catalog::{legacy_catalog, runtime_catalog},
    domain::{
        ApiProviderData, CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType, OAuthKind,
        OAuthProviderData, ProviderConnection, ProviderData, ProviderProfile, VerificationInfo,
    },
    filesystem::digest::bytes_digest,
    services::config_writer::{parse_jsonc_value, parse_toml},
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

fn templated_provider(
    template_id: &str,
    api_key: &str,
    auth_type: ConnectionAuthType,
) -> (ProviderProfile, Uuid) {
    let now = Utc::now();
    let template = legacy_catalog().unwrap().api_template(template_id).unwrap();
    let endpoint = template
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "anthropic")
        .unwrap();
    let connection_id = Uuid::new_v4();
    (
        ProviderProfile {
            id: Uuid::new_v4(),
            name: template.name.clone(),
            template_id: Some(template.id.clone()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: connection_id,
                    template_endpoint_id: Some(endpoint.id.clone()),
                    credential_slot_id: endpoint.credential_slot_id.clone(),
                    protocol: endpoint.protocol,
                    endpoint: endpoint.base_url.clone(),
                    auth_type,
                    api_key: api_key.into(),
                    default_model: "MiniMax-M2.7".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        },
        connection_id,
    )
}

fn cli_adapter_provider(
    template_id: &str,
    protocol: CliProtocol,
    auth_type: ConnectionAuthType,
) -> (ProviderProfile, Uuid) {
    let catalog = runtime_catalog().unwrap();
    let template = catalog.api_template(template_id).unwrap();
    let endpoint = template
        .endpoints
        .iter()
        .find(|endpoint| endpoint.protocol == protocol)
        .unwrap();
    let connection_id = Uuid::new_v4();
    (
        ProviderProfile {
            id: Uuid::new_v4(),
            name: format!("{template_id} saved instance"),
            template_id: Some(template_id.into()),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: connection_id,
                    template_endpoint_id: Some(endpoint.id.clone()),
                    credential_slot_id: endpoint.credential_slot_id.clone(),
                    protocol,
                    endpoint: Url::parse("https://saved-endpoint.invalid/custom/v1").unwrap(),
                    auth_type,
                    api_key: "fixture-template-key-not-real".into(),
                    default_model: "selected-model".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        },
        connection_id,
    )
}

fn oauth_provider(kind: OAuthKind, raw_content: &str) -> ProviderProfile {
    ProviderProfile {
        id: Uuid::new_v4(),
        name: "Fixture OAuth".into(),
        template_id: None,
        revision: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        data: ProviderData::Oauth(OAuthProviderData {
            oauth_kind: kind,
            account_id: Some("fixture-account".into()),
            account_label: None,
            raw_content: raw_content.into(),
            digest: bytes_digest(raw_content.as_bytes()),
            manually_modified: false,
            verification: VerificationInfo::default(),
        }),
    }
}

async fn write_fixture(path: &Path, content: &str) {
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, content).await.unwrap();
}

async fn materialize_plan(plan: &AdapterWritePlan) {
    for file in &plan.files {
        tokio::fs::create_dir_all(file.path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&file.path, &file.target_content)
            .await
            .unwrap();
    }
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
async fn claude_cli_adapter_apply_preserves_selected_bearer_auth() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(
        &paths.config_file,
        r#"{
          "env": {
            "ANTHROPIC_API_KEY": "stale-api-key",
            "ANTHROPIC_AUTH_TOKEN": "stale-auth-token"
          }
        }"#,
    )
    .await;
    let catalog = runtime_catalog().unwrap();
    let template = catalog.api_template("deepseek").unwrap();
    let connections = template
        .endpoints
        .iter()
        .map(|endpoint| ProviderConnection {
            id: Uuid::new_v4(),
            template_endpoint_id: Some(endpoint.id.clone()),
            credential_slot_id: endpoint.credential_slot_id.clone(),
            protocol: endpoint.protocol,
            endpoint: endpoint.base_url.clone(),
            auth_type: if endpoint.protocol == CliProtocol::AnthropicMessages {
                ConnectionAuthType::Bearer
            } else {
                endpoint.default_auth_type().unwrap()
            },
            api_key: "fixture-bearer-token".into(),
            default_model: "manual-model".into(),
            verification: VerificationInfo::default(),
        })
        .collect::<Vec<_>>();
    let connection_id = connections
        .iter()
        .find(|connection| connection.protocol == CliProtocol::AnthropicMessages)
        .unwrap()
        .id;
    let now = Utc::now();
    let provider = ProviderProfile {
        id: Uuid::new_v4(),
        name: "DeepSeek bearer".into(),
        template_id: Some("deepseek".into()),
        revision: 1,
        created_at: now,
        updated_at: now,
        data: ProviderData::Api(ApiProviderData { connections }),
    };
    provider.validate().unwrap();
    let target = ConfigurationTarget::Api {
        cli_id: CliId::ClaudeCode,
        provider_id: provider.id,
        connection_id,
        model: "manual-model".into(),
    };

    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let output = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
    assert!(output.contains(r#""ANTHROPIC_AUTH_TOKEN": "fixture-bearer-token""#));
    assert!(!output.contains("ANTHROPIC_API_KEY"));
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
async fn claude_recognizes_minimax_region_and_credential_kind_from_endpoint_and_key() {
    for (base_url, key_field, key, template_id, expected_auth_type) in [
        (
            "https://api.minimax.io/anthropic",
            "ANTHROPIC_API_KEY",
            "sk-cp-global-fixture",
            "minimax-coding-plan",
            ConnectionAuthType::ApiKey,
        ),
        (
            "https://api.minimaxi.com/anthropic/v1/",
            "ANTHROPIC_AUTH_TOKEN",
            "sk-api-china-fixture",
            "minimax-cn-api",
            ConnectionAuthType::Bearer,
        ),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = ClaudeCodeAdapter;
        let host = environment(temp.path());
        let paths = adapter.resolve_paths(&host, None);
        write_fixture(
            &paths.config_file,
            &format!(
                r#"{{
                  "env": {{
                    "ANTHROPIC_BASE_URL": "{base_url}",
                    "{key_field}": "{key}"
                  }},
                  "model": "MiniMax-M2.7"
                }}"#,
            ),
        )
        .await;

        let current = adapter.read_current(&paths, &host).await.unwrap();
        assert_eq!(current.unmanaged_api_candidates.len(), 1);
        let candidate = &current.unmanaged_api_candidates[0];
        assert_eq!(candidate.template_id.as_deref(), Some(template_id));
        assert_eq!(
            candidate.connection.template_endpoint_id.as_deref(),
            Some("anthropic")
        );
        assert_eq!(candidate.connection.auth_type, expected_auth_type);
        assert_eq!(candidate.connection.endpoint.as_str(), base_url);
        assert_eq!(
            current.current.provider_name.as_deref(),
            legacy_catalog()
                .unwrap()
                .api_template(template_id)
                .map(|template| template.name.as_str())
        );
    }
}

#[tokio::test]
async fn claude_disambiguates_cli_adapter_provider_pairs_by_key_kind() {
    for (key, expected_template_id) in [
        ("sk-api-zai-fixture", "zai"),
        ("sk-cp-zai-fixture", "zai-coding-plan"),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = ClaudeCodeAdapter;
        let host = environment(temp.path());
        let paths = adapter.resolve_paths(&host, None);
        write_fixture(
            &paths.config_file,
            &format!(
                r#"{{
                  "env": {{
                    "ANTHROPIC_BASE_URL": "https://api.z.ai/api/anthropic",
                    "ANTHROPIC_API_KEY": "{key}"
                  }},
                  "model": "manual-model"
                }}"#
            ),
        )
        .await;

        let current = adapter.read_current(&paths, &host).await.unwrap();
        let candidate = &current.unmanaged_api_candidates[0];
        assert_eq!(candidate.template_id.as_deref(), Some(expected_template_id));
        assert_eq!(
            candidate.connection.template_endpoint_id.as_deref(),
            Some("anthropic-messages")
        );
    }
}

#[tokio::test]
async fn claude_credentials_without_a_model_are_importable() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "env": {
            "ANTHROPIC_BASE_URL": "https://gateway.invalid/anthropic",
            "ANTHROPIC_API_KEY": "fixture-key"
          }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert_eq!(current.current.model, None);
    assert_eq!(current.unmanaged_api_candidates.len(), 1);
    let candidate = &current.unmanaged_api_candidates[0];
    assert_eq!(candidate.default_model, None);
    assert!(candidate.available_models.is_empty());
    assert!(candidate.connection.default_model.is_empty());
}

#[tokio::test]
async fn claude_rejects_ambiguous_api_key_and_auth_token_settings() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "env": {
            "ANTHROPIC_BASE_URL": "https://api.minimax.io/anthropic",
            "ANTHROPIC_API_KEY": "sk-api-fixture",
            "ANTHROPIC_AUTH_TOKEN": "sk-cp-fixture"
          },
          "model": "MiniMax-M2.7"
        }"#,
    )
    .await;

    let error = adapter.read_current(&paths, &host).await.unwrap_err();
    assert!(matches!(
        error,
        cliswitch_lib::error::AppError::Unsupported(_)
    ));
    assert!(
        error
            .to_string()
            .contains("both ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN")
    );
}

#[tokio::test]
async fn claude_rejects_ambiguous_process_credential_overrides() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let mut host = environment(temp.path());
    host.present_variables.insert("ANTHROPIC_API_KEY".into());
    host.present_variables.insert("ANTHROPIC_AUTH_TOKEN".into());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(&paths.config_file, "{}\n").await;

    let error = adapter.read_current(&paths, &host).await.unwrap_err();
    assert!(matches!(
        error,
        cliswitch_lib::error::AppError::Unsupported(_)
    ));
    assert!(
        error
            .to_string()
            .contains("process environment contains both")
    );
}

#[tokio::test]
async fn claude_reports_every_new_managed_environment_field_by_presence_only() {
    for field in [
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "CLAUDE_CODE_EFFORT_LEVEL",
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        "API_TIMEOUT_MS",
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = ClaudeCodeAdapter;
        let mut host = environment(temp.path());
        host.present_variables.insert(field.into());
        let paths = adapter.resolve_paths(&host, None);
        write_fixture(&paths.config_file, "{}\n").await;

        let current = adapter.read_current(&paths, &host).await.unwrap();

        assert!(current.current.externally_overridden, "{field}");
        assert!(!host.variables.contains_key(field), "{field}");
        assert!(
            current
                .current
                .diagnostics
                .iter()
                .all(|message| !message.contains("fixture-secret"))
        );
    }
}

#[tokio::test]
async fn claude_preserves_saved_minimax_endpoint_and_auth_type() {
    for (template_id, key, stored_auth_type, expected_base_url, expected_field, removed_field) in [
        (
            "minimax-coding-plan",
            "sk-cp-fixture",
            ConnectionAuthType::ApiKey,
            "https://api.minimax.io/anthropic",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
        ),
        (
            "minimax-api",
            "sk-api-fixture",
            ConnectionAuthType::Bearer,
            "https://api.minimax.io/anthropic",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ),
        (
            "minimax-cn-coding-plan",
            "sk-cp-china-fixture",
            ConnectionAuthType::ApiKey,
            "https://api.minimaxi.com/anthropic",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
        ),
        (
            "minimax-cn-api",
            "sk-api-china-fixture",
            ConnectionAuthType::Bearer,
            "https://api.minimaxi.com/anthropic",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = ClaudeCodeAdapter;
        let paths = adapter.resolve_paths(&environment(temp.path()), None);
        write_fixture(
            &paths.config_file,
            r#"{
              "env": {
                "ANTHROPIC_BASE_URL": "https://stale.invalid/v1",
                "ANTHROPIC_API_KEY": "stale-api-key",
                "ANTHROPIC_AUTH_TOKEN": "stale-auth-token",
                "KEEP_ME": "yes"
              },
              "model": "stale-model"
            }"#,
        )
        .await;
        let (provider, connection_id) = templated_provider(template_id, key, stored_auth_type);
        let target = ConfigurationTarget::Api {
            cli_id: CliId::ClaudeCode,
            provider_id: provider.id,
            connection_id,
            model: "MiniMax-M2.7".into(),
        };

        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        let output = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
        assert!(output.contains(expected_base_url));
        assert!(output.contains(&format!(r#""{expected_field}": "{key}""#)));
        assert!(!output.contains(&format!(r#""{removed_field}""#)));
        assert!(output.contains(r#""KEEP_ME": "yes""#));
    }
}

#[tokio::test]
async fn claude_template_matrix_replaces_all_managed_model_slots_and_tuning() {
    struct Expected {
        anthropic: &'static str,
        haiku: Option<&'static str>,
        sonnet: Option<&'static str>,
        opus: Option<&'static str>,
        subagent: Option<&'static str>,
        effort: Option<&'static str>,
        compact: Option<&'static str>,
        traffic: Option<&'static str>,
        timeout: Option<&'static str>,
    }
    let cases = [
        (
            "deepseek",
            Expected {
                anthropic: "selected-model[1m]",
                haiku: Some("selected-model"),
                sonnet: Some("selected-model[1m]"),
                opus: Some("selected-model[1m]"),
                subagent: Some("selected-model"),
                effort: Some("max"),
                compact: Some("786432"),
                traffic: None,
                timeout: None,
            },
        ),
        (
            "zhipuai",
            Expected {
                anthropic: "selected-model",
                haiku: Some("selected-model"),
                sonnet: Some("selected-model"),
                opus: Some("selected-model"),
                subagent: None,
                effort: None,
                compact: Some("1000000"),
                traffic: Some("1"),
                timeout: Some("3000000"),
            },
        ),
        (
            "zhipuai-coding-plan",
            Expected {
                anthropic: "selected-model",
                haiku: Some("selected-model"),
                sonnet: Some("selected-model"),
                opus: Some("selected-model"),
                subagent: None,
                effort: None,
                compact: Some("1000000"),
                traffic: Some("1"),
                timeout: Some("3000000"),
            },
        ),
        (
            "zai",
            Expected {
                anthropic: "selected-model",
                haiku: Some("selected-model"),
                sonnet: Some("selected-model"),
                opus: Some("selected-model"),
                subagent: None,
                effort: None,
                compact: Some("1000000"),
                traffic: Some("1"),
                timeout: Some("3000000"),
            },
        ),
        (
            "zai-coding-plan",
            Expected {
                anthropic: "selected-model",
                haiku: Some("selected-model"),
                sonnet: Some("selected-model"),
                opus: Some("selected-model"),
                subagent: None,
                effort: None,
                compact: Some("1000000"),
                traffic: Some("1"),
                timeout: Some("3000000"),
            },
        ),
        (
            "opencode",
            Expected {
                anthropic: "selected-model",
                haiku: None,
                sonnet: None,
                opus: None,
                subagent: None,
                effort: None,
                compact: None,
                traffic: None,
                timeout: None,
            },
        ),
        (
            "opencode-go",
            Expected {
                anthropic: "selected-model",
                haiku: None,
                sonnet: None,
                opus: None,
                subagent: None,
                effort: None,
                compact: None,
                traffic: None,
                timeout: None,
            },
        ),
    ];
    for (template_id, expected) in cases {
        let temp = TempDir::new().unwrap();
        let adapter = ClaudeCodeAdapter;
        let paths = adapter.resolve_paths(&environment(temp.path()), None);
        write_fixture(
            &paths.config_file,
            r#"{
              "model": "selected-model",
              "env": {
                "ANTHROPIC_MODEL": "old-main",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old-haiku",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "old-sonnet",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "old-opus",
                "CLAUDE_CODE_SUBAGENT_MODEL": "old-subagent",
                "ANTHROPIC_SMALL_FAST_MODEL": "old-small",
                "CLAUDE_CODE_EFFORT_LEVEL": "old-effort",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "old-compact",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "old-traffic",
                "API_TIMEOUT_MS": "old-timeout",
                "UNMANAGED": "keep"
              }
            }"#,
        )
        .await;
        let (provider, connection_id) = cli_adapter_provider(
            template_id,
            CliProtocol::AnthropicMessages,
            ConnectionAuthType::Bearer,
        );
        let target = ConfigurationTarget::Api {
            cli_id: CliId::ClaudeCode,
            provider_id: provider.id,
            connection_id,
            model: "selected-model".into(),
        };
        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        let output = std::str::from_utf8(&plan.files[0].target_content).unwrap();
        let value = parse_jsonc_value(output).unwrap();
        let env = value["env"].as_object().unwrap();
        let field = |name: &str| env.get(name).and_then(serde_json::Value::as_str);
        assert_eq!(value["model"], "selected-model", "{template_id}");
        assert_eq!(
            field("ANTHROPIC_MODEL"),
            Some(expected.anthropic),
            "{template_id}"
        );
        assert_eq!(
            field("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            expected.haiku,
            "{template_id}"
        );
        assert_eq!(
            field("ANTHROPIC_DEFAULT_SONNET_MODEL"),
            expected.sonnet,
            "{template_id}"
        );
        assert_eq!(
            field("ANTHROPIC_DEFAULT_OPUS_MODEL"),
            expected.opus,
            "{template_id}"
        );
        assert_eq!(
            field("CLAUDE_CODE_SUBAGENT_MODEL"),
            expected.subagent,
            "{template_id}"
        );
        assert_eq!(
            field("CLAUDE_CODE_EFFORT_LEVEL"),
            expected.effort,
            "{template_id}"
        );
        assert_eq!(
            field("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
            expected.compact,
            "{template_id}"
        );
        assert_eq!(
            field("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
            expected.traffic,
            "{template_id}"
        );
        assert_eq!(field("API_TIMEOUT_MS"), expected.timeout, "{template_id}");
        assert_eq!(field("ANTHROPIC_SMALL_FAST_MODEL"), None, "{template_id}");
        assert_eq!(
            field("ANTHROPIC_AUTH_TOKEN"),
            Some("fixture-template-key-not-real")
        );
        assert_eq!(
            field("ANTHROPIC_BASE_URL"),
            Some("https://saved-endpoint.invalid/custom/v1")
        );
        assert_eq!(field("UNMANAGED"), Some("keep"));
    }
}

#[tokio::test]
async fn claude_generic_template_repairs_non_primary_slots() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "selected-model",
          "env": {
            "ANTHROPIC_MODEL": "old",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "old",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "old",
            "CLAUDE_CODE_SUBAGENT_MODEL": "old"
          }
        }"#,
    )
    .await;
    let (provider, connection_id) = provider(CliProtocol::AnthropicMessages);
    let target = ConfigurationTarget::Api {
        cli_id: CliId::ClaudeCode,
        provider_id: provider.id,
        connection_id,
        model: "selected-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    assert_ne!(
        plan.files[0].source_content.as_ref().unwrap(),
        &plan.files[0].target_content
    );
    let output =
        parse_jsonc_value(std::str::from_utf8(&plan.files[0].target_content).unwrap()).unwrap();
    for field in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ] {
        assert_eq!(output["env"][field], "selected-model");
    }
}

#[tokio::test]
async fn claude_plan_is_idempotent_and_frozen_verification_checks_auxiliary_model_slots() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{}\n").await;
    let (provider, connection_id) = provider(CliProtocol::AnthropicMessages);
    let target = ConfigurationTarget::Api {
        cli_id: CliId::ClaudeCode,
        provider_id: provider.id,
        connection_id,
        model: "fixture-model".into(),
    };
    let first = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    materialize_plan(&first).await;
    assert!(adapter.verify_applied(&first).await.unwrap());

    let second = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    assert_eq!(
        second.files[0].source_content,
        Some(first.files[0].target_content.clone())
    );
    assert_eq!(
        second.files[0].target_content,
        first.files[0].target_content
    );

    let output = std::str::from_utf8(&first.files[0].target_content).unwrap();
    let tampered = output.replace(
        r#""ANTHROPIC_DEFAULT_HAIKU_MODEL": "fixture-model""#,
        r#""ANTHROPIC_DEFAULT_HAIKU_MODEL": "tampered-model""#,
    );
    assert_ne!(tampered, output);
    tokio::fs::write(&paths.config_file, tampered)
        .await
        .unwrap();
    assert!(!adapter.verify_applied(&first).await.unwrap());
}

#[tokio::test]
async fn claude_oauth_clears_every_api_template_field() {
    let temp = TempDir::new().unwrap();
    let adapter = ClaudeCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "old",
          "env": {
            "ANTHROPIC_BASE_URL": "https://old.invalid",
            "ANTHROPIC_API_KEY": "old",
            "ANTHROPIC_AUTH_TOKEN": "old",
            "ANTHROPIC_MODEL": "old",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "old",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "old",
            "CLAUDE_CODE_SUBAGENT_MODEL": "old",
            "ANTHROPIC_SMALL_FAST_MODEL": "old",
            "CLAUDE_CODE_EFFORT_LEVEL": "old",
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "old",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "old",
            "API_TIMEOUT_MS": "old",
            "UNMANAGED": "keep"
          }
        }"#,
    )
    .await;
    let provider = oauth_provider(
        OAuthKind::Anthropic,
        r#"{"claudeAiOauth":{"accessToken":"fixture-token"}}"#,
    );
    let target = ConfigurationTarget::Oauth {
        cli_id: CliId::ClaudeCode,
        provider_id: provider.id,
        model: "oauth-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let output =
        parse_jsonc_value(std::str::from_utf8(&plan.files[0].target_content).unwrap()).unwrap();
    assert_eq!(output["model"], "oauth-model");
    assert_eq!(output["env"]["UNMANAGED"], "keep");
    for field in cliswitch_lib::config_templates::CLAUDE_MANAGED_ENV_FIELDS {
        if cfg!(target_os = "macos") && field == "CLAUDE_CODE_OAUTH_TOKEN" {
            assert!(output["env"].get(field).is_some());
        } else {
            assert!(output["env"].get(field).is_none(), "{field}");
        }
    }
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
    let config_file = plan
        .files
        .iter()
        .find(|file| file.path == paths.config_file)
        .unwrap();
    let output = String::from_utf8(config_file.target_content.clone()).unwrap();
    assert!(output.contains("# Scrubbed stable Codex CLI fixture."));
    assert!(output.contains("[profiles.keep_me]"));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(output.contains("experimental_bearer_token = \"fixture-new-key-not-real\""));
    assert!(!output.contains("env_key"));
}

#[tokio::test]
async fn codex_templates_write_reasoning_login_fields_and_a_uuid_model_catalog() {
    for template_id in [
        "deepseek",
        "zhipuai",
        "zhipuai-coding-plan",
        "zai",
        "zai-coding-plan",
        "opencode",
        "opencode-go",
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = CodexAdapter;
        let paths = adapter.resolve_paths(&environment(temp.path()), None);
        let external = temp.path().join("external-models.json");
        write_fixture(&external, r#"{"models":[{"slug":"do-not-touch"}]}"#).await;
        write_fixture(
            &paths.config_file,
            &format!(
                "# keep config comment\nmodel = \"old\"\nmodel_catalog_json = \"{}\"\n\n[profiles.keep]\nmodel = \"profile-model\"\n",
                external.display()
            ),
        )
        .await;
        let (provider, connection_id) = cli_adapter_provider(
            template_id,
            CliProtocol::OpenaiResponses,
            ConnectionAuthType::Bearer,
        );
        let target = ConfigurationTarget::Api {
            cli_id: CliId::Codex,
            provider_id: provider.id,
            connection_id,
            model: "selected-model".into(),
        };
        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        assert_eq!(plan.files.len(), 2, "{template_id}");
        assert!(
            plan.files[0]
                .path
                .starts_with(paths.config_directory.join("cliswitch-models"))
        );
        assert_eq!(plan.files[1].path, paths.config_file);
        let expected_name = format!("{}-{}.json", provider.id.simple(), connection_id.simple());
        assert_eq!(
            plan.files[0].path.file_name().unwrap().to_str().unwrap(),
            expected_name
        );
        assert_eq!(plan.files[0].source_content, None);
        let models =
            parse_jsonc_value(std::str::from_utf8(&plan.files[0].target_content).unwrap()).unwrap();
        assert_eq!(models["models"].as_array().unwrap().len(), 1);
        assert_eq!(models["models"][0]["slug"], "selected-model");
        let config = std::str::from_utf8(&plan.files[1].target_content).unwrap();
        let document = parse_toml(config).unwrap();
        assert!(config.contains("# keep config comment"));
        assert!(config.contains("[profiles.keep]"));
        assert_eq!(document["model"].as_str(), Some("selected-model"));
        let provider_id = namespaced_provider_id(provider.id);
        assert_eq!(
            document["model_provider"].as_str(),
            Some(provider_id.as_str())
        );
        let provider_table = document["model_providers"]
            .as_table()
            .unwrap()
            .get(&provider_id)
            .and_then(toml_edit::Item::as_table)
            .unwrap();
        assert_eq!(
            provider_table["name"].as_str(),
            Some(provider.name.as_str())
        );
        assert_eq!(
            provider_table["base_url"].as_str(),
            Some("https://saved-endpoint.invalid/custom/v1")
        );
        assert_eq!(provider_table["wire_api"].as_str(), Some("responses"));
        assert_eq!(
            provider_table["experimental_bearer_token"].as_str(),
            Some("fixture-template-key-not-real")
        );
        for field in ["env_key", "requires_openai_auth", "auth"] {
            assert!(
                provider_table.get(field).is_none(),
                "{template_id}: {field}"
            );
        }
        assert_eq!(
            document["model_reasoning_effort"].as_str(),
            Some(
                if matches!(
                    template_id,
                    "zhipuai" | "zhipuai-coding-plan" | "zai" | "zai-coding-plan"
                ) {
                    "max"
                } else {
                    "high"
                }
            )
        );
        assert_eq!(
            document["model_catalog_json"].as_str(),
            plan.files[0].path.to_str()
        );
        assert_eq!(
            document
                .get("preferred_auth_method")
                .and_then(|item| item.as_str()),
            (template_id == "deepseek").then_some("apikey")
        );
        assert_eq!(
            document
                .get("forced_login_method")
                .and_then(|item| item.as_str()),
            (template_id == "deepseek").then_some("api")
        );
        assert_eq!(
            tokio::fs::read(&external).await.unwrap(),
            br#"{"models":[{"slug":"do-not-touch"}]}"#
        );
    }
}

#[tokio::test]
async fn codex_exact_vision_template_and_frozen_multi_file_plan_are_stable() {
    let temp = TempDir::new().unwrap();
    let adapter = CodexAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "# keep\nmodel = \"old\"\n").await;
    let (provider, connection_id) = cli_adapter_provider(
        "deepseek",
        CliProtocol::OpenaiResponses,
        ConnectionAuthType::Bearer,
    );
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Codex,
        provider_id: provider.id,
        connection_id,
        model: "deepseek-v4-flash-vision-exp".into(),
    };
    let first = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let model =
        parse_jsonc_value(std::str::from_utf8(&first.files[0].target_content).unwrap()).unwrap();
    assert_eq!(
        model["models"][0]["display_name"],
        "DeepSeek-V4-Flash-Vision"
    );
    assert_eq!(
        model["models"][0]["input_modalities"],
        serde_json::json!(["text", "image"])
    );
    for file in &first.files {
        tokio::fs::create_dir_all(file.path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&file.path, &file.target_content)
            .await
            .unwrap();
    }
    assert!(adapter.verify_applied(&first).await.unwrap());
    let second = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    for (first, second) in first.files.iter().zip(&second.files) {
        assert_eq!(first.path, second.path);
        assert_eq!(first.target_content, second.target_content);
        assert_eq!(
            second.source_content.as_deref(),
            Some(second.target_content.as_slice())
        );
    }
    tokio::fs::write(&first.files[0].path, b"{\"models\":[]}")
        .await
        .unwrap();
    assert!(!adapter.verify_applied(&first).await.unwrap());
}

#[tokio::test]
async fn codex_managed_catalog_preserves_root_extensions_and_scan_diagnoses_damage_or_absence() {
    let temp = TempDir::new().unwrap();
    let adapter = CodexAdapter;
    let manual_directory = temp.path().join("Codex Config With Spaces");
    let paths = adapter.resolve_paths(&environment(temp.path()), Some(manual_directory.clone()));
    write_fixture(&paths.config_file, "model = \"old\"\n").await;
    let (provider, connection_id) = cli_adapter_provider(
        "deepseek",
        CliProtocol::OpenaiResponses,
        ConnectionAuthType::Bearer,
    );
    let catalog_path = manual_directory.join("cliswitch-models").join(format!(
        "{}-{}.json",
        provider.id.simple(),
        connection_id.simple()
    ));
    write_fixture(
        &catalog_path,
        r#"{
          // keep catalog comment
          "extension": { "keep": true },
          "models": [{ "slug": "old" }]
        }"#,
    )
    .await;
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Codex,
        provider_id: provider.id,
        connection_id,
        model: "deepseek-v4-pro".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    assert_eq!(plan.files[0].path, catalog_path);
    let catalog_text = std::str::from_utf8(&plan.files[0].target_content).unwrap();
    assert!(catalog_text.contains("// keep catalog comment"));
    let catalog = parse_jsonc_value(catalog_text).unwrap();
    assert_eq!(catalog["extension"]["keep"], true);
    assert_eq!(catalog["models"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["models"][0]["slug"], "deepseek-v4-pro");
    materialize_plan(&plan).await;

    let current = adapter
        .read_current(&paths, &environment(temp.path()))
        .await
        .unwrap();
    assert!(current.current.sources.iter().any(|source| {
        source.source_id == "codex-model-catalog" && source.display_path == catalog_path
    }));

    tokio::fs::write(&catalog_path, b"{not-json").await.unwrap();
    let current = adapter
        .read_current(&paths, &environment(temp.path()))
        .await
        .unwrap();
    assert!(
        current.current.diagnostics.iter().any(|message| {
            message.contains("Unable to parse the CLISwitch Codex model catalog")
        })
    );
    let error = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        cliswitch_lib::error::AppError::Serialization(_)
    ));
    assert_eq!(tokio::fs::read(&catalog_path).await.unwrap(), b"{not-json");

    tokio::fs::write(&catalog_path, &plan.files[0].target_content)
        .await
        .unwrap();
    adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    tokio::fs::remove_file(&catalog_path).await.unwrap();
    let current = adapter
        .read_current(&paths, &environment(temp.path()))
        .await
        .unwrap();
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .any(|message| { message.contains("model catalog is missing") })
    );
    assert!(
        current
            .current
            .sources
            .iter()
            .any(|source| { source.source_id == "codex-model-catalog" && source.digest.is_none() })
    );
    let recreated = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    assert_eq!(recreated.files[0].source_content, None);
}

#[tokio::test]
async fn codex_oauth_clears_api_template_top_level_fields() {
    let temp = TempDir::new().unwrap();
    let adapter = CodexAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(
        &paths.config_file,
        r#"model = "old"
model_provider = "old-provider"
model_catalog_json = "/tmp/old-models.json"
model_reasoning_effort = "max"
preferred_auth_method = "apikey"
forced_login_method = "api"
unmanaged = "keep"
"#,
    )
    .await;
    let provider = oauth_provider(OAuthKind::Codex, r#"{"tokens":{"account_id":"fixture"}}"#);
    let target = ConfigurationTarget::Oauth {
        cli_id: CliId::Codex,
        provider_id: provider.id,
        model: "oauth-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let config = plan
        .files
        .iter()
        .find(|file| file.path == paths.config_file)
        .unwrap();
    let document = parse_toml(std::str::from_utf8(&config.target_content).unwrap()).unwrap();
    assert_eq!(document["model"].as_str(), Some("oauth-model"));
    assert_eq!(document["model_provider"].as_str(), Some("openai"));
    assert_eq!(
        document["cli_auth_credentials_store"].as_str(),
        Some("file")
    );
    assert_eq!(document["unmanaged"].as_str(), Some("keep"));
    for field in [
        "model_catalog_json",
        "model_reasoning_effort",
        "preferred_auth_method",
        "forced_login_method",
    ] {
        assert!(document.get(field).is_none(), "{field}");
    }
}

#[tokio::test]
async fn codex_recognizes_exact_and_unique_cli_adapter_responses_endpoints() {
    for (provider_id, endpoint, expected_template_id) in [
        (
            "zai-coding-plan",
            "https://api.z.ai/api/v1",
            "zai-coding-plan",
        ),
        ("private-alias", "https://api.deepseek.com", "deepseek"),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = CodexAdapter;
        let host = environment(temp.path());
        let paths = adapter.resolve_paths(&host, None);
        write_fixture(
            &paths.config_file,
            &format!(
                r#"model = "manual-model"
model_provider = "{provider_id}"

[model_providers.{provider_id}]
base_url = "{endpoint}"
wire_api = "responses"
experimental_bearer_token = "fixture-key"
"#
            ),
        )
        .await;

        let current = adapter.read_current(&paths, &host).await.unwrap();
        let candidate = &current.unmanaged_api_candidates[0];
        assert_eq!(candidate.template_id.as_deref(), Some(expected_template_id));
        assert_eq!(
            candidate.connection.template_endpoint_id.as_deref(),
            Some("responses")
        );
    }
}

#[tokio::test]
async fn codex_credentials_without_a_model_are_importable() {
    let temp = TempDir::new().unwrap();
    let adapter = CodexAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"model_provider = "fixture-provider"

[model_providers.fixture-provider]
base_url = "https://gateway.invalid/v1"
wire_api = "responses"
experimental_bearer_token = "fixture-key"
"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert_eq!(current.current.model, None);
    assert_eq!(current.unmanaged_api_candidates.len(), 1);
    let candidate = &current.unmanaged_api_candidates[0];
    assert_eq!(candidate.default_model, None);
    assert!(candidate.available_models.is_empty());
    assert!(candidate.connection.default_model.is_empty());
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
async fn opencode_template_patch_preserves_extensions_and_cleans_current_auth_entry() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    let (provider, connection_id) = provider(CliProtocol::OpenaiResponses);
    let provider_id = namespaced_provider_id(provider.id);
    write_fixture(
        &paths.config_file,
        &format!(
            r#"{{
              // keep config comment
              "unknownRoot": true,
              "provider": {{
                "other": {{ "npm": "keep-other" }},
                "{provider_id}": {{
                  "extension": "keep-provider",
                  "options": {{ "baseURL": "https://old.invalid", "apiKey": "old-inline", "header": "keep-option" }},
                  "models": {{
                    "fixture-model": {{ "name": "old-name", "custom": "keep-model" }},
                    "other-model": {{ "name": "keep-other-model" }}
                  }}
                }}
              }}
            }}"#
        ),
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        &format!(
            r#"{{
              "other": {{ "type": "api", "key": "keep-other-key" }},
              "{provider_id}": {{
                "type": "oauth",
                "key": "old-key",
                "refresh": "old-refresh",
                "access": "old-access",
                "expires": 123,
                "accountId": "old-account",
                "enterpriseUrl": "https://old.invalid",
                "custom": "keep-auth"
              }}
            }}"#
        ),
    )
    .await;
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
    let config_text = std::str::from_utf8(&plan.files[0].target_content).unwrap();
    assert!(config_text.contains("// keep config comment"));
    let config = parse_jsonc_value(config_text).unwrap();
    let current = &config["provider"][&provider_id];
    assert_eq!(config["unknownRoot"], true);
    assert_eq!(config["provider"]["other"]["npm"], "keep-other");
    assert_eq!(current["extension"], "keep-provider");
    assert_eq!(current["options"]["header"], "keep-option");
    assert!(current["options"].get("apiKey").is_none());
    assert_eq!(current["models"]["other-model"]["name"], "keep-other-model");
    assert_eq!(current["models"]["fixture-model"]["custom"], "keep-model");
    assert_eq!(current["models"]["fixture-model"]["reasoning"], true);
    assert_eq!(config["$schema"], "https://opencode.ai/config.json");

    let auth =
        parse_jsonc_value(std::str::from_utf8(&plan.files[1].target_content).unwrap()).unwrap();
    let current_auth = auth[&provider_id].as_object().unwrap();
    assert_eq!(current_auth["type"], "api");
    assert_eq!(current_auth["key"], "fixture-new-key-not-real");
    assert_eq!(current_auth["custom"], "keep-auth");
    for field in ["refresh", "access", "expires", "accountId", "enterpriseUrl"] {
        assert!(!current_auth.contains_key(field));
    }
    assert_eq!(auth["other"]["key"], "keep-other-key");
}

#[tokio::test]
async fn opencode_all_provider_templates_keep_saved_transport_endpoint_and_instance_identity() {
    for template_id in [
        "deepseek",
        "zhipuai",
        "zhipuai-coding-plan",
        "zai",
        "zai-coding-plan",
        "opencode",
        "opencode-go",
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = OpenCodeAdapter;
        let paths = adapter.resolve_paths(&environment(temp.path()), None);
        write_fixture(&paths.config_file, "{}\n").await;
        write_fixture(paths.auth_file.as_ref().unwrap(), "{}\n").await;
        let (provider, connection_id) = cli_adapter_provider(
            template_id,
            CliProtocol::OpenaiResponses,
            ConnectionAuthType::Bearer,
        );
        let provider_id = namespaced_provider_id(provider.id);
        let target = ConfigurationTarget::Api {
            cli_id: CliId::Opencode,
            provider_id: provider.id,
            connection_id,
            model: "selected-model".into(),
        };
        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        let config =
            parse_jsonc_value(std::str::from_utf8(&plan.files[0].target_content).unwrap()).unwrap();
        let current = &config["provider"][&provider_id];
        assert_eq!(config["model"], format!("{provider_id}/selected-model"));
        assert_eq!(current["npm"], "@ai-sdk/openai");
        assert_eq!(current["name"], format!("{template_id} saved instance"));
        assert_eq!(
            current["options"]["baseURL"],
            "https://saved-endpoint.invalid/custom/v1"
        );
        assert_eq!(current["models"]["selected-model"]["reasoning"], true);
        let auth =
            parse_jsonc_value(std::str::from_utf8(&plan.files[1].target_content).unwrap()).unwrap();
        assert_eq!(auth[&provider_id]["type"], "api");
        assert_eq!(auth[&provider_id]["key"], "fixture-template-key-not-real");
    }
}

#[tokio::test]
async fn opencode_plan_is_idempotent_and_frozen_verification_checks_auth() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{}\n").await;
    write_fixture(paths.auth_file.as_ref().unwrap(), "{}\n").await;
    let (provider, connection_id) = provider(CliProtocol::OpenaiChat);
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: provider.id,
        connection_id,
        model: "fixture-model".into(),
    };
    let first = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    materialize_plan(&first).await;
    assert!(adapter.verify_applied(&first).await.unwrap());

    let second = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    for (first_file, second_file) in first.files.iter().zip(&second.files) {
        assert_eq!(
            second_file.source_content,
            Some(first_file.target_content.clone())
        );
        assert_eq!(second_file.target_content, first_file.target_content);
    }

    let auth_file = first
        .files
        .iter()
        .find(|file| file.path == *paths.auth_file.as_ref().unwrap())
        .unwrap();
    let auth = std::str::from_utf8(&auth_file.target_content).unwrap();
    let tampered = auth.replace("fixture-new-key-not-real", "tampered-key");
    assert_ne!(tampered, auth);
    tokio::fs::write(paths.auth_file.as_ref().unwrap(), tampered)
        .await
        .unwrap();
    assert!(!adapter.verify_applied(&first).await.unwrap());
}

#[tokio::test]
async fn opencode_two_namespaced_instances_do_not_replace_each_other() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{}\n").await;
    write_fixture(paths.auth_file.as_ref().unwrap(), "{}\n").await;
    let (first_provider, first_connection) = provider(CliProtocol::OpenaiChat);
    let first_target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: first_provider.id,
        connection_id: first_connection,
        model: "first-model".into(),
    };
    let first = adapter
        .plan_write(&paths, &first_target, &first_provider)
        .await
        .unwrap();
    for file in &first.files {
        tokio::fs::create_dir_all(file.path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&file.path, &file.target_content)
            .await
            .unwrap();
    }
    let (second_provider, second_connection) = provider(CliProtocol::AnthropicMessages);
    let second_target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: second_provider.id,
        connection_id: second_connection,
        model: "second-model".into(),
    };
    let second = adapter
        .plan_write(&paths, &second_target, &second_provider)
        .await
        .unwrap();
    let config =
        parse_jsonc_value(std::str::from_utf8(&second.files[0].target_content).unwrap()).unwrap();
    let auth =
        parse_jsonc_value(std::str::from_utf8(&second.files[1].target_content).unwrap()).unwrap();
    assert!(
        config["provider"]
            .get(namespaced_provider_id(first_provider.id))
            .is_some()
    );
    assert!(
        config["provider"]
            .get(namespaced_provider_id(second_provider.id))
            .is_some()
    );
    assert!(
        auth.get(namespaced_provider_id(first_provider.id))
            .is_some()
    );
    assert!(
        auth.get(namespaced_provider_id(second_provider.id))
            .is_some()
    );
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
async fn opencode_cli_adapter_providers_use_the_declared_chat_transport() {
    for (
        provider_id,
        model_id,
        expected_protocol,
        expected_endpoint,
        expected_package,
        wrong_route_model,
    ) in [
        (
            "opencode",
            "gpt-5.6-sol",
            CliProtocol::OpenaiChat,
            "https://opencode.ai/zen/v1",
            "@ai-sdk/openai-compatible",
            "glm-5",
        ),
        (
            "opencode-go",
            "glm-5.3",
            CliProtocol::OpenaiChat,
            "https://opencode.ai/zen/go/v1",
            "@ai-sdk/openai-compatible",
            "gpt-5.6-luna",
        ),
    ] {
        let temp = TempDir::new().unwrap();
        let adapter = OpenCodeAdapter;
        let host = environment(temp.path());
        let paths = adapter.resolve_paths(&host, None);
        write_fixture(
            &paths.config_file,
            &format!(
                r#"{{
                  "model": "{provider_id}/{model_id}"
                }}"#
            ),
        )
        .await;
        write_fixture(
            paths.auth_file.as_ref().unwrap(),
            &format!(
                r#"{{ "{provider_id}": {{ "type": "api", "key": "fixture-opencode-key" }} }}"#
            ),
        )
        .await;

        let current = adapter.read_current(&paths, &host).await.unwrap();
        assert!(current.current.diagnostics.iter().all(|message| {
            !message.contains("cannot be saved without")
                && !message.contains("no supported model route")
        }));
        assert_eq!(current.current.protocol, Some(expected_protocol));
        let candidate = &current.unmanaged_api_candidates[0];
        assert_eq!(candidate.template_id.as_deref(), Some(provider_id));
        assert!(!candidate.model_routed);
        assert_eq!(candidate.default_model.as_deref(), Some(model_id));
        assert_eq!(
            candidate.connection.template_endpoint_id.as_deref(),
            Some("openai-compatible")
        );
        assert_eq!(candidate.connection.protocol, expected_protocol);
        assert_eq!(candidate.connection.endpoint.as_str(), expected_endpoint);
        assert_eq!(candidate.available_models, [model_id]);

        let provider = ProviderProfile {
            id: Uuid::new_v4(),
            name: "OpenCode routed fixture".into(),
            template_id: candidate.template_id.clone(),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            data: ProviderData::Api(ApiProviderData {
                connections: vec![candidate.connection.clone()],
            }),
        };
        let target = ConfigurationTarget::Api {
            cli_id: CliId::Opencode,
            provider_id: provider.id,
            connection_id: candidate.connection.id,
            model: model_id.into(),
        };
        let plan = adapter
            .plan_write(&paths, &target, &provider)
            .await
            .unwrap();
        let config = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
        assert!(config.contains(expected_package));
        assert!(config.contains(expected_endpoint));

        // A manually entered model ID must not route the saved connection to another protocol.
        let wrong_route_target = ConfigurationTarget::Api {
            cli_id: CliId::Opencode,
            provider_id: provider.id,
            connection_id: candidate.connection.id,
            model: wrong_route_model.into(),
        };
        let plan = adapter
            .plan_write(&paths, &wrong_route_target, &provider)
            .await
            .unwrap();
        let config = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
        assert!(config.contains(wrong_route_model));
    }
}

#[tokio::test]
async fn opencode_cli_adapter_credentials_without_a_model_are_importable() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(&paths.config_file, "{}\n").await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{
          "opencode": { "type": "api", "key": "fixture-zen-key" },
          "opencode-go": { "type": "api", "key": "fixture-go-key" }
        }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert_eq!(current.current.model, None);
    assert_eq!(current.unmanaged_api_candidates.len(), 2);
    for provider_id in ["opencode", "opencode-go"] {
        let candidate = current
            .unmanaged_api_candidates
            .iter()
            .find(|candidate| candidate.source_provider_id == provider_id)
            .unwrap();
        assert_eq!(candidate.default_model, None);
        assert!(candidate.available_models.is_empty());
        assert!(candidate.connection.default_model.is_empty());
        candidate
            .connection
            .validate_without_default_model()
            .unwrap();
    }
    assert!(
        current
            .current
            .diagnostics
            .iter()
            .all(|message| !message.contains("cannot be saved without"))
    );
}

#[tokio::test]
async fn opencode_cli_adapter_accepts_a_configured_manual_model() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{ "model": "opencode/gemini-3.7-flash" }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{ "opencode": { "type": "api", "key": "fixture-zen-key" } }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert!(current.current.diagnostics.iter().all(|message| {
        !message.contains("gemini-3.7-flash") || !message.contains("unavailable")
    }));
    let candidate = &current.unmanaged_api_candidates[0];
    assert!(!candidate.model_routed);
    assert_eq!(candidate.template_id.as_deref(), Some("opencode"));
    assert_eq!(candidate.default_model.as_deref(), Some("gemini-3.7-flash"));
    assert_eq!(candidate.available_models, ["gemini-3.7-flash"]);
}

#[tokio::test]
async fn opencode_cli_adapter_base_url_override_keeps_provider_transport() {
    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{
          "model": "opencode/gpt-5.6-sol",
          "provider": {
            "opencode": {
              "options": { "baseURL": "https://proxy.invalid/zen/v1" }
            }
          }
        }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{ "opencode": { "type": "api", "key": "fixture-zen-key" } }"#,
    )
    .await;

    let current = adapter.read_current(&paths, &host).await.unwrap();
    assert_eq!(current.current.protocol, Some(CliProtocol::OpenaiChat));
    let candidate = &current.unmanaged_api_candidates[0];
    assert_eq!(candidate.template_id.as_deref(), Some("opencode"));
    assert!(!candidate.model_routed);
    assert_eq!(candidate.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://proxy.invalid/zen/v1"
    );
    assert_eq!(candidate.default_model.as_deref(), Some("gpt-5.6-sol"));
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
    assert_eq!(candidate.suggested_name, "Zhipu AI Coding Plan");
    assert_eq!(
        candidate.template_id.as_deref(),
        Some("zhipuai-coding-plan")
    );
    assert_eq!(
        candidate.connection.template_endpoint_id.as_deref(),
        Some("openai-compatible")
    );
    assert_eq!(candidate.connection.credential_slot_id, "api-key");
    assert_eq!(candidate.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(candidate.connection.auth_type, ConnectionAuthType::Bearer);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    assert_eq!(candidate.available_models[0], "glm-5.3");
    assert_eq!(candidate.available_models, ["glm-5.3"]);
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
    assert_eq!(
        candidate.template_id.as_deref(),
        Some("zhipuai-coding-plan")
    );
    assert_eq!(
        candidate.connection.template_endpoint_id.as_deref(),
        Some("anthropic-messages")
    );
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
    assert!(!candidate.model_routed);
    assert_eq!(
        candidate.connection.template_endpoint_id.as_deref(),
        Some("chat")
    );
    assert_eq!(candidate.connection.protocol, CliProtocol::OpenaiChat);
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://openrouter.ai/api/v1"
    );

    let provider = ProviderProfile {
        id: Uuid::new_v4(),
        name: candidate.suggested_name.clone(),
        template_id: candidate.template_id.clone(),
        revision: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        data: ProviderData::Api(ApiProviderData {
            connections: vec![candidate.connection.clone()],
        }),
    };
    let target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: provider.id,
        connection_id: candidate.connection.id,
        model: "fixture-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &target, &provider)
        .await
        .unwrap();
    let config = String::from_utf8(plan.files[0].target_content.clone()).unwrap();
    assert!(config.contains("@openrouter/ai-sdk-provider"));
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
