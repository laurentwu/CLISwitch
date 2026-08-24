import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { SavedConfiguration } from "../../shared/types";
import { ConfigurationTabs } from "./ConfigurationTabs";

const configurations: SavedConfiguration[] = ["配置一", "配置二"].map((name, index) => ({
  id: `00000000-0000-4000-8000-00000000000${index}`,
  name,
  creationOrder: index + 1,
  revision: 1,
  targets: [],
  createdAt: "2026-08-23T00:00:00Z",
  updatedAt: "2026-08-23T00:00:00Z",
}));

describe("ConfigurationTabs", () => {
  it("keeps current first, saved tabs side by side, and plus last", () => {
    render(
      <ConfigurationTabs
        configurations={configurations}
        active="current"
        onSelect={vi.fn()}
        onAdd={vi.fn()}
      />,
    );
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["当前配置", "配置一", "配置二"]);
    expect(screen.getByLabelText("新建配置")).toBeInTheDocument();
  });

  it("supports arrow-key tab navigation", () => {
    const select = vi.fn();
    render(
      <ConfigurationTabs
        configurations={configurations}
        active="current"
        onSelect={select}
        onAdd={vi.fn()}
      />,
    );
    fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowRight" });
    expect(select).toHaveBeenCalledWith(configurations[0].id);
  });

  it("marks the active saved tab when it has unsaved changes", () => {
    render(
      <ConfigurationTabs
        configurations={configurations}
        active={configurations[0].id}
        dirty
        onSelect={vi.fn()}
        onAdd={vi.fn()}
      />,
    );
    expect(screen.getByRole("tab", { name: /配置一/ })).toContainElement(
      screen.getByLabelText("有未保存的修改"),
    );
    expect(screen.getByRole("tab", { name: "配置二" })).not.toHaveTextContent("•");
  });
});
