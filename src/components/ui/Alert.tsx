import { useState, type PropsWithChildren, type ReactNode } from "react";
import { AlertCircle, AlertTriangle, CheckCircle2, Copy, Info, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { clsx } from "clsx";
import { errorGuidance, errorLevel, normalizeError, type AppFailure } from "../../shared/errors";
import type { NotificationTone } from "../../stores/notifications";

function AlertIcon({ tone }: { tone: NotificationTone }) {
  if (tone === "success") return <CheckCircle2 size={18} />;
  if (tone === "warning") return <AlertTriangle size={18} />;
  if (tone === "error") return <AlertCircle size={18} />;
  return <Info size={18} />;
}

export function Alert({
  tone = "info",
  title,
  compact,
  announce,
  action,
  onDismiss,
  children,
}: PropsWithChildren<{
  tone?: NotificationTone;
  title: ReactNode;
  compact?: boolean;
  announce?: boolean;
  action?: ReactNode;
  onDismiss?: () => void;
}>) {
  const { t } = useTranslation();
  return (
    <div
      className={clsx("alert", `alert-${tone}`, compact && "alert-compact")}
      role={announce ? (tone === "error" || tone === "warning" ? "alert" : "status") : undefined}
    >
      <span className="alert-icon" aria-hidden="true">
        <AlertIcon tone={tone} />
      </span>
      <div className="alert-content">
        <strong className="alert-title">{title}</strong>
        {children}
      </div>
      {action ? <div className="alert-action">{action}</div> : null}
      {onDismiss ? (
        <button
          type="button"
          className="alert-dismiss"
          aria-label={t("errors.dismiss")}
          title={t("errors.dismiss")}
          onClick={onDismiss}
        >
          <X size={16} />
        </button>
      ) : null}
    </div>
  );
}

export function ErrorDetails({ error, open = false }: { error: AppFailure; open?: boolean }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(`[${error.code}] ${error.message}`);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <details className="error-details" open={open}>
      <summary>{t("errors.technicalDetails")}</summary>
      <div className="error-details-header">
        <code>{error.code}</code>
        <button type="button" className="error-copy" onClick={() => void copy()}>
          <Copy size={14} /> {copied ? t("common.copied") : t("errors.copyDetails")}
        </button>
      </div>
      <pre>{error.message}</pre>
    </details>
  );
}

export function ErrorAlert({
  error,
  title,
  onRetry,
  compact,
  detailsOpen,
  tone,
}: {
  error: unknown;
  title: ReactNode;
  onRetry?: () => void;
  compact?: boolean;
  detailsOpen?: boolean;
  tone?: NotificationTone;
}) {
  const { t } = useTranslation();
  const normalized = normalizeError(error);
  if (normalized.code === "cancelled") return null;
  const level = tone ?? errorLevel(normalized.code);
  return (
    <Alert
      tone={level}
      title={title}
      compact={compact}
      announce
      action={
        onRetry ? (
          <button type="button" className="button button-secondary" onClick={onRetry}>
            {t("common.retry")}
          </button>
        ) : undefined
      }
    >
      <p>{t(`errors.guidance.${errorGuidance(normalized.code)}`)}</p>
      <ErrorDetails error={normalized} open={detailsOpen} />
    </Alert>
  );
}
