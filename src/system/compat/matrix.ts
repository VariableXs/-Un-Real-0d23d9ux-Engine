/**
 * AI-12 Z-15 应用适配等级库（App Compat Matrix）
 * 红线：纯数据层，无任何执行逻辑；不做自动修复（只提示不建议）；不采集上报（纯本地）。
 */
import raw from "./matrix.json";

export type CompatTier = "A" | "B" | "C";

export interface MatrixEntry {
  process: string;
  name: string;
  category: string;
  tier: CompatTier;
  symptom: string;
  advice: string;
}

export const TIER_ORDER: Record<CompatTier, number> = { A: 0, B: 1, C: 2 };

/** schema 校验：只保留合法条目（JSON 有 schema 校验，坏数据不进库）。 */
function validate(input: unknown): MatrixEntry[] {
  const entries = (input as { entries?: unknown })?.entries;
  if (!Array.isArray(entries)) return [];
  const out: MatrixEntry[] = [];
  for (const e of entries) {
    if (typeof e !== "object" || e === null) continue;
    const r = e as Record<string, unknown>;
    const process = typeof r.process === "string" ? r.process.trim().toLowerCase() : "";
    const tier = r.tier;
    if (!process || (tier !== "A" && tier !== "B" && tier !== "C")) continue;
    if (out.some((x) => x.process === process)) continue;
    out.push({
      process,
      name: typeof r.name === "string" ? r.name : process,
      category: typeof r.category === "string" ? r.category : "tool",
      tier,
      symptom: typeof r.symptom === "string" ? r.symptom : "",
      advice: typeof r.advice === "string" ? r.advice : "",
    });
  }
  return out;
}

export const COMPAT_MATRIX: MatrixEntry[] = validate(raw);
export const MATRIX_VERSION = (raw as { version?: string }).version ?? "unknown";

/** 按进程名查询（大小写不敏感、取后缀匹配，如 foo.bar/chrome.exe 也命中 chrome.exe）。 */
export function queryCompat(process: string): MatrixEntry | null {
  const p = process.trim().toLowerCase();
  if (!p) return null;
  const exact = COMPAT_MATRIX.find((e) => e.process === p);
  if (exact) return exact;
  return COMPAT_MATRIX.find((e) => p.endsWith(`\\${e.process}`) || p.endsWith(`/${e.process}`)) ?? null;
}

/** 多进程聚合：取最差等级（B/C 自动生成登记库预警，与既有 compat.hint 徽标联动）。 */
export function worstTier(entries: (MatrixEntry | null)[]): CompatTier | null {
  let worst: CompatTier | null = null;
  for (const e of entries) {
    if (!e) continue;
    if (!worst || TIER_ORDER[e.tier] > TIER_ORDER[worst]) worst = e.tier;
  }
  return worst;
}

/** 与 LauncherManager 既有 compat.hint（FS/AC 徽标）联动：矩阵等级合成徽标文本。 */
export function hintBadge(entry: MatrixEntry | null, existingHint?: string): string | null {
  if (existingHint === "FS" || existingHint === "AC") return existingHint;
  if (!entry) return null;
  return entry.tier === "C" ? "C" : entry.tier === "B" ? "B" : null;
}
