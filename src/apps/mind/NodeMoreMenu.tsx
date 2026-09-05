import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import type { MindNode, NodeShape } from "../../lib/types";

const FILL_SWATCHES = [
  "rgba(13,20,38,0.88)", "rgba(30,42,74,0.92)", "rgba(52,66,102,0.9)",
  "rgba(20,58,58,0.9)", "rgba(70,48,72,0.9)", "rgba(78,60,34,0.9)",
  "rgba(236,240,248,0.95)", "rgba(255,255,255,0.08)",
];
const BORDER_COLORS = ["#5b7bd0", "#7fc8a9", "#e8c26b", "#e07f7f", "#c68fe0", "#8a93a6"];
const SHAPES: NodeShape[] = ["rect", "rounded", "circle", "triangle", "diamond", "pentagon", "hexagon", "heptagon"];

export function shapeLabel(t: (k: string) => string, s: NodeShape): string {
  const map: Record<NodeShape, string> = {
    rect: t("shapeRect"), rounded: t("shapeRounded"), circle: t("shapeCircle"),
    triangle: t("shapeTriangle"), diamond: t("shapeDiamond"), pentagon: t("shapePentagon"),
    hexagon: t("shapeHexagon"), heptagon: t("shapeHeptagon"),
  };
  return map[s];
}

/**
 * Node "more" panel — pinned to the TOP-LEFT of the screen (below the title
 * bar), completely decoupled from canvas pan/zoom and from the node's own
 * position. Closes on outside click / Esc / command execution (parent).
 */
export function NodeMoreMenu(props: {
  node: MindNode;
  onClose: () => void;
  onPatch: (patch: Partial<MindNode>) => void;
  onCreateRecordFromNode: () => void;
  onOpenLinkedRecord: (() => void) | null;
  onDelete: () => void;
}): React.ReactElement {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);
  /** Fade-out then unmount (module-4 dismissal protocol). */
  const dismiss = (): void => {
    if (closingRef.current) return;
    closingRef.current = true;
    setClosing(true);
    window.setTimeout(props.onClose, 150);
  };

  useEffect(() => {
    // Outside-close on BOTH pointerdown (spec channel, fires first) and
    // mousedown (fallback); closingRef dedupes the double trigger.
    const outside = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) dismiss();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    // Global forced-destroy channel: blank-canvas pointer down resets the
    // whole activation state; this panel fades out through the same protocol.
    const onDismissAll = () => dismiss();
    const timer = setTimeout(() => {
      window.addEventListener("pointerdown", outside, true);
      window.addEventListener("mousedown", outside);
    }, 0);
    window.addEventListener("keydown", onKey);
    window.addEventListener("variable:mm-dismiss-all", onDismissAll);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("mousedown", outside);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("variable:mm-dismiss-all", onDismissAll);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const n = props.node;
  return (
    <div
      ref={ref}
      className={`node-menu card-pop pinned-tl ${closing ? "closing" : ""}`}
      // Isolation: interactions inside the panel never bubble into the canvas
      // root's blank-click dismissal logic.
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    >
      <div className="nm-row">
        <span className="dim small">{t("shape")}</span>
        <div className="shape-grid">
          {SHAPES.map((s) => (
            <button
              key={s}
              type="button"
              className={`shape-btn ${n.shape === s ? "active" : ""}`}
              title={shapeLabel(t, s)}
              aria-label={shapeLabel(t, s)}
              onClick={() => {
                if (s === "circle") {
                  const d = Math.min(n.width, n.height);
                  props.onPatch({ shape: s, width: d, height: d });
                } else {
                  props.onPatch({ shape: s });
                }
              }}
            >
              <ShapeGlyph shape={s} />
            </button>
          ))}
        </div>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("fillColor")}</span>
        <span className="swatch-line">
          {FILL_SWATCHES.map((c) => (
            <button key={c} type="button" className={`swatch ${n.fillColor === c ? "sel" : ""}`} style={{ background: c }} onClick={() => props.onPatch({ fillColor: c })} />
          ))}
        </span>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("borderColor")}</span>
        <span className="swatch-line">
          {BORDER_COLORS.map((c) => (
            <button key={c} type="button" className={`swatch ${n.borderColor === c ? "sel" : ""}`} style={{ background: c }} onClick={() => props.onPatch({ borderColor: c })} />
          ))}
          <button type="button" className={`btn tiny ${n.opacity >= 1 ? "" : "ghost"}`} onClick={() => props.onPatch({ opacity: n.opacity >= 1 ? 0.55 : 1 })}>
            {t("opacity")} {Math.round((n.opacity >= 1 ? 0.55 : 1) * 100)}%
          </button>
        </span>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("preset")}</span>
        <span className="preset-chips">
          {["", "tech", "modern", "minimal", "handdrawn", "pixel", "cyber"].map((p) => (
            <button key={p || "none"} type="button"
              className={`chip ${n.preset === p ? "on" : ""}`}
              onClick={() => props.onPatch({ preset: p })}>
              {p === "" ? (t("presetNone")) : p}
            </button>
          ))}
        </span>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("rotate")}</span>
        <input type="range" min={-180} max={180} step={5} value={n.rotation}
          onChange={(e) => props.onPatch({ rotation: Number(e.target.value) })} />
        <b className="small">{Math.round(n.rotation)}°</b>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("nodeFontSize")}</span>
        <span className="stepper">
          <button type="button" className="icon-btn tiny" onClick={() => props.onPatch({ fontSize: Math.max(10, n.fontSize - 1) })}>−</button>
          <b>{Math.round(n.fontSize)}</b>
          <button type="button" className="icon-btn tiny" onClick={() => props.onPatch({ fontSize: Math.min(34, n.fontSize + 1) })}>+</button>
          <label className="check-line">
            <input
              type="checkbox"
              checked={n.shape !== "rect"}
              onChange={(e) => props.onPatch({ shape: e.target.checked ? "rounded" : "rect" })}
            />
            {t("borderOnOff")}
          </label>
        </span>
      </div>
      <div className="nm-row">
        <label className="check-line">
          <input type="checkbox" checked={n.hidden} onChange={(e) => props.onPatch({ hidden: e.target.checked })} />
          {t("hide")}
        </label>
        <label className="check-line">
          <input type="checkbox" checked={n.collapsed} onChange={(e) => props.onPatch({ collapsed: e.target.checked })} />
          {t("collapse")}
        </label>
        <span className="flex-1" />
        <button type="button" className="btn tiny ghost" onClick={() => props.onPatch({ locked: !n.locked })}>
          {n.locked ? "🔓" : "🔒"} {n.locked ? t("unlock") : t("lock")}
        </button>
      </div>
      <div className="nm-row">
        {n.recordId && props.onOpenLinkedRecord && (
          <button type="button" className="btn tiny ghost" onClick={props.onOpenLinkedRecord}>{t("openLinkedRecord")}</button>
        )}
        {!n.recordId && (
          <button type="button" className="btn tiny ghost" onClick={props.onCreateRecordFromNode}>{t("createRecordFromNode")}</button>
        )}
        <span className="flex-1" />
      </div>
      <div className="nm-row danger-zone">
        <button type="button" className="btn tiny danger" onClick={props.onDelete}>
          {t("deleteNode")}
        </button>
      </div>
    </div>
  );
}

export function ShapeGlyph(props: { shape: NodeShape; size?: number }): React.ReactElement {
  const s = props.size ?? 16;
  if (props.shape === "circle") {
    return <svg width={s} height={s} viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" /></svg>;
  }
  const pts = polygonPointsForGlyph(props.shape);
  return (
    <svg width={s} height={s} viewBox="0 0 16 16">
      <polygon points={pts} fill="none" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
    </svg>
  );
}

function polygonPointsForGlyph(shape: NodeShape): string {
  switch (shape) {
    case "rect": return "1.5,3.5 14.5,3.5 14.5,12.5 1.5,12.5";
    case "rounded": return "M4,3.5 h8 a2.5,2.5 0 0 1 2.5,2.5 v4 a2.5,2.5 0 0 1 -2.5,2.5 h-8 a2.5,2.5 0 0 1 -2.5,-2.5 v-4 a2.5,2.5 0 0 1 2.5,-2.5 z";
    case "triangle": return "8,2 14.5,13.5 1.5,13.5";
    case "diamond": return "8,1.5 14.5,8 8,14.5 1.5,8";
    default: {
      const sides = shape === "pentagon" ? 5 : shape === "hexagon" ? 6 : 7;
      const pts: string[] = [];
      for (let i = 0; i < sides; i++) {
        const a = -Math.PI / 2 + (i * 2 * Math.PI) / sides;
        pts.push(`${(8 + 6.4 * Math.cos(a)).toFixed(2)},${(8 + 6.4 * Math.sin(a)).toFixed(2)}`);
      }
      return pts.join(" ");
    }
  }
}
