import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Circle, LoaderCircle, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, onEvent } from "../../shared/ipc";
import type {
  ApplyPreview,
  ApplyPreviewFile,
  ApplyRunSnapshot,
  ConfigurationTarget,
  SavedConfiguration,
} from "../../shared/types";
import { Alert, Badge, Button, ErrorAlert, Modal, Spinner } from "../ui";

type DiffLine = { kind: "same" | "removed" | "added"; text: string };

function splitLines(value: string): string[] {
  return value.split("\n");
}

function diffLines(before: string, after: string): DiffLine[] {
  const left = splitLines(before);
  const right = splitLines(after);
  const cells = (left.length + 1) * (right.length + 1);
  if (cells > 250_000) {
    return [
      ...left.map((text) => ({ kind: "removed" as const, text })),
      ...right.map((text) => ({ kind: "added" as const, text })),
    ];
  }
  const table = Array.from({ length: left.length + 1 }, () =>
    new Array<number>(right.length + 1).fill(0),
  );
  for (let leftIndex = left.length - 1; leftIndex >= 0; leftIndex -= 1) {
    for (let rightIndex = right.length - 1; rightIndex >= 0; rightIndex -= 1) {
      table[leftIndex][rightIndex] =
        left[leftIndex] === right[rightIndex]
          ? table[leftIndex + 1][rightIndex + 1] + 1
          : Math.max(table[leftIndex + 1][rightIndex], table[leftIndex][rightIndex + 1]);
    }
  }
  const result: DiffLine[] = [];
  let leftIndex = 0;
  let rightIndex = 0;
  while (leftIndex < left.length || rightIndex < right.length) {
    if (
      leftIndex < left.length &&
      rightIndex < right.length &&
      left[leftIndex] === right[rightIndex]
    ) {
      result.push({ kind: "same", text: left[leftIndex] });
      leftIndex += 1;
      rightIndex += 1;
    } else if (
      rightIndex < right.length &&
      (leftIndex === left.length ||
        table[leftIndex][rightIndex + 1] >= table[leftIndex + 1][rightIndex])
    ) {
      result.push({ kind: "added", text: right[rightIndex] });
      rightIndex += 1;
    } else {
      result.push({ kind: "removed", text: left[leftIndex] });
      leftIndex += 1;
    }
  }
  return result;
}

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
  target,
  initialRun,
  open,
  onClose,
}: {
  configuration: SavedConfiguration;
  target?: ConfigurationTarget;
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
      command<ApplyPreview>(target ? "preview_cli_apply" : "preview_apply", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
        ...(target ? { target } : {}),
      }),
    onSuccess: setPreview,
  });
  useEffect(() => {
    if (open && !preview && !runId && !previewMutation.isPending) previewMutation.mutate();
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  const start = useMutation({
    mutationFn: (previewId: string) => command<ApplyRunSnapshot>("start_apply", { previewId }),
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
      start.mutate(value.id);
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
  const previewItem = preview?.items[0];
  const files = previewItem?.files ?? [];
  const [activeFilePath, setActiveFilePath] = useState<string>();
  const activeFile = files.find((file) => file.path === activeFilePath) ?? files[0];
  return (
    <Modal
      open={open}
      wide
      title={runId ? t("config.progress") : t("config.applyPreviewFor", { cli: target?.cliId })}
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
      {!runId && preview && previewItem ? (
        <div className="preview-files">
          <div className="preview-file-tabs" role="tablist" aria-label={t("config.previewFiles")}>
            {files.map((file) => (
              <button
                className={file.path === activeFile?.path ? "preview-file-tab-active" : ""}
                key={file.path}
                role="tab"
                aria-selected={file.path === activeFile?.path}
                onClick={() => setActiveFilePath(file.path)}
              >
                {file.path}
              </button>
            ))}
          </div>
          {activeFile ? <PreviewFileView file={activeFile} /> : null}
        </div>
      ) : null}
    </Modal>
  );
}

function PreviewFileView({ file }: { file: ApplyPreviewFile }) {
  const { t } = useTranslation();
  const source = file.sourceContent ?? "";
  const diff = file.existed
    ? diffLines(source, file.targetContent)
    : splitLines(file.targetContent).map((text) => ({ kind: "added" as const, text }));
  return (
    <section className="preview-file">
      <header className="preview-file-header">
        <strong>{file.path}</strong>
        {!file.existed ? <Badge tone="warn">{t("config.fileNew")}</Badge> : null}
      </header>
      <div className="preview-file-panes">
        <div>
          <h3>{t("config.originalFile")}</h3>
          <pre className="preview-code">{file.existed ? source : t("config.fileMissing")}</pre>
        </div>
        <div>
          <h3>{t("config.newFile")}</h3>
          <pre className="preview-code">{file.targetContent}</pre>
        </div>
      </div>
      <h3>{t("config.fileDiff")}</h3>
      <pre className="preview-diff">
        {diff.map((line, index) => (
          <span
            className={`preview-diff-line preview-diff-${line.kind}`}
            key={`${index}-${line.kind}`}
          >
            {line.kind === "removed" ? "− " : line.kind === "added" ? "+ " : "  "}
            {line.text}
            {"\n"}
          </span>
        ))}
      </pre>
    </section>
  );
}
