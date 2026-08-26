# 项目协作指引

## 项目范围

CLISwitch 是本地 desktop configuration manager，使用 React/Vite frontend 与 Tauri 2/Rust
backend，负责发现、保存、预览、切换和恢复用户级 CLI configuration。

不要在本文件中假定某个特定 CLI；具体兼容范围与 schema 以 [CLI support baseline](CLI_SUPPORT.md)
为准。项目不覆盖 project-level configuration、environment variables、enterprise policy 或
mobile platforms。

## Source of truth

- `README.md` / `README.zh-CN.md`：项目能力、开发和发布流程。
- `CLI_SUPPORT.md`：兼容性契约、field mappings 和已知限制。
- `SECURITY_MODEL.md`：trust boundary、credential handling 和安全限制。
- `src-tauri/catalog/README.md` 及其 JSONC 文件：bundled compatibility catalog；保持 persisted
  IDs 稳定，新增 template 时同步维护 relations。

## 目录与架构边界

- `src/`：React UI、i18n、state 和 typed IPC client。
- `src-tauri/src/`：Rust domain、services、adapters 和 commands。
- `src-tauri/catalog/`：随应用打包且只读的 JSONC catalog；用户 provider instances 保存在 SQLite。
- `src-tauri/migrations/`：SQLite schema migrations；不要改写既有 migration。
- `src/**/*.test.*`、`src-tauri/tests/`、`e2e/`：测试与 fixtures。

通过既有 typed IPC 和 service 边界访问后端、文件与凭据，不要绕过边界直接实现新的通道。

## Security invariants

- plaintext credential design 是有意的；不得把真实凭据写入 logs、commits、screenshots、fixtures
  或测试输出。
- 保留 redaction、ID-scoped secret IPC、path containment、atomic replacement、digest conflict
  checks、backup/rollback 和 cancellation 机制。
- 不新增 generic shell execution、arbitrary filesystem IPC、remote scripts、telemetry 或不必要的
  Tauri capabilities。
- OAuth 的 executable、arguments、environment names 和 temporary paths 仍由 backend 固定控制。

## Change conventions

- 优先做最小且向后兼容的修改；持久化 ID 或 schema 变化必须有 migration 和对应测试。
- 用户可见文案同时维护 `src/i18n/en.ts` 与 `src/i18n/zh-CN.ts`。
- 行为或兼容性契约变化时同步更新 tests 和相关文档；保留未管理的配置字段及其格式。
- 供应商细节只在需要时引用 [PROVIDER_REFERENCES.md](PROVIDER_REFERENCES.md)，不要复制回本文件。

## Toolchain and validation

项目固定使用 Node.js 24、pnpm 11 和 Rust 1.88；依赖安装使用 `pnpm install --frozen-lockfile`。

常规变更至少运行：

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
```

涉及 production boundary、desktop packaging 或 E2E 时，再按需运行
`pnpm check:production-boundary`、`pnpm test:e2e` 和 `pnpm tauri build`。

## External references

供应商文档集中在 [PROVIDER_REFERENCES.md](PROVIDER_REFERENCES.md)，仅在相关集成变更时按需查阅。
