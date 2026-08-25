export interface AppFailure {
  code: string;
  message: string;
}

export type ErrorLevel = "info" | "warning" | "error";

export type ErrorGuidance =
  | "validation"
  | "notFound"
  | "conflict"
  | "blocked"
  | "unsupported"
  | "network"
  | "io"
  | "internal"
  | "cancelled"
  | "unexpected";

function errorFromRecord(value: Record<string, unknown>): AppFailure | undefined {
  if (typeof value.message !== "string") return undefined;
  return {
    code: typeof value.code === "string" && value.code ? value.code : "unknown",
    message: value.message,
  };
}

export function normalizeError(error: unknown): AppFailure {
  if (typeof error === "string") {
    try {
      const parsed: unknown = JSON.parse(error);
      if (parsed && typeof parsed === "object") {
        const normalized = errorFromRecord(parsed as Record<string, unknown>);
        if (normalized) return normalized;
      }
    } catch {
      // IPC implementations may reject with a plain, non-JSON string.
    }
    return { code: "unknown", message: error };
  }
  if (error && typeof error === "object") {
    const normalized = errorFromRecord(error as Record<string, unknown>);
    if (normalized) return normalized;
  }
  return { code: "unknown", message: "Unknown error" };
}

export function errorLevel(code: string): ErrorLevel {
  if (code === "cancelled") return "info";
  if (["validation", "not-found", "conflict", "blocked", "unsupported"].includes(code)) {
    return "warning";
  }
  return "error";
}

export function errorGuidance(code: string): ErrorGuidance {
  switch (code) {
    case "validation":
      return "validation";
    case "not-found":
      return "notFound";
    case "conflict":
      return "conflict";
    case "blocked":
      return "blocked";
    case "unsupported":
      return "unsupported";
    case "network":
      return "network";
    case "io":
      return "io";
    case "database":
    case "migration":
    case "serialization":
      return "internal";
    case "cancelled":
      return "cancelled";
    default:
      return "unexpected";
  }
}

export function isCancellationError(error: unknown): boolean {
  return normalizeError(error).code === "cancelled";
}
