# Provider catalog

The runtime provider/model source is `models.dev.json`, a full snapshot of
`https://models.dev/api.json`. It is compiled into the application as the offline fallback. Run
`pnpm catalog:update` to refresh it and `models.dev.meta.json`; do not hand-edit either generated
file.

`clis.jsonc` remains the small, fixed compatibility policy for supported CLIs, wire protocols,
OAuth modes, and OpenCode package mappings. Upstream npm package names are treated only as data:
CLISwitch maps an explicit allowlist to these built-in adapters and never imports or executes a
package from the snapshot.

At runtime a validated download is stored as private `models.dev.json` plus
`models.dev.meta.json` in the application-data directory. A valid local file takes precedence;
an absent, oversized, or invalid local file falls back to the bundled snapshot. Updates use the
fixed upstream URL, reject redirects, cap response size, validate before activation, and replace
each cache file atomically with a digest-matched sidecar. A failed update leaves the previous
active snapshot unchanged.

`provider-templates.jsonc` and `cli-provider-relations.jsonc` are retained only as migration/test
fixtures for the pre-release static-provider design. Runtime API templates and CLI relations are
generated from models.dev plus `clis.jsonc`; OAuth templates remain fixed and independent of
models.dev. Persisted models.dev provider IDs must remain the upstream IDs.
