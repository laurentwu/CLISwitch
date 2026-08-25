import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { OAuthProviderDetail, PublicProvider } from "../../shared/types";
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

describe("OAuthProviderEditor", () => {
  beforeEach(() => commandMock.mockReset());

  it("shows the complete raw credential as editable plaintext by default", () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          providers={[publicProvider]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(screen.getByRole("textbox", { name: /OAuth 原始内容/ })).toHaveValue(detail.rawContent);
    expect(screen.getByText(/不会验证格式/)).toBeInTheDocument();
  });

  it("shows inline duplicate-name validation and uses an in-app delete dialog", () => {
    const client = new QueryClient();
    const other = { ...publicProvider, id: "other-id", name: "Existing" };
    render(
      <QueryClientProvider client={client}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
          providers={[publicProvider, other]}
          onClose={vi.fn()}
          onError={vi.fn()}
          onStartFlow={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.change(screen.getByRole("textbox", { name: "名称" }), {
      target: { value: " existing " },
    });
    expect(screen.getByText(/名称与已有项目重复/)).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: /删除/ })[0]);
    expect(screen.getByRole("dialog", { name: "确认删除" })).toBeInTheDocument();
  });

  it("keeps edited secret content when saving fails", async () => {
    const onError = vi.fn();
    commandMock.mockImplementation(async (name: string) => {
      if (name === "update_oauth_raw_content") {
        throw { code: "conflict", message: "provider changed" };
      }
      return undefined;
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <OAuthProviderEditor
          detail={detail}
          publicProvider={publicProvider}
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
});
