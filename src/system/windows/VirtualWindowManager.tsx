import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import {
  computeWorkArea,
  cycleVwmFocus,
  minimizeAllVwm,
  restoreShakenVwm,
  setVwmWorkArea,
  snapVwmWin,
  vwmStore,
  type VwmRect,
} from "./vwm";
import { useStore } from "../../lib/store";
import { useI18n } from "../../i18n";
import { askChoice } from "../../components/Modal";
import { VirtualWindowFrame } from "./VirtualWindowFrame";
import { VwmAppContent } from "./VwmAppContent";
import { isTpApp, closeVwmWin, openVwmTpNew, isVwmWinVisible, type VwmWin } from "./vwm";
import { setEmbedSessionState, clearEmbedSessionState, bumpEmbedResync, embedStateStore } from "./embedState";
import { ipc } from "../../lib/ipc";
// AI-01 窗口手感：M-03 抽屉 / Z-42 切换器（含热区）/ M-06 挂起登记
import { MinimizedDrawer } from "./MinimizedDrawer";
import { DesktopHotzone, DesktopSwitcher } from "./DesktopSwitcher";
import { takeAllSuspended } from "./winfeelMenu";

/**
 * 虚拟窗口管理器（Virtual Window Manager）桌面层：
 * - 在桌面层内渲染所有已打开软件的虚拟窗口（图标层之上、任务栏与红绿灯之下）
 * - 环境隔离快捷键：Alt+Tab 在 Variable 窗口间轮转（WebView 内尽力捕获，
 *   被系统 Alt+Tab 抢占时由 Win+Tab 切换器兜底）；Win+方向键贴靠聚焦窗口；
 *   Win+M 最小化全部虚拟窗口（桌面壳层已有 OS 窗口部分）
 * - 最小化窗口保持挂载（display:none），恢复零重载、状态零丢失
 */
export function VirtualWindowManager(props: { settings: Settings }): React.ReactElement | null {
  const { t } = useI18n();
  const wins = useStore(vwmStore, (s) => s.wins);
  const focusedId = useStore(vwmStore, (s) => s.focusedId);
  const snapPreview = useStore(vwmStore, (s) => s.snapPreview);
  const closing = useStore(vwmStore, (s) => s.closing);
  const flying = useStore(vwmStore, (s) => s.flying);
  // AI-01 M-07：对齐参考线（拖拽中由 frame 写入，这里渲染）
  const guides = useStore(vwmStore, (s) => s.guides);
  // AI-01 M-03/Z-42：最小化抽屉 / 桌面切换器
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  // AI-01 M-04：无响应窗口集合（3s 轮询 IsHungAppWindow）
  const [hungIds, setHungIds] = useState<Set<string>>(new Set());

  // AI-01 M-01：Ctrl+Alt+D 全部还原（摇一摇反向操作）；Z-42：Ctrl+Alt+G 切换器
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (!(e.ctrlKey && e.altKey)) return;
      const k = e.key.toLowerCase();
      if (k === "d") {
        e.preventDefault();
        e.stopPropagation();
        restoreShakenVwm();
      } else if (k === "g") {
        e.preventDefault();
        e.stopPropagation();
        setSwitcherOpen((v) => !v);
      } else if (k === "`" || e.code === "Backquote") {
        e.preventDefault();
        e.stopPropagation();
        setDrawerOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // AI-01 M-04：无响应体检（3s 轮询；仅嵌入第三方进程窗口参与，后端 IsHungAppWindow）
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let alive = true;
    const scan = (): void => {
      const st = vwmStore.getState();
      const meta = embedStateStore.getState().meta;
      const idPid: Array<[string, number]> = st.wins
        .filter((w) => isTpApp(w.app) && !w.minimized)
        .map((w): [string, number] => [w.id, meta[w.id]?.rootPid ?? 0])
        .filter((entry): entry is [string, number] => entry[1] > 0);
      if (idPid.length === 0) {
        setHungIds((prev) => (prev.size > 0 ? new Set() : prev));
        return;
      }
      void ipc
        .winHealthScan(idPid.map(([, pid]) => pid))
        .then((hungPids) => {
          if (!alive) return;
          const hs = new Set(hungPids);
          const next = new Set(idPid.filter(([, pid]) => hs.has(pid)).map(([id]) => id));
          setHungIds((prev) => {
            if (prev.size === next.size && [...next].every((id) => prev.has(id))) return prev;
            return next;
          });
        })
        .catch(() => {});
    };
    scan();
    const timer = window.setInterval(scan, 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  // AI-01 M-06 红线：环境退出前自动恢复全部挂起中的第三方进程。
  useEffect(() => {
    return () => {
      const meta = embedStateStore.getState().meta;
      for (const id of takeAllSuspended()) {
        const pid = meta[id]?.rootPid ?? 0;
        if (pid > 0) void ipc.procResume(pid).catch(() => {});
      }
    };
  }, []);

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

  // 批次W-3 + C-1：嵌入监护上报（embed://state）→ 占位卡状态机。
  // exited/orphaned = 占位卡；running = 自动重嵌成功 → 清占位卡 + 边界重同步。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<{ embedId: string; state: "exited" | "orphaned" | "running" }>("embed://state", (e) => {
      if (e.payload.state === "running") {
        clearEmbedSessionState(e.payload.embedId);
        bumpEmbedResync(e.payload.embedId);
      } else {
        setEmbedSessionState(e.payload.embedId, e.payload.state);
      }
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

  // 批次C-1：同进程树新主窗口（如 Chrome 设置页）自动收编为新嵌入会话：
  // WinEventHook 探测 → embed://popup → 开新占位窗（embed_id）→ embed_adopt 重父化登记。
  // adopt 失败（窗口已销毁）→ 关闭刚开的占位窗，不伪造成功。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<{ origin: string; tpId: string; hwnd: number; rootPid: number }>(
      "embed://popup",
      (e) => {
        const { tpId, hwnd, rootPid } = e.payload;
        const embedId = openVwmTpNew(`tp:${tpId}`);
        void ipc
          .embedAdopt(tpId, hwnd, rootPid, embedId)
          .then((ok) => {
            if (!ok) closeVwmWinSafe(embedId);
          })
          .catch(() => closeVwmWinSafe(embedId));
      },
    );
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

  // D-3 全域软件接管看门狗：逃逸窗口事件 → auto 策略直接收编；
  // ask 策略弹询问卡（收进 Variable / 本次保持在桌面 / 总是忽略）。
  // 后端已排除白名单、L4 全屏让位与维护模式；默认「询问」不自动回收。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<{
      hwnd: number;
      rootPid: number;
      title: string;
      image: string;
      auto: boolean;
    }>("watch://escape", (e) => {
      const { hwnd, rootPid, title, image, auto } = e.payload;
      const adopt = (): void => {
        const embedId = openVwmTpNew(`tp:${image.replace(/\.exe$/i, "") || "watch"}`);
        void ipc
          .embedAdopt(image, hwnd, rootPid, embedId)
          .then((ok) => {
            if (!ok) closeVwmWinSafe(embedId);
          })
          .catch(() => closeVwmWinSafe(embedId));
      };
      if (auto) {
        adopt();
        return;
      }
      void (async () => {
        const choice = await askChoice({
          title: t("watchAskTitle"),
          body: `${t("watchAskBody")} ${title || image}`,
          options: [
            { value: "adopt", label: t("watchAdopt") },
            { value: "once", label: t("watchKeepOnce") },
            { value: "always", label: t("watchIgnoreAlways") },
          ],
        });
        if (!choice || choice === "once") {
          void ipc.watchDismiss(image, "once").catch(() => {});
          return;
        }
        if (choice === "always") {
          void ipc.watchDismiss(image, "always").catch(() => {});
          return;
        }
        adopt();
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

  if (wins.length === 0) return null;

  return (
    <div className="vwm-layer" role="presentation">
      {wins.filter(isVwmWinVisible).map((w) => (
        <VirtualWindowFrame
          key={w.id}
          win={w}
          focused={w.id === focusedId}
          zIndex={w.z}
          closing={closing.includes(w.id)}
          flying={flying.includes(w.id)}
          settings={props.settings}
          hung={hungIds.has(w.id)}
        >
          <VwmAppContent winId={w.id} app={w.app} winPath={w.path} settings={props.settings} />
        </VirtualWindowFrame>
      ))}
      {wins.map((w) => (
        <EmbedBridge key={`bridge-${w.id}`} win={w} focused={w.id === focusedId} visible={isVwmWinVisible(w)} />
      ))}
      {snapPreview && snapPreview.w > 0 && (
        <div
          className="vwm-snap-preview"
          aria-hidden
          style={snapPreviewStyle(snapPreview)}
        />
      )}
      {/* AI-01 M-07：拖拽对齐参考线 */}
      {guides?.guideXs.map((x) => (
        <div key={`gx-${x}`} className="vwm-guide-v" aria-hidden style={{ left: x }} />
      ))}
      {guides?.guideYs.map((y) => (
        <div key={`gy-${y}`} className="vwm-guide-h" aria-hidden style={{ top: y }} />
      ))}
      {/* AI-01 M-03：最小化抽屉；Z-42：桌面切换器 + 右缘热区 */}
      <MinimizedDrawer open={drawerOpen} onToggle={() => setDrawerOpen((v) => !v)} onClose={() => setDrawerOpen(false)} />
      <DesktopHotzone width={props.settings.desktopHotzone ?? 0} onTrigger={() => setSwitcherOpen(true)} />
      <DesktopSwitcher open={switcherOpen} onClose={() => setSwitcherOpen(false)} settings={props.settings} />
    </div>
  );
}

function snapPreviewStyle(r: VwmRect): React.CSSProperties {
  return { left: r.x, top: r.y, width: r.w, height: r.h };
}

/** 批次C-1：adopt 失败时收掉刚开的占位窗（容错，不影响其它窗口）。 */
function closeVwmWinSafe(id: string): void {
  try {
    closeVwmWin(id);
  } catch {
    /* 窗口已不存在 */
  }
}

// ---------- 批次W-2：每显示器 DPI 感知 ----------
/** 多屏 API 结构（Chromium window-management；WebView2 需权限，可能拿不到）。 */
type ScreenDetails = {
  screens: Array<{ left: number; top: number; width: number; height: number; devicePixelRatio: number }>;
};
let screenDetails: ScreenDetails | null | undefined; // undefined=未探测 null=不可用

/** 窗口中心所在显示器的 devicePixelRatio（混合 DPI 双屏关键）；拿不到 → 主屏值回退。 */
function monitorDprAt(cx: number, cy: number): number {
  if (screenDetails === undefined) {
    screenDetails = null;
    const g = (window as unknown as { getScreenDetails?: () => Promise<ScreenDetails> })
      .getScreenDetails;
    if (g) void g().then((d) => (screenDetails = d)).catch(() => {});
  }
  if (screenDetails) {
    for (const s of screenDetails.screens) {
      if (cx >= s.left && cx < s.left + s.width && cy >= s.top && cy < s.top + s.height) {
        return s.devicePixelRatio || 1;
      }
    }
  }
  return window.devicePixelRatio || 1;
}

/**
 * 批次E-16：第三方应用嵌入窗口的边界同步桥。
 * 原生子窗口恒渲染在 webview 之上，位置跟随虚拟窗口（内容区 = 标题栏以下）；
 * 最小化=隐藏、恢复=显示；卸载（关闭完成）= 关闭该会话的嵌入窗口。
 * 批次W-1：多嵌入并发 —— 所有调用携带 embedId（= VWM 窗口实例 id）；
 * 焦点仲裁：Z 序顶（聚焦）的嵌入窗口获得原生键盘焦点。
 */
function EmbedBridge({ win, focused, visible = true }: { win: VwmWin; focused: boolean; visible?: boolean }): null {
  const embedId = win.id;
  // 批次C-1：自动重嵌成功后（running 事件）→ 重发边界/显示，恢复画面跟随
  const resync = useStore(embedStateStore, (s) => s.resync[embedId] ?? 0);
  useEffect(() => {
    if (!isTpApp(win.app)) return;
    // 批次W-2：按窗口中心所在显示器的实际 DPR 换算物理像素（混合 DPI 双屏）
    const dpr = monitorDprAt(win.x + win.w / 2, win.y + win.h / 2);
    // 批次W-5：标签组非显示成员 → 隐藏原生嵌入窗口（保活，不关闭会话）
    if (win.minimized || !visible) {
      void ipc.embedVisible(embedId, false).catch(() => {});
      return;
    }
    void ipc
      .embedBounds(
        embedId,
        Math.round(win.x * dpr),
        Math.round((win.y + 38) * dpr),
        Math.round(win.w * dpr),
        Math.round((win.h - 38) * dpr),
      )
      .then(() => ipc.embedVisible(embedId, true))
      .catch(() => {});
  }, [win.app, win.x, win.y, win.w, win.h, win.minimized, visible, embedId, resync]);
  // W-1 焦点仲裁：点击非顶嵌入窗口 → 前端先置顶（pointerFocusVwm）→ 本效应移交原生焦点
  // 批次W-5：标签组非显示成员不参与焦点仲裁
  useEffect(() => {
    if (!isTpApp(win.app) || !focused || win.minimized || !visible) return;
    void ipc.embedFocus(embedId).catch(() => {});
  }, [win.app, win.minimized, focused, visible, embedId]);
  useEffect(() => {
    const tp = isTpApp(win.app);
    return () => {
      if (tp) void ipc.embedClose(embedId).catch(() => {});
    };
  }, [win.app, embedId]);
  return null;
}
