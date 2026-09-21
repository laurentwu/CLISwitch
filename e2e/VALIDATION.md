# UI migration validation — 2026-09-20

## Environment and boundaries

Validated on Linux with Node.js 24.19.0, pnpm 11.21.0, Rust 1.88.0, Chrome and WebKitGTK
2.52.6. All rendered data and CLI files were fixtures in temporary directories. No real user
configuration, credentials, provider catalog updates, migrations, IPC contracts, or Tauri
capabilities were changed.

The four planning reference PNGs were available and inspected alongside implementation
screenshots. The Browser connector was unavailable, so rendering checks used a temporary
Playwright harness against Vite; its fake IPC lived outside the application sources. Desktop
checks used the existing isolated WDIO/Tauri fixture environment.

## Checks

- Frozen dependency installation, formatting, lint, TypeScript, frontend tests, Rust formatting,
  Clippy, and Rust tests passed. Frontend: 98 passed across 22 files. Rust: 227 passed, one existing
  ignored platform test.
- Production frontend build and production-boundary check passed. The frontend build reports a
  non-fatal chunk-size warning. Linux AppImage and deb packaging succeeded.
- The normal `xvfb-run -a pnpm test:e2e` was executed, but its embedded driver failed with
  `Unsupported result type` before meaningful interaction checks could run.
- Supplemental desktop validation used temporary tauri-driver 2.0.6 and WebKitWebDriver 2.52.6,
  a separate embedded-driver port, and software compositing. All six tests in `appearance.e2e.ts`
  and `navigation.e2e.ts` passed. Native `sendKeys` also failed on a plain, non-React input in this
  environment, so this supplemental run populated fixture text through the native input value
  setter plus a bubbling input event. Clicking, Select interaction, Escape, focus restoration,
  resizing, backend IPC, apply and restore remained real. This is not a pass for the unmodified
  default-driver suite or native typing.
- No Windows/macOS WebView or native file-picker smoke test was available. Those remain release
  validation requirements.

## Render and interaction matrix

Current and named configurations, API/OAuth editors, Settings, file preview, and startup failure
were rendered in both languages and both explicit themes at the default window size. System
appearance changes were checked in both directions. The dense configuration/provider/settings
pages were checked at 1180×780, 900×620, and 1536×1024 with all nine zoom steps from 100% to 300%.
Normal page content had no horizontal overflow. Dialog/notification interaction was additionally
checked at default/minimum window sizes and 100%, 200%, and 300% equivalent CSS viewports.
The final rendering run recorded 62 checked screenshots with no page overflow or browser errors,
plus full-height Settings captures. At low window heights notifications join the dialog's scroll
flow so their close/action controls remain reachable.

| Review item          | Result                                                                                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Layout and density   | Left navigation, left-aligned configuration tabs, CLI rows, provider split view and six Settings sections match the specified structure.                |
| Colors               | Paired neutral theme tokens; state and destructive colors are semantic. No decorative gradients were introduced.                                        |
| Text hierarchy       | 24px page titles, 16px section headings, 14px body and controls, 12px secondary/path text.                                                              |
| Icons                | Existing Lucide navigation/actions and CL/CO/OP/QW marks retained.                                                                                      |
| Spacing              | 184px sidebar, 28px normal content padding, 236px provider list; responsive 60px icon navigation.                                                       |
| Control states       | Radix selected/checked state selectors corrected; disabled, invalid, focus, loading, empty and alert states covered by rendering and interaction tests. |
| Complete information | Full credential warning, connection metadata, references, diagnostics, file diffs and unmanaged-provider actions retained.                              |
| Narrow viewports     | Wrapping actions, single-column forms, local tab/code scrolling, non-sticky short-window headers and scrollable dialogs keep controls reachable.        |

Calculated token contrast: normal/state text pairs are at least 4.79:1; input boundaries against
the page background are at least 3.23:1. Semantic foreground/background pairs remain intact in
alerts and diff views.

Temporary evidence for this session is under `/tmp/cliswitch-visual-qKHOQY/`: implementation PNGs,
`report.json`, `e2e-verified.log`, frontend/Rust test logs and packaging logs. These files are not
product assets and may be cleared with the temporary directory.

## PR review follow-up — 2026-09-20

- The macOS CI delay was in the Tauri service's automatic window-state query after refresh,
  not in saving the theme. The suite now explicitly selects the application's only window,
  `main`, which suppresses that automatic query. Existing timeouts are unchanged.
- Refresh checks now mark the outgoing document, wait for its replacement, and then wait for
  the application's main heading. The old page cannot satisfy the saved-theme assertion.
- Formatting, lint, the project's TypeScript check, 100 frontend tests, Rust formatting,
  Clippy, and 227 Rust tests passed; the existing Qwen binary smoke test remained ignored.
  The production frontend build and production-boundary check also passed.
- In-memory checks of the actual refresh helper covered delayed document replacement,
  transient element lookup errors, and a reload that never replaces the document. A separate
  check of the installed Tauri service confirmed explicit window selection bypasses a stalled
  automatic window-state query. These checks do not substitute for native WebView validation.
- The Linux E2E debug build passed. The default embedded-driver run still failed all six
  tests with the previously recorded `Unsupported result type`, before reaching the refresh
  checks. No alternate driver or typing workaround was used for this follow-up.
- An additional strict typecheck of the E2E sources reported four existing diagnostics in
  `navigation.e2e.ts`; an in-memory comparison against the pre-fix commit confirmed the same
  diagnostics before and after this patch. The new helper passed its strict typecheck.
- Native macOS and Windows verification of this follow-up remains for CI.

## Linux E2E and typecheck resolution — 2026-09-20

The two limitations above were addressed in a subsequent, separately approved fix:

- A controlled Linux/Xvfb comparison reproduced `Unsupported result type` with the default
  WebKit DMA-BUF path. Changing only `WEBKIT_DISABLE_DMABUF_RENDERER=1` made the theme test
  pass in 2.6 seconds; removing it reproduced the failure. The Linux E2E child process now
  defaults to this fallback, honors an explicit override, and leaves production rendering
  and other platforms unchanged.
- The complete `xvfb-run -a pnpm test:e2e` build/run passed: all six desktop tests completed
  in a 38-second test phase with the existing embedded driver and normal WebDriver input.
  No external driver, input replacement, dependency update, or timeout increase was needed.
- The four E2E type errors were fixed with string type guards, an awaited collection length,
  and a resolved element array. `tsconfig.e2e.json` is now included by the standard
  `pnpm typecheck` and build commands, so E2E sources are no longer omitted from typechecking.
- Formatting, lint, typechecking, 100 frontend tests, Rust formatting, Clippy, and 227 Rust
  tests passed (one existing Qwen binary smoke test ignored). The production frontend build
  and production-boundary check also passed.
- Configuration checks confirmed the Linux-only default, preservation of explicit overrides,
  and unchanged test timeout/driver. The service still emits diagnostics about absent external
  drivers, which the configured embedded provider does not require; these did not fail the run.
- macOS and Windows verification remains for CI; GPU-backed Linux rendering was not validated
  by this headless run.
