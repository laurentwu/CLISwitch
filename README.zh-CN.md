# CLISwitch

CLISwitch 是一个基于 Tauri 2 的本地桌面应用，用于查看、保存并安全切换 Claude Code、Codex CLI 与 OpenCode 的用户级配置。0.1 面向 Windows、macOS 和 Linux，不支持移动端。

> **凭据警告：** CLISwitch 有意以明文保存并显示 API Key 和 OAuth 内容。文件权限不等于加密。录入凭据前请阅读 [SECURITY_MODEL.md](SECURITY_MODEL.md)。

[English](README.md) · [CLI 兼容矩阵](CLI_SUPPORT.md) · [安全模型](SECURITY_MODEL.md)

## 0.1 能力

- 仅保留“配置、供应商、设置”三个顶层入口。
- 配置区以“当前配置 / 命名配置 / ＋”横向并排展示。
- 支持自动发现，以及手工指定 CLI 可执行文件和配置目录。
- 端点 + Key 供应商可配置 OpenAI Chat Completions、OpenAI Responses 和 Anthropic Messages 接入方式。
- Provider 与模型模板来自随软件内置的完整 [models.dev](https://models.dev) JSON 快照；应用使用私有本地缓存，并可在设置页手动更新。
- 所有上游 Provider 都按名称和 ID 显示；只有通过固定协议、端点和安全策略校验的 Provider 可选，应用不会加载或执行数据库中的 npm 包。
- 只按 Provider 级别映射传输协议，模型按 ID/名称提供建议并隐藏已弃用项；仍可手工输入模型 ID，也保留自定义 Provider。
- Claude Code 支持 Anthropic OAuth，Codex CLI 支持 Codex OAuth；可调用已安装的官方 CLI 登录，也可离线导入 auth 文件。
- API Key 与 OAuth 原文均完整明文显示、复制和编辑；OAuth 原文允许保存任意 UTF-8 内容，即使格式已损坏。
- 应用前预览、摘要冲突检测、按 CLI 顺序执行、原子替换、写后验证、失败回滚、取消，以及只重试失败项。
- 含凭据的备份与 tombstone 恢复；每个源文件仅保留最近 5 份。
- 中英文、浅色/深色/跟随系统主题、单实例与窗口状态保存。
- 无遥测、无自动更新、无远程脚本、无通用 shell/文件系统 IPC。

具体兼容版本、路径和字段映射见 [CLI_SUPPORT.md](CLI_SUPPORT.md)。CLISwitch 只管理用户级配置；环境变量、项目配置或企业策略仍可能覆盖它。

## 开发

项目固定使用 Node.js 24、pnpm 11 与 Rust 1.88 大版本。macOS 最低构建/运行基线为 12。先安装当前操作系统对应的 [Tauri 2 前置依赖](https://v2.tauri.app/start/prerequisites/)，再运行：

```bash
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

可运行 `pnpm catalog:update` 更新仓库内置的 models.dev 发布快照。脚本只访问固定上游地址，
校验 Provider/模型 ID，并生成摘要元数据。

常用验证命令：

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

`pnpm test:e2e` 会生成独立的自动化测试 Debug 构建。WDIO 插件、全局 Tauri API 和测试权限不会进入正常生产构建。测试会创建临时 HOME/XDG/APPDATA，并只在其中放入假 CLI，绝不读取或修改真实用户配置。Linux 无界面 CI 应通过 `xvfb-run -a pnpm test:e2e` 运行。

## 构建与发布

应用标识为 `io.github.laurentwu.cliswitch`。生产产物包括 Windows NSIS、macOS DMG、Linux AppImage 与 deb。0.1 不做代码签名或公证，因此 Windows SmartScreen 和 macOS Gatekeeper 可能提示风险。版本只通过 GitHub Releases 手工发布；应用内更新检查仅比较版本并打开 Releases 页面，不会自动下载或执行任何文件。

项目采用 Apache-2.0 许可证。依赖与素材声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
