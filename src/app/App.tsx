import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AppLayout } from "../components/layout/AppLayout";
import { Modal, Button, ErrorAlert, Spinner, useErrorNotifier } from "../components/ui";
import { ConfigurationPage } from "../components/configuration/ConfigurationPage";
import { ProviderPage } from "../components/providers/ProviderPage";
import { SettingsPage } from "../components/settings/SettingsPage";
import { command, onEvent } from "../shared/ipc";
import type { AppSnapshot, CloseState, StartupStatus } from "../shared/types";
import { useUiStore, type Navigation } from "../stores/ui";

export function App() {
  const { t } = useTranslation();
  const reportError = useErrorNotifier();
  const startup = useQuery({
    queryKey: ["startup-status"],
    queryFn: () => command<StartupStatus>("get_startup_status"),
    retry: false,
  });
  if (startup.isPending) {
    return (
      <div className="splash">
        <Spinner /> {t("common.loading")}
      </div>
    );
  }
  if (startup.isError || !startup.data) {
    const error = startup.error ?? {
      code: "unknown",
      message: "Startup status did not contain data",
    };
    return (
      <div className="fatal-panel">
        <h1>{t("common.error")}</h1>
        <ErrorAlert
          error={error}
          title={t("errors.operations.load")}
          onRetry={() => void startup.refetch()}
          detailsOpen
          tone="error"
        />
      </div>
    );
  }
  if (!startup.data.ready) {
    return (
      <div className="fatal-panel">
        <h1>{t("startup.title")}</h1>
        <p>{t("startup.readOnly")}</p>
        <ErrorAlert
          error={{ code: startup.data.code ?? "startup", message: startup.data.message }}
          title={t("errors.operations.load")}
          detailsOpen
          tone="error"
        />
        <p className="path-text">{startup.data.appDataDirectory}</p>
        <Button
          onClick={() =>
            command("open_startup_data_directory").catch((error) => reportError(error, "open"))
          }
        >
          {t("settings.openDirectory")}
        </Button>
      </div>
    );
  }
  return <ReadyApp />;
}

function ReadyApp() {
  const { t, i18n } = useTranslation();
  const reportError = useErrorNotifier();
  const navigation = useUiStore((state) => state.navigation);
  const dirty = useUiStore((state) => state.dirty);
  const saveCurrent = useUiStore((state) => state.saveCurrent);
  const setNavigation = useUiStore((state) => state.setNavigation);
  const setDirty = useUiStore((state) => state.setDirty);
  const [pending, setPending] = useState<null | (() => void)>(null);
  const [closeConfirmationOpen, setCloseConfirmationOpen] = useState(false);
  const shutdownInFlight = useRef(false);
  const snapshot = useQuery({
    queryKey: ["app-snapshot"],
    queryFn: () => command<AppSnapshot>("get_app_snapshot"),
  });
  const savedZoomPercent = snapshot.data?.settings.uiZoomPercent;

  useEffect(() => {
    if (!snapshot.data) return;
    const language = snapshot.data.settings.language === "zh-cn" ? "zh-CN" : "en";
    void i18n.changeLanguage(language);
    const theme = snapshot.data.settings.theme;
    document.documentElement.dataset.theme = theme;
    if (theme === "system") delete document.documentElement.dataset.theme;
  }, [snapshot.data, i18n]);

  useEffect(() => {
    if (savedZoomPercent === undefined) return;
    void command<void>("set_ui_zoom", { uiZoomPercent: savedZoomPercent }).catch((error) =>
      reportError(error, "zoom"),
    );
  }, [savedZoomPercent, reportError]);

  useEffect(() => {
    void command("set_frontend_dirty", { dirty }).catch((error) =>
      reportError(error, "background"),
    );
  }, [dirty, reportError]);

  const shutdown = useCallback(async () => {
    if (shutdownInFlight.current) return;
    shutdownInFlight.current = true;
    try {
      await command("shutdown_app");
    } catch (error) {
      shutdownInFlight.current = false;
      reportError(error, "close");
    }
  }, [reportError]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onEvent<{ phase: string }>("cliswitch://close-state", () => {
      if (disposed || shutdownInFlight.current) return;
      void command<CloseState>("get_close_state")
        .then((closeState) => {
          if (disposed) return;
          const needsConfirmation =
            closeState.frontendDirty || closeState.oauthActive || closeState.applyActive;
          if (needsConfirmation) setCloseConfirmationOpen(true);
          else void shutdown();
        })
        .catch((error) => {
          if (!disposed) reportError(error, "close");
        });
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      })
      .catch((error) => {
        if (!disposed) reportError(error, "background");
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [reportError, shutdown]);

  const guarded = useCallback((action: () => void) => {
    if (useUiStore.getState().dirty) setPending(() => action);
    else action();
  }, []);

  const navigate = (next: Navigation) => guarded(() => setNavigation(next));

  if (snapshot.isPending) {
    return (
      <div className="splash">
        <Spinner /> {t("common.loading")}
      </div>
    );
  }
  if (!snapshot.data) {
    const error = snapshot.error ?? {
      code: "unknown",
      message: "Application snapshot did not contain data",
    };
    return (
      <div className="fatal-panel">
        <h1>{t("common.error")}</h1>
        <ErrorAlert
          error={error}
          title={t("errors.operations.load")}
          onRetry={() => void snapshot.refetch()}
          detailsOpen
          tone="error"
        />
      </div>
    );
  }

  return (
    <>
      <AppLayout navigation={navigation} onNavigate={navigate}>
        {snapshot.isError ? (
          <ErrorAlert
            error={snapshot.error}
            title={t("errors.operations.refresh")}
            onRetry={() => void snapshot.refetch()}
            tone="warning"
          />
        ) : null}
        {navigation === "configuration" ? (
          <ConfigurationPage snapshot={snapshot.data} guarded={guarded} onError={reportError} />
        ) : null}
        {navigation === "providers" ? (
          <ProviderPage snapshot={snapshot.data} guarded={guarded} onError={reportError} />
        ) : null}
        {navigation === "settings" ? (
          <SettingsPage snapshot={snapshot.data} onError={reportError} />
        ) : null}
      </AppLayout>
      <Modal
        open={Boolean(pending)}
        title={t("config.dirty")}
        onClose={() => setPending(null)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPending(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setDirty(false);
                pending?.();
                setPending(null);
              }}
            >
              {t("common.discard")}
            </Button>
            <Button
              disabled={!saveCurrent}
              onClick={async () => {
                try {
                  if (await saveCurrent?.()) {
                    setDirty(false);
                    pending?.();
                    setPending(null);
                  }
                } catch (error) {
                  reportError(error, "save");
                }
              }}
            >
              {t("common.save")}
            </Button>
          </>
        }
      >
        <p>{t("close.prompt")}</p>
      </Modal>
      <Modal
        open={closeConfirmationOpen}
        title={t("close.title")}
        onClose={() => setCloseConfirmationOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setCloseConfirmationOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => void shutdown()}>
              {t("common.confirm")}
            </Button>
          </>
        }
      >
        <p>{t("close.prompt")}</p>
      </Modal>
    </>
  );
}
