/**
 * 批次C（规格 5.7）跨软件数据流协议 — 单元测试。
 * 覆盖纯逻辑：SVG 卡片折行与确定性渲染（Mind 节点 → Write 图片）。
 * 事件/IPC 部分依赖 Tauri 运行时，不在此测试范围。
 */
import { describe, expect, it } from "vitest";
import { foldLines, mindCardDataUrl } from "../xflow";

describe("foldLines（近似宽度折行）", () => {
  it("空文本返回空数组", () => {
    expect(foldLines("")).toEqual([]);
    expect(foldLines("   ")).toEqual([]);
  });

  it("短文本不折行", () => {
    expect(foldLines("hello world", 3)).toEqual(["hello world"]);
  });

  it("长文本按宽度切分", () => {
    const lines = foldLines("a".repeat(50), 6);
    expect(lines.length).toBe(3);
    expect(lines[0]).toBe("a".repeat(22));
  });

  it("超出行数截断并加省略号", () => {
    const lines = foldLines("b".repeat(200), 3);
    expect(lines.length).toBe(3);
    expect(lines[2]!.endsWith("…")).toBe(true);
  });

  it("空白折叠为单空格", () => {
    expect(foldLines("a\n\n  b\tc", 3)).toEqual(["a b c"]);
  });
});

describe("mindCardDataUrl（Mind 节点 → Write 图片卡片）", () => {
  it("生成 data:image/svg+xml URL", () => {
    const url = mindCardDataUrl("标题", "正文内容");
    expect(url.startsWith("data:image/svg+xml;utf8,")).toBe(true);
  });

  it("确定性：同输入同输出", () => {
    expect(mindCardDataUrl("T", "B")).toBe(mindCardDataUrl("T", "B"));
  });

  it("XML 特殊字符被转义", () => {
    const url = mindCardDataUrl("a<b>&\"c", "x");
    expect(url).not.toContain("<b>");
    // svgEscape 先把 < 转成 &lt;，再经 encodeURIComponent → %26lt%3B
    expect(url).toContain("%26lt%3B");
    expect(url).not.toContain("<svg"); // 顶层标签本身已被整体编码
    expect(url).toContain("%3Csvg%20xmlns");
  });

  it("多行正文高度自适应（行数多 → SVG 更高）", () => {
    const short = mindCardDataUrl("t", "one");
    const long = mindCardDataUrl("t", Array.from({ length: 40 }, (_, i) => `line ${i}`).join("\n"));
    const h = (u: string): number => Number(/height="(\d+)"/.exec(decodeURIComponent(u))?.[1] ?? 0);
    expect(h(long)).toBeGreaterThan(h(short));
  });
});
