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
import type { NotifyAction } from "../../state/notifyStore";
import type { ClosePhase } from "../../components/TitleBar";
import type { BootStats } from "../boot/BootScreen";
import { WallpaperLayer } from "../wallpaper/WallpaperLayer";
import { WintabSwitcher } from "../windows/WintabSwitcher";
import { DesktopIcons } from "../desktop-icons/DesktopIcons";
import { Taskbar } from "../taskbar/Taskbar";
import { StartMenu } from "../startmenu/StartMenu";
import { PrivacyBanner } from "../tray/PrivacyBanner";
import { CompatBanner } from "../compat/CompatBanner";
import { LauncherManager } from "../launcher/LauncherManager";
import { AIHub } from "../ai/AIHub";
import { WelcomeWizard } from "../welcome/WelcomeWizard";
import { getThirdApps, launchThirdApp, reloadThirdApps } from "../launcher/thirdApps";
import { autosaveSnapshot } from "../windows/snapshots";
import { handleDisplayChanged, initDisplayMemory } from "../windows/snapshots";
import { openVwmApp, openVwmSystem, type VwmApp } from "../windows/vwm";
import { VirtualWindowManager } from "../windows/VirtualWindowManager";
import { applySnap, SnapPreviewHost } from "../windows/snap";
import { pushRecent } from "../startmenu/recent";
import { effectiveBinds } from "../../lib/shortcuts";
import { InputFeelRuntime } from "../../features/inputFeel/InputFeelRuntime";
import { CommandPalette } from "../palette/CommandPalette";
import { MiniAppsLayer } from "../vwm/miniframe";
import { DndLayer } from "../../lib/dnd/DragGhost";
// AI-08 Z-28：运行对话框（全局浮层；ctrl+alt+r 呼出）
import { RunDialog } from "../tools/RunDialog";
// AI-11 N-19：性能 HUD 悬浮窗（localStorage 开关，默认关）
import { PerfHud } from "../tools/syshub/PerfHud";

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
  // AI-08 Z-28：运行对话框开关（sys://open-run 驱动）
  const [runOpen, setRunOpen] = useState(false);
  const notifiedRef = useRef(false);
  // 批次E-6：全屏应用运行中（任务栏/红绿灯自动避让）；U 盘拔出横幅
  const [fsApp, setFsApp] = useState(false);
  const [usbRemoved, setUsbRemoved] = useState(false);
  // X-3：扩展推送的桌面小组件（widgets.register）与主题局部 token
  const [extWidgets, setExtWidgets] = useState<{ extId: string; slot: string; title: string }[]>([]);

  // 退出不再弹确认框：所有入口（红绿灯/开始菜单/托盘）直接走保存冲刷 + 关闭。
  const exitDesktop = (): void => {
    // 批次W-4：退出前自动保存一份窗口布局快照（__autosave__，覆盖旧档）
    try {
      autosaveSnapshot();
    } catch {
      /* 快照失败不阻断退出流程 */
    }
    props.onCloseRequested();
  };

  // X-3 扩展事件面：notify.create → 系统通知；widgets.register → 桌面小组件条；
  // theme.patch → 仅接受 CSS 变量形式的局部 token（key 必须以 -- 开头）。
  useEffect(() => {
    let disposed = false;
    const uns: Array<() => void> = [];
    const reg = (p: Promise<() => void>): void => {
      void p.then((u) => {
        if (disposed) u();
        else uns.push(u);
      }).catch(() => {});
    };
    reg(listen<{ title: string; body: string; extId: string }>("ext://notify", (ev) => {
      pushNotify("system", `[扩展] ${ev.payload.title}`, ev.payload.body);
    }));
    reg(listen<{ extId: string; slot: string; title: string }>("ext://widget-registered", (ev) => {
      setExtWidgets((prev) => [...prev.filter((w) => w.extId !== ev.payload.extId), ev.payload]);
    }));
    reg(listen<{ tokens: Record<string, unknown> }>("ext://theme-patch", (ev) => {
      for (const [k, v] of Object.entries(ev.payload.tokens ?? {})) {
        if (k.startsWith("--") && (typeof v === "string" || typeof v === "number")) {
          document.documentElement.style.setProperty(k, String(v));
        }
      }
    }));
    return () => {
      disposed = true;
      uns.forEach((u) => u());
    };
  }, []);

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
    const id = window.setTimeout(() => {
      setEntered(true);
      // A-4：启动落定音（静音/勿扰矩阵由 playSound 处理，失败静默）
      void import("../../lib/sounds").then(({ playSound }) => {
        void import("../../lib/settings").then(({ loadSettings }) =>
          loadSettings().then((s) => playSound("boot", { volume: s.soundVolume, muted: s.soundMuted })).catch(() => {}),
        ).catch(() => {});
      }).catch(() => {});
    }, 1500);
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
  // 批次C-5：L4 智能让位 —— 独占全屏命中时桌面层主动最小化（托盘态，
  // 进程与数据通道全保留）；前台退出 2s 内全量恢复（show + unminimize + focus）。
  useEffect(() => {
    const un = listen<boolean>("sys://fullscreen", (e) => {
      const hit = e.payload === true;
      setFsApp(hit);
      if (hit) void win.minimize().catch(() => {});
      else {
        void win.show().catch(() => {});
        void win.unminimize().catch(() => {});
        void win.setFocus().catch(() => {});
      }
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [win]);

  // 批次C-5：反作弊进程运行 → 横幅声明（kbdhook 已在 Rust 侧主动停用，
  // 不注入/不读取任何键盘状态；进程与数据通道全保留），退出自动解除。
  useEffect(() => {
    const un = listen<boolean>("sys://anticheat", (e) => {
      const hit = e.payload === true;
      if (hit) {
        pushToast("error", t("acTitle"), t("acBody"));
        pushNotify("system", t("acTitle"), t("acBody"));
      } else {
        pushToast("info", t("acTitle"), t("acClear"));
      }
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [t]);

  // 批次W-5：显示器热切换 —— 旧屏布局自动存档，新屏有分屏记忆则整体恢复，
  // 否则出屏窗口吸附回主屏最近合法位置（拔插 HDMI 零丢窗）。
  useEffect(() => {
    initDisplayMemory();
    const un = listen("sys://display-changed", () => {
      handleDisplayChanged();
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  // 批次E-8（规格 45 边界）：通讯软件未读提醒 —— 仅窗口标题信号，不读消息内容
  // F-5.1：附「打开应用 / 忽略」动作按钮（内置动作）
  useEffect(() => {
    const un = listen<{ app: string; title: string }>("sys://im-msg", (e) => {
      const { app, title } = e.payload;
      pushNotify("system", app, title, [
        { label: t("notifyOpenApp"), type: "open-third", data: app },
        { label: t("notifyDismiss"), type: "dismiss" },
      ]);
      pushToast("info", app, t("imNewMsgBody", { app }));
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [t]);

  // F-5.1：通知动作统一处理 —— open-app/open-third 尽力打开，open-path 走 VWM 文件管理器
  useEffect(() => {
    const onAction = (ev: Event): void => {
      const a = (ev as CustomEvent<NotifyAction>).detail;
      if (!a) return;
      if (a.type === "open-app" && a.data) {
        openVwmApp(a.data as Parameters<typeof openVwmApp>[0]);
      } else if (a.type === "open-third" && a.data) {
        const found = getThirdApps().find((x) => x.name === a.data);
        if (found) {
          void launchThirdApp(found.id, found.name).catch((e) =>
            pushToast("error", a.data ?? "", errMessage(e).message),
          );
        } else {
          pushToast("info", a.data, t("notifyThirdMissing"));
        }
      } else if (a.type === "open-path" && a.data) {
        openVwmSystem("explorer", a.data);
      }
      // dismiss：仅忽略，无额外动作
    };
    window.addEventListener("variable:notify-action", onAction);
    return () => window.removeEventListener("variable:notify-action", onAction);
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
    // F-1：设置中心快捷键呼出（ctrl+alt+i，Rust winman 分发）
    const unSet = listen("sys://open-settings", () => props.onOpenSettings());
    // F-2：剪贴板历史快捷键呼出（ctrl+alt+v，Rust winman 分发）
    const unClip = listen("sys://open-clipboard", () => openVwmApp("clipboard"));
    // AI-08 Z-28：运行对话框呼出（ctrl+alt+r 降级口径，Rust winman 分发）
    const unRun = listen("sys://open-run", () => setRunOpen(true));
    return () => {
      void unQp.then((f) => f()).catch(() => {});
      void unDnd.then((f) => f()).catch(() => {});
      void unSet.then((f) => f()).catch(() => {});
      void unClip.then((f) => f()).catch(() => {});
      void unRun.then((f) => f()).catch(() => {});
    };
  }, [t, props.onOpenSettings]);

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
    // D-4 幕布语义：双 Esc 切换时 220ms scale+fade（out 收起 / in 展开）
    const unCurtain = listen<string>("sys://curtain", (e) => {
      const dir = e.payload;
      const root = document.documentElement;
      root.classList.remove("var-curtain-out", "var-curtain-in");
      if (dir === "out" || dir === "in") {
        void root.offsetWidth; // 强制重排，确保动画重放
        root.classList.add(dir === "out" ? "var-curtain-out" : "var-curtain-in");
        window.setTimeout(() => root.classList.remove("var-curtain-out", "var-curtain-in"), 240);
      }
    });
    return () => {
      void unShow.then((f) => f()).catch(() => {});
      void unHide.then((f) => f()).catch(() => {});
      void unWin.then((f) => f()).catch(() => {});
      void unIdx.then((f) => f()).catch(() => {});
      void unSnap.then((f) => f()).catch(() => {});
      void unSys.then((f) => f()).catch(() => {});
      void unCurtain.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [win]);

  // 批次E（规格 4.7）：整表应用快捷键（默认 + 用户覆盖）；变更即重注册。
  // 实机反馈"彻底解决"：被占用的组合后端已自动改用备选组合键（remapped 如实提示），
  // 仅连备选都失败的组合才提示（failed，需用户手动换键）。
  useEffect(() => {
    const binds = effectiveBinds(props.settings.shortcutBinds ?? {});
    void ipc
      .shortcutsApply(binds)
      .then((res) => {
        if (res.remapped.length > 0) {
          const list = res.remapped.map((r) => `${r.from} → ${r.to}`).join(", ");
          pushToast("info", t("scTitle"), `${t("scRemapped")}: ${list}`);
        }
        if (res.failed.length > 0) pushToast("info", t("scTitle"), `${t("scRegisterFailed")}: ${res.failed.join(", ")}`);
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
      {/* AI-06 输入手感组运行时（U-58/U-59、V-62…V-69；默认全部关闭） */}
      <InputFeelRuntime settings={props.settings} />
      {/* AI-11 N-19：性能 HUD 悬浮窗（系统中枢内开关，默认关闭） */}
      <PerfHud />
      <WallpaperLayer settings={props.settings} />
      {/* X-3：扩展小组件条（widgets.register；slot=desktop-top-right） */}
      {extWidgets.length > 0 && (
        <div
          style={{
            position: "fixed",
            top: 52,
            right: 16,
            zIndex: 40,
            display: "flex",
            flexDirection: "column",
            gap: 8,
            pointerEvents: "none",
          }}
        >
          {extWidgets.map((w) => (
            <div
              key={w.extId}
              style={{
                background: "rgba(10,16,30,.72)",
                border: "1px solid rgba(255,255,255,.12)",
                borderRadius: 12,
                padding: "8px 14px",
                color: "#e8eefc",
                fontSize: 12,
                backdropFilter: "blur(8px)",
              }}
            >
              <div style={{ opacity: 0.65, fontSize: 10, marginBottom: 2 }}>🧩 {w.title}</div>
              <div>小组件由扩展 {w.extId} 提供（时钟等自绘内容见扩展窗口）</div>
            </div>
          ))}
        </div>
      )}
      {/* F-1 夜灯模式：暖色遮罩层（覆盖全部内容之上、交互之下；强度 0 = 关闭） */}
      {props.settings.nightLight > 0 && (
        <div
          className="night-light-overlay"
          style={{ opacity: props.settings.nightLight / 100 }}
          aria-hidden
        />
      )}
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
      {/* 兼容层：Wallpaper Engine 共存横幅（libcef 0x80000003 冲突缓解） */}
      <CompatBanner />

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
        settings={props.settings}
      />

      {/* AI-07 N-13：命令面板（Ctrl+K / 全局 ctrl+alt+p → sys://open-palette） */}
      <CommandPalette />

      {/* AI-08 U-17：全局拖放总线视觉层（拖拽幽灵 + 收藏托盘；无会话零开销） */}
      <DndLayer onDropToTarget={() => {/* 各 Drop 目标经 registerDropTarget 自行订阅 */}} />

      {/* AI-08 U-18：迷你应用框架图层（胶囊头小窗，五件首发） */}
      <MiniAppsLayer />

      {/* AI-08 Z-28：运行对话框（别名 → VWM 工具/系统窗口；路径/URI 走系统 ShellExecute） */}
      <RunDialog
        open={runOpen}
        onClose={() => setRunOpen(false)}
        onRunAlias={(id: string) => {
          if (id === "settings") props.onOpenSettings();
          else if (id === "explorer" || id === "taskman") openVwmSystem(id);
          else openVwmApp(id as VwmApp);
        }}
      />

      {/* M7 第三方软件管理器（模态） */}
      <LauncherManager />

      {/* 批次B-9（M3）：AI Hub（终端与云 AI 矩阵统一入口） */}
      <AIHub />

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
