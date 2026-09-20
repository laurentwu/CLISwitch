import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { SavedConfiguration } from "../../shared/types";
import { Button, Modal } from "../ui";
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

function GuardedTabsHarness() {
  const [active, setActive] = useState<string>(configurations[0].id);
  const [pending, setPending] = useState<null | (() => void)>(null);
  return (
    <>
      <ConfigurationTabs
        configurations={configurations}
        active={active}
        dirty
        onSelect={(id) => setPending(() => () => setActive(id))}
        onAdd={vi.fn()}
      />
      <Modal
        open={Boolean(pending)}
        title="是否保存更改？"
        onClose={() => setPending(null)}
        footer={<Button onClick={() => setPending(null)}>取消</Button>}
      >
        未保存的修改
      </Modal>
    </>
  );
}

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

  it("supports arrow-key tab navigation", async () => {
    const select = vi.fn();
    render(
      <ConfigurationTabs
        configurations={configurations}
        active="current"
        onSelect={select}
        onAdd={vi.fn()}
      />,
    );
    const current = screen.getByRole("tab", { name: "当前配置" });
    act(() => current.focus());
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(() => expect(select).toHaveBeenCalledWith(configurations[0].id));
  });

  it("restores focus to the selected tab when a guarded keyboard switch is cancelled", async () => {
    render(<GuardedTabsHarness />);
    const selected = screen.getByRole("tab", { name: /配置一/ });
    act(() => selected.focus());

    await userEvent.keyboard("{ArrowRight}");
    const dialog = await screen.findByRole("dialog", { name: "是否保存更改？" });
    expect(selected).toHaveAttribute("aria-selected", "true");

    await userEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    await waitFor(() => expect(selected).toHaveFocus());
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
