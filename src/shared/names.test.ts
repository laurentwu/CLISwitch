import { describe, expect, it } from "vitest";
import { uniqueCopyName, validateEntityName } from "./names";

describe("entity names", () => {
  const existing = [{ id: "one", name: "Development" }];

  it("validates trimmed Unicode length and case-insensitive uniqueness", () => {
    expect(validateEntityName("  ", existing)).toBe("length");
    expect(validateEntityName("a".repeat(65), existing)).toBe("length");
    expect(validateEntityName("😀".repeat(64), existing)).toBeUndefined();
    expect(validateEntityName("😀".repeat(65), existing)).toBe("length");
    expect(validateEntityName(" development ", existing)).toBe("duplicate");
    expect(validateEntityName("Development", existing, "one")).toBeUndefined();
  });

  it("produces an available copy name", () => {
    expect(uniqueCopyName("Development", "copy", existing)).toBe("Development copy");
    expect(uniqueCopyName("Development", "copy", [...existing, { name: "development copy" }])).toBe(
      "Development copy 2",
    );
  });
});
