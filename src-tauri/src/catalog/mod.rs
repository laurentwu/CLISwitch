use std::collections::HashSet;

use jsonc_parser::{ParseOptions, parse_to_serde_value};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use url::Url;

use crate::{
    domain::{CliId, CliProtocol, ConnectionAuthType, OAuthKind},
    error::{AppError, AppResult},
};

const CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliCatalogFile {
    schema_version: u32,
    clis: Vec<CatalogCli>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTemplateCatalogFile {
    schema_version: u32,
    provider_templates: Vec<ProviderTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelationCatalogFile {
    schema_version: u32,
    relations: Vec<CliProviderRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalog {
    pub schema_version: u32,
    pub clis: Vec<CatalogCli>,
    pub provider_templates: Vec<ProviderTemplate>,
    pub relations: Vec<CliProviderRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCli {
    pub id: CliId,
    pub name: String,
    pub protocols: Vec<CliProtocol>,
    pub auth_modes: Vec<CatalogAuthMode>,
    pub protocol_adapters: Vec<CliProtocolAdapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAuthMode {
    pub id: String,
    pub oauth_kind: OAuthKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliProtocolAdapter {
    pub protocol: CliProtocol,
    pub provider_package: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum ProviderTemplate {
    Api(ApiProviderTemplate),
    Auth(AuthProviderTemplate),
}

impl ProviderTemplate {
    pub fn id(&self) -> &str {
        match self {
            Self::Api(template) => &template.id,
            Self::Auth(template) => &template.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Api(template) => &template.name,
            Self::Auth(template) => &template.name,
        }
    }

    pub const fn mode(&self) -> &'static str {
        match self {
            Self::Api(_) => "api",
            Self::Auth(_) => "auth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProviderTemplate {
    pub id: String,
    pub name: String,
    pub category: String,
    pub credential_slots: Vec<CredentialSlotTemplate>,
    pub endpoints: Vec<ProviderEndpointTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProviderTemplate {
    pub id: String,
    pub name: String,
    pub auth_kind: OAuthKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSlotTemplate {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEndpointTemplate {
    pub id: String,
    pub name: String,
    pub protocol: CliProtocol,
    pub base_url: Url,
    pub credential_slot_id: String,
    pub auth_options: Vec<EndpointAuthOption>,
    pub default_auth_option_id: String,
    pub models: Vec<ProviderModelTemplate>,
}

impl ProviderEndpointTemplate {
    pub fn default_auth_type(&self) -> Option<ConnectionAuthType> {
        self.auth_options
            .iter()
            .find(|option| option.id == self.default_auth_option_id)
            .map(|option| option.auth_type)
    }

    pub fn default_model(&self) -> Option<&str> {
        self.models
            .iter()
            .find(|model| model.default)
            .or_else(|| self.models.first())
            .map(|model| model.id.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointAuthOption {
    pub id: String,
    pub auth_type: ConnectionAuthType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub default: bool,
    pub context: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum CliProviderRelation {
    Api(ApiCliProviderRelation),
    Auth(AuthCliProviderRelation),
}

impl CliProviderRelation {
    pub fn id(&self) -> &str {
        match self {
            Self::Api(relation) => &relation.id,
            Self::Auth(relation) => &relation.id,
        }
    }

    pub const fn cli_id(&self) -> CliId {
        match self {
            Self::Api(relation) => relation.cli_id,
            Self::Auth(relation) => relation.cli_id,
        }
    }

    pub fn provider_template_id(&self) -> &str {
        match self {
            Self::Api(relation) => &relation.provider_template_id,
            Self::Auth(relation) => &relation.provider_template_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCliProviderRelation {
    pub id: String,
    pub cli_id: CliId,
    pub provider_template_id: String,
    pub endpoint_id: String,
    pub auth_option_id: String,
    #[serde(default)]
    pub base_url: Option<Url>,
    #[serde(default)]
    pub provider_package: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub native_provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCliProviderRelation {
    pub id: String,
    pub cli_id: CliId,
    pub provider_template_id: String,
    pub auth_mode_id: String,
}

impl ProviderCatalog {
    pub fn load_embedded() -> AppResult<Self> {
        let cli_file: CliCatalogFile =
            parse_catalog_file("clis.jsonc", include_str!("../../catalog/clis.jsonc"))?;
        let template_file: ProviderTemplateCatalogFile = parse_catalog_file(
            "provider-templates.jsonc",
            include_str!("../../catalog/provider-templates.jsonc"),
        )?;
        let relation_file: RelationCatalogFile = parse_catalog_file(
            "cli-provider-relations.jsonc",
            include_str!("../../catalog/cli-provider-relations.jsonc"),
        )?;
        for (name, version) in [
            ("clis.jsonc", cli_file.schema_version),
            ("provider-templates.jsonc", template_file.schema_version),
            ("cli-provider-relations.jsonc", relation_file.schema_version),
        ] {
            if version != CATALOG_SCHEMA_VERSION {
                return Err(AppError::Serialization(format!(
                    "unsupported {name} schema version {version}"
                )));
            }
        }
        let catalog = Self {
            schema_version: CATALOG_SCHEMA_VERSION,
            clis: cli_file.clis,
            provider_templates: template_file.provider_templates,
            relations: relation_file.relations,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn cli(&self, cli_id: CliId) -> Option<&CatalogCli> {
        self.clis.iter().find(|cli| cli.id == cli_id)
    }

    pub fn template(&self, template_id: &str) -> Option<&ProviderTemplate> {
        self.provider_templates
            .iter()
            .find(|template| template.id() == template_id)
    }

    pub fn api_template(&self, template_id: &str) -> Option<&ApiProviderTemplate> {
        match self.template(template_id) {
            Some(ProviderTemplate::Api(template)) => Some(template),
            _ => None,
        }
    }

    pub fn auth_template(&self, template_id: &str) -> Option<&AuthProviderTemplate> {
        match self.template(template_id) {
            Some(ProviderTemplate::Auth(template)) => Some(template),
            _ => None,
        }
    }

    pub fn auth_template_for_kind(&self, kind: OAuthKind) -> Option<&AuthProviderTemplate> {
        self.provider_templates
            .iter()
            .find_map(|template| match template {
                ProviderTemplate::Auth(template) if template.auth_kind == kind => Some(template),
                _ => None,
            })
    }

    pub fn api_relations(
        &self,
        cli_id: CliId,
        template_id: &str,
    ) -> impl Iterator<Item = &ApiCliProviderRelation> {
        self.relations
            .iter()
            .filter_map(move |relation| match relation {
                CliProviderRelation::Api(relation)
                    if relation.cli_id == cli_id
                        && relation.provider_template_id == template_id =>
                {
                    Some(relation)
                }
                _ => None,
            })
    }

    pub fn api_relation(
        &self,
        cli_id: CliId,
        template_id: &str,
        endpoint_id: &str,
    ) -> Option<&ApiCliProviderRelation> {
        self.api_relations(cli_id, template_id)
            .find(|relation| relation.endpoint_id == endpoint_id)
    }

    pub fn relation_auth_type(
        &self,
        relation: &ApiCliProviderRelation,
    ) -> Option<ConnectionAuthType> {
        self.api_template(&relation.provider_template_id)?
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == relation.endpoint_id)?
            .auth_options
            .iter()
            .find(|option| option.id == relation.auth_option_id)
            .map(|option| option.auth_type)
    }

    pub fn supports_api_endpoint(
        &self,
        cli_id: CliId,
        template_id: &str,
        endpoint_id: &str,
    ) -> bool {
        self.api_relations(cli_id, template_id)
            .any(|relation| relation.endpoint_id == endpoint_id)
    }

    pub fn supports_auth_template(&self, cli_id: CliId, template_id: &str) -> bool {
        self.relations.iter().any(|relation| {
            matches!(
                relation,
                CliProviderRelation::Auth(relation)
                    if relation.cli_id == cli_id
                        && relation.provider_template_id == template_id
            )
        })
    }

    pub fn native_api_relation(
        &self,
        cli_id: CliId,
        native_provider_id: &str,
    ) -> Option<&ApiCliProviderRelation> {
        self.relations.iter().find_map(|relation| match relation {
            CliProviderRelation::Api(relation)
                if relation.cli_id == cli_id
                    && relation
                        .native_provider_ids
                        .iter()
                        .any(|id| id == native_provider_id) =>
            {
                Some(relation)
            }
            _ => None,
        })
    }

    pub fn protocol_package(&self, cli_id: CliId, protocol: CliProtocol) -> Option<&str> {
        self.cli(cli_id)?
            .protocol_adapters
            .iter()
            .find(|adapter| adapter.protocol == protocol)
            .map(|adapter| adapter.provider_package.as_str())
    }

    pub fn package_protocol(&self, cli_id: CliId, provider_package: &str) -> Option<CliProtocol> {
        self.cli(cli_id)?
            .protocol_adapters
            .iter()
            .find(|adapter| adapter.provider_package == provider_package)
            .map(|adapter| adapter.protocol)
    }

    pub fn supports_protocol(&self, cli_id: CliId, protocol: CliProtocol) -> bool {
        self.cli(cli_id)
            .is_some_and(|cli| cli.protocols.contains(&protocol))
    }

    fn validate(&self) -> AppResult<()> {
        let mut cli_ids = HashSet::new();
        for cli in &self.clis {
            if !cli_ids.insert(cli.id) {
                return invalid(format!("duplicate CLI {}", cli.id));
            }
            ensure_nonempty("CLI name", &cli.name)?;
            ensure_unique_strings(
                "CLI auth mode",
                cli.auth_modes.iter().map(|mode| mode.id.as_str()),
            )?;
            let mut protocols = HashSet::new();
            for protocol in &cli.protocols {
                if !protocols.insert(*protocol) {
                    return invalid(format!("CLI {} repeats protocol {protocol}", cli.id));
                }
            }
            let mut adapters = HashSet::new();
            for adapter in &cli.protocol_adapters {
                if !protocols.contains(&adapter.protocol) {
                    return invalid(format!(
                        "CLI {} has an adapter for unsupported protocol {}",
                        cli.id, adapter.protocol
                    ));
                }
                if !adapters.insert(adapter.protocol) {
                    return invalid(format!(
                        "CLI {} repeats the adapter for {}",
                        cli.id, adapter.protocol
                    ));
                }
                ensure_nonempty("provider package", &adapter.provider_package)?;
            }
        }
        for required in CliId::ALL {
            if !cli_ids.contains(&required) {
                return invalid(format!("catalog omits CLI {required}"));
            }
        }

        let mut template_ids = HashSet::new();
        for template in &self.provider_templates {
            ensure_identifier("provider template", template.id())?;
            ensure_nonempty("provider template name", template.name())?;
            if !template_ids.insert(template.id()) {
                return invalid(format!("duplicate provider template {}", template.id()));
            }
            match template {
                ProviderTemplate::Api(template) => validate_api_template(template)?,
                ProviderTemplate::Auth(template) => {
                    if self
                        .provider_templates
                        .iter()
                        .filter_map(|candidate| match candidate {
                            ProviderTemplate::Auth(candidate)
                                if candidate.auth_kind == template.auth_kind =>
                            {
                                Some(candidate)
                            }
                            _ => None,
                        })
                        .count()
                        != 1
                    {
                        return invalid(format!(
                            "auth kind {} must have exactly one template",
                            template.auth_kind
                        ));
                    }
                }
            }
        }

        let mut relation_ids = HashSet::new();
        let mut native_ids = HashSet::new();
        let mut api_routes = HashSet::new();
        let mut default_api_routes = HashSet::new();
        let mut auth_routes = HashSet::new();
        for relation in &self.relations {
            ensure_identifier("CLI provider relation", relation.id())?;
            if !relation_ids.insert(relation.id()) {
                return invalid(format!("duplicate relation {}", relation.id()));
            }
            let cli = self.cli(relation.cli_id()).ok_or_else(|| {
                AppError::Serialization(format!(
                    "relation {} references unknown CLI {}",
                    relation.id(),
                    relation.cli_id()
                ))
            })?;
            match relation {
                CliProviderRelation::Api(relation) => {
                    if !api_routes.insert((
                        relation.cli_id,
                        relation.provider_template_id.as_str(),
                        relation.endpoint_id.as_str(),
                    )) {
                        return invalid(format!(
                            "duplicate CLI/template/endpoint relation {}",
                            relation.id
                        ));
                    }
                    if relation.default
                        && !default_api_routes
                            .insert((relation.cli_id, relation.provider_template_id.as_str()))
                    {
                        return invalid(format!(
                            "CLI {} template {} has multiple default endpoints",
                            relation.cli_id, relation.provider_template_id
                        ));
                    }
                    let template = self
                        .api_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing API template",
                                relation.id
                            ))
                        })?;
                    let endpoint = template
                        .endpoints
                        .iter()
                        .find(|endpoint| endpoint.id == relation.endpoint_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing endpoint",
                                relation.id
                            ))
                        })?;
                    if !cli.protocols.contains(&endpoint.protocol) {
                        return invalid(format!(
                            "relation {} connects {} to unsupported protocol {}",
                            relation.id, cli.id, endpoint.protocol
                        ));
                    }
                    if !endpoint
                        .auth_options
                        .iter()
                        .any(|option| option.id == relation.auth_option_id)
                    {
                        return invalid(format!(
                            "relation {} references a missing auth option",
                            relation.id
                        ));
                    }
                    if relation.base_url.as_ref().is_some_and(|base_url| {
                        !matches!(base_url.scheme(), "http" | "https")
                            || base_url.host_str().is_none()
                            || !base_url.username().is_empty()
                            || base_url.password().is_some()
                            || base_url.query().is_some()
                            || base_url.fragment().is_some()
                    }) {
                        return invalid(format!(
                            "relation {} has an invalid base URL override",
                            relation.id
                        ));
                    }
                    if let Some(provider_package) = relation.provider_package.as_deref() {
                        ensure_nonempty("relation provider package", provider_package)?;
                        if let Some(protocol) = self.package_protocol(cli.id, provider_package)
                            && protocol != endpoint.protocol
                        {
                            return invalid(format!(
                                "relation {} provider package does not match endpoint protocol",
                                relation.id
                            ));
                        }
                    }
                    for native_id in &relation.native_provider_ids {
                        ensure_nonempty("native provider ID", native_id)?;
                        if !native_ids.insert((relation.cli_id, native_id.as_str())) {
                            return invalid(format!(
                                "CLI {} repeats native provider ID {native_id}",
                                relation.cli_id
                            ));
                        }
                    }
                }
                CliProviderRelation::Auth(relation) => {
                    if !auth_routes
                        .insert((relation.cli_id, relation.provider_template_id.as_str()))
                    {
                        return invalid(format!(
                            "duplicate CLI/auth-template relation {}",
                            relation.id
                        ));
                    }
                    let template = self
                        .auth_template(&relation.provider_template_id)
                        .ok_or_else(|| {
                            AppError::Serialization(format!(
                                "relation {} references a missing auth template",
                                relation.id
                            ))
                        })?;
                    if !cli.auth_modes.iter().any(|mode| {
                        mode.id == relation.auth_mode_id && mode.oauth_kind == template.auth_kind
                    }) {
                        return invalid(format!(
                            "relation {} references an incompatible CLI auth mode",
                            relation.id
                        ));
                    }
                }
            }
        }
        for template in &self.provider_templates {
            if !self
                .relations
                .iter()
                .any(|relation| relation.provider_template_id() == template.id())
            {
                return invalid(format!(
                    "provider template {} has no CLI relation",
                    template.id()
                ));
            }
        }
        Ok(())
    }
}

fn validate_api_template(template: &ApiProviderTemplate) -> AppResult<()> {
    ensure_nonempty("provider category", &template.category)?;
    if template.credential_slots.is_empty() {
        return invalid(format!(
            "API template {} has no credential slots",
            template.id
        ));
    }
    if template.endpoints.is_empty() {
        return invalid(format!("API template {} has no endpoints", template.id));
    }
    ensure_unique_strings(
        "credential slot",
        template
            .credential_slots
            .iter()
            .map(|slot| slot.id.as_str()),
    )?;
    for slot in &template.credential_slots {
        ensure_nonempty("credential slot name", &slot.name)?;
    }
    let slot_ids = template
        .credential_slots
        .iter()
        .map(|slot| slot.id.as_str())
        .collect::<HashSet<_>>();
    let mut endpoint_ids = HashSet::new();
    for endpoint in &template.endpoints {
        ensure_identifier("endpoint", &endpoint.id)?;
        ensure_nonempty("endpoint name", &endpoint.name)?;
        if !endpoint_ids.insert(endpoint.id.as_str()) {
            return invalid(format!(
                "template {} repeats endpoint {}",
                template.id, endpoint.id
            ));
        }
        if !matches!(endpoint.base_url.scheme(), "http" | "https")
            || endpoint.base_url.host_str().is_none()
            || !endpoint.base_url.username().is_empty()
            || endpoint.base_url.password().is_some()
        {
            return invalid(format!(
                "template {} endpoint {} has an invalid URL",
                template.id, endpoint.id
            ));
        }
        if !slot_ids.contains(endpoint.credential_slot_id.as_str()) {
            return invalid(format!(
                "template {} endpoint {} references a missing credential slot",
                template.id, endpoint.id
            ));
        }
        if endpoint.auth_options.is_empty() || endpoint.default_auth_type().is_none() {
            return invalid(format!(
                "template {} endpoint {} has no valid default auth option",
                template.id, endpoint.id
            ));
        }
        ensure_unique_strings(
            "endpoint auth option",
            endpoint
                .auth_options
                .iter()
                .map(|option| option.id.as_str()),
        )?;
        ensure_unique_nonempty_strings(
            "endpoint model",
            endpoint.models.iter().map(|model| model.id.as_str()),
        )?;
        if endpoint.models.iter().filter(|model| model.default).count() > 1 {
            return invalid(format!(
                "template {} endpoint {} has multiple default models",
                template.id, endpoint.id
            ));
        }
        for model in &endpoint.models {
            ensure_nonempty("model ID", &model.id)?;
            ensure_nonempty("model name", &model.name)?;
            if model.context == Some(0) || model.output == Some(0) {
                return invalid(format!(
                    "template {} endpoint {} has a zero model limit",
                    template.id, endpoint.id
                ));
            }
        }
    }
    Ok(())
}

fn parse_catalog_file<T: DeserializeOwned>(name: &str, text: &str) -> AppResult<T> {
    let value = parse_to_serde_value(text, &ParseOptions::default())
        .map_err(|error| AppError::Serialization(format!("invalid {name}: {error}")))?;
    serde_json::from_value(value)
        .map_err(|error| AppError::Serialization(format!("invalid {name}: {error}")))
}

fn ensure_unique_strings<'a>(kind: &str, values: impl Iterator<Item = &'a str>) -> AppResult<()> {
    let mut seen = HashSet::new();
    for value in values {
        ensure_identifier(kind, value)?;
        if !seen.insert(value) {
            return invalid(format!("duplicate {kind} {value}"));
        }
    }
    Ok(())
}

fn ensure_unique_nonempty_strings<'a>(
    kind: &str,
    values: impl Iterator<Item = &'a str>,
) -> AppResult<()> {
    let mut seen = HashSet::new();
    for value in values {
        ensure_nonempty(kind, value)?;
        if !seen.insert(value) {
            return invalid(format!("duplicate {kind} {value}"));
        }
    }
    Ok(())
}

fn ensure_identifier(kind: &str, value: &str) -> AppResult<()> {
    ensure_nonempty(kind, value)?;
    if !value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) {
        return invalid(format!("{kind} {value} is not a kebab-case identifier"));
    }
    Ok(())
}

fn ensure_nonempty(kind: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return invalid(format!("{kind} must not be empty"));
    }
    Ok(())
}

fn invalid<T>(message: String) -> AppResult<T> {
    Err(AppError::Serialization(message))
}

static EMBEDDED_CATALOG: Lazy<Result<ProviderCatalog, String>> =
    Lazy::new(|| ProviderCatalog::load_embedded().map_err(|error| error.to_string()));

pub fn embedded_catalog() -> AppResult<&'static ProviderCatalog> {
    EMBEDDED_CATALOG
        .as_ref()
        .map_err(|error| AppError::Serialization(error.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_well_formed() {
        ProviderCatalog::load_embedded().unwrap();
    }

    #[test]
    fn catalog_serializes_as_frontend_discriminated_unions() {
        let value = serde_json::to_value(ProviderCatalog::load_embedded().unwrap()).unwrap();
        let template = &value["providerTemplates"][0];
        assert_eq!(template["mode"], "api");
        assert!(template.get("id").is_some());
        let auth_relation = value["relations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|relation| relation["mode"] == "auth")
            .unwrap();
        assert!(auth_relation.get("authModeId").is_some());
    }

    #[test]
    fn glm_coding_plan_has_three_protocol_endpoints_and_one_shared_slot() {
        let catalog = ProviderCatalog::load_embedded().unwrap();
        let template = catalog.api_template("glm-coding-plan").unwrap();
        assert_eq!(template.endpoints.len(), 3);
        assert_eq!(template.credential_slots.len(), 1);
        assert!(
            template
                .endpoints
                .iter()
                .all(|endpoint| endpoint.credential_slot_id == "api-key")
        );
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::AnthropicMessages
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/anthropic"
        }));
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::OpenaiChat
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/coding/paas/v4"
        }));
        assert!(template.endpoints.iter().any(|endpoint| {
            endpoint.protocol == CliProtocol::OpenaiResponses
                && endpoint.base_url.as_str() == "https://open.bigmodel.cn/api/v1"
        }));
    }

    #[test]
    fn opencode_glm_routes_are_explicit_and_have_no_silent_default() {
        let catalog = ProviderCatalog::load_embedded().unwrap();
        let relations = catalog
            .api_relations(CliId::Opencode, "glm-coding-plan")
            .collect::<Vec<_>>();
        assert_eq!(relations.len(), 3);
        assert!(relations.iter().all(|relation| !relation.default));
        assert_eq!(
            catalog
                .native_api_relation(CliId::Opencode, "zhipuai-coding-plan")
                .map(|relation| relation.endpoint_id.as_str()),
            Some("openai-chat")
        );
    }

    #[test]
    fn opencode_packages_come_from_the_cli_catalog() {
        let catalog = ProviderCatalog::load_embedded().unwrap();
        assert_eq!(
            catalog.protocol_package(CliId::Opencode, CliProtocol::OpenaiChat),
            Some("@ai-sdk/openai-compatible")
        );
        assert_eq!(
            catalog.protocol_package(CliId::Opencode, CliProtocol::OpenaiResponses),
            Some("@ai-sdk/openai")
        );
        assert_eq!(
            catalog
                .native_api_relation(CliId::Opencode, "openrouter")
                .and_then(|relation| relation.provider_package.as_deref()),
            Some("@openrouter/ai-sdk-provider")
        );
    }

    #[test]
    fn claude_minimax_relations_separate_api_and_token_plan_transport() {
        let catalog = ProviderCatalog::load_embedded().unwrap();
        for (template_id, auth_type, base_url) in [
            (
                "minimax-api",
                ConnectionAuthType::ApiKey,
                "https://api.minimax.io/anthropic",
            ),
            (
                "minimax-cn-api",
                ConnectionAuthType::ApiKey,
                "https://api.minimaxi.com/anthropic",
            ),
            (
                "minimax-coding-plan",
                ConnectionAuthType::Bearer,
                "https://api.minimax.io/anthropic",
            ),
            (
                "minimax-cn-coding-plan",
                ConnectionAuthType::Bearer,
                "https://api.minimaxi.com/anthropic",
            ),
        ] {
            let relation = catalog
                .api_relation(CliId::ClaudeCode, template_id, "anthropic")
                .unwrap();
            assert_eq!(catalog.relation_auth_type(relation), Some(auth_type));
            assert_eq!(relation.base_url.as_ref().map(Url::as_str), Some(base_url));
        }

        assert_eq!(
            catalog
                .api_relation(CliId::Opencode, "minimax-coding-plan", "anthropic")
                .and_then(|relation| relation.base_url.as_ref()),
            None
        );
    }

    #[test]
    fn all_previous_opencode_native_provider_ids_are_in_the_relation_catalog() {
        let catalog = ProviderCatalog::load_embedded().unwrap();
        for provider_id in [
            "openai",
            "anthropic",
            "openrouter",
            "zhipuai-coding-plan",
            "zai-coding-plan",
            "minimax-coding-plan",
            "minimax-cn-coding-plan",
            "alibaba-coding-plan",
            "alibaba-coding-plan-cn",
            "tencent-coding-plan",
            "kimi-for-coding",
            "umans-ai-coding-plan",
            "kuae-cloud-coding-plan",
        ] {
            assert!(
                catalog
                    .native_api_relation(CliId::Opencode, provider_id)
                    .is_some(),
                "missing native provider relation for {provider_id}"
            );
        }
    }
}
