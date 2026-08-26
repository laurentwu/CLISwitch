import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { OAuthProviderDetail, ProviderCatalog, PublicProvider } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { OAuthProviderEditor } from "./OAuthProviderEditor";

const commandMock = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ipc", () => ({ command: commandMock }));

const detail: OAuthProviderDetail = {
  id: "oauth-id",
  name: "Personal OAuth",
  templateId: "codex-auth",
  revision: 1,
  createdAt: "2026-08-23T00:00:00Z",
  updatedAt: "2026-08-23T00:00:00Z",
  profileType: "oauth",
  oauthKind: "codex",
  accountId: "account-fixture",
  rawContent: '{"access_token":"full-plaintext-fixture"}',
  digest: "sha256:fixture",
  manuallyModified: false,
  verification: { status: "valid" },
};
const publicProvider: PublicProvider = {
  id: detail.id,
  name: detail.name,
  kind: "oauth",
  templateId: "codex-auth",
  oauthKind: "codex",
  connections: [],
  referencedBy: [],
  revision: 1,
  updatedAt: detail.updatedAt,
};
const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [],
  providerTemplates: [
    { mode: "auth", id: "anthropic-auth", name: "Anthropic Account", authKind: "anthropic" },
    { mode: "auth", id: "codex-auth", name: "Codex Account", authKind: "codex" },
  ],
  relations: [],
};

describe("OAuthProviderEditor", () => {
  beforeEach(() => {
    commandMock.mockReset();
    useUiStore.setState({ dirty: false, saveCurrent: undefined });
  });

  it("shows the complete raw credential and defers validation until save", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          catalog={catalog}
          providers={[publicProvider]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(screen.getByRole("textbox", { name: /OAuth 原始内容/ })).toHaveValue(detail.rawContent);
    expect(screen.getByText(/保存时会校验/)).toBeInTheDocument();
  });

  it("uses the same header action order and reports inline duplicate-name validation", () => {
    const client = new QueryClient();
    const other = { ...publicProvider, id: "other-id", name: "Existing" };
    const onDelete = vi.fn();
    render(
      <QueryClientProvider client={client}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          catalog={catalog}
          providers={[publicProvider, other]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
          onDelete={onDelete}
          onDuplicate={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const header = screen.getByRole("heading", { name: detail.name }).closest("header");
    expect(header).not.toBeNull();
    expect(
      Array.from(header!.querySelectorAll("button"), (button) => button.textContent?.trim()),
    ).toEqual(["删除", "复制", "取消", "保存"]);
    fireEvent.change(screen.getByRole("textbox", { name: "名称" }), {
      target: { value: " existing " },
    });
    expect(screen.getByText(/名称与已有项目重复/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    expect(onDelete).toHaveBeenCalledOnce();
  });

  it("passes a prefilled draft to duplicate without creating immediately", () => {
    const onDuplicate = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          catalog={catalog}
          providers={[publicProvider]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
          onDuplicate={onDuplicate}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "复制" }));

    expect(onDuplicate).toHaveBeenCalledWith({
      templateId: "codex-auth",
      kind: "codex",
      name: detail.name,
      rawContent: detail.rawContent,
    });
    expect(commandMock).not.toHaveBeenCalled();
  });

  it("keeps edited secret content when saving fails", async () => {
    const onError = vi.fn();
    commandMock.mockImplementation(async (name: string) => {
      if (name === "update_oauth_provider") {
        throw { code: "conflict", message: "provider changed" };
      }
      return undefined;
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          catalog={catalog}
          providers={[publicProvider]}
          onClose={vi.fn()}
          onError={onError}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );
    const raw = screen.getByRole("textbox", { name: /OAuth 原始内容/ });
    fireEvent.change(raw, { target: { value: "edited-secret-content" } });
    fireEvent.click(screen.getAllByRole("button", { name: "保存" })[0]);

    await waitFor(() => expect(onError).toHaveBeenCalled());
    expect(onError.mock.calls[0]?.[0]).toMatchObject({
      code: "conflict",
      message: "provider changed",
    });
    expect(onError.mock.calls[0]?.[1]).toBe("save");
    expect(raw).toHaveValue("edited-secret-content");
  });

  it("disables a no-op edit save without changing the stored verification state", () => {
    const onError = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          catalog={catalog}
          providers={[publicProvider]}
          onClose={vi.fn()}
          onError={onError}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const save = screen.getByRole("button", { name: "保存" });
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(commandMock).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });

  it("creates OAuth from the raw editor only after content is provided", async () => {
    const onCreated = vi.fn();
    commandMock.mockImplementation(async (name: string) => {
      if (name === "create_oauth_provider") return { id: "created-oauth" };
      return undefined;
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          catalog={catalog}
          initialTemplateId="codex-auth"
          initialName="New Codex"
          initialRaw=""
          providers={[]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
          onCreated={onCreated}
        />
      </QueryClientProvider>,
    );

    const save = screen.getByRole("button", { name: "保存" });
    expect(save).toBeDisabled();
    const raw = screen.getByRole("textbox", { name: /OAuth 原始内容/ });
    fireEvent.change(raw, { target: { value: '{"tokens":{"access_token":"fixture"}}' } });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("created-oauth"));
    expect(commandMock).toHaveBeenCalledWith("create_oauth_provider", {
      kind: "codex",
      name: "New Codex",
      rawContent: '{"tokens":{"access_token":"fixture"}}',
    });
  });

  it("keeps a failed OAuth creation draft available for correction", async () => {
    const onError = vi.fn();
    commandMock.mockImplementation(async (name: string) => {
      if (name === "create_oauth_provider") {
        throw { code: "conflict", message: "OAuth credentials already exist" };
      }
      return undefined;
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          catalog={catalog}
          initialTemplateId="codex-auth"
          initialName="Copied Codex"
          initialRaw="copied-raw"
          providers={[]}
          onClose={vi.fn()}
          onError={onError}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const raw = screen.getByRole("textbox", { name: /OAuth 原始内容/ });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith(
        { code: "conflict", message: "OAuth credentials already exist" },
        "create",
      ),
    );
    expect(raw).toHaveValue("copied-raw");
  });
});
