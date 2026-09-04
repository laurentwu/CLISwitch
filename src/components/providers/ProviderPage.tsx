import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, LogIn, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import { catalogProviderInfo } from "../../shared/catalog";
import { uniqueCopyName } from "../../shared/names";
import type {
  ApiProviderDraft,
  AppSnapshot,
  OAuthKind,
  ProviderDetail,
  PublicProvider,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  ErrorAlert,
  Modal,
  Spinner,
  type ErrorReporter,
} from "../ui";
import { ApiProviderEditor } from "./ApiProviderEditor";
import { OAuthFlowDialog } from "./OAuthFlowDialog";
import { OAuthProviderEditor, type OAuthProviderDraft } from "./OAuthProviderEditor";

type Flow = {
  kind: OAuthKind;
  mode: "login" | "import";
  replaceProviderId?: string;
  name?: string;
};

type ProviderCreation =
  | {
      key: number;
      mode: "api";
      initialTemplateId?: string;
      initialDraft?: ApiProviderDraft;
      initialName?: string;
      requireTemplateSelection: boolean;
    }
  | {
      key: number;
      mode: "oauth";
      templateId: string;
      initialName: string;
      initialRaw: string;
    };

type WithoutKey<Creation> = Creation extends unknown ? Omit<Creation, "key"> : never;
type ProviderCreationInput = WithoutKey<ProviderCreation>;

export function ProviderPage({
  snapshot,
  guarded,
  onError,
}: {
  snapshot: AppSnapshot;
  guarded: (action: () => void) => void;
  onError: ErrorReporter;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const creationSequence = useRef(0);
  const [selectedId, setSelectedId] = useState<string>();
  const [creation, setCreation] = useState<ProviderCreation>();
  const [flow, setFlow] = useState<Flow>();
  const [deleteProvider, setDeleteProvider] = useState<PublicProvider>();
  const providers = useQuery({
    queryKey: ["providers"],
    queryFn: () => command<PublicProvider[]>("list_providers"),
    initialData: snapshot.providers,
  });
  const selected = providers.data.find((provider) => provider.id === selectedId);
  const secret = useQuery({
    queryKey: ["provider-secret", selectedId],
    queryFn: () =>
      command<ProviderDetail>("get_provider_secret_detail", { providerId: selectedId }),
    enabled: Boolean(selectedId),
    gcTime: 0,
  });
  useEffect(
    () => () => {
      if (selectedId) queryClient.removeQueries({ queryKey: ["provider-secret", selectedId] });
    },
    [selectedId, queryClient],
  );

  const selectImmediately = (id?: string) => {
    if (selectedId) queryClient.removeQueries({ queryKey: ["provider-secret", selectedId] });
    setCreation(undefined);
    setSelectedId(id);
  };
  const select = (id?: string) => guarded(() => selectImmediately(id));
  const nextCreationKey = () => {
    creationSequence.current += 1;
    return creationSequence.current;
  };
  const startCreation = (next: ProviderCreationInput) => {
    if (selectedId) queryClient.removeQueries({ queryKey: ["provider-secret", selectedId] });
    setSelectedId(undefined);
    setCreation({ ...next, key: nextCreationKey() } as ProviderCreation);
  };
  const startAdd = () =>
    guarded(() =>
      startCreation({
        mode: "api",
        initialTemplateId: "",
        requireTemplateSelection: true,
      }),
    );
  const duplicateApi = (draft: ApiProviderDraft) => {
    const connections = draft.connections.map((connection) => {
      const clone = { ...connection };
      delete clone.id;
      return clone;
    });
    guarded(() => {
      setDirty(false);
      startCreation({
        mode: "api",
        initialDraft: {
          ...draft,
          name: uniqueCopyName(draft.name, t("common.duplicate"), providers.data),
          connections,
        },
        requireTemplateSelection: true,
      });
    });
  };
  const duplicateOAuth = (draft: OAuthProviderDraft) => {
    guarded(() => {
      setDirty(false);
      startCreation({
        mode: "oauth",
        templateId: draft.templateId,
        initialName: uniqueCopyName(draft.name, t("common.duplicate"), providers.data),
        initialRaw: draft.rawContent,
      });
    });
  };
  const remove = useMutation({
    mutationFn: (provider: PublicProvider) =>
      command("delete_provider", { providerId: provider.id, expectedRevision: provider.revision }),
    onSuccess: async () => {
      setDirty(false);
      setDeleteProvider(undefined);
      selectImmediately(undefined);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error) => onError(error, "delete"),
  });

  return (
    <div className="page">
      <header className="page-header">
        <div>
          <h1>{t("providers.title")}</h1>
          <p>{t("settings.riskText")}</p>
        </div>
        <div className="section-actions">
          <Button variant="secondary" onClick={startAdd}>
            <Plus size={16} /> {t("providers.add")}
          </Button>
        </div>
      </header>
      {providers.isError ? (
        <ErrorAlert
          error={providers.error}
          title={t("errors.query.providers")}
          onRetry={() => void providers.refetch()}
          tone="warning"
        />
      ) : null}
      <div className="split-view">
        <aside className="provider-list" aria-label={t("providers.title")}>
          {providers.data.map((provider) => (
            <button
              key={provider.id}
              className={
                selectedId === provider.id ? "provider-item provider-item-active" : "provider-item"
              }
              onClick={() => select(provider.id)}
            >
              <span className="provider-icon">
                {provider.kind === "api" ? <KeyRound size={17} /> : <LogIn size={17} />}
              </span>
              <span className="provider-summary">
                <strong>{provider.name}</strong>
                <small>
                  {provider.templateId && catalogProviderInfo(snapshot.catalog, provider.templateId)
                    ? `${catalogProviderInfo(snapshot.catalog, provider.templateId)?.name} (${provider.templateId})`
                    : provider.templateName || t("providers.customTemplate")}
                </small>
              </span>
              <Badge tone={provider.referencedBy.length ? "neutral" : "good"}>
                {provider.referencedBy.length}
              </Badge>
            </button>
          ))}
          {!providers.data.length ? <EmptyState>{t("common.none")}</EmptyState> : null}
        </aside>
        <section className="detail-pane">
          {creation?.mode === "api" ? (
            <ApiProviderEditor
              key={`create-api:${creation.key}`}
              initialTemplateId={creation.initialTemplateId}
              initialDraft={creation.initialDraft}
              initialName={creation.initialName}
              requireTemplateSelection={creation.requireTemplateSelection}
              providers={providers.data}
              catalog={snapshot.catalog}
              onClose={() => select(undefined)}
              onError={onError}
              onChooseOAuthTemplate={(template, currentName) => {
                guarded(() => {
                  setDirty(false);
                  startCreation({
                    mode: "oauth",
                    templateId: template.id,
                    initialName: currentName.trim() || template.name,
                    initialRaw: "",
                  });
                });
              }}
            />
          ) : null}
          {creation?.mode === "oauth" ? (
            <OAuthProviderEditor
              key={`create-oauth:${creation.key}`}
              catalog={snapshot.catalog}
              initialTemplateId={creation.templateId}
              initialName={creation.initialName}
              initialRaw={creation.initialRaw}
              providers={providers.data}
              onError={onError}
              onStartFlow={(kind, mode, name) => setFlow({ kind, mode, name })}
              onChooseApiTemplate={(templateId, currentName) => {
                const template = snapshot.catalog.providerTemplates.find(
                  (candidate) => candidate.id === templateId && candidate.mode === "api",
                );
                guarded(() => {
                  setDirty(false);
                  startCreation({
                    mode: "api",
                    initialTemplateId: templateId,
                    initialName: currentName.trim() || template?.name || "",
                    requireTemplateSelection: true,
                  });
                });
              }}
              onCreated={(id) => selectImmediately(id)}
            />
          ) : null}
          {!creation && selectedId && secret.isPending ? <Spinner /> : null}
          {!creation && selectedId && secret.isError ? (
            <Card>
              <ErrorAlert
                error={secret.error}
                title={t("errors.query.providerDetail")}
                onRetry={() => void secret.refetch()}
                tone={secret.data ? "warning" : undefined}
              />
            </Card>
          ) : null}
          {!creation && selected && secret.data?.profileType === "api" ? (
            <ApiProviderEditor
              key={`${secret.data.id}:${secret.data.revision}`}
              detail={secret.data}
              providers={providers.data}
              catalog={snapshot.catalog}
              onClose={() => select(undefined)}
              onError={onError}
              onDuplicate={duplicateApi}
              onDelete={() => setDeleteProvider(selected)}
              deleteDisabled={selected.referencedBy.length > 0}
            />
          ) : null}
          {!creation && selected && secret.data?.profileType === "oauth" ? (
            <OAuthProviderEditor
              key={`${secret.data.id}:${secret.data.revision}`}
              detail={secret.data}
              publicProvider={selected}
              catalog={snapshot.catalog}
              providers={providers.data}
              onError={onError}
              onStartFlow={(kind, mode, name, replaceProviderId) =>
                guarded(() => setFlow({ kind, mode, replaceProviderId, name }))
              }
              onDuplicate={duplicateOAuth}
              onDelete={() => setDeleteProvider(selected)}
              deleteDisabled={selected.referencedBy.length > 0}
            />
          ) : null}
          {!creation && selected && secret.data?.profileType === "api" ? (
            <Card>
              <h3>{t("providers.references")}</h3>
              {selected.referencedBy.length ? (
                <ul>
                  {selected.referencedBy.map((reference) => (
                    <li key={reference}>{reference}</li>
                  ))}
                </ul>
              ) : (
                <p>{t("common.none")}</p>
              )}
            </Card>
          ) : null}
          {!creation && !selectedId ? (
            <EmptyState>{t("providers.emptySelection")}</EmptyState>
          ) : null}
        </section>
      </div>
      {flow ? (
        <OAuthFlowDialog
          open
          kind={flow.kind}
          mode={flow.mode}
          replaceProviderId={flow.replaceProviderId}
          defaultName={flow.name}
          providers={providers.data}
          onClose={() => setFlow(undefined)}
          onCompleted={(id) => {
            setFlow(undefined);
            setDirty(false);
            if (id) selectImmediately(id);
          }}
        />
      ) : null}
      <Modal
        open={Boolean(deleteProvider)}
        title={t("common.confirmDelete")}
        onClose={() => setDeleteProvider(undefined)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteProvider(undefined)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              disabled={remove.isPending}
              onClick={() => deleteProvider && remove.mutate(deleteProvider)}
            >
              {t("common.delete")}
            </Button>
          </>
        }
      >
        <p>
          {deleteProvider?.name}: {t("providers.deleteWarning")}
        </p>
      </Modal>
    </div>
  );
}
