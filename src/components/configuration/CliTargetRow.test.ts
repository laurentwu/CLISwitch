import { describe, expect, it } from "vitest";
import type { ProviderCatalog, PublicProvider } from "../../shared/types";
import { makeTarget } from "./CliTargetRow";

const catalog: ProviderCatalog = {
  schemaVersion: 1,
  clis: [
    {
      id: "opencode",
      name: "OpenCode",
      protocols: ["openai-responses", "openai-chat", "anthropic-messages"],
      authModes: [],
      protocolAdapters: [],
    },
  ],
  providerTemplates: [
    {
      mode: "api",
      id: "glm-coding-plan",
      name: "GLM Coding Plan",
      category: "coding-plan",
      credentialSlots: [{ id: "api-key", name: "API Key" }],
      endpoints: [
        {
          id: "anthropic-messages",
          name: "Anthropic Messages",
          protocol: "anthropic-messages",
          baseUrl: "https://example.test/anthropic",
          credentialSlotId: "api-key",
          authOptions: [{ id: "bearer", authType: "bearer" }],
          defaultAuthOptionId: "bearer",
          models: [],
        },
        {
          id: "responses",
          name: "Responses",
          protocol: "openai-responses",
          baseUrl: "https://example.test/responses",
          credentialSlotId: "api-key",
          authOptions: [{ id: "bearer", authType: "bearer" }],
          defaultAuthOptionId: "bearer",
          models: [],
        },
      ],
    },
  ],
  relations: [
    {
      mode: "api",
      id: "opencode-glm-anthropic",
      cliId: "opencode",
      providerTemplateId: "glm-coding-plan",
      endpointId: "anthropic-messages",
      authOptionId: "bearer",
      default: false,
      nativeProviderIds: [],
    },
    {
      mode: "api",
      id: "opencode-glm-responses",
      cliId: "opencode",
      providerTemplateId: "glm-coding-plan",
      endpointId: "responses",
      authOptionId: "bearer",
      default: false,
      nativeProviderIds: [],
    },
  ],
};

const provider: PublicProvider = {
  id: "provider",
  name: "GLM",
  kind: "api",
  templateId: "glm-coding-plan",
  connections: [
    {
      id: "anthropic",
      templateEndpointId: "anthropic-messages",
      credentialSlotId: "api-key",
      protocol: "anthropic-messages",
      endpoint: "https://example.test/anthropic",
      authType: "bearer",
      defaultModel: "anthropic-model",
      verification: { status: "never-tested" },
    },
    {
      id: "responses",
      templateEndpointId: "responses",
      credentialSlotId: "api-key",
      protocol: "openai-responses",
      endpoint: "https://example.test/responses",
      authType: "bearer",
      defaultModel: "responses-model",
      verification: { status: "never-tested" },
    },
  ],
  referencedBy: [],
  revision: 1,
  updatedAt: "2026-08-23T00:00:00Z",
};

describe("makeTarget", () => {
  it("requires an explicit OpenCode endpoint when openai-compatible is not declared", () => {
    expect(makeTarget(catalog, "opencode", provider)).toMatchObject({
      connectionId: "",
      model: "",
    });
  });
});
