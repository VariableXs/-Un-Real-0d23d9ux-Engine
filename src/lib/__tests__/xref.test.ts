/**
 * 批次C（规格 5.7.3）跨软件引用协议 — 单元测试。
 * 覆盖纯逻辑：href 编解码、锚点生成、引用扫描/去重、过期判定、
 * sanitize 对引用锚点的放行与危险链接拦截。
 * 跳转（emit/窗口）依赖 Tauri 运行时，不在此测试范围。
 */
import { describe, expect, it } from "vitest";
import { collectXrefs, isStale, parseXrefHref, xrefAnchorHtml, xrefHref } from "../xref";
import { sanitizeHtml } from "../sanitize";

describe("xrefHref / parseXrefHref（编解码往返）", () => {
  it("mind-node / write-doc 基本往返", () => {
    const href = xrefHref("mind-node", "n-123", 1725500000000);
    expect(href).toBe("xref:mind-node/n-123/1725500000000");
    const x = parseXrefHref(href);
    expect(x).toEqual({ kind: "mind-node", id: "n-123", ver: 1725500000000 });
  });

  it("code-file 含路径的 id 会被 URL 编码并还原", () => {
    const href = xrefHref("code-file", "src/lib/main.rs", 7);
    expect(href).toBe("xref:code-file/src%2Flib%2Fmain.rs/7");
    const x = parseXrefHref(href);
    expect(x).toEqual({ kind: "code-file", id: "src/lib/main.rs", ver: 7 });
  });

  it("非 xref / 非法格式返回 null", () => {
    expect(parseXrefHref("https://example.com")).toBeNull();
    expect(parseXrefHref("xref:")).toBeNull();
    expect(parseXrefHref("xref:mind-node")).toBeNull();
    expect(parseXrefHref("xref:mind-node/n1/abc")).toBeNull();
    expect(parseXrefHref("xref:other-kind/n1/5")).toBeNull();
    expect(parseXrefHref("xref:write-doc//5")).toBeNull();
  });
});

describe("xrefAnchorHtml（锚点生成）", () => {
  it("生成带 class 与 href 的 <a>", () => {
    const html = xrefAnchorHtml("mind-node", "n1", 5, "用户研究");
    expect(html).toBe('<a class="xref-link" href="xref:mind-node/n1/5">↗ 用户研究</a>');
  });

  it("标题中的 HTML 特殊字符被剔除", () => {
    const html = xrefAnchorHtml("write-doc", "d1", 9, '<b>"标题" & 注入</b>');
    expect(html).not.toContain("<b>");
    expect(html).toContain("标题");
  });

  it("空标题回退为 kind", () => {
    const html = xrefAnchorHtml("write-doc", "d1", 9, "  ");
    expect(html).toContain(">↗ write-doc</a>");
  });
});

describe("collectXrefs（引用扫描与去重）", () => {
  it("扫描全部引用并按 kind+id 去重取最小 ver", () => {
    const html =
      '<p><a class="xref-link" href="xref:mind-node/n1/100">a</a>' +
      '<a class="xref-link" href="xref:mind-node/n1/40">a-old</a>' +
      '<a class="xref-link" href="xref:write-doc/d1/7">b</a></p>';
    const refs = collectXrefs(html);
    expect(refs).toHaveLength(2);
    const n1 = refs.find((r) => r.id === "n1");
    expect(n1).toEqual({ kind: "mind-node", id: "n1", ver: 40 });
  });

  it("忽略普通链接与空文档", () => {
    expect(collectXrefs('<a href="https://example.com">x</a>')).toHaveLength(0);
    expect(collectXrefs("")).toHaveLength(0);
  });
});

describe("isStale（过期判定）", () => {
  it("源版本大于引用版本 → 过期", () => {
    expect(isStale({ kind: "mind-node", id: "n1", ver: 100 }, 150)).toBe(true);
    expect(isStale({ kind: "mind-node", id: "n1", ver: 100 }, 100)).toBe(false);
    expect(isStale({ kind: "mind-node", id: "n1", ver: 100 }, 90)).toBe(false);
  });
});

describe("sanitizeHtml 与引用锚点（规格 5.7.3 安全边界）", () => {
  it("放行 xref: 锚点（href + 引用样式类）", () => {
    const out = sanitizeHtml('<a class="xref-link" href="xref:mind-node/n1/5">↗ 节点</a>');
    expect(out).toContain('href="xref:mind-node/n1/5"');
    expect(out).toContain('class="xref-link"');
  });

  it("拦截 javascript: 与 http(s) 外链", () => {
    expect(sanitizeHtml('<a href="javascript:alert(1)">x</a>')).not.toContain("javascript:");
    expect(sanitizeHtml('<a href="https://evil.example">x</a>')).not.toContain("https://evil.example");
  });

  it("拦截带 onclick 的引用锚点", () => {
    const out = sanitizeHtml('<a class="xref-link" href="xref:mind-node/n1/5" onclick="alert(1)">x</a>');
    expect(out).not.toContain("onclick");
  });
});
