use std::path::Path;

use cliswitch_lib::{
    config_templates::{
        RenderedManagedConfig, TemplateBindings, TemplateSelection, render_managed_config,
        resolve_templates,
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
