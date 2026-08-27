import { useEffect, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Play, Save, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import { providerDisplayName, providerSupportsCli } from "../../shared/catalog";
import { uniqueCopyName, validateEntityName } from "../../shared/names";
import type {
  ApplyRunSnapshot,
  CliId,
  ConfigurationTarget,
  ProviderCatalog,
  PublicProvider,
  SavedConfiguration,
  ScanSnapshot,
} from "../../shared/types";
import { CLI_IDS } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Badge, Button, Card, Field, Input, Modal, Select, type ErrorReporter } from "../ui";
import { ApplyPreviewDialog } from "./ApplyPreviewDialog";
import { CliTargetRow, makeTarget } from "./CliTargetRow";

export function SavedConfigurationTab({
  configuration,
  matchStatus,
  latestApply,
  providers,
  catalog,
  configurations,
  scan,
  onDeleted,
  onError,
}: {
  configuration: SavedConfiguration;
  matchStatus?: string;
  latestApply?: ApplyRunSnapshot;
  providers: PublicProvider[];
  catalog: ProviderCatalog;
  configurations: SavedConfiguration[];
  scan?: ScanSnapshot;
  onDeleted: () => void;
  onError: ErrorReporter;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const dirty = useUiStore((state) => state.dirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const resumableApply = latestApply && !latestApply.finishedAt ? latestApply : undefined;
  const [name, setName] = useState(configuration.name);
  const [targets, setTargets] = useState<ConfigurationTarget[]>(configuration.targets);
  const [applyOpen, setApplyOpen] = useState(Boolean(resumableApply));
  const [syncProvider, setSyncProvider] = useState("");
  const [duplicateOpen, setDuplicateOpen] = useState(false);
  const [duplicateName, setDuplicateName] = useState("");
  const [deleteOpen, setDeleteOpen] = useState(false);
  const nameIssue = validateEntityName(name, configurations, configuration.id);
  const duplicateNameIssue = validateEntityName(duplicateName, configurations);
  const targetsValid = targets.every(
    (target) =>
      target.model.trim().length > 0 &&
      (target.targetType !== "api" || target.connectionId.length > 0),
  );
  const save = useMutation({
    mutationFn: () =>
      command<SavedConfiguration>("update_configuration", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
        request: { name: name.trim(), targets },
      }),
    onSuccess: (value) => {
      queryClient.setQueryData<SavedConfiguration[]>(["configurations"], (items) =>
        items?.map((item) => (item.id === value.id ? value : item)),
      );
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
      setDirty(false);
    },
    onError: (error) => onError(error, "save"),
  });
  useEffect(() => {
    setSaveCurrent(async () => {
      if (nameIssue || !targetsValid) {
        onError(
          {
            code: "validation",
            message: nameIssue
              ? t(nameIssue === "length" ? "validation.nameLength" : "validation.nameDuplicate")
              : t("validation.configurationTargets"),
          },
          "save",
        );
        return false;
      }
      try {
        await save.mutateAsync();
        return true;
      } catch {
        return false;
      }
    });
    return () => setSaveCurrent(undefined);
  }, [name, targets, configuration.revision]); // eslint-disable-line react-hooks/exhaustive-deps
  const mark = <T,>(setter: (value: T) => void, value: T) => {
    setter(value);
    setDirty(true);
  };
  const updateTarget = (cliId: CliId, target: ConfigurationTarget) =>
    mark(
      setTargets,
      targets.map((item) => (item.cliId === cliId ? target : item)),
    );
  const toggle = (cliId: CliId, included: boolean) => {
    if (!included) {
      mark(
        setTargets,
        targets.filter((target) => target.cliId !== cliId),
      );
      return;
    }
    const provider = providers.find((item) => providerSupportsCli(catalog, cliId, item));
    const target = provider && makeTarget(catalog, cliId, provider);
    if (!target) {
      onError({ code: "validation", message: t("config.noCompatibleProvider") }, "configure");
      return;
    }
    mark(setTargets, [...targets, target]);
  };
  const syncOptions = providers.filter((provider) => provider.kind === "api");
  const sync = () => {
    const provider = providers.find((item) => item.id === syncProvider);
    if (!provider || provider.kind !== "api") return;
    const next = [...targets];
    for (const cliId of CLI_IDS) {
      const target = makeTarget(catalog, cliId, provider);
      if (!target) continue;
      const index = next.findIndex((item) => item.cliId === cliId);
      if (index >= 0) next[index] = target;
      else next.push(target);
    }
    mark(setTargets, next);
  };
  const duplicate = useMutation({
    mutationFn: () =>
      command<SavedConfiguration>("duplicate_configuration", {
        configurationId: configuration.id,
        name: duplicateName.trim(),
      }),
    onSuccess: () => {
      setDuplicateOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["configurations"] });
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(error, "duplicate"),
  });
  const remove = useMutation({
    mutationFn: () =>
      command("delete_configuration", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
      }),
    onSuccess: () => {
      setDeleteOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["configurations"] });
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
      onDeleted();
    },
    onError: (error) => onError(error, "delete"),
  });
  const sortedTargets = useMemo(
    () => CLI_IDS.map((id) => targets.find((target) => target.cliId === id)),
    [targets],
  );
  return (
    <div className="page-section">
      <Card>
        <div className="configuration-header">
          <div className="configuration-name">
            <Field
              label={t("providers.name")}
              hint={
                nameIssue
                  ? t(nameIssue === "length" ? "validation.nameLength" : "validation.nameDuplicate")
                  : undefined
              }
            >
              <Input value={name} onChange={(event) => mark(setName, event.target.value)} />
            </Field>
            {matchStatus ? (
              <Badge tone={matchStatus === "applied" ? "good" : "warn"}>
                {t(`status.${matchStatus}`)}
              </Badge>
            ) : null}
          </div>
          <div className="section-actions">
            <Button
              variant="secondary"
              onClick={() => {
                setDuplicateName(
                  uniqueCopyName(configuration.name, t("common.duplicate"), configurations),
                );
                setDuplicateOpen(true);
              }}
            >
              <Copy size={16} /> {t("common.duplicate")}
            </Button>
            <Button variant="danger" onClick={() => setDeleteOpen(true)}>
              <Trash2 size={16} /> {t("common.delete")}
            </Button>
            <Button
              disabled={save.isPending || Boolean(nameIssue) || !targetsValid}
              onClick={() => save.mutate()}
            >
              <Save size={16} /> {t("common.save")}
              {dirty ? (
                <span className="dirty-marker" aria-label={t("config.unsavedMarker")}>
                  •
                </span>
              ) : null}
            </Button>
            <Button disabled={!targetsValid} onClick={() => setApplyOpen(true)}>
              <Play size={16} /> {t("config.apply")}
            </Button>
          </div>
        </div>
        <p className="muted">
          {t("config.lastApplied")}:{" "}
          {configuration.lastAppliedAt
            ? new Date(configuration.lastAppliedAt).toLocaleString()
            : t("config.neverApplied")}
          {configuration.lastApplySummary ? ` · ${configuration.lastApplySummary}` : ""}
        </p>
        <div className="sync-row">
          <Select value={syncProvider} onChange={(event) => setSyncProvider(event.target.value)}>
            <option value="">{t("config.provider")}</option>
            {syncOptions.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.templateId
                  ? providerDisplayName(catalog, provider.templateId, provider.name)
                  : provider.name}
              </option>
            ))}
          </Select>
          <Button variant="secondary" disabled={!syncProvider} onClick={sync}>
            {t("config.sync")}
          </Button>
        </div>
      </Card>
      <div className="target-list">
        {CLI_IDS.map((cliId, index) => {
          const target = sortedTargets[index];
          const detected = scan?.items.find((item) => item.cliId === cliId);
          return (
            <Card key={cliId}>
              <div className="card-title-row">
                <label className="switch-row">
                  <input
                    type="checkbox"
                    checked={Boolean(target)}
                    onChange={(event) => toggle(cliId, event.target.checked)}
                  />
                  <span>
                    {t("config.included")}: {cliId}
                  </span>
                </label>
                <Badge>{t(`status.${detected?.status ?? "not-scanned"}`)}</Badge>
              </div>
              {target ? (
                <CliTargetRow
                  cliId={cliId}
                  target={target}
                  providers={providers}
                  catalog={catalog}
                  onChange={(value) => updateTarget(cliId, value)}
                />
              ) : null}
            </Card>
          );
        })}
      </div>
      <ApplyPreviewDialog
        configuration={configuration}
        initialRun={resumableApply}
        open={applyOpen}
        onClose={() => setApplyOpen(false)}
      />
      <Modal
        open={duplicateOpen}
        title={t("config.duplicateTitle")}
        onClose={() => setDuplicateOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDuplicateOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              disabled={Boolean(duplicateNameIssue) || duplicate.isPending}
              onClick={() => duplicate.mutate()}
            >
              {t("common.duplicate")}
            </Button>
          </>
        }
      >
        <Field
          label={t("config.duplicateName")}
          hint={
            duplicateNameIssue
              ? t(
                  duplicateNameIssue === "length"
                    ? "validation.nameLength"
                    : "validation.nameDuplicate",
                )
              : undefined
          }
        >
          <Input
            autoFocus
            value={duplicateName}
            onChange={(event) => setDuplicateName(event.target.value)}
          />
        </Field>
      </Modal>
      <Modal
        open={deleteOpen}
        title={t("common.confirmDelete")}
        onClose={() => setDeleteOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>
              {t("common.delete")}
            </Button>
          </>
        }
      >
        <p>
          {configuration.name}: {t("config.deleteWarning")}
        </p>
      </Modal>
    </div>
  );
}
