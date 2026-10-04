import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Pencil, Copy, Trash2, Lock, LockOpen, Palette, MoreHorizontal } from "lucide-react";

interface Props {
  hasSel: boolean;
  count: number;
  locked?: boolean;
  onEdit: () => void;
  onCopy: () => void;
  onDelete: () => void;
  onLock: () => void;
  onStyle: () => void;
  onMore: () => void;
}

/**
 * Hidden bottom dock (spec 5.1): reveals when the pointer enters the bottom
 * edge zone, hides again 1s after the pointer leaves. Anchored to the viewport,
 * glass background; buttons stop propagation so the canvas never reacts.
 */
export function DockBar(props: Props): React.ReactElement {
  const { lang } = useI18n();
  const [shown, setShown] = useState(false);
  const hideTimer = useRef<number>(0);
  const zoneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onMove = (e: MouseEvent): void => {
      const nearBottom = window.innerHeight - e.clientY <= 22;
      if (nearBottom && !shown) {
        clearTimeout(hideTimer.current);
        setShown(true);
      } else if (!nearBottom && shown) {
        const target = zoneRef.current;
        const over = !!target && (() => {
          const r = target.getBoundingClientRect();
          return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top - 8 && e.clientY <= r.bottom + 8;
        })();
        if (!over) {
          clearTimeout(hideTimer.current);
          hideTimer.current = window.setTimeout(() => setShown(false), 1000);
        }
      }
    };
    window.addEventListener("mousemove", onMove, { passive: true });
    return () => {
      window.removeEventListener("mousemove", onMove);
      clearTimeout(hideTimer.current);
    };
  }, [shown]);

  const btn = (label: string, icon: React.ReactNode, fn: () => void, disabled?: boolean) => (
    <button
      type="button"
      className="dock-btn"
      title={label}
      aria-label={label}
      disabled={disabled}
      onPointerDown={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.preventDefault()}
      onClick={fn}
    >
      {icon}
    </button>
  );

  return (
    <div ref={zoneRef} className={`dock-wrap ${shown ? "shown" : ""}`} onPointerDown={(e) => e.stopPropagation()}>
      <div className="dock-bar card-pop">
        {btn(lang !== "en" ? "编辑" : "Edit", <Pencil size={14} />, props.onEdit, !props.hasSel)}
        {btn(lang !== "en" ? `复制 (${props.count})` : `Copy (${props.count})`, <Copy size={14} />, props.onCopy, !props.hasSel)}
        {btn(props.locked ? (lang !== "en" ? "解锁" : "Unlock") : lang !== "en" ? "锁定" : "Lock",
          props.locked ? <LockOpen size={14} /> : <Lock size={14} />, props.onLock, !props.hasSel)}
        {btn(lang !== "en" ? "风格" : "Style", <Palette size={14} />, props.onStyle, props.count !== 1)}
        {btn(lang !== "en" ? "更多" : "More", <MoreHorizontal size={14} />, props.onMore, props.count !== 1)}
        <span className="dock-sep" />
        {btn(lang !== "en" ? "删除" : "Delete", <Trash2 size={14} />, props.onDelete, !props.hasSel)}
      </div>
    </div>
  );
}
