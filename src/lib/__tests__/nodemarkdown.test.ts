import { describe, expect, it } from "vitest";
import { looksLikeMarkdown, mdToNodeHtml } from "../nodemarkdown";
import { highlightCode, tokenizeCode } from "../codehighlight";
import { sanitizeHtml } from "../sanitize";

describe("looksLikeMarkdown", () => {
  it("检测常见 Markdown 结构", () => {
    expect(looksLikeMarkdown("# 标题\n正文")).toBe(true);
    expect(looksLikeMarkdown("普通**加粗**文本")).toBe(true);
    expect(looksLikeMarkdown("- 列表项")).toBe(true);
    expect(looksLikeMarkdown("1. 第一\n2. 第二")).toBe(true);
    expect(looksLikeMarkdown("> 引用")).toBe(true);
    expect(looksLikeMarkdown("```js\ncode\n```")).toBe(true);
  });
  it("纯文本不误判", () => {
    expect(looksLikeMarkdown("今天天气不错")).toBe(false);
    expect(looksLikeMarkdown("hello world 123")).toBe(false);
    expect(looksLikeMarkdown("a * b * c")).toBe(false); // 乘号不是斜体
  });
});

describe("mdToNodeHtml", () => {
  it("标题/加粗/斜体/删除线/行内代码", () => {
    const html = mdToNodeHtml("# 标题\n**粗** *斜* ~~删~~ `码`");
    expect(html).toContain("<h1>标题</h1>");
    expect(html).toContain("<strong>粗</strong>");
    expect(html).toContain("<em>斜</em>");
    expect(html).toContain("<s>删</s>");
    expect(html).toContain("<code>码</code>");
  });
  it("``` 围栏 → 嵌入式代码段（保留语言）", () => {
    const html = mdToNodeHtml("前文\n```ts\nconst a = 1;\n```\n后文");
    expect(html).toContain('<pre class="mm-code" data-lang="ts">const a = 1;</pre>');
  });
  it("列表 / 引用 / 分割线 / 链接（链接只留文字）", () => {
    const html = mdToNodeHtml("- 甲\n- 乙\n\n> 引用行\n\n---\n\n[点我](https://x.com)");
    expect(html).toContain("<ul><li>甲</li><li>乙</li></ul>");
    expect(html).toContain("<blockquote>引用行</blockquote>");
    expect(html).toContain("<hr>");
    expect(html).toContain("<u>点我</u>");
    expect(html).not.toContain("href");
  });
  it("HTML 注入被转义", () => {
    const html = mdToNodeHtml("<script>alert(1)</script>\n<img src=x onerror=1>");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;script&gt;");
  });
});

describe("highlightCode", () => {
  it("关键词/字符串/注释/数字分色", () => {
    const src = "// 注释\nconst s = \"hi\"; let n = 42;";
    const html = highlightCode(src, "ts");
    expect(html).toContain("tok-com");
    expect(html).toContain("tok-kw");
    expect(html).toContain("tok-str");
    expect(html).toContain("tok-num");
    expect(html).toContain("<code>");
  });
  it("先转义再包 span，无注入风险", () => {
    const html = highlightCode("const a = \"<script>\";", "js");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });
  it("Python 的 # 注释", () => {
    const toks = tokenizeCode("# py comment\nx = 1", "py");
    expect(toks[0]?.cls).toBe("tok-com");
  });
});

describe("sanitizeHtml 白名单扩展", () => {
  it("保留 pre.mm-code 的 class 与 data-lang", () => {
    const out = sanitizeHtml('<pre class="mm-code" data-lang="ts">const a = 1;</pre>');
    expect(out).toContain('class="mm-code"');
    expect(out).toContain('data-lang="ts"');
  });
  it("丢弃不认识的 class 词表", () => {
    const out = sanitizeHtml('<span class="evil-xss">x</span>');
    expect(out).not.toContain("evil-xss");
    expect(out).toContain("<span>x</span>");
  });
  it("h4/h5/h6 放行", () => {
    expect(sanitizeHtml("<h4>小标题</h4>")).toContain("<h4>");
  });
  it("代码高亮 span 类名放行", () => {
    const out = sanitizeHtml('<pre class="mm-code"><code><span class="tok-kw">const</span></code></pre>');
    expect(out).toContain('class="tok-kw"');
  });
});
