import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import { createTauriCapabilities } from "@wdio/tauri-service";
import type {} from "@wdio/types";

const e2eRoot = mkdtempSync(join(tmpdir(), "cliswitch-e2e-"));
const fixtureBin = join(e2eRoot, "bin");
const home = join(e2eRoot, "home");
const roamingAppData = join(home, "AppData", "Roaming");
const localAppData = join(home, "AppData", "Local");

// WDIO runs config-level onComplete hooks before service teardown. Defer the
// cleanup until the launcher exits so Windows has released the app's database.
process.once("exit", () => {
  try {
    rmSync(e2eRoot, {
      recursive: true,
      force: true,
      maxRetries: 5,
      retryDelay: 100,
    });
  } catch {
    // Temporary-directory cleanup must not replace the test result.
  }
});

// Windows resolves Tauri's app data directory through the Known Folder API,
// which rejects overridden APPDATA paths when their directory tree is missing.
for (const directory of [fixtureBin, home, roamingAppData, localAppData]) {
  mkdirSync(directory, { recursive: true });
}

const isWindows = process.platform === "win32";
const fixture = resolve(isWindows ? "e2e/fixtures/fake-cli.ps1" : "e2e/fixtures/fake-cli.sh");
for (const command of ["claude", "codex", "opencode"]) {
  const destination = join(fixtureBin, `${command}${isWindows ? ".ps1" : ""}`);
  copyFileSync(fixture, destination);
  if (!isWindows) chmodSync(destination, 0o700);
}

const cargoTarget = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : resolve("src-tauri/target");
const appBinaryPath = join(cargoTarget, "debug", `cliswitch${isWindows ? ".exe" : ""}`);
const appEnvironment: Record<string, string> = {
  HOME: home,
  USERPROFILE: home,
  PATH: `${fixtureBin}${delimiter}${process.env.PATH ?? ""}`,
  CLAUDE_CONFIG_DIR: join(home, ".claude"),
  CODEX_HOME: join(home, ".codex"),
  XDG_CONFIG_HOME: join(home, ".config"),
  XDG_DATA_HOME: join(home, ".local", "share"),
  APPDATA: roamingAppData,
  LOCALAPPDATA: localAppData,
};

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        driverProvider: "embedded",
        env: appEnvironment,
        captureBackendLogs: true,
        captureFrontendLogs: true,
      },
    ],
  ],
  capabilities: [createTauriCapabilities(appBinaryPath)],
  framework: "mocha",
  reporters: ["spec"],
  waitforTimeout: 15_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  mochaOpts: { ui: "bdd", timeout: 60_000 },
};
