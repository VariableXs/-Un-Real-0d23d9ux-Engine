/**
 * U-15 / V-17 任务栏溢出折叠（AI-03 任务栏与托盘组）：
 * 纯几何计算 —— 中心区可用宽度装不下的图标收进折叠菜单。
 * 触发口径（V-17）：图标被压缩到最小宽度以下（此处以最小可用宽 40px 计）；
 * 固定（pinned）图标永远在外（锁定常驻）。
 */

export interface OverflowItem {
  id: string;
  /** 图标当前占宽（px） */
  width: number;
  /** 固定图标不参与溢出（锁定常驻） */
  pinned: boolean;
}

/** 图标最小可用宽度：低于该值视为被压缩（触发溢出阈值 = 最小宽度被压缩 20% 的等价实现）。 */
export const MIN_ICON_WIDTH = 40;

/**
 * 计算溢出项：从右往左，固定项保留；非固定项在空间不足时进折叠区。
 * capacity = 可用宽度（px）；返回溢出的 id 列表（保持原顺序）。
 */
export function computeOverflow(items: OverflowItem[], capacity: number): string[] {
  let used = 0;
  const overflow: string[] = [];
  // 先累加所有 pinned（必须显示）
  for (const it of items) if (it.pinned) used += Math.max(MIN_ICON_WIDTH, it.width);
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.pinned) continue;
    const w = Math.max(MIN_ICON_WIDTH, it.width);
    if (used + w > capacity) {
      overflow.unshift(it.id);
    } else {
      used += w;
    }
  }
  return overflow;
}
