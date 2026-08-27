import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../../i18n";
import type { AppSnapshot, CatalogStatus } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { SettingsPage } from "./SettingsPage";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

const snapshot: AppSnapshot = {
  catalog: {
    schemaVersion: 1,
    clis: [],
    providerTemplates: [],
    relations: [],
  },
  settings: {
    language: "zh-cn",
    theme: "system",
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

const bundledStatus: CatalogStatus = {
  source: "bundled",
  cachePath: "/tmp/cliswitch/models.dev.json",
  metadataPath: "/tmp/cliswitch/models.dev.meta.json",
  fetchedAt: null,
  etag: null,
  digest: "bundled-digest",
  providerCount: 203,
  modelCount: 7343,
  lastError: null,
  updateAvailable: false,
};

describe("SettingsPage provider database", () => {
  beforeEach(async () => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
    await i18n.changeLanguage("zh-CN");
  });

  it("shows catalog status and replaces it after a successful manual update", async () => {
    const localStatus: CatalogStatus = {
      ...bundledStatus,
      source: "local",
      fetchedAt: "2026-08-26T20:00:00Z",
      etag: "fixture-etag",
      digest: "local-digest",
      providerCount: 204,
      modelCount: 7350,
    };
    commandMock.mockImplementation((name: string) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "update_catalog") return Promise.resolve(localStatus);
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <SettingsPage snapshot={snapshot} onError={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("203 个 provider，7343 个模型")).toBeInTheDocument();
    expect(screen.getByText(/当前来源/)).toHaveTextContent("内置快照");

    fireEvent.click(screen.getByRole("button", { name: "更新数据库" }));

    await waitFor(() => {
      expect(screen.getByText("204 个 provider，7350 个模型")).toBeInTheDocument();
      expect(screen.getByText(/当前来源/)).toHaveTextContent("本地缓存");
      expect(screen.getByRole("status")).toHaveTextContent("Provider 数据库已更新");
    });
    expect(commandMock).toHaveBeenCalledWith("update_catalog");
  });
});
