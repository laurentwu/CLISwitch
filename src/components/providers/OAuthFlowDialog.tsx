import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { command, onEvent } from "../../shared/ipc";
import { validateEntityName } from "../../shared/names";
import type { OAuthKind, OAuthSessionSnapshot, PublicProvider } from "../../shared/types";
import { Badge, Button, ErrorAlert, Field, Input, Modal, Spinner } from "../ui";

export function OAuthFlowDialog({
  open,
  kind,
  mode,
  replaceProviderId,
  defaultName,
  providers,
  onClose,
  onCompleted,
}: {
  open: boolean;
  kind: OAuthKind;
  mode: "login" | "import";
  replaceProviderId?: string;
  defaultName?: string;
  providers: PublicProvider[];
  onClose: () => void;
  onCompleted: (providerId?: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [name, setName] = useState(defaultName ?? "");
  const [deviceAuth, setDeviceAuth] = useState(false);
  const [sessionId, setSessionId] = useState<string>();
  const [input, setInput] = useState("");
  const [subscriptionError, setSubscriptionError] = useState<unknown>();
  const [browserError, setBrowserError] = useState<unknown>();
  const nameIssue = validateEntityName(name, providers, replaceProviderId);

  const session = useQuery({
    queryKey: ["oauth-session", sessionId],
    queryFn: () => command<OAuthSessionSnapshot>("get_oauth_snapshot", { sessionId }),
    enabled: Boolean(sessionId),
    refetchInterval: (query) => (query.state.data?.finishedAt ? false : 500),
  });
  useEffect(() => {
    if (!sessionId) return;
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onEvent<{ sessionId: string }>("cliswitch://oauth-progress", (event) => {
      if (event.sessionId === sessionId) void session.refetch();
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
  }, [sessionId]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (session.data?.stage === "success") {
      void queryClient.invalidateQueries({ queryKey: ["providers"] });
      onCompleted(session.data.providerId ?? undefined);
    }
  }, [session.data?.stage]); // eslint-disable-line react-hooks/exhaustive-deps

  const begin = useMutation({
    mutationFn: async () => {
      if (mode === "import") {
        return command<PublicProvider | null>("import_oauth", {
          kind,
          name: name.trim(),
          replaceProviderId: replaceProviderId ?? null,
        });
      }
      return command<OAuthSessionSnapshot>("start_oauth_login", {
        kind,
        name: name.trim(),
        replaceProviderId: replaceProviderId ?? null,
        deviceAuth,
      });
    },
    onSuccess: (value) => {
      if (!value) return;
      if ("stage" in value) setSessionId(value.id);
      else {
        void queryClient.invalidateQueries({ queryKey: ["providers"] });
        onCompleted(value.id);
      }
    },
  });
  const cancel = useMutation({
    mutationFn: () => command("cancel_oauth_login", { sessionId }),
  });
  const send = useMutation({
    mutationFn: () => command("send_oauth_input", { sessionId, input }),
    onSuccess: () => setInput(""),
  });
  const browserUrl = session.data?.message
    .match(/https:\/\/[^\s<>"']+/)?.[0]
    ?.replace(/[),.;]+$/, "");

  return (
    <Modal
      open={open}
      title={mode === "login" ? t("providers.login") : t("providers.import")}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.close")}
          </Button>
          {sessionId && !session.data?.finishedAt ? (
            <Button variant="danger" onClick={() => cancel.mutate()}>
              {t("common.cancel")}
            </Button>
          ) : null}
          {!sessionId ? (
            <Button disabled={Boolean(nameIssue) || begin.isPending} onClick={() => begin.mutate()}>
              {begin.isPending ? <Spinner /> : null}
              {mode === "login" ? t("providers.login") : t("providers.import")}
            </Button>
          ) : null}
        </>
      }
    >
      {begin.isError ? (
        <ErrorAlert error={begin.error} title={t("errors.operations.oauth")} />
      ) : null}
      {cancel.isError ? (
        <ErrorAlert error={cancel.error} title={t("errors.operations.oauth")} />
      ) : null}
      {send.isError ? <ErrorAlert error={send.error} title={t("errors.operations.oauth")} /> : null}
      {session.isError ? (
        <ErrorAlert
          error={session.error}
          title={t("errors.query.oauthProgress")}
          onRetry={() => void session.refetch()}
          tone={session.data ? "warning" : undefined}
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
      {browserError ? (
        <ErrorAlert error={browserError} title={t("errors.operations.open")} compact />
      ) : null}
      {!sessionId ? (
        <>
          <Field
            label={t("providers.name")}
            hint={
              nameIssue
                ? t(nameIssue === "length" ? "validation.nameLength" : "validation.nameDuplicate")
                : undefined
            }
          >
            <Input autoFocus value={name} onChange={(event) => setName(event.target.value)} />
          </Field>
          {mode === "login" && kind === "codex" ? (
            <label className="switch-row">
              <input
                type="checkbox"
                checked={deviceAuth}
                onChange={(event) => setDeviceAuth(event.target.checked)}
              />
              {t("providers.deviceAuth")}
            </label>
          ) : null}
          <p className="muted">
            {mode === "login" ? t("providers.loginIsolation") : t("providers.importOffline")}
          </p>
        </>
      ) : (
        <div className="oauth-session">
          {session.isPending ? <Spinner /> : null}
          {session.data ? (
            <>
              <Badge
                tone={
                  session.data.stage === "success"
                    ? "good"
                    : session.data.stage === "failed"
                      ? "bad"
                      : "neutral"
                }
              >
                {t(`status.${session.data.stage}`)}
              </Badge>
              <pre className="oauth-output">{session.data.message}</pre>
              {browserUrl ? (
                <Button
                  variant="secondary"
                  onClick={() =>
                    command("open_oauth_browser_url", { kind, url: browserUrl })
                      .then(() => setBrowserError(undefined))
                      .catch((error) => setBrowserError(error))
                  }
                >
                  {t("providers.openBrowser")}
                </Button>
              ) : null}
            </>
          ) : null}
          {!session.data?.finishedAt ? (
            <div className="input-action">
              <Input
                value={input}
                placeholder={t("providers.confirmationCode")}
                onChange={(event) => setInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && input) send.mutate();
                }}
              />
              <Button disabled={!input || send.isPending} onClick={() => send.mutate()}>
                {t("common.confirm")}
              </Button>
            </div>
          ) : null}
        </div>
      )}
    </Modal>
  );
}
