import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ProviderCatalog, ScanSnapshot } from "../../shared/types";
import { CurrentConfigurationTab } from "./CurrentConfigurationTab";

const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [],
  providerTemplates: [
    {
      mode: "api",
      id: "glm-coding-plan",
      name: "GLM Coding Plan",
      category: "coding-plan",
      credentialSlots: [],
      endpoints: [],
    },
  ],
  relations: [],
};

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
  it("labels successful scan diagnostics without calling the scan a failure", () => {
    const client = new QueryClient();
    const diagnostic =
      "OpenCode has multiple configured models and no explicit or valid last-used model";
    const scan: ScanSnapshot = {
      ...codexOAuthScan,
      items: [
        {
          ...codexOAuthScan.items[0],
          current: {
            ...codexOAuthScan.items[0].current!,
            diagnostics: [diagnostic],
          },
        },
      ],
    };
    render(
      <QueryClientProvider client={client}>
        <CurrentConfigurationTab
          scan={scan}
          configurations={[]}
          providers={[]}
          catalog={catalog}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByText("扫描诊断")).toBeInTheDocument();
    expect(screen.getByText(diagnostic)).toBeInTheDocument();
    expect(screen.queryByText("扫描失败")).not.toBeInTheDocument();
  });

  it("offers to save detected Codex OAuth with an OAuth-specific default name", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <CurrentConfigurationTab
          scan={codexOAuthScan}
          configurations={[]}
          providers={[]}
          catalog={catalog}
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
              suggestedName: "GLM Coding Plan",
              templateId: "glm-coding-plan",
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
        <CurrentConfigurationTab
          scan={scan}
          configurations={[]}
          providers={[]}
          catalog={catalog}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("button", { name: "将 GLM Coding Plan 保存为供应商" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "将 Custom gateway 保存为供应商" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "将 GLM Coding Plan 保存为供应商" }));
    const dialog = screen.getByRole("dialog", { name: "保存未纳管供应商" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue("GLM Coding Plan");
    expect(within(dialog).getByRole("combobox", { name: /默认模型/ })).toHaveValue("glm-current");
    expect(within(dialog).getByText("zhipuai-coding-plan")).toBeInTheDocument();
    expect(within(dialog).getByText("GLM Coding Plan")).toBeInTheDocument();
    expect(within(dialog).queryByText("glm-coding-plan")).not.toBeInTheDocument();
  });
});
