import { Node, mergeAttributes } from "@tiptap/core";
import { isSafeMediaSrc } from "../../lib/sanitize";

/** Local-video block node rendered with the native HTML5 player. */
export const VideoNode = Node.create({
  name: "video",
  group: "block",
  atom: true,
  draggable: true,

  addAttributes() {
    return {
      src: { default: null },
    };
  },

  parseHTML() {
    return [{ tag: "video[src]" }];
  },

  renderHTML({ node }) {
    const src = String(node.attrs.src ?? "");
    if (!isSafeMediaSrc(src)) return ["div", { class: "video-block broken" }, "(invalid video source)"];
    return ["video", mergeAttributes({ src, controls: "", preload: "metadata", class: "video-block" })];
  },

  addCommands() {
    return {
      setVideo:
        (src: string) =>
        ({ commands }: { commands: import("@tiptap/core").SingleCommands }) =>
          commands.insertContent({ type: this.name, attrs: { src } }),
    };
  },
});

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    video: {
      setVideo: (src: string) => ReturnType;
    };
  }
}
