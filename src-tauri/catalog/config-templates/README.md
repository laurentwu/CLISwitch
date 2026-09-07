# Bundled CLI configuration templates

This directory contains an audited, byte-for-byte subset of
[`laurentwu/CLIAdapter`](https://github.com/laurentwu/CLIAdapter) at commit
`7ea4dcc5e874d76a14e54a8e15f4fec7b8c5522d`. The upstream `LICENSE` is kept next to the
resources. `manifest.json` records the source identity, role, protocol, provider/model identity,
and SHA-256 digest of every compiled resource.

These templates are versioned with CLISwitch. They are not downloaded or discovered at runtime.
The backend compiles an explicit resource allowlist, verifies it against the manifest, parses the
selected template, and applies only the reviewed managed fields. Runtime provider-catalog refreshes
update endpoint discovery data only; they do not update these files.

CLISwitch deliberately adapts upstream values at the local trust boundary:

- saved endpoints, authentication types, credentials, display names, and instance UUIDs override
  upstream examples;
- Codex model-catalog output paths are generated beneath the resolved config directory;
- OpenCode native providers and environment-key authentication are converted to namespaced
  instances and the existing local `auth.json` format;
- supported OpenCode packages are fixed by the selected CLISwitch protocol.

To update the bundle:

1. Select and record one reviewed upstream commit; never use a moving branch.
2. Fetch only the existing allowlisted roles and provider/model IDs, preserving relative paths and
   original bytes. Do not run `pnpm catalog:update` as part of this process.
3. Recompute every SHA-256 in `manifest.json`, then review both the upstream diff and the manifest
   diff, including the upstream license.
4. Update the Rust resource table and explicit managed-field rules for every new or removed field,
   placeholder, package, protocol, provider, or exact-model resource. Unknown content must fail
   validation rather than silently fall back.
5. Update renderer, adapter, idempotence, conflict, rollback, and expected-output tests before
   releasing the new template version with the application.

Do not edit an imported upstream template to make it fit CLISwitch. Put intentional adaptations in
the typed Rust renderer so their behavior remains reviewable and tested.
