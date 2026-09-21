import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { ThemeProvider, useAppTheme } from "../app/ThemeProvider";
import { CLI_IDS, type CliId } from "../shared/types";
import { CliIcon } from "./CliIcon";

function ThemeControls() {
  const { applySavedTheme } = useAppTheme();
  return (
    <>
      <button onClick={() => applySavedTheme("light")}>Light</button>
      <button onClick={() => applySavedTheme("dark")}>Dark</button>
    </>
  );
}

function RecoveryHarness() {
  const { applySavedTheme } = useAppTheme();
  const [cliId, setCliId] = useState<CliId>("opencode");

  return (
    <>
      <button onClick={() => applySavedTheme("dark")}>Dark</button>
      <button onClick={() => setCliId("qwen")}>Qwen</button>
      <CliIcon cliId={cliId} />
    </>
  );
}

describe("CliIcon", () => {
  afterEach(() => {
    document.documentElement.classList.remove("dark");
    document.documentElement.style.colorScheme = "";
    delete document.documentElement.dataset.theme;
  });

  it("renders a non-focusable decorative image for every supported CLI", () => {
    const view = render(
      <ThemeProvider>
        {CLI_IDS.map((cliId) => (
          <div data-cli-icon={cliId} key={cliId}>
            <CliIcon cliId={cliId} />
          </div>
        ))}
      </ThemeProvider>,
    );

    const images = view.container.querySelectorAll<HTMLImageElement>(".cli-mark > img.cli-icon");
    expect(images).toHaveLength(CLI_IDS.length);
    expect(screen.queryAllByRole("img")).toHaveLength(0);
    for (const image of images) {
      expect(image).toHaveAttribute("alt", "");
      expect(image.draggable).toBe(false);
      expect(image.tabIndex).toBe(-1);
    }
  });

  it("switches only OpenCode to a distinct asset when the resolved theme changes", () => {
    const view = render(
      <ThemeProvider>
        <ThemeControls />
        {CLI_IDS.map((cliId) => (
          <div data-cli-icon={cliId} key={cliId}>
            <CliIcon cliId={cliId} />
          </div>
        ))}
      </ThemeProvider>,
    );
    const sources = () =>
      CLI_IDS.map(
        (cliId) =>
          view.container.querySelector<HTMLImageElement>(`[data-cli-icon="${cliId}"] img`)!.src,
      );
    const lightSources = sources();

    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    const darkSources = sources();

    CLI_IDS.forEach((cliId, index) => {
      if (cliId === "opencode") expect(darkSources[index]).not.toBe(lightSources[index]);
      else expect(darkSources[index]).toBe(lightSources[index]);
    });
  });

  it("replaces only a failed image with its fallback without changing parent interaction", () => {
    const view = render(
      <ThemeProvider>
        <label>
          <input type="checkbox" />
          <CliIcon cliId="qwen" />
          Use Qwen Code
        </label>
        <CliIcon cliId="codex" />
      </ThemeProvider>,
    );
    const images = view.container.querySelectorAll<HTMLImageElement>("img.cli-icon");

    fireEvent.error(images[0]);

    expect(screen.getByText("QW", { selector: ".cli-mark" })).toBeInTheDocument();
    expect(view.container.querySelectorAll("img.cli-icon")).toHaveLength(1);
    expect(view.container.querySelector("img.cli-icon-codex")).toBeInTheDocument();

    const checkbox = screen.getByRole("checkbox", { name: "Use Qwen Code" });
    fireEvent.click(checkbox);
    expect(checkbox).toBeChecked();
  });

  it("retries after changing to a distinct theme asset or CLI", () => {
    const view = render(
      <ThemeProvider>
        <RecoveryHarness />
      </ThemeProvider>,
    );
    fireEvent.error(view.container.querySelector("img.cli-icon")!);
    expect(screen.getByText("OP", { selector: ".cli-mark" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(view.container.querySelector("img.cli-icon")).toBeInTheDocument();
    expect(screen.queryByText("OP", { selector: ".cli-mark" })).not.toBeInTheDocument();

    fireEvent.error(view.container.querySelector("img.cli-icon")!);
    expect(screen.getByText("OP", { selector: ".cli-mark" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Qwen" }));
    expect(view.container.querySelector("img.cli-icon")).toBeInTheDocument();
    expect(screen.queryByText("OP", { selector: ".cli-mark" })).not.toBeInTheDocument();
  });
});
