import { useEffect, useRef, useState } from "react";
import { getCurrentWindow, getAllWindows } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { HardDrive, X } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast, uiStore, useUi, type AppMode, type QuickSection } from "../../state/uiStore";
import { openQuickPanel } from "../../state/uiStore";
import { pushNotify, toggleDnd } from "../../state/notifyStore";
import type { ClosePhase } from "../../components/TitleBar";
import type { BootStats } from "../boot/BootScreen";
import { WallpaperLayer } from "../wallpaper/WallpaperLayer";
import { WintabSwitcher } from "../windows/WintabSwitcher";
import { DesktopIcons } from "../desktop-icons/DesktopIcons";
import { Taskbar } from "../taskbar/Taskbar";
import { StartMenu } from "../startmenu/StartMenu";
import { PrivacyBanner } from "../tray/PrivacyBanner";
import { LauncherManager } from "../launcher/LauncherManager";
import { WelcomeWizard } from "../welcome/WelcomeWizard";
import { getThirdApps, launchThirdApp, reloadThirdApps } from "../launcher/thirdApps";
import { openVwmApp, openVwmSystem } from "../windows/vwm";
import { VirtualWindowManager } from "../windows/VirtualWindowManager";
import { applySnap, SnapPreviewHost } from "../windows/snap";
import { pushRecent } from "../startmenu/recent";
import { effectiveBinds } from "../../lib/shortcuts";

/**
 * 桌面环境 shell（L0+L1，M3 形态）：
 * 全屏覆盖 Windows 桌面 → 壁纸层 + 桌面图标网格 + 右上角 Mac 风格红绿灯
 * + 底部 Win11 风格任务栏 + 开始菜单。
 * 系统托盘在 M5 加入；四款软件 M4 拆窗（当前以视图切换过渡）。
 *
 * 批次A 阶段4/5：`entering`（exit 编排期挂载）→ 任务栏从中央展开、
 * 图标交错淡入、红绿灯由暗态激活；挂载 ~1.7s 后推送一次"本地数据就绪"
 * 通知（数字来自真实 BootStats）。首次启动（wizardDone=false）显示欢迎向导。
 *
 * 红绿灯行为（桌面窗口，需求指定 绿|黄|红 顺序）：
 * - 🟢 = 退出 Variable（选择框：隐藏到托盘 / 完全退出，未保存记录先走保存冲刷）
 * - 🟡 = 全屏（最大化-还原；避让任务栏时还原为全覆盖）
 * - 🔴 = 最小化 Variable（Alt+Tab 可返回）
 */
export function DesktopShell(props: {
  settings: Settings;
  /** 批次A：true = 启动退出编排期（字母落位中），入场动画与红绿灯暗态生效。 */
  entering?: boolean;
  /** 真实启动摘要（阶段5 磁盘同步通知用；仅桌面窗口首次挂载时有值）。 */
  bootStats?: BootStats | null;
  closePhase: ClosePhase;
  onCloseRequested: () => void;
  onOpenApp: (app: AppMode) => void;
  onOpenSettings: () => void;
  onPatchSettings: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const win = getCurrentWindow();
  const startOpen = useUi((s) => s.startOpen);
  const [entered, setEntered] = useState(false);
  const notifiedRef = useRef(false);
  // 批次E-6：全屏应用运行中（任务栏/红绿灯自动避让）；U 盘拔出横幅
  const [fsApp, setFsApp] = useState(false);
  const [usbRemoved, setUsbRemoved] = useState(false);

  // 退出不再弹确认框：所有入口（红绿灯/开始菜单/托盘）直接走保存冲刷 + 关闭。
  const exitDesktop = (): void => props.onCloseRequested();

  // 批次D（规格 4.4.3）：Win+数字 → 任务栏第 n 位（文件管理器 / 四软件 / 第三方）
  const launchIndex = (n: number): void => {
    if (n === 1) {
      openVwmSystem("explorer");
      return;
    }
    const apps: AppMode[] = ["write", "mindmap", "project", "fate"];
    const idx = n - 2;
    if (idx < apps.length) {
      const app = apps[idx];
      if (app) openVwmApp(app);
      return;
    }
    const a = getThirdApps()[n - 6];
    if (a) void launchThirdApp(a.id, a.name);
  };

  const closeStart = (): void => uiStore.setState({ startOpen: false });

  // 批次E（规格 5.9.1）：拖入 exe/lnk/bat/cmd → 直接登记第三方软件。
  // Tauri v2 webview 接管拖放（HTML5 drop 不触发），走 onDragDropEvent 拿真实路径。
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((ev) => {
      if (ev.payload.type !== "drop") return;
      const paths = ev.payload.paths.filter((p) => /\.(exe|lnk|bat|cmd)$/i.test(p));
      if (paths.length === 0) return;
      void (async () => {
        let ok = 0;
        for (const p of paths) {
          try {
            await ipc.tpAdd(p);
            ok++;
          } catch (e) {
            pushToast("error", t("addApp"), errMessage(e).message);
          }
        }
        if (ok > 0) {
          await reloadThirdApps();
          pushToast("success", t("tpAdded"));
        }
      })();
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 入场编排收尾：~1.5s 后移除 .entering → 红绿灯激活为完整颜色（CSS 过渡）。
  useEffect(() => {
    if (!props.entering) return undefined;
    const id = window.setTimeout(() => setEntered(true), 1500);
    return () => window.clearTimeout(id);
  }, [props.entering]);

  // 阶段5（规格 7.0s 位）：磁盘同步通知 —— 一次性，数字全部来自真实启动摘要。
  useEffect(() => {
    if (!props.bootStats || notifiedRef.current) return undefined;
    notifiedRef.current = true;
    const s = props.bootStats;
    const id = window.setTimeout(() => {
      const body = t("diskSyncBody", { files: s.workspaceFiles, records: s.records, maps: s.mindmaps });
      pushToast("success", t("diskSyncTitle"), body);
      pushNotify("system", t("diskSyncTitle"), body);
    }, 1750);
    return () => window.clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.bootStats !== undefined && props.bootStats !== null]);

  // M5：托盘菜单"退出 Variable"→ 走与红灯一致的确认 + 保存冲刷流程。
  useEffect(() => {
    const un = win.listen("tray://quit", () => void exitDesktop());
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [win]);

  // M7：第三方软件登记（桌面窗口加载一次；增删改由各入口自行 reload）。
  useEffect(() => {
    void reloadThirdApps();
  }, []);

  // M8 拔出保护 + 批次E-7：后端监测到便携数据卷消失 → 常驻横幅 + 通知中心留痕
  useEffect(() => {
    const un = listen("usb://removed", () => {
      setUsbRemoved(true);
      pushNotify("system", t("usbDriveRemoved"), t("usbDriveRemovedBody"));
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [t]);

  // 批次E-6（规格 8.x）：全屏应用检测 → 任务栏/红绿灯自动避让（退出即恢复）
  useEffect(() => {
    const un = listen<boolean>("sys://fullscreen", (e) => setFsApp(e.payload === true));
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  // 批次E-8（规格 45 边界）：通讯软件未读提醒 —— 仅窗口标题信号，不读消息内容
  useEffect(() => {
    const un = listen<{ app: string; title: string }>("sys://im-msg", (e) => {
      const { app, title } = e.payload;
      pushNotify("system", app, title);
      pushToast("info", app, t("imNewMsgBody", { app }));
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [t]);

  // 批次E-6：每日自动换壁纸 —— 本地缓存目录按当天日期取一张（零网络），启动时应用一次
  useEffect(() => {
    if (!props.settings.wallpaperDaily || !props.settings.wallpaperPoolDir) return;
    void (async () => {
      try {
        const picked = await ipc.wpPickDaily(props.settings.wallpaperPoolDir, "date");
        if (!picked || picked === props.settings.customBg.imagePath) return;
        props.onPatchSettings({
          wallpaperMode: "image",
          customBg: { ...props.settings.customBg, type: "image", imagePath: picked },
        });
      } catch {
        /* 缓存目录不可用：如实跳过，不打扰 */
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.settings.wallpaperDaily, props.settings.wallpaperPoolDir]);

  // 批次E-6：Win+Tab 多窗口切换器（开关门控在 WintabSwitcher 内部）

  // 批次C（规格 6.1/6.2/6.3/6.5）：全局快捷键（Rust 注册）→ 快捷面板分区 / 勿扰切换。
  // 桌面窗口被最小化时事件落在隐藏窗口（面板不显示）——无残留，重开即同步。
  useEffect(() => {
    const unQp = listen<string | null>("quickpanel://open", (e) => {
      const section = (e.payload || null) as QuickSection | null;
      openQuickPanel(section);
    });
    const unDnd = listen("dnd://toggle", () => {
      const dnd = toggleDnd();
      pushToast("info", t("dndTitle"), dnd ? t("dndOn") : t("dndOff"));
    });
    return () => {
      void unQp.then((f) => f()).catch(() => {});
      void unDnd.then((f) => f()).catch(() => {});
    };
  }, [t]);

  // 批次D（规格 4.3/4.4）：Win+D / Ctrl+Shift+D / 裸 Win 键 / Win+数字 / Win+方向键
  useEffect(() => {
    const unShow = listen("sys://show-desktop", () => void win.minimize().catch(() => {}));
    const unHide = listen("sys://toggle-hide", () => {
      void (async () => {
        const vis = await win.isVisible().catch(() => true);
        if (vis) void win.hide().catch(() => {});
        else {
          void win.show().catch(() => {});
          void win.unminimize().catch(() => {});
          void win.setFocus().catch(() => {});
        }
      })();
    });
    const unWin = listen("sys://win-key", () => {
      uiStore.setState((s) => ({ startOpen: !s.startOpen }));
    });
    const unIdx = listen<number>("sys://launch-index", (e) => launchIndex(Number(e.payload)));
    const unSnap = listen<string>("sys://snap", (e) => {
      const dir = e.payload;
      if (dir === "left" || dir === "right" || dir === "up" || dir === "down") void applySnap(dir);
    });
    // 批次E-9：后端快捷键（ctrl+alt+e 等）→ 系统窗口进 VWM 虚拟窗口（不再开 OS 窗口）
    const unSys = listen<string>("sys://open-system", (e) => {
      const kind = e.payload;
      if (kind === "explorer" || kind === "recycle") openVwmSystem(kind);
    });
    return () => {
      void unShow.then((f) => f()).catch(() => {});
      void unHide.then((f) => f()).catch(() => {});
      void unWin.then((f) => f()).catch(() => {});
      void unIdx.then((f) => f()).catch(() => {});
      void unSnap.then((f) => f()).catch(() => {});
      void unSys.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [win]);

  // 批次E（规格 4.7）：整表应用快捷键（默认 + 用户覆盖）；变更即重注册。
  useEffect(() => {
    const binds = effectiveBinds(props.settings.shortcutBinds ?? {});
    void ipc
      .shortcutsApply(binds)
      .then((failed) => {
        if (failed.length > 0) pushToast("info", t("scTitle"), `${t("scRegisterFailed")}: ${failed.join(", ")}`);
      })
      .catch((e) => console.warn("[shortcuts] apply failed", errMessage(e).message));
  }, [props.settings.shortcutBinds]);

  // 批次E：Win+M → 最小化全部 Variable 窗口
  useEffect(() => {
    const un = listen("sys://minimize-all", () => {
      void (async () => {
        try {
          const wins = await getAllWindows();
          for (const w of wins) void w.minimize().catch(() => {});
        } catch {
          /* window API unavailable */
        }
      })();
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  // 批次D：启动编排结束后应用"避让任务栏"状态（编排期保持全屏，避免字母落位错位）
  useEffect(() => {
    if (props.entering) return;
    if (props.settings.avoidTaskbar) void ipc.winSetAvoidTaskbar(true).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.entering]);

  // 批次E：开始菜单"最近使用"记录（官方软件统一入口）
  const labelForApp = (app: AppMode): string =>
    app === "write"
      ? "Variable Write"
      : app === "mindmap"
        ? "Variable Mind"
        : app === "project"
          ? "Variable Code"
          : "Variable Fate";
  const onOpenApp = (app: AppMode): void => {
    closeStart();
    pushRecent("app", app, labelForApp(app));
    props.onOpenApp(app);
  };

  return (
    <div
      className={`desktop-shell${props.entering && !entered ? " entering" : ""}`}
      data-phase={props.closePhase}
      data-fullscreen={fsApp || undefined}
    >
      <WallpaperLayer settings={props.settings} />
      <DesktopIcons
        onOpenApp={onOpenApp}
        onOpenSystem={(kind) => openVwmSystem(kind)}
        onOpenSettings={props.onOpenSettings}
        iconSize={props.settings.iconSize}
        wallpaperMode={props.settings.wallpaperMode}
        wallpaperPoolDir={props.settings.wallpaperPoolDir}
        customBg={props.settings.customBg}
        onPatchSettings={props.onPatchSettings}
      />

      {/* M5 隐私核心：摄像头/麦克风被占用时顶部横幅（本机 ConsentStore 检测） */}
      <PrivacyBanner />

      {/* 批次E-7：U 盘意外拔出 —— 常驻横幅（数据卷消失后每秒都在风险中，必须显式确认） */}
      {usbRemoved && (
        <div className="privacy-banner usb-removed-banner" role="alert">
          <div className="privacy-item">
            <HardDrive size={16} strokeWidth={1.8} className="privacy-icon" />
            <div className="privacy-text">
              <b>{t("usbDriveRemoved")}</b>
              <span className="dim small">{t("usbDriveRemovedBody")}</span>
            </div>
            <button
              type="button"
              className="privacy-dismiss"
              aria-label={t("privacyDismiss")}
              title={t("privacyDismiss")}
              onClick={() => setUsbRemoved(false)}
            >
              <X size={14} />
            </button>
          </div>
        </div>
      )}

      {/* 批次E-6：Win+Tab 多窗口切换器（可选开关关闭时不启用） */}
      {props.settings.winTabSwitcher && <WintabSwitcher />}

      {/* 批次E-16：桌面红绿灯已按需求移除 —— 退出改为连按两次 Esc（确认框保留） */}
      <SnapPreviewHost />

      {/* 虚拟窗口管理器：四款软件以虚拟窗口托管于桌面层内
          （Z 序 / 聚焦 / 拖拽 / 贴靠 / 最小化到任务栏 / 右上角 Mac 红绿灯 / 同软件多开） */}
      <VirtualWindowManager settings={props.settings} />

      <StartMenu
        open={startOpen}
        onClose={closeStart}
        onOpenApp={onOpenApp}
        onOpenSettings={() => {
          closeStart();
          props.onOpenSettings();
        }}
        onOpenSearch={() => uiStore.setState({ searchOpen: true, startOpen: false })}
        onOpenLauncher={() => {
          closeStart();
          uiStore.setState({ launcherOpen: true });
        }}
        onExit={() => void exitDesktop()}
      />

      <Taskbar
        startOpen={startOpen}
        onToggleStart={() => uiStore.setState({ startOpen: !startOpen })}
        onOpenSearch={() => uiStore.setState({ searchOpen: true, startOpen: false })}
        onOpenApp={onOpenApp}
        onShowDesktop={closeStart}
        onOpenSettings={() => {
          closeStart();
          props.onOpenSettings();
        }}
        pos={props.settings.taskbarPos}
      />

      {/* M7 第三方软件管理器（模态） */}
      <LauncherManager />

      {/* 批次A：首次启动欢迎向导（exit 编排结束后出现；完成/跳过后不再显示） */}
      {!props.settings.wizardDone && !props.entering && (
        <WelcomeWizard
          currentWallpaper={props.settings.wallpaperMode}
          onPatch={(patch) => props.onPatchSettings(patch)}
        />
      )}
    </div>
  );
}
