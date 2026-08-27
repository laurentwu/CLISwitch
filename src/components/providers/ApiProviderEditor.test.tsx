import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ApiProviderDetail, ProviderCatalog } from "../../shared/types";
import { useNotificationStore } from "../../stores/notifications";
import { NotificationViewport } from "../ui";
import { ApiProviderEditor } from "./ApiProviderEditor";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({
  command: commandMock,
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

const modelsDevCatalog: ProviderCatalog = {
  ...catalog,
  providerTemplates: [
    ...catalog.providerTemplates,
    {
      mode: "api",
      id: "dynamic-demo",
      name: "Dynamic Demo",
      category: "models.dev",
      credentialSlots: [{ id: "api-key", name: "API Key" }],
      endpoints: [
        {
          id: "default",
          name: "OpenAI Chat Completions",
          protocol: "openai-chat",
          baseUrl: "https://dynamic.example/v1",
          credentialSlotId: "api-key",
          authOptions: [{ id: "bearer", authType: "bearer" }],
          defaultAuthOptionId: "bearer",
          models: [],
        },
      ],
    },
  ],
  providerInfo: [
    {
      id: "dynamic-demo",
      name: "Dynamic Demo",
      npm: "@ai-sdk/openai-compatible",
      env: ["DYNAMIC_API_KEY"],
      api: "https://dynamic.example/v1",
      doc: "https://dynamic.example/docs",
      protocol: "openai-chat",
      authType: "bearer",
      endpoint: "https://dynamic.example/v1",
      selectable: true,
      supportedClis: ["opencode"],
      models: [
        { id: "dynamic-model", name: "Dynamic Model", selectable: true },
        {
          id: "disabled-model",
          name: "Disabled Model",
          selectable: false,
          disabledReason: "model requires an override",
        },
      ],
    },
    {
      id: "disabled-demo",
      name: "Disabled Demo",
      npm: "unsupported-package",
      env: ["DISABLED_API_KEY"],
      doc: "",
      selectable: false,
      disabledReason: "provider adapter is unsupported",
      supportedClis: [],
      models: [],
    },
  ],
};

describe("ApiProviderEditor", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useNotificationStore.getState().clear();
  });

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

  it("creates a models.dev provider with a provider-level connection and catalog model", async () => {
    commandMock.mockResolvedValue({});
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApiProviderEditor
          providers={[]}
          catalog={modelsDevCatalog}
          onClose={vi.fn()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const templateSelect = screen.getByRole("combobox", { name: /Provider 模板/ });
    const disabledOption = screen.getByRole("option", {
      name: /Disabled Demo \(disabled-demo\)/,
    });
    expect(disabledOption).toBeDisabled();
    expect(disabledOption).toHaveAttribute("title", "provider adapter is unsupported");

    fireEvent.change(templateSelect, { target: { value: "dynamic-demo" } });

    expect(screen.getByRole("combobox", { name: /默认模型/ })).toHaveValue("dynamic-model");
    expect(
      document.querySelector('datalist#models-0 option[value="dynamic-model"]'),
    ).not.toBeNull();
    expect(document.querySelector('datalist#models-0 option[value="disabled-model"]')).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: /API Key/ }), {
      target: { value: "fixture-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("create_provider", expect.anything()),
    );
    const createCall = commandMock.mock.calls.find(([name]) => name === "create_provider");
    const draft = createCall?.[1]?.draft;
    expect(draft.templateId).toBe("dynamic-demo");
    expect(draft.connections).toHaveLength(1);
    expect(draft.connections[0]).toMatchObject({
      protocol: "openai-chat",
      endpoint: "https://dynamic.example/v1",
      defaultModel: "dynamic-model",
    });
    expect(draft.connections[0].templateEndpointId).toBeUndefined();
  });

  it("accepts HTTP endpoints across the IPv4 loopback range", async () => {
    commandMock.mockResolvedValue({});
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApiProviderEditor
          initialDraft={{
            name: "Loopback provider",
            connections: [
              {
                credentialSlotId: "api-key",
                protocol: "openai-chat",
                endpoint: "http://127.0.0.2:11434/v1",
                authType: "bearer",
                apiKey: "fixture-key",
                defaultModel: "fixture-model",
              },
            ],
          }}
          providers={[]}
          catalog={catalog}
          onClose={vi.fn()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("create_provider", expect.anything()),
    );
    expect(commandMock.mock.calls[0]?.[1]?.draft.connections[0].endpoint).toBe(
      "http://127.0.0.2:11434/v1",
    );
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

  it("shows fetched models in the default-model suggestions and reports success in a toast", async () => {
    const detail: ApiProviderDetail = {
      id: "provider-1",
      name: "Custom provider",
      profileType: "api",
      revision: 1,
      createdAt: "2026-08-23T00:00:00Z",
      updatedAt: "2026-08-23T00:00:00Z",
      connections: [
        {
          id: "connection-1",
          credentialSlotId: "api-key",
          protocol: "openai-responses",
          endpoint: "https://example.test/v1",
          authType: "bearer",
          apiKey: "secret",
          defaultModel: "saved-default",
          verification: { status: "never-tested" },
        },
      ],
    };
    commandMock
      .mockResolvedValueOnce(["fetched-first", "fetched-second", "fetched-first"])
      .mockResolvedValueOnce(["fetched-latest"]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApiProviderEditor
          detail={detail}
          providers={[]}
          catalog={catalog}
          onClose={vi.fn()}
          onError={vi.fn()}
        />
        <NotificationViewport />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));

    await waitFor(() => {
      expect(
        document.querySelector('datalist#models-0 option[value="fetched-first"]'),
      ).not.toBeNull();
      expect(
        document.querySelector('datalist#models-0 option[value="fetched-second"]'),
      ).not.toBeNull();
    });
    expect(
      document.querySelectorAll('datalist#models-0 option[value="fetched-first"]'),
    ).toHaveLength(1);
    expect(screen.getByRole("combobox", { name: /默认模型/ })).toHaveValue("saved-default");
    expect(screen.getByRole("status")).toHaveTextContent("获取模型成功");

    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));

    await waitFor(() => {
      expect(
        document.querySelector('datalist#models-0 option[value="fetched-latest"]'),
      ).not.toBeNull();
      expect(document.querySelector('datalist#models-0 option[value="fetched-first"]')).toBeNull();
    });
  });

  it("selects the first fetched model when the default model is empty", async () => {
    const detail: ApiProviderDetail = {
      id: "provider-1",
      name: "Custom provider",
      profileType: "api",
      revision: 1,
      createdAt: "2026-08-23T00:00:00Z",
      updatedAt: "2026-08-23T00:00:00Z",
      connections: [
        {
          id: "connection-1",
          credentialSlotId: "api-key",
          protocol: "openai-responses",
          endpoint: "https://example.test/v1",
          authType: "bearer",
          apiKey: "secret",
          defaultModel: "",
          verification: { status: "never-tested" },
        },
      ],
    };
    commandMock.mockResolvedValue(["fetched-first", "fetched-second"]);
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

    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));

    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: /默认模型/ })).toHaveValue("fetched-first"),
    );
  });
});
