import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ApiProviderDetail, ProviderTemplate, PublicProvider } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { makeAppSnapshot } from "../../test/fixtures";
import { renderWithQueryClient } from "../../test/render";
import { chooseSelectOption } from "../../test/select";
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

const snapshot = makeAppSnapshot({
  catalog: {
    providerTemplates: [
      apiTemplate("openai-api", "OpenAI", "api"),
      apiTemplate("glm-coding-plan", "GLM Coding Plan", "coding-plan"),
      apiTemplate("local-gateway", "Local Gateway", "gateway"),
      { mode: "auth", id: "anthropic-auth", name: "Anthropic Account", authKind: "anthropic" },
      { mode: "auth", id: "codex-auth", name: "Codex Account", authKind: "codex" },
    ],
  },
});

function renderPage(
  pageSnapshot = snapshot,
  guarded: (action: () => void) => void = (action) => action(),
) {
  renderWithQueryClient(
    <ProviderPage snapshot={pageSnapshot} guarded={guarded} onError={vi.fn()} />,
    {
      defaultOptions: { queries: { retry: false, staleTime: Infinity } },
    },
  );
}

describe("ProviderPage", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
  });

  it("shows template names as subtitles while retaining icons and reference counts", () => {
    const providers: PublicProvider[] = [
      {
        id: "api-provider",
        name: "OpenAI account",
        kind: "api",
        templateName: "OpenAI",
        connections: [],
        referencedBy: ["Work", "Review"],
        revision: 1,
        updatedAt: "2026-08-25T00:00:00Z",
      },
      {
        id: "oauth-provider",
        name: "Claude login",
        kind: "oauth",
        templateName: "Anthropic Account",
        connections: [],
        referencedBy: ["Personal"],
        revision: 1,
        updatedAt: "2026-08-25T00:00:00Z",
      },
      {
        id: "custom-provider",
        name: "Local gateway",
        kind: "api",
        templateName: null,
        connections: [],
        referencedBy: [],
        revision: 1,
        updatedAt: "2026-08-25T00:00:00Z",
      },
    ];
    renderPage({ ...snapshot, providers });

    const list = screen.getByRole("complementary", { name: "供应商" });
    const rows = within(list).getAllByRole("button");

    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.querySelector("small")?.textContent)).toEqual([
      "OpenAI",
      "Anthropic Account",
      "自定义供应商",
    ]);
    expect(rows.map((row) => row.querySelector(".badge")?.textContent)).toEqual(["2", "1", "0"]);
    expect(rows.map((row) => row.querySelectorAll(".badge").length)).toEqual([1, 1, 1]);
    expect(rows.map((row) => Boolean(row.querySelector(".provider-icon")))).toEqual([
      true,
      true,
      true,
    ]);
    expect(list).not.toHaveTextContent("端点 + Key");
    expect(list).not.toHaveTextContent("OAuth");
  });

  it("opens an inline add editor with OAuth and API template groups", async () => {
    renderPage();

    const pageHeader = screen.getByRole("heading", { name: "供应商", level: 1 }).closest("header");
    expect(pageHeader).not.toBeNull();
    expect(within(pageHeader!).getAllByRole("button")).toHaveLength(1);
    fireEvent.click(within(pageHeader!).getByRole("button", { name: "添加" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "添加供应商", level: 2 })).toBeInTheDocument();
    const template = screen.getByRole("combobox", { name: /Provider 模板/ });
    expect(template).toHaveTextContent("选择模板");
    await userEvent.click(template);
    expect(screen.getByRole("group", { name: "OAuth" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "官方 API" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Codex Account" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "GLM Coding Plan" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "自定义供应商" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("textbox", { name: /OAuth 原始内容/ })).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("https://api.example.com/v1")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "添加接入方式" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("shows API fields and defaults after an API template is selected", async () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    await chooseSelectOption(
      screen.getByRole("combobox", { name: /Provider 模板/ }),
      "GLM Coding Plan",
    );

    expect(screen.getByRole("heading", { name: "GLM Coding Plan", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("GLM Coding Plan");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveTextContent(
      "GLM Coding Plan",
    );
    expect(screen.getByDisplayValue("https://glm-coding-plan.example.test/v1")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /OAuth 原始内容/ })).not.toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("shows custom API defaults when the custom option is selected", async () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));

    await chooseSelectOption(
      screen.getByRole("combobox", { name: /Provider 模板/ }),
      "自定义供应商",
    );

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
      if (name === "list_providers") return [created];
      return undefined;
    });
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    await chooseSelectOption(screen.getByRole("combobox", { name: /Provider 模板/ }), "OpenAI");
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

  it("guards a dirty inline editor before switching provider modes", async () => {
    const guarded = vi.fn((action: () => void) => action());
    renderPage(snapshot, guarded);
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    guarded.mockClear();
    guarded.mockImplementation(() => {});
    await chooseSelectOption(screen.getByRole("combobox", { name: /Provider 模板/ }), "OpenAI");
    fireEvent.change(screen.getByRole("textbox", { name: /API Key/ }), {
      target: { value: "unsaved-secret" },
    });
    await chooseSelectOption(
      screen.getByRole("combobox", { name: /Provider 模板/ }),
      "Codex Account",
    );

    expect(guarded).toHaveBeenCalledOnce();
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveTextContent("OpenAI");
    expect(screen.getByDisplayValue("unsaved-secret")).toBeInTheDocument();
  });

  it("switches to OAuth fields and starts official login from the editor", async () => {
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    await chooseSelectOption(
      screen.getByRole("combobox", { name: /Provider 模板/ }),
      "Codex Account",
    );

    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("Codex Account");
    expect(screen.getByRole("textbox", { name: /OAuth 原始内容/ })).toHaveValue("");
    expect(screen.getByRole("button", { name: "导入 auth" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("https://api.example.com/v1")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "官方登录" }));
    const dialog = screen.getByRole("dialog", { name: "官方登录" });
    expect(within(dialog).getByRole("textbox", { name: "名称" })).toHaveValue("Codex Account");
    expect(
      within(dialog).getByRole("checkbox", { name: "使用官方设备授权流程" }),
    ).toBeInTheDocument();
  });

  it("routes the OAuth editor import-auth button to the import flow", async () => {
    renderPage();
    expect(screen.queryByRole("button", { name: "导入" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    await chooseSelectOption(
      screen.getByRole("combobox", { name: /Provider 模板/ }),
      "Anthropic Account",
    );

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
    ).toEqual(["删除", "复制", "保存"]);

    commandMock.mockClear();
    fireEvent.click(within(editorHeader!).getByRole("button", { name: "复制" }));

    expect(screen.getByRole("textbox", { name: /^名称/ })).toHaveValue("Existing Provider 复制");
    expect(screen.getByRole("combobox", { name: /Provider 模板/ })).toHaveTextContent("OpenAI");
    expect(screen.getByDisplayValue("test-secret")).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalled();
  });
});
