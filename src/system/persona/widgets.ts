/**
 * F163 桌面小组件开关集 · 完整设计。
 *
 * 主册判据：三组件增删改位全链录屏；时钟秒级准确；天气数据源一致（与 F101 面板同值）。
 *
 * 【功能定义】三枚官方桌面小组件：时钟（大字模拟/数字双式）/天气（F101 数据）/
 * 系统快照（F062 四体检灯迷你版）；桌面右键「添加小组件」启用；走 4K 管线与 E1
 * 令牌；自由摆放（F084 自由模式同族）。
 *
 * 【状态与异常】天气断网 → 组件显示「数据截至 XX」；时钟与系统时间强一致（F187）；
 * 组件与图标重叠 → 置于图标层之上但可穿透点击设置（显式开关）；性能保护 → 全组件
 * 渲染超预算自动降透明帧率。
 *
 * 【设计细节】组件渲染走桌面层独立脏区；透明度滑杆 20%-100%（20% 下限保可读）；
 * 时钟数字式走等宽令牌；快照组件红黄灯与 F120 同源同色；右键菜单三项固定+扩展位
 * （后续官方组件注册制）；数据更新无感（天气 30min/时钟秒/快照 10min）。
 */

import { personaStore } from "./store";

export const SECTION = "widgets";

export const OPACITY_MIN = 0.2;
export const OPACITY_MAX = 1;
/** 数据刷新节律（主册设计细节）。 */
export const REFRESH_MS: Record<WidgetKind, number> = { clock: 1000, weather: 30 * 60 * 1000, snapshot: 10 * 60 * 1000 };
/** 性能保护预算：全组件渲染超预算（帧预算 16.6ms 的 20%）自动降透明帧率。 */
export const RENDER_BUDGET_MS = 3.3;

export type WidgetKind = "clock" | "weather" | "snapshot";
export type ClockStyle = "digital" | "analog";
export type WidgetSize = "small" | "medium" | "large";

export const WIDGET_KINDS: readonly { kind: WidgetKind; zh: string; en: string }[] = [
  { kind: "clock", zh: "时钟", en: "Clock" },
  { kind: "weather", zh: "天气", en: "Weather" },
  { kind: "snapshot", zh: "系统快照", en: "System snapshot" },
] as const;

export interface WidgetInstance {
  id: string;
  kind: WidgetKind;
  x: number;
  y: number;
  size: WidgetSize;
  /** 透明度 20%-100%。 */
  opacity: number;
  /** 时钟双式（仅 clock）。 */
  clockStyle?: ClockStyle;
  /** 穿透点击（组件与图标重叠时置于图标层之上但可穿透——显式开关）。 */
  clickThrough: boolean;
}

export interface WidgetConfig {
  instances: WidgetInstance[];
  /** 性能保护是否允许自动降级（默认开）。 */
  perfGuard: boolean;
}

export function defaultWidgetConfig(): WidgetConfig {
  return { instances: [], perfGuard: true };
}

export function loadWidgetConfig(): WidgetConfig {
  const stored = personaStore.getWith(SECTION, "widgets", undefined) as Partial<WidgetConfig> | undefined;
  return {
    instances: Array.isArray(stored?.instances) ? (stored?.instances as WidgetInstance[]) : [],
    perfGuard: typeof stored?.perfGuard === "boolean" ? stored.perfGuard : true,
  };
}

export function saveWidgetConfig(c: WidgetConfig): void {
  personaStore.set(SECTION, { widgets: c });
}

export function clampOpacity(v: number): number {
  return Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, Math.round(v * 100) / 100));
}

/** 添加组件（右键菜单→选择卡→放置）。 */
export function addWidget(config: WidgetConfig, kind: WidgetKind, x: number, y: number, size: WidgetSize = "medium"): WidgetConfig {
  const inst: WidgetInstance = {
    id: `widget-${kind}-${Date.now().toString(36)}`,
    kind,
    x,
    y,
    size,
    opacity: 1,
    clockStyle: kind === "clock" ? "digital" : undefined,
    clickThrough: false,
  };
  return { ...config, instances: [...config.instances, inst] };
}

export function removeWidget(config: WidgetConfig, id: string): WidgetConfig {
  return { ...config, instances: config.instances.filter((i) => i.id !== id) };
}

export function moveWidget(config: WidgetConfig, id: string, x: number, y: number): WidgetConfig {
  return { ...config, instances: config.instances.map((i) => (i.id === id ? { ...i, x, y } : i)) };
}

export function updateWidget(config: WidgetConfig, id: string, patch: Partial<Omit<WidgetInstance, "id" | "kind">>): WidgetConfig {
  return { ...config, instances: config.instances.map((i) => (i.id === id ? { ...i, ...patch, opacity: patch.opacity !== undefined ? clampOpacity(patch.opacity) : i.opacity } : i)) };
}

/** 时钟秒级准确判定：组件时刻与系统时刻差 <1s（F187 强一致）。 */
export function clockIsAccurate(widgetMs: number, systemMs: number): boolean {
  return Math.abs(widgetMs - systemMs) < 1000;
}

/** 性能保护：总渲染耗时超预算 → 自动降透明（×0.6）+ 帧率减半；恢复由下次采样驱动。 */
export function perfGuardAction(config: WidgetConfig, totalRenderMs: number): { degrade: boolean; next: WidgetConfig } {
  if (!config.perfGuard || totalRenderMs <= RENDER_BUDGET_MS) return { degrade: false, next: config };
  return {
    degrade: true,
    next: { ...config, instances: config.instances.map((i) => ({ ...i, opacity: clampOpacity(i.opacity * 0.6) })) },
  };
}
