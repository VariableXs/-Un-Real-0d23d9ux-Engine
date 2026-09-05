import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { uiStore, useUi } from "../../state/uiStore";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import { CosmicBackground } from "../../features/background/CosmicBackground";
import { Sidebar } from "../../apps/write/folders/Sidebar";
import { EditorView } from "../../apps/write/editor/EditorView";
import { MindmapView } from "../../apps/mind/MindmapView";
import { ProjectAnalysisView } from "../../apps/code/ProjectAnalysisView";
import { CodeXrefPanel } from "../../apps/code/XrefPanel";
import { FateView } from "../../apps/fate/FateView";
import { focusVwmWin, isTpApp, vwmWindowTitle, type VwmApp } from "./vwm";
import { ExplorerWindow } from "../explorer/ExplorerWindow";

/**
 * 虚拟窗口的软件内容宿主：
 * 与 App.tsx 软件窗口分支完全一致的挂载形态 —— Sidebar + content-area 视图，
 * CosmicBackground（WebGL 星空/极光）原样保留（外层框架建立包含块，
 * fixed 定位的背景被约束在本窗口内，光影行为与独立窗口时期零差异）。
 * 业务组件零修改：数据、编辑器、算法、快捷键事件协议全部照旧。
 */

export function VwmAppContent(props: {
  winId: string;
  app: VwmApp;
  /** explorer 初始定位路径（VwmWin.path；null/undefined = 默认位置）。 */
  winPath?: string | null;
  settings: Settings;
}): React.ReactElement {
  const { app, settings } = props;
  // 打字时背景自动降级（与 App.tsx 软件窗口行为一致，按窗口独立跟踪）
  const [editing, setEditing] = useState(false);
  const currentDocId = useUi((s) => s.currentDocId);
  const [, force] = useState(0);

  useEffect(() => {
    const onDown = (e: KeyboardEvent): void => {
      if (e.key.length === 1 || e.key === "Backspace") {
        const ae = document.activeElement as HTMLElement | null;
        if (ae && (ae.isContentEditable || ae.tagName === "INPUT" || ae.tagName === "TEXTAREA")) {
          setEditing(true);
        }
      }
    };
    const onUp = (): void => {
      setEditing(false);
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, []);

  // 批次C（规格 5.7.3）协议延续：Code 面板引用 Write 文档 → 打开/聚焦 Write 并切到该文档。
  // 拆窗时期监听在 Write OS 窗口；虚拟窗口化后由桌面窗口内的 Write 实例承接。
  useEffect(() => {
    if (app !== "write" || !isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<{ kind: string; id: string }>("xref://focus", (ev) => {
      if (ev.payload.kind !== "write-doc") return;
      uiStore.setState({ currentDocId: ev.payload.id });
      focusVwmWin(props.winId);
      force((n) => n + 1);
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
  }, [app, props.winId]);

  // 批次E-16：第三方应用 —— 内容由 SetParent 的原生窗口呈现（位于 webview 之上），
  // 这里只铺一块透明占位，保证虚拟窗口/标题栏/贴靠体系一致。
  if (isTpApp(app)) {
    return <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(app)} />;
  }

  // 系统窗口（文件管理器/回收站）：内嵌模式复用 ExplorerWindow 视图本体，
  // 标题栏/几何记忆/全局宿主由 VWM 框架与桌面壳层接管（业务逻辑零修改）。
  if (app === "explorer" || app === "recycle") {
    return (
      <div className="vwm-app vwm-sys">
        <ExplorerWindow
          embedded
          initialView={app}
          initialPath={props.winPath ?? undefined}
        />
      </div>
    );
  }

  const bg = (
    <CosmicBackground
      theme={settings.theme}
      perfMode={settings.perfMode}
      bgTier={settings.bgTier}
      reduceMotion={settings.reduceMotion}
      safeMode={settings.safeMode}
      editing={editing}
      customBg={settings.customBg}
    />
  );

  return (
    <div className="vwm-app">
      {bg}
      <div className="app-root vwm-app-root">
        <div className="main-row">
          <Sidebar />
          <div className="content-area">
            {app === "write" ? (
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
            ) : app === "project" ? (
              <>
                <ProjectAnalysisView settings={settings} />
                <CodeXrefPanel />
              </>
            ) : app === "fate" ? (
              <FateView />
            ) : (
              <MindmapView settings={settings} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
