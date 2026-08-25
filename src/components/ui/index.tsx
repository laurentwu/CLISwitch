import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  PropsWithChildren,
  ReactNode,
} from "react";
import { X } from "lucide-react";
import { clsx } from "clsx";
import { useTranslation } from "react-i18next";

export { Alert, ErrorAlert, ErrorDetails } from "./Alert";
export { AppErrorBoundary } from "./ErrorBoundary";
export {
  NotificationViewport,
  useErrorNotifier,
  type ErrorOperation,
  type ErrorReporter,
} from "./Notifications";

export function Button({
  variant = "primary",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger" | "ghost";
}) {
  return <button className={clsx("button", `button-${variant}`, className)} {...props} />;
}

export function IconButton({
  label,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return <button className="icon-button" aria-label={label} title={label} {...props} />;
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={clsx("input", props.className)} {...props} />;
}

export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={clsx("input", props.className)} {...props} />;
}

export function Textarea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={clsx("input", "textarea", props.className)} {...props} />;
}

export function Field({
  label,
  hint,
  children,
}: PropsWithChildren<{ label: ReactNode; hint?: ReactNode }>) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </label>
  );
}

export function Card({ className, children }: PropsWithChildren<{ className?: string }>) {
  return <section className={clsx("card", className)}>{children}</section>;
}

export function Badge({
  tone = "neutral",
  children,
}: PropsWithChildren<{ tone?: "neutral" | "good" | "warn" | "bad" }>) {
  return <span className={clsx("badge", `badge-${tone}`)}>{children}</span>;
}

export function Modal({
  title,
  open,
  onClose,
  children,
  footer,
  wide,
}: PropsWithChildren<{
  title: ReactNode;
  open: boolean;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
}>) {
  const { t } = useTranslation();
  if (!open) return null;
  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <section
        className={clsx("modal", wide && "modal-wide")}
        role="dialog"
        aria-modal="true"
        aria-label={typeof title === "string" ? title : undefined}
      >
        <header className="modal-header">
          <h2>{title}</h2>
          <IconButton label={t("common.closeLabel")} onClick={onClose}>
            <X size={18} />
          </IconButton>
        </header>
        <div className="modal-body">{children}</div>
        {footer ? <footer className="modal-footer">{footer}</footer> : null}
      </section>
    </div>
  );
}

export function EmptyState({ children }: PropsWithChildren) {
  return <div className="empty-state">{children}</div>;
}

export function Spinner() {
  const { t } = useTranslation();
  return <span className="spinner" aria-label={t("common.loadingLabel")} />;
}
