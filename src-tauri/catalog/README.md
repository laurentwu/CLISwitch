# Provider catalog

The runtime provider source is `providers.json`, a snapshot of
`https://laurentwu.github.io/CLIAdapter/providers.json`. It is compiled into the application as the
offline fallback. Run `pnpm catalog:update` to refresh it and `providers.meta.json`; do not
hand-edit either generated file.

`clis.jsonc` remains the small, fixed compatibility policy for supported CLIs, wire protocols,
OAuth modes, and OpenCode package mappings. The source protocol names map only to these built-in
adapters; no remote package or code is imported or executed.

At runtime a validated download is stored as private `providers.json` plus
`providers.meta.json` in the application-data directory. A valid local file takes precedence;
an absent, oversized, or invalid local file falls back to the bundled snapshot. Updates use the
fixed upstream URL, reject redirects, cap response size, validate before activation, and replace
each cache file atomically with a digest-matched sidecar. A failed update leaves the previous
active snapshot unchanged.

`provider-templates.jsonc` and `cli-provider-relations.jsonc` are retained for migration,
historical import/validation, and regression tests for the pre-release static-provider design.
New runtime API templates and CLI relations are generated from the one to three endpoints actually
declared by each CLIAdapter provider plus `clis.jsonc`; OAuth templates remain fixed and independent
of CLIAdapter. Persisted provider IDs must remain the upstream IDs. The source provides no model
catalog, so model IDs are entered manually for every endpoint before save.
