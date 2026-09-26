/**
 * F163 桌面小组件引擎深化 · 摆放碰撞/吸附网格 + 更新调度（带抖动） + 越界回收。
 *
 * 主册判据延伸：
 * - 【设计细节】「组件渲染走桌面层独立脏区（不拖累图标刷新）」「数据更新
 *   无感（天气 30min/时钟秒/快照 10min）」「组件与图标重叠 → 置于图标层
 *   之上但可穿透点击设置」。
 * - 自由摆放（F084 自由模式同族）：8px 吸附网格 + 碰撞推开 + 越界拉回。
 */

import { REFRESH_MS, type WidgetConfig, type WidgetInstance, type WidgetKind } from "./widgets";

// ---------- 几何（尺寸档 → 像素） ----------

const SIZE_PX: Record<WidgetSize_, { w: number; h: number }> = {
  small: { w: 120, h: 120 },
  medium: { w: 200, h: 160 },
  large: { w: 320, h: 240 },
};
type WidgetSize_ = WidgetInstance["size"];

export function widgetSizePx(inst: Pick<WidgetInstance, "size">): { w: number; h: number } {
  return { ...SIZE_PX[inst.size] };
}

export const SNAP_GRID_PX = 8;

/** 吸附到 8px 网格（自由摆放的对齐纪律——手感同 F084）。 */
export function snapToGrid(x: number, y: number): { x: number; y: number } {
  return { x: Math.round(x / SNAP_GRID_PX) * SNAP_GRID_PX, y: Math.round(y / SNAP_GRID_PX) * SNAP_GRID_PX };
}

// ---------- 碰撞检测与推开 ----------

interface Rect { x: number; y: number; w: number; h: number }

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** 矩形 → Rect。 */
function rectOf(inst: WidgetInstance): Rect {
  const s = widgetSizePx(inst);
  return { x: inst.x, y: inst.y, w: s.w, h: s.h };
}

/**
 * 摆放解析：新位置与既有实例碰撞 → 沿主轴向下推开到首个空位；
 * 全程吸附网格。返回修正后的坐标（与输入相同表示无碰撞）。
 */
export function resolvePlacement(config: WidgetConfig, movedId: string, nx: number, ny: number): { x: number; y: number; pushed: boolean } {
  const moved = config.instances.find((i) => i.id === movedId);
  if (!moved) return { x: nx, y: ny, pushed: false };
  const snapped = snapToGrid(nx, ny);
  let rect: Rect = { ...rectOf(moved), x: snapped.x, y: snapped.y };
  let pushed = false;
  const others = config.instances.filter((i) => i.id !== movedId);
  for (let guard = 0; guard < 64; guard++) {
    const hit = others.find((o) => o.clickThrough !== true && intersects(rect, rectOf(o)));
    if (!hit) break;
    // 向下推开一个网格步（碰撞体的底缘 + 网格）。
    const hitRect = rectOf(hit);
    rect = { ...rect, y: hitRect.y + hitRect.h + SNAP_GRID_PX };
    pushed = true;
  }
  return { x: rect.x, y: rect.y, pushed };
}

// ---------- 越界回收（屏幕热插拔/分辨率下降后的拉回） ----------

export interface ScreenBounds { width: number; height: number }

/** 越界拉回：右/下越界的组件平移回屏内（保持网格吸附）；完全放不下缩小一档。 */
export function reclaimOffscreen(config: WidgetConfig, screen: ScreenBounds): { config: WidgetConfig; reclaimed: string[]; downsized: string[] } {
  const reclaimed: string[] = [];
  const downsized: string[] = [];
  const instances = config.instances.map((inst) => {
    const s = widgetSizePx(inst);
    let x = inst.x;
    let y = inst.y;
    let size = inst.size;
    if (x + s.w > screen.width || y + s.h > screen.height) {
      if (s.w <= screen.width && s.h <= screen.height) {
        x = Math.max(0, Math.round((screen.width - s.w) / SNAP_GRID_PX) * SNAP_GRID_PX);
        y = Math.max(0, Math.round((screen.height - s.h) / SNAP_GRID_PX) * SNAP_GRID_PX);
        reclaimed.push(inst.id);
      } else if (size !== "small") {
        size = size === "large" ? "medium" : "small";
        const s2 = widgetSizePx({ size });
        x = 0;
        y = 0;
        void s2;
        downsized.push(inst.id);
        reclaimed.push(inst.id);
      } else {
        x = 0;
        y = 0;
        reclaimed.push(inst.id);
      }
    }
    return { ...inst, x, y, size };
  });
  return { config: { ...config, instances }, reclaimed, downsized };
}

// ---------- 更新调度（节律 + 抖动——防同刻齐发） ----------

export interface UpdateSchedule {
  instanceId: string;
  kind: WidgetKind;
  intervalMs: number;
  /** 首次更新延迟（0..jitterMs 随机抖动）。 */
  firstDelayMs: number;
}

export const SCHEDULE_JITTER_MS = 15_000;

/** 为全部实例生成更新计划（时钟秒级准确不抖动——例外规则显式）。 */
export function buildUpdateSchedule(config: WidgetConfig, rand: () => number = Math.random): UpdateSchedule[] {
  return config.instances.map((inst) => {
    const base = REFRESH_MS[inst.kind];
    if (inst.kind === "clock") {
      return { instanceId: inst.id, kind: inst.kind, intervalMs: base, firstDelayMs: 0 }; // 秒级强一致（F187）
    }
    return {
      instanceId: inst.id,
      kind: inst.kind,
      intervalMs: base,
      firstDelayMs: Math.floor(rand() * SCHEDULE_JITTER_MS),
    };
  });
}

/** 下次到期时刻（当前时间基准）。 */
export function nextDueAt(schedule: UpdateSchedule, lastRunAt: number): number {
  return lastRunAt + schedule.intervalMs;
}

// ---------- F163 状态与异常深化：断网降级 + 性能执法 ----------

/** 天气数据保鲜窗（超龄 = 断网降级显示「数据截至 HH:MM」——主册【状态与异常】）。 */
export const WEATHER_STALE_MS = 2 * 60 * 60 * 1000; // 2 小时（F101 缓存节律的两倍容许）

export interface WeatherFreshness {
  stale: boolean;
  /** 降级显示文案（stale=true 时组件角标消费）。 */
  label: string;
  ageMs: number;
}

export function weatherFreshness(lastFetchAt: number | null, now: number): WeatherFreshness {
  if (lastFetchAt === null) {
    return { stale: true, label: "暂无数据——联网后自动更新", ageMs: Number.POSITIVE_INFINITY };
  }
  const ageMs = Math.max(0, now - lastFetchAt);
  const stale = ageMs > WEATHER_STALE_MS;
  return {
    stale,
    label: stale ? `数据截至 ${new Date(lastFetchAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : "实时",
    ageMs,
  };
}

/** 性能保护执法（主册「全组件渲染超预算自动降透明帧率」的判定出口）。 */
export interface PerfGuardDecision {
  triggered: boolean;
  /** 降透明系数（触发时 0.6——主册口径）。 */
  factor: number;
  detail: string;
}

export function perfGuardAction(totalRenderMs: number, budgetMs: number, perfGuardEnabled: boolean): PerfGuardDecision {
  if (!perfGuardEnabled) return { triggered: false, factor: 1, detail: "性能保护关闭——按用户选择不干预（诚实呈现卡顿风险）" };
  if (totalRenderMs <= budgetMs) return { triggered: false, factor: 1, detail: `渲染 ${totalRenderMs.toFixed(2)}ms ≤ 预算 ${budgetMs}ms——全速` };
  return { triggered: true, factor: 0.6, detail: `渲染 ${totalRenderMs.toFixed(2)}ms 超预算 ${budgetMs}ms——自动降透明 ×0.6（保帧率不保花活）` };
}
