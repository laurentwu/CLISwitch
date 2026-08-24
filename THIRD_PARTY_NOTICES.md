# Third-party notices

CLISwitch itself is licensed under Apache-2.0. It includes or links software maintained by third parties under their own licenses. Copyright remains with the respective authors.

This notice was reviewed against `pnpm-lock.yaml` and `src-tauri/Cargo.lock` on 2026-08-23. The lockfiles are the canonical exact-version inventory. To regenerate the machine-readable inputs used for release review:

```bash
pnpm licenses list --prod --json > pnpm-licenses.json
cargo metadata --manifest-path src-tauri/Cargo.toml --locked --format-version 1 > cargo-metadata.json
```

The generated JSON files are review artifacts and are not committed because they contain redundant package metadata. A release owner must review new or changed license expressions before publishing.

## Principal runtime components

| Component                                                   | License family                                                       |
| ----------------------------------------------------------- | -------------------------------------------------------------------- |
| Tauri, Tauri plugins, tauri-build                           | Apache-2.0 OR MIT                                                    |
| React, React DOM                                            | MIT                                                                  |
| TanStack Query                                              | MIT                                                                  |
| Zustand                                                     | MIT                                                                  |
| react-hook-form, Zod                                        | MIT                                                                  |
| i18next, react-i18next                                      | MIT                                                                  |
| Tailwind CSS, tailwind-merge                                | MIT                                                                  |
| Lucide React                                                | ISC                                                                  |
| Tokio, Serde, SQLx, reqwest, url, uuid, chrono, tracing     | MIT and/or Apache-2.0 as identified in Cargo.lock metadata           |
| jsonc-parser, toml_edit, portable-pty, sysinfo, directories | Permissive licenses identified in Cargo.lock metadata                |
| SQLite bundled through SQLx/system SQLite                   | Public-domain SQLite terms; wrapper crates retain their own licenses |

Build and test-only tools—including Vite, TypeScript, ESLint, Prettier, Vitest, WebdriverIO, and the WDIO Tauri plugins—are not intentionally shipped as production frontend code. They retain their respective permissive licenses. The dedicated E2E binary is not a release artifact.

## Assets and product names

The CLISwitch application icon and neutral CLI marks in this repository are original project assets released under the repository's Apache-2.0 license. No official Anthropic, OpenAI, Claude, Codex, or OpenCode logo artwork is bundled. Those names are used only to describe compatibility and remain trademarks of their respective owners; no endorsement is implied.

For full license texts, consult each package source distributed in the package manager cache or its upstream repository. Apache-2.0 text for CLISwitch is in [LICENSE](LICENSE).
