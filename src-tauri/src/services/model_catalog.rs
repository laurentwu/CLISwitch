use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    domain::{CliProtocol, ConnectionAuthType, ProviderConnection},
    error::{AppError, AppResult},
};

const MAX_BODY: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

#[derive(Debug, Clone)]
pub struct ModelCatalogService {
    client: reqwest::Client,
}

impl ModelCatalogService {
    pub fn new() -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("CLISwitch/0.1")
            .build()
            .map_err(|error| AppError::Network(error.to_string()))?;
        Ok(Self { client })
    }

    pub async fn list_models(&self, connection: &ProviderConnection) -> AppResult<Vec<String>> {
        let endpoint = models_url(&connection.endpoint, connection.protocol)?;
        let bytes = self.fetch_bounded(endpoint, Some(connection)).await?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let models = value
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| AppError::Network("model response does not contain data[]".into()))?
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if models.is_empty() {
            return Err(AppError::Network(
                "model response contained no model IDs".into(),
            ));
        }
        Ok(models)
    }

    pub async fn test_connection(&self, connection: &ProviderConnection) -> AppResult<()> {
        self.list_models(connection).await.map(|_| ())
    }

    async fn fetch_bounded(
        &self,
        mut url: Url,
        credentials: Option<&ProviderConnection>,
    ) -> AppResult<Vec<u8>> {
        let original_origin = origin(&url);
        for redirect_count in 0..=MAX_REDIRECTS {
            let same_origin = origin(&url) == original_origin;
            let mut request = self
                .client
                .get(url.clone())
                .header(header::ACCEPT, "application/json");
            if same_origin && let Some(connection) = credentials {
                request = match connection.auth_type {
                    ConnectionAuthType::ApiKey => request.header("x-api-key", &connection.api_key),
                    ConnectionAuthType::Bearer => request.header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", connection.api_key),
                    ),
                };
                if connection.protocol == CliProtocol::AnthropicMessages {
                    request = request.header("anthropic-version", "2023-06-01");
                }
            }
            let response = request
                .send()
                .await
                .map_err(|error| AppError::Network(error.to_string()))?;
            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(AppError::Network("too many redirects".into()));
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| AppError::Network("redirect omitted Location".into()))?;
                url = url
                    .join(location)
                    .map_err(|error| AppError::Network(error.to_string()))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(AppError::Network("redirect uses a forbidden scheme".into()));
                }
                continue;
            }
            if !response.status().is_success() {
                return Err(AppError::Network(format!(
                    "model endpoint returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_BODY as u64)
            {
                return Err(AppError::Network("model response is too large".into()));
            }
            return bounded_response(response).await;
        }
        Err(AppError::Network("redirect loop".into()))
    }
}

fn models_url(base: &Url, protocol: CliProtocol) -> AppResult<Url> {
    let mut url = base.clone();
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::Validation(
            "model endpoint must use HTTP(S)".into(),
        ));
    }
    let current = url.path().trim_end_matches('/');
    let path = match protocol {
        CliProtocol::AnthropicMessages => {
            if current.ends_with("/v1") {
                format!("{current}/models")
            } else {
                format!("{current}/v1/models")
            }
        }
        CliProtocol::OpenaiChat | CliProtocol::OpenaiResponses => {
            format!("{current}/models")
        }
    };
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

async fn bounded_response(response: reqwest::Response) -> AppResult<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| AppError::Network(error.to_string()))?;
        if output.len() + chunk.len() > MAX_BODY {
            return Err(AppError::Network("model response is too large".into()));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn origin(url: &Url) -> (String, Option<String>, Option<u16>) {
    (
        url.scheme().to_string(),
        url.host_str().map(str::to_ascii_lowercase),
        url.port_or_known_default(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: Url,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Url,
}

pub async fn check_github_release(current_version: &str) -> AppResult<ReleaseCheck> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("CLISwitch/0.1")
        .build()
        .map_err(|error| AppError::Network(error.to_string()))?
        .get("https://api.github.com/repos/laurentwu/CLISwitch/releases/latest")
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(AppError::NotFound(
            "no GitHub release is published yet".into(),
        ));
    }
    let response = response
        .error_for_status()
        .map_err(|error| AppError::Network(error.to_string()))?;
    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    if release.html_url.scheme() != "https" || release.html_url.host_str() != Some("github.com") {
        return Err(AppError::Network(
            "GitHub returned an unexpected release URL".into(),
        ));
    }
    let latest = release.tag_name.trim_start_matches('v');
    let current = semver::Version::parse(current_version)
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let latest_version =
        semver::Version::parse(latest).map_err(|error| AppError::Network(error.to_string()))?;
    Ok(ReleaseCheck {
        current_version: current.to_string(),
        latest_version: latest_version.to_string(),
        update_available: latest_version > current,
        release_url: release.html_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[test]
    fn builds_protocol_specific_models_urls() {
        let openai = Url::parse("https://example.test/v1").unwrap();
        assert_eq!(
            models_url(&openai, CliProtocol::OpenaiResponses)
                .unwrap()
                .as_str(),
            "https://example.test/v1/models"
        );
        let anthropic = Url::parse("https://example.test").unwrap();
        assert_eq!(
            models_url(&anthropic, CliProtocol::AnthropicMessages)
                .unwrap()
                .as_str(),
            "https://example.test/v1/models"
        );
    }

    #[tokio::test]
    async fn cross_origin_redirect_drops_all_credentials() {
        let source = MockServer::start().await;
        let destination = MockServer::start().await;
        let destination_url = format!("{}/models", destination.uri());
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", destination_url.as_str()),
            )
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "fixture-model" }]
            })))
            .mount(&destination)
            .await;
        let connection = ProviderConnection {
            id: uuid::Uuid::new_v4(),
            template_endpoint_id: None,
            credential_slot_id: "api-key".into(),
            protocol: CliProtocol::OpenaiResponses,
            endpoint: Url::parse(&format!("{}/v1", source.uri())).unwrap(),
            auth_type: ConnectionAuthType::Bearer,
            api_key: "redirect-secret-value".into(),
            default_model: "fixture-model".into(),
            verification: crate::domain::VerificationInfo::default(),
        };
        let models = ModelCatalogService::new()
            .unwrap()
            .list_models(&connection)
            .await
            .unwrap();
        assert_eq!(models, vec!["fixture-model"]);
        let requests = destination.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].headers.get("authorization").is_none());
        assert!(requests[0].headers.get("x-api-key").is_none());
    }
}
