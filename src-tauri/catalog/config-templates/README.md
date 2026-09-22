# Bundled CLI configuration templates

This directory contains an audited, byte-for-byte subset of
[`laurentwu/CLIAdapter`](https://github.com/laurentwu/CLIAdapter) compiled from exactly two pinned
commits:

- the default source, commit `7ea4dcc5e874d76a14e54a8e15f4fec7b8c5522d`, used by every Claude Code,
  Codex CLI, and Qwen Code resource, plus the shared `LICENSE`;
- a newer OpenCode-only source, commit `25ce581516599103b2455cab6770f021aa5f2f91`
  (`fix(opencode)!: separate credentials and support JSONC (#22)`), used by all 23 OpenCode
  resources: the root `opencode.jsonc`/`auth.json` pair and each provider directory's
  `opencode.jsonc`/`auth.json`/`provider.json`.

The upstream `LICENSE` is kept next to the resources. `manifest.json` (schema version 2) records
the role, protocol, provider/model identity, SHA-256 digest, and — for the OpenCode subset — the
overriding source commit and `cli/` upstream path of every compiled resource. Resources outside the
OpenCode subset must not declare a source override; OpenCode resources must declare the newer
commit. No other source commit, repository, path, or URL is ever accepted.

These templates are versioned with CLISwitch. They are not downloaded or discovered at runtime.
The backend compiles an explicit resource allowlist, verifies it against the manifest, parses the
selected template, and applies only the reviewed managed fields. Runtime provider-catalog refreshes
update endpoint discovery data only; they do not update these files.

CLISwitch deliberately adapts upstream values at the local trust boundary:

- saved endpoints, authentication types, credentials, display names, and instance UUIDs override
  upstream examples;
- Codex model-catalog output paths are generated beneath the resolved config directory;
- OpenCode standard connections for the seven bundled providers reuse the provider-native OpenCode
  slot (native `model` reference and the same-named `auth.json` entry) exactly as the dedicated
  template declares it; custom providers and known providers with custom connections use the
  CLIAdapter root generic template with `cliswitch_<provider UUID>` instance IDs, saved transport
  values, and the separate generic `auth.json` entry;
- supported OpenCode packages are fixed by the selected CLISwitch protocol;
- Qwen provider groups and file-local env keys are replaced with connection-UUID-derived names;
  applying also writes Qwen's OpenAI auth and exact model/base-URL startup selection. An existing
  unique route is updated in place, while duplicate routes fail closed.

To update the bundle:

1. Select and record one reviewed upstream commit per source; never use a moving branch. Keep the
   two-source split unless every CLI is reviewed together in one change.
2. Fetch only the existing allowlisted roles and provider/model IDs, preserving relative paths and
   original bytes from `cli/` for OpenCode resources. Do not run `pnpm catalog:update` as part of
   this process.
3. Recompute every SHA-256 in `manifest.json`, set the schema version and per-resource source
   fields, then review both the upstream diff and the manifest diff, including the upstream
   license.
4. Update the Rust resource table and explicit managed-field rules for every new or removed field,
   placeholder, package, protocol, provider, or exact-model resource. Unknown content must fail
   validation rather than silently fall back.
5. Update renderer, adapter, idempotence, conflict, rollback, and expected-output tests before
   releasing the new template version with the application.

Do not edit an imported upstream template to make it fit CLISwitch. Put intentional adaptations in
the typed Rust renderer so their behavior remains reviewable and tested.
