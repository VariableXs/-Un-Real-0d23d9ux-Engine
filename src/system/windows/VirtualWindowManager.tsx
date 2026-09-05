import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import {
  computeWorkArea,
  cycleVwmFocus,
  minimizeAllVwm,
  setVwmWorkArea,
  snapVwmWin,
  vwmStore,
  type VwmRect,
} from "./vwm";
import { useStore } from "../../lib/store";
import { VirtualWindowFrame } from "./VirtualWindowFrame";
import { VwmAppContent } from "./VwmAppContent";
import { isTpApp, type VwmWin } from "./vwm";
import { ipc } from "../../lib/ipc";

/**
 * 虚拟窗口管理器（Virtual Window Manager）桌面层：
 * - 在桌面层内渲染所有已打开软件的虚拟窗口（图标层之上、任务栏与红绿灯之下）
 * - 环境隔离快捷键：Alt+Tab 在 Variable 窗口间轮转（WebView 内尽力捕获，
 *   被系统 Alt+Tab 抢占时由 Win+Tab 切换器兜底）；Win+方向键贴靠聚焦窗口；
 *   Win+M 最小化全部虚拟窗口（桌面壳层已有 OS 窗口部分）
 * - 最小化窗口保持挂载（display:none），恢复零重载、状态零丢失
 */
export function VirtualWindowManager(props: { settings: Settings }): React.ReactElement | null {
  const wins = useStore(vwmStore, (s) => s.wins);
  const focusedId = useStore(vwmStore, (s) => s.focusedId);
  const snapPreview = useStore(vwmStore, (s) => s.snapPreview);
  const closing = useStore(vwmStore, (s) => s.closing);
  const flying = useStore(vwmStore, (s) => s.flying);

  // 工作区跟随视口尺寸与任务栏停靠位置（最大化/贴靠/边缘判定都基于它）
  useEffect(() => {
    const apply = (): void =>
      setVwmWorkArea(computeWorkArea(props.settings.taskbarPos, window.innerWidth, window.innerHeight));
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, [props.settings.taskbarPos]);

  // Alt+Tab：Variable 环境内窗口轮转（系统未抢占时生效）
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Tab" && e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        cycleVwmFocus(e.shiftKey);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Win+方向键（Rust 全局键 → sys://snap）：贴靠当前聚焦的虚拟窗口。
  // 桌面窗口自身持有 OS 焦点时才响应（文件管理器等 OS 窗口聚焦时让给既有 applySnap）。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<string>("sys://snap", (e) => {
      void (async () => {
        const dir = e.payload;
        if (dir !== "left" && dir !== "right" && dir !== "up" && dir !== "down") return;
        const s = vwmStore.getState();
        if (!s.focusedId) return;
        const focused = await getCurrentWindow()
          .isFocused()
          .catch(() => false);
        if (!focused) return;
        snapVwmWin(s.focusedId, dir);
      })();
    });
    void p
      .then((f) => {
        if (disposed) f();
        else un = f;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  // Win+M（sys://minimize-all）：虚拟窗口一并最小化（OS 窗口部分由 DesktopShell 处理）
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen("sys://minimize-all", () => minimizeAllVwm());
    void p
      .then((f) => {
        if (disposed) f();
        else un = f;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  if (wins.length === 0) return null;

  return (
    <div className="vwm-layer" role="presentation">
      {wins.map((w) => (
        <VirtualWindowFrame
          key={w.id}
          win={w}
          focused={w.id === focusedId}
          zIndex={w.z}
          closing={closing.includes(w.id)}
          flying={flying.includes(w.id)}
        >
          <VwmAppContent winId={w.id} app={w.app} winPath={w.path} settings={props.settings} />
        </VirtualWindowFrame>
      ))}
      {wins.map((w) => (
        <EmbedBridge key={`bridge-${w.id}`} win={w} />
      ))}
      {snapPreview && snapPreview.w > 0 && (
        <div
          className="vwm-snap-preview"
          aria-hidden
          style={snapPreviewStyle(snapPreview)}
        />
      )}
    </div>
  );
}

function snapPreviewStyle(r: VwmRect): React.CSSProperties {
  return { left: r.x, top: r.y, width: r.w, height: r.h };
}

/**
 * 批次E-16：第三方应用嵌入窗口的边界同步桥。
 * 原生子窗口恒渲染在 webview 之上，位置跟随虚拟窗口（内容区 = 标题栏以下）；
 * 最小化=隐藏、恢复=显示；卸载（关闭完成）= 关闭嵌入窗口。
 */
function EmbedBridge({ win }: { win: VwmWin }): null {
  useEffect(() => {
    if (!isTpApp(win.app)) return;
    const dpr = window.devicePixelRatio || 1;
    if (win.minimized) {
      void ipc.embedVisible(false).catch(() => {});
      return;
    }
    void ipc
      .embedBounds(
        Math.round(win.x * dpr),
        Math.round((win.y + 38) * dpr),
        Math.round(win.w * dpr),
        Math.round((win.h - 38) * dpr),
      )
      .then(() => ipc.embedVisible(true))
      .catch(() => {});
  }, [win.app, win.x, win.y, win.w, win.h, win.minimized]);
  useEffect(() => {
    const tp = isTpApp(win.app);
    return () => {
      if (tp) void ipc.embedClose().catch(() => {});
    };
  }, [win.app]);
  return null;
}
