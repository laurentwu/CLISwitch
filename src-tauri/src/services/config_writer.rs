use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstRootNode},
};
use serde_json::Value as JsonValue;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub enum JsonPatch {
    SetString { path: Vec<String>, value: String },
    SetValue { path: Vec<String>, value: JsonValue },
    Remove { path: Vec<String> },
}

pub fn parse_jsonc_value(text: &str) -> AppResult<JsonValue> {
    let effective = if text.trim().is_empty() { "{}" } else { text };
    jsonc_parser::parse_to_serde_value(effective, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(error.to_string()))
}

pub fn patch_jsonc(text: &str, patches: &[JsonPatch]) -> AppResult<String> {
    let effective = if text.trim().is_empty() { "{}\n" } else { text };
    let root = CstRootNode::parse(effective, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    let root_object = root
        .object_value()
        .ok_or_else(|| AppError::Unsupported("JSONC root must be an object".into()))?;
    for patch in patches {
        match patch {
            JsonPatch::SetString { path, value } => {
                set_json_path(&root_object, path, CstInputValue::String(value.clone()))?
            }
            JsonPatch::SetValue { path, value } => {
                set_json_path(&root_object, path, json_to_cst(value))?
            }
            JsonPatch::Remove { path } => remove_json_path(&root_object, path)?,
        }
    }
    let output = root.to_string();
    CstRootNode::parse(&output, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    Ok(output)
}

fn object_at_path(
    root: &jsonc_parser::cst::CstObject,
    path: &[String],
    create: bool,
) -> AppResult<Option<jsonc_parser::cst::CstObject>> {
    let mut current = root.clone();
    for segment in path {
        current = match current.get(segment) {
            Some(property) => property.object_value().ok_or_else(|| {
                AppError::Unsupported(format!("JSONC field {segment} must be an object"))
            })?,
            None if create => current.object_value_or_set(segment),
            None => return Ok(None),
        };
    }
    Ok(Some(current))
}

fn set_json_path(
    root: &jsonc_parser::cst::CstObject,
    path: &[String],
    value: CstInputValue,
) -> AppResult<()> {
    let (name, parent_path) = path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON patch path cannot be empty".into()))?;
    let parent = object_at_path(root, parent_path, true)?.expect("created parent object");
    match parent.get(name) {
        Some(property) => property.set_value(value),
        None => {
            parent.append(name, value);
        }
    }
    Ok(())
}

fn remove_json_path(root: &jsonc_parser::cst::CstObject, path: &[String]) -> AppResult<()> {
    let (name, parent_path) = path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON patch path cannot be empty".into()))?;
    if let Some(parent) = object_at_path(root, parent_path, false)?
        && let Some(property) = parent.get(name)
    {
        property.remove();
    }
    Ok(())
}

fn json_to_cst(value: &JsonValue) -> CstInputValue {
    match value {
        JsonValue::Null => CstInputValue::Null,
        JsonValue::Bool(value) => CstInputValue::Bool(*value),
        JsonValue::Number(value) => CstInputValue::Number(value.to_string()),
        JsonValue::String(value) => CstInputValue::String(value.clone()),
        JsonValue::Array(values) => CstInputValue::Array(values.iter().map(json_to_cst).collect()),
        JsonValue::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_to_cst(value)))
                .collect(),
        ),
    }
}

pub fn patch_codex_api_toml(
    text: &str,
    provider_id: &str,
    provider_name: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> AppResult<String> {
    let mut document = parse_toml(text)?;
    document["model"] = value(model);
    document["model_provider"] = value(provider_id);
    let providers = ensure_table(&mut document, "model_providers")?;
    let provider = ensure_child_table(providers, provider_id)?;
    provider["name"] = value(provider_name);
    provider["base_url"] = value(base_url);
    provider["wire_api"] = value("responses");
    provider["experimental_bearer_token"] = value(api_key);
    provider.remove("env_key");
    provider.remove("requires_openai_auth");
    provider.remove("auth");
    let output = document.to_string();
    parse_toml(&output)?;
    Ok(output)
}

pub fn patch_codex_oauth_toml(text: &str, model: &str) -> AppResult<String> {
    let mut document = parse_toml(text)?;
    document["model"] = value(model);
    document["model_provider"] = value("openai");
    document["cli_auth_credentials_store"] = value("file");
    let output = document.to_string();
    parse_toml(&output)?;
    Ok(output)
}

pub fn parse_toml(text: &str) -> AppResult<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|error| AppError::Serialization(error.to_string()))
}

fn ensure_table<'a>(document: &'a mut DocumentMut, key: &str) -> AppResult<&'a mut Table> {
    if document.get(key).is_none() {
        document[key] = Item::Table(Table::new());
    }
    document[key]
        .as_table_mut()
        .ok_or_else(|| AppError::Unsupported(format!("TOML field {key} must be a table")))
}

fn ensure_child_table<'a>(table: &'a mut Table, key: &str) -> AppResult<&'a mut Table> {
    if table.get(key).is_none() {
        table[key] = Item::Table(Table::new());
    }
    table[key]
        .as_table_mut()
        .ok_or_else(|| AppError::Unsupported(format!("TOML field {key} must be a table")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_patch_preserves_comments_unknown_keys_and_crlf() {
        let source = "{\r\n  // keep me\r\n  \"unknown\": true,\r\n  \"env\": { \"OTHER\": \"keep\", },\r\n}\r\n";
        let output = patch_jsonc(
            source,
            &[
                JsonPatch::SetString {
                    path: vec!["model".into()],
                    value: "claude-sonnet".into(),
                },
                JsonPatch::SetString {
                    path: vec!["env".into(), "ANTHROPIC_BASE_URL".into()],
                    value: "https://example.test".into(),
                },
            ],
        )
        .unwrap();
        assert!(output.contains("// keep me"));
        assert!(output.contains("\"unknown\": true"));
        assert!(output.contains("\"OTHER\": \"keep\""));
        assert!(output.contains("\r\n"));
        assert!(output.contains("claude-sonnet"));
    }

    #[test]
    fn jsonc_patch_refuses_unknown_shape() {
        let error = patch_jsonc(
            r#"{ "env": "not-an-object" }"#,
            &[JsonPatch::SetString {
                path: vec!["env".into(), "KEY".into()],
                value: "value".into(),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Unsupported(_)));
    }

    #[test]
    fn toml_patch_preserves_comments_and_unmanaged_tables() {
        let source = "# keep me\nmodel = \"old\"\n\n[profiles.work]\nmodel = \"other\"\n";
        let output = patch_codex_api_toml(
            source,
            "cliswitch_123",
            "Example",
            "https://example.test/v1",
            "secret",
            "gpt-test",
        )
        .unwrap();
        assert!(output.contains("# keep me"));
        assert!(output.contains("[profiles.work]"));
        assert!(output.contains("wire_api = \"responses\""));
        assert!(output.contains("experimental_bearer_token = \"secret\""));
    }
}
