import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import { useI18n, I18nContext, makeT } from "./i18n";
import type { Lang } from "./i18n/dictionaries";
import { ipc, errMessage } from "./lib/ipc";
import { loadSettings, saveSetting, type Settings } from "./lib/settings";
import { uiStore, pushToast, resetGlobalCanvasInteraction, useUi } from "./state/uiStore";
import type { AppMode } from "./state/uiStore";
import { isTauriRuntime } from "./entries/runtime";
import { appWindowLabel, trackSelfGeom } from "./system/windows/appWindows";
import { openVwmApp } from "./system/windows/vwm";
import type { BootstrapInfo } from "./lib/types";

import { ErrorBoundary } from "./components/ErrorBoundary";
import { TitleBar, type ClosePhase } from "./components/TitleBar";
import { ToastHost } from "./components/ToastHost";
import { ContextMenuHost } from "./components/ContextMenu";
import { ConfirmBubbleHost, ChoiceHost, ConfirmHost, NetConsentHost, PromptHost, Modal } from "./components/Modal";
// 桌面分支宿主清单见下方 DesktopShell 兄弟节点：ConfirmHost / ChoiceHost / PromptHost /
// ConfirmBubbleHost / NetConsentHost 缺一不可 —— askConfirm/askChoice/askPrompt 的
// promise 由对应 Host 组件resolve，缺宿主 = 按钮"点击无反应"（红灯退出、新建、重命名）。
import { CosmicBackground } from "./features/background/CosmicBackground";
import { BootScreen, type BootStats } from "./system/boot/BootScreen";
import { DesktopShell } from "./system/desktop/DesktopShell";
import { Sidebar } from "./apps/write/folders/Sidebar";
import { EditorView } from "./apps/write/editor/EditorView";
import { MindmapView } from "./apps/mind/MindmapView";
import { ProjectAnalysisView } from "./apps/code/ProjectAnalysisView";
import { CodeXrefPanel } from "./apps/code/XrefPanel";
import { FateView } from "./apps/fate/FateView";
import { SearchOverlay } from "./apps/write/search/SearchOverlay";
import { SettingsModal } from "./features/settings/SettingsModal";
import { OobeGate } from "./features/oobe/OobeWizard";

export type AppEntryType = "desktop" | AppMode;

export default function App(props: { appType: AppEntryType }): React.ReactElement {
  return (
    <ErrorBoundary>
      <AppInner appType={props.appType} />
    </ErrorBoundary>
  );
}

function AppInner(props: { appType: AppEntryType }): React.ReactElement {
  const appType = props.appType;
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [boot, setBoot] = useState<BootstrapInfo | null>(null);
  const [closePhase, setClosePhase] = useState<ClosePhase>("idle");
  const [ready, setReady] = useState(false);
  // 启动仪式门控（仅桌面窗口）：真实加载（boot://event）完成前不进入应用、不发 IPC。
  // 软件窗口按需创建、轻量秒开，不重播启动仪式；浏览器 dev 模式直接跳过。
  // 批次A：loading → exit（字母落位编排，桌面 shell 在其下挂载）→ done。
  const [bootPhase, setBootPhase] = useState<"loading" | "exit" | "done">(
    !isTauriRuntime() || appType !== "desktop" ? "done" : "loading",
  );
  const [bootStats, setBootStats] = useState<BootStats | null>(null);
  const [editing, setEditing] = useState(false);
  const editingTimer = useRef<number>(0);
  const mode = useUi((s) => s.mode);
  const focusMode = useUi((s) => s.focusMode);
  const currentDocId = useUi((s) => s.currentDocId);

  // M4 拆窗：软件窗口内视图由 appType 锁定（mode 变化不影响渲染，仅作内部状态）。
  const view: AppMode | "desktop" = appType === "desktop" ? mode : appType;

  // 批次C（规格 5.7.3）：Write 窗口常驻监听引用跳转 —— Code 面板/其他位置
  // 引用某篇 Write 文档时，打开（或聚焦）Write 并切到该文档。
  useEffect(() => {
    if (appType !== "write" || !isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<{ kind: string; id: string }>("xref://focus", (ev) => {
      if (ev.payload.kind !== "write-doc") return;
      uiStore.setState({ currentDocId: ev.payload.id, mode: "write" });
    });
    void p
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, [appType]);

  // 软件窗口：注册内部 mode + 挂载几何持久化（位置/尺寸记忆）。
  useEffect(() => {
    if (appType === "desktop") return;
    uiStore.setState({ mode: appType });
    let un: (() => void) | undefined;
    void trackSelfGeom(appWindowLabel(appType)).then((f) => {
      un = f;
    });
    return () => un?.();
  }, [appType]);

  // ---------- cross-window settings sync ----------
  // 其他窗口（软件窗口设置页 / 桌面设置）改了设置并落库后，后端广播
  // settings://changed；本窗口若不是发起者，则重载设置驱动壁纸等 UI 更新。
  useEffect(() => {
    if (!isTauriRuntime() || appType !== "desktop") return;
    let disposed = false;
    let un: (() => void) | undefined;
    const selfLabel = getCurrentWindow().label;
    const sub = listen<{ origin: string }>("settings://changed", (ev) => {
      if (ev.payload.origin === selfLabel) return;
      void loadSettings().then((s) => {
        if (!disposed) setSettingsState(s);
      });
    });
    void sub
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, [appType]);

  // S-1：托盘切换防截屏 → 同步本地设置状态并落库（跨窗口由 settings://changed 广播）
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const sub = listen<boolean>("shield://changed", (ev) => {
      if (disposed) return;
      void saveSetting("privacyShield", ev.payload);
      setSettingsState((prev) => (prev ? { ...prev, privacyShield: ev.payload } : prev));
    });
    void sub
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  // track editor typing for background degradation
  useEffect(() => {
    const onDown = (e: KeyboardEvent): void => {
      if (e.key.length === 1 || e.key === "Backspace") {
        const ae = document.activeElement as HTMLElement | null;
        if (ae && (ae.isContentEditable || ae.tagName === "INPUT" || ae.tagName === "TEXTAREA")) {
          setEditing(true);
          window.clearTimeout(editingTimer.current);
          editingTimer.current = window.setTimeout(() => setEditing(false), 1500);
        }
      }
    };
    window.addEventListener("keydown", onDown, true);
    return () => window.removeEventListener("keydown", onDown, true);
  }, []);

  // 批次E-17：左右 Shift 同时按住 = 隐藏环境（可见时；重新打开由 Rust 键盘钩子负责，
  // 因为隐藏后键盘事件不再进入本窗口）
  useEffect(() => {
    if (appType !== "desktop") return;
    let l = false;
    let r = false;
    let fired = false;
    const reset = (): void => {
      if (!l || !r) fired = false;
    };
    const onDown = (e: KeyboardEvent): void => {
      if (e.repeat) return;
      if (e.code === "ShiftLeft") l = true;
      if (e.code === "ShiftRight") r = true;
      if (l && r && !fired) {
        fired = true;
        void ipc.winHideToTray().catch(() => {});
      }
    };
    const onUp = (e: KeyboardEvent): void => {
      if (e.code === "ShiftLeft") { l = false; reset(); }
      if (e.code === "ShiftRight") { r = false; reset(); }
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, [appType]);

  // ---------- module-0 global capture guard (Click Outside, unblockable) ----------
  // CAPTURE-phase pointerDown runs BEFORE any bubbling stopPropagation in the
  // tree, so no floating layer can ever intercept the blank-click dismissal.
  // Any left click whose target is NOT one of the floating panels themselves
  // (context menu / node menu / edge popover / inspector) and NOT a text frame
  // force-destroys the global activation state machine. Per-component fade-out
  // protocols then run through the `variable:mm-dismiss-all` broadcast.
  useEffect(() => {
    const EXCLUDE = ".ctx-menu,.node-menu,.edge-pop,.inspector,.mm-node";
    const onPointerDown = (e: PointerEvent): void => {
      if (e.button !== 0) return;
      const t = e.target as HTMLElement | null;
      if (!t || !t.closest) return;
      if (t.closest(EXCLUDE)) return;
      resetGlobalCanvasInteraction();
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => window.removeEventListener("pointerdown", onPointerDown, true);
  }, []);

  // ---------- boot (runs only after the real loading sequence finished) ----------
  useEffect(() => {
    if (bootPhase === "loading") return;
    (async () => {
      try {
        const s = await loadSettings();
        setSettingsState(s);
        // S-1：开机恢复防截屏模式（Rust 侧看护线程负责补打新窗口）
        if (s.privacyShield && isTauriRuntime()) {
          await ipc.shieldSet(true).catch(() => {});
        }
        document.documentElement.lang = s.language === "en" ? "en" : s.language === "zh-TW" ? "zh-TW" : "zh-CN";
        const info = await ipc.bootstrap().catch((e) => {
          // Browser dev mode (`npm run dev`) has no Tauri IPC backend — that is
          // expected, not an error. Only surface real failures inside Tauri.
          if (!isTauriRuntime()) console.info("[bootstrap] no Tauri backend, running in browser mode");
          else pushToast("error", "Bootstrap failed", errMessage(e).message);
          return null;
        });
        setBoot(info);
        // restore last opened doc (Write 窗口语义；其余窗口非致命)
        try {
          const raw = await ipc.getSettings();
          const lastDoc = raw["lastDocId"];
          if (lastDoc && appType !== "desktop") {
            const d = await ipc.getDocument(lastDoc).catch(() => null);
            if (d && !d.deletedAt) uiStore.setState({ currentDocId: d.id });
          }
        } catch { /* non-fatal */ }
        // recovery files?
        try {
          if (appType !== "desktop") {
            const rec = await ipc.listRecoveryFiles();
            if (rec.length > 0) uiStore.setState({ recoveryPrompt: rec });
          }
        } catch { /* non-fatal */ }
      } finally {
        setReady(true);
        const splash = document.getElementById("boot-splash");
        splash?.classList.add("done");
        setTimeout(() => splash?.remove(), 500);
      }
    })();
  }, [bootPhase, appType]);

  // B-33：--force-raster 标记 → 强制软件渲染（safeMode）
  useEffect(() => {
    ipc
      .diagFlags()
      .then((f) => {
        if (f.forceRaster) setSettingsState((prev) => (prev && !prev.safeMode ? { ...prev, safeMode: true } : prev));
      })
      .catch(() => {});
  }, []);
  const patchSettings = useCallback((patch: Partial<Settings>) => {
    setSettingsState((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...patch };
      for (const [k, v] of Object.entries(patch)) {
        void saveSetting(k as keyof Settings & string, v).catch(() => {});
      }
      if (patch.language) document.documentElement.lang = patch.language === "en" ? "en" : "zh-CN";
      return next;
    });
  }, []);

  const i18n = useMemo(
    () => ({
      lang: settings?.language ?? "zh",
      setLang: (l: Lang) => patchSettings({ language: l }),
      t: makeT(settings?.language ?? "zh"),
    }),
    [settings?.language, patchSettings],
  );
  // E-3 残留报告文案（Provider 在渲染期才包裹，本组件内直接取 context 值）
  const tI18n = useI18n().t;

  // ---------- apply theme/ui CSS variables ----------
  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    root.dataset.theme = settings.theme;
    // A-2.3：reduceMotion / 性能低档位 → 全部动效降级 80ms（动效是增益不是依赖）
    root.dataset.reduceMotion = String(!!settings.reduceMotion);
    root.style.setProperty("--editor-font", settings.fontFamily);
    root.style.setProperty("--editor-font-size", `${settings.fontSize}px`);
    root.style.setProperty("--editor-line-height", String(settings.lineHeight));
    root.style.setProperty("--ui-zoom", String(settings.uiZoom));
    const appEl = document.getElementById("app-root");
    if (appEl) (appEl as HTMLElement).style.zoom = String(settings.uiZoom);
  }, [settings]);

  // persist last opened doc
  useEffect(() => {
    if (currentDocId) void ipc.setSettings({ lastDocId: currentDocId }).catch(() => {});
  }, [currentDocId]);

  // ---------- global shortcuts ----------
  useEffect(() => {
    if (!settings) return;
    // Mind map canvas can patch mind-defaults (grid mode etc.) via this event.
    const onMindPatch = (e: Event): void => {
      const patch = (e as CustomEvent<Record<string, unknown>>).detail;
      setSettingsState((prev) => {
        if (!prev) return prev;
        const next = { ...prev, mindDefaults: { ...prev.mindDefaults, ...patch } };
        void saveSetting("mindDefaults", next.mindDefaults).catch(() => {});
        return next;
      });
    };
    window.addEventListener("variable:mind-defaults-patch", onMindPatch);
    const onKey = async (e: KeyboardEvent): Promise<void> => {
      const mod = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      if (mod && !e.shiftKey && k === "n") {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("variable:new-doc"));
      } else if (mod && e.shiftKey && k === "f") {
        e.preventDefault();
        uiStore.setState({ searchOpen: true });
      } else if (mod && k === ",") {
        e.preventDefault();
        uiStore.setState({ settingsOpen: true });
      } else if (mod && e.shiftKey && k === "m" && appType === "desktop") {
        // 拆窗后 write↔mindmap 切换仅是桌面窗口的过渡残留（M4 后桌面窗口不会进入应用视图）。
        e.preventDefault();
        uiStore.setState((s) => ({ mode: s.mode === "write" ? "mindmap" : "write" }));
      } else if (e.key === "F11") {
        // 桌面环境常驻全屏：F11 仅切换专注模式（隐藏 UI 元素），不再退出全屏。
        e.preventDefault();
        const next = !uiStore.getState().focusMode;
        uiStore.setState({ focusMode: next });
      } else if (e.key === "Escape" && uiStore.getState().focusMode) {
        uiStore.setState({ focusMode: false });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("variable:mind-defaults-patch", onMindPatch);
    };
  }, [settings, appType]);

  // ---------- guarded close flow ----------
  // 桌面窗口 = 退出整个 Variable（先关全部软件窗口）；软件窗口红灯 = 只关自己。
  // 批次E-3：desktop 退出前先走 exit_prepare（checkpoint → 断代理 → 残留扫描）；
  // 非零残留弹报告，用户可「我知道风险」跳过（二次确认）或返回。
  const exitResidueBypassRef = useRef(false);
  const [exitResidues, setExitResidues] = useState<{ path: string; kind: string; size: number }[] | null>(null);
  const requestClose = useCallback(async (bypassResidueCheck = false) => {
    setClosePhase("flushing");
    try {
      window.dispatchEvent(new CustomEvent("variable:flush-save"));
      // give the editor's flush handler a moment to complete its IPC round-trip
      await new Promise((r) => setTimeout(r, 350));
      setClosePhase("idle");
      if (appType === "desktop") {
        // 批次W-1 退出会话：全部嵌入窗口先收 WM_CLOSE（应用自行退出，30s
        // 超时者由后端脱离留在桌面，绝不强杀），再关壳层窗口
        await ipc.embedCloseAll().catch(() => {});
        // 批次E-3：容器 checkpoint → 断代理 → 残留扫描；0 残留静默通过
        if (!bypassResidueCheck && !exitResidueBypassRef.current) {
          const prep = await ipc.exitPrepare().catch(() => null);
          if (prep && prep.residues.length > 0) {
            setExitResidues(prep.residues.map((r) => ({ path: r.path, kind: r.kind ?? "file", size: r.size ?? 0 })));
            return; // 暂停退出，等用户在报告里决定
          }
        }
        const wins = await getAllWebviewWindows();
        await Promise.all(
          wins.filter((w) => w.label.startsWith("app-")).map((w) => w.destroy().catch(() => {})),
        );
      }
      await getCurrentWindow().destroy();
    } catch (e) {
      console.error("close flow failed", e);
      setClosePhase("failed");
    }
  }, [appType]);

  // E-3 残留报告：继续退出（跳过扫描）或返回
  const exitResidueProceed = useCallback(() => {
    exitResidueBypassRef.current = true;
    setExitResidues(null);
    void requestClose(true);
  }, [requestClose]);
  const exitResidueCancel = useCallback(() => setExitResidues(null), []);

  // 批次E-18：Del+Backspace（Rust 侧检测）→ 真正退出：直接走保存冲刷+关闭
  useEffect(() => {
    if (appType !== "desktop" || !isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const sub = listen("sys://quit-request", () => {
      if (!disposed) void requestClose();
    });
    void sub
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appType, requestClose]);

  // beforeunload best-effort recovery write is handled in EditorView via flush event.

  // ---------- 启动仪式 + 阶段4/5 退出编排 ----------
  // BootScreen 全程保持挂载（同一 fragment 子位），不重挂、不重播；
  // exit 期桌面 shell 在其下提前挂载（字母落位与任务栏展开交叠）。
  if (view === "desktop" || bootPhase !== "done") {
    const desktopReady = Boolean(settings && ready) && view === "desktop";
    return (
      <>
        {desktopReady && settings ? (
          <I18nContext.Provider value={i18n}>
            <DesktopShell
              settings={settings}
              entering={bootPhase === "exit"}
              bootStats={bootStats}
              closePhase={closePhase}
              onCloseRequested={() => void requestClose()}
              onOpenApp={(app) => openVwmApp(app)}
              onOpenSettings={() => uiStore.setState({ settingsOpen: true, settingsTab: "appearance", startOpen: false })}
              onPatchSettings={patchSettings}
            />
            <SearchOverlay />
            <SettingsModal settings={settings} onChange={patchSettings} bootstrap={boot} />
            {settings && boot && (
              <OobeGate settings={settings} onDone={patchSettings} dataDir={boot.dataDir} />
            )}
            <ToastHost />
            <ContextMenuHost />
            <ConfirmHost />
            <ChoiceHost />
            <PromptHost />
            <ConfirmBubbleHost />
            <NetConsentHost />
            {/* 批次E-3：退出残留报告（非零残留才出现；可跳过=「我知道风险」） */}
            {exitResidues && (
              <Modal open onClose={exitResidueCancel} title={tI18n("exitResidueTitle")} width={560}>
                <div className="backup-list" style={{ maxHeight: 260, overflowY: "auto", marginBottom: 12 }}>
                  {exitResidues.map((r) => (
                    <div key={r.path} className="backup-row" style={{ gap: 6 }}>
                      <span className="dim small">{r.kind === "reg" ? "🔑" : "📄"}</span>
                      <span className="ellipsis small" title={r.path}>{r.path}</span>
                    </div>
                  ))}
                </div>
                <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                  <button type="button" className="btn ghost" onClick={exitResidueCancel}>
                    {tI18n("exitResidueReturn")}
                  </button>
                  <button type="button" className="btn danger" onClick={exitResidueProceed}>
                    {tI18n("exitResidueSkip")}
                  </button>
                </div>
              </Modal>
            )}
          </I18nContext.Provider>
        ) : bootPhase !== "done" || !settings || !ready ? (
          <div className="boot-hold" aria-busy="true" />
        ) : null}
        {bootPhase !== "done" && (
          <BootScreen
            onExitStart={() => setBootPhase((p) => (p === "loading" ? "exit" : p))}
            onStats={setBootStats}
            onDone={() => setBootPhase("done")}
          />
        )}
      </>
    );
  }

  if (!settings || !ready) {
    return <div className="boot-hold" aria-busy="true" />;
  }

  return (
    <I18nContext.Provider value={i18n}>
      <CosmicBackground
        theme={settings.theme}
        perfMode={settings.perfMode}
        bgTier={settings.bgTier}
        reduceMotion={settings.reduceMotion}
        safeMode={settings.safeMode}
        editing={editing || closePhase !== "idle"}
        customBg={settings.customBg}
      />
      <div id="app-root" className={`app-root theme-${settings.theme} ${focusMode ? "focus" : ""}`}>
        <TitleBar
          onCloseRequested={() => void requestClose()}
          closePhase={closePhase}
          appType={appType}
        />
        <div className="main-row">
          <Sidebar />
          <div className="content-area">
            {view === "write" ? (
              <EditorView
                key={currentDocId ?? "empty"}
                settings={{
                  fontFamily: settings.fontFamily,
                  fontSize: settings.fontSize,
                  lineHeight: settings.lineHeight,
                  widthPct: settings.editorWidthPct,
                  align: settings.editorAlign,
                  autosaveDelayMs: settings.autosaveDelayMs,
                  showStatusBar: settings.showStatusBar,
                }}
              />
            ) : view === "project" ? (
              <>
                <ProjectAnalysisView settings={settings} />
                {/* 批次C（规格 5.7.3）：Code 引用 Write 技术文档的面板 */}
                <CodeXrefPanel />
              </>
            ) : view === "fate" ? (
              <FateView />
            ) : (
              <MindmapView settings={settings} />
            )}
          </div>
        </div>
        <SearchOverlay />
        <SettingsModal settings={settings} onChange={patchSettings} bootstrap={boot} />
        {settings && boot && (
          <OobeGate settings={settings} onDone={patchSettings} dataDir={boot.dataDir} />
        )}
        {closePhase === "failed" && (
          <Modal
            open
            title={i18n.t("saveFailed")}
            onClose={() => setClosePhase("idle")}
            footer={
              <div className="row end gap8">
                <button type="button" className="btn ghost" onClick={() => setClosePhase("idle")}>{i18n.t("cancelClose")}</button>
                <button type="button" className="btn primary" onClick={() => void requestClose()}>{i18n.t("retrySave")}</button>
                <button type="button" className="btn danger" onClick={() => void getCurrentWindow().destroy()}>{i18n.t("forceClose")}</button>
              </div>
            }
          >
            <p className="confirm-body">{i18n.t("closeSaveFailed")}</p>
          </Modal>
        )}
        <RecoveryPromptHost />
        <ContextMenuHost />
        <ConfirmBubbleHost />
        <ConfirmHost />
        <ChoiceHost />
        <PromptHost />
        <ToastHost />
      </div>
    </I18nContext.Provider>
  );
}

// ---------- recovery prompt ----------
function RecoveryPromptHost(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const entries = useUi((s) => s.recoveryPrompt);
  const [detail, setDetail] = useState<{ title: string; html: string } | null>(null);
  const first = entries?.[0];
  if (!entries || entries.length === 0 || !first) return null;

  async function restore(): Promise<void> {
    if (!first) return;
    try {
      const docId = await ipc.recoverToDocument(first.id);
      uiStore.setState({ recoveryPrompt: null, currentDocId: docId, mode: "write" });
      pushToast("success", lang !== "en" ? "已恢复为记录" : "Recovered as a record");
    } catch (e) {
      pushToast("error", lang !== "en" ? "恢复失败" : "Recover failed", errMessage(e).message);
    }
  }

  return (
    <Modal open onClose={() => uiStore.setState({ recoveryPrompt: null })} title={t("recoveryPromptTitle")} width={520}
      footer={
        <div className="row end gap8">
          <button type="button" className="btn ghost danger" onClick={() => void ipc.deleteRecoveryFile(first.id).then(() => uiStore.setState({ recoveryPrompt: null })).catch(() => {})}>
            {t("recoverDelete")}
          </button>
          <button type="button" className="btn ghost" onClick={() => uiStore.setState({ recoveryPrompt: null })}>
            {t("recoverIgnore")}
          </button>
          <button type="button" className="btn primary" onClick={() => void restore()}>
            {t("recoverRestore")}
          </button>
        </div>
      }
    >
      <p>
        <b>{first.title}</b>
        <span className="dim"> — {t("recoveredAt")} {new Date(first.savedAt).toLocaleString()}</span>
      </p>
      <p className="dim small ellipsis-2">{first.preview || "…"}</p>
      {entries.length > 1 && <p className="dim small">{lang !== "en" ? `还有 ${entries.length - 1} 个恢复文件，可稍后处理。` : `${entries.length - 1} more recovery file(s) remain.`}</p>}
      <button type="button" className="btn tiny ghost" onClick={() => void ipc.readRecoveryFile(first.id).then((c) => setDetail({ title: c.title, html: c.contentHtml })).catch(() => {})}>
        {lang !== "en" ? "预览内容" : "Preview"}
      </button>
      {detail && (
        <div className="recovery-preview" dangerouslySetInnerHTML={{ __html: detail.html.slice(0, 4000) }} />
      )}
    </Modal>
  );
}
