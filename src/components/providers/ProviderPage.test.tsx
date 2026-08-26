import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
import { CUSTOM_PROVIDER_TEMPLATE } from "./ProviderTemplateSelect";

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

function renderPage(
  pageSnapshot = snapshot,
  guarded: (action: () => void) => void = (action) => action(),
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  render(
    <QueryClientProvider client={client}>
      <ProviderPage snapshot={pageSnapshot} guarded={guarded} onError={vi.fn()} />
    </QueryClientProvider>,
  );
}

describe("ProviderPage", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
  });

  it("opens an inline add editor with OAuth and API template groups", () => {
    renderPage();

    const pageHeader = screen.getByRole("heading", { name: "供应商", level: 1 }).closest("header");
    expect(pageHeader).not.toBeNull();
    expect(within(pageHeader!).getAllByRole("button")).toHaveLength(1);
    fireEvent.click(within(pageHeader!).getByRole("button", { name: "添加" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "添加供应商", level: 2 })).toBeInTheDocument();
    const template = screen.getByRole("combobox", { name: /Provider 模板/ });
    expect(template).toHaveValue("");
    expect(screen.getByRole("group", { name: "OAuth" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "官方 API" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Codex Account" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "GLM Coding Plan" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "自定义供应商" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /OAuth 原始内容/ })).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("https://api.example.com/v1")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "添加接入方式" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("shows API fields and defaults after an API template is selected", () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "glm-coding-plan" },
    });

    expect(screen.getByRole("heading", { name: "GLM Coding Plan", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("GLM Coding Plan");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveValue("glm-coding-plan");
    expect(screen.getByDisplayValue("https://glm-coding-plan.example.test/v1")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /OAuth 原始内容/ })).not.toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("shows custom API defaults when the custom option is selected", () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: CUSTOM_PROVIDER_TEMPLATE },
    });

    expect(screen.getByRole("heading", { name: "自定义供应商", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("");
    expect(screen.getByDisplayValue("https://api.example.com/v1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "添加接入方式" })).toBeInTheDocument();
  });

  it("creates an API provider from the inline editor", async () => {
    const created = {
      id: "created-api",
      name: "OpenAI",
      kind: "api",
      connections: [],
      referencedBy: [],
      revision: 1,
      updatedAt: "2026-08-25T00:00:00Z",
    } satisfies PublicProvider;
    commandMock.mockImplementation(async (name: string) => {
      if (name === "create_provider") return created;
      return undefined;
    });
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "openai-api" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: /API Key/ }), {
      target: { value: "api-secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith(
        "create_provider",
        expect.objectContaining({
          draft: expect.objectContaining({
            name: "OpenAI",
            templateId: "openai-api",
          }),
        }),
      ),
    );
  });

  it("guards a dirty inline editor before switching provider modes", () => {
    const guarded = vi.fn((action: () => void) => action());
    renderPage(snapshot, guarded);
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    guarded.mockClear();
    guarded.mockImplementation(() => {});
    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "openai-api" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: /API Key/ }), {
      target: { value: "unsaved-secret" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "codex-auth" },
    });

    expect(guarded).toHaveBeenCalledOnce();
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveValue("openai-api");
    expect(screen.getByDisplayValue("unsaved-secret")).toBeInTheDocument();
  });

  it("switches to OAuth fields and starts official login from the editor", () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "codex-auth" },
    });

    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("Codex Account");
    expect(screen.getByRole("textbox", { name: /OAuth 原始内容/ })).toHaveValue("");
    expect(screen.getByRole("button", { name: "导入 auth" })).toBeInTheDocument();
    expect(screen.queryByDisplayValue("https://api.example.com/v1")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "官方登录" }));
    const dialog = screen.getByRole("dialog", { name: "官方登录" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue("Codex Account");
    expect(
      within(dialog).getByRole("checkbox", { name: "使用官方设备授权流程" }),
    ).toBeInTheDocument();
  });

  it("routes the OAuth editor import-auth button to the import flow", () => {
    renderPage();
    expect(screen.queryByRole("button", { name: "导入" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.change(screen.getByRole("combobox", { name: /Provider 模板/ }), {
      target: { value: "anthropic-auth" },
    });

    fireEvent.click(screen.getByRole("button", { name: "导入 auth" }));
    const dialog = screen.getByRole("dialog", { name: "导入 auth" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue("Anthropic Account");
    expect(within(dialog).queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("opens API duplicate as a prefilled unsaved add editor", async () => {
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
      return undefined;
    });
    renderPage({ ...snapshot, providers: [provider] });

    fireEvent.click(screen.getByRole("button", { name: /Existing Provider/ }));
    const heading = await screen.findByRole("heading", { name: "Existing Provider", level: 2 });
    const editorHeader = heading.closest("header");
    expect(editorHeader).not.toBeNull();
    expect(
      within(editorHeader!)
        .getAllByRole("button")
        .map((button) => button.textContent?.trim()),
    ).toEqual(["删除", "复制", "取消", "保存"]);

    commandMock.mockClear();
    fireEvent.click(within(editorHeader!).getByRole("button", { name: "复制" }));

    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("Existing Provider 复制");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveValue("openai-api");
    expect(screen.getByDisplayValue("test-secret")).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });
});
