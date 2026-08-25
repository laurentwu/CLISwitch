import type { ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import { AppErrorBoundary } from "./ErrorBoundary";

function BrokenView(): ReactNode {
  throw new Error("render exploded");
}

describe("AppErrorBoundary", () => {
  afterEach(() => vi.restoreAllMocks());

  it("turns an unrecoverable render failure into a retryable full-screen error", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <AppErrorBoundary>
        <BrokenView />
      </AppErrorBoundary>,
    );

    expect(screen.getByRole("heading", { name: "界面无法继续显示" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveClass("alert-error");
    expect(screen.getByRole("button", { name: "重试" })).toBeInTheDocument();
    expect(screen.getByText("render exploded")).toBeInTheDocument();
  });
});
