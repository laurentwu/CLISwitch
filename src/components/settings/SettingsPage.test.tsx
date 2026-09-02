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
  cachePath: "/tmp/cliswitch/providers.json",
  metadataPath: "/tmp/cliswitch/providers.meta.json",
  fetchedAt: null,
  etag: null,
  digest: "bundled-digest",
  providerCount: 7,
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
      providerCount: 8,
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

    expect(await screen.findByText("7 个 provider")).toBeInTheDocument();
    expect(screen.queryByText(/models\.dev|数据来自/)).not.toBeInTheDocument();
    expect(screen.getByText(/当前来源/)).toHaveTextContent("内置快照");

    fireEvent.click(screen.getByRole("button", { name: "更新数据库" }));

    await waitFor(() => {
      expect(screen.getByText("8 个 provider")).toBeInTheDocument();
      expect(screen.getByText(/当前来源/)).toHaveTextContent("本地缓存");
      expect(screen.getByRole("status")).toHaveTextContent("Provider 数据库已更新");
    });
    expect(commandMock).toHaveBeenCalledWith("update_catalog");
  });
});
