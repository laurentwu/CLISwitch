import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ScanSnapshot } from "../../shared/types";
import { CurrentConfigurationTab } from "./CurrentConfigurationTab";

const codexOAuthScan: ScanSnapshot = {
  id: "00000000-0000-4000-8000-000000000001",
  generatedAt: "2026-08-23T00:00:00Z",
  items: [
    {
      cliId: "codex",
      label: "Codex CLI",
      status: "unmanaged",
      executablePath: "/fixture/bin/codex",
      configDirectory: "/fixture/.codex",
      version: "codex-cli 1.0",
      source: "manual override",
      providerCandidates: [
        {
          id: "00000000-0000-4000-8000-000000000002",
          sourceProviderId: "codex",
          suggestedName: "Codex OAuth",
          availableModels: [],
        },
      ],
      current: {
        providerName: "openai",
        authKind: "oauth",
        model: "gpt-fixture",
        sources: [
          {
            sourceId: "codex-auth",
            displayPath: "/fixture/.codex/auth.json",
            digest: "sha256:fixture",
          },
        ],
        externallyOverridden: false,
        diagnostics: [],
      },
    },
  ],
};

describe("CurrentConfigurationTab", () => {
  it("offers to save detected Codex OAuth with an OAuth-specific default name", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <CurrentConfigurationTab
          scan={codexOAuthScan}
          configurations={[]}
          providers={[]}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "将 Codex OAuth 保存为供应商" }));
    expect(screen.getByRole("dialog", { name: "保存未纳管供应商" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "名称" })).toHaveValue("Codex OAuth");
  });

  it("shows every detected OpenCode provider and lets the user choose its default model", () => {
    const client = new QueryClient();
    const scan: ScanSnapshot = {
      id: "00000000-0000-4000-8000-000000000010",
      generatedAt: "2026-08-23T00:00:00Z",
      items: [
        {
          cliId: "opencode",
          label: "OpenCode",
          status: "unmanaged",
          executablePath: "/fixture/bin/opencode",
          configDirectory: "/fixture/.config/opencode",
          source: "manual override",
          current: {
            providerName: "zhipuai-coding-plan",
            protocol: "openai-chat",
            authKind: "api",
            model: "glm-current",
            sources: [],
            externallyOverridden: false,
            diagnostics: [],
          },
          providerCandidates: [
            {
              id: "00000000-0000-4000-8000-000000000011",
              sourceProviderId: "zhipuai-coding-plan",
              suggestedName: "Zhipu AI Coding Plan",
              protocol: "openai-chat",
              endpoint: "https://open.bigmodel.cn/api/coding/paas/v4",
              authType: "bearer",
              availableModels: ["glm-current", "glm-other"],
              defaultModel: "glm-current",
            },
            {
              id: "00000000-0000-4000-8000-000000000012",
              sourceProviderId: "custom-gateway",
              suggestedName: "Custom gateway",
              protocol: "openai-responses",
              endpoint: "https://gateway.invalid/v1",
              authType: "bearer",
              availableModels: ["custom-model"],
              defaultModel: "custom-model",
            },
          ],
        },
      ],
    };
    render(
      <QueryClientProvider client={client}>
        <CurrentConfigurationTab scan={scan} configurations={[]} providers={[]} onError={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("button", { name: "将 Zhipu AI Coding Plan 保存为供应商" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "将 Custom gateway 保存为供应商" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "将 Zhipu AI Coding Plan 保存为供应商" }));
    const dialog = screen.getByRole("dialog", { name: "保存未纳管供应商" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue(
      "Zhipu AI Coding Plan",
    );
    expect(within(dialog).getByRole("combobox", { name: "默认模型" })).toHaveValue("glm-current");
    expect(within(dialog).getByText("zhipuai-coding-plan")).toBeInTheDocument();
  });
});
