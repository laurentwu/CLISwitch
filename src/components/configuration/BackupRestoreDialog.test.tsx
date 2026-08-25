import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import { BackupRestoreDialog } from "./BackupRestoreDialog";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

describe("BackupRestoreDialog", () => {
  it("shows a retryable query error instead of presenting a failed load as an empty list", async () => {
    commandMock.mockRejectedValueOnce({ code: "io", message: "backup directory unreadable" });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={client}>
        <BackupRestoreDialog open onClose={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("无法加载备份列表")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveClass("alert-error");
    expect(screen.getByRole("button", { name: "重试" })).toBeInTheDocument();
    expect(screen.queryByText("无")).not.toBeInTheDocument();
    expect(screen.getByText("backup directory unreadable")).toBeInTheDocument();
  });
});
