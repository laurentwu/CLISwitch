import type { AppSettings } from "../src/shared/types";
import { refreshApp } from "./helpers/refresh";

async function settingsCommand<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return (await browser.tauri.execute(
    ({ core }, request: { command: string; args: Record<string, unknown> }) =>
      core.invoke(request.command, request.args),
    { command, args },
  )) as T;
}

async function selectPreference(label: string, option: string) {
  const fieldLabel = await $(`//label[normalize-space()=${JSON.stringify(label)}]`);
  const id = await fieldLabel.getAttribute("for");
  await $(`[id=${JSON.stringify(id)}]`).click();
  await $(`//*[@role='option' and normalize-space()=${JSON.stringify(option)}]`).click();
}

async function assertNoPageOverflow() {
  const dimensions = await browser.execute(() => {
    const main = document.querySelector("main")!;
    return {
      rootWidth: document.documentElement.clientWidth,
      rootScroll: document.documentElement.scrollWidth,
      mainWidth: main.clientWidth,
      mainScroll: main.scrollWidth,
    };
  });
  expect(dimensions.rootScroll).toBeLessThanOrEqual(dimensions.rootWidth + 1);
  expect(dimensions.mainScroll).toBeLessThanOrEqual(dimensions.mainWidth + 1);
}

describe("Desktop appearance and keyboard interaction", () => {
  it("saves themes through Radix Select and restores the saved preference on reload", async () => {
    await $("nav button:nth-child(3)").click();
    await selectPreference("语言", "English");
    await $(".page-header button").click();
    await expect($("h1")).toHaveText("Settings");

    for (const [option, preference] of [
      ["Dark", "dark"],
      ["Light", "light"],
    ] as const) {
      await selectPreference("Theme", option);
      await $(".page-header button").click();
      await browser.waitUntil(
        async () => (await $("html").getAttribute("data-theme")) === preference,
      );
      await refreshApp();
      await expect($("html")).toHaveAttribute("data-theme", preference);
      await $("nav button:nth-child(3)").click();
    }
  });

  it("keeps current details and dialog keyboard focus usable", async () => {
    await $("nav button:nth-child(1)").click();
    const details = await $('button[aria-label="Show Qwen Code details"]');
    await expect(details).toHaveAttribute("aria-expanded", "false");
    await details.click();
    await expect(details).toHaveAttribute("aria-expanded", "true");
    await expect($('.current-cli-row[data-cli-id="qwen"] dl')).toBeDisplayed();

    const add = await $('button[aria-label="New configuration"]');
    await add.click();
    await expect($("[role=dialog]")).toBeDisplayed();
    await browser.keys("Escape");
    await $("[role=dialog]").waitForExist({ reverse: true });
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.activeElement?.getAttribute("aria-label"))) ===
        "New configuration",
    );
  });

  it("keeps dense pages and primary actions reachable at every supported zoom", async function () {
    this.timeout(240_000);
    for (const [width, height] of [
      [1180, 780],
      [900, 620],
      [1536, 1024],
    ]) {
      await browser.setWindowSize(width, height);
      for (const zoom of [100, 125, 150, 175, 200, 225, 250, 275, 300]) {
        const settings = await settingsCommand<AppSettings>("get_settings");
        await settingsCommand("update_settings", {
          settings: { ...settings, uiZoomPercent: zoom },
          expectedRevision: settings.revision,
        });
        await refreshApp();
        for (const pageIndex of [1, 2, 3]) {
          await $(`nav button:nth-child(${pageIndex})`).click();
          await assertNoPageOverflow();
        }
        const save = await $(".page-header button");
        await save.scrollIntoView();
        await expect(save).toBeDisplayed();
      }
    }
    await settingsCommand("set_ui_zoom", { uiZoomPercent: 100 });
    const saved = await settingsCommand<AppSettings>("get_settings");
    await settingsCommand("update_settings", {
      settings: { ...saved, language: "zh-cn", theme: "system", uiZoomPercent: 100 },
      expectedRevision: saved.revision,
    });
    await browser.setWindowSize(1180, 780);
    await refreshApp();
  });
});
