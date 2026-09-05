import { createStore, useStore } from "../../lib/store";
import type { AppMode } from "../../state/uiStore";

/**
 * 批次C（规格 5.6.1）：预装软件卸载状态（桌面窗口内共享）。
 * - 卸载 = 桌面/任务栏/开始菜单/搜索隐藏入口 + 关闭运行窗口；数据默认保留
 * - 记录卸载时间，24 小时内"最近卸载"可一键恢复（规格 5.6.3）
 * - 彻底删除（可选）走后端 official_purge，仅 write/mindmap 支持数据库级清除
 * - 状态存 localStorage（仅本机 UI 层，与数据目录无关）
 */

const LS_KEY = "variable:apps:official:v1";
const RESTORE_WINDOW_MS = 24 * 60 * 60 * 1000;

type UninstalledMap = Partial<Record<AppMode, number>>;

function load(): UninstalledMap {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as UninstalledMap;
      if (p && typeof p === "object") return p;
    }
  } catch {
    /* corrupted → defaults */
  }
  return {};
}

function save(m: UninstalledMap): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(m));
  } catch {
    /* storage full/blocked */
  }
}

const officialStore = createStore<{ uninstalled: UninstalledMap }>({ uninstalled: load() });

export function useUninstalledOfficial(): Partial<Record<AppMode, number>> {
  return useStore(officialStore, (s) => s.uninstalled);
}

export function isOfficialUninstalled(app: AppMode): boolean {
  return officialStore.getState().uninstalled[app] !== undefined;
}

/** 卸载（隐藏入口）。记录时间用于 24h 恢复窗口。 */
export function markOfficialUninstalled(app: AppMode): void {
  const m = { ...officialStore.getState().uninstalled, [app]: Date.now() };
  officialStore.setState({ uninstalled: m });
  save(m);
}

/** 重新安装 / 恢复：重新显示入口（数据未被删除时原样可用）。 */
export function markOfficialInstalled(app: AppMode): void {
  const m = { ...officialStore.getState().uninstalled };
  delete m[app];
  officialStore.setState({ uninstalled: m });
  save(m);
}

/** 最近卸载（24h 内可恢复）。 */
export function recentUninstalled(): { app: AppMode; at: number }[] {
  const now = Date.now();
  return Object.entries(officialStore.getState().uninstalled)
    .map(([app, at]) => ({ app: app as AppMode, at: at ?? 0 }))
    .filter((e) => now - e.at < RESTORE_WINDOW_MS);
}

export const OFFICIAL_RESTORE_MS = RESTORE_WINDOW_MS;
