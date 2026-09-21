import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import { useNotificationStore } from "../../stores/notifications";
import { NotificationViewport, useErrorNotifier } from "./Notifications";

function ErrorTrigger({ cancelled = false }: { cancelled?: boolean }) {
  const reportError = useErrorNotifier();
  return (
    <button
      onClick={() =>
        reportError(
          cancelled
            ? { code: "cancelled", message: "operation cancelled" }
            : { code: "conflict", message: "revision changed" },
          "save",
        )
      }
    >
      Trigger
    </button>
  );
}

describe("NotificationViewport", () => {
  beforeEach(() => useNotificationStore.getState().clear());
  afterEach(() => vi.useRealTimers());

  it("shows a localized summary, guidance, and raw technical details while deduplicating", async () => {
    render(
      <>
        <ErrorTrigger />
        <NotificationViewport />
      </>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Trigger" }));
    fireEvent.click(screen.getByRole("button", { name: "Trigger" }));

    const notification = (await screen.findByText("保存失败")).closest(".alert");
    expect(notification).toHaveClass("alert-warning");
    expect(screen.getByText(/数据已在其他位置发生变化/)).toBeInTheDocument();
    expect(screen.getByText("revision changed")).toBeInTheDocument();
    expect(screen.getByText("×2")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
    await waitFor(() => expect(screen.queryByText("保存失败")).not.toBeInTheDocument());
  });

  it("silences cancellations and automatically removes errors", () => {
    vi.useFakeTimers();
    const id = useNotificationStore.getState().push({ tone: "error", title: "Failure" });
    render(
      <>
        <ErrorTrigger cancelled />
        <NotificationViewport />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Trigger" }));
    expect(useNotificationStore.getState().notifications).toHaveLength(1);

    act(() => vi.advanceTimersByTime(8_000));
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
    expect(id).toBeGreaterThan(0);
  });

  it("runs an action once and removes its stable notification", async () => {
    const run = vi.fn();
    const id = useNotificationStore.getState().push({
      tone: "info",
      title: "Update available",
      action: { label: "Open", run },
    });
    render(<NotificationViewport />);

    const action = await screen.findByRole("button", { name: "Open" });
    fireEvent.click(action);
    fireEvent.click(action);

    expect(run).toHaveBeenCalledOnce();
    expect(useNotificationStore.getState().notifications.some((item) => item.id === id)).toBe(
      false,
    );
  });

  it("updates a stable toast and restarts only its deadline when a duplicate arrives", async () => {
    vi.useFakeTimers();
    render(<NotificationViewport />);
    let id = 0;
    act(() => {
      id = useNotificationStore.getState().push({ tone: "info", title: "Saved" });
    });
    act(() => vi.advanceTimersByTime(2_000));
    act(() => {
      expect(useNotificationStore.getState().push({ tone: "info", title: "Saved" })).toBe(id);
    });
    act(() => vi.advanceTimersByTime(1_000));
    expect(useNotificationStore.getState().notifications).toHaveLength(1);
    expect(useNotificationStore.getState().notifications[0].occurrences).toBe(2);
    act(() => vi.advanceTimersByTime(2_000));
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });

  it("dismisses queue eviction and store clearing from Sonner too", async () => {
    render(<NotificationViewport />);
    act(() => {
      useNotificationStore.getState().push({ tone: "info", title: "Oldest" });
    });
    await screen.findByText("Oldest");
    act(() => {
      for (const title of ["Second", "Third", "Fourth"]) {
        useNotificationStore.getState().push({ tone: "info", title });
      }
    });
    await waitFor(() => expect(screen.queryByText("Oldest")).not.toBeInTheDocument());
    expect(useNotificationStore.getState().notifications).toHaveLength(3);
    act(() => useNotificationStore.getState().clear());
    await waitFor(() => expect(document.querySelectorAll("[data-sonner-toast]")).toHaveLength(0));
  });
});
