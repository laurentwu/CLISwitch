use std::path::Path;

use cliswitch_lib::{
    config_templates::{
        QwenTemplateBindings, RenderedManagedConfig, TemplateBindings, TemplateSelection,
        render_managed_config, resolve_templates,
    },
    domain::{CliId, CliProtocol, ConnectionAuthType},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureInput {
    provider_id: String,
    provider_name: String,
    endpoint: String,
    api_key: String,
    model: String,
}

fn input() -> FixtureInput {
    serde_json::from_str(include_str!("fixtures/config-templates/render-input.json")).unwrap()
}

fn expected() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/config-templates/render-expected.json"
    ))
    .unwrap()
}

fn bindings<'a>(input: &'a FixtureInput, catalog: &'a Path) -> TemplateBindings<'a> {
    TemplateBindings {
        provider_id: &input.provider_id,
        provider_name: &input.provider_name,
        endpoint: &input.endpoint,
        auth_type: ConnectionAuthType::Bearer,
        api_key: &input.api_key,
        model: &input.model,
        model_catalog_path: Some(catalog),
        qwen: None,
    }
}

#[test]
fn independent_fixtures_match_claude_deepseek_rendering() {
    let input = input();
    let catalog = std::env::temp_dir().join("cliswitch-fixture-models.json");
    let templates = resolve_templates(&TemplateSelection {
        cli_id: CliId::ClaudeCode,
        template_id: Some("deepseek"),
        protocol: CliProtocol::AnthropicMessages,
        model: &input.model,
    })
    .unwrap();
    let RenderedManagedConfig::Claude(rendered) =
        render_managed_config(&templates, &bindings(&input, &catalog)).unwrap()
    else {
        unreachable!()
    };
    let actual = json!({
        "$schema": rendered.schema,
        "model": rendered.model,
        "env": rendered.env,
    });
    assert_eq!(actual, expected()["claudeDeepseek"]);
}

#[test]
fn independent_fixtures_match_codex_exact_model_rendering() {
    let input = input();
    let catalog = std::env::temp_dir().join("cliswitch-fixture-models.json");
    let templates = resolve_templates(&TemplateSelection {
        cli_id: CliId::Codex,
        template_id: Some("deepseek"),
        protocol: CliProtocol::OpenaiResponses,
        model: &input.model,
    })
    .unwrap();
    let RenderedManagedConfig::Codex(rendered) =
        render_managed_config(&templates, &bindings(&input, &catalog)).unwrap()
    else {
        unreachable!()
    };
    let actual = json!({
        "model": rendered.model,
        "reasoningEffort": rendered.reasoning_effort,
        "preferredAuthMethod": rendered.preferred_auth_method,
        "forcedLoginMethod": rendered.forced_login_method,
        "displayName": rendered.model_entry["display_name"],
        "inputModalities": rendered.model_entry["input_modalities"],
    });
    assert_eq!(actual, expected()["codexDeepseekVision"]);
}

#[test]
fn independent_fixtures_match_opencode_protocol_adaptation() {
    let input = input();
    let catalog = std::env::temp_dir().join("cliswitch-fixture-models.json");
    let templates = resolve_templates(&TemplateSelection {
        cli_id: CliId::Opencode,
        template_id: Some("deepseek"),
        protocol: CliProtocol::OpenaiChat,
        model: &input.model,
    })
    .unwrap();
    let RenderedManagedConfig::OpenCode(rendered) =
        render_managed_config(&templates, &bindings(&input, &catalog)).unwrap()
    else {
        unreachable!()
    };
    let actual = json!({
        "$schema": rendered.schema,
        "model": rendered.model_reference,
        "npm": rendered.npm_package,
        "name": rendered.model_name,
        "reasoning": rendered.reasoning,
    });
    assert_eq!(actual, expected()["opencodeChat"]);
}

#[test]
fn qwen_provider_and_generic_templates_render_only_typed_user_bindings() {
    let input = input();
    let catalog = std::env::temp_dir().join("cliswitch-fixture-models.json");
    let group_id = "cliswitch_qwen_0123456789abcdef0123456789abcdef";
    let env_key = "CLISWITCH_QWEN_KEY_0123456789ABCDEF0123456789ABCDEF";

    for template_id in [
        Some("deepseek"),
        Some("zhipuai"),
        Some("zhipuai-coding-plan"),
        Some("zai"),
        Some("zai-coding-plan"),
        Some("opencode"),
        Some("opencode-go"),
        None,
    ] {
        let templates = resolve_templates(&TemplateSelection {
            cli_id: CliId::Qwen,
            template_id,
            protocol: CliProtocol::OpenaiChat,
            model: &input.model,
        })
        .unwrap();
        let rendered = render_managed_config(
            &templates,
            &TemplateBindings {
                provider_id: &input.provider_id,
                provider_name: &input.provider_name,
                endpoint: &input.endpoint,
                auth_type: ConnectionAuthType::Bearer,
                api_key: &input.api_key,
                model: &input.model,
                model_catalog_path: Some(&catalog),
                qwen: Some(QwenTemplateBindings { group_id, env_key }),
            },
        )
        .unwrap();
        let RenderedManagedConfig::Qwen(rendered) = rendered else {
            unreachable!()
        };

        assert_eq!(rendered.group_id, group_id);
        assert_eq!(rendered.protocol, "openai");
        assert_eq!(rendered.model, input.model);
        assert_eq!(rendered.endpoint, input.endpoint);
        assert_eq!(rendered.env_key, env_key);
        assert_eq!(rendered.api_key, input.api_key);
        assert_eq!(
            rendered.model_entry,
            json!({
                "id": input.model,
                "name": input.model,
                "envKey": env_key,
                "baseUrl": input.endpoint,
            })
        );
        let serialized = serde_json::to_string(&rendered.model_entry).unwrap();
        assert!(!serialized.contains("<model-id>"));
        assert!(!serialized.contains("DEEPSEEK_API_KEY"));
    }
}

#[test]
fn qwen_templates_reject_non_chat_protocols() {
    let result = resolve_templates(&TemplateSelection {
        cli_id: CliId::Qwen,
        template_id: Some("deepseek"),
        protocol: CliProtocol::OpenaiResponses,
        model: "fixture-model",
    });
    let Err(error) = result else {
        panic!("Qwen must reject non-Chat protocols");
    };
    assert!(error.to_string().contains("OpenAI Chat Completions"));
}
