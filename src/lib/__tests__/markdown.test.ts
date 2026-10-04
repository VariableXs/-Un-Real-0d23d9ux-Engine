import { describe, expect, it } from "vitest";
import { markdownToHtml } from "../markdown";

describe("markdownToHtml", () => {
  it("converts headings, lists and paragraphs", () => {
    const html = markdownToHtml("# Title\n\n- a\n- b\n\nplain text");
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<ul>");
    expect(html).toContain("<li>a</li>");
    expect(html).toContain("<p>plain text</p>");
  });

  it("escapes html in content", () => {
    const html = markdownToHtml("<script>alert(1)</script>");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });

  it("keeps fenced code blocks verbatim (no styling inside)", () => {
    const html = markdownToHtml("```\n**not bold** <raw>\n```");
    expect(html).toContain("<pre><code>");
    expect(html).toContain("**not bold**");
    expect(html).toContain("&lt;raw&gt;");
  });

  it("supports inline styles and links", () => {
    const html = markdownToHtml("[site](https://a.b) **bold** *it* `c`");
    expect(html).toContain('<a href="https://a.b">site</a>');
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<em>it</em>");
    expect(html).toContain("<code>c</code>");
  });

  it("handles ordered lists and quotes", () => {
    const html = markdownToHtml("1. one\n2. two\n\n> quoted");
    expect(html).toContain("<ol>");
    expect(html).toContain("<blockquote>");
  });
});
