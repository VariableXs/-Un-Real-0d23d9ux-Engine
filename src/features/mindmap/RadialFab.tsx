import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";

export interface RadialItem {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}

/**
 * 纯几何：把 count 个按钮沿半径 radius 的圆弧均匀展开。
 * angleFrom/angleTo 为数学角度（度，逆时针，0°=右、90°=上），屏幕坐标 y 向下，
 * 因此返回 (cos·r, −sin·r)。count ≤ 1 或半径非正时全部回到圆心。
 */
export function radialPositions(count: number, radius: number, angleFrom = 90, angleTo = 180): { x: number; y: number }[] {
  const pts: { x: number; y: number }[] = [];
  if (!Number.isFinite(count) || !Number.isFinite(radius) || count <= 0 || radius <= 0) return pts;
  const n = Math.max(0, Math.floor(count));
  for (let i = 0; i < n; i++) {
    const t = n <= 1 ? 0 : i / (n - 1);
    const a = ((angleFrom + (angleTo - angleFrom) * t) * Math.PI) / 180;
    pts.push({ x: Math.cos(a) * radius, y: -Math.sin(a) * radius });
  }
  return pts;
}

/**
 * 悬浮圆形径向菜单（常用操作）：默认只露一个 44px 圆形按钮，点击后选项沿
 * 左上弧线展开。容器 pointer-events:none —— 只有按钮本身可点，周围画布的
 * 拖拽平移/滚轮缩放完全不受影响；展开项点击后自动收起。
 */
export function RadialFab(props: { items: RadialItem[]; fabLabel?: string }): React.ReactElement | null {
  const { lang } = useI18n();
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  // 展开态点外面任意处（含画布）收起；不拦截那次交互。
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent): void => {
      const t = e.target as HTMLElement | null;
      if (t && wrapRef.current?.contains(t)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const RADIUS = 78;
  const pos = radialPositions(props.items.length, RADIUS, 90, 180);

  return (
    <div ref={wrapRef} className={`radial-fab${open ? " open" : ""}`} onPointerDown={(e) => e.stopPropagation()}>
      {props.items.map((it, i) => {
        const p = pos[i] ?? { x: 0, y: 0 };
        return (
          <button
            key={it.label}
            type="button"
            className="radial-item"
            aria-label={it.label}
            title={it.label}
            data-tip={it.label}
            disabled={it.disabled}
            style={{
              transform: open ? `translate(${p.x}px, ${p.y}px) scale(1)` : "translate(0,0) scale(.25)",
              opacity: open ? 1 : 0,
              pointerEvents: open ? "auto" : "none",
              transitionDelay: open ? `${i * 22}ms` : "0ms",
            }}
            onClick={() => {
              setOpen(false);
              it.onClick();
            }}
          >
            {it.icon}
          </button>
        );
      })}
      <button
        type="button"
        className="fab-main"
        aria-label={props.fabLabel ?? (lang === "zh" ? "快捷操作" : "Quick actions")}
        title={props.fabLabel ?? (lang === "zh" ? "快捷操作" : "Quick actions")}
        data-tip={props.fabLabel ?? (lang === "zh" ? "快捷操作" : "Quick actions")}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className={`fab-plus${open ? " rot" : ""}`}>+</span>
      </button>
    </div>
  );
}
