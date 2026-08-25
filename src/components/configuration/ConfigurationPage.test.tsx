import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { AppSnapshot } from "../../shared/types";
import { ConfigurationPage } from "./ConfigurationPage";

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

describe("ConfigurationPage", () => {
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
});
