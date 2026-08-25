import { describe, expect, it } from "vitest";
import { errorGuidance, errorLevel, isCancellationError, normalizeError } from "./errors";

describe("normalizeError", () => {
  it("preserves structured IPC errors", () => {
    expect(normalizeError({ code: "conflict", message: "revision changed" })).toEqual({
      code: "conflict",
      message: "revision changed",
    });
  });

  it("parses serialized IPC errors and accepts ordinary Error instances", () => {
    expect(normalizeError('{"code":"network","message":"offline"}')).toEqual({
      code: "network",
      message: "offline",
    });
    expect(normalizeError(new Error("frontend failure"))).toEqual({
      code: "unknown",
      message: "frontend failure",
    });
  });

  it("classifies recoverable and cancelled errors", () => {
    expect(errorLevel("blocked")).toBe("warning");
    expect(errorGuidance("conflict")).toBe("conflict");
    expect(errorLevel("database")).toBe("error");
    expect(isCancellationError({ code: "cancelled", message: "cancelled" })).toBe(true);
  });
});
