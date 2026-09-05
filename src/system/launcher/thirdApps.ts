import { errMessage, ipc, type ThirdApp } from "../../lib/ipc";
import { pushToast, uiStore } from "../../state/uiStore";
import { createStore, useStore } from "../../lib/store";

/**
 * M7 第三方软件登记（桌面窗口内共享状态）：
 * - 单一数据源 tpStore；DesktopIcons / StartMenu / LauncherManager 共用
 * - 由 DesktopShell 挂载时加载一次，增删改后调用 reloadThirdApps()
 * - 启动失败如实 toast（目标可能已被移动/卸载），不伪造成功
 */

const tpStore = createStore<{ apps: ThirdApp[] }>({ apps: [] });

export async function reloadThirdApps(): Promise<void> {
  try {
    const apps = await ipc.tpList();
    tpStore.setState({ apps });
    // 批次E-16：未自定义图标的第三方应用自动提取 Windows 原生图标
    // （exe 资源里的 HICON → data URL），与系统里看到的一致
    void fillNativeIcons(apps);
  } catch (e) {
    console.warn("[launcher] tp_list failed", errMessage(e).message);
  }
}

/** 为缺少图标的登记项提取 Windows 原生图标（exe/lnk 目标；失败静默跳过）。 */
async function fillNativeIcons(apps: ThirdApp[]): Promise<void> {
  for (const a of apps) {
    if (a.icon) continue;
    // .lnk 也直接传：Rust 端 icon_dataurl 会先解析快捷方式目标再提取
    const target = a.target ?? a.path;
    if (!target) continue;
    try {
      const url = await ipc.iconDataurl(target);
      if (!url) continue;
      const cur = tpStore.getState().apps;
      tpStore.setState({
        apps: cur.map((x) => (x.id === a.id ? { ...x, icon: url } : x)),
      });
    } catch {
      /* 提取失败（非 exe/图标缺失）→ 保持占位图标 */
    }
  }
}

export function useThirdApps(): ThirdApp[] {
  return useStore(tpStore, (s) => s.apps);
}

/** 批次D：非响应式读取（Win+数字 快速启动用）。 */
export function getThirdApps(): ThirdApp[] {
  return tpStore.getState().apps;
}

/**
 * 批次E-16：第三方应用一律在环境内打开 —— 先开虚拟窗口（占位），
 * 再由后端启动并把原生窗口 SetParent 嵌进来（从任务栏/Alt+Tab 消失）。
 * 无法嵌入（UWP/管理员权限等）→ 如实回退独立窗口并关闭占位窗口。
 */
export async function launchThirdApp(id: string, name: string): Promise<void> {
  const { openVwmApp, closeVwmApp } = await import("../windows/vwm");
  const tpApp = `tp:${id}` as Parameters<typeof openVwmApp>[0];
  openVwmApp(tpApp);
  try {
    const r = await ipc.embedLaunch(id);
    if (!r.attached) {
      closeVwmApp(tpApp);
      pushToast("info", name, r.reason || "已按独立窗口运行");
    }
  } catch (e) {
    closeVwmApp(tpApp);
    pushToast("error", name, errMessage(e).message);
  }
}

/** 管理器开关（桌面窗口内渲染 LauncherManager 模态）。tab: 第三方 / 已安装软件。 */
export function openLauncherManager(tab: "third" | "installed" = "third"): void {
  uiStore.setState({ launcherOpen: true, launcherTab: tab });
}

// ---------- 批次C（规格 5.5）：任务栏固定 ----------

const PINS_KEY = "variable:taskbar:pins:v1";

function loadPins(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(PINS_KEY) ?? "[]") as unknown;
    if (Array.isArray(raw)) return raw.filter((x): x is string => typeof x === "string");
  } catch {
    /* corrupted → defaults */
  }
  return [];
}

export function useTaskbarPins(): string[] {
  return useStore(pinStore, (s) => s.pins);
}

const pinStore = createStore<{ pins: string[] }>({ pins: loadPins() });

function savePins(pins: string[]): void {
  pinStore.setState({ pins });
  try {
    localStorage.setItem(PINS_KEY, JSON.stringify(pins));
  } catch {
    /* storage full/blocked */
  }
}

export function toggleTaskbarPin(id: string): void {
  const cur = pinStore.getState().pins;
  savePins(cur.includes(id) ? cur.filter((p) => p !== id) : [...cur, id]);
}
