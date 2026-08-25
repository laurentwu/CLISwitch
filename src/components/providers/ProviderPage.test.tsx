import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type {
  ApiProviderDetail,
  AppSnapshot,
  ProviderTemplate,
  PublicProvider,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { ProviderPage } from "./ProviderPage";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({
  command: commandMock,
  onEvent: vi.fn().mockResolvedValue(vi.fn()),
}));

function apiTemplate(id: string, name: string, category: string): ProviderTemplate {
  return {
    mode: "api",
    id,
    name,
    category,
    credentialSlots: [{ id: "api-key", name: "API Key" }],
    endpoints: [
      {
        id: "responses",
        name: `${name} Responses`,
        protocol: "openai-responses",
        baseUrl: `https://${id}.example.test/v1`,
        credentialSlotId: "api-key",
        authOptions: [{ id: "bearer", authType: "bearer" }],
        defaultAuthOptionId: "bearer",
        models: [{ id: `${id}-default`, name: `${name} Default`, default: true }],
      },
    ],
  };
}

const snapshot: AppSnapshot = {
  catalog: {
    schemaVersion: 1,
    clis: [],
    providerTemplates: [
      apiTemplate("openai-api", "OpenAI", "api"),
      apiTemplate("glm-coding-plan", "GLM Coding Plan", "coding-plan"),
      apiTemplate("local-gateway", "Local Gateway", "gateway"),
      { mode: "auth", id: "anthropic-auth", name: "Anthropic Account", authKind: "anthropic" },
      { mode: "auth", id: "codex-auth", name: "Codex Account", authKind: "codex" },
    ],
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

function renderPage(pageSnapshot = snapshot) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  render(
    <QueryClientProvider client={client}>
      <ProviderPage snapshot={pageSnapshot} guarded={(action) => action()} onError={vi.fn()} />
    </QueryClientProvider>,
  );
}

describe("ProviderPage", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
  });

  it("shows one add action and one import action, then groups add templates", () => {
    renderPage();

    const header = screen.getByRole("heading", { name: "供应商", level: 1 }).closest("header");
    expect(header).not.toBeNull();
    const headerButtons = within(header!).getAllByRole("button");
    expect(headerButtons).toHaveLength(2);
    expect(headerButtons[0]).toHaveAccessibleName("添加");
    expect(headerButtons[1]).toHaveAccessibleName("导入");

    fireEvent.click(headerButtons[0]);
    const dialog = screen.getByRole("dialog", { name: "选择供应商模板" });
    expect(within(dialog).getByRole("heading", { name: "OAuth" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "官方 API" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Coding Plan" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "其他 / 自定义" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: /自定义供应商/ })).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("starts an API provider with the selected template and its default name", () => {
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.click(screen.getByRole("button", { name: /GLM Coding Plan/ }));

    expect(screen.queryByRole("dialog", { name: "选择供应商模板" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "GLM Coding Plan", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "名称" })).toHaveValue("GLM Coding Plan");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveValue("glm-coding-plan");
    expect(screen.getByDisplayValue("https://glm-coding-plan.example.test/v1")).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("opens the custom provider defaults and preserves them when template selection is cancelled", () => {
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.click(screen.getByRole("button", { name: /自定义供应商/ }));

    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveValue("");
    expect(screen.getByDisplayValue("https://api.example.com/v1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    const picker = screen.getByRole("dialog", { name: "选择供应商模板" });
    fireEvent.click(within(picker).getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("dialog", { name: "选择供应商模板" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("");
    expect(screen.getByDisplayValue("https://api.example.com/v1")).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("keeps the selected provider visible when template selection is cancelled", async () => {
    const provider: PublicProvider = {
      id: "existing-provider",
      name: "Existing Provider",
      kind: "api",
      templateId: "openai-api",
      templateName: "OpenAI",
      templateMode: "api",
      templateCategory: "api",
      connections: [
        {
          id: "existing-connection",
          templateEndpointId: "responses",
          credentialSlotId: "api-key",
          protocol: "openai-responses",
          endpoint: "https://openai-api.example.test/v1",
          authType: "bearer",
          defaultModel: "openai-api-default",
          verification: { status: "never-tested" },
        },
      ],
      referencedBy: [],
      revision: 1,
      updatedAt: "2026-08-25T00:00:00Z",
    };
    const detail: ApiProviderDetail = {
      id: provider.id,
      name: provider.name,
      templateId: provider.templateId,
      profileType: "api",
      connections: provider.connections.map((connection) => ({
        ...connection,
        apiKey: "test-secret",
      })),
      revision: provider.revision,
      createdAt: "2026-08-25T00:00:00Z",
      updatedAt: provider.updatedAt,
    };
    commandMock.mockImplementation(async (name: string) => {
      if (name === "get_provider_secret_detail") return detail;
      return [provider];
    });
    renderPage({ ...snapshot, providers: [provider] });

    fireEvent.click(screen.getByRole("button", { name: /Existing Provider/ }));
    expect(
      await screen.findByRole("heading", { name: "Existing Provider", level: 2 }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    const picker = screen.getByRole("dialog", { name: "选择供应商模板" });
    fireEvent.click(within(picker).getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("dialog", { name: "选择供应商模板" })).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Existing Provider", level: 2 }),
    ).toBeInTheDocument();
  });

  it("routes OAuth add to official login with the template name", () => {
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.click(screen.getByRole("button", { name: /Codex Account/ }));

    const dialog = screen.getByRole("dialog", { name: "官方登录" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue("Codex Account");
    expect(
      within(dialog).getByRole("checkbox", { name: "使用官方设备授权流程" }),
    ).toBeInTheDocument();
  });

  it("limits import choices to OAuth templates and routes to auth import", () => {
    renderPage();

    fireEvent.click(screen.getByRole("button", { name: "导入" }));
    const picker = screen.getByRole("dialog", { name: "选择要导入的 OAuth 模板" });
    expect(within(picker).getByRole("button", { name: /Anthropic Account/ })).toBeInTheDocument();
    expect(within(picker).getByRole("button", { name: /Codex Account/ })).toBeInTheDocument();
    expect(within(picker).queryByText("OpenAI")).not.toBeInTheDocument();
    expect(within(picker).queryByText("GLM Coding Plan")).not.toBeInTheDocument();
    expect(within(picker).queryByText("自定义供应商")).not.toBeInTheDocument();

    fireEvent.click(within(picker).getByRole("button", { name: /Anthropic Account/ }));
    const importDialog = screen.getByRole("dialog", { name: "导入 auth" });
    expect(within(importDialog).getByRole("textbox", { name: "名称" })).toHaveValue(
      "Anthropic Account",
    );
    expect(within(importDialog).queryByRole("checkbox")).not.toBeInTheDocument();
  });
});
