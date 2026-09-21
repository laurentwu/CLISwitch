import {
  Children,
  cloneElement,
  Fragment,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useRef,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type PropsWithChildren,
  type ReactElement,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { cn } from "../../lib/utils";
import { Button as ButtonPrimitive } from "./primitives/button";
import { Input as InputPrimitive } from "./primitives/input";
import { Textarea as TextareaPrimitive } from "./primitives/textarea";
import {
  Field as FieldPrimitive,
  FieldDescription,
  FieldError,
  FieldLabel,
} from "./primitives/field";
import { Badge as BadgePrimitive } from "./primitives/badge";
import { Card as CardPrimitive, CardContent } from "./primitives/card";
import { Spinner as SpinnerPrimitive } from "./primitives/spinner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./primitives/dialog";
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./primitives/alert-dialog";
import { Empty, EmptyDescription } from "./primitives/empty";
import { registerModalHost } from "./notificationHost";
import { modalTrigger, observeModalTriggers } from "./modalFocus";

export { Alert, ErrorAlert, ErrorDetails } from "./Alert";
export { AppErrorBoundary } from "./ErrorBoundary";
export {
  NotificationViewport,
  useErrorNotifier,
  type ErrorOperation,
  type ErrorReporter,
} from "./Notifications";
export {
  AppSelect,
  type AppSelectProps,
  type SelectGroupData,
  type SelectOption,
} from "./AppSelect";

export function Button({
  variant = "primary",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "danger" | "danger-outline" | "ghost";
}) {
  const mapped =
    variant === "primary"
      ? "default"
      : variant === "secondary"
        ? "outline"
        : variant === "danger"
          ? "destructive"
          : variant === "danger-outline"
            ? "destructive-outline"
            : "ghost";
  return <ButtonPrimitive variant={mapped} className={cn("button", className)} {...props} />;
}

export function IconButton({
  label,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <ButtonPrimitive
      variant="ghost"
      size="icon"
      className="icon-button"
      aria-label={label}
      title={label}
      {...props}
    />
  );
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <InputPrimitive {...props} className={cn("input", props.className)} />;
}

export function Textarea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <TextareaPrimitive {...props} className={cn("input", "textarea", props.className)} />;
}

export function Field({
  label,
  hint,
  children,
  controlId: requestedControlId,
}: PropsWithChildren<{ label: ReactNode; hint?: ReactNode; controlId?: string }>) {
  const generatedId = useId();
  const childNodes = Children.toArray(children);
  const controlIndex = childNodes.findIndex(isValidElement);
  const child = controlIndex >= 0 ? childNodes[controlIndex] : undefined;
  const childProps = isValidElement(child)
    ? (child.props as { id?: string; "aria-describedby"?: string; "aria-invalid"?: boolean })
    : undefined;
  const controlId = requestedControlId ?? childProps?.id ?? generatedId;
  const hintId = hint ? `${controlId}-description` : undefined;
  const controlNodes = childNodes.map((node, index) =>
    index === controlIndex && isValidElement(node) && node.type !== Fragment && !requestedControlId
      ? cloneElement(node as ReactElement<Record<string, unknown>>, {
          id: controlId,
          "aria-describedby":
            [childProps?.["aria-describedby"], hintId].filter(Boolean).join(" ") || undefined,
        })
      : node,
  );
  return (
    <FieldPrimitive className="field" data-invalid={Boolean(childProps?.["aria-invalid"])}>
      <FieldLabel className="field-label" htmlFor={controlId}>
        {label}
      </FieldLabel>
      {controlNodes}
      {hint ? (
        childProps?.["aria-invalid"] ? (
          <FieldError id={hintId}>{hint}</FieldError>
        ) : (
          <FieldDescription className="field-hint" id={hintId}>
            {hint}
          </FieldDescription>
        )
      ) : null}
    </FieldPrimitive>
  );
}

export function Card({ className, children }: PropsWithChildren<{ className?: string }>) {
  return (
    <CardPrimitive className={cn("card", className)}>
      <CardContent className="grid min-w-0 gap-4 p-0">{children}</CardContent>
    </CardPrimitive>
  );
}

export function Badge({
  tone = "neutral",
  children,
}: PropsWithChildren<{ tone?: "neutral" | "good" | "warn" | "bad" }>) {
  const variant =
    tone === "good"
      ? "success"
      : tone === "warn"
        ? "warning"
        : tone === "bad"
          ? "error"
          : "outline";
  return (
    <BadgePrimitive variant={variant} className="badge">
      {children}
    </BadgePrimitive>
  );
}

export function Modal({
  title,
  open,
  onClose,
  children,
  footer,
  wide,
  description,
}: PropsWithChildren<{
  title: ReactNode;
  open: boolean;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
  description?: ReactNode;
}>) {
  const { t } = useTranslation();
  const { hostRef, capture, restore } = useModalFocus();
  const descriptionId = useId();
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent
        closeLabel={t("common.closeLabel")}
        className={cn("modal", wide && "modal-wide")}
        ref={hostRef}
        onOpenAutoFocus={capture}
        onCloseAutoFocus={restore}
        aria-describedby={description ? descriptionId : undefined}
      >
        <DialogHeader className="modal-header">
          <DialogTitle>{title}</DialogTitle>
          {description ? (
            <DialogDescription id={descriptionId}>{description}</DialogDescription>
          ) : null}
        </DialogHeader>
        <div className="modal-body">{children}</div>
        {footer ? <DialogFooter className="modal-footer">{footer}</DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}

export function ConfirmModal({
  title,
  open,
  onClose,
  children,
  footer,
  description,
}: PropsWithChildren<{
  title: ReactNode;
  open: boolean;
  onClose: () => void;
  footer?: ReactNode;
  description?: ReactNode;
}>) {
  const { hostRef, focusCancel, restore } = useModalFocus();
  const descriptionId = useId();
  return (
    <AlertDialog open={open} onOpenChange={(next) => !next && onClose()}>
      <AlertDialogContent
        className="modal"
        ref={hostRef}
        onOpenAutoFocus={focusCancel}
        onCloseAutoFocus={restore}
        aria-describedby={description ? descriptionId : undefined}
      >
        <AlertDialogHeader className="modal-header">
          <AlertDialogTitle>{title}</AlertDialogTitle>
          {description ? (
            <AlertDialogDescription id={descriptionId}>{description}</AlertDialogDescription>
          ) : null}
        </AlertDialogHeader>
        <div className="modal-body">{children}</div>
        {footer ? <AlertDialogFooter className="modal-footer">{footer}</AlertDialogFooter> : null}
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function EmptyState({ children }: PropsWithChildren) {
  return (
    <Empty className="empty-state">
      <EmptyDescription>{children}</EmptyDescription>
    </Empty>
  );
}

function useModalFocus() {
  const returnFocus = useRef<HTMLElement | null>(null);
  const content = useRef<HTMLElement | null>(null);
  useEffect(observeModalTriggers, []);
  const hostRef = useCallback((node: HTMLDivElement | null) => {
    content.current = node;
    if (node) {
      const active = document.activeElement;
      // React autoFocus may run before Radix's mount autofocus event. Capture
      // the opener here as well, since Radix skips that event when focus is inside.
      returnFocus.current =
        modalTrigger() ?? (active instanceof HTMLElement && !node.contains(active) ? active : null);
      const unregister = registerModalHost(node);
      return () => {
        unregister();
        if (content.current === node) content.current = null;
      };
    }
  }, []);
  const capture = () => {
    const active = document.activeElement;
    returnFocus.current = modalTrigger() ?? (active instanceof HTMLElement ? active : null);
  };
  const restore = (event: Event) => {
    event.preventDefault();
    let target = returnFocus.current;
    // A guarded tab switch may have focused the attempted tab before the dialog opened.
    if (target?.getAttribute("role") === "tab") {
      target =
        target.closest('[role="tablist"]')?.querySelector('[aria-selected="true"]') ?? target;
    }
    if (!target?.isConnected || target.matches(":disabled")) {
      target = document.querySelector(".page-header h1, .main-content button:not(:disabled)");
    }
    target?.focus();
  };
  const focusCancel = (event: Event) => {
    capture();
    event.preventDefault();
    const cancel = content.current?.querySelector<HTMLButtonElement>(
      '[data-slot="alert-dialog-footer"] button:not(:disabled)',
    );
    (cancel ?? content.current)?.focus();
  };
  return { hostRef, capture, restore, focusCancel };
}

export function Spinner() {
  const { t } = useTranslation();
  return <SpinnerPrimitive className="spinner" aria-label={t("common.loadingLabel")} />;
}
