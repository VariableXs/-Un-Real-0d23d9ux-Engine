import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import {
  computeWorkArea,
  cycleVwmFocus,
  cycleVwmFocusFiltered,
  focusVwmWin,
  minimizeAllVwm,
  minimizeVwmWin,
  resizeVwmWin,
  restoreShakenVwm,
  setVwmWorkArea,
  snapVwmWin,
  unhideAllVwm,
  vwmStore,
  type VwmRect,
} from "./vwm";
import { parseScreenDetails, type ScreenInfo } from "./winfeel";
import { pushToast } from "../../state/uiStore";
import { useStore } from "../../lib/store";
import { useI18n } from "../../i18n";
import { askChoice, askConfirm } from "../../components/Modal";
import { VirtualWindowFrame } from "./VirtualWindowFrame";
import { VwmAppContent } from "./VwmAppContent";
import { isTpApp, closeVwmWin, openVwmTpNew, isVwmWinVisible, type VwmWin } from "./vwm";
import { setEmbedSessionState, clearEmbedSessionState, bumpEmbedResync, embedStateStore } from "./embedState";
import { ipc } from "../../lib/ipc";
// AI-01 窗口手感：M-03 抽屉 / Z-42 切换器（含热区）/ M-06 挂起登记
import { MinimizedDrawer } from "./MinimizedDrawer";
import { DesktopHotzone, DesktopSwitcher } from "./DesktopSwitcher";
import { takeAllSuspended } from "./winfeelMenu";
// AI-02 窗口编排组：编排中心 / 舞台侧幕 / 时间机器自动快照 / 规则引擎钩子 /
// 失联救援 / 焦点历史 / 多选编组 / 嵌入焦点联动
import { WindowOrchestrator } from "./WindowOrchestrator";
import { StageRail } from "./StageRail";
import { scheduleAutoSnap } from "./timeline";
import { installRuleHook } from "./rulesApply";
import { healthCheck } from "./rescue";
import { focusBack, focusForward, recordFocus } from "./focusHistory";
import {
  clearMultiSelect,
  multiSelected,
  planGroupClose,
  planSwap,
  subscribeMultiSelect,
  type MultiSelectOp,
} from "./multiselect";
import { noteVwmFocusChange } from "./embedFocusLink";
import { dragActive } from "./dragCancel";

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
  // AI-02 窗口编排组：编排中心 / 舞台 / 多选工具条
  const [orchOpen, setOrchOpen] = useState(false);
  const [activeStage, setActiveStage] = useState<string | null>(null);
  const [selIds, setSelIds] = useState<string[]>([]);

  // N-03：规则引擎开窗钩子（幂等安装——新开窗口按规则裁决贴靠/置顶/透明度/入组）
  useEffect(() => {
    installRuleHook();
  }, []);

  // N-01：布局变化 → 自动快照（去抖 8s；仅几何签名变化时调度，聚焦等无关变更不打扰）
  useEffect(() => {
    const sigOf = (wins: VwmWin[]): string =>
      wins.map((w) => `${w.id}:${w.x},${w.y},${w.w},${w.h},${w.state},${w.minimized ? 1 : 0},${w.z}`).join("|");
    let lastSig = sigOf(vwmStore.getState().wins);
    return vwmStore.subscribe(() => {
      const s = vwmStore.getState();
      const sig = sigOf(s.wins);
      if (sig === lastSig) return;
      lastSig = sig;
      if (s.wins.length > 0) scheduleAutoSnap(s.wins);
    });
  }, []);

  // V-22：启动 + 工作区变更 → 失联窗口体检与拉回（保持尺寸贴最近可视边缘）
  useEffect(() => {
    let lastWa: string | null = null;
    return vwmStore.subscribe(() => {
      const s = vwmStore.getState();
      const key = `${s.workArea.x},${s.workArea.y},${s.workArea.w},${s.workArea.h}`;
      if (key === lastWa) return;
      lastWa = key;
      if (s.wins.length === 0) return;
      const rep = healthCheck(s.wins, s.workArea);
      if (rep.lost.length === 0) return;
      vwmStore.setState((cur) => ({
        wins: cur.wins.map((w) => (rep.moved[w.id] ? { ...w, ...rep.moved[w.id]! } : w)),
      }));
      pushToast("success", t("orchRescue"), `${t("orchRescuedN")} ${rep.lost.length}`);
    });
  }, [t]);

  // V-25 + V-27：真实焦点变化 → 历史栈记录（去重/截断前进分支） + 嵌入视觉态让位
  useEffect(() => {
    recordFocus(focusedId);
    noteVwmFocusChange(focusedId);
  }, [focusedId]);

  // V-24：多选工具条（选择集变化即刷新）
  useEffect(() => subscribeMultiSelect(() => setSelIds(multiSelected())), []);

  // AI-02 键位：Ctrl+Alt+O 编排中心 / V-25 Ctrl+Alt+[ ] 焦点历史回溯 / V-24 Esc 退出编组
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.ctrlKey && e.altKey && !e.shiftKey) {
        const k = e.key.toLowerCase();
        if (k === "o") {
          e.preventDefault();
          e.stopPropagation();
          setOrchOpen((v) => !v);
          return;
        }
        if (e.key === "[" || e.code === "BracketLeft") {
          e.preventDefault();
          e.stopPropagation();
          const back = focusBack();
          if (back) focusVwmWin(back);
          return;
        }
        if (e.key === "]" || e.code === "BracketRight") {
          e.preventDefault();
          e.stopPropagation();
          const fwd = focusForward();
          if (fwd) focusVwmWin(fwd);
          return;
        }
      }
      // V-24：Esc 退出多选编组（拖拽中的 Esc 已被 V-30 在捕获早期消费；此处仅在确有编组时拦截）
      if (e.key === "Escape" && !dragActive() && multiSelected().length > 0) {
        e.preventDefault();
        e.stopImmediatePropagation();
        clearMultiSelect();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // ---------- V-24/V-26 多选工具条操作 ----------
  /** 当前选中成员的操作视图（store 即时取，避免工具条按钮作用陈旧几何）。 */
  const selOps = (): MultiSelectOp[] => {
    const s = vwmStore.getState();
    return selIds
      .map((id) => s.wins.find((w) => w.id === id))
      .filter((w): w is VwmWin => !!w)
      .map((w) => ({ winId: w.id, rect: { x: w.x, y: w.y, w: w.w, h: w.h } }));
  };
  // V-26：互换位置（多选恰好两窗）
  const selSwap = (): void => {
    const ops = selOps();
    if (ops.length !== 2) return;
    const plan = planSwap(ops[0]!, ops[1]!);
    resizeVwmWin(plan.a.winId, plan.a.rect);
    resizeVwmWin(plan.b.winId, plan.b.rect);
  };
  // V-24：一起最小化（编组即散）
  const selMinimize = (): void => {
    for (const id of selIds) minimizeVwmWin(id);
    clearMultiSelect();
  };
  // V-24：一起关闭（确认门恒开——批量关闭必须确认）
  const selClose = (): void => {
    void (async () => {
      const plan = planGroupClose(selOps());
      const ok = await askConfirm({
        title: t("v24CloseTitle"),
        body: t("v24CloseBody"),
        danger: true,
        okLabel: t("v24CloseYes"),
      });
      if (!ok) return;
      for (const id of plan.winIds) closeVwmWinSafe(id);
      clearMultiSelect();
    })();
  };

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
      } else if (k === "h" && !e.repeat) {
        // 批次F：Ctrl+Alt+H 恢复全部隐藏窗口（隐藏 toast 的承诺入口；无隐藏时静默）
        e.preventDefault();
        e.stopPropagation();
        const n = unhideAllVwm();
        if (n > 0) pushToast("info", t("wfMenuHide"), t("wfHiddenRestored", { n }));
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
  // M-08：Ctrl+Alt+Tab 按设置过滤（app=同应用 / monitor=同屏）；空集合 → toast + 全局兜底
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Tab" && e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        if (e.ctrlKey && props.settings.altTabFilter !== "off") {
          const s = vwmStore.getState();
          const focused = s.wins.find((w) => w.id === s.focusedId);
          const ok = cycleVwmFocusFiltered(
            props.settings.altTabFilter === "app" && focused
              ? { byApp: focused.app }
              : focused
                ? { sameMonitorAs: focused, screens: currentScreens() }
                : {},
          );
          if (!ok) {
            pushToast("info", t("wfAltTabFiltered"), t("wfAltTabEmpty"));
            cycleVwmFocus(e.shiftKey);
          }
          return;
        }
        cycleVwmFocus(e.shiftKey);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props.settings.altTabFilter, t]);

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
    <div className="vwm-layer" role="presentation" data-testid="vwm">
      {/* 批次F：隐藏窗口不渲染框架（EmbedBridge 仍保留 → embed_visible(false) 隐藏原生窗口） */}
      {wins.filter((w) => isVwmWinVisible(w) && !w.hidden).map((w) => (
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
        <EmbedBridge key={`bridge-${w.id}`} win={w} focused={w.id === focusedId} visible={isVwmWinVisible(w) && !w.hidden} />
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
      {/* AI-02 V-24：多选工具条（≥2 选中时出现；批量操作入口） */}
      {selIds.length >= 2 && (
        <div className="vwm-selbar" role="toolbar" aria-label={t("v24Exit")}>
          <span className="vwm-selbar-count" aria-label={`${selIds.length}`}>
            {selIds.length}
          </span>
          {selIds.length === 2 && (
            <button type="button" className="btn tiny" onClick={selSwap}>
              {t("v26Swap")}
            </button>
          )}
          <button type="button" className="btn tiny" onClick={selMinimize}>
            {t("v24Min")}
          </button>
          <button type="button" className="btn tiny danger" onClick={selClose}>
            {t("v24CloseBtn")}
          </button>
          <button type="button" className="btn tiny ghost" onClick={() => clearMultiSelect()}>
            {t("v24Exit")}
          </button>
        </div>
      )}
      {/* AI-02 N-02：舞台侧幕（左缘竖排气泡；空舞台 + 无多选时自动不渲染） */}
      <StageRail activeStageId={activeStage} onActivate={(id) => setActiveStage(id)} />
      {/* AI-02 N-01…N-06：窗口编排中心（Ctrl+Alt+O 呼出） */}
      {orchOpen && <WindowOrchestrator settings={props.settings} onClose={() => setOrchOpen(false)} />}
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

/** M-08：当前屏幕矩形列表（解析失败 → 空 = 单屏不过滤）。 */
function currentScreens(): ScreenInfo[] {
  if (screenDetails === undefined) monitorDprAt(0, 0); // 触发惰性探测
  return screenDetails ? (parseScreenDetails(screenDetails) ?? []) : [];
}

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
