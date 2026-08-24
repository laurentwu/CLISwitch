use crate::domain::{CliProtocol, ConnectionAuthType};

/// Declarative bridge from an OpenCode built-in provider to the connection shape stored by
/// CLISwitch. OpenCode's explicit user configuration always wins over these defaults.
///
/// Supporting another built-in API-key provider should require only one new row here and a
/// fixture assertion. Providers that declare `npm`, `options.baseURL`, and `models` in
/// `opencode.json(c)` do not need a row at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenCodeProviderRelation {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    pub npm_package: &'static str,
    pub protocol: CliProtocol,
    pub auth_type: ConnectionAuthType,
    pub default_endpoint: &'static str,
}

// Defaults reviewed against OpenCode's models.dev catalog on 2026-08-24. Keep CLI_SUPPORT.md in
// sync when a row changes.
pub(crate) const OPENCODE_PROVIDER_RELATIONS: &[OpenCodeProviderRelation] = &[
    OpenCodeProviderRelation {
        provider_id: "openai",
        display_name: "OpenAI",
        npm_package: "@ai-sdk/openai",
        protocol: CliProtocol::OpenaiResponses,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://api.openai.com/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "anthropic",
        display_name: "Anthropic",
        npm_package: "@ai-sdk/anthropic",
        protocol: CliProtocol::AnthropicMessages,
        auth_type: ConnectionAuthType::ApiKey,
        default_endpoint: "https://api.anthropic.com",
    },
    OpenCodeProviderRelation {
        provider_id: "openrouter",
        display_name: "OpenRouter",
        npm_package: "@openrouter/ai-sdk-provider",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://openrouter.ai/api/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "zhipuai-coding-plan",
        display_name: "Zhipu AI Coding Plan",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://open.bigmodel.cn/api/coding/paas/v4",
    },
    OpenCodeProviderRelation {
        provider_id: "zai-coding-plan",
        display_name: "Z.AI Coding Plan",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://api.z.ai/api/coding/paas/v4",
    },
    OpenCodeProviderRelation {
        provider_id: "minimax-coding-plan",
        display_name: "MiniMax Token Plan (minimax.io)",
        npm_package: "@ai-sdk/anthropic",
        protocol: CliProtocol::AnthropicMessages,
        auth_type: ConnectionAuthType::ApiKey,
        default_endpoint: "https://api.minimax.io/anthropic/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "minimax-cn-coding-plan",
        display_name: "MiniMax Token Plan (minimaxi.com)",
        npm_package: "@ai-sdk/anthropic",
        protocol: CliProtocol::AnthropicMessages,
        auth_type: ConnectionAuthType::ApiKey,
        default_endpoint: "https://api.minimaxi.com/anthropic/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "alibaba-coding-plan",
        display_name: "Alibaba Coding Plan",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://coding-intl.dashscope.aliyuncs.com/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "alibaba-coding-plan-cn",
        display_name: "Alibaba Coding Plan (China)",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://coding.dashscope.aliyuncs.com/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "tencent-coding-plan",
        display_name: "Tencent Coding Plan (China)",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://api.lkeap.cloud.tencent.com/coding/v3",
    },
    OpenCodeProviderRelation {
        provider_id: "kimi-for-coding",
        display_name: "Kimi For Coding",
        npm_package: "@ai-sdk/anthropic",
        protocol: CliProtocol::AnthropicMessages,
        auth_type: ConnectionAuthType::ApiKey,
        default_endpoint: "https://api.kimi.com/coding/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "umans-ai-coding-plan",
        display_name: "Umans AI Coding Plan",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://api.code.umans.ai/v1",
    },
    OpenCodeProviderRelation {
        provider_id: "kuae-cloud-coding-plan",
        display_name: "KUAE Cloud Coding Plan",
        npm_package: "@ai-sdk/openai-compatible",
        protocol: CliProtocol::OpenaiChat,
        auth_type: ConnectionAuthType::Bearer,
        default_endpoint: "https://coding-plan-endpoint.kuaecloud.net/v1",
    },
];

pub(crate) fn provider_relation(provider_id: &str) -> Option<&'static OpenCodeProviderRelation> {
    OPENCODE_PROVIDER_RELATIONS
        .iter()
        .find(|relation| relation.provider_id == provider_id)
}

pub(crate) fn package_protocol(npm_package: &str) -> Option<CliProtocol> {
    match npm_package {
        "@ai-sdk/openai-compatible" => Some(CliProtocol::OpenaiChat),
        "@ai-sdk/openai" => Some(CliProtocol::OpenaiResponses),
        "@ai-sdk/anthropic" => Some(CliProtocol::AnthropicMessages),
        _ => None,
    }
}

pub(crate) const fn protocol_auth_type(protocol: CliProtocol) -> ConnectionAuthType {
    match protocol {
        CliProtocol::AnthropicMessages => ConnectionAuthType::ApiKey,
        CliProtocol::OpenaiChat | CliProtocol::OpenaiResponses => ConnectionAuthType::Bearer,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use url::Url;

    use super::*;

    #[test]
    fn provider_relations_are_unique_and_well_formed() {
        let mut ids = HashSet::new();
        for relation in OPENCODE_PROVIDER_RELATIONS {
            assert!(ids.insert(relation.provider_id));
            assert!(!relation.display_name.trim().is_empty());
            assert!(!relation.npm_package.trim().is_empty());
            let endpoint = Url::parse(relation.default_endpoint).unwrap();
            assert!(matches!(endpoint.scheme(), "http" | "https"));
            assert_eq!(
                relation.auth_type,
                protocol_auth_type(relation.protocol),
                "relation {} has an incompatible auth type",
                relation.provider_id
            );
        }
    }

    #[test]
    fn zhipuai_relation_maps_to_a_cliswitch_connection() {
        let relation = provider_relation("zhipuai-coding-plan").unwrap();
        assert_eq!(relation.protocol, CliProtocol::OpenaiChat);
        assert_eq!(relation.auth_type, ConnectionAuthType::Bearer);
        assert_eq!(
            relation.default_endpoint,
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
    }
}
