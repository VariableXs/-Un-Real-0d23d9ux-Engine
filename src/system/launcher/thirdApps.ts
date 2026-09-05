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
  } catch (e) {
    console.warn("[launcher] tp_list failed", errMessage(e).message);
  }
}

export function useThirdApps(): ThirdApp[] {
  return useStore(tpStore, (s) => s.apps);
}

/** 批次D：非响应式读取（Win+数字 快速启动用）。 */
export function getThirdApps(): ThirdApp[] {
  return tpStore.getState().apps;
}

export async function launchThirdApp(id: string, name: string): Promise<void> {
  try {
    await ipc.tpLaunch(id);
  } catch (e) {
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
