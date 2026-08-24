import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Circle, LoaderCircle, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, errorMessage, onEvent } from "../../shared/ipc";
import type { ApplyPreview, ApplyRunSnapshot, SavedConfiguration } from "../../shared/types";
import { Badge, Button, Modal, Spinner } from "../ui";

function StateIcon({ state }: { state: string }) {
  if (["success", "success-unverified", "unchanged"].includes(state))
    return <CheckCircle2 className="state-good" size={17} />;
  if (["failed", "conflict", "running-blocked"].includes(state))
    return <XCircle className="state-bad" size={17} />;
  if (state === "writing") return <LoaderCircle className="spin" size={17} />;
  if (["not-installed", "incompatible", "cancelled"].includes(state))
    return <AlertTriangle className="state-warn" size={17} />;
  return <Circle size={17} />;
}

export function ApplyPreviewDialog({
  configuration,
  initialRun,
  open,
  onClose,
  onError,
}: {
  configuration: SavedConfiguration;
  initialRun?: ApplyRunSnapshot;
  open: boolean;
  onClose: () => void;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [preview, setPreview] = useState<ApplyPreview>();
  const [runId, setRunId] = useState<string | undefined>(initialRun?.id);
  const previewMutation = useMutation({
    mutationFn: () =>
      command<ApplyPreview>("preview_apply", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
      }),
    onSuccess: setPreview,
    onError: (error) => onError(errorMessage(error)),
  });
  useEffect(() => {
    if (open && !preview && !runId && !previewMutation.isPending) previewMutation.mutate();
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  const close = () => {
    if (runId && !run.data?.finishedAt) {
      onClose();
      return;
    }
    setPreview(undefined);
    setRunId(undefined);
    onClose();
  };
  const start = useMutation({
    mutationFn: () => command<ApplyRunSnapshot>("start_apply", { previewId: preview?.id }),
    onSuccess: (run) => {
      setRunId(run.id);
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(errorMessage(error)),
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
    let cleanup: (() => void) | undefined;
    void onEvent<{ runId: string }>("cliswitch://apply-progress", (event) => {
      if (event.runId === runId) void run.refetch();
    }).then((unlisten) => (cleanup = unlisten));
    return () => cleanup?.();
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
    onError: (error) => onError(errorMessage(error)),
  });
  const retry = useMutation({
    mutationFn: () => command<ApplyPreview>("retry_apply_items", { runId }),
    onSuccess: (value) => {
      setPreview(value);
      setRunId(undefined);
    },
    onError: (error) => onError(errorMessage(error)),
  });
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
      <div className="apply-list">
        {items?.map((item) => (
          <article className="apply-row" key={item.cliId}>
            <StateIcon state={item.state} />
            <div className="apply-content">
              <div className="card-title-row">
                <strong>{item.cliId}</strong>
                <Badge>{t(`status.${item.state}`)}</Badge>
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
                <div className="diagnostic">{item.message}</div>
              ) : null}
              {"warning" in item && item.warning ? (
                <div className="diagnostic">{item.warning}</div>
              ) : null}
            </div>
          </article>
        ))}
      </div>
    </Modal>
  );
}
