/**
 * Z-08 键位注册表 — 类型定义（PB-0 地基）。
 *
 * 红线（承 APEX/SUMMIT/化境 铁律）：
 * - 任何新键位未经本注册表仲裁不得注册；
 * - 注册表落地前新键位只允许 ctrl+alt+ 空闲槽与裸功能键；
 * - Combo 一律使用规范串（小写、修饰键 ctrl/alt/shift/super 有序、键尾），
 *   归一化复用 src/lib/shortcuts.ts 的 normalizeAccel。
 */

export type KeyScope = "global" | "window" | "context";

export interface KeyBinding {
  /** 唯一 id（action id 或面板内功能 id）。 */
  id: string;
  /** 规范组合串，如 "ctrl+alt+t"。 */
  combo: string;
  /** 作用域：global=全局 / window=窗口级 / context=上下文级（组件挂载期）。 */
  scope: KeyScope;
  /** 同 scope 同 combo 冲突时数值高者优先。 */
  priority: number;
  /** 注册来源（便于审计与占用榜展示），如 "app.tsx" / "settings" / "Z-14:profile"。 */
  source: string;
  /** i18n 词典 key（速查浮层/提示条展示用）。 */
  descKey: string;
}

export type ConflictKind = "error" | "warn";

export interface ConflictRecord {
  kind: ConflictKind;
  combo: string;
  /** 冲突双方 binding id。 */
  ids: [string, string];
  /** 同 scope = error；跨 scope 同 combo = warn。 */
  reason: string;
}

export type RegisterResult =
  | { ok: true; binding: KeyBinding }
  | { ok: false; error: string; conflicts: ConflictRecord[] };
