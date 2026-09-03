import { describe, expect, it } from "vitest";
import type {
  CliProtocol,
  ProviderCatalog,
  PublicProvider,
  PublicProviderConnection,
} from "./types";
import {
  connectionDisplayName,
  connectionsForCli,
  preferredConnectionForCli,
  providerSupportsCli,
} from "./catalog";

const endpoint = (id: string, protocol: CliProtocol) => ({
  id,
  name: id,
  protocol,
  baseUrl: `https://${id}.example.test/v1`,
  credentialSlotId: "api-key",
  authOptions: [{ id: "bearer", authType: "bearer" as const }],
  defaultAuthOptionId: "bearer",
  models: [],
});

const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [
    {
      id: "claude-code",
      name: "Claude Code",
      protocols: ["anthropic-messages"],
      authModes: [{ id: "anthropic-oauth", oauthKind: "anthropic" }],
      protocolAdapters: [],
    },
    {
      id: "codex",
      name: "Codex CLI",
      protocols: ["openai-responses"],
      authModes: [{ id: "codex-oauth", oauthKind: "codex" }],
      protocolAdapters: [],
    },
    {
      id: "opencode",
      name: "OpenCode",
      protocols: ["openai-chat", "openai-responses", "anthropic-messages"],
      authModes: [],
      protocolAdapters: [],
    },
  ],
  providerTemplates: [
    {
      mode: "api",
      id: "multi-endpoint-plan",
      name: "Multi-endpoint plan",
      category: "coding-plan",
      credentialSlots: [{ id: "api-key", name: "API Key" }],
      endpoints: [
        endpoint("anthropic", "anthropic-messages"),
        endpoint("chat", "openai-chat"),
        endpoint("responses", "openai-responses"),
      ],
    },
    {
      mode: "auth",
      id: "anthropic-auth",
      name: "Anthropic Account",
      authKind: "anthropic",
    },
  ],
  relations: [
    {
      mode: "api",
      id: "claude-plan-anthropic",
      cliId: "claude-code",
      providerTemplateId: "multi-endpoint-plan",
      endpointId: "anthropic",
      authOptionId: "bearer",
      default: true,
      nativeProviderIds: [],
    },
    {
      mode: "api",
      id: "opencode-plan-anthropic",
      cliId: "opencode",
      providerTemplateId: "multi-endpoint-plan",
      endpointId: "anthropic",
      authOptionId: "bearer",
      default: false,
      nativeProviderIds: [],
    },
    {
      mode: "api",
      id: "opencode-plan-chat",
      cliId: "opencode",
      providerTemplateId: "multi-endpoint-plan",
      endpointId: "chat",
      authOptionId: "bearer",
      default: true,
      nativeProviderIds: [],
    },
    {
      mode: "auth",
      id: "claude-anthropic-auth",
      cliId: "claude-code",
      providerTemplateId: "anthropic-auth",
      authModeId: "anthropic-oauth",
    },
  ],
};

const connection = (
  id: string,
  protocol: CliProtocol,
  templateEndpointId?: string,
): PublicProviderConnection => ({
  id,
  templateEndpointId,
  credentialSlotId: "api-key",
  protocol,
  endpoint: `https://${id}.example.test/v1`,
  authType: "bearer",
  defaultModel: "model-a",
  verification: { status: "never-tested" },
});

const templatedApiProvider: PublicProvider = {
  id: "templated-api",
  name: "Templated API",
  kind: "api",
  templateId: "multi-endpoint-plan",
  connections: [
    connection("anthropic-connection", "anthropic-messages", "anthropic"),
    connection("chat-connection", "openai-chat", "chat"),
    connection("responses-connection", "openai-responses", "responses"),
  ],
  referencedBy: [],
  revision: 1,
  updatedAt: "2026-08-23T00:00:00Z",
};

describe("provider catalog selectors", () => {
  it("uses the template endpoint name as the connection label", () => {
    expect(
      connectionDisplayName(catalog, templatedApiProvider, templatedApiProvider.connections[1]),
    ).toBe("chat");
  });

  it("falls back to the protocol when the endpoint name is unavailable", () => {
    const custom: PublicProvider = {
      ...templatedApiProvider,
      templateId: null,
    };

    expect(connectionDisplayName(catalog, custom, custom.connections[1])).toBe("openai-chat");
  });

  it("filters templated connections through explicit CLI relations and honors the default", () => {
    expect(
      connectionsForCli(catalog, "claude-code", templatedApiProvider).map(
        (candidate) => candidate.id,
      ),
    ).toEqual(["anthropic-connection"]);
    expect(
      connectionsForCli(catalog, "opencode", templatedApiProvider).map((candidate) => candidate.id),
    ).toEqual(["anthropic-connection", "chat-connection"]);
    expect(preferredConnectionForCli(catalog, "opencode", templatedApiProvider)?.id).toBe(
      "chat-connection",
    );
    expect(providerSupportsCli(catalog, "codex", templatedApiProvider)).toBe(false);
  });

  it("falls back to CLI protocol capabilities only for custom API providers", () => {
    const custom: PublicProvider = {
      ...templatedApiProvider,
      id: "custom-api",
      templateId: null,
      connections: [
        connection("custom-anthropic", "anthropic-messages"),
        connection("custom-responses", "openai-responses"),
      ],
    };

    expect(connectionsForCli(catalog, "codex", custom).map((candidate) => candidate.id)).toEqual([
      "custom-responses",
    ]);
    expect(preferredConnectionForCli(catalog, "codex", custom)?.id).toBe("custom-responses");
  });

  it("keeps connections from a saved provider whose source template left the catalog", () => {
    const stale: PublicProvider = {
      ...templatedApiProvider,
      id: "stale-api",
      templateId: "previous-catalog-provider",
      connections: [
        connection("stale-chat", "openai-chat"),
        connection("stale-responses", "openai-responses"),
      ],
    };

    expect(connectionsForCli(catalog, "codex", stale).map((candidate) => candidate.id)).toEqual([
      "stale-responses",
    ]);
    expect(providerSupportsCli(catalog, "opencode", stale)).toBe(true);
  });

  it("matches auth templates by relation and legacy OAuth providers by CLI auth mode", () => {
    const templatedOAuth: PublicProvider = {
      id: "templated-oauth",
      name: "Anthropic Account",
      kind: "oauth",
      templateId: "anthropic-auth",
      oauthKind: "anthropic",
      connections: [],
      referencedBy: [],
      revision: 1,
      updatedAt: "2026-08-23T00:00:00Z",
    };
    const legacyOAuth: PublicProvider = {
      ...templatedOAuth,
      id: "legacy-oauth",
      templateId: null,
      oauthKind: "codex",
    };

    expect(providerSupportsCli(catalog, "claude-code", templatedOAuth)).toBe(true);
    expect(providerSupportsCli(catalog, "codex", templatedOAuth)).toBe(false);
    expect(providerSupportsCli(catalog, "codex", legacyOAuth)).toBe(true);
    expect(providerSupportsCli(catalog, "claude-code", legacyOAuth)).toBe(false);
  });

  it("uses CLIAdapter provider metadata and explicit endpoint relations", () => {
    const dynamicCatalog: ProviderCatalog = {
      ...catalog,
      providerTemplates: [
        ...catalog.providerTemplates,
        {
          mode: "api",
          id: "dynamic-chat",
          name: "Dynamic Chat",
          category: "cli-adapter",
          credentialSlots: [{ id: "api-key", name: "API Key" }],
          endpoints: [endpoint("openai-compatible", "openai-chat")],
        },
      ],
      relations: [
        ...catalog.relations,
        {
          mode: "api",
          id: "opencode-dynamic-chat",
          cliId: "opencode",
          providerTemplateId: "dynamic-chat",
          endpointId: "openai-compatible",
          authOptionId: "bearer",
          default: true,
          nativeProviderIds: ["dynamic-chat"],
        },
      ],
      providerInfo: [
        {
          id: "dynamic-chat",
          name: "Dynamic Chat",
          env: ["DYNAMIC_API_KEY"],
          selectable: true,
          supportedClis: ["opencode"],
          endpoints: [
            {
              id: "openai-compatible",
              protocol: "openai-chat",
              endpoint: "https://dynamic.example/v1",
              selectable: true,
              supportedClis: ["opencode"],
            },
          ],
        },
        {
          id: "disabled-chat",
          name: "Disabled Chat",
          env: [],
          selectable: false,
          disabledReason: "provider adapter is unsupported",
          supportedClis: [],
          endpoints: [],
        },
      ],
    };
    const dynamicProvider: PublicProvider = {
      ...templatedApiProvider,
      id: "dynamic-provider",
      templateId: "dynamic-chat",
      connections: [
        connection("dynamic-chat-connection", "openai-chat", "openai-compatible"),
        connection("dynamic-responses-connection", "openai-responses", "responses"),
      ],
    };
    const disabledProvider: PublicProvider = {
      ...dynamicProvider,
      id: "disabled-provider",
      templateId: "disabled-chat",
    };

    expect(
      connectionsForCli(dynamicCatalog, "opencode", dynamicProvider).map(
        (candidate) => candidate.id,
      ),
    ).toEqual(["dynamic-chat-connection"]);
    expect(providerSupportsCli(dynamicCatalog, "opencode", dynamicProvider)).toBe(true);
    expect(providerSupportsCli(dynamicCatalog, "codex", dynamicProvider)).toBe(false);
    expect(connectionsForCli(dynamicCatalog, "opencode", disabledProvider)).toEqual([]);
    expect(providerSupportsCli(dynamicCatalog, "opencode", disabledProvider)).toBe(false);
  });
});
