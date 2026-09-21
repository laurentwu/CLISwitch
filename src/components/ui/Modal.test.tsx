import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import { useNotificationStore } from "../../stores/notifications";
import { Button, ConfirmModal, Input, Modal, NotificationViewport } from ".";

function DialogHarness() {
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState(false);
  return (
    <>
      <Button onClick={() => setOpen(true)}>Open backups</Button>
      <Modal open={open} title="Backups" onClose={() => setOpen(false)}>
        <Input autoFocus aria-label="Filter" />
        <Button onClick={() => setConfirm(true)}>Restore</Button>
        <ConfirmModal
          open={confirm}
          title="Confirm restore"
          description="Replace the current file from this backup."
          onClose={() => setConfirm(false)}
          footer={
            <>
              <Button onClick={() => setConfirm(false)}>Cancel restore</Button>
              <Button variant="danger" onClick={() => setError(true)}>
                Confirm
              </Button>
            </>
          }
        >
          {error ? <p>Restore failed</p> : null}
        </ConfirmModal>
      </Modal>
      <NotificationViewport />
    </>
  );
}

describe("Radix dialog adapters", () => {
  afterEach(() => {
    act(() => useNotificationStore.getState().clear());
    vi.useRealTimers();
  });

  it("traps keyboard focus in the top dialog and restores focus through nested dismissal", async () => {
    const user = userEvent.setup();
    render(<DialogHarness />);
    await user.click(screen.getByRole("button", { name: "Open backups" }));
    const dialog = screen.getByRole("dialog", { name: "Backups" });
    expect(dialog).not.toHaveAttribute("aria-describedby");
    expect(screen.getByRole("textbox", { name: "Filter" })).toHaveFocus();
    expect(within(dialog).getByRole("button", { name: "关闭对话框" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore" }));

    const confirmation = screen.getByRole("alertdialog", { name: "Confirm restore" });
    expect(confirmation).toHaveAccessibleDescription("Replace the current file from this backup.");
    expect(screen.getByRole("button", { name: "Cancel restore" })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "Confirm" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Cancel restore" })).toHaveFocus();
    await user.keyboard("{Escape}");

    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Restore" })).toHaveFocus();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Open backups" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Open backups" }));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.getByRole("button", { name: "Open backups" })).toHaveFocus());
  });

  it("keeps destructive confirmation open after a failure and ignores the overlay", async () => {
    const user = userEvent.setup();
    render(<DialogHarness />);
    await user.click(screen.getByRole("button", { name: "Open backups" }));
    await user.click(screen.getByRole("button", { name: "Restore" }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(screen.getByText("Restore failed")).toBeInTheDocument();
    await user.click(document.querySelector('[data-slot="alert-dialog-overlay"]') as HTMLElement);
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("moves one live region into the top focus scope without resetting notification expiry", async () => {
    vi.useFakeTimers();
    render(
      <StrictMode>
        <DialogHarness />
      </StrictMode>,
    );
    act(() => {
      useNotificationStore.getState().push({ tone: "info", title: "Saved" });
    });
    act(() => vi.advanceTimersByTime(2_000));
    fireEvent.click(screen.getByRole("button", { name: "Open backups" }));
    const dialog = screen.getByRole("dialog", { name: "Backups" });
    expect(dialog.querySelector("[data-notification-host]")).not.toBeNull();
    expect(document.querySelectorAll("[data-sonner-toaster]")).toHaveLength(1);
    expect(document.querySelectorAll("[aria-live]")).toHaveLength(1);
    act(() => vi.advanceTimersByTime(1_000));
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });
});
