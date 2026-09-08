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
import { closeVwmWin, focusVwmWin, isTpApp, isVwmTool, tpIdOf, vwmWindowTitle, type VwmApp } from "./vwm";
import {
  useEmbedSessionState,
  clearEmbedSessionState,
  clearEmbedSessionAll,
  embedStateStore,
  type EmbedSessionState,
} from "./embedState";
import { getThirdApps } from "../launcher/thirdApps";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { useI18n } from "../../i18n";
import { ExplorerWindow } from "../explorer/ExplorerWindow";
import { CalculatorApp } from "../tools/CalculatorApp";
import { NotesApp } from "../tools/NotesApp";
import { CalendarApp } from "../tools/CalendarApp";
import { SnapshotApp } from "../tools/SnapshotApp";
import { ClipboardHistoryApp } from "../tools/ClipboardHistoryApp";
import { RenameApp } from "../tools/RenameApp";
import { DupeApp } from "../tools/DupeApp";
import { SpaceApp } from "../tools/SpaceApp";
import { ChecksumApp } from "../tools/ChecksumApp";
// AI-08 基础工具组六件（Z-22 时钟中心 / Z-24 Emoji / Z-25 放大镜 / Z-26 换算 / Z-27 系统信息 / V-98 打印队列）
import { ClockHubApp } from "../tools/ClockHubApp";
import { EmojiPanelApp } from "../tools/EmojiPanelApp";
import { MagnifierApp } from "../tools/MagnifierApp";
import { ConverterApp } from "../tools/ConverterApp";
import { SysInfoApp } from "../tools/SysInfoApp";
import { PrintQueueApp } from "../tools/PrintQueueApp";
import { SysHubApp } from "../tools/SysHubApp";
import { TaskManApp } from "../taskman/TaskManApp";
import { L3CaptureView } from "./L3CaptureView";

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

  // 批次W-3：嵌入会话状态（Rust 监护线程经 embed://state 上报）
  const embedState = useEmbedSessionState(props.winId);

  // 批次E-16：第三方应用 —— 内容由 SetParent 的原生窗口呈现（位于 webview 之上），
  // 这里只铺一块透明占位，保证虚拟窗口/标题栏/贴靠体系一致。
  // 批次W-3：进程退出/窗口消失 → Rust 监护线程 embed://state → 占位卡（如实状态 +
  // [重新打开] [关闭占位]），嵌入层崩溃只影响本占位卡，不波及 Shell 其它部分。
  if (isTpApp(app)) {
    return <TpPlaceholder winId={props.winId} app={app} state={embedState} />;
  }

  // F-2 实用工具：独立工具 UI（无 Sidebar，走各自样式；背景层按需铺）。
  if (isVwmTool(app)) {
    return (
      <div className="vwm-app vwm-tool">
        {app === "calc" && <CalculatorApp />}
        {app === "notes" && <NotesApp winId={props.winId} />}
        {app === "calendar" && <CalendarApp />}
        {app === "snapshot" && <SnapshotApp />}
        {app === "clipboard" && <ClipboardHistoryApp />}
        {app === "rename" && <RenameApp winId={props.winId} />}
        {app === "dupe" && <DupeApp winId={props.winId} />}
        {app === "space" && <SpaceApp winId={props.winId} />}
        {app === "checksum" && <ChecksumApp winId={props.winId} />}
        {/* AI-08 基础工具组六件（Z-22/Z-24/Z-25/Z-26/Z-27/V-98） */}
        {app === "clockhub" && <ClockHubApp winId={props.winId} />}
        {app === "emoji" && <EmojiPanelApp winId={props.winId} />}
        {app === "magnifier" && <MagnifierApp winId={props.winId} />}
        {app === "convert" && <ConverterApp winId={props.winId} />}
        {app === "sysinfo" && <SysInfoApp winId={props.winId} />}
        {app === "printqueue" && <PrintQueueApp winId={props.winId} />}
        {/* AI-11 系统集成与硬件组：系统中枢（U-43..U-48 / N-19..N-25 / V-51..V-60） */}
        {app === "syshub" && <SysHubApp winId={props.winId} />}
      </div>
    );
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

  // F-3 任务管理器：单实例系统窗口。
  if (app === "taskman") {
    return (
      <div className="vwm-app vwm-sys vwm-taskman">
        <TaskManApp />
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

/**
 * 批次W-3：第三方占位层 —— running 时透明占位；exited/orphaned/failed 时占位卡。
 * 占位卡复用嵌入失败占位视觉语言（如实状态 + 动作），绝不自动重启进程。
 * 批次C-2：failed（捕获失败）→「框选窗口」手动收编兜底（5s 内点击目标窗口）。
 */
function TpPlaceholder(props: {
  winId: string;
  app: VwmApp;
  state: EmbedSessionState;
}): React.ReactElement {
  const { t } = useI18n();
  const tpId = tpIdOf(props.app);
  if (props.state === "running") {
    // 批次C-4：画布在收到 embed-frame 帧前静默（L1/L2 零开销）；L3 会话自动激活
    // 帧合成与输入转发。
    return (
      <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)}>
        <L3CaptureView embedId={props.winId} />
      </div>
    );
  }
  const name = getThirdApps().find((a) => a.id === tpId)?.name ?? tpId;
  // 重新打开：同一占位窗口（同 embed_id）重嵌新会话；成功则复位为透明占位，
  // 失败（无法嵌入）如实 toast 并保持占位卡，绝不伪造成功。
  const reopen = (): void => {
    ipc
      .embedLaunch(tpId, props.winId)
      .then((r) => {
        if (r.attached) clearEmbedSessionState(props.winId);
        else pushToast("info", name, r.reason || "已按独立窗口运行");
      })
      .catch((e: unknown) => pushToast("error", name, errMessage(e).message));
  };
  // 批次C-2：框选窗口 —— 5s 内点击目标窗口 → embed_adopt 重父化收编；
  // 超时/未选中如实 toast，占位卡保持。收编成功 → 复位为透明占位。
  const pick = (): void => {
    pushToast("info", name, t("tpEmbedPickHint"));
    ipc
      .embedPick(5_000)
      .then((hwnd) => {
        if (!hwnd) {
          pushToast("info", name, t("tpEmbedPickTimeout"));
          return;
        }
        const meta = embedStateStore.getState().meta[props.winId];
        const rootPid = meta?.rootPid ?? 0;
        void ipc
          .embedAdopt(tpId, hwnd, rootPid, props.winId)
          .then((ok) => {
            if (ok) clearEmbedSessionAll(props.winId);
            else pushToast("info", name, t("tpEmbedPickTimeout"));
          })
          .catch((e: unknown) => pushToast("error", name, errMessage(e).message));
      })
      .catch((e: unknown) => pushToast("error", name, errMessage(e).message));
  };
  const message =
    props.state === "orphaned"
      ? t("tpEmbedOrphaned", { name })
      : props.state === "failed"
        ? t("tpEmbedFailed", { name })
        : t("tpEmbedExited", { name });
  return (
    <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)}>
      <div className="vwm-tp-card" role="status">
        <p className="vwm-tp-card-msg">{message}</p>
        <div className="vwm-tp-card-actions">
          {props.state === "exited" && (
            <button type="button" className="btn primary" onClick={reopen}>
              {t("tpEmbedReopen")}
            </button>
          )}
          {props.state === "failed" && (
            <button type="button" className="btn primary" onClick={pick}>
              {t("tpEmbedPick")}
            </button>
          )}
          <button type="button" className="btn" onClick={() => closeVwmWin(props.winId)}>
            {t("tpEmbedDismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}
