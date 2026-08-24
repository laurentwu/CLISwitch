import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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
      candidateId: "00000000-0000-4000-8000-000000000002",
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

    fireEvent.click(screen.getByRole("button", { name: "保存未纳管供应商" }));
    expect(screen.getByRole("dialog", { name: "保存未纳管供应商" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "名称" })).toHaveValue("Codex OAuth");
  });
});
