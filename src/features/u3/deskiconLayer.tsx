/**
 * I 通用域 · AI-U3 桌面接线件（F501/F502/F503/F539/F549 的 UI 装配层）。
 *
 * 职责（与 J1 J1Runtime 的桌面装配同哲学——逻辑模块已在 deskicons.ts，
 * 本文件把它们装配成桌面可消费的件）：
 * - <IconLabel>：桌面图标文字渲染件（双层渲染 + 两行封顶 + 选中底衬 +
 *   壁纸亮度选字色）——桌面图标列表按 props 传入壁纸亮度即可获得判据全效；
 * - resnapDesktopIcons()：F503 密度变化重排命令出口（派发 vx-u3-resnap，
 *   桌面图标容器消费——携带新格距与重排结果，桌面只做落位不重算）;
 * - <ClockHoverTip>：F549 时钟悬停完整日期 Tooltip（500ms 延迟 F205 同源，
 *   三要素一行；与 F078 日历飞出分工——本件只读不可点）；
 * - layoutLockGuard()：F539 布局锁定拖拽裁决挂点（桌面拖拽启动前查询，
 *   拒绝时派发抖动反馈事件 vx-u3-shake）。
 *
 * 诚实边界：桌面图标容器本体归桌面领地（desktop-design 等），本文件只提供
 * 「可嵌入件 + 事件出口」——不直接改写桌面状态（不越权，章十四开放扩展）。
 */

import React, { useEffect, useRef, useState } from "react";
import { u3Store } from "./u3store";
import {
  iconReadConfig, wrapIconLabel, pickIconTextColor, SELECT_PILL_BG, iconTextLayers,
  effectiveGrid, resnapToGrid, gridDensityConfig,
} from "./deskicons";
import { hoverDateLine, clockHoverConfig } from "./clockcal";
import { layoutDragVerdict, layoutLockConfig, LOCK_SHAKE_MS } from "./winkeys";
import { dispatchU3Action } from "./actions";

/* ------------------------------- F501/F502 IconLabel ------------------------------- */

export interface IconLabelProps {
  name: string;
  /** 壁纸平均感知亮度 [0,1]（桌面壁纸分析结果——auto 模式的选字依据）。 */
  wallpaperLuma: number;
  /** F297 壁纸暗色压暗是否启用（联动一致性——选字与所见一致）。 */
  darkOverlay?: boolean;
  selected?: boolean;
  /** 图标文字尺寸档（桌面密度换算，默认 12px 桌面基线）。 */
  fontSizePx?: number;
}

/** 桌面图标文字渲染件（F501 双层渲染 + F502 两行封顶 + 选中胶囊底衬）。 */
export function IconLabel(props: IconLabelProps): React.ReactElement {
  const cfg = iconReadConfig();
  // 字色裁决：auto 按壁纸亮度（含 F297 压暗联动）；light/dark 固定档直译
  const picked: "light" | "dark" =
    cfg.mode === "auto"
      ? pickIconTextColor(props.wallpaperLuma, props.darkOverlay ?? false)
      : cfg.mode === "light"
        ? "light"
        : "dark";
  const layers = iconTextLayers(picked);
  const wrap = wrapIconLabel(props.name);
  const align = u3Store.getWith("iconWrap", "centerAlign", true) ? "center" : "initial";

  return (
    <span
      className={"u3-icon-label-demo" + (props.selected ? " is-selected" : "")}
      style={{
        ...(cfg.selectedPill && props.selected ? { background: SELECT_PILL_BG } : null),
        color: layers.color,
        textShadow: layers.textShadow,
        textAlign: align as "center" | "initial",
        fontSize: props.fontSizePx ?? 12,
        display: "block",
      }}
      // 全文三路可达之第一路：Tooltip（F205 体系；重命名/属性两路由桌面右键菜单承接）
      title={wrap.truncated ? props.name : undefined}
    >
      {wrap.lines[0]}
      {wrap.lines[1] && (
        <>
          <br />
          {wrap.lines[1]}
        </>
      )}
    </span>
  );
}

/* ------------------------------- F503 网格重排出口 ------------------------------- */

export interface ResnapCommand {
  /** 图标当前点位（桌面坐标）。 */
  points: Array<{ x: number; y: number }>;
  /** 重排前格距（换档前生效值）。 */
  from: { colPx: number; rowPx: number };
}

/**
 * F503 密度变化重排命令：算好重排结果后派发 vx-u3-resnap（桌面图标容器
 * 消费——直接落位，不再二次计算）。返回结果供调用方（面板）即时预览。
 */
export function resnapDesktopIcons(cmd: ResnapCommand): Array<{ x: number; y: number }> {
  const to = effectiveGrid(gridDensityConfig());
  const snapped = resnapToGrid(cmd.points, cmd.from, to);
  if (typeof window !== "undefined" && typeof CustomEvent === "function") {
    window.dispatchEvent(new CustomEvent("vx-u3-resnap", { detail: { to, points: snapped } }));
  }
  void dispatchU3Action("desktop.resnap", "runtime", { to, count: snapped.length });
  return snapped;
}

/* ------------------------------- F539 布局锁定裁决挂点 ------------------------------- */

export interface LayoutGuardVerdict {
  allowed: boolean;
  /** 拒绝时应触发的抖动时长（ms；0=无）。 */
  shakeMs: number;
  statusbar: string;
}

/** 桌面图标拖拽启动前查询（拒绝时派发 vx-u3-shake 抖动反馈——温和拒绝）。 */
export function layoutLockGuard(): LayoutGuardVerdict {
  const cfg = layoutLockConfig();
  const v = layoutDragVerdict(cfg.locked);
  if (!v.allowed && typeof window !== "undefined" && typeof CustomEvent === "function") {
    window.dispatchEvent(new CustomEvent("vx-u3-shake", { detail: { ms: cfg.shakeMs, message: v.statusbar } }));
  }
  return { allowed: v.allowed, shakeMs: v.allowed ? 0 : cfg.shakeMs, statusbar: v.statusbar };
}

/** 抖动反馈常量出口（桌面消费同一份 120ms——一处一事实）。 */
export const DESK_SHAKE_MS = LOCK_SHAKE_MS;

/* ------------------------------- F549 时钟悬停 Tooltip ------------------------------- */

/** 悬停延迟（F205 全局 500ms——clockcal 同源常量，此处引用防漂移）。 */
const HOVER_DELAY_FALLBACK = 500;

/** 任务栏时钟悬停 Tooltip（三要素一行；只读不可点——与 F078 分工）。 */
export function ClockHoverTip(): React.ReactElement {
  const [visible, setVisible] = useState(false);
  const [line, setLine] = useState("");
  const timer = useRef<number | null>(null);
  const cfg = clockHoverConfig();

  const enter = () => {
    if (timer.current !== null) return;
    timer.current = window.setTimeout(() => {
      const now = new Date();
      setLine(hoverDateLine(now.getFullYear(), now.getMonth() + 1, now.getDate(), cfg.showLunar));
      setVisible(true);
    }, cfg.delayMs || HOVER_DELAY_FALLBACK);
  };
  const leave = () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    setVisible(false);
  };
  useEffect(() => () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
  }, []);

  return (
    <span
      onMouseEnter={enter}
      onMouseLeave={leave}
      onFocus={enter}
      onBlur={leave}
      tabIndex={0}
      style={{ position: "relative", display: "inline-block", cursor: "default" }}
      aria-label="完整日期"
    >
      <slot />
      {visible && (
        <span
          role="tooltip"
          style={{
            position: "absolute",
            bottom: "calc(100% + 6px)",
            left: "50%",
            transform: "translateX(-50%)",
            whiteSpace: "nowrap",
            padding: "5px 10px",
            borderRadius: 8,
            fontSize: 12,
            background: "var(--vx-surface-3, #2a2f3a)",
            color: "var(--vx-text, #e8eaed)",
            border: "1px solid var(--vx-border, rgba(255,255,255,0.1))",
            boxShadow: "0 4px 16px rgba(0,0,0,0.35)",
            zIndex: 2100,
            pointerEvents: "none",
          }}
        >
          {line}
        </span>
      )}
    </span>
  );
}
