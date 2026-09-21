import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useAppTheme } from "./ThemeProvider";

function ThemeProbe() {
  const { savedTheme, resolvedTheme, applySavedTheme } = useAppTheme();
  return (
    <>
      <output>{`${savedTheme}:${resolvedTheme}`}</output>
      <button onClick={() => applySavedTheme("light")}>Light</button>
      <button onClick={() => applySavedTheme("dark")}>Dark</button>
      <button onClick={() => applySavedTheme("system")}>System</button>
    </>
  );
}

function installMatchMedia(initialMatches: boolean) {
  let matches = initialMatches;
  const listeners = new Set<() => void>();
  const addEventListener = vi.fn((_type: string, listener: () => void) => listeners.add(listener));
  const removeEventListener = vi.fn((_type: string, listener: () => void) =>
    listeners.delete(listener),
  );
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      get matches() {
        return matches;
      },
      media: query,
      onchange: null,
      addEventListener,
      removeEventListener,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  return {
    addEventListener,
    removeEventListener,
    update(next: boolean) {
      matches = next;
      act(() => listeners.forEach((listener) => listener()));
    },
  };
}

describe("ThemeProvider", () => {
  afterEach(() => {
    document.documentElement.classList.remove("dark");
    document.documentElement.style.colorScheme = "";
    delete document.documentElement.dataset.theme;
  });

  it("resolves system initially, follows changes, and ignores them for explicit themes", () => {
    const media = installMatchMedia(true);
    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>,
    );

    expect(screen.getByText("system:dark")).toBeInTheDocument();
    expect(document.documentElement).toHaveClass("dark");
    expect(document.documentElement).not.toHaveAttribute("data-theme");

    fireEvent.click(screen.getByRole("button", { name: "Light" }));
    expect(screen.getByText("light:light")).toBeInTheDocument();
    expect(document.documentElement).not.toHaveClass("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");

    media.update(false);
    media.update(true);
    expect(screen.getByText("light:light")).toBeInTheDocument();
    expect(document.documentElement).not.toHaveClass("dark");

    fireEvent.click(screen.getByRole("button", { name: "System" }));
    expect(screen.getByText("system:dark")).toBeInTheDocument();
    expect(document.documentElement).toHaveClass("dark");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(media.addEventListener).toHaveBeenCalledOnce();
  });

  it("applies explicit dark mode and removes its one media listener on unmount", () => {
    const media = installMatchMedia(false);
    const view = render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(screen.getByText("dark:dark")).toBeInTheDocument();
    expect(document.documentElement).toHaveClass("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    view.unmount();
    expect(media.removeEventListener).toHaveBeenCalledOnce();
    expect(media.removeEventListener.mock.calls[0]?.[1]).toBe(
      media.addEventListener.mock.calls[0]?.[1],
    );
  });
});
