import { useEffect, useRef } from "react";
import { AppWindow, Archive, Ban, Files, Type } from "lucide-react";
import { useI18n } from "../../i18n";
import { pushToast } from "../../state/uiStore";
import { useStore } from "../store";
import {
  compatibility,
  dndStore,
  endDrag,
  selectDndPayload,
  selectDndTrayVisible,
  setDndHover,
  startDrag,
  stash,
  unstash,
  type DndPayload,
} from "./bus";
import "../../styles/ai08-dnd.css";

/**
 * U-17 拖放总线视觉层（DndLayer）：
 * - 拖拽会话激活 → 渲染统一拖拽幽灵（fixed 跟随 pointermove：
 *   缩略块 + 数量徽标 N× + 来源色条）；位置更新走 DOM 直改（ref），
 *   不进 React 渲染循环——高频 pointermove 下避免整层重渲染。
 * - 悬停目标命中判定：elementFromPoint + closest("[data-dnd-target]")，
 *   目标高亮（elev-1 阴影 + 虚线框）/ 不兼容「禁止」态均由本层直接打类，
 *   目标组件零成本（注册 + 挂 data-dnd-target 属性即可参与）。
 * - pointerup 命中兼容目标 → onDropToTarget(targetId, payload)。
 * - 屏幕边缘（<24px）停留 >600ms → 自动寄存到收藏托盘
 *   （跨虚拟桌面拖放断链的解法：桌面切换后从托盘取出重新进入拖拽态）。
 * - 无会话且无寄存 → 返回 null（零常驻开销）。
 */

/** 边缘判定宽度与停留时长（毫秒）。 */
const EDGE = 24;
const DWELL = 600;

/** 最近指针位置（模块级：托盘取出时播种，会话挂载即显示幽灵于该点）。 */
let lastPtr: { x: number; y: number } | null = null;
/** lastPtr 是否为「取出」播种（决定会话挂载时幽灵是否立即可见）。 */
let lastPtrFresh = false;

function payloadIcon(p: DndPayload): React.ReactElement {
  if (p.kind === "files") return <Files size={14} className="dnd-ghost-ic" />;
  if (p.kind === "text") return <Type size={14} className="dnd-ghost-ic" />;
  return <AppWindow size={14} className="dnd-ghost-ic" />;
}

/** 拖拽会话（仅会话激活期间挂载）。 */
function DndSession(props: { onDropToTarget?: (targetId: string, payload: DndPayload) => void }): React.ReactElement {
  const { t } = useI18n();
  const payload = useStore(dndStore, selectDndPayload);
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const edgeTimer = useRef<number | null>(null);
  /** 当前悬停命中的目标 id（DOM 高亮同步用，与 store.hoverId 镜像）。 */
  const hoverIdRef = useRef<string | null>(null);
  /** 当前被打高亮类的目标元素 + 类名（切换/收尾时移除）。 */
  const hiRef = useRef<{ el: Element; cls: string } | null>(null);
  /** 回调经 ref 转发：父层重渲染换回调不要求重挂会话。 */
  const onDropRef = useRef(props.onDropToTarget);
  onDropRef.current = props.onDropToTarget;

  const applyGhost = (x: number, y: number): void => {
    const g = ghostRef.current;
    if (!g) return;
    g.classList.add("dnd-ghost-show");
    g.style.transform = `translate3d(${x + 14}px, ${y + 12}px, 0)`;
  };

  const clearHighlight = (): void => {
    if (hiRef.current) {
      hiRef.current.el.classList.remove("dnd-hit", "dnd-hit-no");
      hiRef.current = null;
    }
    hoverIdRef.current = null;
    ghostRef.current?.classList.remove("dnd-ghost-no");
    document.body.classList.remove("dnd-forbidden");
    setDndHover(null);
  };

  const nearEdge = (x: number, y: number): boolean => {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    return x < EDGE || y < EDGE || x > vw - EDGE || y > vh - EDGE;
  };

  // 边缘停留达阈值 → 寄存（单槽被占用时 stash 自行拒绝）
  const edgeDwell = (): void => {
    edgeTimer.current = null;
    const s = dndStore.getState();
    if (!s.payload || !lastPtr || !nearEdge(lastPtr.x, lastPtr.y)) return;
    if (stash()) pushToast("info", t("dndTrayLabel"), t("dndStashHint"));
  };

  useEffect(() => {
    document.body.classList.add("dnd-dragging");

    const onMove = (e: PointerEvent): void => {
      lastPtr = { x: e.clientX, y: e.clientY };
      applyGhost(e.clientX, e.clientY);

      // 边缘停留计时（进入边缘起表，离开即取消）
      if (nearEdge(e.clientX, e.clientY)) {
        if (edgeTimer.current === null) edgeTimer.current = window.setTimeout(edgeDwell, DWELL);
      } else if (edgeTimer.current !== null) {
        window.clearTimeout(edgeTimer.current);
        edgeTimer.current = null;
      }

      // 悬停命中判定（幽灵 pointer-events:none，不会挡住 elementFromPoint）
      const s = dndStore.getState();
      if (!s.payload) return;
      const el = document.elementFromPoint(e.clientX, e.clientY);
      const targetEl = el ? el.closest("[data-dnd-target]") : null;
      const id = targetEl?.getAttribute("data-dnd-target") ?? null;
      const def = id ? s.targets[id] : undefined;
      if (targetEl && def) {
        if (hoverIdRef.current !== id) {
          clearHighlight();
          hoverIdRef.current = id;
          const compat = compatibility(s.payload, def.accepts);
          const cls = compat === "accept" ? "dnd-hit" : "dnd-hit-no";
          targetEl.classList.add(cls);
          hiRef.current = { el: targetEl, cls };
          setDndHover(id);
          ghostRef.current?.classList.toggle("dnd-ghost-no", compat === "forbidden");
          document.body.classList.toggle("dnd-forbidden", compat === "forbidden");
        }
      } else if (hoverIdRef.current !== null) {
        clearHighlight();
      }
    };

    const onUp = (): void => {
      const s = dndStore.getState();
      if (!s.payload) return;
      const id = hoverIdRef.current;
      if (id) {
        const def = s.targets[id];
        if (def && compatibility(s.payload, def.accepts) === "accept") {
          onDropRef.current?.(id, s.payload);
        }
      }
      endDrag(); // 无论是否命中，会话就此结束
    };

    const onCancel = (): void => {
      endDrag();
    };

    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);

    // 托盘「取出」播种的指针位置：挂载即显示幽灵（新会话则等首次 move）
    if (lastPtrFresh && lastPtr) {
      lastPtrFresh = false;
      applyGhost(lastPtr.x, lastPtr.y);
    }

    return () => {
      document.body.classList.remove("dnd-dragging", "dnd-forbidden");
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      if (edgeTimer.current !== null) {
        window.clearTimeout(edgeTimer.current);
        edgeTimer.current = null;
      }
      clearHighlight();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!payload) return <div ref={ghostRef} className="dnd-ghost" aria-hidden />;
  const n = payload.kind === "files" ? payload.files?.length ?? 0 : 0;
  return (
    <div ref={ghostRef} className="dnd-ghost" aria-hidden>
      {/* 来源色条 */}
      <span
        className="dnd-ghost-bar"
        style={payload.sourceColor ? { background: payload.sourceColor } : undefined}
      />
      <span className="dnd-ghost-main">
        {payloadIcon(payload)}
        <span className="dnd-ghost-label ellipsis">{payload.sourceLabel}</span>
        {n > 1 && <span className="dnd-ghost-badge">{n}×</span>}
        <Ban size={13} className="dnd-ghost-ban" />
      </span>
    </div>
  );
}

/** 收藏托盘（屏幕左缘常驻小槽；有寄存物时显示）。 */
function DndTray(): React.ReactElement {
  const { t } = useI18n();
  const stashed = useStore(dndStore, selectDndTrayVisible);
  const payload = useStore(dndStore, (s) => (s.stash ? s.stash : null));
  if (!stashed || !payload) return <></>;

  const takeOut = (e: React.MouseEvent): void => {
    lastPtr = { x: e.clientX, y: e.clientY };
    lastPtrFresh = true;
    const p = unstash();
    if (p) startDrag(p); // 重新进入拖拽态
  };

  const n = payload.kind === "files" ? payload.files?.length ?? 0 : 0;
  return (
    <button
      type="button"
      className="dnd-tray"
      onClick={takeOut}
      title={t("dndTrayTakeOut")}
      aria-label={t("dndTrayTakeOut")}
    >
      <span
        className="dnd-tray-bar"
        style={payload.sourceColor ? { background: payload.sourceColor } : undefined}
      />
      <Archive size={14} className="dnd-tray-ic" />
      <span className="dnd-tray-label">{payload.sourceLabel}</span>
      {n > 1 && <span className="dnd-tray-badge">{n}×</span>}
    </button>
  );
}

/**
 * 全局拖放视觉层入口（挂载于桌面壳层，位于 VWM 之上、模态之下）：
 * 无拖拽会话且无寄存物时返回 null——不注册任何监听、不渲染任何节点。
 */
export function DndLayer(props: {
  onDropToTarget?: (targetId: string, payload: DndPayload) => void;
}): React.ReactElement | null {
  const active = useStore(dndStore, (s) => s.payload !== null);
  const trayVisible = useStore(dndStore, (s) => s.stash !== null);
  if (!active && !trayVisible) return null;
  return (
    <>
      {active && <DndSession onDropToTarget={props.onDropToTarget} />}
      {trayVisible && <DndTray />}
    </>
  );
}
