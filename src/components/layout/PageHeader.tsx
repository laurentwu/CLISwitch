import {
  createContext,
  useCallback,
  useContext,
  useState,
  type PropsWithChildren,
  type ReactNode,
} from "react";

const ToolbarHostContext = createContext<HTMLElement | null | undefined>(undefined);
const PageHeaderHostSetterContext = createContext<(host: HTMLElement | null) => void>(
  () => undefined,
);

export function PageToolbarProvider({ children }: PropsWithChildren) {
  const [host, setHost] = useState<HTMLElement | null>(null);
  return (
    <ToolbarHostContext.Provider value={host}>
      <PageHeaderHostSetterContext.Provider value={setHost}>
        {children}
      </PageHeaderHostSetterContext.Provider>
    </ToolbarHostContext.Provider>
  );
}

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
}) {
  const setHost = useContext(PageHeaderHostSetterContext);
  const hostRef = useCallback((node: HTMLDivElement | null) => setHost(node), [setHost]);
  return (
    <header className="page-header">
      <div>
        <h1 tabIndex={-1}>{title}</h1>
        {description ? <p>{description}</p> : null}
      </div>
      <div className="page-toolbar" ref={hostRef}>
        {actions}
      </div>
    </header>
  );
}

export function usePageToolbarHost() {
  return useContext(ToolbarHostContext);
}
