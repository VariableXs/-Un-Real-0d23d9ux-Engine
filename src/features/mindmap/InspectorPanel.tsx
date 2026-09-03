import { useEffect, useRef, useState } from "react";
import { ChevronUp, ChevronDown, X } from "lucide-react";
import { useI18n } from "../../i18n";
import type { MindNode, NodeShape } from "../../lib/types";
import { NODE_PRESETS, ShapeGlyphRow } from "./presets";

interface Props {
  node: MindNode;
  onPatch: (patch: Partial<MindNode>) => void;
  onZOrder: (dir: number) => void;
  onClose: () => void;
}

/**
 * Single-frame property inspector. Pinned to the LITERAL screen top-left
 * corner (top:0; left:0) per contract-5 rev 2 — completely decoupled from
 * canvas pan/zoom and from the node's position. Closes on outside click / X,
 * always through the fade-out dismissal protocol.
 */
export function InspectorPanel(props: Props): React.ReactElement {
  const { t, lang } = useI18n();
  const n = props.node;
  const ref = useRef<HTMLDivElement>(null);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);

  /** Fade-out then unmount (parent clears selection). */
  const dismiss = (): void => {
    if (closingRef.current) return;
    closingRef.current = true;
    setClosing(true);
    window.setTimeout(props.onClose, 150);
  };

  useEffect(() => {
    const timer = window.setTimeout(() => {
      window.addEventListener("mousedown", onDown);
    }, 0);
    const onDown = (e: MouseEvent): void => {
      if (!ref.current?.contains(e.target as Node)) dismiss();
    };
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("mousedown", onDown);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [x, setX] = useState(String(Math.round(n.x)));
  const [y, setY] = useState(String(Math.round(n.y)));
  const [w, setW] = useState(String(Math.round(n.width)));
  const [h, setH] = useState(String(Math.round(n.height)));

  useEffect(() => {
    setX(String(Math.round(n.x)));
    setY(String(Math.round(n.y)));
    setW(String(Math.round(n.width)));
    setH(String(Math.round(n.height)));
  }, [n.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (document.activeElement?.tagName !== "INPUT") {
      setX(String(Math.round(n.x)));
      setY(String(Math.round(n.y)));
      setW(String(Math.round(n.width)));
      setH(String(Math.round(n.height)));
    }
  }, [n.x, n.y, n.width, n.height]);

  const num = (v: string): number | null => {
    const f = Number(v);
    return Number.isFinite(f) ? f : null;
  };
  const commit = (setter: (patch: Partial<MindNode>) => void) => ({
    x: () => { const v = num(x); if (v !== null) setter({ x: v }); },
    y: () => { const v = num(y); if (v !== null) setter({ y: v }); },
    w: () => { const v = num(w); if (v !== null && v >= 60) setter({ width: Math.min(v, 900) }); },
    h: () => { const v = num(h); if (v !== null && v >= 30) setter({ height: Math.min(v, 1400) }); },
  });
  const c = commit(props.onPatch);

  return (
    <div ref={ref} className={`inspector card-pop ${closing ? "closing" : ""}`} onPointerDown={(e) => e.stopPropagation()}>
      <div className="row between">
        <strong className="small">{lang === "zh" ? "属性" : "Properties"}</strong>
        <span>
          <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "上移一层" : "Forward"} aria-label="forward" onClick={() => props.onZOrder(1)}><ChevronUp size={12} /></button>
          <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "下移一层" : "Backward"} aria-label="backward" onClick={() => props.onZOrder(-1)}><ChevronDown size={12} /></button>
          <button type="button" className="icon-btn tiny" aria-label="close" onClick={dismiss}><X size={12} /></button>
        </span>
      </div>

      <div className="insp-grid">
        <label>X<input value={x} onChange={(e) => setX(e.target.value)} onBlur={c.x} onKeyDown={(e) => { e.stopPropagation(); if (e.key === "Enter") c.x(); }} /></label>
        <label>Y<input value={y} onChange={(e) => setY(e.target.value)} onBlur={c.y} onKeyDown={(e) => { e.stopPropagation(); if (e.key === "Enter") c.y(); }} /></label>
        <label>{lang === "zh" ? "宽" : "W"}<input value={w} onChange={(e) => setW(e.target.value)} onBlur={c.w} onKeyDown={(e) => { e.stopPropagation(); if (e.key === "Enter") c.w(); }} /></label>
        <label>{lang === "zh" ? "高" : "H"}<input value={h} onChange={(e) => setH(e.target.value)} onBlur={c.h} onKeyDown={(e) => { e.stopPropagation(); if (e.key === "Enter") c.h(); }} /></label>
      </div>

      <label className="insp-row">
        <span>{t("rotate")}</span>
        <input type="range" min={-180} max={180} step={5} value={n.rotation}
          onChange={(e) => props.onPatch({ rotation: Number(e.target.value) })} />
        <b className="small">{Math.round(n.rotation)}°</b>
      </label>

      <label className="insp-row">
        <span>{t("opacity")}</span>
        <input type="range" min={20} max={100} value={Math.round(n.opacity * 100)}
          onChange={(e) => props.onPatch({ opacity: Number(e.target.value) / 100 })} />
        <b className="small">{Math.round(n.opacity * 100)}%</b>
      </label>

      <div className="insp-row">
        <span>{t("shape")}</span>
        <ShapeGlyphRow
          current={n.shape}
          onPick={(s: NodeShape) => {
            if (s === "circle") {
              const d = Math.min(n.width, n.height);
              props.onPatch({ shape: s, width: d, height: d });
            } else props.onPatch({ shape: s });
          }}
        />
      </div>

      <div className="insp-row">
        <span>{lang === "zh" ? "风格" : "Preset"}</span>
        <span className="preset-chips">
          {NODE_PRESETS.map((p) => (
            <button key={p || "none"} type="button"
              className={`chip ${n.preset === p ? "on" : ""}`}
              onClick={() => props.onPatch({ preset: p })}>
              {p === "" ? (lang === "zh" ? "默认" : "None") : p}
            </button>
          ))}
        </span>
      </div>

      <div className="insp-row">
        <span>{t("nodeFontSize")}</span>
        <span className="stepper">
          <button type="button" className="icon-btn tiny" onClick={() => props.onPatch({ fontSize: Math.max(10, n.fontSize - 1) })}>−</button>
          <b>{Math.round(n.fontSize)}</b>
          <button type="button" className="icon-btn tiny" onClick={() => props.onPatch({ fontSize: Math.min(34, n.fontSize + 1) })}>+</button>
        </span>
        <span className="flex-1" />
        <label className="check-line"><input type="checkbox" checked={n.locked} onChange={(e) => props.onPatch({ locked: e.target.checked })} />{t("lock")}</label>
        <label className="check-line"><input type="checkbox" checked={n.hidden} onChange={(e) => props.onPatch({ hidden: e.target.checked })} />{t("hide")}</label>
        <label className="check-line"><input type="checkbox" checked={n.collapsed} onChange={(e) => props.onPatch({ collapsed: e.target.checked })} />{t("collapse")}</label>
      </div>
    </div>
  );
}
