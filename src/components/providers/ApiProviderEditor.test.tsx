import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ApiProviderDetail, ProviderCatalog } from "../../shared/types";
import { ApiProviderEditor } from "./ApiProviderEditor";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({
  command: commandMock,
  errorMessage: (error: unknown) => String(error),
}));

const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [],
  providerTemplates: [
    {
      mode: "api",
      id: "glm-coding-plan",
      name: "GLM Coding Plan",
      category: "coding-plan",
      credentialSlots: [{ id: "api-key", name: "Coding Plan API Key" }],
      endpoints: [
        ["anthropic", "Anthropic Messages", "anthropic-messages", "https://example.test/anthropic"],
        ["openai-chat", "OpenAI Chat Completions", "openai-chat", "https://example.test/chat"],
        ["openai-responses", "OpenAI Responses", "openai-responses", "https://example.test/v1"],
      ].map(([id, name, protocol, baseUrl]) => ({
        id,
        name,
        protocol: protocol as "anthropic-messages" | "openai-chat" | "openai-responses",
        baseUrl,
        credentialSlotId: "api-key",
        authOptions: [{ id: "bearer", authType: "bearer" as const }],
        defaultAuthOptionId: "bearer",
        models: [{ id: "glm-suggested", name: "GLM Suggested", default: true }],
      })),
    },
  ],
  relations: [],
};

describe("ApiProviderEditor", () => {
  beforeEach(() => commandMock.mockReset());

  it("expands a provider template into all endpoints and one shared credential input", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApiProviderEditor providers={[]} catalog={catalog} onClose={vi.fn()} onError={vi.fn()} />
      </QueryClientProvider>,
    );

    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "glm-coding-plan" },
    });

    expect(screen.getByRole("heading", { name: "Anthropic Messages" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "OpenAI Chat Completions" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "OpenAI Responses" })).toBeInTheDocument();
    expect(screen.getAllByRole("textbox", { name: /Coding Plan API Key/ })).toHaveLength(1);
    expect(screen.getAllByDisplayValue("glm-suggested")).toHaveLength(3);
  });

  it("keeps fetched model suggestions with their connection after removing an earlier row", async () => {
    const detail: ApiProviderDetail = {
      id: "provider-1",
      name: "Custom provider",
      profileType: "api",
      revision: 1,
      createdAt: "2026-08-23T00:00:00Z",
      updatedAt: "2026-08-23T00:00:00Z",
      connections: ["first", "second"].map((id) => ({
        id,
        credentialSlotId: `key-${id}`,
        protocol: "openai-responses" as const,
        endpoint: `https://${id}.example.test/v1`,
        authType: "bearer" as const,
        apiKey: `secret-${id}`,
        defaultModel: `default-${id}`,
        verification: { status: "never-tested" as const },
      })),
    };
    commandMock.mockImplementation((_name: string, args?: Record<string, unknown>) =>
      Promise.resolve([`fetched-${String(args?.connectionId)}`]),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApiProviderEditor
          detail={detail}
          providers={[]}
          catalog={catalog}
          onClose={vi.fn()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const fetchButtons = screen.getAllByRole("button", { name: "获取模型" });
    fireEvent.click(fetchButtons[0]);
    fireEvent.click(fetchButtons[1]);
    await waitFor(() => {
      expect(
        document.querySelector('datalist#models-1 option[value="fetched-second"]'),
      ).not.toBeNull();
    });

    fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]);

    expect(
      document.querySelector('datalist#models-0 option[value="fetched-second"]'),
    ).not.toBeNull();
    expect(document.querySelector('datalist#models-0 option[value="fetched-first"]')).toBeNull();
  });
});
