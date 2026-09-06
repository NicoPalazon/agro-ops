import { describe, expect, it } from "vitest";
import { safeReturnPath } from "./return-path";

describe("safeReturnPath", () => {
  it("preserves the exact internal console root", () => {
    expect(safeReturnPath("/internal")).toBe("/internal");
  });

  it("preserves internal console descendants", () => {
    expect(safeReturnPath("/internal/system-status?tab=worker")).toBe(
      "/internal/system-status?tab=worker",
    );
  });

  it.each([
    "/internalized",
    "https://attacker.example/internal/system-status",
    "//attacker.example/internal/system-status",
    "/login",
    "/\\attacker.example",
    "/internal/%2f%2fattacker.example",
    ["/internal", "/internal/system-status"],
  ])("rejects unsafe or non-internal return path %s", (value) => {
    expect(safeReturnPath(value)).toBe("/internal");
  });
});
