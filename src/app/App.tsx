import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AppLayout } from "../components/layout/AppLayout";
import { Modal, Button, Spinner } from "../components/ui";
import { ConfigurationPage } from "../components/configuration/ConfigurationPage";
import { ProviderPage } from "../components/providers/ProviderPage";
import { SettingsPage } from "../components/settings/SettingsPage";
import { command, errorMessage, onEvent } from "../shared/ipc";
import type { AppSnapshot, CloseState, StartupStatus } from "../shared/types";
import { useUiStore, type Navigation } from "../stores/ui";

export function App() {
  const { t } = useTranslation();
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
    return (
      <div className="fatal-panel">
        <h1>{t("common.error")}</h1>
        <p>{errorMessage(startup.error)}</p>
        <Button onClick={() => startup.refetch()}>{t("common.retry")}</Button>
      </div>
    );
  }
  if (!startup.data.ready) {
    return (
      <div className="fatal-panel">
        <h1>{t("startup.title")}</h1>
        <p>{t("startup.readOnly")}</p>
        <p>
          <strong>{startup.data.code ?? "startup"}</strong>: {startup.data.message}
        </p>
        <p className="path-text">{startup.data.appDataDirectory}</p>
        <Button
          onClick={() =>
            command("open_startup_data_directory").catch(() => {
              /* The diagnostic remains readable even if the OS opener is unavailable. */
            })
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
  const navigation = useUiStore((state) => state.navigation);
  const dirty = useUiStore((state) => state.dirty);
  const saveCurrent = useUiStore((state) => state.saveCurrent);
  const setNavigation = useUiStore((state) => state.setNavigation);
  const setDirty = useUiStore((state) => state.setDirty);
  const [pending, setPending] = useState<null | (() => void)>(null);
  const [fatal, setFatal] = useState<string>();
  const [closeConfirmationOpen, setCloseConfirmationOpen] = useState(false);
  const shutdownInFlight = useRef(false);
  const snapshot = useQuery({
    queryKey: ["app-snapshot"],
    queryFn: () => command<AppSnapshot>("get_app_snapshot"),
  });

  useEffect(() => {
    if (!snapshot.data) return;
    const language = snapshot.data.settings.language === "zh-cn" ? "zh-CN" : "en";
    void i18n.changeLanguage(language);
    const theme = snapshot.data.settings.theme;
    document.documentElement.dataset.theme = theme;
    if (theme === "system") delete document.documentElement.dataset.theme;
  }, [snapshot.data, i18n]);

  useEffect(() => {
    void command("set_frontend_dirty", { dirty }).catch((error) => setFatal(errorMessage(error)));
  }, [dirty]);

  const shutdown = useCallback(async () => {
    if (shutdownInFlight.current) return;
    shutdownInFlight.current = true;
    try {
      await command("shutdown_app");
    } catch (error) {
      shutdownInFlight.current = false;
      setFatal(errorMessage(error));
    }
  }, []);

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
          if (!disposed) setFatal(errorMessage(error));
        });
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      })
      .catch((error) => {
        if (!disposed) setFatal(errorMessage(error));
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [shutdown]);

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
  if (snapshot.isError || !snapshot.data) {
    return (
      <div className="fatal-panel">
        <h1>{t("common.error")}</h1>
        <p>{fatal ?? errorMessage(snapshot.error)}</p>
        <Button onClick={() => snapshot.refetch()}>{t("common.retry")}</Button>
      </div>
    );
  }

  return (
    <>
      <AppLayout navigation={navigation} onNavigate={navigate}>
        {fatal ? (
          <div className="global-error" role="alert">
            {fatal}
          </div>
        ) : null}
        {navigation === "configuration" ? (
          <ConfigurationPage snapshot={snapshot.data} guarded={guarded} onError={setFatal} />
        ) : null}
        {navigation === "providers" ? (
          <ProviderPage snapshot={snapshot.data} guarded={guarded} onError={setFatal} />
        ) : null}
        {navigation === "settings" ? (
          <SettingsPage snapshot={snapshot.data} onError={setFatal} />
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
                  setFatal(errorMessage(error));
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
