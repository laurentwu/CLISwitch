import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArchiveRestore, RefreshCw, Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, errorMessage } from "../../shared/ipc";
import { validateEntityName } from "../../shared/names";
import type {
  AppSettings,
  CliId,
  DetectedCli,
  DetectedProviderCandidate,
  ProviderCatalog,
  PublicProvider,
  SavedConfiguration,
  ScanSnapshot,
} from "../../shared/types";
import { CLI_IDS } from "../../shared/types";
import { Badge, Button, Card, Field, Input, Modal, Spinner } from "../ui";
import { BackupRestoreDialog } from "./BackupRestoreDialog";

function statusTone(status: DetectedCli["status"]): "neutral" | "good" | "warn" | "bad" {
  if (status === "detected") return "good";
  if (["unmanaged", "partially-detected", "externally-overridden"].includes(status)) return "warn";
  if (["unreadable", "invalid-config"].includes(status)) return "bad";
  return "neutral";
}

export function CurrentConfigurationTab({
  scan,
  configurations,
  providers,
  catalog,
  onError,
}: {
  scan?: ScanSnapshot | null;
  configurations: SavedConfiguration[];
  providers: PublicProvider[];
  catalog: ProviderCatalog;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [candidate, setCandidate] = useState<DetectedProviderCandidate>();
  const [candidateName, setCandidateName] = useState("");
  const [candidateModel, setCandidateModel] = useState("");
  const [saveOpen, setSaveOpen] = useState(false);
  const [configurationName, setConfigurationName] = useState("");
  const [backupsOpen, setBackupsOpen] = useState(false);
  const [backupCli, setBackupCli] = useState<CliId>();
  const refresh = useMutation({
    mutationFn: () => command<ScanSnapshot>("scan_clis"),
    onSuccess: (value) => {
      queryClient.setQueryData(["scan"], value);
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(errorMessage(error)),
  });
  const saveCandidate = useMutation({
    mutationFn: () =>
      command<PublicProvider>("save_unmanaged_candidate_provider", {
        snapshotId: scan?.id,
        candidateId: candidate?.id,
        name: candidateName.trim(),
        defaultModel: candidateModel || null,
      }),
    onSuccess: () => {
      setCandidate(undefined);
      setCandidateName("");
      setCandidateModel("");
      void queryClient.invalidateQueries({ queryKey: ["providers"] });
      refresh.mutate();
    },
    onError: (error) => onError(errorMessage(error)),
  });
  const saveCurrent = useMutation({
    mutationFn: () =>
      command<SavedConfiguration>("save_current_as_configuration", {
        name: configurationName.trim(),
      }),
    onSuccess: () => {
      setSaveOpen(false);
      setConfigurationName("");
      void queryClient.invalidateQueries({ queryKey: ["configurations"] });
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(errorMessage(error)),
  });
  const chooseConfigDirectory = async (cliId: CliId) => {
    try {
      const selected = await command<AppSettings | null>("select_cli_config_directory", { cliId });
      if (selected) {
        await queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
        refresh.mutate();
      }
    } catch (error) {
      onError(errorMessage(error));
    }
  };
  const candidateNameIssue = validateEntityName(candidateName, providers);
  const configurationNameIssue = validateEntityName(configurationName, configurations);
  const candidateTemplateName = candidate?.templateId
    ? (catalog.providerTemplates.find((template) => template.id === candidate.templateId)?.name ??
      candidate.templateId)
    : t("providers.customTemplate");
  return (
    <div className="page-section">
      <div className="section-actions">
        <Button variant="secondary" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
          {refresh.isPending ? <Spinner /> : <RefreshCw size={16} />} {t("config.scan")}
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setBackupCli(undefined);
            setBackupsOpen(true);
          }}
        >
          <ArchiveRestore size={16} /> {t("config.backups")}
        </Button>
        <Button disabled={!scan} onClick={() => setSaveOpen(true)}>
          <Save size={16} /> {t("config.saveCurrent")}
        </Button>
      </div>
      <div className="cli-card-grid">
        {CLI_IDS.map((cliId) => {
          const item = scan?.items.find((candidate) => candidate.cliId === cliId);
          if (!item) {
            const label =
              cliId === "claude-code"
                ? "Claude Code"
                : cliId === "codex"
                  ? "Codex CLI"
                  : "OpenCode";
            return (
              <Card key={cliId}>
                <header className="card-title-row">
                  <span className="cli-mark" aria-hidden="true">
                    {label.slice(0, 2).toUpperCase()}
                  </span>
                  <h3>{label}</h3>
                  <Badge>{t("config.notScanned")}</Badge>
                </header>
                <div className="section-actions">
                  <Button variant="secondary" onClick={() => chooseConfigDirectory(cliId)}>
                    {t("settings.chooseDirectory")}
                  </Button>
                </div>
              </Card>
            );
          }
          return (
            <Card key={item.cliId}>
              <header className="card-title-row">
                <span className="cli-mark" aria-hidden="true">
                  {item.label.slice(0, 2).toUpperCase()}
                </span>
                <div>
                  <h3>{item.label}</h3>
                  <small>{item.version ?? "—"}</small>
                </div>
                <Badge tone={statusTone(item.status)}>{t(`status.${item.status}`)}</Badge>
              </header>
              <dl className="detail-grid">
                <dt>{t("config.executable")}</dt>
                <dd className="path-text">{item.executablePath ?? "—"}</dd>
                <dt>{t("config.directory")}</dt>
                <dd className="path-text">{item.configDirectory}</dd>
                <dt>{t("config.provider")}</dt>
                <dd>{item.current?.providerName ?? "—"}</dd>
                <dt>{t("config.protocol")}</dt>
                <dd>{item.current?.protocol ?? "—"}</dd>
                <dt>{t("providers.authType")}</dt>
                <dd>{item.current?.authKind ?? "—"}</dd>
                <dt>{t("config.model")}</dt>
                <dd>{item.current?.model ?? "—"}</dd>
              </dl>
              {item.current?.diagnostics.map((message, index) => (
                <div className="diagnostic" key={index}>
                  {message}
                </div>
              ))}
              <div className="section-actions">
                <Button variant="secondary" onClick={() => chooseConfigDirectory(item.cliId)}>
                  {t("settings.chooseDirectory")}
                </Button>
                <Button
                  variant="secondary"
                  onClick={() => {
                    setBackupCli(item.cliId);
                    setBackupsOpen(true);
                  }}
                >
                  <ArchiveRestore size={15} /> {t("config.backups")}
                </Button>
              </div>
              {item.providerCandidates?.length ? (
                <div className="section-actions">
                  {item.providerCandidates.map((providerCandidate) => (
                    <Button
                      key={providerCandidate.id}
                      variant="secondary"
                      onClick={() => {
                        setCandidate(providerCandidate);
                        setCandidateName(providerCandidate.suggestedName);
                        setCandidateModel(
                          providerCandidate.defaultModel ??
                            providerCandidate.availableModels[0] ??
                            "",
                        );
                      }}
                    >
                      {t("config.manageCandidateNamed", {
                        name: providerCandidate.suggestedName,
                      })}
                    </Button>
                  ))}
                </div>
              ) : null}
            </Card>
          );
        })}
      </div>
      {!scan ? (
        <Card>
          <p>{t("config.scan")}</p>
          <Button onClick={() => refresh.mutate()}>{t("config.scan")}</Button>
        </Card>
      ) : null}
      <Modal
        open={Boolean(candidate)}
        title={t("config.manageCandidate")}
        onClose={() => {
          setCandidate(undefined);
          setCandidateModel("");
        }}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => {
                setCandidate(undefined);
                setCandidateModel("");
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button
              disabled={Boolean(candidateNameIssue) || saveCandidate.isPending}
              onClick={() => saveCandidate.mutate()}
            >
              {t("common.save")}
            </Button>
          </>
        }
      >
        <Field
          label={t("providers.name")}
          hint={
            candidateNameIssue
              ? t(
                  candidateNameIssue === "length"
                    ? "validation.nameLength"
                    : "validation.nameDuplicate",
                )
              : undefined
          }
        >
          <Input
            autoFocus
            value={candidateName}
            onChange={(event) => setCandidateName(event.target.value)}
          />
        </Field>
        {candidate ? (
          <Field label={t("providers.defaultModel")} hint={t("providers.modelHint")}>
            <Input
              list="candidate-models"
              value={candidateModel}
              onChange={(event) => setCandidateModel(event.target.value)}
            />
            <datalist id="candidate-models">
              {candidate.availableModels.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </datalist>
          </Field>
        ) : null}
        {candidate ? (
          <dl className="detail-grid">
            <dt>{t("config.cliProviderId")}</dt>
            <dd>{candidate.sourceProviderId}</dd>
            <dt>{t("providers.template")}</dt>
            <dd>{candidateTemplateName}</dd>
            <dt>{t("config.protocol")}</dt>
            <dd>{candidate.protocol ?? "—"}</dd>
            <dt>{t("providers.endpoint")}</dt>
            <dd className="path-text">{candidate.endpoint ?? "—"}</dd>
          </dl>
        ) : null}
      </Modal>
      <Modal
        open={saveOpen}
        title={t("config.saveCurrent")}
        onClose={() => setSaveOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setSaveOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              disabled={Boolean(configurationNameIssue) || saveCurrent.isPending}
              onClick={() => saveCurrent.mutate()}
            >
              {t("common.save")}
            </Button>
          </>
        }
      >
        <Field
          label={t("providers.name")}
          hint={
            configurationNameIssue
              ? t(
                  configurationNameIssue === "length"
                    ? "validation.nameLength"
                    : "validation.nameDuplicate",
                )
              : undefined
          }
        >
          <Input
            autoFocus
            value={configurationName}
            onChange={(event) => setConfigurationName(event.target.value)}
          />
        </Field>
      </Modal>
      <BackupRestoreDialog
        open={backupsOpen}
        cliId={backupCli}
        onClose={() => setBackupsOpen(false)}
        onError={onError}
      />
    </div>
  );
}
