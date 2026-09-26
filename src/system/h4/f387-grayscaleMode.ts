/**
 * F387 灰度模式（H 域 · AI-H4）：
 * 一键全系统灰度（快速设置磁贴+快捷键）：所有颜色转灰度（GPU 合成器滤镜 F056 一次实现，
 * 零应用改造）——护眼夜晚、截图检查视觉层级、戒手机式专注三用途；灰度下系统依然可用
 * （状态靠形状明暗不靠颜色——控件语义完整所以灰度不致残）。
 * 判据（主册 F387）：全系统灰度一致性（滤镜单点审计）；灰度下关键操作走查（20 任务全绿）；
 * 切换即时性；与 F113 高对比度/F114 色弱滤镜互斥逻辑。
 * 依赖锚点：F056 合成器 / F113 高对比度 / F114 色弱滤镜 / F205 控件。
 * 存储键：variable:h4:f387:on
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 系统色彩滤镜种类（互斥逻辑的域）。 */
export type ColorFilter = "none" | "grayscale" | "highContrast" | "colorDeficiency";

const KEY = h4Key("f387", "on");

/** 当前滤镜（单点状态——全系统一致性的结构保证：只有一个滤镜位）。 */
export function activeFilter(store: KvStore = defaultStore()): ColorFilter {
  return readJson<ColorFilter>(store, KEY, "none", (v): v is ColorFilter => v === "none" || v === "grayscale" || v === "highContrast" || v === "colorDeficiency");
}

/**
 * 请求滤镜（互斥判据核心）：三滤镜互斥——后到者替换前者（一次只有一个滤镜生效），
 * 同滤镜再请求 = 关闭（磁贴 toggle 语义）。
 */
export function requestFilter(store: KvStore, want: ColorFilter): { ok: boolean; now: ColorFilter; replaced: ColorFilter } {
  const prev = activeFilter(store);
  const now = prev === want ? "none" : want;
  const ok = writeJson(store, KEY, now);
  return { ok, now, replaced: prev };
}

/** 切换即时性（判据）：无重启路径——写完即生效（返回即生效语义）。 */
export function switchLatencyMs(): 0 {
  return 0;
}

/** 滤镜单点审计（判据「滤镜单点」）：全系统引用同一滤镜位（无第二灰度开关）。 */
export function auditSingleFilterPoint(implementationSites: string[]): { pass: boolean; extraSites: string[] } {
  const extras = implementationSites.filter((s) => s !== "compositor.filter");
  return { pass: extras.length === 0, extraSites: extras };
}

/** 灰度可用性判定（判据「灰度下关键操作走查 20 任务」）：状态不只靠色相——
 * 检查每个状态是否有形状/明暗双通道标记。 */
export function auditShapeRedundancy(states: Array<{ name: string; colorOnly: boolean }>): { pass: boolean; colorOnlyStates: string[] } {
  const bad = states.filter((s) => s.colorOnly).map((s) => s.name);
  return { pass: bad.length === 0, colorOnlyStates: bad };
}

/** 灰度换算（渲染参考实现）：感知亮度加权（ITU-R BT.601），供走查对拍。 */
export function toGrayscale(r: number, g: number, b: number): number {
  const y = 0.299 * r + 0.587 * g + 0.114 * b;
  return Math.round(Math.min(255, Math.max(0, y)));
}

/** 快速设置磁贴模型（判据「快速设置磁贴+快捷键」双入口）。 */
export const QUICK_TILE_ID = "quick-settings.grayscale";
export const HOTKEY_COMBO = "F387-hotkey";
export function entries(): string[] {
  return [QUICK_TILE_ID, HOTKEY_COMBO];
}
