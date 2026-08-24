import { describe, expect, it } from "vitest";
import type { PublicProvider } from "../../shared/types";
import { makeTarget } from "./CliTargetRow";

const provider: PublicProvider = {
  id: "provider",
  name: "multi",
  kind: "api",
  codingPlan: false,
  connections: [
    {
      id: "chat",
      protocol: "openai-chat",
      endpoint: "https://example.test/v1",
      authType: "bearer",
      defaultModel: "chat-model",
      verification: { status: "never-tested" },
    },
    {
      id: "responses",
      protocol: "openai-responses",
      endpoint: "https://example.test/v1",
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
  it("uses the fixed Responses-first OpenCode priority", () => {
    expect(makeTarget("opencode", provider)).toMatchObject({
      connectionId: "responses",
      model: "responses-model",
    });
  });
});
