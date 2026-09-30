/**
 * AURORA-10000 领域04 · 族0078 任务栏托盘区 + 族0097 系统托盘扩展（AI-16/AI-20 批次，勿删）。
 * 托盘项模型：折叠/固定/排序/低频折叠/角标 + 扩展状态项登记。
 */
import { getD4 } from "./prefs";

export interface TrayItem {
  id: string;
  name: string;
  pinned: boolean;
  /** 使用频次（低频折叠 F01946）。 */
  useCount: number;
  badge: number | null;
  /** 图标样式族（统一描边 F01945）。 */
  style: "system" | "accent" | "mono";
}

/** 折叠决策：返回外显与隐藏集合（F01926/27/32/46）。 */
export function foldTray(items: readonly TrayItem[]): { shown: TrayItem[]; hidden: TrayItem[] } {
  const maxPinned = getD4<number>("F01927") ?? 3;
  const lowFold = getD4<boolean>("F01946") ?? true;
  const pinned = items.filter((t) => t.pinned).slice(0, maxPinned);
  const rest = items.filter((t) => !pinned.includes(t));
  const shown = [...pinned, ...(lowFold ? rest.filter((t) => t.useCount >= 3) : rest)];
  const hidden = rest.filter((t) => !shown.includes(t));
  return { shown, hidden };
}

/** 隐藏计数提示（F01932）。 */
export function hiddenCount(hidden: readonly TrayItem[]): number { return hidden.length; }

/** 时钟格式（F01938 秒显 / F02457 12/24 联动）。 */
export function clockFormat(date: Date): string {
  const seconds = getD4<boolean>("F01938") ?? false;
  const h24 = (getD4<string>("F02457") ?? "24") === "24";
  const hh = h24 ? date.getHours() : date.getHours() % 12 || 12;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(hh)}:${pad(date.getMinutes())}${seconds ? `:${pad(date.getSeconds())}` : ""}${h24 ? "" : date.getHours() < 12 ? " AM" : " PM"}`;
}

/** 多时区轮换（F01939）：zone 为 IANA 名；空配置返回本地。 */
export function rotateTimezone(zones: readonly string[], tick: number): { zone: string; time: string } {
  if (zones.length === 0) return { zone: "local", time: clockFormat(new Date()) };
  const zone = zones[tick % zones.length] ?? "local";
  const time = new Intl.DateTimeFormat("zh-CN", {
    timeZone: zone, hour: "2-digit", minute: "2-digit", hour12: false,
  }).format(new Date());
  return { zone, time };
}

/* ---------------- 族0097 托盘扩展状态项 ---------------- */

export type ExtendedStatusKind =
  | "charging" | "battery-health" | "power-curve" | "speedtest" | "net-usage"
  | "vpn" | "proxy" | "firewall" | "update" | "backup" | "sync"
  | "mic-in-use" | "cam-in-use" | "sharing" | "dnd" | "focus"
  | "display-layout" | "audio-out" | "bt-battery" | "print" | "download"
  | "upload" | "scheduled" | "health" | "aggregate";

export interface ExtendedStatus {
  kind: ExtendedStatusKind;
  active: boolean;
  /** 角标文本（如「87%」「1.2MB/s」）。 */
  badge?: string;
  /** 聚合面板（F02425）归属分组。 */
  group: "power" | "network" | "privacy" | "system" | "tasks";
}

/** 聚合面板：按分组归并当前活跃状态（F02425）。 */
export function aggregateStatus(items: readonly ExtendedStatus[]): Map<string, ExtendedStatus[]> {
  const m = new Map<string, ExtendedStatus[]>();
  for (const it of items) {
    if (!it.active) continue;
    const arr = m.get(it.group) ?? [];
    arr.push(it);
    m.set(it.group, arr);
  }
  return m;
}
