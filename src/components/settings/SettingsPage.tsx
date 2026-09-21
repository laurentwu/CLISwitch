import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database, ExternalLink, FolderOpen, RefreshCw, Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import { cliDisplayName } from "../../shared/names";
import type { AppSettings, AppSnapshot, CatalogStatus, CliId } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Alert, AppSelect, Button, Field, Input, type ErrorReporter } from "../ui";
import { useAppTheme } from "../../app/ThemeProvider";
import { Checkbox } from "../ui/primitives/checkbox";
import { Separator } from "../ui/primitives/separator";
import { FieldGroup } from "../ui/primitives/field";
import { PageHeader } from "../layout/PageHeader";

const UI_ZOOM_PERCENTAGES = [100, 125, 150, 175, 200, 225, 250, 275, 300] as const;

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let amount = value / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && amount >= 1024; index += 1) {
    amount /= 1024;
    unit = units[index];
  }
  return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${unit}`;
}

export function SettingsPage({
  snapshot,
  onError,
}: {
  snapshot: AppSnapshot;
  onError: ErrorReporter;
}) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const { applySavedTheme } = useAppTheme();
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const [settings, setSettings] = useState(snapshot.settings);
  const savedZoomRef = useRef(snapshot.settings.uiZoomPercent);
  const appliedZoomRef = useRef(snapshot.settings.uiZoomPercent);
  const requestedZoomRef = useRef(snapshot.settings.uiZoomPercent);
  const zoomQueueRef = useRef<Promise<void>>(Promise.resolve());
  const [catalogMessage, setCatalogMessage] = useState<string>();
  const [releaseMessage, setReleaseMessage] = useState<string>();
  const catalogStatus = useQuery({
    queryKey: ["catalog-status"],
    queryFn: () => command<CatalogStatus>("get_catalog_status"),
    retry: false,
  });
  const updateCatalog = useMutation({
    mutationFn: () => command<CatalogStatus>("update_catalog"),
    onSuccess: async (value) => {
      setCatalogMessage(t("settings.catalogUpdated"));
      queryClient.setQueryData(["catalog-status"], value);
      await queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: async (error) => {
      onError(error, "catalogUpdate");
      await catalogStatus.refetch();
    },
  });
  const save = useMutation({
    mutationFn: () =>
      command<AppSettings>("update_settings", { settings, expectedRevision: settings.revision }),
    onSuccess: async (value) => {
      savedZoomRef.current = value.uiZoomPercent;
      setSettings(value);
      setDirty(false);
      applySavedTheme(value.theme);
      await i18n.changeLanguage(value.language === "zh-cn" ? "zh-CN" : "en");
      await queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(error, "save"),
  });
  const saveCurrentRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    saveCurrentRef.current = async () => {
      try {
        await save.mutateAsync();
        return true;
      } catch {
        return false;
      }
    };
  });
  const dirty = JSON.stringify(settings) !== JSON.stringify(snapshot.settings);
  useEffect(() => setDirty(dirty), [dirty, setDirty]);
  useEffect(() => {
    const saveCurrent = () => saveCurrentRef.current();
    setSaveCurrent(saveCurrent);
    return () => setSaveCurrent(undefined);
  }, [setSaveCurrent]);
  useEffect(
    () => () => {
      const savedZoom = savedZoomRef.current;
      void zoomQueueRef.current.then(() =>
        command<void>("set_ui_zoom", { uiZoomPercent: savedZoom }).catch((error) =>
          onError(error, "zoom"),
        ),
      );
    },
    [onError],
  );
  const previewZoom = (uiZoomPercent: number) => {
    requestedZoomRef.current = uiZoomPercent;
    setSettings((current) => ({ ...current, uiZoomPercent }));
    zoomQueueRef.current = zoomQueueRef.current.then(async () => {
      try {
        await command<void>("set_ui_zoom", { uiZoomPercent });
        appliedZoomRef.current = uiZoomPercent;
      } catch (error) {
        if (requestedZoomRef.current === uiZoomPercent) {
          const appliedZoom = appliedZoomRef.current;
          requestedZoomRef.current = appliedZoom;
          setSettings((current) => ({ ...current, uiZoomPercent: appliedZoom }));
        }
        onError(error, "zoom");
      }
    });
  };
  const choose = async (cliId: CliId, kind: "executable" | "directory") => {
    try {
      const value = await command<AppSettings | null>(
        kind === "executable" ? "select_cli_executable" : "select_cli_config_directory",
        { cliId },
      );
      if (value) {
        setSettings((current) => ({
          ...current,
          revision: value.revision,
          manualLocations: value.manualLocations,
        }));
        await queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
      }
    } catch (error) {
      onError(error, "selectPath");
    }
  };
  const clearLocation = (cliId: CliId, kind: "executable" | "directory") => {
    setSettings((current) => ({
      ...current,
      manualLocations: current.manualLocations.map((location) =>
        location.cliId === cliId
          ? {
              ...location,
              [kind === "executable" ? "executablePath" : "configDirectory"]: null,
            }
          : location,
      ),
    }));
  };
  const checkUpdate = async () => {
    try {
      const value = await command<{ updateAvailable: boolean; latestVersion: string }>(
        "check_github_release",
      );
      setReleaseMessage(value.updateAvailable ? `v${value.latestVersion}` : t("settings.upToDate"));
    } catch (error) {
      onError(error, "updateCheck");
    }
  };
  return (
    <div className="page settings-page">
      <PageHeader
        title={t("settings.title")}
        description={`CLISwitch ${snapshot.appVersion}`}
        actions={
          <Button disabled={save.isPending} onClick={() => save.mutate()}>
            <Save size={16} /> {t("common.save")}
          </Button>
        }
      />
      <section className="settings-section">
        <h2>{t("settings.appearanceBehavior")}</h2>
        <FieldGroup className="form-grid settings-preferences-grid">
          <Field label={t("settings.language")}>
            <AppSelect
              value={settings.language}
              onValueChange={(value) =>
                setSettings({
                  ...settings,
                  language: value as AppSettings["language"],
                })
              }
              groups={[
                {
                  options: [
                    { value: "zh-cn", label: "简体中文" },
                    { value: "en", label: "English" },
                  ],
                },
              ]}
            />
          </Field>
          <Field label={t("settings.theme")}>
            <AppSelect
              value={settings.theme}
              onValueChange={(value) =>
                setSettings({ ...settings, theme: value as AppSettings["theme"] })
              }
              groups={[
                {
                  options: [
                    { value: "light", label: t("settings.light") },
                    { value: "dark", label: t("settings.dark") },
                    { value: "system", label: t("settings.system") },
                  ],
                },
              ]}
            />
          </Field>
          <Field label={t("settings.uiZoom")}>
            <AppSelect
              value={String(settings.uiZoomPercent)}
              onValueChange={(value) => previewZoom(Number(value))}
              groups={[
                {
                  options: UI_ZOOM_PERCENTAGES.map((value) => ({
                    value: String(value),
                    label: `${value}%`,
                  })),
                },
              ]}
            />
          </Field>
        </FieldGroup>
        <label className="switch-row" htmlFor="settings-scan-startup">
          <Checkbox
            id="settings-scan-startup"
            checked={settings.scanOnStartup}
            onCheckedChange={(checked) =>
              setSettings({ ...settings, scanOnStartup: checked === true })
            }
          />
          {t("settings.scanStartup")}
        </label>
        <label className="switch-row" htmlFor="settings-risk-accepted">
          <Checkbox
            id="settings-risk-accepted"
            checked={settings.plaintextRiskAccepted}
            onCheckedChange={(checked) =>
              setSettings({ ...settings, plaintextRiskAccepted: checked === true })
            }
          />
          {t("settings.plaintextAck")}
        </label>
      </section>
      <Separator />
      <section className="settings-section">
        <h2>{t("settings.locations")}</h2>
        <div className="locations-list">
          {settings.manualLocations.map((location) => (
            <div className="location-row" key={location.cliId}>
              <strong>{cliDisplayName(location.cliId)}</strong>
              <div className="location-control">
                <Input
                  readOnly
                  value={location.executablePath ?? ""}
                  aria-label={`${cliDisplayName(location.cliId)} · ${t("config.executable")}`}
                  placeholder={t("settings.autoDetected")}
                />
                <Button
                  variant="secondary"
                  aria-label={t("settings.chooseExecutableFor", {
                    cli: cliDisplayName(location.cliId),
                  })}
                  onClick={() => choose(location.cliId, "executable")}
                >
                  {t("settings.chooseExecutable")}
                </Button>
                <Button
                  variant="ghost"
                  disabled={!location.executablePath}
                  onClick={() => clearLocation(location.cliId, "executable")}
                >
                  {t("common.clear")}
                </Button>
              </div>
              <div className="location-control">
                <Input
                  readOnly
                  value={location.configDirectory ?? ""}
                  aria-label={`${cliDisplayName(location.cliId)} · ${t("config.directory")}`}
                  placeholder={t("settings.autoDetected")}
                />
                <Button
                  variant="secondary"
                  aria-label={t("settings.chooseDirectoryFor", {
                    cli: cliDisplayName(location.cliId),
                  })}
                  onClick={() => choose(location.cliId, "directory")}
                >
                  {t("settings.chooseDirectory")}
                </Button>
                <Button
                  variant="ghost"
                  disabled={!location.configDirectory}
                  onClick={() => clearLocation(location.cliId, "directory")}
                >
                  {t("common.clear")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      </section>
      <Separator />
      <section className="settings-section">
        <h2>{t("settings.dataBackups")}</h2>
        <p className="muted">{t("settings.dataDirectory")}</p>
        <div className="input-action">
          <Input
            aria-label={t("settings.dataDirectory")}
            readOnly
            value={snapshot.appDataDirectory}
          />
          <Button
            variant="secondary"
            onClick={() =>
              command("open_app_data_directory").catch((error) => onError(error, "open"))
            }
          >
            <FolderOpen size={15} /> {t("settings.openDirectory")}
          </Button>
        </div>
        <p>
          {t("settings.backupUsage")}: {formatBytes(snapshot.backupBytes)}
        </p>
      </section>
      <Separator />
      <section className="settings-section">
        <div className="card-title-row">
          <h2>
            <Database size={18} /> {t("settings.catalogTitle")}
          </h2>
          <Button
            variant="secondary"
            disabled={updateCatalog.isPending}
            onClick={() => updateCatalog.mutate()}
          >
            <RefreshCw size={15} />
            {updateCatalog.isPending ? t("settings.catalogUpdating") : t("settings.catalogUpdate")}
          </Button>
        </div>
        {catalogStatus.isPending ? <p>{t("common.loading")}</p> : null}
        {catalogStatus.isError ? (
          <Alert tone="warning" title={t("settings.catalogStatusUnavailable")} />
        ) : null}
        {catalogStatus.data ? (
          <div className="catalog-status-grid">
            <span>
              {t("settings.catalogSource")}:{" "}
              {t(`settings.catalogSource_${catalogStatus.data.source}`)}
            </span>
            <span>
              {t("settings.catalogCounts", {
                providers: catalogStatus.data.providerCount,
              })}
            </span>
            <span>
              {t("settings.catalogUpdatedAt")}: {catalogStatus.data.fetchedAt ?? t("common.none")}
            </span>
            <span className="path-text">{catalogStatus.data.cachePath}</span>
          </div>
        ) : null}
        {catalogStatus.data?.lastError ? (
          <Alert tone="warning" title={catalogStatus.data.lastError} />
        ) : null}
        {catalogMessage ? <Alert tone="info" title={catalogMessage} announce /> : null}
      </section>
      <Separator />
      <section className="settings-section">
        <h2>{t("settings.about")}</h2>
        <div className="card-title-row">
          <div>
            <h2>
              {t("settings.version")}: {snapshot.appVersion}
            </h2>
            <p>
              Apache-2.0 · {t("settings.thirdParty")}: THIRD_PARTY_NOTICES.md ·
              github.com/laurentwu/CLISwitch
            </p>
          </div>
          <Button variant="secondary" onClick={checkUpdate}>
            <ExternalLink size={15} /> {t("settings.checkUpdate")}
          </Button>
        </div>
        {releaseMessage ? <Alert tone="info" title={releaseMessage} announce /> : null}
      </section>
    </div>
  );
}
