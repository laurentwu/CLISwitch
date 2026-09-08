import type {
  ApiProviderDraft,
  ApplyRunSnapshot,
  AppSettings,
  AppSnapshot,
  BackupMetadata,
  PublicProvider,
  SavedConfiguration,
  ScanSnapshot,
} from "../src/shared/types";

type CommandRequest = {
  command: string;
  args: Record<string, unknown>;
};

async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return (await browser.tauri.execute(
    ({ core }, request: CommandRequest) => core.invoke(request.command, request.args),
    { command, args },
  )) as T;
}

async function scanWhenIdle(): Promise<ScanSnapshot> {
  let scan: ScanSnapshot | undefined;
  let unexpectedError: unknown;
  await browser.waitUntil(
    async () => {
      try {
        scan = await invoke<ScanSnapshot>("scan_clis");
        return true;
      } catch (error) {
        const message =
          error instanceof Error ? error.message : (JSON.stringify(error) ?? String(error));
        if (!message.includes("another apply or restore operation is active")) {
          unexpectedError = error;
          return true;
        }
        return false;
      }
    },
    {
      interval: 250,
      timeout: 30_000,
      timeoutMsg: "Expected the active desktop operation to finish before scanning",
    },
  );
  if (unexpectedError) throw unexpectedError;
  if (!scan) throw new Error("Scan completed without a snapshot");
  return scan;
}

describe("CLISwitch desktop shell", () => {
  const waitForSelectedConfiguration = async (name: string) => {
    await browser.waitUntil(
      async () => {
        const selected = await $('[role=tab][aria-selected="true"]');
        return (await selected.isExisting()) && (await selected.getText()) === name;
      },
      { timeoutMsg: `Expected configuration tab "${name}" to become selected` },
    );
  };

  it("opens on Current configuration and exposes exactly three top-level sections", async () => {
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    const navigation = await $$("nav button");
    await expect(navigation).toBeElementsArrayOfSize(3);
    await expect($("[role=tab]")).toHaveText(
      expect.stringMatching(/Current configuration|当前配置/),
    );

    const cliCards = await $$(".cli-card-grid .card");
    await expect(cliCards).toBeElementsArrayOfSize(4);
    for (const version of await $$(".cli-card-grid small")) {
      await expect(version).toHaveText("fixture-cli 0.1.0");
    }
    await expect(
      $("//*[contains(@class, 'cli-card-grid')]//*[contains(., 'Qwen Code')]"),
    ).toBeDisplayed();
  });

  it("creates a named configuration and keeps the three-section navigation usable", async () => {
    const add = await $('button[aria-label="新建配置"], button[aria-label="New configuration"]');
    await add.click();
    await expect($("[role=dialog]")).toBeDisplayed();
    await $("[role=dialog] input").setValue("E2E configuration");
    const create = await $("[role=dialog] .modal-footer button:last-child");
    await create.click();
    await waitForSelectedConfiguration("E2E configuration");

    const navigation = await $$("nav button");
    await navigation[1].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Providers|供应商/));
    await navigation[2].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Settings|设置/));
    await navigation[0].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    await waitForSelectedConfiguration("E2E configuration");
  });

  it("imports Qwen and completes preview, A/B/A switching, rescan, and restore", async () => {
    const originalScan = await scanWhenIdle();
    const originalQwen = originalScan.items.find((item) => item.cliId === "qwen");
    const originalDigest = originalQwen?.current?.sources.find(
      (source) => source.sourceId === "qwen-settings",
    )?.digest;
    expect(originalDigest).toBeTruthy();

    const navigation = await $$("nav button");
    await navigation[2].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Settings|设置/));
    const riskCheckbox = await $(".risk-card input[type=checkbox]");
    if (!(await riskCheckbox.isSelected())) await riskCheckbox.click();
    await $(".page-header button").click();
    await browser.waitUntil(async () => {
      const settings = await invoke<AppSettings>("get_settings");
      return settings.plaintextRiskAccepted;
    });

    await navigation[0].click();
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    const currentConfigurationTab = await $(
      "//button[@role='tab' and (normalize-space()='Current configuration' or normalize-space()='当前配置')]",
    );
    await currentConfigurationTab.waitForClickable();
    await currentConfigurationTab.click();
    await browser.waitUntil(
      async () => (await currentConfigurationTab.getAttribute("aria-selected")) === "true",
      { timeoutMsg: "Expected Current configuration to become selected" },
    );
    await scanWhenIdle();
    const scanButton = await $(
      "//button[contains(normalize-space(.), 'Scan') or contains(normalize-space(.), '扫描')]",
    );
    const previousScanId = (await invoke<AppSnapshot>("get_app_snapshot")).current?.id;
    await scanButton.waitForClickable();
    await scanButton.click();
    await browser.waitUntil(
      async () => {
        const snapshot = await invoke<AppSnapshot>("get_app_snapshot");
        return Boolean(snapshot.current?.id && snapshot.current.id !== previousScanId);
      },
      { timeout: 30_000, timeoutMsg: "Expected the UI scan to finish" },
    );
    const manageCandidate = await $(
      "//*[contains(@class, 'card')][.//h3[normalize-space()='Qwen Code']]//button[contains(normalize-space(.), 'as provider') or contains(normalize-space(.), '保存为供应商')]",
    );
    await manageCandidate.waitForClickable();
    await manageCandidate.click();
    const candidateDialog = await $("[role=dialog]");
    await candidateDialog.$("input:not(#candidate-model)").setValue("Qwen account A");
    await candidateDialog.$("#candidate-model").setValue("fixture-qwen-model");
    await candidateDialog.$(".modal-footer button:last-child").click();

    const qwenCard = await $("//*[contains(@class, 'card')][.//h3[normalize-space()='Qwen Code']]");
    await browser.waitUntil(async () => (await qwenCard.getText()).includes("Qwen account A"));

    await $(
      "//button[contains(normalize-space(.), 'Save as new configuration') or contains(normalize-space(.), '保存为新配置')]",
    ).click();
    const saveCurrentDialog = await $("[role=dialog]");
    await saveCurrentDialog.$("input").setValue("Qwen account A configuration");
    await saveCurrentDialog.$(".modal-footer button:last-child").click();
    await $(
      "//button[@role='tab' and normalize-space()='Qwen account A configuration']",
    ).waitForExist();

    const providers = await invoke<PublicProvider[]>("list_providers");
    const providerA = providers.find((provider) => provider.name === "Qwen account A");
    expect(providerA?.connections).toHaveLength(1);
    const connectionA = providerA!.connections[0];
    const providerBDraft: ApiProviderDraft = {
      name: "Qwen account B",
      connections: [
        {
          credentialSlotId: "api-key",
          protocol: "openai-chat",
          endpoint: "https://qwen-e2e.invalid/v1",
          authType: "bearer",
          apiKey: "fixture-qwen-key-b-not-real",
          defaultModel: "fixture-qwen-model",
        },
      ],
    };
    const providerB = await invoke<PublicProvider>("create_provider", { draft: providerBDraft });
    const configurationB = await invoke<SavedConfiguration>("create_configuration", {
      request: {
        name: "Qwen account B configuration",
        targets: [
          {
            targetType: "api",
            cliId: "qwen",
            providerId: providerB.id,
            connectionId: providerB.connections[0].id,
            model: "fixture-qwen-model",
          },
        ],
      },
    });
    await browser.refresh();
    await expect($("h1")).toHaveText(expect.stringMatching(/Configurations|配置/));
    const configurationA = (await invoke<SavedConfiguration[]>("list_configurations")).find(
      (configuration) => configuration.name === "Qwen account A configuration",
    );
    expect(configurationA).toBeDefined();
    const initialBackups = await invoke<BackupMetadata[]>("list_backups", { cliId: "qwen" });
    expect(initialBackups).toHaveLength(0);

    const applyConfiguration = async (
      name: string,
      configurationId: string,
      expectedConnectionId: string,
    ) => {
      await $(`//button[@role='tab' and normalize-space()=${JSON.stringify(name)}]`).click();
      await waitForSelectedConfiguration(name);
      const previousRunId = (await invoke<AppSnapshot>("get_app_snapshot")).latestApply?.id;
      await $(".configuration-header .section-actions button:last-child").click();
      let completedRun: ApplyRunSnapshot | undefined;
      await browser.waitUntil(
        async () => {
          const snapshot = await invoke<AppSnapshot>("get_app_snapshot");
          const run = snapshot.latestApply;
          if (
            !run?.finishedAt ||
            run.id === previousRunId ||
            run.configurationId !== configurationId
          )
            return false;
          const item = run.items.find((candidate) => candidate.cliId === "qwen");
          if (item?.state !== "success" && item?.state !== "unchanged") return false;
          completedRun = run;
          return true;
        },
        { timeout: 30_000, timeoutMsg: `Qwen apply did not finish for ${name}` },
      );
      if (!completedRun) throw new Error(`Qwen apply finished without a snapshot for ${name}`);
      await $(
        "//*[@role='dialog']//*[contains(@class, 'modal-footer')]//button[contains(normalize-space(.), 'Close') or contains(normalize-space(.), '关闭')]",
      ).click();
      const scan = await scanWhenIdle();
      const qwen = scan.items.find((item) => item.cliId === "qwen");
      expect(qwen?.current?.managedConnectionId).toBe(expectedConnectionId);
      return completedRun;
    };

    await $("//button[@role='tab' and normalize-space()='Qwen account A configuration']").click();
    const qwenTarget = await $(
      "//*[contains(@class, 'target-list')]//*[contains(@class, 'card')][contains(., 'Qwen Code')]",
    );
    await qwenTarget.$("button").click();
    const previewDialog = await $("[role=dialog]");
    await expect(previewDialog).toHaveText(expect.stringMatching(/Qwen Code/));
    await expect(previewDialog).toHaveText(expect.stringMatching(/settings\.json/));
    await previewDialog.$(".modal-footer button").click();

    const runs = [
      await applyConfiguration("Qwen account A configuration", configurationA!.id, connectionA.id),
      await applyConfiguration(
        "Qwen account B configuration",
        configurationB.id,
        providerB.connections[0].id,
      ),
      await applyConfiguration("Qwen account A configuration", configurationA!.id, connectionA.id),
    ];
    const successfulWrites = runs.filter(
      (run) => run.items.find((item) => item.cliId === "qwen")?.state === "success",
    ).length;
    expect(successfulWrites).toBeGreaterThanOrEqual(2);
    let backupsAfterApply: BackupMetadata[] = [];
    await browser.waitUntil(
      async () => {
        backupsAfterApply = await invoke<BackupMetadata[]>("list_backups", { cliId: "qwen" });
        return backupsAfterApply.length === successfulWrites;
      },
      {
        timeout: 30_000,
        timeoutMsg: "Expected each successful Qwen write to create a backup",
      },
    );

    await $(
      "//button[@role='tab' and (normalize-space()='Current configuration' or normalize-space()='当前配置')]",
    ).click();
    const qwenBackupButton = await $(
      "//*[contains(@class, 'card')][.//h3[normalize-space()='Qwen Code']]//button[contains(normalize-space(.), 'Backups') or contains(normalize-space(.), '备份')]",
    );
    await qwenBackupButton.click();
    await browser.waitUntil(
      async () => (await $$("[role=dialog] .backup-row")).length === backupsAfterApply.length,
      {
        timeout: 30_000,
        timeoutMsg: "Expected Qwen backups to load",
      },
    );
    const backupRows = await $$("[role=dialog] .backup-row");
    expect(backupRows.length).toBe(backupsAfterApply.length);
    await backupRows[backupRows.length - 1].$("button").click();
    const restoreDialog = await $$("[role=dialog]");
    await restoreDialog[restoreDialog.length - 1].$(".modal-footer button:last-child").click();

    const backupsBeforeRestore = new Set(backupsAfterApply.map((backup) => backup.id));
    await browser.waitUntil(
      async () => {
        const backups = await invoke<BackupMetadata[]>("list_backups", { cliId: "qwen" });
        return backups.some((backup) => !backupsBeforeRestore.has(backup.id));
      },
      { timeoutMsg: "Expected Qwen restore to create an undo backup" },
    );
    const restoredScan = await scanWhenIdle();
    const restoredQwen = restoredScan.items.find((item) => item.cliId === "qwen");
    expect(
      restoredQwen?.current?.sources.find((source) => source.sourceId === "qwen-settings")?.digest,
    ).toBe(originalDigest);
    expect(restoredQwen?.current?.managedConnectionId).toBe(connectionA.id);
    expect(configurationB.targets[0]).toMatchObject({ cliId: "qwen" });
  });
});
