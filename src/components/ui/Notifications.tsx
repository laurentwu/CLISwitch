import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useAppTheme } from "../../app/ThemeProvider";
import {
  errorGuidance,
  errorLevel,
  isCancellationError,
  normalizeError,
} from "../../shared/errors";
import {
  useNotificationStore,
  type NotificationTone,
  type UserNotification,
} from "../../stores/notifications";
import { Alert, ErrorDetails } from "./Alert";
import { Button } from "./primitives/button";
import { Toaster } from "./primitives/sonner";
import { attachNotificationHost } from "./notificationHost";

export type ErrorOperation =
  | "generic"
  | "load"
  | "refresh"
  | "save"
  | "create"
  | "duplicate"
  | "copy"
  | "delete"
  | "scan"
  | "selectPath"
  | "configure"
  | "apply"
  | "restore"
  | "connectionTest"
  | "fetchModels"
  | "oauth"
  | "open"
  | "updateCheck"
  | "catalogUpdate"
  | "zoom"
  | "close"
  | "background";

export type ErrorReporter = (error: unknown, operation?: ErrorOperation) => void;

export function useErrorNotifier(): ErrorReporter {
  const { t } = useTranslation();
  const push = useNotificationStore((state) => state.push);
  return useCallback(
    (error: unknown, operation: ErrorOperation = "generic") => {
      if (isCancellationError(error)) return;
      const normalized = normalizeError(error);
      push({
        tone: errorLevel(normalized.code),
        title: t(`errors.operations.${operation}`),
        description: t(`errors.guidance.${errorGuidance(normalized.code)}`),
        detail: normalized.message,
        code: normalized.code,
        dedupeKey: `${operation}\0${normalized.code}\0${normalized.message}`,
      });
    },
    [push, t],
  );
}

function timeoutFor(tone: NotificationTone): number {
  return tone === "success" || tone === "info" ? 3_000 : 8_000;
}

function NotificationToast({ notification }: { notification: UserNotification }) {
  const dismiss = useNotificationStore((state) => state.dismiss);
  const error =
    notification.detail && notification.code
      ? { code: notification.code, message: notification.detail }
      : undefined;
  return (
    <Alert
      tone={notification.tone}
      title={
        <>
          {notification.title}
          {notification.occurrences > 1 ? (
            <span className="notification-count">×{notification.occurrences}</span>
          ) : null}
        </>
      }
      action={
        notification.action ? (
          <Button
            variant="outline"
            onClick={() => {
              if (
                !useNotificationStore
                  .getState()
                  .notifications.some((item) => item.id === notification.id)
              )
                return;
              dismiss(notification.id);
              notification.action?.run();
            }}
          >
            {notification.action.label}
          </Button>
        ) : undefined
      }
      onDismiss={() => dismiss(notification.id)}
    >
      {notification.description ? <p>{notification.description}</p> : null}
      {error ? <ErrorDetails error={error} /> : null}
    </Alert>
  );
}

export function NotificationViewport() {
  const { t } = useTranslation();
  const { resolvedTheme } = useAppTheme();
  const notifications = useNotificationStore((state) => state.notifications);
  const renderedIds = useRef(new Set<number>());
  const [host] = useState(() => {
    const element = document.createElement("div");
    element.dataset.notificationHost = "";
    return element;
  });

  useLayoutEffect(() => attachNotificationHost(host), [host]);

  useEffect(() => {
    const timers = notifications.map((notification) =>
      window.setTimeout(
        () => useNotificationStore.getState().dismiss(notification.id),
        Math.max(0, notification.createdAt + timeoutFor(notification.tone) - Date.now()),
      ),
    );
    return () => timers.forEach(window.clearTimeout);
  }, [notifications]);

  useEffect(() => {
    const currentIds = new Set(notifications.map((notification) => notification.id));
    for (const id of renderedIds.current) {
      if (!currentIds.has(id)) toast.dismiss(String(id));
    }
    for (const notification of notifications) {
      toast.custom(() => <NotificationToast notification={notification} />, {
        id: String(notification.id),
        duration: Infinity,
        onDismiss: () => useNotificationStore.getState().dismiss(notification.id),
      });
    }
    renderedIds.current = currentIds;
  }, [notifications]);

  useEffect(
    () => () => {
      for (const id of renderedIds.current) toast.dismiss(String(id));
    },
    [],
  );

  return createPortal(
    <Toaster
      theme={resolvedTheme}
      duration={Infinity}
      visibleToasts={3}
      containerAriaLabel={t("errors.notifications")}
      closeButton={false}
    />,
    host,
  );
}
