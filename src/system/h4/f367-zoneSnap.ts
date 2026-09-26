/**
 * F367 桌面分区吸附（H 域 · AI-H4）：
 * 桌面图标拖近已排布图标的对齐线（8px 内）时显示吸附参考线自动贴齐——随手放也整齐；
 * 「分区」可选启用（桌面划四个逻辑区：工作/下载/待办/收藏，拖图标进区时区域淡显边界，
 * 右键「按区排列」一键归位）；分区是可选习惯工具不是强制结构。
 * 判据（主册 F367）：吸附 8px 阈值与参考线；分区四区定义与淡显；按区排列用例；
 * 未启用零差异判据。
 * 依赖锚点：F084 图标拖拽网格。
 */

export const SNAP_THRESHOLD_PX = 8;

export type ZoneId = "work" | "downloads" | "todo" | "favorites";

/** 分区四区定义（判据「四区定义」——一处一事实）。 */
export const ZONES: Array<{ id: ZoneId; name: string }> = [
  { id: "work", name: "工作" },
  { id: "downloads", name: "下载" },
  { id: "todo", name: "待办" },
  { id: "favorites", name: "收藏" },
];

export interface GridCell {
  x: number;
  y: number;
}

export interface SnapResult {
  /** 吸附后的网格坐标（吸附与否都给出落点——自由位即原坐标）。 */
  cell: GridCell;
  /** 吸附参考线（有吸附才出现——参考线与吸附同源）。 */
  guides: { vertical: number | null; horizontal: number | null };
  /** 是否发生吸附（<8px 才吸——超阈值保持自由位）。 */
  snapped: boolean;
}

/**
 * 网格吸附：拖动坐标在 8px 内贴齐网格线并给出参考线；超过阈值自由放。
 * 「未启用零差异」判据：禁用时本函数不被调用（结构上无副作用路径）。
 */
export function snapToGrid(px: number, py: number, gridStep = 100, threshold = SNAP_THRESHOLD_PX): SnapResult {
  const gx = Math.round(px / gridStep) * gridStep;
  const gy = Math.round(py / gridStep) * gridStep;
  const dx = Math.abs(px - gx);
  const dy = Math.abs(py - gy);
  const doX = dx <= threshold;
  const doY = dy <= threshold;
  return {
    cell: { x: doX ? gx : px, y: doY ? gy : py },
    guides: { vertical: doX ? gx : null, horizontal: doY ? gy : null },
    snapped: doX || doY,
  };
}

/* ---------- 分区（可选启用） ---------- */

export interface ZoneLayout {
  enabled: boolean;
  /** 每区矩形（桌面坐标系，非重叠）。 */
  rects: Record<ZoneId, { x: number; y: number; w: number; h: number }>;
}

/** 默认四区：竖向四等分（调用方可给任意矩形布局——四区定义不变）。 */
export function defaultZoneLayout(desktop: { w: number; h: number }): ZoneLayout {
  const h = Math.floor(desktop.h / 4);
  const mk = (i: number) => ({ x: 0, y: i * h, w: desktop.w, h: i === 3 ? desktop.h - h * 3 : h });
  return {
    enabled: false,
    rects: { work: mk(0), downloads: mk(1), todo: mk(2), favorites: mk(3) },
  };
}

/** 点所在分区（区外返回 null——不强行归类）。 */
export function zoneAt(layout: ZoneLayout, x: number, y: number): ZoneId | null {
  if (!layout.enabled) return null;
  for (const z of ZONES) {
    const r = layout.rects[z.id];
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return z.id;
  }
  return null;
}

/** 淡显边界参数（判据「区域淡显」——拖拽经过区时渲染面取用）。 */
export function zoneHighlight(layout: ZoneLayout, zone: ZoneId): { visible: boolean; opacity: number; rect: { x: number; y: number; w: number; h: number } } {
  const r = layout.rects[zone];
  return { visible: layout.enabled, opacity: 0.08, rect: r };
}

/** 按区排列（右键「按区排列」）：每区图标按名排序后网格铺排进该区。 */
export function arrangeByZones(layout: ZoneLayout, items: Array<{ id: string; zone: ZoneId | null; name: string }>, cell = 100): Array<{ id: string; x: number; y: number }> {
  const placed: Array<{ id: string; x: number; y: number }> = [];
  if (!layout.enabled) return placed; // 未启用零差异：不产生任何摆放指令
  for (const z of ZONES) {
    const inZone = items.filter((i) => i.zone === z.id).sort((a, b) => a.name.localeCompare(b.name));
    const r = layout.rects[z.id];
    const cols = Math.max(1, Math.floor(r.w / cell));
    inZone.forEach((item, idx) => {
      placed.push({ id: item.id, x: r.x + (idx % cols) * cell, y: r.y + Math.floor(idx / cols) * cell });
    });
  }
  return placed;
}

/** 未启用零差异审计：关闭分区后，拖放落位与普通网格（F084）完全一致。 */
export function disabledZeroDifference(layout: ZoneLayout, px: number, py: number, gridStep = 100): boolean {
  if (layout.enabled) return false;
  const plain = snapToGrid(px, py, gridStep);
  return zoneAt(layout, px, py) === null && plain.snapped;
}
