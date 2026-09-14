import type { AppSettings, AppSnapshot, ProviderCatalog } from "../shared/types";

type AppSnapshotOverrides = Omit<Partial<AppSnapshot>, "settings" | "catalog"> & {
  settings?: Partial<AppSettings>;
  catalog?: Partial<ProviderCatalog>;
};

export function makeAppSnapshot(overrides: AppSnapshotOverrides = {}): AppSnapshot {
  const { settings, catalog, ...snapshotOverrides } = overrides;

  return {
    catalog: {
      schemaVersion: 1,
      clis: [],
      providerTemplates: [],
      relations: [],
      ...catalog,
    },
    settings: {
      language: "zh-cn",
      theme: "system",
      uiZoomPercent: 100,
      scanOnStartup: false,
      plaintextRiskAccepted: false,
      revision: 1,
      manualLocations: [],
      ...settings,
    },
    providers: [],
    configurations: [],
    current: null,
    latestApply: null,
    configurationStatuses: {},
    appDataDirectory: "/tmp/cliswitch",
    backupBytes: 0,
    appVersion: "0.1.0",
    ...snapshotOverrides,
  };
}
