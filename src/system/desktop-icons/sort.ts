/**
 * V-01 桌面图标排序（纯函数，供 vitest 与 DesktopIcons 共用）。
 *
 * 数据诚实边界：
 * - 第三方登记项（tp-*）只有 addedAt（登记时间）——「日期」排序可用；
 * - 没有任何来源提供图标大小数据（ipc 无估算命令）——「大小」排序下
 *   无数据项恒排后；当前全部项都无数据，排序退化为基线稳定序（如实注明，不做假排序）。
 * - 排序必须稳定（同 key 保持原序，ES2019+ Array.sort 已保证稳定）。
 */
import type { SortMode } from "./layout";

export interface SortItem<T> {
  def: T;
  /** 显示名（排序键之一）。 */
  label: string;
  /** 类型分组：0 系统 / 1 官方软件 / 2 第三方 / 3 文件架（基线序）。 */
  group: number;
  /** 仅第三方登记项有（登记时间 ms）；其余 undefined。 */
  addedAt?: number;
}

/** 基线（type）顺序：系统 → 官方 → 第三方 → 文件架，组内保持原序。 */
function byGroup<T>(a: SortItem<T>, b: SortItem<T>): number {
  return a.group - b.group;
}

export function sortItems<T>(items: SortItem<T>[], mode: SortMode): SortItem<T>[] {
  const arr = [...items];
  switch (mode) {
    case "name":
      arr.sort((a, b) => a.label.localeCompare(b.label, "zh"));
      break;
    case "date":
      // 有登记时间的（第三方）按时间倒序在前；无数据的按基线稳定序兜底在后。
      arr.sort((a, b) => {
        if (a.addedAt !== undefined && b.addedAt !== undefined) return b.addedAt - a.addedAt;
        if (a.addedAt !== undefined) return -1;
        if (b.addedAt !== undefined) return 1;
        return byGroup(a, b);
      });
      break;
    case "size":
      // 诚实边界：无大小数据 → 全部视为无数据恒排后 → 退化为基线稳定序。
      arr.sort(byGroup);
      break;
    case "type":
    default:
      arr.sort(byGroup);
      break;
  }
  return arr;
}