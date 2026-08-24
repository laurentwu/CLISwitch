import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "./i18n";
import "./styles.css";
import { App } from "./app/App";
import { queryClient } from "./app/queryClient";

async function bootstrap() {
  // The WebdriverIO bridge is compiled only into the dedicated E2E build. Production mode
  // constant-folds this branch away, leaving no automation bridge or global Tauri API.
  if (import.meta.env.MODE === "e2e") {
    await import("@wdio/tauri-plugin");
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </StrictMode>,
  );
}

void bootstrap();
