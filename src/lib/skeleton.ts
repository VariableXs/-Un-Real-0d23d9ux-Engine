/**
 * 任务 76（专项 E1 · 加载感知骨架屏）—— 骨架屏统一规格逻辑核。
 *
 * 总案 E1 验收口径：
 * - 形状=真实内容轮廓：骨架行几何由内容组件的行高常量推导（同源常量，
 *   加载完成无跳动换位）；
 * - 代码=单组件三处复用：文件管理器/设置页/看板三类列表消费同一注册表，
 *   新增消费方=注册一行规格，不改骨架内核；
 * - 体验=加载 ≥300ms 才显示骨架（快路径不闪）；
 * - 低配/reduce-motion：静态灰版替代脉冲动画（专项 E4 联动）。
 *
 * 纯确定性逻辑（规格生成 + 显示时机判定），渲染由消费方按 spec 画灰块。
 */

// ---- 内容轮廓同源常量（与消费方真实行高一致；改行高两处同改） ----

export const CONTENT_GEOMETRY = {
  /** 文件管理器列表行高（px）——与 Files 列表行渲染一致。 */
  files: 40,
  /** 设置页分组卡高（px）。 */
  settings: 96,
  /** 看板列卡高（px）。 */
  board: 120,
} as const;

export type SkeletonConsumer = keyof typeof CONTENT_GEOMETRY;

/** 单条骨架行规格（几何=真实内容轮廓）。 */
export interface SkeletonRow {
  kind: "row" | "card" | "tile";
  /** 内容区宽度占比 0..100（标题 70% / 副行 45% 等轮廓节奏）。 */
  widthPct: number;
  heightPx: number;
  /** 首行缩进（分组卡标题偏移），px。 */
  indentPx: number;
}

export interface SkeletonSpec {
  consumer: SkeletonConsumer;
  rows: SkeletonRow[];
  /** 动效档：pulse=正常；static=reduce-motion/低配（E4 联动）。 */
  motion: "pulse" | "static";
}

/** 宽度节奏（标题→副行→徽标位），同一节奏逐行重复 = 真实列表轮廓。 */
const WIDTH_PULSE = [70, 45, 58] as const;

/** 注册表驱动：三类消费方各自的行规格生成（加消费方=加一行映射）。 */
function rowsFor(consumer: SkeletonConsumer, rowCount: number): SkeletonRow[] {
  const h = CONTENT_GEOMETRY[consumer];
  const kind: SkeletonRow["kind"] =
    consumer === "files" ? "row" : consumer === "settings" ? "card" : "tile";
  const n = Math.max(1, Math.min(24, Math.trunc(rowCount) || 1));
  return Array.from({ length: n }, (_, i) => ({
    kind,
    // i % WIDTH_PULSE.length 恒在界内，非空断言成立。
    widthPct: WIDTH_PULSE[i % WIDTH_PULSE.length]!,
    heightPx: h,
    indentPx: consumer === "settings" && i === 0 ? 12 : 0,
  }));
}

/** 生成骨架规格（唯一入口；三处消费方同走此函数=单组件复用）。 */
export function skeletonFor(
  consumer: SkeletonConsumer,
  rowCount: number,
  opts?: { reduceMotion?: boolean; lowSpec?: boolean },
): SkeletonSpec {
  const reduce = Boolean(opts?.reduceMotion);
  const low = Boolean(opts?.lowSpec);
  return {
    consumer,
    rows: rowsFor(consumer, rowCount),
    motion: reduce || low ? "static" : "pulse",
  };
}

// ---- 显示时机：快路径不闪（≥300ms 才出骨架） ----

/** 总案 E1 定值：300ms。 */
export const SKELETON_DELAY_MS = 300;

/**
 * 是否应显示骨架：仅当加载已持续 ≥300ms 且尚未完成。
 * （elapsedMs 由消费方用 performance.now 差值传入，本模块不做时钟依赖。）
 */
export function shouldShowSkeleton(elapsedMs: number, done: boolean): boolean {
  if (done) {
    return false;
  }
  return Number.isFinite(elapsedMs) && elapsedMs >= SKELETON_DELAY_MS;
}

/** 快路径提示：完成瞬间骨架立即撤除（撤除不占动画帧）。 */
export function hideImmediately(done: boolean): boolean {
  return done;
}
