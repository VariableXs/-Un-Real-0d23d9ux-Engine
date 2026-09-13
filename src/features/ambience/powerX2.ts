/**
 * UNREAL-X AI-02 · 族0011/0012/0013/0018/0019/0020 氛围侧落点（X00251~X00500 氛围参数）。
 *
 * 电源剧场氛围预设：仪式/唤醒各阶段的环境光档（供组件映射到
 * design/tokens.css 语义 token，模块本身只出归一档位，不写裸色值）。
 */

/** 环境光档位：0 = 全亮（现状），1~4 逐级压暗。 */
export const AMBIENT_LEVELS = [0, 1, 2, 3, 4] as const;
export type AmbientLevel = (typeof AMBIENT_LEVELS)[number];

/** 关机仪式各阶段的环境光档（与 oobe/powerTheater.CEREMONY_STAGES 对齐，取前 8）。 */
export const SHUTDOWN_AMBIENT: readonly AmbientLevel[] = [0, 1, 2, 3, 3, 4, 4, 4];

/** 唤醒剧场各阶段的环境光档（与 WAKE_STAGES 对齐，取前 5）。 */
export const WAKE_AMBIENT: readonly AmbientLevel[] = [4, 3, 2, 1, 0];

/** 环境光档 → 语义 token 名（组件用 var(...) 引用，禁止裸值）。 */
export function ambientToken(level: number): string {
  const l = Math.min(4, Math.max(0, Math.round(level) || 0));
  return `--aurora-power-veil-l${l}`;
}

/** reduce-motion 降级：所有档压平为纯淡入淡出（档位只取 0 或 4）。 */
export function ambientFlat(level: number, reduceMotion: boolean): AmbientLevel {
  if (!reduceMotion) return (Math.min(4, Math.max(0, Math.round(level) || 0)) as AmbientLevel);
  return level >= 3 ? 4 : 0;
}

/** 仪式氛围摘要（设置页展示）。 */
export function ambientCaption(kind: "shutdown" | "wake"): string {
  const arr = kind === "shutdown" ? SHUTDOWN_AMBIENT : WAKE_AMBIENT;
  return `${kind === "shutdown" ? "关机" : "唤醒"}氛围：${arr.join("→")}（0 全亮 → 4 全暗）`;
}
