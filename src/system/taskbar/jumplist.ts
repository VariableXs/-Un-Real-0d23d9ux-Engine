import type { AppMode } from "../../state/uiStore";
import { getRecent } from "../startmenu/recent";

/**
 * M-10 / U-15 跳转列表数据层（AI-03 任务栏与托盘组）：
 * - 数据源：recent.ts 最近使用记录 + VWM 活动实例窗口（只读消费，不改其内部）
 * - 每应用聚合：最近记录（≤7）→ 任务栏右键「最近」段
 * - 列表 ≤10 项（与常用动作合计），超出由调用方折叠
 * - 不接管 Windows 系统任务栏跳转列表（系统壳是系统的领地）
 */

export interface JumpRecentItem {
  kind: "app" | "sys" | "tp";
  id: string;
  name: string;
  ts: number;
}

export const JUMP_RECENT_MAX = 7;

/** 按应用聚合最近记录：kind+id 与 appKey 匹配（app=AppMode；tp=第三方 id；sys=系统入口）。 */
export function aggregateRecentForApp(appKey: string): JumpRecentItem[] {
  return getRecent()
    .filter((e) => e.id === appKey)
    .slice(0, JUMP_RECENT_MAX)
    .map((e) => ({ kind: e.kind, id: e.id, name: e.name, ts: e.ts }));
}

export interface VwmWinBrief {
  id: string;
  app: string;
  title: string;
  z: number;
}

/** 官方软件的活动实例窗口（VWM 内，z 降序）。 */
export function windowsForApp(wins: VwmWinBrief[], app: AppMode): VwmWinBrief[] {
  return wins
    .filter((w) => w.app === app)
    .sort((a, b) => b.z - a.z);
}
