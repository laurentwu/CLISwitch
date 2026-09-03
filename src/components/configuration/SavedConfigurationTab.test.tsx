import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ProviderCatalog, PublicProvider, SavedConfiguration } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { useNotificationStore } from "../../stores/notifications";
import { NotificationViewport, useErrorNotifier } from "../ui";
import { SavedConfigurationTab } from "./SavedConfigurationTab";

const commandMock = vi.hoisted(() => vi.fn());
const onEventMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock, onEvent: onEventMock }));

const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [],
  providerTemplates: [],
  relations: [],
};

const configuration: SavedConfiguration = {
  id: "configuration-1",
  name: "Primary",
  creationOrder: 1,
  revision: 1,
  targets: [],
  createdAt: "2026-08-23T00:00:00Z",
  updatedAt: "2026-08-23T00:00:00Z",
};

const duplicate: SavedConfiguration = {
  ...configuration,
  id: "configuration-2",
  name: "Existing",
  creationOrder: 2,
};

const target: SavedConfiguration["targets"][number] = {
  targetType: "api",
  cliId: "codex",
  providerId: "provider-1",
  connectionId: "connection-1",
  model: "model-a",
};

const targetConfiguration: SavedConfiguration = {
  ...configuration,
  targets: [target],
};

const targetCatalog: ProviderCatalog = {
  ...catalog,
  clis: [
    {
      id: "codex",
      name: "Codex CLI",
      protocols: ["openai-responses"],
      authModes: [],
      protocolAdapters: [],
    },
  ],
};

const provider: PublicProvider = {
  id: "provider-1",
  name: "Provider",
  kind: "api",
  templateId: undefined,
  connections: [
    {
      id: "connection-1",
      templateEndpointId: undefined,
      credentialSlotId: "api-key",
      protocol: "openai-responses" as const,
      endpoint: "https://example.test/v1",
      authType: "bearer" as const,
      defaultModel: "model-a",
      verification: { status: "never-tested" as const },
    },
  ],
  referencedBy: [],
  revision: 1,
  updatedAt: "2026-08-23T00:00:00Z",
};

function Harness() {
  const reportError = useErrorNotifier();
  return (
    <>
      <SavedConfigurationTab
        configuration={configuration}
        providers={[]}
        catalog={catalog}
        configurations={[configuration, duplicate]}
        onDeleted={vi.fn()}
        onError={reportError}
      />
      <NotificationViewport />
    </>
  );
}

describe("SavedConfigurationTab", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useNotificationStore.getState().clear();
    useUiStore.setState({
      navigation: "configuration",
      configurationId: "current",
      dirty: false,
      saveCurrent: undefined,
    });
  });

  it("reports guarded-save validation and keeps the pending transition blocked", async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Harness />
      </QueryClientProvider>,
    );
    fireEvent.change(screen.getByRole("textbox", { name: "名称" }), {
      target: { value: " existing " },
    });
    await waitFor(() => expect(useUiStore.getState().saveCurrent).toBeTypeOf("function"));

    let saved: boolean | undefined;
    await act(async () => {
      saved = await useUiStore.getState().saveCurrent?.();
    });

    expect(saved).toBe(false);
    const notification = screen.getByRole("alert");
    expect(notification).toHaveClass("alert-warning");
    expect(within(notification).getByText("保存失败")).toBeInTheDocument();
    expect(
      within(notification).getByText("名称与已有项目重复（不区分大小写）。"),
    ).toBeInTheDocument();
    expect(commandMock).not.toHaveBeenCalledWith("update_configuration", expect.anything());
  });

  it("saves a dirty configuration before applying without opening a preview", async () => {
    const saved = { ...targetConfiguration, revision: 2 };
    commandMock.mockImplementation(async (name: string) => {
      if (name === "update_configuration") return saved;
      if (name === "apply_configuration") {
        return {
          id: "run-1",
          previewId: "preview-1",
          configurationId: saved.id,
          startedAt: "2026-08-23T00:00:00Z",
          finishedAt: "2026-08-23T00:00:01Z",
          cancelRequested: false,
          items: [{ cliId: "codex", state: "success", message: null }],
        };
      }
      return [];
    });
    onEventMock.mockResolvedValue(() => undefined);
    useUiStore.setState({ dirty: true });

    render(
      <QueryClientProvider client={new QueryClient()}>
        <SavedConfigurationTab
          configuration={targetConfiguration}
          providers={[provider]}
          catalog={targetCatalog}
          configurations={[targetConfiguration]}
          onDeleted={vi.fn()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(screen.getAllByRole("button", { name: /预览/ })).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: /应用配置/ }));
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("apply_configuration", {
        configurationId: saved.id,
        expectedRevision: saved.revision,
      }),
    );
    expect(commandMock.mock.invocationCallOrder[0]).toBeLessThan(
      commandMock.mock.invocationCallOrder[1],
    );
    expect(commandMock.mock.calls[0][0]).toBe("update_configuration");
    expect(commandMock).not.toHaveBeenCalledWith("preview_apply", expect.anything());
  });

  it("does not report a failed save twice when applying", async () => {
    const saveError = Object.assign(new Error("save failed"), { code: "conflict" });
    commandMock.mockImplementation(async (name: string) => {
      if (name === "update_configuration") throw saveError;
      return [];
    });
    useUiStore.setState({ dirty: true });
    const onError = vi.fn();

    render(
      <QueryClientProvider client={new QueryClient()}>
        <SavedConfigurationTab
          configuration={targetConfiguration}
          providers={[provider]}
          catalog={targetCatalog}
          configurations={[targetConfiguration]}
          onDeleted={vi.fn()}
          onError={onError}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /应用配置/ }));

    await waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
    expect(onError).toHaveBeenCalledWith(saveError, "save");
    expect(commandMock).not.toHaveBeenCalledWith("apply_configuration", expect.anything());
  });

  it("distinguishes saved instances that use the same catalog provider", () => {
    const providerId = "zhipuai-coding-plan";
    const catalogProvider: ProviderCatalog = {
      ...targetCatalog,
      providerTemplates: [
        {
          mode: "api",
          id: providerId,
          name: "Zhipu AI Coding Plan",
          category: "cli-adapter",
          credentialSlots: [{ id: "api-key", name: "API Key" }],
          endpoints: [
            {
              id: "responses",
              name: "OpenAI Responses",
              protocol: "openai-responses",
              baseUrl: "https://open.bigmodel.cn/api/v1",
              credentialSlotId: "api-key",
              authOptions: [{ id: "bearer", authType: "bearer" }],
              defaultAuthOptionId: "bearer",
              models: [],
            },
          ],
        },
      ],
      relations: [
        {
          mode: "api",
          id: "codex-zhipuai-coding-plan-responses",
          cliId: "codex",
          providerTemplateId: providerId,
          endpointId: "responses",
          authOptionId: "bearer",
          default: true,
          nativeProviderIds: [],
        },
      ],
      providerInfo: [
        {
          id: providerId,
          name: "Zhipu AI Coding Plan",
          env: ["ZHIPU_API_KEY"],
          selectable: true,
          supportedClis: ["codex"],
          endpoints: [
            {
              id: "responses",
              protocol: "openai-responses",
              endpoint: "https://open.bigmodel.cn/api/v1",
              selectable: true,
              supportedClis: ["codex"],
            },
          ],
        },
      ],
    };
    const standardProvider: PublicProvider = {
      ...provider,
      name: "Zhipu AI Coding Plan",
      templateId: providerId,
      connections: [
        {
          ...provider.connections[0],
          templateEndpointId: "responses",
        },
      ],
    };
    const flashProvider: PublicProvider = {
      ...standardProvider,
      id: "provider-flash",
      name: "Zhipu AI Coding Plan Flash",
      connections: [
        {
          ...standardProvider.connections[0],
          id: "connection-flash",
        },
      ],
    };
    const namedConfiguration: SavedConfiguration = {
      ...targetConfiguration,
      targets: [
        {
          ...target,
          providerId: standardProvider.id,
        },
      ],
    };

    render(
      <QueryClientProvider client={new QueryClient()}>
        <SavedConfigurationTab
          configuration={namedConfiguration}
          providers={[standardProvider, flashProvider]}
          catalog={catalogProvider}
          configurations={[namedConfiguration]}
          onDeleted={vi.fn()}
          onError={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getAllByRole("option", {
        name: "Zhipu AI Coding Plan (zhipuai-coding-plan)",
      }),
    ).toHaveLength(2);
    expect(
      screen.getAllByRole("option", {
        name: "Zhipu AI Coding Plan Flash (zhipuai-coding-plan)",
      }),
    ).toHaveLength(2);

    const targetProvider = screen.getByRole("combobox", { name: "供应商" });
    fireEvent.change(targetProvider, { target: { value: flashProvider.id } });
    expect(targetProvider).toHaveValue(flashProvider.id);
  });
});
