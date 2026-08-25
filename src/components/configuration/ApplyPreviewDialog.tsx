import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Circle, LoaderCircle, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, onEvent } from "../../shared/ipc";
import type { ApplyPreview, ApplyRunSnapshot, SavedConfiguration } from "../../shared/types";
import { Alert, Badge, Button, ErrorAlert, Modal, Spinner } from "../ui";

function StateIcon({ state }: { state: string }) {
  if (["success", "unchanged"].includes(state))
    return <CheckCircle2 className="state-good" size={17} />;
  if (state === "failed") return <XCircle className="state-bad" size={17} />;
  if (state === "writing") return <LoaderCircle className="spin" size={17} />;
  if (
    [
      "success-unverified",
      "conflict",
      "running-blocked",
      "not-installed",
      "incompatible",
      "cancelled",
    ].includes(state)
  )
    return <AlertTriangle className="state-warn" size={17} />;
  return <Circle size={17} />;
}

function stateTone(state: string): "neutral" | "good" | "warn" | "bad" {
  if (["success", "unchanged"].includes(state)) return "good";
  if (state === "failed") return "bad";
  if (
    [
      "success-unverified",
      "conflict",
      "running-blocked",
      "not-installed",
      "incompatible",
      "cancelled",
    ].includes(state)
  )
    return "warn";
  return "neutral";
}

export function ApplyPreviewDialog({
  configuration,
  initialRun,
  open,
  onClose,
}: {
  configuration: SavedConfiguration;
  initialRun?: ApplyRunSnapshot;
  open: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [preview, setPreview] = useState<ApplyPreview>();
  const [runId, setRunId] = useState<string | undefined>(initialRun?.id);
  const [subscriptionError, setSubscriptionError] = useState<unknown>();
  const previewMutation = useMutation({
    mutationFn: () =>
      command<ApplyPreview>("preview_apply", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
      }),
    onSuccess: setPreview,
  });
  useEffect(() => {
    if (open && !preview && !runId && !previewMutation.isPending) previewMutation.mutate();
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  const start = useMutation({
    mutationFn: () => command<ApplyRunSnapshot>("start_apply", { previewId: preview?.id }),
    onSuccess: (run) => {
      setRunId(run.id);
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
  });
  const run = useQuery({
    queryKey: ["apply-run", runId],
    queryFn: () => command<ApplyRunSnapshot>("get_apply_snapshot", { runId }),
    enabled: Boolean(runId),
    initialData: initialRun?.id === runId ? initialRun : undefined,
    refetchInterval: (query) => (query.state.data?.finishedAt ? false : 500),
  });
  useEffect(() => {
    if (!runId) return;
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onEvent<{ runId: string }>("cliswitch://apply-progress", (event) => {
      if (event.runId === runId) void run.refetch();
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        setSubscriptionError(undefined);
        cleanup = unlisten;
      })
      .catch((error) => {
        if (!disposed) setSubscriptionError(error);
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [runId]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (run.data?.finishedAt) {
      void queryClient.invalidateQueries({ queryKey: ["configurations"] });
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    }
  }, [run.data?.finishedAt, queryClient]);
  const cancel = useMutation({
    mutationFn: () => command("cancel_apply", { runId }),
  });
  const retry = useMutation({
    mutationFn: () => command<ApplyPreview>("retry_apply_items", { runId }),
    onSuccess: (value) => {
      setPreview(value);
      setRunId(undefined);
      setSubscriptionError(undefined);
    },
  });
  const close = () => {
    if (runId && !run.data?.finishedAt) {
      onClose();
      return;
    }
    setPreview(undefined);
    setRunId(undefined);
    setSubscriptionError(undefined);
    previewMutation.reset();
    start.reset();
    cancel.reset();
    retry.reset();
    onClose();
  };
  const items = runId ? run.data?.items : preview?.items;
  return (
    <Modal
      open={open}
      wide
      title={runId ? t("config.progress") : t("config.applyPreview")}
      onClose={close}
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            {t("common.close")}
          </Button>
          {runId && !run.data?.finishedAt ? (
            <Button variant="danger" disabled={cancel.isPending} onClick={() => cancel.mutate()}>
              {t("common.cancel")}
            </Button>
          ) : null}
          {runId &&
          run.data?.finishedAt &&
          run.data.items.some((item) =>
            ["failed", "conflict", "running-blocked"].includes(item.state),
          ) ? (
            <Button onClick={() => retry.mutate()}>{t("common.retryFailed")}</Button>
          ) : null}
          {!runId && preview ? (
            <Button
              disabled={start.isPending || !preview.items.some((item) => item.state === "waiting")}
              onClick={() => start.mutate()}
            >
              {start.isPending ? <Spinner /> : null}
              {t("common.confirm")}
            </Button>
          ) : null}
        </>
      }
    >
      {previewMutation.isPending || (runId && run.isPending) ? <Spinner /> : null}
      {previewMutation.isError ? (
        <ErrorAlert
          error={previewMutation.error}
          title={t("errors.query.applyPreview")}
          onRetry={() => previewMutation.mutate()}
        />
      ) : null}
      {start.isError ? (
        <ErrorAlert error={start.error} title={t("errors.operations.apply")} />
      ) : null}
      {cancel.isError ? (
        <ErrorAlert error={cancel.error} title={t("errors.operations.apply")} />
      ) : null}
      {retry.isError ? (
        <ErrorAlert error={retry.error} title={t("errors.operations.apply")} />
      ) : null}
      {run.isError ? (
        <ErrorAlert
          error={run.error}
          title={t("errors.query.applyProgress")}
          onRetry={() => void run.refetch()}
          tone={run.data ? "warning" : undefined}
        />
      ) : null}
      {subscriptionError ? (
        <ErrorAlert
          error={subscriptionError}
          title={t("errors.query.liveUpdates")}
          compact
          tone="warning"
        />
      ) : null}
      <div className="apply-list">
        {items?.map((item) => (
          <article className="apply-row" key={item.cliId}>
            <StateIcon state={item.state} />
            <div className="apply-content">
              <div className="card-title-row">
                <strong>{item.cliId}</strong>
                <Badge tone={stateTone(item.state)}>{t(`status.${item.state}`)}</Badge>
              </div>
              {"providerName" in item ? (
                <div>
                  {item.providerName} · {item.protocol ?? "OAuth"} · {item.model}
                </div>
              ) : null}
              {"path" in item && item.path ? <div className="path-text">{item.path}</div> : null}
              {"changes" in item
                ? item.changes.map((change) => (
                    <div className="diff-row" key={change.field}>
                      <span>{change.field}</span>
                      <del>{change.before ?? "—"}</del>
                      <span>→</span>
                      <ins>{change.after ?? "—"}</ins>
                    </div>
                  ))
                : null}
              {"message" in item && item.message ? (
                <Alert
                  compact
                  tone={item.state === "failed" ? "error" : "warning"}
                  title={t(`status.${item.state}`)}
                >
                  <p>{item.message}</p>
                </Alert>
              ) : null}
              {"warning" in item && item.warning ? (
                <Alert compact tone="warning" title={t(`status.${item.state}`)}>
                  <p>{item.warning}</p>
                </Alert>
              ) : null}
            </div>
          </article>
        ))}
      </div>
    </Modal>
  );
}
