import { lazy, memo, Suspense, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { uiStore, useUi } from "../../state/uiStore";
import type { Settings } from "../../lib/settings";
import { isTauriRuntime } from "../../entries/runtime";
import { CosmicBackground } from "../../features/background/CosmicBackground";
import { Sidebar } from "../../apps/write/folders/Sidebar";
import { closeVwmWin, focusVwmWin, isEngineApp, isTpApp, isVwmTool, engineSessionOf, tpIdOf, vwmWindowTitle, type VwmApp } from "./vwm";
import {
  useEmbedSessionState,
  clearEmbedSessionState,
  type EmbedSessionState,
} from "./embedState";
import {
  ENGINE_BOOT_ESTIMATE,
  ENGINE_BOOT_STAGES,
  ENGINE_VWM_SUPPORT,
} from "../engine/engineModel";
import {
  cancelEngineApp,
  useEngineSession,
} from "../engine/engineSessions";
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
// C 桌面体验域·后段 AI-D2 三件：终端 2.0（F095/F096）/ 画图件（F103）/ 相册（F105）
import { TermApp } from "../tools/TermApp";
import { PaintApp } from "../tools/PaintApp";
import { AlbumApp } from "../tools/AlbumApp";
import { TaskManApp } from "../taskman/TaskManApp";

// 代码分割（性能）：VWM 内嵌四个重软件视图原本静态打包进环境主 chunk，
// 与 App.tsx 同步改为按需加载 —— 只有用户真正打开对应窗口时才拉取其代码
// （含富文本编辑器等重依赖），环境启动不再为空载这些视图付出体积与内存。
const EditorView = lazy(() =>
  import("../../apps/write/editor/EditorView").then((m) => ({ default: m.EditorView })),
);
const MindmapView = lazy(() =>
  import("../../apps/mind/MindmapView").then((m) => ({ default: m.MindmapView })),
);
const ProjectAnalysisView = lazy(() =>
  import("../../apps/code/ProjectAnalysisView").then((m) => ({ default: m.ProjectAnalysisView })),
);
const CodeXrefPanel = lazy(() =>
  import("../../apps/code/XrefPanel").then((m) => ({ default: m.CodeXrefPanel })),
);
const FateView = lazy(() =>
  import("../../apps/fate/FateView").then((m) => ({ default: m.FateView })),
);

/**
 * 虚拟窗口的软件内容宿主：
 * 与 App.tsx 软件窗口分支完全一致的挂载形态 —— Sidebar + content-area 视图，
 * CosmicBackground（WebGL 星空/极光）原样保留（外层框架建立包含块，
 * fixed 定位的背景被约束在本窗口内，光影行为与独立窗口时期零差异）。
 * 业务组件零修改：数据、编辑器、算法、快捷键事件协议全部照旧。
 */

export const VwmAppContent = memo(function VwmAppContent(props: {
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

  // 阶段6（任务50/51）：引擎流窗口 —— 拉起协议占位卡（复用 embed 占位卡视觉语言）
  // + 冷启动阶段化叙事 + 能力对照表。就绪后画面流由引擎代理回传（Hyper-V 底座实机链路）。
  if (isEngineApp(app)) {
    return <EngineStreamPane winId={props.winId} app={app} />;
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
        {/* C 桌面体验域·后段 AI-D2 三件（F095/F096 · F103 · F105） */}
        {app === "term2" && <TermApp winId={props.winId} />}
        {app === "paint" && <PaintApp winId={props.winId} />}
        {app === "album" && <AlbumApp winId={props.winId} />}
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
            <Suspense fallback={null}>
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
            </Suspense>
          </div>
        </div>
      </div>
    </div>
  );
});

/**
 * 批次W-3：第三方占位层 —— running 时透明占位；orphaned 时占位卡。
 * M2（R9）：exited 事件在监护层直接自动关占位窗（R7 硬约束），failed 态与
 * 「框选窗口」兜底已下线（捕获失败在 launchThirdApp 层静默关窗）。
 * 占位卡复用嵌入失败占位视觉语言（如实状态 + 动作），绝不自动重启进程。
 */
function TpPlaceholder(props: {
  winId: string;
  app: VwmApp;
  state: EmbedSessionState;
}): React.ReactElement {
  const { t } = useI18n();
  const tpId = tpIdOf(props.app);
  if (props.state === "running") {
    // M3：纯透明占位 —— 已收编窗口是完整原生顶层窗（拥有式嵌入），Variable
    // 不画任何内容；此 div 仅承载 aria-label 与命中测试语义。
    return <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)} />;
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
  // M2（R9）：failed 态与「框选窗口」手动收编兜底已随 C-2 通道下线 ——
  // 捕获失败在 launchThirdApp 层静默关窗，不再产生 failed 占位卡。
  const message = props.state === "orphaned" ? t("tpEmbedOrphaned", { name }) : t("tpEmbedExited", { name });
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
          <button type="button" className="btn" onClick={() => closeVwmWin(props.winId)}>
            {t("tpEmbedDismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * 阶段6（任务50/51）：引擎流窗口内容层。
 * - starting → 冷启动阶段化叙事卡（命名阶段逐个点亮 + 预期等待 20-40s 如实提示 +
 *   取消路径）；进度只按「已到达阶段」推进，绝不按时间伪造百分比。
 * - ready → 画面流承载占位（真实流媒体由 Hyper-V 底座实机链路回传；此处预留
 *   流协议抽象挂点，协议可替换 RDP/Spice）。
 * - crashed / closed → 如实状态卡（原因 + 动作），绝不自动重启引擎。
 * - 能力对照表（任务50 完善性）：哪些 VWM 特性引擎窗不支持，如实列出。
 */
function EngineStreamPane(props: { winId: string; app: VwmApp }): React.ReactElement {
  const appKey = engineSessionOf(props.app);
  const session = useEngineSession();
  const stageIdx = session.stage ? ENGINE_BOOT_STAGES.findIndex((s) => s.key === session.stage) : -1;
  if (session.lifecycle === "starting") {
    return (
      <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)}>
        <div className="vwm-tp-card" role="status">
          <p className="vwm-tp-card-msg">正在拉起「{appKey}」的 Windows 引擎…</p>
          <ol className="engine-boot-stages">
            {ENGINE_BOOT_STAGES.map((s, i) => (
              <li key={s.key} className={i <= stageIdx ? "engine-stage done" : "engine-stage"}>
                {i <= stageIdx ? "✓" : "·"} {s.label}
              </li>
            ))}
          </ol>
          <p className="vwm-tp-card-msg engine-eta">
            预期等待 {ENGINE_BOOT_ESTIMATE.minS}-{ENGINE_BOOT_ESTIMATE.maxS} 秒（首次冷启动较慢，实际以事件推进为准）
          </p>
          <div className="vwm-tp-card-actions">
            <button
              type="button"
              className="btn"
              onClick={() => {
                cancelEngineApp(appKey);
                closeVwmWin(props.winId);
              }}
            >
              取消等待
            </button>
          </div>
        </div>
      </div>
    );
  }
  if (session.lifecycle === "ready") {
    // 就绪：画面流承载面（流协议抽象挂点 —— RDP/Spice 可替换，任务53 延迟探针在此打点）。
    return (
      <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)}>
        <div className="vwm-tp-card" role="status">
          <p className="vwm-tp-card-msg">引擎就绪 · 画面流连接中（{appKey}）</p>
          <div className="vwm-tp-card-actions">
            <button
              type="button"
              className="btn primary"
              onClick={() => {
                // S3.3 v1：画质档取设置总线（engineQuality），全屏 RDP 会话；
                // mstsc 窗口出现后由 embed 管线收编进本窗位（复用既有语义）。
                void (async () => {
                  try {
                    const s = await import("../../lib/settings").then((m) => m.loadSettings());
                    const r = await ipc.engineStreamOpen(appKey, s.engineQuality, "desktop");
                    pushToast("info", `画面流已发起（${r.quality} 档 · 全屏），连接窗口将自动收编`);
                  } catch (e) {
                    const m = errMessage(e);
                    pushToast("error", m.message);
                  }
                })();
              }}
            >
              连接画面流（全屏 · RDP）
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => {
                void ipc.engineStreamClose(appKey).catch(() => {});
              }}
            >
              断开画面流
            </button>
          </div>
          <details className="engine-cap-table">
            <summary>引擎窗口能力说明</summary>
            <ul>
              {ENGINE_VWM_SUPPORT.map((f) => (
                <li key={f.feature}>
                  {f.supported ? "✓" : "✕"} {f.feature} —— {f.note}
                </li>
              ))}
            </ul>
          </details>
        </div>
      </div>
    );
  }
  const msg =
    session.lifecycle === "crashed"
      ? `引擎已崩溃：${session.reason ?? "未知原因"}。Variable 桌面不受影响。`
      : session.reason
        ? `引擎会话已结束：${session.reason}`
        : "引擎未运行。";
  return (
    <div className="vwm-app vwm-tp" aria-label={vwmWindowTitle(props.app)}>
      <div className="vwm-tp-card" role="status">
        <p className="vwm-tp-card-msg">{msg}</p>
        <div className="vwm-tp-card-actions">
          <button type="button" className="btn" onClick={() => closeVwmWin(props.winId)}>
            关闭占位
          </button>
        </div>
      </div>
    </div>
  );
}
