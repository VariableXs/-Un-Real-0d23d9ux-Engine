import type { VwmWin, VwmRect } from "./vwm";
import { useState } from "react";
import { useI18n } from "../../i18n";
import { appAccent } from "../../components/AppGlyphs";
import { isTpApp } from "./vwm";
import { ipc } from "../../lib/ipc";
import {
  closeVwmWin,
  minimizeVwmWin,
  moveVwmWin,
  pointerFocusVwm,
  resizeVwmWin,
  setVwmSnapPreview,
  settleVwmWin,
  snapVwmRect,
  snapZoneForVwm,
  toggleMaxVwmWin,
  unmaxVwmTo,
  vwmStore,
  vwmWindowTitle,
} from "./vwm";

/**
 * 虚拟窗口框架：
 * - 标题栏拖拽移动（最大化态拖动 → Windows 习惯还原跟随）+ 双击最大化/还原
 * - 拖到屏幕边缘 → 贴靠预览（左右半屏 / 四角 1/4 / 顶部最大化），松手应用
 * - 八向边缘缩放（min 820×540，与既有系统窗口一致）
 * - 右上角 Mac 风格红绿灯：🟢 退出（关闭）/ 🟡 全屏（最大化-还原）/ 🔴 最小化
 *
 * 壳层只做几何与层级调度；children（软件视图）零触碰。
 */

const MIN_W = 820;
const MIN_H = 540;
const EDGE = 12; // 贴靠判定边距（CSS 像素）

type SnapDir = "left" | "right" | "up" | "tl" | "tr" | "bl" | "br";

function zoneFromPointer(px: number, py: number, wa: VwmRect): SnapDir | null {
  const nearTop = py <= wa.y + EDGE;
  const nearLeft = px <= wa.x + EDGE;
  const nearRight = px >= wa.x + wa.w - EDGE;
  const nearBottom = py >= wa.y + wa.h - EDGE;
  if (nearTop && nearLeft) return "tl";
  if (nearTop && nearRight) return "tr";
  if (nearBottom && nearLeft) return "bl";
  if (nearBottom && nearRight) return "br";
  if (nearTop) return "up";
  if (nearLeft) return "left";
  if (nearRight) return "right";
  return null;
}

const APP_TITLES = vwmWindowTitle;

export function VirtualWindowFrame(props: {
  win: VwmWin;
  focused: boolean;
  zIndex: number;
  /** 批次E-14 动效：关闭仪式中（缩小淡出）/ 最小化飞行中（飞向任务栏）。 */
  closing?: boolean;
  flying?: boolean;
  children: React.ReactNode;
}): React.ReactElement {
  const { t } = useI18n();
  const win = props.win;
  const title = APP_TITLES(win.app);
  // 拖拽中半透明 + 抬起阴影；贴靠/最大化时平滑滑入
  const [dragging, setDragging] = useState(false);
  const [snapping, setSnapping] = useState(false);
  const playSnap = (): void => {
    setSnapping(true);
    window.setTimeout(() => setSnapping(false), 240);
  };

  // ---------- 标题栏拖拽 ----------
  const onTitlePointerDown = (e: React.PointerEvent): void => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest(".vwm-lights")) return;
    pointerFocusVwm(win.id);
    const s0 = vwmStore.getState();
    const wa = s0.workArea;
    let offX = e.clientX - win.x;
    const offY = e.clientY - win.y;
    if (win.state === "max") {
      // Windows 习惯：拖动即还原，指针保持在标题栏内的相对横向位置
      const r = win.restore ?? {
        x: wa.x + 60,
        y: wa.y + 40,
        w: Math.min(1180, wa.w - 120),
        h: Math.min(760, wa.h - 80),
      };
      offX = Math.round(((e.clientX - wa.x) / Math.max(1, wa.w)) * r.w);
      unmaxVwmTo(win.id, { x: e.clientX - offX, y: e.clientY - offY, w: r.w, h: r.h });
    }
    const drag = { offX, offY };
    let pendingZone: SnapDir | null = null;
    let lastZone: SnapDir | null = null;
    setDragging(true);

    const onMove = (ev: PointerEvent): void => {
      const s = vwmStore.getState();
      const w = s.wins.find((x) => x.id === win.id);
      if (!w) return void cleanup();
      const nx = ev.clientX - drag.offX;
      const ny = Math.max(s.workArea.y - 8, ev.clientY - drag.offY);
      // 保守钳制：至少保留 120px 可见，避免窗口被完全拖出屏幕
      const cx = Math.min(
        Math.max(nx, s.workArea.x - w.w + 120),
        Math.max(s.workArea.x, s.workArea.x + s.workArea.w - 120),
      );
      moveVwmWin(win.id, cx, ny);
      pendingZone = zoneFromPointer(ev.clientX, ev.clientY, s.workArea);
      if (pendingZone !== lastZone) {
        lastZone = pendingZone;
        setVwmSnapPreview(pendingZone ? snapZoneForVwm(pendingZone, s.workArea) : null);
      }
    };
    const cleanup = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    const onUp = (): void => {
      if (pendingZone) {
        playSnap();
        if (pendingZone === "up") toggleMaxVwmWin(win.id);
        else snapVwmRect(win.id, snapZoneForVwm(pendingZone, vwmStore.getState().workArea));
      }
      settleVwmWin(win.id);
      setVwmSnapPreview(null);
      setDragging(false);
      cleanup();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  // ---------- 八向缩放 ----------
  const beginResize = (e: React.PointerEvent, dir: string): void => {
    if (e.button !== 0 || win.state === "max") return;
    e.stopPropagation();
    pointerFocusVwm(win.id);
    const start = { x: e.clientX, y: e.clientY, wx: win.x, wy: win.y, ww: win.w, wh: win.h };
    const onMove = (ev: PointerEvent): void => {
      const dx = ev.clientX - start.x;
      const dy = ev.clientY - start.y;
      let { wx, wy, ww, wh } = start;
      if (dir.includes("e")) ww = Math.max(MIN_W, start.ww + dx);
      if (dir.includes("s")) wh = Math.max(MIN_H, start.wh + dy);
      if (dir.includes("w")) {
        ww = Math.max(MIN_W, start.ww - dx);
        wx = start.wx + (start.ww - ww);
      }
      if (dir.includes("n")) {
        wh = Math.max(MIN_H, start.wh - dy);
        wy = start.wy + (start.wh - wh);
      }
      resizeVwmWin(win.id, { x: wx, y: wy, w: ww, h: wh });
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      settleVwmWin(win.id);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const maximized = win.state === "max";

  return (
    <div
      className={`vwm-window${props.focused ? " focused" : ""}${win.minimized && !props.flying ? " minimized" : ""}${maximized ? " maximized" : ""}${dragging ? " dragging" : ""}${snapping ? " snapping" : ""}${props.closing ? " closing" : ""}${props.flying ? " flying" : ""}`}
      style={{ left: win.x, top: win.y, width: win.w, height: win.h, zIndex: props.zIndex }}
      onPointerDown={() => {
        pointerFocusVwm(win.id);
        if (isTpApp(win.app)) void ipc.embedFocus().catch(() => {});
      }}
      role="dialog"
      aria-label={title}
    >
      <div
        className="vwm-titlebar"
        onPointerDown={onTitlePointerDown}
        onDoubleClick={() => { playSnap(); toggleMaxVwmWin(win.id); }}
      >
        <span className="vwm-app-dot" aria-hidden style={{ background: appAccent(win.app) }} />
        <span className="vwm-title">{title}</span>
        <span className="vwm-titlebar-space" />
        {/* 右上角 Mac 风格红绿灯（需求指定顺序：左绿 中黄 右红）：
            🟢 退出（关闭窗口）/ 🟡 全屏（最大化-还原）/ 🔴 最小化 */}
        <div className="vwm-lights" role="group" aria-label="Window controls">
          <button
            type="button"
            className="win-btn"
            aria-label={t("close")}
            title={t("close")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => {
              if (isTpApp(win.app)) void ipc.embedClose().catch(() => {});
              closeVwmWin(win.id);
            }}
          >
            <span className="win-dot green" />
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label={maximized ? t("restore") : t("maximize")}
            title={maximized ? t("restore") : t("maximize")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => { playSnap(); toggleMaxVwmWin(win.id); }}
          >
            <span className="win-dot yellow" />
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label={t("minimize")}
            title={t("minimize")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => minimizeVwmWin(win.id)}
          >
            <span className="win-dot red" />
          </button>
        </div>
      </div>

      <div className="vwm-content">{props.children}</div>

      {!maximized && (
        <>
          {(["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const).map((dir) => (
            <div key={dir} className={`vwm-rz ${dir}`} data-dir={dir} onPointerDown={(e) => beginResize(e, dir)} />
          ))}
        </>
      )}
    </div>
  );
}
