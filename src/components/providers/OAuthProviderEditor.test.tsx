import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { OAuthProviderDetail, PublicProvider } from "../../shared/types";
import { OAuthProviderEditor } from "./OAuthProviderEditor";

const detail: OAuthProviderDetail = {
  id: "oauth-id",
  name: "Personal OAuth",
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
  oauthKind: "codex",
  codingPlan: false,
  connections: [],
  referencedBy: [],
  revision: 1,
  updatedAt: detail.updatedAt,
};

describe("OAuthProviderEditor", () => {
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
});
