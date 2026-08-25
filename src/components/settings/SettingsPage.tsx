import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, FolderOpen, Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import type { AppSettings, AppSnapshot, CliId } from "../../shared/types";
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
  const [updateMessage, setUpdateMessage] = useState<string>();
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
      setUpdateMessage(value.updateAvailable ? `v${value.latestVersion}` : t("settings.upToDate"));
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
        {updateMessage ? <Alert tone="info" title={updateMessage} announce /> : null}
      </Card>
    </div>
  );
}
