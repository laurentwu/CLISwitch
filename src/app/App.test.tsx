import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { AppSnapshot } from "../shared/types";
import { useUiStore } from "../stores/ui";
import { App } from "./App";

const commandMock = vi.hoisted(() => vi.fn());
const onEventMock = vi.hoisted(() => vi.fn());
vi.mock("../shared/ipc", () => ({ command: commandMock, onEvent: onEventMock }));
vi.mock("../components/configuration/ConfigurationPage", () => ({
  ConfigurationPage: () => <div>Configuration page</div>,
}));
vi.mock("../components/providers/ProviderPage", () => ({
  ProviderPage: () => <div>Provider page</div>,
}));
vi.mock("../components/settings/SettingsPage", () => ({
  SettingsPage: () => <div>Settings page</div>,
}));

const snapshot: AppSnapshot = {
  catalog: { schemaVersion: 1, clis: [], providerTemplates: [], relations: [] },
  settings: {
    language: "zh-cn",
    theme: "system",
    uiZoomPercent: 225,
    scanOnStartup: false,
    plaintextRiskAccepted: false,
    revision: 1,
    manualLocations: [],
  },
  providers: [],
  configurations: [],
  current: null,
  latestApply: null,
  configurationStatuses: {},
  appDataDirectory: "/tmp/cliswitch",
  backupBytes: 0,
  appVersion: "0.1.0",
};

describe("App startup", () => {
  beforeEach(() => {
    commandMock.mockReset();
    onEventMock.mockReset();
    onEventMock.mockResolvedValue(vi.fn());
    useUiStore.setState({
      navigation: "configuration",
      configurationId: "current",
      dirty: false,
      saveCurrent: undefined,
    });
  });

  it("applies the persisted interface zoom after loading the snapshot", async () => {
    commandMock.mockImplementation((name: string) => {
      if (name === "get_startup_status") {
        return Promise.resolve({
          ready: true,
          code: null,
          message: null,
          appDataDirectory: "/tmp/cliswitch",
        });
      }
      if (name === "get_app_snapshot") return Promise.resolve(snapshot);
      if (name === "set_ui_zoom" || name === "set_frontend_dirty") return Promise.resolve();
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("set_ui_zoom", { uiZoomPercent: 225 }),
    );
  });
});
