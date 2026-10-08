/**
 * F161 档案深化 · 递归结构 diff + 三方合并（base/mine/theirs）。
 *
 * 主册判据延伸：
 * - F161「差异预览准确」——预览不是计数：递归 diff 精确到叶子键，
 *   逐条给出 from→to 与人话分类（新增/删除/修改）；
 * - 「冲突面板三选全对」的迁移面：跨机合并需要三方合并模型——
 *   base（共同祖先）/mine/theirs，双改同键 = 冲突显性列出（不静默覆盖）。
 */

// ---------- 递归 diff ----------

export type DiffKind = "added" | "removed" | "changed";

export interface LeafDiff {
  path: string;
  kind: DiffKind;
  from: unknown;
  to: unknown;
}

/** 递归到叶子键的结构 diff（数组整体比较——数组内序是语义）。 */
export function structuralDiff(a: unknown, b: unknown, prefix = ""): LeafDiff[] {
  if (a === b) return [];
  const bothObj = isPlainObject(a) && isPlainObject(b);
  if (!bothObj) return [{ path: prefix || "(root)", kind: "changed", from: a, to: b }];
  const out: LeafDiff[] = [];
  const ao = a as Record<string, unknown>;
  const bo = b as Record<string, unknown>;
  for (const k of Object.keys(ao)) {
    if (!(k in bo)) out.push({ path: join(prefix, k), kind: "removed", from: ao[k], to: null });
    else out.push(...structuralDiff(ao[k], bo[k], join(prefix, k)));
  }
  for (const k of Object.keys(bo)) {
    if (!(k in ao)) out.push({ path: join(prefix, k), kind: "added", from: null, to: bo[k] });
  }
  return out;
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function join(prefix: string, k: string): string {
  return prefix ? `${prefix}.${k}` : k;
}

/** diff → 人话摘要（差异预览面板直接展示）。 */
export function describeDiff(diffs: LeafDiff[], lang: "zh" | "en" = "zh"): string[] {
  const zh = { added: "新增", removed: "删除", changed: "修改" } as const;
  const en = { added: "added", removed: "removed", changed: "changed" } as const;
  const names = lang === "zh" ? zh : en;
  return diffs.map((d) => `${names[d.kind]} ${d.path}: ${previewValue(d.from)} → ${previewValue(d.to)}`);
}

function previewValue(v: unknown): string {
  if (v === null || v === undefined) return "∅";
  const s = JSON.stringify(v);
  return s !== undefined && s.length > 40 ? `${s.slice(0, 40)}…` : s ?? "∅";
}

// ---------- 三方合并（F161 跨机迁移的冲突面） ----------

export type MergeSide = "mine" | "theirs" | "base";

export interface MergeConflict {
  path: string;
  base: unknown;
  mine: unknown;
  theirs: unknown;
}

export interface MergeResult {
  merged: Record<string, unknown>;
  conflicts: MergeConflict[];
  /** 自动合并数（冲突外全部自动——冲突面板只摆真正需要人裁决的）。 */
  autoMerged: number;
}

/**
 * 三方合并：单侧改动自动取该侧；双侧同改同值自动去重；
 * 双侧改且不同 → 冲突（保留 mine 于结果、冲突列出供裁决——不静默）。
 */
export function threeWayMerge(base: unknown, mine: unknown, theirs: unknown): MergeResult {
  const conflicts: MergeConflict[] = [];
  let autoMerged = 0;
  const merged = mergeNode(base, mine, theirs, "", conflicts, () => autoMerged++, 0).value as Record<string, unknown>;
  return { merged, conflicts, autoMerged };
}

interface MergeNodeResult {
  value: unknown;
}

function mergeNode(base: unknown, mine: unknown, theirs: unknown, path: string, conflicts: MergeConflict[], countAuto: () => void, depth: number): MergeNodeResult {
  if (depth > 32) throw new Error(`合并深度超限（${path}）——疑似循环引用`);
  const eq = (x: unknown, y: unknown): boolean => stableEq(x, y);
  if (eq(mine, theirs)) {
    countAuto();
    return { value: mine }; // 双侧一致（含双侧未改）。
  }
  if (eq(base, mine)) {
    countAuto();
    return { value: theirs }; // 我没改 → 取对方。
  }
  if (eq(base, theirs)) {
    countAuto();
    return { value: mine }; // 对方没改 → 保留我的。
  }
  if (isPlainObject(mine) && isPlainObject(theirs) && isPlainObject(base)) {
    const out: Record<string, unknown> = {};
    const keys = new Set([...Object.keys(mine), ...Object.keys(theirs), ...Object.keys(base)]);
    for (const k of keys) {
      const sub = mergeNode(base[k], mine[k], theirs[k], join(path, k), conflicts, countAuto, depth + 1);
      if (sub.value !== undefined) out[k] = sub.value;
    }
    return { value: out };
  }
  conflicts.push({ path: path || "(root)", base, mine, theirs });
  return { value: mine }; // 冲突时结果暂取 mine——裁决面板决定最终值。
}

function stableEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
