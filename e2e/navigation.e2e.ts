describe("CLISwitch desktop shell", () => {
  it("opens on Current configuration and exposes exactly three top-level sections", async () => {
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    const navigation = await $$("nav button");
    await expect(navigation).toBeElementsArrayOfSize(3);
    await expect($("[role=tab]")).toHaveText(
      expect.stringMatching(/Current configuration|当前配置/),
    );

    const cliCards = await $$(".cli-card-grid .card");
    await expect(cliCards).toBeElementsArrayOfSize(3);
    for (const version of await $$(".cli-card-grid small")) {
      await expect(version).toHaveText("fixture-cli 0.1.0");
    }
  });

  it("creates a named configuration and keeps the three-section navigation usable", async () => {
    const add = await $('button[aria-label="新建配置"], button[aria-label="New configuration"]');
    await add.click();
    await expect($("[role=dialog]")).toBeDisplayed();
    await $("[role=dialog] input").setValue("E2E configuration");
    const create = await $("[role=dialog] .modal-footer button:last-child");
    await create.click();
    await expect($('[role=tab][aria-selected="true"]')).toHaveText("E2E configuration");

    const navigation = await $$("nav button");
    await navigation[1].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Providers|供应商/));
    await navigation[2].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Settings|设置/));
    await navigation[0].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    await expect($('[role=tab][aria-selected="true"]')).toHaveText("E2E configuration");
  });
});
