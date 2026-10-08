/**
 * F169 快捷键深化 · 速查表模型 + 纯键盘走查脚本生成。
 *
 * 主册判据延伸：
 * - F169「全表条目与实际行为一致性」的对用户面：速查表（cheat sheet）
 *   是"写了的承诺"的可视化——按域分组、按修饰键聚类、可打印栅格布局；
 * - 十五章验收协议「纯键盘盲操作走一遍」：走查不是凭感觉——本模块
 *   生成逐键走查脚本（输入序列 + 期望结果），走查即执行即记录；
 * - 冲突矩阵可视化数据：哪些组合被占、被谁占、系统保留占用一目了然。
 */

import type { ChordBinding } from "./chord-engine";

// ---------- 速查表（分组 + 打印栅格） ----------

export interface CheatGroup {
  scope: "system" | "app";
  /** 组名（域内交互词典的同名组——十章一致性）。 */
  title: string;
  entries: Array<{ combo: string; actionId: string }>;
}

/** 绑定表 → 分组速查表（系统组永远在最前——权限与来源一目了然）。 */
export function buildCheatSheet(bindings: ChordBinding[]): CheatGroup[] {
  const groups = new Map<string, CheatGroup>();
  for (const b of bindings) {
    if (!b.enabled) continue;
    const scope = b.actionId.startsWith("system.") ? "system" : "app";
    const key = scope;
    if (!groups.has(key)) groups.set(key, { scope, title: scope === "system" ? "系统" : "应用", entries: [] });
    groups.get(key)!.entries.push({ combo: b.sequence.join(" "), actionId: b.actionId });
  }
  return [...groups.values()].sort((a, b) => (a.scope === "system" ? -1 : 1) - (b.scope === "system" ? -1 : 1));
}

/** 打印栅格：列主序、余数均摊（列高差 ≤1——美观是算出来的）。 */
export function printableGrid(entries: { combo: string; actionId: string }[], columns = 3): Array<Array<{ combo: string; actionId: string }>> {
  const base = Math.floor(entries.length / columns);
  const rem = entries.length % columns;
  const grid: Array<Array<{ combo: string; actionId: string }>> = [];
  let cursor = 0;
  for (let c = 0; c < columns; c++) {
    const take = base + (c < rem ? 1 : 0);
    grid.push(entries.slice(cursor, cursor + take));
    cursor += take;
  }
  return grid;
}

// ---------- 冲突矩阵（占用图谱） ----------

export interface OccupancyMatrix {
  /** 组合 → 占用者（≥2 = 冲突暴露）。 */
  occupancy: Map<string, string[]>;
  conflicts: Array<{ combo: string; holders: string[] }>;
  /** 系统保留占用（不可重录面）。 */
  reserved: string[];
}

export function occupancyMatrix(bindings: ChordBinding[], reservedCombos: string[]): OccupancyMatrix {
  const occupancy = new Map<string, string[]>();
  for (const b of bindings) {
    if (!b.enabled) continue;
    const combo = b.sequence.join(" ");
    occupancy.set(combo, [...(occupancy.get(combo) ?? []), b.actionId]);
  }
  const conflicts = [...occupancy.entries()].filter(([, holders]) => holders.length >= 2).map(([combo, holders]) => ({ combo, holders }));
  return { occupancy, conflicts, reserved: reservedCombos };
}

/** 候选组合可用性查询（重录时的即时建议——键序连接与序列契约一致：空格分两段）。 */
export function suggestFreeCombos(matrix: OccupancyMatrix, base: string, candidates: string[]): { free: string[]; taken: string[] } {
  const free: string[] = [];
  const taken: string[] = [];
  for (const c of candidates) {
    const combo = `${base} ${c}`;
    (matrix.occupancy.has(combo) || matrix.reserved.includes(combo) ? taken : free).push(combo);
  }
  return { free, taken };
}

// ---------- 纯键盘走查脚本（十五章验收协议的机械面） ----------

export interface WalkStep {
  /** 模拟输入（键序）。 */
  input: string[];
  /** 期望结果（可判定）。 */
  expect: string;
  /** 步骤说明（走查者读）。 */
  note: string;
}

export interface WalkScript {
  scope: string;
  steps: WalkStep[];
  /** 证据字段：每步要记录什么（体验日志十三章口径）。 */
  evidenceFields: string[];
}

/** 生成一条"Tab 全程可达 + Enter 激活 + Esc 退出"的盲走脚本。 */
export function blindWalkScript(scope: string, focusableCount: number): WalkScript {
  const steps: WalkStep[] = [];
  for (let i = 0; i < focusableCount; i++) {
    steps.push({ input: ["Tab"], expect: `焦点落到第 ${i + 1} 个可聚焦元素`, note: "焦点环必须可见（不许为了好看删掉）" });
  }
  steps.push(
    { input: ["Shift", "Tab"], expect: "焦点回到第 1 个（反向遍历成立）", note: "反向可达性——只验正向是半吊子走查" },
    { input: ["Enter"], expect: "当前焦点元素激活", note: "Enter 语义与鼠标点击一致" },
    { input: ["Escape"], expect: "浮层/面板关闭，焦点归还触发元素", note: "焦点不丢在宇宙里（十四章键盘纪律）" },
  );
  return { scope, steps, evidenceFields: ["时刻", "焦点元素", "可见反馈", "耗时", "体验结论"] };
}
