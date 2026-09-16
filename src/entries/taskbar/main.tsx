import React, { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { convertFileSrc } from "@tauri-apps/api/core";
import { I18nContext, makeT } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { loadSettings, type Settings } from "../../lib/settings";
import { uiStore, useUi, type AppMode } from "../../state/uiStore";
import {
  forwardDesktopEvent,
  forwardExit,
  initProjectionConsumer,
  startHitmapReporting,
} from "../../state/projection";
import { openVwmApp } from "../../system/windows/vwm";
import { Taskbar } from "../../system/taskbar/Taskbar";
import { StartMenu } from "../../system/startmenu/StartMenu";
import { desktopAppLabel } from "../../system/desktop-icons/DesktopIcons";
import { reloadThirdApps } from "../../system/launcher/thirdApps";
import { pushRecent, reloadRecent } from "../../system/startmenu/recent";
import { reloadOfficial } from "../../system/launcher/official";
import { ToastHost } from "../../components/ToastHost";
import { ContextMenuHost } from "../../components/ContextMenu";
import { ConfirmBubbleHost, ChoiceHost, ConfirmHost, NetConsentHost, PromptHost } from "../../components/Modal";
import { setupEntryRuntime } from "../runtime";
import "../../styles/global.css";
import "../../styles/desktop.css";
// Win11 新版开始菜单面板（三栏棋盘）—— 必须在 desktop.css 之后加载
import "../../styles/startmenu-board.css";
import "../../styles/taskbar-window.css";

/**
 * M4-B 任务栏独立原生顶层窗入口：
 * - 后端 spawn_taskbar_window 创建全屏透明 TOPMOST+TOOLWINDOW 窗加载本页；
 * - 渲染与桌面版像素级一致的 Taskbar/StartMenu（含 QuickPanel/日历/托盘抽屉等
 *   任务栏树内浮层），状态经投影协议从桌面窗同步（state/projection.ts）；
 * - hitmap 上报驱动后端点击穿透状态机；taskbar://state 驱动收起/呼出动画。
 */

setupEntryRuntime("taskbar");

type BarMode = "shown" | "summoned" | "collapsed";

function TaskbarEntry(): React.ReactElement {
  const [settings, setSettings] = useState<Settings | null>(null);
  const startOpen = useUi((s) => s.startOpen);
  const [barMode, setBarMode] = useState<BarMode>("shown");

  // M4 诊断：barMode 写进窗口标题——外部队（QA 探针 GetWindowTextW）可直读
  // 任务栏窗前端状态机的当前视角，无需 devtools。
  useEffect(() => {
    document.title = `Variable Taskbar [${barMode}]`;
  }, [barMode]);

  // 设置装载 + 跨窗同步（桌面窗设置页变更 → settings://changed → 重载）
  useEffect(() => {
    let disposed = false;
    let un: (() => void) | undefined;
    const selfLabel = getCurrentWindow().label;
    void loadSettings().then((s) => {
      if (!disposed) setSettings(s);
    });
    const sub = listen<{ origin: string }>("settings://changed", (ev) => {
      if (ev.payload.origin === selfLabel) return;
      void loadSettings().then((s) => {
        if (!disposed) setSettings(s);
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
  }, []);

  // 主题/语言变量（与 App.tsx 桌面分支同口径；不应用 uiZoom——保持逻辑 px 与后端 hitmap 换算一致）
  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    root.dataset.theme = settings.theme;
    root.dataset.reduceMotion = String(!!settings.reduceMotion);
    root.style.setProperty("--dur-scale", String(settings.motionScale ?? 1));
    root.dataset.rtlPilot = String(!!settings.rtlPilot);
    root.lang = settings.language === "en" ? "en" : settings.language === "zh-TW" ? "zh-TW" : "zh-CN";
  }, [settings]);

  // 投影消费（快照落地 + hello 重试）+ 后端状态机事件 + hitmap 上报 + 第三方登记装载
  useEffect(() => {
    let stopped = false;
    let stopConsumer: (() => void) | undefined;
    void initProjectionConsumer().then((d) => {
      if (stopped) d();
      else stopConsumer = d;
    });

    const unState = listen<{ mode: string }>("taskbar://state", (e) => {
      const m = e.payload?.mode;
      if (m === "shown" || m === "summoned" || m === "collapsed") setBarMode(m);
    });

    const stopHitmap = startHitmapReporting(() => {
      const s = uiStore.getState();
      return s.startOpen || s.quickOpen;
    });

    void reloadThirdApps().catch(() => {});

    // 跨窗 localStorage 失效：桌面窗写入最近使用/卸载状态 → 本窗缓存重读
    const onStorage = (e: StorageEvent): void => {
      if (e.key === "variable:recent:v1") reloadRecent();
      if (e.key === "variable:apps:official:v1") reloadOfficial();
    };
    window.addEventListener("storage", onStorage);

    // 任务栏树内派发、桌面窗消费的事件 → 转发（通知动作 / 壁纸与设计中心挂载协议）
    const forwards = ["variable:notify-action", "ai04:open-feature"].map((name) => {
      const fn = (e: Event): void => forwardDesktopEvent(name, (e as CustomEvent).detail);
      window.addEventListener(name, fn);
      return () => window.removeEventListener(name, fn);
    });

    return () => {
      stopped = true;
      stopConsumer?.();
      stopHitmap();
      void unState.then((f) => f()).catch(() => {});
      window.removeEventListener("storage", onStorage);
      for (const f of forwards) f();
    };
  }, []);

  const i18n = useMemo(
    () => ({
      lang: (settings?.language ?? "zh") as Lang,
      // 任务栏窗不改语言（设置页在桌面窗，改完经 settings://changed 回流）
      setLang: (_l: Lang) => {},
      t: makeT((settings?.language ?? "zh") as Lang),
    }),
    [settings?.language],
  );

  if (!settings) {
    return <div className="boot-hold" aria-busy="true" />;
  }

  // 「每日一图」卡片（与桌面版 StartMenu 同口径：图片类壁纸取当前壁纸，其余退化渐变）
  const heroImage =
    settings.customBg.imagePath &&
    (settings.wallpaperMode === "image" ||
      settings.wallpaperMode === "living" ||
      settings.wallpaperMode === "hybrid")
      ? convertFileSrc(settings.customBg.imagePath)
      : undefined;

  const closeStart = (): void => uiStore.setState({ startOpen: false });
  const openApp = (app: AppMode): void => {
    closeStart();
    pushRecent("app", app, desktopAppLabel(app));
    openVwmApp(app); // 投影窗内自动转发桌面执行
  };
  const openSettings = (): void =>
    uiStore.setState({ startOpen: false, settingsOpen: true, settingsTab: "appearance" });

  return (
    <I18nContext.Provider value={i18n}>
      <div
        className={`taskbar-window-root${barMode === "collapsed" ? " tbw-hidden" : ""}`}
        data-mode={barMode}
        data-reduce-motion={settings.reduceMotion ? "1" : undefined}
        data-testid="taskbar-window-root"
      >
        <Taskbar
          startOpen={startOpen}
          onToggleStart={() => uiStore.setState({ startOpen: !startOpen })}
          onOpenSearch={() => uiStore.setState({ searchOpen: true, startOpen: false })}
          onOpenApp={openApp}
          onShowDesktop={closeStart}
          onOpenSettings={openSettings}
          pos={settings.taskbarPos}
          settings={settings}
        />
        <StartMenu
          open={startOpen}
          onClose={closeStart}
          onOpenApp={openApp}
          heroImage={heroImage}
          onOpenSettings={openSettings}
          onOpenSearch={(query?: string) =>
            uiStore.setState({ searchOpen: true, startOpen: false, searchInitialQuery: query ?? "" })
          }
          onOpenLauncher={() => uiStore.setState({ startOpen: false, launcherOpen: true })}
          onExit={forwardExit}
        />
        {/* 任务栏树内浮层的宿主（toast/右键菜单/确认框等；缺宿主 = 点了没反应） */}
        <ToastHost />
        <ContextMenuHost />
        <ConfirmHost />
        <ChoiceHost />
        <PromptHost />
        <ConfirmBubbleHost />
        <NetConsentHost />
      </div>
    </I18nContext.Provider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <TaskbarEntry />
  </React.StrictMode>,
);
