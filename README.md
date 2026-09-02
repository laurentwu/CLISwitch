# CLISwitch

CLISwitch is a local Tauri 2 desktop application for inspecting, saving, and safely switching user-level configurations for Claude Code, Codex CLI, and OpenCode. Version 0.1 targets Windows, macOS, and Linux; mobile platforms are intentionally out of scope.

> **Credential warning:** CLISwitch deliberately stores and displays API keys and OAuth material in plaintext. File permissions are not encryption. Read [SECURITY_MODEL.md](SECURITY_MODEL.md) before entering credentials.

[简体中文](README.zh-CN.md) · [CLI compatibility](CLI_SUPPORT.md) · [Security model](SECURITY_MODEL.md)

## 0.1 capabilities

- Three top-level sections: Configurations, Providers, and Settings.
- A horizontal `Current configuration / named configurations / +` workspace.
- Local discovery with explicit executable and config-directory overrides.
- Endpoint + key providers with OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages connections.
- Provider templates sourced from a bundled [CLIAdapter](https://github.com/laurentwu/CLIAdapter) snapshot, with a private local cache and a manual update action in Settings.
- Every upstream provider is visible by name and ID. A provider contributes only the one to three protocol endpoints it declares, and only endpoints which pass the fixed protocol and security policy are selectable.
- Models are not merged from an external catalog: each endpoint requires a manually entered model ID before saving. A saved connection can still request its live `/models` list. Custom providers remain available for endpoints outside the catalog.
- Anthropic OAuth for Claude Code and Codex OAuth for Codex CLI, using installed official CLIs or offline auth-file import.
- Full plaintext viewing, copying, and editing of API keys and OAuth source content.
- Per-CLI file previews, save-before-apply, optimistic digest conflict checks, sequential per-CLI writes, atomic replacement, verification, rollback, cancellation, and retry of failed items only.
- Credential-bearing backups with tombstones and the latest five versions retained per source file.
- Chinese and English UI, light/dark/system themes, single-instance behavior, and persisted window state.
- No telemetry, automatic updater, remote scripts, or generic shell/filesystem IPC.

The exact supported schemas and field mappings are recorded in [CLI_SUPPORT.md](CLI_SUPPORT.md). CLISwitch manages user-level configuration only; project configuration, environment variables, and enterprise policy can override it.

## Development

Pinned toolchain majors are Node.js 24, pnpm 11, and Rust 1.88. macOS 12 or newer is the build/runtime baseline. Install the normal [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system, then run:

```bash
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

Refresh the checked-in CLIAdapter provider snapshot with `pnpm catalog:update`. The updater uses the
fixed upstream URL, validates provider identities and declared protocol endpoints, and records a
digest sidecar.

Common verification commands:

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
pnpm test:e2e
pnpm tauri build
```

`pnpm test:e2e` builds a dedicated automation-enabled debug binary. Its WDIO plugins, global Tauri API, and permissions are absent from normal production builds. The test configuration creates isolated HOME/XDG/APPDATA trees and installs only fixture CLIs there.

Linux development additionally needs WebKitGTK 4.1 and the other Tauri system packages. In headless CI, run E2E through `xvfb-run -a pnpm test:e2e`.

## Builds and releases

The Tauri application identifier is `io.github.laurentwu.cliswitch`. Production targets are Windows NSIS, macOS DMG, and Linux AppImage plus deb. Version 0.1 artifacts are intentionally unsigned; Windows SmartScreen and macOS Gatekeeper may warn. Releases are published manually on GitHub Releases, and the in-app update check only compares versions and opens the release page—it never downloads or executes an update.

Apache-2.0 licensed. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for dependency and asset notices.
