import { act, fireEvent, render, screen } from "@testing-library/react";
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

  it("shows a localized summary, guidance, and raw technical details while deduplicating", () => {
    render(
      <>
        <ErrorTrigger />
        <NotificationViewport />
      </>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Trigger" }));
    fireEvent.click(screen.getByRole("button", { name: "Trigger" }));

    expect(screen.getByRole("alert")).toHaveClass("alert-warning");
    expect(screen.getByText("保存失败")).toBeInTheDocument();
    expect(screen.getByText(/数据已在其他位置发生变化/)).toBeInTheDocument();
    expect(screen.getByText("revision changed")).toBeInTheDocument();
    expect(screen.getByText("×2")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
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
    expect(screen.queryByText("Failure")).not.toBeInTheDocument();
    expect(id).toBeGreaterThan(0);
  });
});
