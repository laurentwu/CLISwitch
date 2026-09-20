import type { PropsWithChildren } from "react";
import { createPortal } from "react-dom";
import { usePageToolbarHost } from "./PageHeader";

export function PageToolbarPortal({ children }: PropsWithChildren) {
  const host = usePageToolbarHost();
  return host ? createPortal(children, host) : host === undefined ? children : null;
}
