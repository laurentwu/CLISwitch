import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { AppSnapshot } from "../shared/types";
import { useUiStore } from "../stores/ui";
import { App } from "./App";

const commandMock = vi.hoisted(() => vi.fn());
const onEventMock = vi.hoisted(() => vi.fn());
vi.mock("../shared/ipc", () => ({ command: commandMock, onEvent: onEventMock }));
vi.mock("../components/configuration/ConfigurationPage", () => ({
  ConfigurationPage: () => <div>Configuration page</div>,
}));
vi.mock("../components/providers/ProviderPage", () => ({
  ProviderPage: () => <div>Provider page</div>,
}));
vi.mock("../components/settings/SettingsPage", () => ({
  SettingsPage: () => <div>Settings page</div>,
}));

const snapshot: AppSnapshot = {
  catalog: { schemaVersion: 1, clis: [], providerTemplates: [], relations: [] },
  settings: {
    language: "zh-cn",
    theme: "system",
    uiZoomPercent: 225,
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

function mockReadyAppCommands() {
  commandMock.mockImplementation((name: string) => {
    if (name === "get_startup_status") {
      return Promise.resolve({
        ready: true,
        code: null,
        message: null,
        appDataDirectory: "/tmp/cliswitch",
      });
    }
    if (name === "get_app_snapshot") return Promise.resolve(snapshot);
    if (name === "set_ui_zoom" || name === "set_frontend_dirty") return Promise.resolve();
    return Promise.reject(new Error(`unexpected command: ${name}`));
  });
}

function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

async function openUnsavedChangesDialog() {
  await screen.findByText("Configuration page");
  fireEvent.click(screen.getByRole("button", { name: "供应商" }));
  return screen.getByRole("dialog", { name: "是否保存更改？" });
}

describe("App startup", () => {
  beforeEach(() => {
    commandMock.mockReset();
    onEventMock.mockReset();
    onEventMock.mockResolvedValue(vi.fn());
    useUiStore.setState({
      navigation: "configuration",
      configurationId: "current",
      dirty: false,
      saveCurrent: undefined,
    });
  });

  it("applies the persisted interface zoom after loading the snapshot", async () => {
    mockReadyAppCommands();
    renderApp();

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("set_ui_zoom", { uiZoomPercent: 225 }),
    );
  });

  it("presents the unsaved changes actions in the confirmed layout", async () => {
    mockReadyAppCommands();
    useUiStore.setState({ dirty: true, saveCurrent: vi.fn().mockResolvedValue(true) });
    renderApp();

    const dialog = await openUnsavedChangesDialog();
    expect(within(dialog).getByText("关闭后，未保存的修改将会丢失。")).toBeInTheDocument();

    const footer = dialog.querySelector(".modal-footer");
    expect(footer).not.toBeNull();
    const actions = within(footer as HTMLElement).getAllByRole("button");
    expect(actions.map((action) => action.textContent)).toEqual(["不保存", "取消", "保存"]);
    expect(actions[0]).toHaveClass("button-secondary", "unsaved-dialog-discard");
    expect(within(dialog).getByRole("button", { name: "关闭对话框" })).toBeInTheDocument();
  });

  it("keeps edits when the dialog is cancelled, closed, or dismissed via the backdrop", async () => {
    mockReadyAppCommands();
    useUiStore.setState({ dirty: true, saveCurrent: vi.fn().mockResolvedValue(true) });
    renderApp();

    let dialog = await openUnsavedChangesDialog();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog", { name: "是否保存更改？" })).not.toBeInTheDocument();

    dialog = await openUnsavedChangesDialog();
    fireEvent.click(within(dialog).getByRole("button", { name: "关闭对话框" }));
    expect(screen.queryByRole("dialog", { name: "是否保存更改？" })).not.toBeInTheDocument();

    dialog = await openUnsavedChangesDialog();
    fireEvent.mouseDown(dialog.parentElement as HTMLElement);
    expect(screen.queryByRole("dialog", { name: "是否保存更改？" })).not.toBeInTheDocument();
    expect(screen.getByText("Configuration page")).toBeInTheDocument();
    expect(useUiStore.getState().dirty).toBe(true);
  });

  it("continues without saving when the user discards the edits", async () => {
    mockReadyAppCommands();
    const saveCurrent = vi.fn().mockResolvedValue(true);
    useUiStore.setState({ dirty: true, saveCurrent });
    renderApp();

    const dialog = await openUnsavedChangesDialog();
    fireEvent.click(within(dialog).getByRole("button", { name: "不保存" }));

    expect(await screen.findByText("Provider page")).toBeInTheDocument();
    expect(useUiStore.getState().dirty).toBe(false);
    expect(saveCurrent).not.toHaveBeenCalled();
  });

  it("saves before continuing with the pending action", async () => {
    mockReadyAppCommands();
    const saveCurrent = vi.fn().mockResolvedValue(true);
    useUiStore.setState({ dirty: true, saveCurrent });
    renderApp();

    const dialog = await openUnsavedChangesDialog();
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

    expect(await screen.findByText("Provider page")).toBeInTheDocument();
    expect(saveCurrent).toHaveBeenCalledOnce();
    expect(useUiStore.getState().dirty).toBe(false);
  });

  it("keeps the dialog and edits when saving does not complete", async () => {
    mockReadyAppCommands();
    const saveCurrent = vi.fn().mockResolvedValue(false);
    useUiStore.setState({ dirty: true, saveCurrent });
    renderApp();

    const dialog = await openUnsavedChangesDialog();
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

    await waitFor(() => expect(saveCurrent).toHaveBeenCalledOnce());
    expect(screen.getByRole("dialog", { name: "是否保存更改？" })).toBeInTheDocument();
    expect(screen.getByText("Configuration page")).toBeInTheDocument();
    expect(useUiStore.getState().dirty).toBe(true);
  });
});
