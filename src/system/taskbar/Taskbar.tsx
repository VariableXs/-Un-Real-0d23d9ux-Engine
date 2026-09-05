import { useEffect, useRef, useState } from "react";
import { getAllWindows } from "@tauri-apps/api/window";
import {
  AppWindow, Bell, Bluetooth, ChevronLeft, ChevronRight, ChevronUp, FolderOpen, Moon, Pin, PinOff,
  Play, Search, Trash2, Volume2, VolumeX, Wifi, WifiOff, X,
} from "lucide-react";
import { useI18n } from "../../i18n";
import type { AppMode } from "../../state/uiStore";
import type { TaskbarPos } from "../../lib/settings";
import { closeQuickPanel, openQuickPanel, pushToast, useUi } from "../../state/uiStore";
import { errMessage, ipc, type SysBrief, type SysDisk, type ThirdApp } from "../../lib/ipc";
import { useDnd } from "../../state/notifyStore";
import { desktopAppLabel, desktopIconDefs } from "../desktop-icons/DesktopIcons";
import { openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { openSystemWindow } from "../windows/appWindows";
import {
  launchThirdApp, openLauncherManager, reloadThirdApps, toggleTaskbarPin,
  useTaskbarPins, useThirdApps,
} from "../launcher/thirdApps";
import { useUninstalledOfficial } from "../launcher/official";
import { startHardwarePolling, useHw } from "../tray/hardware";
import { QuickPanel, useNotifyBadge } from "../tray/QuickPanel";
import { pushRecent } from "../startmenu/recent";

/**
 * Win11 风格任务栏（M3 → 批次E，桌面环境 L1）：
 * - 居中毛玻璃浮条：V 开始按钮 + 全局搜索 + 文件管理器 + 固定/运行中的软件图标
 * - 运行态（规格 5.5）：官方四软件 = Tauri 窗口存在；第三方 = 后端进程镜像匹配。
 *   运行中未固定 → 显示并带指示点；固定的 → 无论是否运行都显示
 * - 右键图标 → Jump List；悬停 2s → 预览浮层（关闭窗口）；中键 → 官方=关窗 / 第三方=新进程
 * - 右侧托盘区（M5）+ 托盘折叠（批次E）+ 本地时钟（批次D：点击弹日历）+ 最右"显示桌面"细条
 * - 左侧小组件区（规格 4.4.2）：CPU 波形 / 内存占用 / 磁盘容量（sysinfo 本地读取，零网络）
 * - 批次E：停靠位置四向（settings.taskbarPos）+ 空白右键菜单 + 滚轮调音量
 */

const CPU_HIST = 40;

export function Taskbar(props: {
  startOpen: boolean;
  onToggleStart: () => void;
  onOpenSearch: () => void;
  onOpenApp: (app: AppMode) => void;
  onShowDesktop: () => void;
  onOpenSettings: () => void;
  pos: TaskbarPos;
}): React.ReactElement {
  const { t, lang } = useI18n();
  const [now, setNow] = useState(() => new Date());
  const quickOpen = useUi((s) => s.quickOpen);
  const quickSection = useUi((s) => s.quickSection);
  const [officialRunning, setOfficialRunning] = useState<Set<AppMode>>(new Set());
  const [tpRunning, setTpRunning] = useState<Set<string>>(new Set());
  const wifi = useHw((s) => s.wifi);
  const bluetooth = useHw((s) => s.bluetooth);
  const audio = useHw((s) => s.audio);
  const unread = useNotifyBadge();
  const dnd = useDnd();
  const thirds = useThirdApps();
  const pins = useTaskbarPins();
  const uninstalled = useUninstalledOfficial();

  useEffect(() => startHardwarePolling(), []);

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 15000);
    return () => window.clearInterval(id);
  }, []);

  // 批次C：运行态轮询（3s；官方 = 窗口枚举，第三方 = 后端进程匹配，失败静默保持原值）
  useEffect(() => {
    let alive = true;
    const poll = async (): Promise<void> => {
      try {
        const wins = await getAllWindows();
        const apps = new Set<AppMode>();
        for (const w of wins) {
          if (w.label.startsWith("app-")) apps.add(w.label.slice(4) as AppMode);
        }
        if (alive) setOfficialRunning(apps);
      } catch {
        /* window API unavailable */
      }
      try {
        const ids = await ipc.tpRunning();
        if (alive) setTpRunning(new Set(ids));
      } catch {
        /* backend busy — keep previous */
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 3000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  const locale = lang === "en" ? "en-US" : "zh-CN";
  const time = now.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false });
  const date = now.toLocaleDateString(locale, { year: "numeric", month: "2-digit", day: "2-digit" });

  const officialJump = (app: AppMode, x: number, y: number): void => {
    const items: MenuItem[] = [
      { label: t("desktopOpen"), icon: <Play size={13} />, onClick: () => props.onOpenApp(app) },
      {
        label: t("uninstallMenu"),
        icon: <Trash2 size={13} />,
        onClick: () => openLauncherManager("installed"),
      },
    ];
    openContextMenu(x, y, items);
  };

  const thirdJump = (a: ThirdApp, x: number, y: number): void => {
    const pinned = pins.includes(a.id);
    const items: MenuItem[] = [
      { label: t("desktopOpen"), icon: <Play size={13} />, onClick: () => void launchThirdApp(a.id, a.name) },
      {
        label: t("runAsAdmin"),
        icon: <Play size={13} />,
        onClick: () => {
          void ipc
            .tpLaunchAdmin(a.id)
            .catch((e) => console.warn("[launcher] admin run failed", errMessage(e).message));
        },
      },
      {
        label: pinned ? t("tbUnpin") : t("tbPin"),
        icon: pinned ? <PinOff size={13} /> : <Pin size={13} />,
        onClick: () => toggleTaskbarPin(a.id),
      },
      { separator: true },
      {
        label: t("tpRemove"),
        icon: <Trash2 size={13} />,
        danger: true,
        onClick: () => {
          void ipc
            .tpRemove(a.id)
            .then(() => reloadThirdApps())
            .catch((e) => console.warn("[launcher] remove failed", errMessage(e).message));
        },
      },
    ];
    openContextMenu(x, y, items);
  };

  const defs = desktopIconDefs().filter((d) => uninstalled[d.app] === undefined);
  // 批次C：任务栏显示的第三方 = 固定的 ∪ 运行中的
  const pinnedSet = new Set(pins);
  const taskbarThirds = thirds.filter((a) => pinnedSet.has(a.id) || tpRunning.has(a.id));

  // ---- 批次D（规格 4.4.2）小组件区：CPU/内存（2s 轮询）+ 磁盘（30s）+ 日历（纯本地） ----
  const [brief, setBrief] = useState<SysBrief | null>(null);
  const [disks, setDisks] = useState<SysDisk[]>([]);
  const [calOpen, setCalOpen] = useState(false);
  const [calMonth, setCalMonth] = useState(() => new Date());
  const cpuHist = useRef<number[]>([]);
  const cpuCanvas = useRef<HTMLCanvasElement | null>(null);

  // ---- 批次E：悬停 2s 预览浮层 + 托盘折叠 ----
  const [hover, setHover] = useState<{ id: string; label: string; running: boolean; official?: AppMode } | null>(null);
  const hoverTimer = useRef<number | null>(null);
  const [trayCollapsed, setTrayCollapsed] = useState<boolean>(() => {
    try {
      return localStorage.getItem("variable:tray:collapsed:v1") === "1";
    } catch {
      return false;
    }
  });

  const hoverEnter = (id: string, label: string, running: boolean, official?: AppMode): void => {
    if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
    hoverTimer.current = window.setTimeout(() => setHover({ id, label, running, official }), 2000);
  };
  const hoverLeave = (): void => {
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    setHover(null);
  };
  const closeOfficial = async (app: AppMode): Promise<void> => {
    try {
      const w = await getAllWindows();
      const target = w.find((x) => x.label === `app-${app}`);
      if (target) await target.close();
    } catch (e) {
      pushToast("error", desktopAppLabel(app), errMessage(e).message);
    }
    setHover(null);
  };

  // 批次E：托盘图标滚轮调音量（音量图标上，↑增 ↓减，本地 Core Audio）
  const wheelVolume = (deltaY: number): void => {
    if (audio.kind !== "ok") return;
    const cur = audio.value.volume;
    const next = Math.min(100, Math.max(0, Math.round(cur) + (deltaY < 0 ? 5 : -5)));
    if (next === Math.round(cur)) return;
    void ipc
      .audioSet(next, false)
      .catch((e) => console.warn("[taskbar] volume wheel failed", errMessage(e).message));
  };

  // 批次E：任务栏空白右键菜单
  const blankMenu = (x: number, y: number): void => {
    openContextMenu(x, y, [
      { label: t("showDesktop"), onClick: props.onShowDesktop },
      { label: t("launcherTitle"), onClick: openLauncherManager },
      { label: t("taskbarSettings"), onClick: props.onOpenSettings },
    ]);
  };

  const setCollapsed = (v: boolean): void => {
    setTrayCollapsed(v);
    try {
      localStorage.setItem("variable:tray:collapsed:v1", v ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  // 批次E（规格 6.5）：隐私占用指示点（摄像头=橙点 / 麦克风=绿点，悬停显示占用软件）
  const [privacy, setPrivacy] = useState<{ kind: "microphone" | "webcam"; app: string }[]>([]);
  useEffect(() => {
    let alive = true;
    const poll = (): void => {
      void ipc
        .privacyUsage()
        .then((u) => alive && setPrivacy(u))
        .catch(() => {});
    };
    poll();
    const id = window.setInterval(poll, 5000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    let alive = true;
    const poll = async (): Promise<void> => {
      try {
        const b = await ipc.sysBrief();
        if (!alive) return;
        const hist = cpuHist.current;
        hist.push(b.cpu);
        if (hist.length > CPU_HIST) hist.shift();
        setBrief(b);
      } catch {
        /* backend busy — keep previous */
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 2000);
    const pollDisks = (): void => {
      void ipc
        .sysDisks()
        .then((d) => alive && setDisks(d))
        .catch(() => {});
    };
    pollDisks();
    const idDisks = window.setInterval(pollDisks, 30000);
    return () => {
      alive = false;
      window.clearInterval(id);
      window.clearInterval(idDisks);
    };
  }, []);

  // CPU 波形（真实采样历史，非假动画）
  useEffect(() => {
    const c = cpuCanvas.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const w = c.width;
    const h = c.height;
    ctx.clearRect(0, 0, w, h);
    const hist = cpuHist.current;
    if (hist.length < 2) return;
    ctx.beginPath();
    for (let i = 0; i < hist.length; i++) {
      const x = (i / (CPU_HIST - 1)) * w;
      const y = h - Math.min(1, Math.max(0, (hist[i] ?? 0) / 100)) * (h - 2) - 1;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#7fa8d6";
    ctx.lineWidth = 1.4;
    ctx.stroke();
  }, [brief]);

  const memPct = brief && brief.memTotal > 0 ? Math.round((brief.memUsed / brief.memTotal) * 100) : null;
  const fmtBytes = (n: number): string => {
    if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(0)}GB`;
    if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(0)}MB`;
    return `${n}B`;
  };

  // 日历（本地时区，零网络）
  const calYear = calMonth.getFullYear();
  const calMon = calMonth.getMonth();
  const firstDow = new Date(calYear, calMon, 1).getDay(); // 0=周日
  const offset = (firstDow + 6) % 7; // 周一为一周起点（中文习惯）
  const daysInMonth = new Date(calYear, calMon + 1, 0).getDate();
  const today = new Date();
  const isThisMonth = today.getFullYear() === calYear && today.getMonth() === calMon;
  const calCells: (number | null)[] = [
    ...Array.from({ length: offset }, () => null),
    ...Array.from({ length: daysInMonth }, (_, i) => i + 1),
  ];
  const weekdayLabels =
    lang === "en"
      ? ["M", "T", "W", "T", "F", "S", "S"]
      : ["一", "二", "三", "四", "五", "六", "日"];
  const calMonthLabel = today.toLocaleDateString(locale, { year: "numeric", month: "long" })
    .replace(String(today.getFullYear()), String(calYear));

  return (
    <div
      className="taskbar"
      data-pos={props.pos}
      onContextMenu={(e) => {
        // 批次E：空白右键（图标自身右键已 stopPropagation 在各自 handler 内 preventDefault）
        const tEl = e.target as HTMLElement | null;
        if (tEl?.closest("button, .tb-widget, .tb-calendar, .quick-panel")) return;
        e.preventDefault();
        blankMenu(e.clientX, e.clientY);
      }}
    >
      {/* 批次D（规格 4.4.2）左侧小组件区：CPU 波形 / 内存 / 磁盘容量（本地 sysinfo） */}
      <div className="taskbar-widgets" role="group" aria-label={t("widgets")}>
        <div
          className="tb-widget"
          title={`${t("tbCpu")}: ${brief ? `${brief.cpu.toFixed(0)}%` : "—"}`}
        >
          <canvas ref={cpuCanvas} width={56} height={20} aria-hidden />
          <span className="tb-widget-num">{brief ? `${brief.cpu.toFixed(0)}%` : "—"}</span>
        </div>
        <div
          className="tb-widget"
          title={brief ? `${t("tbMem")}: ${fmtBytes(brief.memUsed)} / ${fmtBytes(brief.memTotal)}` : t("tbMem")}
        >
          <div className="tb-mem" aria-hidden>
            <div style={{ width: `${memPct ?? 0}%` }} />
          </div>
          <span className="tb-widget-num">{memPct !== null ? `${memPct}%` : "—"}</span>
        </div>
        {disks.length > 0 && (
          <div
            className="tb-widget tb-disks"
            title={disks.map((d) => `${d.letter}: ${fmtBytes(d.free)} / ${fmtBytes(d.total)}`).join("  ")}
          >
            {disks.map((d) => {
              const usedPct = Math.round(((d.total - d.free) / d.total) * 100);
              const low = usedPct >= 90;
              return (
                <span key={d.letter} className={`tb-disk${low ? " low" : ""}`}>
                  <i>{d.letter}</i>
                  <span className="tb-disk-bar" aria-hidden>
                    <span style={{ width: `${usedPct}%` }} />
                  </span>
                </span>
              );
            })}
          </div>
        )}
      </div>

      <div className="taskbar-center" role="toolbar" aria-label={t("startMenu")}>
        <button
          type="button"
          className={`tb-btn tb-v${props.startOpen ? " active" : ""}`}
          aria-label={t("startMenu")}
          title={t("startMenu")}
          onClick={props.onToggleStart}
        >
          V
        </button>
        <button
          type="button"
          className="tb-btn"
          aria-label={t("globalSearch")}
          title={t("globalSearch")}
          onClick={props.onOpenSearch}
        >
          <Search size={19} strokeWidth={1.7} />
        </button>
        <span className="tb-sep" aria-hidden />
        {/* M6：文件管理器（系统级入口，Win11 任务栏习惯位置） */}
        <button
          type="button"
          className="tb-btn"
          aria-label={t("explorerWin")}
          title={t("explorerWin")}
          onClick={() => {
            pushRecent("sys", "explorer", t("explorerWin"));
            void openSystemWindow("explorer");
          }}
        >
          <span className="tb-app-icon" style={{ ["--hue" as string]: "210" }}>
            <FolderOpen size={19} strokeWidth={1.7} />
          </span>
        </button>
        {/* 批次C：官方四软件（卸载后不显示；运行中带指示点；批次E：悬停预览/中键关窗） */}
        {defs.map((d) => {
          const Icon = d.icon;
          const running = officialRunning.has(d.app);
          return (
            <button
              key={d.id}
              type="button"
              className={`tb-btn${running ? " running" : ""}`}
              aria-label={desktopAppLabel(d.app)}
              title={desktopAppLabel(d.app)}
              onClick={() => {
                pushRecent("app", d.app, desktopAppLabel(d.app));
                props.onOpenApp(d.app);
              }}
              onAuxClick={(e) => {
                // 批次E：中键 = 运行中关窗（Windows 习惯：中键新实例；官方单实例 → 关窗等价回桌面）
                if (e.button === 1 && running) void closeOfficial(d.app);
              }}
              onMouseEnter={() => hoverEnter(d.id, desktopAppLabel(d.app), running, d.app)}
              onMouseLeave={hoverLeave}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                hoverLeave();
                officialJump(d.app, e.clientX, e.clientY);
              }}
            >
              <span className="tb-app-icon" style={{ ["--hue" as string]: String(d.hue) }}>
                <Icon size={19} strokeWidth={1.7} />
              </span>
              {running && <span className="tb-dot" aria-hidden />}
            </button>
          );
        })}
        {/* 批次C：第三方（固定 ∪ 运行中；批次E：中键 = 再次启动新进程） */}
        {taskbarThirds.map((a) => {
          const running = tpRunning.has(a.id);
          return (
            <button
              key={a.id}
              type="button"
              className={`tb-btn${running ? " running" : ""}`}
              aria-label={a.name}
              title={a.name}
              onClick={() => {
                pushRecent("tp", a.id, a.name);
                void launchThirdApp(a.id, a.name);
              }}
              onAuxClick={(e) => {
                if (e.button === 1) void launchThirdApp(a.id, a.name);
              }}
              onMouseEnter={() => hoverEnter(a.id, a.name, running)}
              onMouseLeave={hoverLeave}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                hoverLeave();
                thirdJump(a, e.clientX, e.clientY);
              }}
            >
              <span className="tb-app-icon" style={{ ["--hue" as string]: "158" }}>
                {a.icon ? (
                  <img src={a.icon} alt="" className="tb-custom-icon" />
                ) : (
                  <AppWindow size={19} strokeWidth={1.7} />
                )}
              </span>
              {running && <span className="tb-dot" aria-hidden />}
            </button>
          );
        })}
      </div>

      {/* 批次E：悬停 2s 预览浮层（名称 + 运行态 + 官方关窗按钮，零截图依赖） */}
      {hover && (
        <div className="tb-hover-pop card-pop" role="tooltip" onMouseEnter={hoverLeave}>
          <span className="tb-hover-name">{hover.label}</span>
          <span className={`tb-hover-state${hover.running ? "" : " dim"}`}>
            {hover.running ? t("tbRunning") : t("tbNotRunning")}
          </span>
          {hover.running && hover.official && (
            <button
              type="button"
              className="icon-btn small"
              aria-label={t("closeWindow")}
              title={t("closeWindow")}
              onClick={() => void closeOfficial(hover.official as AppMode)}
            >
              <X size={13} />
            </button>
          )}
        </div>
      )}

      {/* M5/批次C 托盘区 + 批次E 托盘折叠 ^：各图标按分区展开快捷面板（规格 6.1/6.2/6.3/6.5） */}
      <div className="tray-area" role="toolbar" aria-label={t("trayQuick")}>
        {/* 批次E（规格 6.5）：摄像头/麦克风占用指示点（悬停显示软件名） */}
        {privacy.map((p) => (
          <span
            key={`${p.kind}-${p.app}`}
            className={`tb-privacy-dot ${p.kind === "webcam" ? "cam" : "mic"}`}
            title={`${p.kind === "webcam" ? t("privacyCamInUse") : t("privacyMicInUse")}: ${p.app}`}
          />
        ))}
        {trayCollapsed ? (
          <button
            type="button"
            className="tb-btn tray-btn"
            aria-label={t("trayExpand")}
            title={t("trayExpand")}
            onClick={() => setCollapsed(false)}
          >
            <ChevronUp size={16} strokeWidth={1.7} />
          </button>
        ) : (
          <>
            <button
              type="button"
              className={`tb-btn tray-btn${quickOpen ? " active" : ""}`}
              aria-label={t("trayNetwork")}
              title={t("trayNetwork")}
              onClick={() => openQuickPanel("wifi")}
            >
              {wifi.kind === "ok" && wifi.value.connected ? (
                <Wifi size={16} strokeWidth={1.7} />
              ) : (
                <WifiOff size={16} strokeWidth={1.7} className={wifi.kind === "ok" ? "dim" : "hw-unknown"} />
              )}
            </button>
            {bluetooth.kind === "ok" && bluetooth.value.available && (
              <button
                type="button"
                className={`tb-btn tray-btn${quickOpen ? " active" : ""}${bluetooth.value.enabled ? "" : " dim"}`}
                aria-label={t("trayBluetooth")}
                title={t("trayBluetooth")}
                onClick={() => openQuickPanel("bluetooth")}
              >
                <Bluetooth size={16} strokeWidth={1.7} />
              </button>
            )}
            <button
              type="button"
              className={`tb-btn tray-btn${quickOpen ? " active" : ""}${audio.kind === "ok" && audio.value.muted ? " dim" : ""}`}
              aria-label={t("trayAudio")}
              title={t("trayAudio")}
              onClick={() => openQuickPanel("audio")}
              onWheel={(e) => wheelVolume(e.deltaY)}
            >
              {audio.kind === "ok" && audio.value.muted ? (
                <VolumeX size={16} strokeWidth={1.7} />
              ) : (
                <Volume2 size={16} strokeWidth={1.7} />
              )}
            </button>
            <button
              type="button"
              className="tb-btn tray-btn"
              aria-label={t("trayCollapse")}
              title={t("trayCollapse")}
              onClick={() => setCollapsed(true)}
            >
              <ChevronUp size={16} strokeWidth={1.7} style={{ transform: "rotate(180deg)" }} />
            </button>
          </>
        )}
        <button
          type="button"
          className={`tb-btn tray-btn${quickOpen ? " active" : ""}`}
          aria-label={dnd ? t("dndTitle") : t("notifyCenter")}
          title={dnd ? `${t("dndTitle")} · ${t("notifyCenter")}` : t("notifyCenter")}
          onClick={() => openQuickPanel(null)}
        >
          {dnd ? (
            <Moon size={16} strokeWidth={1.7} />
          ) : (
            <Bell size={16} strokeWidth={1.7} />
          )}
          {unread > 0 && <span className="tb-badge" aria-hidden>{unread > 9 ? "9+" : unread}</span>}
        </button>
      </div>

      {/* 批次D：时钟点击弹日历（本地时区，零网络） */}
      <button
        type="button"
        className={`tb-clock${calOpen ? " active" : ""}`}
        aria-label={date}
        onClick={() => setCalOpen(!calOpen)}
      >
        <span className="tb-time">{time}</span>
        <span className="tb-date">{date}</span>
      </button>

      {calOpen && (
        <div className="tb-calendar card-pop" role="dialog" aria-label={t("calendar")}>
          <div className="cal-head">
            <button
              type="button"
              className="icon-btn small"
              aria-label={t("calPrev")}
              onClick={() => setCalMonth(new Date(calYear, calMon - 1, 1))}
            >
              <ChevronLeft size={14} />
            </button>
            <span className="cal-title">{calMonthLabel}</span>
            <button
              type="button"
              className="icon-btn small"
              aria-label={t("calNext")}
              onClick={() => setCalMonth(new Date(calYear, calMon + 1, 1))}
            >
              <ChevronRight size={14} />
            </button>
          </div>
          <div className="cal-grid" aria-hidden>
            {weekdayLabels.map((w, i) => (
              <span key={`w${i}`} className="cal-dow">{w}</span>
            ))}
            {calCells.map((d, i) =>
              d === null ? (
                <span key={`e${i}`} />
              ) : (
                <span
                  key={d}
                  className={`cal-day${isThisMonth && d === today.getDate() ? " today" : ""}`}
                >
                  {d}
                </span>
              ),
            )}
          </div>
        </div>
      )}

      <button
        type="button"
        className="tb-show-desktop"
        aria-label={t("showDesktop")}
        title={t("showDesktop")}
        onClick={props.onShowDesktop}
      />

      <QuickPanel open={quickOpen} section={quickSection} onClose={closeQuickPanel} />
    </div>
  );
}
