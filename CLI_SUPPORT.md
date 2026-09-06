# CLI support baseline

This matrix is the compatibility contract for CLISwitch 0.1, reviewed 2026-09-06 against the
stable public CLI schemas. CLISwitch fingerprints the supported shape and refuses known
incompatible shapes rather than replacing an entire file. Re-test these mappings before each
release because upstream CLIs can change independently.

| CLI         | Discovery and user files                                                                                                                                                                     | API protocols                                     | OAuth                                                                    | Schema fingerprint                                                     |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| Claude Code | `claude`; `CLAUDE_CONFIG_DIR` or `~/.claude`; `settings.json`; Linux/Windows `.credentials.json`                                                                                             | Anthropic Messages                                | Anthropic only; official `claude auth login`; macOS `claude setup-token` | `stable-2026-09:templated-model-slots+env/.credentials.json`           |
| Codex CLI   | `codex`; `CODEX_HOME` or `~/.codex`; `config.toml`; `auth.json`; CLISwitch-owned `cliswitch-models/<provider UUID>-<connection UUID>.json`                                                   | OpenAI Responses only                             | Codex only; official `codex login`; file credential store                | `stable-2026-09:templated-responses+model-catalog/file-auth`           |
| OpenCode    | `opencode`; `$XDG_CONFIG_HOME/opencode` or `~/.config/opencode`; `opencode.jsonc` preferred over `opencode.json`; `$XDG_DATA_HOME/opencode/auth.json`; `$XDG_STATE_HOME/opencode/model.json` | OpenAI Chat, OpenAI Responses, Anthropic Messages | Not supported in 0.1                                                     | `stable-v1:templated-provider-leaves+auth.type-api+state.model.recent` |

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
strings, and fragments are rejected. Models are not read or merged from the provider source. A
complete unmanaged API endpoint remains savable when its CLI configuration has no model: the save
dialog can request the candidate's live `/models` list or accept a manually entered model ID, and a
non-empty selection is still required before saving. Live listing uses only the short-lived scan
candidate ID or an already saved connection ID; endpoint and credential values are not accepted
from that dialog. Both OpenAI-style `data[].id` responses and Codex model-catalog
`models[].slug` responses are supported; entries without a non-empty identifier are ignored.

The old static API provider templates are not used for new records but remain available for
historical compatibility and tests. Refreshing the provider database never deletes or overwrites
saved providers or configurations. OAuth templates are fixed by the CLI contract and are
independent of CLIAdapter. Custom providers remain available for endpoints outside the database.

Configuration-file templates are a separate, immutable bundle under
`src-tauri/catalog/config-templates`. They are pinned to CLIAdapter commit
`7ea4dcc5e874d76a14e54a8e15f4fec7b8c5522d`, compiled through an explicit allowlist, and verified
against a SHA-256 manifest. A provider-specific file is selected only by the saved `template_id`;
otherwise the CLI-generic file is used. Settings refreshes only the provider/endpoint database and
never change this template version. Selected models remain user values, not a template allowlist.

## Managed field mappings

### Claude Code

- The selected model is written to top-level `model`, `env.ANTHROPIC_MODEL`, and the template's
  Haiku, Sonnet, Opus, and subagent slots. The generic template writes all five environment model
  slots. DeepSeek adds `[1m]` to the main/Sonnet/Opus mappings without duplicating an existing
  suffix. Zhipu/Z.AI omit the subagent slot, and OpenCode Zen/Go omit all auxiliary slots. Fields
  omitted by the selected template are removed. `env.ANTHROPIC_SMALL_FAST_MODEL` is always cleared.
  Reading still treats top-level `model` as primary and falls back to `env.ANTHROPIC_MODEL`.
- Endpoint: `env.ANTHROPIC_BASE_URL`.
- X-Api-Key auth: `env.ANTHROPIC_API_KEY`.
- Bearer auth: `env.ANTHROPIC_AUTH_TOKEN`.
- DeepSeek manages `CLAUDE_CODE_EFFORT_LEVEL` and `CLAUDE_CODE_AUTO_COMPACT_WINDOW` from its
  template. Zhipu/Z.AI manage the compact window, nonessential-traffic flag, and API timeout.
  Inapplicable tuning fields are removed when templates are switched.
- OAuth: Linux/Windows `.credentials.json`; macOS setup token in `env.CLAUDE_CODE_OAUTH_TOKEN`.
- OAuth retains only the top-level selected model from these managed API fields and clears API
  endpoint, credentials, all environment model slots, and tuning parameters. API and OAuth fields
  that conflict with the selected mode are removed; unrelated JSONC fields, comments, ordering,
  and line endings are retained.
- Process environment variables matching any managed Claude environment field are reported by
  presence only as external overrides. Their values are not captured. If both API credential
  variables are present, scanning refuses to choose between them.

### Codex CLI

- `model` selects the model and `model_provider` selects a namespaced
  `cliswitch_<provider UUID>` table.
- API templates set `model_reasoning_effort` (`max` for Zhipu/Z.AI, `high` otherwise) and
  `model_catalog_json` to an absolute CLISwitch-owned path beneath the resolved config directory.
  DeepSeek also sets `preferred_auth_method = "apikey"` and `forced_login_method = "api"`; those
  fields are removed for other API templates.
- The provider table uses `base_url`, `wire_api = "responses"`, and
  `experimental_bearer_token` for a custom Responses endpoint. Every preview displays a warning
  because the field is documented upstream but discouraged. `env_key`, `requires_openai_auth`,
  and `auth` are removed only from the current namespaced table.
- The CLISwitch model-catalog file contains exactly the selected model. DeepSeek has three exact
  metadata templates (including image input for `deepseek-v4-flash-vision-exp`); other model IDs
  use their provider or generic template. Existing unrelated root fields are retained, while the
  managed `models` array is replaced. Existing external catalogs are never read, changed, or
  deleted.
- OAuth writes `auth.json`, selects the normal OpenAI provider, and sets
  `cli_auth_credentials_store = "file"` through the supported TOML patch. It removes API template
  login, reasoning, and model-catalog fields without deleting dormant provider tables or catalog
  files.
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
  `models` object with a name and `reasoning = true`. The provider package is fixed by the saved
  connection protocol, even when a provider-specific upstream template uses another native
  transport.
- Provider, options, model, and auth entries are patched at leaf paths. Other provider instances,
  models, comments, and extension fields are retained. The current instance's legacy inline
  `options.apiKey` is removed; `auth.json` receives `type = "api"` and the key. Known OAuth fields
  on that same auth entry are cleared while unknown fields and other auth entries remain.

## Behavior outside the baseline

Unreadable files, malformed roots, unsupported field types, beta/unknown managed schemas,
permission failures, non-regular files, unsafe symlinks, and post-preview digest changes are
reported per CLI. One failed CLI does not stop the remaining queue. A missing CLI is skipped and
does not cause CLISwitch to create a speculative configuration for it.

OAuth is intentionally one-to-one: Anthropic OAuth can target only Claude Code, and Codex OAuth can
target only Codex CLI. OpenCode uses endpoint + key providers only in 0.1.
