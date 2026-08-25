import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { command, errorMessage } from "../../shared/ipc";
import { validateEntityName } from "../../shared/names";
import type {
  AppSnapshot,
  PublicProvider,
  SavedConfiguration,
  ScanSnapshot,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Button, Field, Input, Modal } from "../ui";
import { ConfigurationTabs } from "./ConfigurationTabs";
import { CurrentConfigurationTab } from "./CurrentConfigurationTab";
import { SavedConfigurationTab } from "./SavedConfigurationTab";

export function ConfigurationPage({
  snapshot,
  guarded,
  onError,
}: {
  snapshot: AppSnapshot;
  guarded: (action: () => void) => void;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const active = useUiStore((state) => state.configurationId);
  const dirty = useUiStore((state) => state.dirty);
  const setActive = useUiStore((state) => state.setConfigurationId);
  const setDirty = useUiStore((state) => state.setDirty);
  const [addOpen, setAddOpen] = useState(false);
  const [name, setName] = useState("");
  const configurations = useQuery({
    queryKey: ["configurations"],
    queryFn: () => command<SavedConfiguration[]>("list_configurations"),
    initialData: snapshot.configurations,
  });
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: () => command<PublicProvider[]>("list_providers"),
    initialData: snapshot.providers,
  });
  const scan = useQuery({
    queryKey: ["scan"],
    queryFn: () => command<ScanSnapshot>("scan_clis"),
    initialData: snapshot.current ?? undefined,
    enabled: Boolean(snapshot.current),
  });
  useEffect(() => {
    if (active !== "current" && !configurations.data.some((item) => item.id === active))
      setActive("current");
  }, [active, configurations.data, setActive]);
  const create = useMutation({
    mutationFn: () =>
      command<SavedConfiguration>("create_configuration", {
        request: { name: name.trim(), targets: [] },
      }),
    onSuccess: (value) => {
      queryClient.setQueryData<SavedConfiguration[]>(["configurations"], (items) => [
        ...(items ?? []),
        value,
      ]);
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
      setAddOpen(false);
      setName("");
      setActive(value.id);
      setDirty(false);
    },
    onError: (error) => onError(errorMessage(error)),
  });
  const nameIssue = validateEntityName(name, configurations.data);
  const selected = configurations.data.find((item) => item.id === active);
  return (
    <div className="page">
      <header className="page-header">
        <h1>{t("config.title")}</h1>
      </header>
      <ConfigurationTabs
        configurations={configurations.data}
        active={active}
        dirty={dirty}
        onSelect={(id) =>
          guarded(() => {
            setDirty(false);
            setActive(id);
          })
        }
        onAdd={() => guarded(() => setAddOpen(true))}
      />
      {active === "current" ? (
        <CurrentConfigurationTab
          scan={scan.data}
          configurations={configurations.data}
          providers={providers.data}
          catalog={snapshot.catalog}
          onError={onError}
        />
      ) : null}
      {selected ? (
        <SavedConfigurationTab
          key={`${selected.id}:${selected.revision}`}
          configuration={selected}
          matchStatus={snapshot.configurationStatuses[selected.id]}
          latestApply={
            snapshot.latestApply?.configurationId === selected.id ? snapshot.latestApply : undefined
          }
          providers={providers.data}
          catalog={snapshot.catalog}
          configurations={configurations.data}
          scan={scan.data}
          onDeleted={() => setActive("current")}
          onError={onError}
        />
      ) : null}
      <Modal
        open={addOpen}
        title={t("config.add")}
        onClose={() => setAddOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAddOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              disabled={Boolean(nameIssue) || create.isPending}
              onClick={() => create.mutate()}
            >
              {t("common.create")}
            </Button>
          </>
        }
      >
        <Field
          label={t("providers.name")}
          hint={
            nameIssue
              ? t(`validation.name${nameIssue === "length" ? "Length" : "Duplicate"}`)
              : undefined
          }
        >
          <Input autoFocus value={name} onChange={(event) => setName(event.target.value)} />
        </Field>
      </Modal>
    </div>
  );
}
