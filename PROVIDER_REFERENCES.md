# Provider references

本文档集中保存各供应商的官方 API、认证和定价文档链接。仅在 provider integration、endpoint 或
pricing 相关变更时按需查阅；具体兼容行为以项目内 catalog 和 `CLI_SUPPORT.md` 为准。

## OpenAI

https://developers.openai.com/api/reference/resources/responses/methods/create
https://developers.openai.com/api/docs/pricing

## Anthropic

https://platform.claude.com/docs/en/api/messages/create
https://platform.claude.com/docs/en/about-claude/pricing

## OpenRouter

https://openrouter.ai/docs/api/api-reference/chat/create-a-chat-completion
https://openrouter.ai/pricing

## OpenCode Zen

https://opencode.ai/docs/zen/
https://opencode.ai/docs/providers/
https://opencode.ai/zen/v1/models

OpenCode's `opencode/<model-id>` models use the shared Zen base URL. Responses models use
`@ai-sdk/openai`, Chat Completions models use `@ai-sdk/openai-compatible`, and Anthropic Messages
models use `@ai-sdk/anthropic`. The Google-backed models use `@ai-sdk/google`, which is recorded as
unsupported in CLISwitch because that adapter is not part of the OpenCode baseline.

## OpenCode Go

https://opencode.ai/docs/go/
https://opencode.ai/zen/go/v1/models

OpenCode Go uses the `opencode-go/<model-id>` prefix and the same three protocol families under the
Go base URL. The available model list and endpoint assignments are upstream-managed; CLISwitch
keeps the documented list as catalog suggestions and selects the endpoint from the chosen model.

## GLM Coding Plan（中国站）

https://docs.bigmodel.cn/cn/coding-plan/overview
https://docs.bigmodel.cn/cn/coding-plan/tool/others
https://docs.bigmodel.cn/cn/coding-plan/tool/codex

## Z.AI Coding Plan

https://docs.z.ai/devpack/overview
https://docs.z.ai/devpack/quick-start

## MiniMax Token Plan（minimax.io）

https://platform.minimax.io/docs/api-reference/text-chat-anthropic
https://platform.minimax.io/docs/guides/pricing-token-plan

## MiniMax Token Plan（minimaxi.com）

https://platform.minimaxi.com/docs/api-reference/text-chat-anthropic
https://platform.minimaxi.com/docs/guides/pricing-token-plan

## Alibaba Cloud Coding Plan（International）

https://www.alibabacloud.com/help/en/model-studio/coding-plan

## Alibaba Cloud Coding Plan（中国站）

https://help.aliyun.com/zh/model-studio/coding-plan

## Tencent Cloud Coding Plan

https://cloud.tencent.com/document/product/1823/130092

## Kimi Code

https://www.kimi.com/code/docs/
https://www.kimi.com/en/help/membership/membership-pricing

## Umans AI Coding Plan

https://app.umans.ai/offers/code/docs
https://app.umans.ai/offers/code

## KUAE Cloud Coding Plan

https://docs.mthreads.com/kuaecloud/kuaecloud-doc-online/coding_plan/plan_overview/
https://docs.mthreads.com/kuaecloud/kuaecloud-doc-online/coding_plan/tools_config/
