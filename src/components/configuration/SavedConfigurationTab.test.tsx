import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { ProviderCatalog, SavedConfiguration } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { useNotificationStore } from "../../stores/notifications";
import { NotificationViewport, useErrorNotifier } from "../ui";
import { SavedConfigurationTab } from "./SavedConfigurationTab";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

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
});
