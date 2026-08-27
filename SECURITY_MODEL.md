# CLISwitch security model

## Trust boundary

CLISwitch is a single-user desktop configuration manager. Its trusted components are the packaged local WebView frontend and the Rust backend. It does not expose a network server, accept remote WebView content, load remote scripts, provide generic command execution, or provide arbitrary filesystem IPC. Production Content Security Policy denies network access from the WebView. Backend network requests are limited to user-triggered model listing/connection tests, a manual GitHub release check, and a manual provider-database download from the fixed `https://models.dev/api.json` URL; official CLI child processes perform OAuth login.

The models.dev document is untrusted data. Downloads reject redirects, enforce a bounded body,
validate provider/model identity and URL structure, and become active only after a successful
private atomic cache write. npm package names are matched against fixed built-in adapters; no
package, script, request header, or model-level endpoint override from the document is executed.
Provider endpoints require HTTPS, except HTTP loopback addresses used by local model servers.

CLISwitch is not a secret vault, sandbox, malware defense, or enterprise policy bypass. A process running as the same OS user, an administrator, malware, backup software, or a debugging/instrumentation tool can read its secrets.

## Intentional plaintext design

The product decision for 0.1 is that every credential is easy to use and therefore shown in full plaintext:

- API keys are stored in SQLite and returned only by ID-scoped secret-detail IPC.
- OAuth source content is stored in private application auth files and returned only by ID-scoped secret-detail IPC.
- Applying a configuration can place credentials in the target CLI's own user-level files.
- Backups intentionally contain the original credential-bearing bytes needed for recovery.
- Opening a provider detail places the full secret in WebView memory. Leaving the detail removes its short-lived query cache, but this is not guaranteed secure memory erasure.
- Copying a secret places it on the operating-system clipboard. CLISwitch does not promise clipboard isolation or secure clearing.

Saving any secret requires an explicit risk acknowledgement. POSIX mode restrictions and Windows per-user access control reduce accidental cross-user access; they are not encryption. CLISwitch does not use an OS keychain in 0.1.

## Residual data

SQLite uses WAL mode. Secrets may remain in the database, WAL/journal pages, filesystem free space, crash dumps, logs produced by other software, system backups, virtual-machine snapshots, cloud-synced folders, or storage snapshots. Rotation and ordinary deletion are not cryptographic erasure. Secure disposal depends on the operating system, filesystem, storage encryption, retention system, and the user's own backup policy.

Application logs and IPC events are designed to carry metadata only. Known credentials and common token forms are redacted again before diagnostic text is emitted. Public provider DTOs never include API keys or OAuth source. This reduces accidental disclosure but is not a proof that arbitrary third-party error text can never contain sensitive data.

## OAuth

CLISwitch does not own or embed a third-party OAuth client. It launches the installed official `claude` or `codex` executable with fixed arguments in an isolated temporary home, captures the resulting user credential artifact, and cleans up session-owned temporary data. Renderer input cannot select the executable, arguments, environment-variable names, or temporary path.

On Linux and Windows, Claude authentication is managed as file content. On macOS, CLISwitch uses the official Claude `setup-token` flow and does not read the macOS Keychain. Codex uses its file auth store. OAuth imports perform local structural recognition only and are marked “not online verified.” Stable account identity is used when available to distinguish refreshes from an external login.

The raw OAuth editor deliberately accepts any UTF-8 text, including empty or malformed content. Saving marks it user-modified and unverified; applying writes those bytes without schema validation and may make the target CLI unable to sign in. Re-login or re-import is the recovery path.

## Safe writes and limits

- Only the documented user-level files listed in [CLI_SUPPORT.md](CLI_SUPPORT.md) are managed.
- Canonical target containment and symlink resolution are checked before writes.
- A preview records source digests; changed sources become conflicts instead of being overwritten.
- Replacement is atomic where supported, multi-file failures roll back already-written files, and the result is verified.
- Credential backups are private, digest-checked, bounded to five versions per source, and can represent “file did not exist” tombstones.
- OAuth switching is blocked when the target CLI is running or its running state cannot be determined reliably. Other CLI items continue.
- CLISwitch waits for the current atomic unit, cancels outstanding OAuth/apply work, and then exits when the user confirms a protected close.

User-level configuration may still be overridden by process environment, a project configuration file, managed/enterprise policy, or a newer CLI schema. CLISwitch reports known overrides and refuses unknown managed shapes instead of rewriting the whole file, but it cannot enumerate every policy mechanism.

## Desktop production boundary

Production builds have DevTools disabled, a local-only CSP, a minimal Tauri capability, and no WDIO automation plugin. Desktop E2E uses a separate `e2e` Cargo feature, inline test-only capability, and Vite `e2e` mode. Those features are absent from normal builds. The application supports only Windows, macOS, and Linux.

Report suspected vulnerabilities privately through the repository's GitHub security reporting channel. Do not include real credentials in an issue, screenshot, fixture, or log.
