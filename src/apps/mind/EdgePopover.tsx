import { useEffect, useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import type { MindEdge } from "../../lib/types";

const EDGE_COLORS = ["#7f9bd9", "#7fc8a9", "#e8c26b", "#e07f7f", "#c68fe0", "#8a93a6"];

/**
 * Edge property popover (context menu on an edge). Pinned to the LITERAL
 * screen top-left corner (top:0; left:0), fully decoupled from the canvas and
 * from where the menu was opened (contract-5 rev 2). Closes through the
 * fade-out dismissal protocol.
 */
export function EdgePopover(props: {
  edge: MindEdge;
  onClose: () => void;
  onPatch: (patch: Partial<MindEdge>) => void;
  onDelete: () => void;
}): React.ReactElement {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const [label, setLabel] = useState(props.edge.label);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);
  const dismiss = (): void => {
    if (closingRef.current) return;
    closingRef.current = true;
    setClosing(true);
    window.setTimeout(props.onClose, 150);
  };

  useEffect(() => setLabel(props.edge.label), [props.edge.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    // Outside-close on BOTH pointerdown (spec channel) and mousedown
    // (fallback); closingRef dedupes the double trigger.
    const outside = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) dismiss();
    };
    // Global forced-destroy channel (blank-canvas pointer down).
    const onDismissAll = () => dismiss();
    const timer = setTimeout(() => {
      window.addEventListener("pointerdown", outside, true);
      window.addEventListener("mousedown", outside);
    }, 0);
    window.addEventListener("variable:mm-dismiss-all", onDismissAll);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("mousedown", outside);
      window.removeEventListener("variable:mm-dismiss-all", onDismissAll);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const e = props.edge;
  return (
    <div
      ref={ref}
      className={`edge-pop card-pop pinned-tl ${closing ? "closing" : ""}`}
      onPointerDown={(ev) => ev.stopPropagation()}
      onClick={(ev) => ev.stopPropagation()}
    >
      <div className="nm-row">
        <span className="dim small">{t("edgeDirection")}</span>
        <div className="seg">
          {(["forward", "both", "none"] as const).map((d) => (
            <button key={d} type="button" className={e.direction === d ? "on" : ""} onClick={() => props.onPatch({ direction: d })}>
              {d === "forward" ? "→" : d === "both" ? "↔" : "—"}
            </button>
          ))}
        </div>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("lineStyle")}</span>
        <div className="seg">
          {(["solid", "dashed", "dotted"] as const).map((s) => (
            <button key={s} type="button" className={e.lineStyle === s ? "on" : ""} onClick={() => props.onPatch({ lineStyle: s })}>
              {s === "solid" ? t("lsSolid") : s === "dashed" ? t("lsDashed") : t("lsDotted")}
            </button>
          ))}
        </div>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("pathStyle")}</span>
        <div className="seg">
          {(["curve", "straight", "ortho"] as const).map((p) => (
            <button key={p} type="button" className={e.pathStyle === p ? "on" : ""} onClick={() => props.onPatch({ pathStyle: p })}>
              {p === "curve" ? t("psCurve") : p === "straight" ? t("psStraight") : t("psOrtho")}
            </button>
          ))}
        </div>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("color")}</span>
        <span className="swatch-line">
          {EDGE_COLORS.map((c) => (
            <button key={c} type="button" className={`swatch ${e.color === c ? "sel" : ""}`} style={{ background: c }} onClick={() => props.onPatch({ color: c })} />
          ))}
        </span>
      </div>
      <div className="nm-row">
        <span className="dim small">{t("edgeLabel")}</span>
        <input
          className="text-input tiny"
          value={label}
          maxLength={60}
          onChange={(ev) => setLabel(ev.target.value)}
          onKeyDown={(ev) => ev.stopPropagation()}
          onBlur={() => label !== e.label && props.onPatch({ label })}
        />
      </div>
      <div className="nm-row">
        <label className="check-line">
          <input
            type="checkbox"
            checked={e.animated}
            onChange={(ev) => props.onPatch({ animated: ev.target.checked })}
          />
          {t("animatedEdge")}
        </label>
        <label className="check-line">
          <input
            type="checkbox"
            checked={e.glow}
            onChange={(ev) => props.onPatch({ glow: ev.target.checked })}
          />
          {t("edgeGlow")}
        </label>
        <span className="flex-1" />
        <button type="button" className="btn tiny danger" onClick={props.onDelete}>
          <Trash2 size={12} /> {t("deleteEdge")}
        </button>
      </div>
    </div>
  );
}
