import type { NodeShape } from "../../lib/types";

export const NODE_PRESETS = ["", "tech", "modern", "minimal", "handdrawn", "pixel", "cyber"] as const;
export type NodePreset = (typeof NODE_PRESETS)[number];

/** Compact shape picker row shared by the inspector and node menu. */
export function ShapeGlyphRow(props: {
  current: NodeShape;
  onPick: (s: NodeShape) => void;
}): React.ReactElement {
  const shapes: NodeShape[] = ["rect", "rounded", "circle", "triangle", "diamond", "pentagon", "hexagon", "heptagon"];
  return (
    <span className="shape-grid">
      {shapes.map((s) => (
        <button
          key={s}
          type="button"
          className={`shape-btn ${props.current === s ? "active" : ""}`}
          onClick={() => props.onPick(s)}
        >
          <PresetGlyph shape={s} />
        </button>
      ))}
    </span>
  );
}

function PresetGlyph(props: { shape: NodeShape }): React.ReactElement {
  const pts = (p: string): string => p;
  if (props.shape === "circle") {
    return <svg width="15" height="15" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" /></svg>;
  }
  const poly: Record<string, string> = {
    rect: pts("1.5,3.5 14.5,3.5 14.5,12.5 1.5,12.5"),
    triangle: pts("8,2 14.5,13.5 1.5,13.5"),
    diamond: pts("8,1.5 14.5,8 8,14.5 1.5,8"),
    pentagon: pts("8,1.6 14.3,6.2 11.9,13.4 4.1,13.4 1.7,6.2"),
    hexagon: pts("11.5,2 14.5,8 11.5,14 4.5,14 1.5,8 4.5,2"),
    heptagon: pts("10.7,1.9 14.3,6.6 13.2,12.1 8,14.4 2.8,12.1 1.7,6.6 5.3,1.9"),
  };
  if (props.shape === "rounded") {
    return (
      <svg width="15" height="15" viewBox="0 0 16 16">
        <rect x="1.5" y="3.5" width="13" height="9" rx="3" fill="none" stroke="currentColor" />
      </svg>
    );
  }
  return (
    <svg width="15" height="15" viewBox="0 0 16 16">
      <polygon points={poly[props.shape] ?? poly["rect"]} fill="none" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
    </svg>
  );
}
