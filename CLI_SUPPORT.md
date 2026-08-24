# CLI support baseline

This matrix is the compatibility contract for CLISwitch 0.1, last reviewed 2026-08-23 against stable public CLI documentation. CLISwitch fingerprints the supported shape and refuses known incompatible shapes rather than replacing an entire file. Re-test these mappings before each release because upstream CLIs can change independently.

| CLI         | Discovery and user files                                                                                                                              | API protocols                                     | OAuth                                                                             | 0.1 schema fingerprint                                                         |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Claude Code | `claude`; `CLAUDE_CONFIG_DIR` or `~/.claude`; `settings.json`; Linux/Windows `.credentials.json`                                                      | Anthropic Messages                                | Anthropic only; official `claude auth login`; macOS official `claude setup-token` | `stable-2026-08:settings.model+env/.credentials.json`                          |
| Codex CLI   | `codex`; `CODEX_HOME` or `~/.codex`; `config.toml`; `auth.json`                                                                                       | OpenAI Responses only                             | Codex only; official `codex login`; file credential store                         | `stable-2026-08:model_providers.responses+experimental_bearer_token/file-auth` |
| OpenCode    | `opencode`; `$XDG_CONFIG_HOME/opencode` or `~/.config/opencode`; `opencode.jsonc` preferred over `opencode.json`; `$XDG_DATA_HOME/opencode/auth.json` | OpenAI Chat, OpenAI Responses, Anthropic Messages | Not supported in 0.1                                                              | `stable-v1:provider/npm/options/models+auth.type-api`                          |

Executable discovery uses, in order, a user-approved manual path, the process PATH, an approved login-shell PATH on Unix, and documented/common per-user install locations. Config-directory overrides are separate from executable overrides.

## Managed field mappings

### Claude Code

- Model: top-level `model` (read also recognizes `env.ANTHROPIC_MODEL`).
- Endpoint: `env.ANTHROPIC_BASE_URL`.
- X-Api-Key auth: `env.ANTHROPIC_API_KEY`.
- Bearer auth: `env.ANTHROPIC_AUTH_TOKEN`.
- OAuth: Linux/Windows `.credentials.json`; macOS setup token in `env.CLAUDE_CODE_OAUTH_TOKEN`.
- API and OAuth fields that conflict with the selected mode are removed; unrelated JSONC fields, comments, ordering, and line endings are retained.
- Process environment variables with these names are reported as external overrides.

### Codex CLI

- `model` selects the model and `model_provider` selects a namespaced `cliswitch_<provider UUID>` table.
- The provider table uses `base_url`, `wire_api = "responses"`, and `experimental_bearer_token` for a custom Responses endpoint. The latter is documented upstream but discouraged, so every preview displays a warning.
- OAuth writes `auth.json`, selects the normal OpenAI provider, and sets `cli_auth_credentials_store = "file"` through the supported TOML patch.
- A non-Responses custom `wire_api` is refused. `forced_login_method` is surfaced as an override.
- Unmanaged TOML tables, comments, ordering, and line endings are retained.

### OpenCode stable schema

- Only the stable singular `provider` object is supported. The beta plural `providers` schema is explicitly refused.
- Managed providers use namespaced IDs `cliswitch_<provider UUID>` and set the global `model` to `<provider ID>/<model>`.
- Package mapping is fixed and tested:

| CLISwitch protocol      | OpenCode `npm` package      | Auth entry                      |
| ----------------------- | --------------------------- | ------------------------------- |
| OpenAI Chat Completions | `@ai-sdk/openai-compatible` | `{ "type": "api", "key": "…" }` |
| OpenAI Responses        | `@ai-sdk/openai`            | `{ "type": "api", "key": "…" }` |
| Anthropic Messages      | `@ai-sdk/anthropic`         | `{ "type": "api", "key": "…" }` |

- Provider endpoint is written to `options.baseURL`; the selected model is placed in the provider `models` object.
- Config JSONC and auth JSON are patched at managed paths while retaining unrelated fields and formatting where the CST writer supports it.

## Behavior outside the baseline

Unreadable files, malformed roots, unsupported field types, beta/unknown managed schemas, permission failures, non-regular files, unsafe symlinks, and post-preview digest changes are reported per CLI. One failed CLI does not stop the remaining queue. A missing CLI is skipped and does not cause CLISwitch to create a speculative configuration for it.

OAuth is intentionally one-to-one: Anthropic OAuth can target only Claude Code, and Codex OAuth can target only Codex CLI. OpenCode uses endpoint + key providers only in 0.1.
