import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { AppSnapshot, PublicProvider, SavedConfiguration } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { ConfigurationPage } from "./ConfigurationPage";

const commandMock = vi.hoisted(() => vi.fn());
const onEventMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock, onEvent: onEventMock }));

const snapshot: AppSnapshot = {
  catalog: {
    schemaVersion: 1,
    clis: [
      {
        id: "claude-code",
        name: "Claude Code",
        protocols: ["anthropic-messages"],
        authModes: [],
        protocolAdapters: [],
      },
      {
        id: "codex",
        name: "Codex CLI",
        protocols: ["openai-responses"],
        authModes: [],
        protocolAdapters: [],
      },
      {
        id: "opencode",
        name: "OpenCode",
        protocols: ["openai-responses", "openai-chat", "anthropic-messages"],
        authModes: [],
        protocolAdapters: [],
      },
    ],
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
  current: {
    id: "scan-1",
    generatedAt: "2026-08-23T00:00:00Z",
    items: [
      {
        cliId: "claude-code",
        label: "Claude Code",
        status: "detected",
        executablePath: "/usr/bin/claude",
        configDirectory: "/tmp/.claude",
        version: "1.0.0",
        source: "hidden-discovery-source",
        current: {
          providerName: "测试供应商",
          protocol: "anthropic-messages",
          authKind: "api-key",
          model: "claude-test",
          sources: [],
          externallyOverridden: false,
          diagnostics: [],
        },
      },
    ],
  },
  latestApply: null,
  configurationStatuses: {},
  appDataDirectory: "/tmp/cliswitch",
  backupBytes: 0,
  appVersion: "0.1.0",
};

const applyConfiguration: SavedConfiguration = {
  id: "configuration-apply",
  name: "Apply configuration",
  creationOrder: 1,
  revision: 1,
  targets: [
    {
      targetType: "api",
      cliId: "codex",
      providerId: "provider-apply",
      connectionId: "connection-apply",
      model: "model-a",
    },
  ],
  createdAt: "2026-08-23T00:00:00Z",
  updatedAt: "2026-08-23T00:00:00Z",
};

const applyProvider: PublicProvider = {
  id: "provider-apply",
  name: "Apply provider",
  kind: "api",
  templateId: undefined,
  connections: [
    {
      id: "connection-apply",
      templateEndpointId: undefined,
      credentialSlotId: "api-key",
      protocol: "openai-responses",
      endpoint: "https://example.test/v1",
      authType: "bearer",
      defaultModel: "model-a",
      verification: { status: "never-tested" },
    },
  ],
  referencedBy: [],
  revision: 1,
  updatedAt: "2026-08-23T00:00:00Z",
};

describe("ConfigurationPage", () => {
  beforeEach(() => {
    commandMock.mockReset();
    onEventMock.mockReset();
    useUiStore.setState({
      configurationId: "current",
      dirty: false,
      saveCurrent: undefined,
    });
  });

  it("omits the CLI subtitle and discovery source details", () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Infinity } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <ConfigurationPage snapshot={snapshot} guarded={(action) => action()} onError={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(screen.getByRole("heading", { name: "配置", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("测试供应商")).toBeInTheDocument();
    expect(screen.queryByText("Claude Code · Codex CLI · OpenCode")).not.toBeInTheDocument();
    expect(screen.queryByText("发现来源")).not.toBeInTheDocument();
    expect(screen.queryByText("hidden-discovery-source")).not.toBeInTheDocument();
  });

  it("keeps cached content visible when background refreshes fail", async () => {
    commandMock.mockImplementation(async (name: string) => {
      if (name === "list_configurations") {
        throw Object.assign(new Error("temporarily offline"), { code: "network" });
      }
      if (name === "list_providers") return snapshot.providers;
      return snapshot.current;
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: 0 } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <ConfigurationPage snapshot={snapshot} guarded={(action) => action()} onError={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(screen.getByText("测试供应商")).toBeInTheDocument();
    expect(await screen.findByText("无法刷新配置列表")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveClass("alert-warning");
  });

  it("keeps the apply progress dialog open when saving remounts the keyed tab", async () => {
    const saved = { ...applyConfiguration, revision: 2 };
    const run = {
      id: "run-apply",
      previewId: "preview-apply",
      configurationId: saved.id,
      startedAt: "2026-08-23T00:00:00Z",
      finishedAt: "2026-08-23T00:00:01Z",
      cancelRequested: false,
      items: [{ cliId: "codex" as const, state: "success" as const, message: null }],
    };
    commandMock.mockImplementation(async (name: string) => {
      if (name === "update_configuration") return saved;
      if (name === "apply_configuration") return run;
      if (name === "list_configurations") return [applyConfiguration];
      if (name === "list_providers") return [applyProvider];
      if (name === "scan_clis") return applySnapshot.current;
      return undefined;
    });
    onEventMock.mockResolvedValue(() => undefined);
    useUiStore.setState({ configurationId: applyConfiguration.id, dirty: true });
    const applySnapshot = {
      ...snapshot,
      configurations: [applyConfiguration],
      providers: [applyProvider],
    };
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Infinity } },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <ConfigurationPage
          snapshot={applySnapshot}
          guarded={(action) => action()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "应用配置" }));
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("apply_configuration", {
        configurationId: saved.id,
        expectedRevision: saved.revision,
      }),
    );
    expect(screen.getByRole("heading", { name: "应用进度" })).toBeInTheDocument();
  });
});
