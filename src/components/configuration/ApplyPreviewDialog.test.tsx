import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type {
  ApplyPreview,
  ApplyRunSnapshot,
  ConfigurationTarget,
  SavedConfiguration,
} from "../../shared/types";
import { ApplyPreviewDialog } from "./ApplyPreviewDialog";

const commandMock = vi.hoisted(() => vi.fn());
const onEventMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock, onEvent: onEventMock }));

const configuration: SavedConfiguration = {
  id: "configuration-1",
  name: "Primary",
  creationOrder: 1,
  revision: 2,
  targets: [],
  createdAt: "2026-08-23T00:00:00Z",
  updatedAt: "2026-08-23T00:00:00Z",
};

const target: ConfigurationTarget = {
  targetType: "api",
  cliId: "codex",
  providerId: "provider-1",
  connectionId: "connection-1",
  model: "model-new",
};

const preview: ApplyPreview = {
  id: "preview-1",
  configurationId: configuration.id,
  configurationRevision: configuration.revision,
  createdAt: "2026-08-23T00:00:00Z",
  expiresAt: "2026-08-23T00:05:00Z",
  items: [
    {
      cliId: "codex",
      state: "waiting",
      path: "/tmp/codex/config.toml",
      providerName: "Provider",
      protocol: "openai-responses",
      model: "model-new",
      changes: [],
      files: [
        {
          path: "/tmp/codex/config.toml",
          existed: true,
          sourceContent: 'model = "model-old"\n',
          targetContent: 'model = "model-new"\n',
        },
      ],
    },
  ],
};

describe("ApplyPreviewDialog", () => {
  it("shows a read-only full-file preview for one CLI", async () => {
    commandMock.mockResolvedValue(preview);
    onEventMock.mockResolvedValue(() => undefined);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApplyPreviewDialog configuration={configuration} target={target} open onClose={vi.fn()} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("原文件")).toBeInTheDocument();
    expect(screen.getAllByText('model = "model-old"').length).toBeGreaterThan(0);
    expect(screen.getAllByText('model = "model-new"').length).toBeGreaterThan(0);
    expect(screen.getByText("文件差异")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "确认" })).not.toBeInTheDocument();
    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("preview_cli_apply", {
        configurationId: configuration.id,
        expectedRevision: configuration.revision,
        target,
      }),
    );
  });

  it("starts retry automatically after rebuilding failed items", async () => {
    const failedRun: ApplyRunSnapshot = {
      id: "run-failed",
      previewId: "preview-original",
      configurationId: configuration.id,
      startedAt: "2026-08-23T00:00:00Z",
      finishedAt: "2026-08-23T00:00:01Z",
      cancelRequested: false,
      items: [{ cliId: "codex", state: "failed", message: "write failed" }],
    };
    const retryPreview = { ...preview, id: "preview-retry" };
    const retriedRun: ApplyRunSnapshot = {
      ...failedRun,
      id: "run-retried",
      previewId: retryPreview.id,
      finishedAt: null,
      items: [{ cliId: "codex", state: "writing", message: null }],
    };
    commandMock.mockImplementation(async (name: string) => {
      if (name === "get_apply_snapshot") return failedRun;
      if (name === "retry_apply_items") return retryPreview;
      if (name === "start_apply") return retriedRun;
      return undefined;
    });
    onEventMock.mockResolvedValue(() => undefined);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ApplyPreviewDialog
          configuration={configuration}
          initialRun={failedRun}
          open
          onClose={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "重试失败项" }));

    await waitFor(() =>
      expect(commandMock).toHaveBeenCalledWith("start_apply", { previewId: retryPreview.id }),
    );
  });
});
