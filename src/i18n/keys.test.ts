import { describe, expect, it } from "vitest";
import { en } from "./en";
import { zhCN } from "./zh-CN";

function keys(value: object, prefix = ""): string[] {
  return Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return child && typeof child === "object" ? keys(child, path) : [path];
  });
}

describe("translations", () => {
  it("keeps English and Chinese keys exactly aligned", () => {
    expect(keys(en).sort()).toEqual(keys(zhCN).sort());
  });
});
