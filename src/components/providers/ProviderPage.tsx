import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, LogIn, Plus, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, errorMessage } from "../../shared/ipc";
import type { AppSnapshot, OAuthKind, ProviderDetail, PublicProvider } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Badge, Button, Card, EmptyState, Modal, Spinner } from "../ui";
import { ApiProviderEditor } from "./ApiProviderEditor";
import { OAuthFlowDialog } from "./OAuthFlowDialog";
import { OAuthProviderEditor } from "./OAuthProviderEditor";

type Flow = {
  kind: OAuthKind;
  mode: "login" | "import";
  replaceProviderId?: string;
  name?: string;
};

export function ProviderPage({
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
  const setDirty = useUiStore((state) => state.setDirty);
  const [selectedId, setSelectedId] = useState<string>();
  const [createApi, setCreateApi] = useState(false);
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
    setCreateApi(false);
    setSelectedId(id);
  };
  const select = (id?: string) => guarded(() => selectImmediately(id));
  const remove = useMutation({
    mutationFn: (provider: PublicProvider) =>
      command("delete_provider", { providerId: provider.id, expectedRevision: provider.revision }),
    onSuccess: async () => {
      setDirty(false);
      setDeleteProvider(undefined);
      selectImmediately(undefined);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error) => onError(errorMessage(error)),
  });

  return (
    <div className="page">
      <header className="page-header">
        <div>
          <h1>{t("providers.title")}</h1>
          <p>{t("settings.riskText")}</p>
        </div>
        <div className="section-actions">
          <Button
            variant="secondary"
            onClick={() =>
              guarded(() => {
                selectImmediately(undefined);
                setCreateApi(true);
              })
            }
          >
            <Plus size={16} /> {t("providers.addApi")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => guarded(() => setFlow({ kind: "anthropic", mode: "login" }))}
          >
            <LogIn size={16} /> {t("providers.addClaudeOauth")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => guarded(() => setFlow({ kind: "codex", mode: "login" }))}
          >
            <LogIn size={16} /> {t("providers.addCodexOauth")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => guarded(() => setFlow({ kind: "anthropic", mode: "import" }))}
          >
            <Upload size={16} /> {t("providers.importClaudeOauth")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => guarded(() => setFlow({ kind: "codex", mode: "import" }))}
          >
            <Upload size={16} /> {t("providers.importCodexOauth")}
          </Button>
        </div>
      </header>
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
                  {provider.kind === "api" ? t("providers.typeApi") : t("providers.typeOauth")}
                </small>
              </span>
              {provider.codingPlan ? <Badge>Coding Plan</Badge> : null}
              <Badge tone={provider.referencedBy.length ? "neutral" : "good"}>
                {provider.referencedBy.length}
              </Badge>
            </button>
          ))}
          {!providers.data.length ? <EmptyState>{t("common.none")}</EmptyState> : null}
        </aside>
        <section className="detail-pane">
          {createApi ? (
            <ApiProviderEditor
              providers={providers.data}
              onClose={() => select(undefined)}
              onError={onError}
            />
          ) : null}
          {!createApi && selectedId && secret.isPending ? <Spinner /> : null}
          {!createApi && selectedId && secret.isError ? (
            <Card>
              <div className="global-error" role="alert">
                {errorMessage(secret.error)}
              </div>
              <Button variant="secondary" onClick={() => secret.refetch()}>
                {t("common.retry")}
              </Button>
            </Card>
          ) : null}
          {!createApi && selected && secret.data?.profileType === "api" ? (
            <ApiProviderEditor
              key={`${secret.data.id}:${secret.data.revision}`}
              detail={secret.data}
              providers={providers.data}
              onClose={() => select(undefined)}
              onError={onError}
            />
          ) : null}
          {!createApi && selected && secret.data?.profileType === "oauth" ? (
            <OAuthProviderEditor
              key={`${secret.data.id}:${secret.data.revision}`}
              detail={secret.data}
              publicProvider={selected}
              providers={providers.data}
              onClose={() => select(undefined)}
              onError={onError}
              onStartFlow={(mode) =>
                guarded(() =>
                  setFlow({
                    kind: selected.oauthKind!,
                    mode,
                    replaceProviderId: selected.id,
                    name: selected.name,
                  }),
                )
              }
            />
          ) : null}
          {!createApi && selected && secret.data?.profileType === "api" ? (
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
          {!createApi && !selectedId ? <EmptyState>{t("providers.addApi")}</EmptyState> : null}
          {!createApi &&
          selected &&
          secret.data?.profileType === "api" &&
          selected.referencedBy.length === 0 ? (
            <Button
              className="floating-delete"
              variant="danger"
              onClick={() => setDeleteProvider(selected)}
            >
              <Trash2 size={15} /> {t("common.delete")}
            </Button>
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
            if (id) select(id);
          }}
          onError={onError}
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
