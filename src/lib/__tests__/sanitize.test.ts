import { describe, expect, it } from "vitest";
import { sanitizeHtml, isSafeMediaSrc } from "../sanitize";

describe("sanitizeHtml", () => {
  it("removes script tags entirely", () => {
    const out = sanitizeHtml('<p>a</p><script>alert(1)</script>');
    expect(out).not.toContain("script");
    expect(out).not.toContain("alert");
    expect(out).toContain("<p>");
  });

  it("removes event handlers", () => {
    const out = sanitizeHtml('<img src="C:\\pic.png" onerror="alert(1)">');
    expect(out).not.toContain("onerror");
    expect(out).toContain("src=");
  });

  it("keeps allowed formatting tags", () => {
    const out = sanitizeHtml("<h1><strong>x</strong></h1><ul><li>i</li></ul><blockquote>q</blockquote><hr>");
    expect(out).toContain("<h1>");
    expect(out).toContain("<strong>");
    expect(out).toContain("<ul>");
    expect(out).toContain("<hr>");
  });

  it("rejects javascript: media sources", () => {
    const out = sanitizeHtml('<img src="javascript:alert(1)">');
    expect(out).not.toContain("javascript:");
  });

  it("accepts local absolute and asset sources", () => {
    expect(isSafeMediaSrc("C:\\Users\\me\\a.png")).toBe(true);
    expect(isSafeMediaSrc("/home/me/a.png")).toBe(true);
    expect(isSafeMediaSrc("http://asset.localhost/x.png")).toBe(true);
    expect(isSafeMediaSrc("https://evil.example/x.png")).toBe(false);
  });

  it("strips iframe/object/style/link/meta", () => {
    const out = sanitizeHtml('<iframe src="x"></iframe><object></object><style>p{}</style>');
    expect(out).not.toMatch(/iframe|object|style/i);
  });

  it("keeps safe inline style props only", () => {
    const out = sanitizeHtml('<span style="color:#fff;background:url(x)">t</span>');
    expect(out).toContain("color");
    expect(out).not.toContain("url(");
  });
});
