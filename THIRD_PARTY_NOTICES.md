# Third-party notices

CLISwitch itself is licensed under Apache-2.0. It includes or links software maintained by third parties under their own licenses. Copyright remains with the respective authors.

The baseline notice was reviewed against `pnpm-lock.yaml` and `src-tauri/Cargo.lock` on 2026-09-06; the UI dependency additions below were reviewed on 2026-09-20. The lockfiles are the canonical exact-version inventory. To regenerate the machine-readable inputs used for release review:

```bash
pnpm licenses list --prod --json > pnpm-licenses.json
cargo metadata --manifest-path src-tauri/Cargo.toml --locked --format-version 1 > cargo-metadata.json
```

Generated license and dependency JSON files are review artifacts and are not committed because they
contain redundant package metadata. The bundled CLIAdapter provider snapshot is tracked separately
below. A release owner must review new or changed license expressions before publishing.

## Principal runtime components

| Component                                                           | License family                                                       |
| ------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Tauri, Tauri plugins, tauri-build                                   | Apache-2.0 OR MIT                                                    |
| React, React DOM                                                    | MIT                                                                  |
| TanStack Query                                                      | MIT                                                                  |
| Zustand                                                             | MIT                                                                  |
| react-hook-form, Zod                                                | MIT                                                                  |
| i18next, react-i18next                                              | MIT                                                                  |
| Tailwind CSS, tailwind-merge                                        | MIT                                                                  |
| shadcn/ui generated Radix components, radix-ui and Radix primitives | MIT                                                                  |
| class-variance-authority, Sonner, tw-animate-css                    | Apache-2.0 (CVA), MIT (Sonner and tw-animate-css)                    |
| Lucide React                                                        | ISC                                                                  |
| Tokio, Serde, SQLx, reqwest, url, uuid, chrono, tracing             | MIT and/or Apache-2.0 as identified in Cargo.lock metadata           |
| jsonc-parser, toml_edit, portable-pty, sysinfo, directories         | Permissive licenses identified in Cargo.lock metadata                |
| SQLite bundled through SQLx/system SQLite                           | Public-domain SQLite terms; wrapper crates retain their own licenses |

## UI components

The sources in `src/components/ui/primitives` were generated from the official shadcn/ui Radix
registry using shadcn 4.16.2 (`radix-nova`, neutral, Lucide), then adapted for CLISwitch's tokens,
Radix state attributes, localized dialog labels, and notification integration. No CLILoom
application code or artwork is included. The shadcn CLI is a development-only dependency.

The MIT notice applies to the generated shadcn/ui sources and these runtime libraries:

- shadcn/ui: Copyright (c) 2023 shadcn.
- radix-ui / Radix primitives: Copyright (c) 2022 WorkOS.
- Sonner: Copyright (c) 2023 Emil Kowalski.
- tw-animate-css: Copyright (c) 2025 Wombosvideo.

```text
MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

`class-variance-authority` 0.7.1 is Apache-2.0 licensed; its license is distributed in the
package's `LICENSE` file. No third-party blocks, online fonts, or remote runtime UI assets are used.

## Bundled CLIAdapter data and templates

`src-tauri/catalog/providers.json` is a generated snapshot of the CLIAdapter provider database from
<https://laurentwu.github.io/CLIAdapter/providers.json>. CLIAdapter is Copyright (c) 2026 Laurent
Wu and is distributed under the MIT License. The snapshot is data only; CLISwitch does not bundle
or execute remote code from it. See the upstream project at
<https://github.com/laurentwu/CLIAdapter>.

`src-tauri/catalog/config-templates` preserves the allowlisted configuration templates and license
from CLIAdapter commit `7ea4dcc5e874d76a14e54a8e15f4fec7b8c5522d`. The original MIT text is
included at [src-tauri/catalog/config-templates/LICENSE](src-tauri/catalog/config-templates/LICENSE).
These files are parsed as local data and are never executed.

Build and test-only tools—including Vite, TypeScript, ESLint, Prettier, Vitest, WebdriverIO, and the WDIO Tauri plugins—are not intentionally shipped as production frontend code. They retain their respective permissive licenses. The dedicated E2E binary is not a release artifact.

## Assets and product names

The CLISwitch application icon is an original project asset released under the repository's
Apache-2.0 license. The CLI identity icons under `src/assets/cli` are official upstream assets
bundled only to identify compatible products:

- The Claude Code icon is from the official Claude Code documentation asset at
  <https://code.claude.com/docs/en/overview> (the exact bundled file has SHA-256
  `d4e5e8e9990ac52f147a74ae80b9e69cdc612264b84a76cdb5c43b53b73a1707`). Claude and
  Claude Code are trademarks of Anthropic; the icon remains subject to Anthropic's applicable
  terms.
- The Codex icon is the OpenAI mark shipped by the official Codex VS Code extension, version
  `26.5908.31748`, from
  <https://marketplace.visualstudio.com/items?itemName=openai.chatgpt>. The mark remains the
  property of OpenAI and is used according to the [OpenAI design guidelines](https://openai.com/brand/).
- The light and dark OpenCode icons are official brand assets from the OpenCode repository at
  commit `cf494c2029d4334dfe6defc31209341ca97c2e94`; see
  <https://opencode.ai/brand>. The upstream repository is MIT licensed, while OpenCode names and
  marks remain the property of their respective owner.
- The Qwen Code icon is from the Apache-2.0-licensed Qwen Code repository at commit
  `74c0916a3ce21057abddeb435851edf0f9f2235d`; Qwen names and marks remain the property of
  their respective owner. See <https://github.com/QwenLM/qwen-code>.

These third-party icons are not original CLISwitch assets and are not relicensed under the
CLISwitch Apache-2.0 license. All product names and marks remain the property of their respective
owners. Their use describes compatibility and does not imply affiliation, sponsorship, or
endorsement.

For full license texts, consult each package source distributed in the package manager cache or its upstream repository. Apache-2.0 text for CLISwitch is in [LICENSE](LICENSE).
