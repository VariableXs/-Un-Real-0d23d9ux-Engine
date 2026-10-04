import { describe, expect, it } from "vitest";
import { isPlainFileName, validateItemName } from "../pathGuard";

describe("isPlainFileName", () => {
  it("accepts simple names", () => {
    expect(isPlainFileName("photo.png")).toBe(true);
    expect(isPlainFileName("笔记-2026.md")).toBe(true);
  });
  it("rejects traversal and separators", () => {
    expect(isPlainFileName("..\\evil.exe")).toBe(false);
    expect(isPlainFileName("../../etc/passwd")).toBe(false);
    expect(isPlainFileName("a/b")).toBe(false);
    expect(isPlainFileName("..")).toBe(false);
    expect(isPlainFileName("")).toBe(false);
  });
});

describe("validateItemName", () => {
  it("rejects empty", () => {
    expect(validateItemName("   ").reason).toBe("empty");
  });
  it("rejects too long", () => {
    expect(validateItemName("x".repeat(121)).reason).toBe("too-long");
  });
  it("rejects path separators", () => {
    expect(validateItemName("a/b").reason).toBe("illegal");
    expect(validateItemName("a\\b").reason).toBe("illegal");
  });
  it("accepts normal names trimmed-able", () => {
    expect(validateItemName("  工作  ").ok).toBe(true);
  });
});
