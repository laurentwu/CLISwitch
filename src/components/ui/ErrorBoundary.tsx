import { Component, type ErrorInfo, type PropsWithChildren, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ErrorAlert } from "./Alert";

interface BoundaryState {
  error?: Error;
}

class ErrorBoundaryCore extends Component<
  PropsWithChildren<{ fallback: (error: Error, reset: () => void) => ReactNode }>,
  BoundaryState
> {
  state: BoundaryState = {};

  static getDerivedStateFromError(error: Error): BoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Do not mirror render failures to the WebView console: diagnostic text may contain secrets.
    void error;
    void info;
  }

  reset = () => this.setState({ error: undefined });

  render() {
    if (this.state.error) return this.props.fallback(this.state.error, this.reset);
    return this.props.children;
  }
}

export function AppErrorBoundary({ children }: PropsWithChildren) {
  const { t } = useTranslation();
  return (
    <ErrorBoundaryCore
      fallback={(error, reset) => (
        <div className="fatal-panel">
          <h1>{t("errors.renderFailure")}</h1>
          <ErrorAlert
            error={{ code: "frontend-render", message: error.message }}
            title={t("errors.operations.load")}
            onRetry={reset}
            detailsOpen
            tone="error"
          />
        </div>
      )}
    >
      {children}
    </ErrorBoundaryCore>
  );
}
