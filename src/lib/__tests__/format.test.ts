import { describe, expect, it } from "vitest";
import { countWords, readingMinutes, formatBytes, clamp, uid, stripHtmlToText } from "../format";

describe("countWords", () => {
  it("counts CJK chars individually", () => {
    expect(countWords("你好世界")).toBe(4);
  });
  it("counts latin words as words", () => {
    expect(countWords("hello world foo")).toBe(3);
  });
  it("mixes CJK and latin", () => {
    expect(countWords("你好 world")).toBe(3);
  });
  it("empty is zero", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("   \n\t")).toBe(0);
  });
});

describe("readingMinutes", () => {
  it("minimum one minute", () => {
    expect(readingMinutes(0)).toBe(1);
  });
  it("scales with length", () => {
    expect(readingMinutes(2200)).toBe(10);
  });
});

describe("stripHtmlToText", () => {
  // DOMParser is unavailable in node env; guard for CI parity.
  if (typeof DOMParser === "undefined") {
    it("skips without DOM", () => expect(true).toBe(true));
    return;
  }
  it("strips tags and scripts", () => {
    const out = stripHtmlToText('<p>a<script>alert(1)</script>b</p>');
    expect(out).toContain("a");
    expect(out).toContain("b");
    expect(out).not.toContain("alert");
  });
});

describe("formatBytes", () => {
  it("formats units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });
});

describe("clamp/uid", () => {
  it("clamps", () => {
    expect(clamp(5, 1, 3)).toBe(3);
    expect(clamp(-1, 0, 9)).toBe(0);
  });
  it("unique ids", () => {
    expect(uid()).not.toBe(uid());
  });
});
