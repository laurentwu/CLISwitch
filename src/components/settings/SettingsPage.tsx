import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database, ExternalLink, FolderOpen, RefreshCw, Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import type { AppSettings, AppSnapshot, CatalogStatus, CliId } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Alert, Button, Card, Field, Input, Select, type ErrorReporter } from "../ui";

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
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const [settings, setSettings] = useState(snapshot.settings);
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
      setSettings(value);
      setDirty(false);
      document.documentElement.dataset.theme = value.theme;
      if (value.theme === "system") delete document.documentElement.dataset.theme;
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
      <header className="page-header">
        <div>
          <h1>{t("settings.title")}</h1>
          <p>CLISwitch {snapshot.appVersion}</p>
        </div>
        <Button disabled={save.isPending} onClick={() => save.mutate()}>
          <Save size={16} /> {t("common.save")}
        </Button>
      </header>
      <Card>
        <div className="form-grid two-columns">
          <Field label={t("settings.language")}>
            <Select
              value={settings.language}
              onChange={(event) =>
                setSettings({
                  ...settings,
                  language: event.target.value as AppSettings["language"],
                })
              }
            >
              <option value="zh-cn">简体中文</option>
              <option value="en">English</option>
            </Select>
          </Field>
          <Field label={t("settings.theme")}>
            <Select
              value={settings.theme}
              onChange={(event) =>
                setSettings({ ...settings, theme: event.target.value as AppSettings["theme"] })
              }
            >
              <option value="light">{t("settings.light")}</option>
              <option value="dark">{t("settings.dark")}</option>
              <option value="system">{t("settings.system")}</option>
            </Select>
          </Field>
        </div>
        <label className="switch-row">
          <input
            type="checkbox"
            checked={settings.scanOnStartup}
            onChange={(event) => setSettings({ ...settings, scanOnStartup: event.target.checked })}
          />
          {t("settings.scanStartup")}
        </label>
      </Card>
      <Card className="risk-card">
        <h2>{t("settings.riskTitle")}</h2>
        <p>{t("settings.riskText")}</p>
        <label className="switch-row">
          <input
            type="checkbox"
            checked={settings.plaintextRiskAccepted}
            onChange={(event) =>
              setSettings({ ...settings, plaintextRiskAccepted: event.target.checked })
            }
          />
          {t("settings.plaintextAck")}
        </label>
      </Card>
      <Card>
        <h2>{t("settings.locations")}</h2>
        <div className="locations-list">
          {settings.manualLocations.map((location) => (
            <div className="location-row" key={location.cliId}>
              <strong>{location.cliId}</strong>
              <Input
                readOnly
                value={location.executablePath ?? ""}
                placeholder={t("settings.chooseExecutable")}
              />
              <Button variant="secondary" onClick={() => choose(location.cliId, "executable")}>
                {t("settings.chooseExecutable")}
              </Button>
              <Input
                readOnly
                value={location.configDirectory ?? ""}
                placeholder={t("settings.chooseDirectory")}
              />
              <Button variant="secondary" onClick={() => choose(location.cliId, "directory")}>
                {t("settings.chooseDirectory")}
              </Button>
            </div>
          ))}
        </div>
      </Card>
      <Card>
        <h2>{t("settings.dataDirectory")}</h2>
        <div className="input-action">
          <Input readOnly value={snapshot.appDataDirectory} />
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
      </Card>
      <Card>
        <div className="card-title-row">
          <div>
            <h2>
              <Database size={18} /> {t("settings.catalogTitle")}
            </h2>
          </div>
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
      </Card>
      <Card>
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
      </Card>
    </div>
  );
}
