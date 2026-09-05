import type { MindNode, NodeShape } from "../../lib/types";
import { useI18n } from "../../i18n";

interface Ops {
  align: (dir: "l" | "cx" | "r" | "t" | "cy" | "b") => void;
  distribute: (axis: "h" | "v") => void;
  autoLayout: () => void;
  uniform: (what: "size" | "shape" | "color" | "font" | "border") => void;
  uniformPreset: () => void;
  chainConnect: () => void;
  exportSelected: () => void;
  copy: () => void;
  deleteSel: () => void;
}

/** Floating bar shown when multiple nodes are selected. */
export function SelectionOpsBar(props: {
  count: number;
  ops: Ops;
}): React.ReactElement | null {
  const { t } = useI18n();
  if (props.count === 0) return null;
  const b = (label: string, fn: () => void, key?: string) => (
    <button key={key ?? label} type="button" className="ops-btn" onClick={fn}>{label}</button>
  );
  return (
    <div className="selection-ops card-pop" role="toolbar">
      <span className="dim small">{t("selectionOps", { n: props.count })}</span>
      <span className="tb-sep" />
      {b("⇤", () => props.ops.align("l"), "al")}
      {b("⇔", () => props.ops.align("cx"), "ac")}
      {b("⇥", () => props.ops.align("r"), "ar")}
      {b("⇡", () => props.ops.align("t"), "at")}
      {b("⇕", () => props.ops.align("cy"), "ay")}
      {b("⇣", () => props.ops.align("b"), "ab")}
      {b(t("distH"), () => props.ops.distribute("h"))}
      {b(t("distV"), () => props.ops.distribute("v"))}
      {b(t("autoLayout"), () => props.ops.autoLayout())}
      {b(t("uniformSize"), () => props.ops.uniform("size"))}
      {b(t("uniformShape"), () => props.ops.uniform("shape"))}
      {b(t("uniformColor"), () => props.ops.uniform("color"))}
      {b(t("uniformFont"), () => props.ops.uniform("font"))}
      {b(t("uniformBorder"), () => props.ops.uniform("border"))}
      {b(t("preset"), () => props.ops.uniformPreset())}
      {b(t("connectSelected"), () => props.ops.chainConnect())}
      {b(t("copyNode"), () => props.ops.copy())}
      {b(t("exportSelected"), () => props.ops.exportSelected())}
      <button type="button" className="ops-btn danger" onClick={() => props.ops.deleteSel()}>{t("delete")}</button>
    </div>
  );
}

// Re-export helper used by parent to compute uniform targets.
export function firstOf(nodes: Map<string, MindNode>, ids: Set<string>): MindNode | null {
  for (const id of ids) {
    const n = nodes.get(id);
    if (n) return n;
  }
  return null;
}

export type { NodeShape };
