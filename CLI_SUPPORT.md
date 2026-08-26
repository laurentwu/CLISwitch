# CLI support baseline

This matrix is the compatibility contract for CLISwitch 0.1, last reviewed 2026-08-23 against stable public CLI documentation. CLISwitch fingerprints the supported shape and refuses known incompatible shapes rather than replacing an entire file. Re-test these mappings before each release because upstream CLIs can change independently.

| CLI         | Discovery and user files                                                                                                                                                                     | API protocols                                     | OAuth                                                                             | 0.1 schema fingerprint                                                         |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Claude Code | `claude`; `CLAUDE_CONFIG_DIR` or `~/.claude`; `settings.json`; Linux/Windows `.credentials.json`                                                                                             | Anthropic Messages                                | Anthropic only; official `claude auth login`; macOS official `claude setup-token` | `stable-2026-08:settings.model+env/.credentials.json`                          |
| Codex CLI   | `codex`; `CODEX_HOME` or `~/.codex`; `config.toml`; `auth.json`                                                                                                                              | OpenAI Responses only                             | Codex only; official `codex login`; file credential store                         | `stable-2026-08:model_providers.responses+experimental_bearer_token/file-auth` |
| OpenCode    | `opencode`; `$XDG_CONFIG_HOME/opencode` or `~/.config/opencode`; `opencode.jsonc` preferred over `opencode.json`; `$XDG_DATA_HOME/opencode/auth.json`; `$XDG_STATE_HOME/opencode/model.json` | OpenAI Chat, OpenAI Responses, Anthropic Messages | Not supported in 0.1                                                              | `stable-v1:provider/npm/options/models+auth.type-api+state.model.recent`       |

Executable discovery uses, in order, a user-approved manual path, the process PATH, an approved login-shell PATH on Unix, and documented/common per-user install locations. Config-directory overrides are separate from executable overrides.

## Managed field mappings

### Claude Code

- Model: top-level `model` (read also recognizes `env.ANTHROPIC_MODEL`).
- Endpoint: `env.ANTHROPIC_BASE_URL`.
- X-Api-Key auth: `env.ANTHROPIC_API_KEY`.
- Bearer auth: `env.ANTHROPIC_AUTH_TOKEN`.
- MiniMax endpoints are recognized only on the official `api.minimax.io` and `api.minimaxi.com`
  hosts. A key beginning with `sk-cp-` is imported as the matching Token Plan; other MiniMax keys
  are imported as the matching pay-as-you-go API profile. Both `/anthropic` and the legacy
  `/anthropic/v1` stored form are accepted during discovery.
- Claude writes MiniMax's CLI-specific `https://<host>/anthropic` Base URL. Token Plan keys use
  `ANTHROPIC_AUTH_TOKEN`; pay-as-you-go keys use `ANTHROPIC_API_KEY`.
- OAuth: Linux/Windows `.credentials.json`; macOS setup token in `env.CLAUDE_CODE_OAUTH_TOKEN`.
- API and OAuth fields that conflict with the selected mode are removed; unrelated JSONC fields, comments, ordering, and line endings are retained.
- Process environment variables with these names are reported as external overrides. If both
  `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` are present in the process environment, scanning
  refuses to choose between them and reports a credential conflict instead.

### Codex CLI

- `model` selects the model and `model_provider` selects a namespaced `cliswitch_<provider UUID>` table.
- The provider table uses `base_url`, `wire_api = "responses"`, and `experimental_bearer_token` for a custom Responses endpoint. The latter is documented upstream but discouraged, so every preview displays a warning.
- OAuth writes `auth.json`, selects the normal OpenAI provider, and sets `cli_auth_credentials_store = "file"` through the supported TOML patch.
- A non-Responses custom `wire_api` is refused. `forced_login_method` is surfaced as an override.
- Unmanaged TOML tables, comments, ordering, and line endings are retained.

### OpenCode stable schema

- Only the stable singular `provider` object is supported. The beta plural `providers` schema is explicitly refused.
- Managed providers use namespaced IDs `cliswitch_<provider UUID>` and set the global `model` to `<provider ID>/<model>`.
- Current-model detection follows OpenCode's precedence: an explicit global `model`, then the first `recent` entry in `$XDG_STATE_HOME/opencode/model.json` (defaulting to `~/.local/state/opencode/model.json`), then a single unambiguous provider/model pair from the config. Ambiguous configured models are reported instead of guessed.
- Package mapping is loaded from `src-tauri/catalog/clis.jsonc` and tested:

| CLISwitch protocol      | OpenCode `npm` package      | Auth entry                      |
| ----------------------- | --------------------------- | ------------------------------- |
| OpenAI Chat Completions | `@ai-sdk/openai-compatible` | `{ "type": "api", "key": "…" }` |
| OpenAI Responses        | `@ai-sdk/openai`            | `{ "type": "api", "key": "…" }` |
| Anthropic Messages      | `@ai-sdk/anthropic`         | `{ "type": "api", "key": "…" }` |

- OpenCode credentials are enumerated from `auth.json`, then joined to the singular `provider`
  configuration by provider ID. Every complete `type: "api"` entry is offered separately for
  saving; OAuth entries are identified but are not savable in 0.1.
- Self-described custom providers need no template entry when they declare a supported `npm`,
  `options.baseURL`, and at least one model. Built-in providers may use this declarative fallback
  relation catalog. Explicit user configuration overrides every fallback value.

| OpenCode provider ID     | CLISwitch name                    | CLISwitch protocol | Auth    | Default endpoint                                |
| ------------------------ | --------------------------------- | ------------------ | ------- | ----------------------------------------------- |
| `openai`                 | OpenAI                            | OpenAI Responses   | Bearer  | `https://api.openai.com/v1`                     |
| `anthropic`              | Anthropic                         | Anthropic Messages | API key | `https://api.anthropic.com`                     |
| `openrouter`             | OpenRouter                        | OpenAI Chat        | Bearer  | `https://openrouter.ai/api/v1`                  |
| `zhipuai-coding-plan`    | GLM Coding Plan                   | OpenAI Chat        | Bearer  | `https://open.bigmodel.cn/api/coding/paas/v4`   |
| `zai-coding-plan`        | Z.AI Coding Plan                  | OpenAI Chat        | Bearer  | `https://api.z.ai/api/coding/paas/v4`           |
| `minimax-coding-plan`    | MiniMax Token Plan (minimax.io)   | Anthropic Messages | API key | `https://api.minimax.io/anthropic/v1`           |
| `minimax-cn-coding-plan` | MiniMax Token Plan (minimaxi.com) | Anthropic Messages | API key | `https://api.minimaxi.com/anthropic/v1`         |
| `alibaba-coding-plan`    | Alibaba Coding Plan               | OpenAI Chat        | Bearer  | `https://coding-intl.dashscope.aliyuncs.com/v1` |
| `alibaba-coding-plan-cn` | Alibaba Coding Plan (China)       | OpenAI Chat        | Bearer  | `https://coding.dashscope.aliyuncs.com/v1`      |
| `tencent-coding-plan`    | Tencent Coding Plan (China)       | OpenAI Chat        | Bearer  | `https://api.lkeap.cloud.tencent.com/coding/v3` |
| `kimi-for-coding`        | Kimi For Coding                   | Anthropic Messages | API key | `https://api.kimi.com/coding/v1`                |
| `umans-ai-coding-plan`   | Umans AI Coding Plan              | OpenAI Chat        | Bearer  | `https://api.code.umans.ai/v1`                  |
| `kuae-cloud-coding-plan` | KUAE Cloud Coding Plan            | OpenAI Chat        | Bearer  | `https://coding-plan-endpoint.kuaecloud.net/v1` |

The GLM Coding Plan template is one provider with one shared credential slot and three endpoint
identities: Anthropic Messages at `https://open.bigmodel.cn/api/anthropic`, OpenAI Chat at
`https://open.bigmodel.cn/api/coding/paas/v4`, and OpenAI Responses at
`https://open.bigmodel.cn/api/v1`. OpenCode has a relation to all three. None is marked as the
OpenCode default, so a saved configuration must explicitly select one endpoint.

Provider/CLI maintenance is split across three bundled, read-only JSONC files:

- `src-tauri/catalog/clis.jsonc` defines CLIs, supported protocols, auth modes, and protocol SDK
  packages.
- `src-tauri/catalog/provider-templates.jsonc` defines API templates (credential slots, endpoints,
  protocols, and suggested models) and auth templates.
- `src-tauri/catalog/cli-provider-relations.jsonc` joins a CLI to a template endpoint or auth mode,
  records recognized native provider IDs, and may override the Base URL required by a particular
  CLI.

Model lists are suggestions, not allowlists. Users may enter another model ID. Existing database
providers migrate as custom providers without endpoint inference; existing auth providers receive
their exact auth-template identity.

- Provider endpoint is written to `options.baseURL`; the selected model is placed in the provider `models` object.
- Config JSONC and auth JSON are patched at managed paths while retaining unrelated fields and formatting where the CST writer supports it.

## Behavior outside the baseline

Unreadable files, malformed roots, unsupported field types, beta/unknown managed schemas, permission failures, non-regular files, unsafe symlinks, and post-preview digest changes are reported per CLI. One failed CLI does not stop the remaining queue. A missing CLI is skipped and does not cause CLISwitch to create a speculative configuration for it.

OAuth is intentionally one-to-one: Anthropic OAuth can target only Claude Code, and Codex OAuth can target only Codex CLI. OpenCode uses endpoint + key providers only in 0.1.
