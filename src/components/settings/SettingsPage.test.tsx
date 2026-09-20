import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../../i18n";
import { ThemeProvider } from "../../app/ThemeProvider";
import { CLI_IDS, type AppSnapshot, type CatalogStatus } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { makeAppSnapshot } from "../../test/fixtures";
import { renderWithQueryClient } from "../../test/render";
import { chooseSelectOption } from "../../test/select";
import { SettingsPage } from "./SettingsPage";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

const snapshot = makeAppSnapshot();

const bundledStatus: CatalogStatus = {
  source: "bundled",
  cachePath: "/tmp/cliswitch/providers.json",
  metadataPath: "/tmp/cliswitch/providers.meta.json",
  fetchedAt: null,
  etag: null,
  digest: "bundled-digest",
  providerCount: 7,
  lastError: null,
  updateAvailable: false,
};

describe("SettingsPage provider database", () => {
  beforeEach(async () => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
    document.documentElement.classList.remove("dark");
    delete document.documentElement.dataset.theme;
    await i18n.changeLanguage("zh-CN");
  });

  it("renders the six open settings sections in their fixed order", () => {
    commandMock.mockResolvedValue(bundledStatus);
    const view = renderWithQueryClient(
      <SettingsPage
        snapshot={makeAppSnapshot({
          settings: {
            manualLocations: CLI_IDS.map((cliId) => ({
              cliId,
              executablePath: null,
              configDirectory: null,
            })),
          },
        })}
        onError={vi.fn()}
      />,
    );

    expect(
      Array.from(view.container.querySelectorAll(".settings-section"), (section) =>
        section.querySelector("h2")?.textContent?.trim(),
      ),
    ).toEqual([
      "外观与行为",
      "明文凭据风险",
      "CLI 路径覆盖",
      "数据与备份",
      "Provider 数据库",
      "关于",
    ]);
    expect(view.container.querySelectorAll(".settings-section")).toHaveLength(6);
    expect(screen.getAllByPlaceholderText("自动发现")).toHaveLength(8);
  });

  it("shows catalog status and replaces it after a successful manual update", async () => {
    const localStatus: CatalogStatus = {
      ...bundledStatus,
      source: "local",
      fetchedAt: "2026-08-26T20:00:00Z",
      etag: "fixture-etag",
      digest: "local-digest",
      providerCount: 8,
    };
    commandMock.mockImplementation((name: string) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "update_catalog") return Promise.resolve(localStatus);
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    renderWithQueryClient(<SettingsPage snapshot={snapshot} onError={vi.fn()} />, {
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });

    expect(await screen.findByText("7 个 provider")).toBeInTheDocument();
    expect(screen.queryByText(/models\.dev|数据来自/)).not.toBeInTheDocument();
    expect(screen.getByText(/当前来源/)).toHaveTextContent("内置快照");

    fireEvent.click(screen.getByRole("button", { name: "更新数据库" }));

    await waitFor(() => {
      expect(screen.getByText("8 个 provider")).toBeInTheDocument();
      expect(screen.getByText(/当前来源/)).toHaveTextContent("本地缓存");
      expect(screen.getByText("Provider 数据库已更新")).toBeInTheDocument();
    });
    expect(commandMock).toHaveBeenCalledWith("update_catalog");
  });

  it("previews a supported zoom immediately and persists it on save", async () => {
    commandMock.mockImplementation((name: string, args?: Record<string, unknown>) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "set_ui_zoom") return Promise.resolve();
      if (name === "update_settings") {
        return Promise.resolve({
          ...(args?.settings as AppSnapshot["settings"]),
          revision: 2,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    renderWithQueryClient(<SettingsPage snapshot={snapshot} onError={vi.fn()} />, {
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });

    const zoom = screen.getByRole("combobox", { name: "界面缩放" });
    await userEvent.click(zoom);
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "100%",
      "125%",
      "150%",
      "175%",
      "200%",
      "225%",
      "250%",
      "275%",
      "300%",
    ]);

    await userEvent.click(screen.getByRole("option", { name: "175%" }));
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("set_ui_zoom", { uiZoomPercent: 175 }),
    );
    expect(useUiStore.getState().dirty).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("update_settings", {
        settings: expect.objectContaining({ uiZoomPercent: 175 }),
        expectedRevision: 1,
      }),
    );
  });

  it("restores the saved zoom when an unsaved preview is discarded", async () => {
    commandMock.mockImplementation((name: string) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "set_ui_zoom") return Promise.resolve();
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    const view = renderWithQueryClient(<SettingsPage snapshot={snapshot} onError={vi.fn()} />, {
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });

    await chooseSelectOption(screen.getByRole("combobox", { name: "界面缩放" }), "250%");
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("set_ui_zoom", { uiZoomPercent: 250 }),
    );

    view.unmount();
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("set_ui_zoom", { uiZoomPercent: 100 }),
    );
  });

  it("shows, clears, and persists Qwen executable and configuration paths", async () => {
    const qwenSnapshot = makeAppSnapshot({
      settings: {
        manualLocations: [
          {
            cliId: "qwen",
            executablePath: "/fixture/bin/qwen",
            configDirectory: "/fixture/.qwen",
          },
        ],
      },
    });
    commandMock.mockImplementation((name: string, args?: Record<string, unknown>) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "update_settings") {
        return Promise.resolve({
          ...(args?.settings as AppSnapshot["settings"]),
          revision: 2,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    renderWithQueryClient(<SettingsPage snapshot={qwenSnapshot} onError={vi.fn()} />, {
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });

    const row = screen.getByText("Qwen Code").closest(".location-row");
    expect(row).not.toBeNull();
    expect(row!.querySelector('input[value="/fixture/bin/qwen"]')).not.toBeNull();
    expect(row!.querySelector('input[value="/fixture/.qwen"]')).not.toBeNull();
    const clearButtons = Array.from(row!.querySelectorAll("button")).filter(
      (button) => button.textContent === "清除",
    );
    expect(clearButtons).toHaveLength(2);
    fireEvent.click(clearButtons[0]);
    fireEvent.click(clearButtons[1]);
    expect(useUiStore.getState().dirty).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("update_settings", {
        settings: expect.objectContaining({
          manualLocations: [
            {
              cliId: "qwen",
              executablePath: null,
              configDirectory: null,
            },
          ],
        }),
        expectedRevision: 1,
      }),
    );
  });

  it("applies a theme draft only after a successful save", async () => {
    const onError = vi.fn();
    commandMock.mockImplementation((name: string, args?: Record<string, unknown>) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "update_settings") {
        return Promise.resolve({
          ...(args?.settings as AppSnapshot["settings"]),
          revision: 2,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    renderWithQueryClient(
      <ThemeProvider>
        <SettingsPage snapshot={snapshot} onError={onError} />
      </ThemeProvider>,
    );

    await chooseSelectOption(screen.getByRole("combobox", { name: "主题" }), "深色");
    expect(document.documentElement).not.toHaveClass("dark");
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(document.documentElement).toHaveClass("dark"));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(onError).not.toHaveBeenCalled();
  });

  it("keeps the applied theme unchanged when saving the draft fails", async () => {
    const onError = vi.fn();
    commandMock.mockImplementation((name: string) => {
      if (name === "get_catalog_status") return Promise.resolve(bundledStatus);
      if (name === "update_settings") return Promise.reject(new Error("save failed"));
      return Promise.reject(new Error(`unexpected command: ${name}`));
    });
    renderWithQueryClient(
      <ThemeProvider>
        <SettingsPage snapshot={snapshot} onError={onError} />
      </ThemeProvider>,
    );

    await chooseSelectOption(screen.getByRole("combobox", { name: "主题" }), "深色");
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onError).toHaveBeenCalledWith(expect.any(Error), "save"));
    expect(document.documentElement).not.toHaveClass("dark");
    expect(document.documentElement).not.toHaveAttribute("data-theme", "dark");
  });
});
