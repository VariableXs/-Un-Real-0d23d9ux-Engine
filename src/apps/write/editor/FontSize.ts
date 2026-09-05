import { Extension } from "@tiptap/core";
import "@tiptap/extension-text-style";

/** Font size via the textStyle mark (px). */
export const FontSize = Extension.create({
  name: "fontSize",
  addOptions() {
    return { types: ["textStyle"] };
  },
  addGlobalAttributes() {
    return [
      {
        types: this.options.types,
        attributes: {
          fontSize: {
            default: null,
            parseHTML: (element) => element.style.fontSize?.replace(/px$/, "") ?? null,
            renderHTML: (attributes) => {
              if (!attributes.fontSize) return {};
              const n = Number(attributes.fontSize);
              if (!Number.isFinite(n) || n < 8 || n > 72) return {};
              return { style: `font-size: ${n}px` };
            },
          },
        },
      },
    ];
  },
  addCommands() {
    return {
      setFontSize:
        (size: number | null) =>
        ({ chain }: { chain: () => import("@tiptap/core").ChainedCommands }) =>
          chain().setMark("textStyle", { fontSize: size === null ? null : String(size) }).run(),
      unsetFontSize:
        () =>
        ({ chain }: { chain: () => import("@tiptap/core").ChainedCommands }) =>
          chain().setMark("textStyle", { fontSize: null }).run(),
    };
  },
});

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    fontSize: {
      setFontSize: (size: number | null) => ReturnType;
      unsetFontSize: () => ReturnType;
    };
  }
}
