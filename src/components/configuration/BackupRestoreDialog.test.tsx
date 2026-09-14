import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import { renderWithQueryClient } from "../../test/render";
import { BackupRestoreDialog } from "./BackupRestoreDialog";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

describe("BackupRestoreDialog", () => {
  it("shows a retryable query error instead of presenting a failed load as an empty list", async () => {
    commandMock.mockRejectedValueOnce({ code: "io", message: "backup directory unreadable" });
    renderWithQueryClient(<BackupRestoreDialog open onClose={vi.fn()} />, {
      defaultOptions: { queries: { retry: false } },
    });

    expect(await screen.findByText("无法加载备份列表")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveClass("alert-error");
    expect(screen.getByRole("button", { name: "重试" })).toBeInTheDocument();
    expect(screen.queryByText("无")).not.toBeInTheDocument();
    expect(screen.getByText("backup directory unreadable")).toBeInTheDocument();
  });

  it("shows the Qwen product name instead of its persisted CLI ID", async () => {
    commandMock.mockResolvedValueOnce([
      {
        id: "backup-1",
        cliId: "qwen",
        sourceFileId: "qwen-settings",
        originalPath: "/fixture/.qwen/settings.json",
        createdAt: "2026-09-07T00:00:00Z",
        originallyExisted: true,
        containsCredentials: true,
      },
    ]);
    renderWithQueryClient(<BackupRestoreDialog open cliId="qwen" onClose={vi.fn()} />, {
      defaultOptions: { queries: { retry: false } },
    });

    expect(await screen.findByText("Qwen Code")).toBeInTheDocument();
    expect(screen.queryByText("qwen")).not.toBeInTheDocument();
  });
});
