import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
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
  useEffect(() => {
    const timer = window.setTimeout(() => dismiss(notification.id), timeoutFor(notification.tone));
    return () => window.clearTimeout(timer);
  }, [dismiss, notification.createdAt, notification.id, notification.tone]);

  const action = notification.action ? (
    <button
      type="button"
      className="button button-secondary"
      onClick={() => {
        dismiss(notification.id);
        notification.action?.run();
      }}
    >
      {notification.action.label}
    </button>
  ) : undefined;
  const error =
    notification.detail && notification.code
      ? { code: notification.code, message: notification.detail }
      : undefined;
  return (
    <Alert
      tone={notification.tone}
      announce
      title={
        <>
          {notification.title}
          {notification.occurrences > 1 ? (
            <span className="notification-count">×{notification.occurrences}</span>
          ) : null}
        </>
      }
      action={action}
      onDismiss={() => dismiss(notification.id)}
    >
      {notification.description ? <p>{notification.description}</p> : null}
      {error ? <ErrorDetails error={error} /> : null}
    </Alert>
  );
}

export function NotificationViewport() {
  const { t } = useTranslation();
  const notifications = useNotificationStore((state) => state.notifications);
  if (!notifications.length) return null;
  return (
    <section className="notification-viewport" aria-label={t("errors.notifications")}>
      {notifications.map((notification) => (
        <NotificationToast key={notification.id} notification={notification} />
      ))}
    </section>
  );
}
