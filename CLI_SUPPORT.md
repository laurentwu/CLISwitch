# CLI support baseline

This matrix is the compatibility contract for CLISwitch 0.1, reviewed 2026-08-25 against the
stable public CLI schemas. CLISwitch fingerprints the supported shape and refuses known
incompatible shapes rather than replacing an entire file. Re-test these mappings before each
release because upstream CLIs can change independently.

| CLI         | Discovery and user files                                                                                                                                                                     | API protocols                                     | OAuth                                                                    | Schema fingerprint                                                             |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| Claude Code | `claude`; `CLAUDE_CONFIG_DIR` or `~/.claude`; `settings.json`; Linux/Windows `.credentials.json`                                                                                             | Anthropic Messages                                | Anthropic only; official `claude auth login`; macOS `claude setup-token` | `stable-2026-08:settings.model+env/.credentials.json`                          |
| Codex CLI   | `codex`; `CODEX_HOME` or `~/.codex`; `config.toml`; `auth.json`                                                                                                                              | OpenAI Responses only                             | Codex only; official `codex login`; file credential store                | `stable-2026-08:model_providers.responses+experimental_bearer_token/file-auth` |
| OpenCode    | `opencode`; `$XDG_CONFIG_HOME/opencode` or `~/.config/opencode`; `opencode.jsonc` preferred over `opencode.json`; `$XDG_DATA_HOME/opencode/auth.json`; `$XDG_STATE_HOME/opencode/model.json` | OpenAI Chat, OpenAI Responses, Anthropic Messages | Not supported in 0.1                                                     | `stable-v1:provider/npm/options/models+auth.type-api+state.model.recent`       |

Executable discovery uses, in order, a user-approved manual path, the process PATH, an approved
login-shell PATH on Unix, and documented/common per-user install locations. Config-directory
overrides are separate from executable overrides.

## Provider database and compatibility policy

Provider records come from the bundled `src-tauri/catalog/providers.json` snapshot. A validated
private local copy takes precedence and can be refreshed from the fixed
`https://laurentwu.github.io/CLIAdapter/providers.json` URL in Settings. The current snapshot has
seven providers. Each provider declares between one and three endpoints; CLISwitch never invents a
missing protocol connection.

Source protocols are mapped to fixed built-in adapters:

| Source protocol      | Internal protocol       | Supported CLIs        | OpenCode package            |
| -------------------- | ----------------------- | --------------------- | --------------------------- |
| `anthropic-messages` | Anthropic Messages      | Claude Code, OpenCode | `@ai-sdk/anthropic`         |
| `responses`          | OpenAI Responses        | Codex CLI, OpenCode   | `@ai-sdk/openai`            |
| `openai-compatible`  | OpenAI Chat Completions | OpenCode              | `@ai-sdk/openai-compatible` |

For OpenCode, `openai-compatible` is the native choice when it is declared. Otherwise the user
must select one of the provider's actual compatible endpoints. Claude Code and Codex CLI likewise
use only a declared compatible endpoint. Unknown protocols are disabled rather than inferred.

Provider endpoints must use HTTPS. Embedded credentials, unresolved `${…}` placeholders, query
strings, and fragments are rejected. Models are not read or merged from the provider source: every
declared endpoint requires a manually entered model ID before a provider can be saved. Live model
listing remains available only for an already saved connection.

The old static API provider templates are not used for new records but remain available for
historical compatibility and tests. Refreshing the provider database never deletes or overwrites
saved providers or configurations. OAuth templates are fixed by the CLI contract and are
independent of CLIAdapter. Custom providers remain available for endpoints outside the database.

## Managed field mappings

### Claude Code

- Model: top-level `model` (read also recognizes `env.ANTHROPIC_MODEL`).
- Endpoint: `env.ANTHROPIC_BASE_URL`.
- X-Api-Key auth: `env.ANTHROPIC_API_KEY`.
- Bearer auth: `env.ANTHROPIC_AUTH_TOKEN`.
- OAuth: Linux/Windows `.credentials.json`; macOS setup token in `env.CLAUDE_CODE_OAUTH_TOKEN`.
- API and OAuth fields that conflict with the selected mode are removed; unrelated JSONC fields,
  comments, ordering, and line endings are retained.
- Process environment variables with these names are reported as external overrides. If both API
  credential variables are present, scanning refuses to choose between them.

### Codex CLI

- `model` selects the model and `model_provider` selects a namespaced
  `cliswitch_<provider UUID>` table.
- The provider table uses `base_url`, `wire_api = "responses"`, and
  `experimental_bearer_token` for a custom Responses endpoint. Every preview displays a warning
  because the field is documented upstream but discouraged.
- OAuth writes `auth.json`, selects the normal OpenAI provider, and sets
  `cli_auth_credentials_store = "file"` through the supported TOML patch.
- A non-Responses custom `wire_api` is refused. `forced_login_method` is surfaced as an override.
- Unmanaged TOML tables, comments, ordering, and line endings are retained.

### OpenCode stable schema

- Only the stable singular `provider` object is supported. The beta plural `providers` schema is
  explicitly refused.
- Managed providers use namespaced IDs `cliswitch_<provider UUID>` and set the global `model` to
  `<provider ID>/<model>`.
- Current-model detection follows this precedence: explicit global `model`, the first `recent`
  entry in `$XDG_STATE_HOME/opencode/model.json` (defaulting to `~/.local/state/opencode/model.json`),
  then a single unambiguous provider/model pair from the config. Ambiguous configured models are
  reported instead of guessed.
- Credentials are enumerated from `auth.json` and joined to the singular `provider` configuration
  by provider ID. Complete `type: "api"` entries are offered separately for saving; OAuth entries
  are recognized but are not savable in 0.1.
- Provider endpoint is written to `options.baseURL`; the selected model is placed in the provider
  `models` object. Config JSONC and auth JSON are patched at managed paths while retaining
  unrelated fields and formatting where the CST writer supports it.

## Behavior outside the baseline

Unreadable files, malformed roots, unsupported field types, beta/unknown managed schemas,
permission failures, non-regular files, unsafe symlinks, and post-preview digest changes are
reported per CLI. One failed CLI does not stop the remaining queue. A missing CLI is skipped and
does not cause CLISwitch to create a speculative configuration for it.

OAuth is intentionally one-to-one: Anthropic OAuth can target only Claude Code, and Codex OAuth can
target only Codex CLI. OpenCode uses endpoint + key providers only in 0.1.
