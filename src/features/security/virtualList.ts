/**
 * 任务 36（AI-V）：极简虚拟列表窗口计算（定高行、滚动窗口 ± 缓冲）。
 *
 * 不引第三方依赖（项目零新增 npm 依赖纪律）。给定滚动位置与视口，返回应渲染的
 * [start, end) 区间，配合 ±overscan 缓冲行，使万条记录滚动流畅。
 *
 * 渲染层约定：外层容器固定高度 + overflow:auto，内部撑起 total*rowHeight 高的占位，
 * 仅渲染 [start,end) 区间行并 translateY(start*rowHeight)。
 */

export interface VirtualWindow {
  /** 起始索引（含）。 */
  start: number;
  /** 结束索引（不含）。 */
  end: number;
  /** 列表总高（px）。 */
  totalHeight: number;
  /** 视口顶部偏移（px），用于行容器 transform。 */
  offsetY: number;
}

export interface VirtualWindowOpts {
  /** 当前滚动距离（px）。 */
  scrollTop: number;
  /** 视口高度（px）。 */
  viewportHeight: number;
  /** 单条定高（px）。 */
  rowHeight: number;
  /** 记录总数。 */
  total: number;
  /** 上下缓冲行数（默认 6）。 */
  overscan?: number;
}

/**
 * 计算应渲染的窗口区间。
 * 边界：total=0 → 返回空窗口；滚动越界被钳制到合法区间。
 */
export function computeWindow(opts: VirtualWindowOpts): VirtualWindow {
  const { scrollTop, viewportHeight, rowHeight, total } = opts;
  const overscan = opts.overscan ?? 6;
  const totalHeight = Math.max(0, total) * rowHeight;

  if (total <= 0 || rowHeight <= 0) {
    return { start: 0, end: 0, totalHeight, offsetY: 0 };
  }

  const firstVisible = Math.floor(Math.max(0, scrollTop) / rowHeight);
  const visibleCount = Math.ceil(viewportHeight / rowHeight);
  const start = Math.max(0, firstVisible - overscan);
  const end = Math.min(total, firstVisible + visibleCount + overscan);

  return {
    start,
    end,
    totalHeight,
    offsetY: start * rowHeight,
  };
}
