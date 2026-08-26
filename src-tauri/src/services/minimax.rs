use url::Url;

use crate::domain::{ConnectionAuthType, ProviderData, ProviderProfile};

pub const GLOBAL_API_TEMPLATE_ID: &str = "minimax-api";
pub const CHINA_API_TEMPLATE_ID: &str = "minimax-cn-api";
pub const GLOBAL_TOKEN_PLAN_TEMPLATE_ID: &str = "minimax-coding-plan";
pub const CHINA_TOKEN_PLAN_TEMPLATE_ID: &str = "minimax-cn-coding-plan";
pub const ANTHROPIC_ENDPOINT_ID: &str = "anthropic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimaxRegion {
    Global,
    China,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimaxCredentialKind {
    Api,
    TokenPlan,
}

impl MinimaxCredentialKind {
    pub const fn auth_type(self) -> ConnectionAuthType {
        match self {
            Self::Api => ConnectionAuthType::ApiKey,
            Self::TokenPlan => ConnectionAuthType::Bearer,
        }
    }
}

pub fn classify_credential(api_key: &str) -> MinimaxCredentialKind {
    if api_key.starts_with("sk-cp-") {
        MinimaxCredentialKind::TokenPlan
    } else {
        MinimaxCredentialKind::Api
    }
}

pub fn recognize_anthropic_endpoint(endpoint: &Url) -> Option<MinimaxRegion> {
    if endpoint.scheme() != "https"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.port_or_known_default() != Some(443)
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return None;
    }
    let path = endpoint.path().trim_end_matches('/');
    if !matches!(path, "/anthropic" | "/anthropic/v1") {
        return None;
    }
    match endpoint.host_str()? {
        "api.minimax.io" => Some(MinimaxRegion::Global),
        "api.minimaxi.com" => Some(MinimaxRegion::China),
        _ => None,
    }
}

pub const fn template_id(
    region: MinimaxRegion,
    credential_kind: MinimaxCredentialKind,
) -> &'static str {
    match (region, credential_kind) {
        (MinimaxRegion::Global, MinimaxCredentialKind::Api) => GLOBAL_API_TEMPLATE_ID,
        (MinimaxRegion::China, MinimaxCredentialKind::Api) => CHINA_API_TEMPLATE_ID,
        (MinimaxRegion::Global, MinimaxCredentialKind::TokenPlan) => GLOBAL_TOKEN_PLAN_TEMPLATE_ID,
        (MinimaxRegion::China, MinimaxCredentialKind::TokenPlan) => CHINA_TOKEN_PLAN_TEMPLATE_ID,
    }
}

fn template_region(template_id: &str) -> Option<MinimaxRegion> {
    match template_id {
        GLOBAL_API_TEMPLATE_ID | GLOBAL_TOKEN_PLAN_TEMPLATE_ID => Some(MinimaxRegion::Global),
        CHINA_API_TEMPLATE_ID | CHINA_TOKEN_PLAN_TEMPLATE_ID => Some(MinimaxRegion::China),
        _ => None,
    }
}

/// Makes an explicitly selected MiniMax template agree with the credential's billing kind.
/// The region remains user-selected; only API versus Token Plan is inferred from the secret.
pub fn normalize_provider_credential_kind(provider: &mut ProviderProfile) -> bool {
    let Some(current_template_id) = provider.template_id.as_deref() else {
        return false;
    };
    let Some(region) = template_region(current_template_id) else {
        return false;
    };
    let ProviderData::Api(api) = &mut provider.data else {
        return false;
    };
    let Some(connection) = api.connections.iter_mut().find(|connection| {
        connection.template_endpoint_id.as_deref() == Some(ANTHROPIC_ENDPOINT_ID)
    }) else {
        return false;
    };
    let credential_kind = classify_credential(&connection.api_key);
    let expected_template_id = template_id(region, credential_kind);
    let expected_auth_type = credential_kind.auth_type();
    let changed =
        current_template_id != expected_template_id || connection.auth_type != expected_auth_type;
    provider.template_id = Some(expected_template_id.into());
    connection.auth_type = expected_auth_type;
    changed
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::domain::{ApiProviderData, CliProtocol, ProviderConnection, VerificationInfo};

    use super::*;

    #[test]
    fn token_plan_prefix_is_the_only_special_credential_kind() {
        assert_eq!(
            classify_credential("sk-cp-fixture"),
            MinimaxCredentialKind::TokenPlan
        );
        assert_eq!(
            classify_credential("sk-api-fixture"),
            MinimaxCredentialKind::Api
        );
        assert_eq!(
            classify_credential("legacy-key"),
            MinimaxCredentialKind::Api
        );
        assert_eq!(
            classify_credential("SK-CP-fixture"),
            MinimaxCredentialKind::Api
        );
    }

    #[test]
    fn endpoint_recognition_is_exact_and_accepts_claude_and_canonical_paths() {
        for endpoint in [
            "https://api.minimax.io/anthropic",
            "https://api.minimax.io/anthropic/",
            "https://api.minimax.io/anthropic/v1",
            "https://api.minimaxi.com/anthropic/v1/",
        ] {
            assert!(recognize_anthropic_endpoint(&Url::parse(endpoint).unwrap()).is_some());
        }
        for endpoint in [
            "http://api.minimax.io/anthropic",
            "https://api.minimax.io.example/anthropic",
            "https://api.minimax.io/anthropic/v1/messages",
            "https://api.minimax.io/anthropic?redirect=1",
        ] {
            assert_eq!(
                recognize_anthropic_endpoint(&Url::parse(endpoint).unwrap()),
                None
            );
        }
    }

    #[test]
    fn saved_minimax_template_follows_the_key_kind_without_changing_region() {
        let now = Utc::now();
        let mut provider = ProviderProfile {
            id: Uuid::new_v4(),
            name: "MiniMax".into(),
            template_id: Some(GLOBAL_API_TEMPLATE_ID.into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: Some(ANTHROPIC_ENDPOINT_ID.into()),
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::AnthropicMessages,
                    endpoint: Url::parse("https://api.minimax.io/anthropic/v1").unwrap(),
                    auth_type: ConnectionAuthType::ApiKey,
                    api_key: "sk-cp-fixture".into(),
                    default_model: "MiniMax-M2.7".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        };

        assert!(normalize_provider_credential_kind(&mut provider));
        assert_eq!(
            provider.template_id.as_deref(),
            Some(GLOBAL_TOKEN_PLAN_TEMPLATE_ID)
        );
        let ProviderData::Api(api) = provider.data else {
            unreachable!();
        };
        assert_eq!(api.connections[0].auth_type, ConnectionAuthType::Bearer);
    }
}
