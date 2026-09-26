/**
 * J 鼠标域 · AI-J1 运行时（React 薄壳 · v3 重构）。
 *
 * v3 职责收缩：逻辑内核全部迁往 windowRuntime.ts（窗口无关、框架无关）——
 * 本文件只剩三件事：
 * 1. 桌面窗全量层（J1Runtime）：副本渲染（F620 衬底/F608 磁吸视觉）、锚标、
 *    墨迹、画中实时墨迹、F614 建档气泡、未接线动作的一次性显性提示；
 * 2. 软件窗口层（J1AppWindowLayer）：write/mind/code/fate 四窗的锚标/墨迹层
 *    ——headless 内核 + 轻渲染（F604/F617 在软件窗口真实生效）；
 * 3. 作用域声明：documentElement 的 data-app-id/data-app-class——F605 应用
 *    覆盖、F616 应用档案、F615 侧键作用域的真实挂点（此前全系统无人声明，
 *    解析链永远落到默认档——v3 接通）。
 *
 * 内建动作装配（actions.ts installBuiltinHandlers）：桌面窗与软件窗各自装配
 * Tauri 窗口管理（minimize/toggle-max）+ 编辑三兄弟 + 声明式 nav.up + 事件化
 * file.new/close-tab/sys-action。nav.back/forward、view.refresh 需要各窗口的
 * 历史栈/视图消费者（explorer 标签页历史等）——v3 不越权代接，未接线动作走
 * 显性登记（遥测 no-feedback + 每动作一次性提示），诚实呈现接线边界。
 */

import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createWindowRuntime } from "./windowRuntime";
import { TRAIL_FADE_MS } from "./gestures";
import { composeOverlay } from "./overlay";
import type { PointerOverlayConfig } from "./overlay";
import { j1Store } from "./j1store";
import { installBuiltinHandlers } from "./actions";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
// 锚标/墨迹/副本层样式（全窗口挂载层需要——不能只随设置面板加载）。
import "../../styles/mouse-j1.css";

/* ------------------------------- 指针副本层 ------------------------------- */

/** 指针箭头 1:1 复刻（标准箭头轮廓，viewBox 32 高精度——4K 放大不糊）。 */
const ARROW_PATH = "M2 2 L2 24.5 L8.2 19.6 L12.2 28.6 L15.8 27 L11.9 18.3 L19.4 17.6 Z";

function PointerReplica(props: { x: number; y: number; filter: string; shadow: string }): React.ReactElement {
  return (
    <div
      aria-hidden
      style={{
        position: "fixed",
        left: 0,
        top: 0,
        transform: `translate(${props.x}px, ${props.y}px)`,
        transformOrigin: "2px 2px",
        pointerEvents: "none",
        zIndex: 2147483000, // F335 优先平面（顶层，浮层之上）
        filter: props.filter || undefined,
        willChange: "transform",
      }}
    >
      <svg width={32} height={32} viewBox="0 0 32 32" style={{ display: "block", filter: props.shadow || undefined }}>
        <path d={ARROW_PATH} fill="var(--vx-pointer-core, #ffffff)" stroke="var(--vx-pointer-line, #1b1b1b)" strokeWidth={1} />
      </svg>
    </div>
  );
}

function AnchorMark(props: { x: number; y: number }): React.ReactElement {
  return <div aria-hidden className="j1-autoscroll-anchor" style={{ left: props.x, top: props.y }} />;
}

function InkLayer(props: { pts: { x: number; y: number }[]; live?: boolean }): React.ReactElement | null {
  if (props.pts.length < 2) return null;
  return (
    <svg aria-hidden className="j1-gesture-ink" style={{ position: "fixed", inset: 0, width: "100%", height: "100%", pointerEvents: "none", zIndex: 2147483001 }}>
      <polyline
        points={props.pts.map((p) => `${p.x},${p.y}`).join(" ")}
        fill="none"
        stroke="var(--vx-ink, var(--vx-accent, #4f7cff))"
        strokeWidth={props.live ? 2 : 2.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity={props.live ? 0.45 : 0.9}
      />
    </svg>
  );
}

/* ------------------------------- 装配辅助 ------------------------------- */

/** Tauri 窗口句柄（window.minimize/toggle-max 的真实落点；浏览器 dev 返回 undefined→事件降级）。 */
function tauriWindowHandle(): { minimize: () => Promise<void>; toggleMaximize: () => Promise<void> } | undefined {
  if (!isTauriRuntime()) return undefined;
  try {
    const w = getCurrentWindow();
    return { minimize: () => w.minimize(), toggleMaximize: () => w.toggleMaximize() };
  } catch {
    return undefined;
  }
}

/** F620 衬底三件套（副本渲染参数——与 j1store overlay 节同源）。 */
function useOverlayStyle(): { cssFilter: string; boxShadow: string; active: boolean } {
  return useMemo(() => {
    const s = j1Store.get("overlay");
    const c: PointerOverlayConfig = { outline: s.outline as boolean | undefined ?? true, shadow: s.shadow as boolean | undefined ?? false, ring: s.ring as boolean | undefined ?? false };
    return composeOverlay(c);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

/* ------------------------------- 桌面窗全量层 ------------------------------- */

export function J1Runtime(): React.ReactElement | null {
  const [replica, setReplica] = useState<{ x: number; y: number } | null>(null);
  const [anchorUi, setAnchorUi] = useState<{ x: number; y: number } | null>(null);
  const [ink, setInk] = useState<{ x: number; y: number }[] | null>(null);
  const [liveInk, setLiveInk] = useState<{ x: number; y: number }[] | null>(null);

  useEffect(() => {
    // 作用域声明（F605/F616/F615 的真实挂点——全系统唯一直属桌面声明处）。
    document.documentElement.dataset.appId = "desktop";
    document.documentElement.dataset.appClass = "list";
    // 未接线动作提示去重（每个动作一次性——显性但不骚扰，章九）。
    const warned = new Set<string>();
    const rt = createWindowRuntime(
      { entry: "desktop", appScope: "desktop", appClass: "list", replica: true },
      {
        onReplica: setReplica,
        onAnchor: setAnchorUi,
        onInk: (pts) => {
          if (!pts) {
            setInk(null);
            return;
          }
          setInk(pts);
          window.setTimeout(() => setInk(null), TRAIL_FADE_MS);
        },
        onLiveInk: setLiveInk,
        onDeviceClone: (info) => {
          pushToast("info", "已为本机指针设备建档", info.evicted ? `档案达上限，淘汰最久未用：${info.evicted}` : "手感参数将跟随该设备记忆（F614）");
        },
        onActionUnhandled: (action, source) => {
          if (warned.has(action)) return;
          warned.add(action);
          pushToast("info", `动作「${action}」在本窗口未接线`, `${source === "gesture" ? "手势" : "侧键"}已识别但无处理器——已登记体验日志（每个动作只提示一次）`);
        },
      },
    );
    const wiring = installBuiltinHandlers({ tauriWindow: tauriWindowHandle() });
    return () => {
      rt.dispose();
      wiring.dispose();
      delete document.documentElement.dataset.appId;
      delete document.documentElement.dataset.appClass;
    };
  }, []);

  const overlay = useOverlayStyle();
  if (!replica && !anchorUi && !ink && !liveInk) return null;

  return (
    <>
      {replica && overlay.active && <PointerReplica x={replica.x} y={replica.y} filter={overlay.cssFilter} shadow={overlay.boxShadow} />}
      {anchorUi && <AnchorMark x={anchorUi.x} y={anchorUi.y} />}
      {liveInk && <InkLayer pts={liveInk} live />}
      {!liveInk && ink && <InkLayer pts={ink} />}
    </>
  );
}

/* ------------------------------- 软件窗口层（write/mind/code/fate） ------------------------------- */

/** 软件窗口 → 作用域/滚轮类目映射（per-app 档案的类目判据：文档逐档、列表平滑）。 */
const APP_WINDOW_SCOPE: Record<string, { scope: string; klass: string; label: string }> = {
  write: { scope: "app-write", klass: "document", label: "Variable Write" },
  code: { scope: "app-code", klass: "code", label: "Variable Code" },
  mind: { scope: "app-mind", klass: "list", label: "Variable Mind" },
  fate: { scope: "app-fate", klass: "list", label: "Variable Fate" },
};

export function J1AppWindowLayer(props: { appType: string }): React.ReactElement | null {
  const [anchorUi, setAnchorUi] = useState<{ x: number; y: number } | null>(null);
  const [ink, setInk] = useState<{ x: number; y: number }[] | null>(null);
  const [liveInk, setLiveInk] = useState<{ x: number; y: number }[] | null>(null);

  useEffect(() => {
    const id = APP_WINDOW_SCOPE[props.appType] ?? { scope: `app-${props.appType}`, klass: "document", label: props.appType };
    document.documentElement.dataset.appId = id.scope;
    document.documentElement.dataset.appClass = id.klass;
    const rt = createWindowRuntime(
      { entry: id.scope, appScope: id.scope, appClass: id.klass, replica: false },
      {
        onAnchor: setAnchorUi,
        onInk: (pts) => {
          if (!pts) {
            setInk(null);
            return;
          }
          setInk(pts);
          window.setTimeout(() => setInk(null), TRAIL_FADE_MS);
        },
        onLiveInk: setLiveInk,
        onDeviceClone: (info) => {
          pushToast("info", "已为本机指针设备建档", info.evicted ? `档案达上限，淘汰最久未用：${info.evicted}` : undefined);
        },
        // 软件窗口不弹「未接线」提示（创作场景少打扰）——遥测已显性记录。
      },
    );
    const wiring = installBuiltinHandlers({ tauriWindow: tauriWindowHandle() });
    return () => {
      rt.dispose();
      wiring.dispose();
      delete document.documentElement.dataset.appId;
      delete document.documentElement.dataset.appClass;
    };
  }, [props.appType]);

  if (!anchorUi && !ink && !liveInk) return null;
  return (
    <>
      {anchorUi && <AnchorMark x={anchorUi.x} y={anchorUi.y} />}
      {liveInk && <InkLayer pts={liveInk} live />}
      {!liveInk && ink && <InkLayer pts={ink} />}
    </>
  );
}
