import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n, I18nContext, makeT } from "./i18n";
import { ipc, errMessage } from "./lib/ipc";
import { loadSettings, saveSetting, type Settings } from "./lib/settings";
import { uiStore, pushToast, resetGlobalCanvasInteraction, useUi } from "./state/uiStore";
import type { BootstrapInfo } from "./lib/types";

/** True only inside the real Tauri webview — the dev-mode stub in main.tsx
 *  marks itself so `vite dev` in a plain browser never reports IPC failures. */
function isTauriRuntime(): boolean {
  const internals = (window as { __TAURI_INTERNALS__?: { __variableDevStub?: boolean } }).__TAURI_INTERNALS__;
  return typeof window !== "undefined" && !!internals && internals.__variableDevStub !== true;
}
import { ErrorBoundary } from "./components/ErrorBoundary";
import { TitleBar, type ClosePhase } from "./components/TitleBar";
import { ToastHost } from "./components/ToastHost";
import { ContextMenuHost } from "./components/ContextMenu";
import { ConfirmBubbleHost, ConfirmHost, PromptHost, Modal } from "./components/Modal";
import { CosmicBackground } from "./features/background/CosmicBackground";
import { StartupAnimation } from "./features/startup/StartupAnimation";
import { Sidebar } from "./features/folders/Sidebar";
import { EditorView } from "./features/editor/EditorView";
import { MindmapView } from "./features/mindmap/MindmapView";
import { ProjectAnalysisView } from "./features/projectviz/ProjectAnalysisView";
import { FateView } from "./features/fate/FateView";
import { SearchOverlay } from "./features/search/SearchOverlay";
import { SettingsModal } from "./features/settings/SettingsModal";

export default function App(): React.ReactElement {
  return (
    <ErrorBoundary>
      <AppInner />
    </ErrorBoundary>
  );
}

function AppInner(): React.ReactElement {
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [boot, setBoot] = useState<BootstrapInfo | null>(null);
  const [closePhase, setClosePhase] = useState<ClosePhase>("idle");
  const [ready, setReady] = useState(false);
  const [editing, setEditing] = useState(false);
  const editingTimer = useRef<number>(0);
  const mode = useUi((s) => s.mode);
  const focusMode = useUi((s) => s.focusMode);
  const immersive = useUi((s) => s.immersive);
  const currentDocId = useUi((s) => s.currentDocId);

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

  // ---------- boot ----------
  useEffect(() => {
    (async () => {
      try {
        const s = await loadSettings();
        setSettingsState(s);
        document.documentElement.lang = s.language === "en" ? "en" : "zh-CN";
        const info = await ipc.bootstrap().catch((e) => {
          // Browser dev mode (`npm run dev`) has no Tauri IPC backend — that is
          // expected, not an error. Only surface real failures inside Tauri.
          if (!isTauriRuntime()) console.info("[bootstrap] no Tauri backend, running in browser mode");
          else pushToast("error", "Bootstrap failed", errMessage(e).message);
          return null;
        });
        setBoot(info);
        // restore last opened doc
        try {
          const raw = await ipc.getSettings();
          const lastDoc = raw["lastDocId"];
          if (lastDoc) {
            const d = await ipc.getDocument(lastDoc).catch(() => null);
            if (d && !d.deletedAt) uiStore.setState({ currentDocId: d.id });
          }
        } catch { /* non-fatal */ }
        // recovery files?
        try {
          const rec = await ipc.listRecoveryFiles();
          if (rec.length > 0) uiStore.setState({ recoveryPrompt: rec });
        } catch { /* non-fatal */ }
      } finally {
        setReady(true);
        const splash = document.getElementById("boot-splash");
        splash?.classList.add("done");
        setTimeout(() => splash?.remove(), 500);
      }
    })();
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
      setLang: (l: "zh" | "en") => patchSettings({ language: l }),
      t: makeT(settings?.language ?? "zh"),
    }),
    [settings?.language, patchSettings],
  );

  // ---------- apply theme/ui CSS variables ----------
  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    root.dataset.theme = settings.theme;
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
    // 沉浸模式开关（Ctrl+Shift+H）：用捕获阶段注册 —— 节点编辑框等子层会
    // stopPropagation 冒泡事件，捕获阶段永远先到达，编辑中也能一键进出。
    const onImmersiveKey = (e: KeyboardEvent): void => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "h") {
        e.preventDefault();
        uiStore.setState((s) => ({ immersive: !s.immersive }));
      }
    };
    window.addEventListener("keydown", onImmersiveKey, true);
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
      } else if (mod && e.shiftKey && k === "m") {
        e.preventDefault();
        uiStore.setState((s) => ({ mode: s.mode === "write" ? "mindmap" : "write" }));
      } else if (e.key === "F11") {
        e.preventDefault();
        const next = !uiStore.getState().focusMode;
        uiStore.setState({ focusMode: next });
        void getCurrentWindow().setFullscreen(next).catch(() => {});
      } else if (e.key === "Escape" && uiStore.getState().focusMode) {
        uiStore.setState({ focusMode: false });
        void getCurrentWindow().setFullscreen(false).catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keydown", onImmersiveKey, true);
      window.removeEventListener("variable:mind-defaults-patch", onMindPatch);
    };
  }, [settings]);

  // ---------- guarded close flow ----------
  const requestClose = useCallback(async () => {
    setClosePhase("flushing");
    try {
      window.dispatchEvent(new CustomEvent("variable:flush-save"));
      // give the editor's flush handler a moment to complete its IPC round-trip
      await new Promise((r) => setTimeout(r, 350));
      setClosePhase("idle");
      await getCurrentWindow().destroy();
    } catch (e) {
      console.error("close flow failed", e);
      setClosePhase("failed");
    }
  }, []);

  // beforeunload best-effort recovery write is handled in EditorView via flush event.

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
      <div id="app-root" className={`app-root theme-${settings.theme} ${focusMode ? "focus" : ""} ${immersive ? "immersive" : ""}`}>
        {!immersive && <TitleBar onCloseRequested={() => void requestClose()} closePhase={closePhase} />}
        {immersive && (
          <button
            type="button"
            className="immersive-exit"
            aria-label={i18n.lang === "zh" ? "退出沉浸模式 (Ctrl+Shift+H)" : "Exit immersive (Ctrl+Shift+H)"}
            title={i18n.lang === "zh" ? "退出沉浸模式 (Ctrl+Shift+H)" : "Exit immersive (Ctrl+Shift+H)"}
            onClick={() => uiStore.setState({ immersive: false })}
          >
            ⤢
          </button>
        )}
        <div className="main-row">
          <Sidebar />
          <div className="content-area">
            {mode === "write" ? (
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
            ) : mode === "project" ? (
              <ProjectAnalysisView settings={settings} />
            ) : mode === "fate" ? (
              <FateView />
            ) : (
              <MindmapView settings={settings} />
            )}
          </div>
        </div>
        <SearchOverlay />
        <SettingsModal settings={settings} onChange={patchSettings} bootstrap={boot} />
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
        <PromptHost />
        <ToastHost />
      </div>
      <StartupAnimation enabled={settings.launchAnim && !settings.safeMode && !settings.reduceMotion} />
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
      pushToast("success", lang === "zh" ? "已恢复为记录" : "Recovered as a record");
    } catch (e) {
      pushToast("error", lang === "zh" ? "恢复失败" : "Recover failed", errMessage(e).message);
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
      {entries.length > 1 && <p className="dim small">{lang === "zh" ? `还有 ${entries.length - 1} 个恢复文件，可稍后处理。` : `${entries.length - 1} more recovery file(s) remain.`}</p>}
      <button type="button" className="btn tiny ghost" onClick={() => void ipc.readRecoveryFile(first.id).then((c) => setDetail({ title: c.title, html: c.contentHtml })).catch(() => {})}>
        {lang === "zh" ? "预览内容" : "Preview"}
      </button>
      {detail && (
        <div className="recovery-preview" dangerouslySetInnerHTML={{ __html: detail.html.slice(0, 4000) }} />
      )}
    </Modal>
  );
}
