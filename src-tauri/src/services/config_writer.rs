use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstRootNode},
};
use serde_json::Value as JsonValue;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub enum JsonPatch {
    SetString {
        path: Vec<String>,
        value: String,
    },
    SetValue {
        path: Vec<String>,
        value: JsonValue,
    },
    SetArrayObjectString {
        array_path: Vec<String>,
        index: usize,
        object_path: Vec<String>,
        value: String,
    },
    AppendArrayObject {
        path: Vec<String>,
        value: JsonValue,
    },
    Remove {
        path: Vec<String>,
    },
    RemoveString {
        path: Vec<String>,
    },
}

pub fn parse_jsonc_value(text: &str) -> AppResult<JsonValue> {
    let effective = if text.trim().is_empty() { "{}" } else { text };
    jsonc_parser::parse_to_serde_value(effective, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(error.to_string()))
}

pub fn patch_jsonc(text: &str, patches: &[JsonPatch]) -> AppResult<String> {
    let effective = if text.trim().is_empty() { "{}\n" } else { text };
    let mut shape = parse_jsonc_value(effective)?;
    for patch in patches {
        validate_and_apply_json_shape(&mut shape, patch)?;
    }
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
            JsonPatch::SetArrayObjectString {
                array_path,
                index,
                object_path,
                value,
            } => set_array_object_string(&root_object, array_path, *index, object_path, value)?,
            JsonPatch::AppendArrayObject { path, value } => {
                append_array_object(&root_object, path, value)?
            }
            JsonPatch::Remove { path } | JsonPatch::RemoveString { path } => {
                remove_json_path(&root_object, path)?
            }
        }
    }
    let output = root.to_string();
    CstRootNode::parse(&output, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    Ok(output)
}

fn validate_and_apply_json_shape(root: &mut JsonValue, patch: &JsonPatch) -> AppResult<()> {
    match patch {
        JsonPatch::SetArrayObjectString {
            array_path,
            index,
            object_path,
            value,
        } => {
            let array = value_at_path_mut(root, array_path)?
                .as_array_mut()
                .ok_or_else(|| unsupported_path(array_path, "must be an array"))?;
            let object = array
                .get_mut(*index)
                .and_then(JsonValue::as_object_mut)
                .ok_or_else(|| {
                    unsupported_path(array_path, "has no object at the planned index")
                })?;
            set_object_string_shape(object, object_path, value)?;
            return Ok(());
        }
        JsonPatch::AppendArrayObject { path, value } => {
            if !value.is_object() {
                return Err(AppError::Validation(
                    "JSON array append value must be an object".into(),
                ));
            }
            let (name, parent_path) = path
                .split_last()
                .ok_or_else(|| AppError::Validation("JSON patch path cannot be empty".into()))?;
            let parent = object_at_json_path_mut(root, parent_path, true)?;
            let array = parent
                .entry(name.clone())
                .or_insert_with(|| JsonValue::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| unsupported_path(path, "must be an array"))?;
            array.push(value.clone());
            return Ok(());
        }
        _ => {}
    }
    let (path, replacement) = match patch {
        JsonPatch::SetString { path, value } => (path, Some(JsonValue::String(value.clone()))),
        JsonPatch::SetValue { path, value } => (path, Some(value.clone())),
        JsonPatch::Remove { path } | JsonPatch::RemoveString { path } => (path, None),
        JsonPatch::SetArrayObjectString { .. } | JsonPatch::AppendArrayObject { .. } => {
            unreachable!("handled above")
        }
    };
    let (name, parent_path) = path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON patch path cannot be empty".into()))?;
    let mut current = root
        .as_object_mut()
        .ok_or_else(|| AppError::Unsupported("JSONC root must be an object".into()))?;
    for segment in parent_path {
        if !current.contains_key(segment) {
            current.insert(segment.clone(), JsonValue::Object(serde_json::Map::new()));
        }
        current = current
            .get_mut(segment)
            .and_then(JsonValue::as_object_mut)
            .ok_or_else(|| {
                AppError::Unsupported(format!("JSONC field {segment} must be an object"))
            })?;
    }
    match replacement {
        Some(replacement) => {
            if let Some(existing) = current.get(name)
                && !same_json_kind(existing, &replacement)
            {
                return Err(AppError::Unsupported(format!(
                    "JSONC field {} has an incompatible type",
                    path.join(".")
                )));
            }
            current.insert(name.clone(), replacement);
        }
        None => {
            if matches!(patch, JsonPatch::RemoveString { .. })
                && current.get(name).is_some_and(|value| !value.is_string())
            {
                return Err(AppError::Unsupported(format!(
                    "JSONC field {} must be a string",
                    path.join(".")
                )));
            }
            current.remove(name);
        }
    }
    Ok(())
}

fn unsupported_path(path: &[String], message: &str) -> AppError {
    AppError::Unsupported(format!("JSONC field {} {message}", path.join(".")))
}

fn value_at_path_mut<'a>(root: &'a mut JsonValue, path: &[String]) -> AppResult<&'a mut JsonValue> {
    let mut current = root;
    for segment in path {
        current = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .ok_or_else(|| unsupported_path(path, "is missing or has an incompatible type"))?;
    }
    Ok(current)
}

fn object_at_json_path_mut<'a>(
    root: &'a mut JsonValue,
    path: &[String],
    create: bool,
) -> AppResult<&'a mut serde_json::Map<String, JsonValue>> {
    let mut current = root
        .as_object_mut()
        .ok_or_else(|| AppError::Unsupported("JSONC root must be an object".into()))?;
    for segment in path {
        if create && !current.contains_key(segment) {
            current.insert(segment.clone(), JsonValue::Object(serde_json::Map::new()));
        }
        current = current
            .get_mut(segment)
            .and_then(JsonValue::as_object_mut)
            .ok_or_else(|| unsupported_path(path, "must be an object"))?;
    }
    Ok(current)
}

fn set_object_string_shape(
    root: &mut serde_json::Map<String, JsonValue>,
    path: &[String],
    value: &str,
) -> AppResult<()> {
    let (name, parent_path) = path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON object patch path cannot be empty".into()))?;
    let mut current = root;
    for segment in parent_path {
        current = current
            .get_mut(segment)
            .and_then(JsonValue::as_object_mut)
            .ok_or_else(|| unsupported_path(path, "must traverse objects"))?;
    }
    if current
        .get(name)
        .is_some_and(|existing| !existing.is_string())
    {
        return Err(unsupported_path(path, "must be a string"));
    }
    current.insert(name.clone(), JsonValue::String(value.into()));
    Ok(())
}

fn same_json_kind(left: &JsonValue, right: &JsonValue) -> bool {
    matches!(
        (left, right),
        (JsonValue::Null, JsonValue::Null)
            | (JsonValue::Bool(_), JsonValue::Bool(_))
            | (JsonValue::Number(_), JsonValue::Number(_))
            | (JsonValue::String(_), JsonValue::String(_))
            | (JsonValue::Array(_), JsonValue::Array(_))
            | (JsonValue::Object(_), JsonValue::Object(_))
    )
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
        Some(property) => {
            if let CstInputValue::String(new_value) = &value
                && property
                    .value()
                    .and_then(|value| value.as_string_lit())
                    .and_then(|value| value.decoded_value().ok())
                    .as_deref()
                    == Some(new_value)
            {
                return Ok(());
            }
            property.set_value(value);
        }
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

fn set_array_object_string(
    root: &jsonc_parser::cst::CstObject,
    array_path: &[String],
    index: usize,
    object_path: &[String],
    value: &str,
) -> AppResult<()> {
    let (name, parent_path) = array_path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON array path cannot be empty".into()))?;
    let parent = object_at_path(root, parent_path, false)?
        .ok_or_else(|| unsupported_path(array_path, "is missing"))?;
    let array = parent
        .array_value(name)
        .ok_or_else(|| unsupported_path(array_path, "must be an array"))?;
    let object = array
        .elements()
        .get(index)
        .and_then(|element| element.as_object())
        .ok_or_else(|| unsupported_path(array_path, "has no object at the planned index"))?;
    set_json_path(&object, object_path, CstInputValue::String(value.into()))
}

fn append_array_object(
    root: &jsonc_parser::cst::CstObject,
    path: &[String],
    value: &JsonValue,
) -> AppResult<()> {
    let (name, parent_path) = path
        .split_last()
        .ok_or_else(|| AppError::Validation("JSON array path cannot be empty".into()))?;
    let parent = object_at_path(root, parent_path, true)?.expect("created parent object");
    let array = parent
        .array_value_or_create(name)
        .ok_or_else(|| unsupported_path(path, "must be an array"))?;
    array.append(json_to_cst(value));
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
    patch_codex_api_toml_from_template(
        text,
        &CodexApiTemplatePatch {
            provider_id,
            provider_name,
            base_url,
            api_key,
            model,
            model_reasoning_effort: "high",
            model_catalog_json: None,
            preferred_auth_method: None,
            forced_login_method: None,
        },
    )
}

pub struct CodexApiTemplatePatch<'a> {
    pub provider_id: &'a str,
    pub provider_name: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub model_reasoning_effort: &'a str,
    pub model_catalog_json: Option<&'a str>,
    pub preferred_auth_method: Option<&'a str>,
    pub forced_login_method: Option<&'a str>,
}

pub fn patch_codex_api_toml_from_template(
    text: &str,
    patch: &CodexApiTemplatePatch<'_>,
) -> AppResult<String> {
    let mut document = parse_toml(text)?;
    set_document_string(&mut document, "model", patch.model)?;
    set_document_string(&mut document, "model_provider", patch.provider_id)?;
    set_document_string(
        &mut document,
        "model_reasoning_effort",
        patch.model_reasoning_effort,
    )?;
    set_optional_document_string(
        &mut document,
        "model_catalog_json",
        patch.model_catalog_json,
    )?;
    set_optional_document_string(
        &mut document,
        "preferred_auth_method",
        patch.preferred_auth_method,
    )?;
    set_optional_document_string(
        &mut document,
        "forced_login_method",
        patch.forced_login_method,
    )?;
    let providers = ensure_table(&mut document, "model_providers")?;
    let provider = ensure_child_table(providers, patch.provider_id)?;
    set_table_string(provider, "name", patch.provider_name)?;
    set_table_string(provider, "base_url", patch.base_url)?;
    set_table_string(provider, "wire_api", "responses")?;
    set_table_string(provider, "experimental_bearer_token", patch.api_key)?;
    remove_table_string(provider, "env_key")?;
    if provider
        .get("requires_openai_auth")
        .is_some_and(|item| item.as_bool().is_none())
    {
        return Err(AppError::Unsupported(
            "TOML field requires_openai_auth must be a boolean".into(),
        ));
    }
    provider.remove("requires_openai_auth");
    provider.remove("auth");
    let output = document.to_string();
    parse_toml(&output)?;
    Ok(output)
}

pub fn patch_codex_oauth_toml(text: &str, model: &str) -> AppResult<String> {
    let mut document = parse_toml(text)?;
    set_document_string(&mut document, "model", model)?;
    set_document_string(&mut document, "model_provider", "openai")?;
    set_document_string(&mut document, "cli_auth_credentials_store", "file")?;
    for key in [
        "model_catalog_json",
        "model_reasoning_effort",
        "preferred_auth_method",
        "forced_login_method",
    ] {
        remove_document_string(&mut document, key)?;
    }
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

fn set_document_string(document: &mut DocumentMut, key: &str, new_value: &str) -> AppResult<()> {
    if let Some(existing) = document.get(key)
        && existing.as_str().is_none()
    {
        return Err(AppError::Unsupported(format!(
            "TOML field {key} must be a string"
        )));
    }
    document[key] = value(new_value);
    Ok(())
}

fn set_optional_document_string(
    document: &mut DocumentMut,
    key: &str,
    new_value: Option<&str>,
) -> AppResult<()> {
    if let Some(new_value) = new_value {
        set_document_string(document, key, new_value)
    } else {
        remove_document_string(document, key)
    }
}

fn set_table_string(table: &mut Table, key: &str, new_value: &str) -> AppResult<()> {
    if let Some(existing) = table.get(key)
        && existing.as_str().is_none()
    {
        return Err(AppError::Unsupported(format!(
            "TOML field {key} must be a string"
        )));
    }
    table[key] = value(new_value);
    Ok(())
}

fn remove_document_string(document: &mut DocumentMut, key: &str) -> AppResult<()> {
    if document
        .get(key)
        .is_some_and(|existing| existing.as_str().is_none())
    {
        return Err(AppError::Unsupported(format!(
            "TOML field {key} must be a string"
        )));
    }
    document.remove(key);
    Ok(())
}

fn remove_table_string(table: &mut Table, key: &str) -> AppResult<()> {
    if table
        .get(key)
        .is_some_and(|existing| existing.as_str().is_none())
    {
        return Err(AppError::Unsupported(format!(
            "TOML field {key} must be a string"
        )));
    }
    table.remove(key);
    Ok(())
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
    fn jsonc_patch_refuses_an_incompatible_managed_leaf_type() {
        let error = patch_jsonc(
            r#"{ "model": false }"#,
            &[JsonPatch::SetString {
                path: vec!["model".into()],
                value: "new-model".into(),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Unsupported(_)));
    }

    #[test]
    fn jsonc_patch_treats_slashes_dots_and_quotes_as_one_path_segment() {
        let model = "org/model.with.\"quote\"";
        let output = patch_jsonc(
            "{}\n",
            &[JsonPatch::SetString {
                path: vec!["models".into(), model.into(), "name".into()],
                value: "雪\\model".into(),
            }],
        )
        .unwrap();
        let value = parse_jsonc_value(&output).unwrap();
        assert_eq!(value["models"][model]["name"], "雪\\model");
    }

    #[test]
    fn jsonc_array_patches_preserve_comments_unknown_fields_and_crlf() {
        let source = "{\r\n  \"groups\": {\r\n    \"a/b\": [\r\n      {\r\n        // keep model metadata\r\n        \"id\": \"old\",\r\n        \"name\": \"Old\",\r\n        \"generationConfig\": { \"temperature\": 0.2 },\r\n      },\r\n      { \"id\": \"other\", \"name\": \"Other\" },\r\n    ],\r\n  },\r\n}\r\n";
        let output = patch_jsonc(
            source,
            &[
                JsonPatch::SetArrayObjectString {
                    array_path: vec!["groups".into(), "a/b".into()],
                    index: 0,
                    object_path: vec!["id".into()],
                    value: "new/model".into(),
                },
                JsonPatch::AppendArrayObject {
                    path: vec!["groups".into(), "a/b".into()],
                    value: serde_json::json!({ "id": "third", "name": "Third" }),
                },
            ],
        )
        .unwrap();
        assert!(output.contains("// keep model metadata\r\n"));
        assert!(output.contains("\"generationConfig\": { \"temperature\": 0.2 }"));
        assert!(output.contains("{ \"id\": \"other\", \"name\": \"Other\" }"));
        assert!(output.contains("\r\n"));
        let value = parse_jsonc_value(&output).unwrap();
        assert_eq!(value["groups"]["a/b"][0]["id"], "new/model");
        assert_eq!(value["groups"]["a/b"][2]["id"], "third");
    }

    #[test]
    fn jsonc_array_patches_validate_every_operation_before_rendering() {
        let error = patch_jsonc(
            r#"{ "groups": { "a": [{ "id": false }] } }"#,
            &[JsonPatch::SetArrayObjectString {
                array_path: vec!["groups".into(), "a".into()],
                index: 0,
                object_path: vec!["id".into()],
                value: "model".into(),
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

    #[test]
    fn toml_patch_round_trips_special_characters_in_keys_and_values() {
        let provider_id = r#"provider /.\"quoted\"\ 雪"#;
        let provider_name = r#"Provider \"Snow 雪\"\ name"#;
        let model = r#"org/model.\"snow 雪\"\ variant"#;
        let output = patch_codex_api_toml_from_template(
            "",
            &CodexApiTemplatePatch {
                provider_id,
                provider_name,
                base_url: "https://example.test/a path",
                api_key: r#"key \"quoted\"\ 雪"#,
                model,
                model_reasoning_effort: "high",
                model_catalog_json: Some(r#"/tmp/catalog \"snow 雪\"\ models.json"#),
                preferred_auth_method: None,
                forced_login_method: None,
            },
        )
        .unwrap();

        let document = parse_toml(&output).unwrap();
        assert_eq!(document.get("model").and_then(Item::as_str), Some(model));
        let provider = document
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get(provider_id))
            .and_then(Item::as_table)
            .unwrap();
        assert_eq!(
            provider.get("name").and_then(Item::as_str),
            Some(provider_name)
        );
        assert_eq!(
            provider
                .get("experimental_bearer_token")
                .and_then(Item::as_str),
            Some(r#"key \"quoted\"\ 雪"#)
        );
        assert_eq!(
            document.get("model_catalog_json").and_then(Item::as_str),
            Some(r#"/tmp/catalog \"snow 雪\"\ models.json"#)
        );
    }

    #[test]
    fn toml_patch_refuses_an_incompatible_managed_leaf_type() {
        let error = patch_codex_api_toml(
            "model = false\n",
            "cliswitch_123",
            "Example",
            "https://example.test/v1",
            "secret",
            "gpt-test",
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Unsupported(_)));
    }
}
