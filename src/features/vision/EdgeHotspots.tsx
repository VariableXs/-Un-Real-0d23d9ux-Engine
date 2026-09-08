/**
 * AI-17 · Z-69 边缘热区（Edge Hotspots）运行时。
 * 默认仅左下角=开始菜单；悬停 300ms 防误触；单击触发可选；
 * 全屏应用内全部禁用。由 VisionRuntime 挂载（仅桌面窗口）。
 */
import { useEffect, useRef } from "react";
import {
  loadHotspotConfig, hitEdge, actionFor, isFullscreenActive, type EdgePosition, type HotspotAction,
} from "../../lib/hotspots";
import { uiStore } from "../../state/uiStore";

function dispatchAction(action: HotspotAction): void {
  switch (action) {
    case "start-menu":
      uiStore.setState((s) => ({ startOpen: !s.startOpen }));
      break;
    case "quick-panel":
      uiStore.setState({ quickOpen: true, startOpen: false });
      break;
    case "desktop":
    case "vwm":
      // 预留：环境/桌面级动作由后续扩展注册
      break;
    default:
      break; // none：完全无行为
  }
}

export function EdgeHotspots(): null {
  const dwellTimer = useRef<number>(0);
  const hoveredEdge = useRef<EdgePosition | null>(null);

  useEffect(() => {
    const onMove = (e: PointerEvent): void => {
      const cfg = loadHotspotConfig();
      if (!cfg.enabled || isFullscreenActive()) {
        hoveredEdge.current = null;
        return;
      }
      const edge = hitEdge(e.clientX, e.clientY, window.innerWidth, window.innerHeight, cfg.hitPx);
      if (edge === hoveredEdge.current) return;
      hoveredEdge.current = edge;
      window.clearTimeout(dwellTimer.current);
      if (edge && actionFor(cfg, edge) !== "none") {
        dwellTimer.current = window.setTimeout(() => {
          // 触发瞬间再次校验（防误触后移出）
          if (hoveredEdge.current === edge && !isFullscreenActive()) {
            dispatchAction(actionFor(loadHotspotConfig(), edge));
          }
        }, cfg.hoverDwellMs);
      }
    };
    const onClick = (e: MouseEvent): void => {
      const cfg = loadHotspotConfig();
      if (!cfg.enabled || isFullscreenActive()) return;
      const edge = hitEdge(e.clientX, e.clientY, window.innerWidth, window.innerHeight, cfg.hitPx);
      if (edge && actionFor(cfg, edge) !== "none") dispatchAction(actionFor(cfg, edge));
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("click", onClick, true);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("click", onClick, true);
      window.clearTimeout(dwellTimer.current);
    };
  }, []);

  return null; // 纯行为组件，无渲染
}
