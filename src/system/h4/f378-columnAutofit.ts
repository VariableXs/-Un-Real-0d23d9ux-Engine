/**
 * F378 列宽双击自适应（H 域 · AI-H4）：
 * 详情列表列宽三操作：拖拽边界自由调（最小列宽 40px 防消失）、双击边界=按内容自适应
 * （该列最长可见项恰好放下）、列宽记忆归 F219；拖拽时显示宽度提示气泡（当前像素值）；
 * 最右列吃满剩余空间不留白。
 * 判据（主册 F378）：自适应算法（最长可见项+12px 余量）；拖拽气泡实时；最右列吃满；
 * 记忆联动；最小 40px 钳制。
 * 依赖锚点：F219 排序与视图记忆。
 */

/** 最小列宽（判据 40px 防消失）。 */
export const MIN_COLUMN_PX = 40;
/** 自适应余量（判据 +12px）。 */
export const AUTOFIT_PADDING_PX = 12;

export interface ColumnSpec {
  id: string;
  width: number;
  /** true = 最右列（吃满剩余空间）。 */
  last: boolean;
}

export interface CellText {
  columnId: string;
  /** 该列可见行的文本（像素宽度由 measureText 注入——本层与渲染解耦）。 */
  text: string;
}

/** 文本像素宽度测量接口（渲染层注入；测试用等宽近似）。 */
export type TextMeasurer = (text: string) => number;

/**
 * 双击自适应（判据算法）：该列最长可见项 + 12px 余量，最小 40px 钳制；
 * 空列回退 40px。
 */
export function autofitWidth(cells: CellText[], columnId: string, measure: TextMeasurer): number {
  const longest = cells.filter((c) => c.columnId === columnId).reduce((max, c) => Math.max(max, measure(c.text)), 0);
  return Math.max(MIN_COLUMN_PX, Math.ceil(longest) + AUTOFIT_PADDING_PX);
}

/** 拖拽调宽：最小 40px 钳制（判据「防消失」）。 */
export function dragWidth(requestedPx: number): number {
  if (!Number.isFinite(requestedPx)) return MIN_COLUMN_PX;
  return Math.max(MIN_COLUMN_PX, Math.round(requestedPx));
}

/** 拖拽气泡文本（判据「提示气泡实时」——实时值即返回值）。 */
export function dragBubble(requestedPx: number): string {
  return `${dragWidth(requestedPx)} px`;
}

/**
 * 最右列吃满（判据）：列宽分配后，剩余空间全部给最右列（不为负——
 * 前列吃超时最右列钳回 MIN_COLUMN_PX 并如实报告 overflow）。
 */
export function distributeWidths(columns: ColumnSpec[], tableWidth: number): { widths: Record<string, number>; overflow: boolean } {
  const widths: Record<string, number> = {};
  let used = 0;
  let lastId: string | null = null;
  for (const c of columns) {
    if (c.last) {
      lastId = c.id;
      continue;
    }
    widths[c.id] = c.width;
    used += c.width;
  }
  let overflow = false;
  if (lastId) {
    const remaining = tableWidth - used;
    if (remaining >= MIN_COLUMN_PX) {
      widths[lastId] = remaining;
    } else {
      widths[lastId] = MIN_COLUMN_PX;
      overflow = remaining < MIN_COLUMN_PX;
    }
  }
  return { widths, overflow };
}

/** F219 记忆联动：列宽快照进出（重启后保持判据的数据面）。 */
export interface ColumnMemory {
  widths: Record<string, number>;
}

export function snapshotMemory(widths: Record<string, number>): ColumnMemory {
  return { widths: { ...widths } };
}

export function restoreMemory(mem: ColumnMemory | null, columns: ColumnSpec[]): Record<string, number> {
  if (!mem) return Object.fromEntries(columns.map((c) => [c.id, c.width]));
  return Object.fromEntries(columns.map((c) => [c.id, mem.widths[c.id] ?? c.width]));
}
