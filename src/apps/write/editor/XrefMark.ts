import { Mark, mergeAttributes } from "@tiptap/core";

/**
 * 批次C（规格 5.7.3）：跨软件引用锚点 Mark。
 * 仅匹配 xref: 协议链接（xref:<kind>/<id>/<ver>），随文档 HTML 持久化。
 * 普通超链接不在支持范围（保持最小内核、零新依赖）。
 */
export const XrefMark = Mark.create({
  name: "xrefRef",
  inclusive: false,

  parseHTML() {
    return [{ tag: 'a[href^="xref:"]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["a", mergeAttributes({ class: "xref-link" }, HTMLAttributes), 0];
  },
});
