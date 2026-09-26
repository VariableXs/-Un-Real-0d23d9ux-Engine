/**
 * J 鼠标域 · F614/F616 档案 + F623 入 vxtheme · 纵深引擎（批次七）。
 *
 * profiles.ts（v3）管档案的运行时挂载。本引擎管档案的「数据生命周期」
 * ——导出、导入、合并、迁移：
 *
 * 1. 版本化导出——档案包带 formatVersion（当前 2）；导出即快照
 *    （深拷贝 + 校验和），v1 旧包导入走迁移链（v1→v2：补 defaults
 *    字段、曲线 id 归一）。迁移链只向前，永不向后（十年接口纪律）。
 *
 * 2. 冲突裁决——导入包与现有档案同 id 同名时的三选：skip（保留
 *    现有）/ replace（整包覆盖）/ merge（字段级合并：导入包有而
 *    现有无的字段补入，冲突字段保留现有）。默认 merge——丢用户
 *    数据是章十二红线，宁可保守。
 *
 * 3. 校验和——导出包尾部 FNV-1a 校验（与 edid 指纹同一算法——
 *    仓库只有一种哈希口径）。导入时校验不过 = 拒收 + 三要素报错
 *    （不静默跳过损坏档案：静默 = 数据丢失而不自知）。
 *
 * 判据锚点：
 * - v1→v2 迁移链 → migrateProfilePack()
 * - 三选冲突裁决默认 merge → mergeProfiles()
 * - 校验和拒收路径 → verifyPack()
 * - 导出快照与运行时隔离（改运行时不动快照）→ exportPack()
 */

import { fnv1a32 } from "./edid";

const textEncoder = new TextEncoder();
/** 字符串 → FNV-1a（fnv1a32 吃字节；档案包内容先编码再哈希）。 */
function strChecksum(s: string): string {
  return fnv1a32(textEncoder.encode(s)).toString(16).padStart(8, "0");
}

/** 当前档案包格式版本。 */
export const PACK_VERSION = 2;

/** 档案字段（与 profiles.ts 四件套口径对齐的快照形态）。 */
export interface ProfileSnapshot {
  id: string;
  name: string;
  sens?: number;
  curve?: string;
  wheelMode?: string;
  sideButtons?: Record<string, string>;
  /** v2 新增：应用级档案（F616）；v1 迁移时补空。 */
  appOverrides?: Record<string, string>;
}

export interface ProfilePack {
  formatVersion: number;
  exportedAt: string;
  profiles: ProfileSnapshot[];
  /** FNV-1a(profiles 序列化) —— 尾部校验。 */
  checksum: string;
}

/* ------------------------------- 导出 ------------------------------- */

/**
 * 导出快照：运行时档案数组 → 独立包。深拷贝隔离（导出后再改运行时，
 * 快照不变——导出的那一刻就是历史，历史不可变）。
 */
export function exportPack(profiles: ProfileSnapshot[], exportedAt: string): ProfilePack {
  const snap: ProfileSnapshot[] = JSON.parse(JSON.stringify(profiles));
  return {
    formatVersion: PACK_VERSION,
    exportedAt,
    profiles: snap,
    checksum: packChecksum(snap),
  };
}

/** 校验和口径：仅对 profiles 数组算（版本/时刻可变，内容定校验）。 */
export function packChecksum(profiles: ProfileSnapshot[]): string {
  return strChecksum(JSON.stringify(profiles));
}

/* ------------------------------- 校验与迁移 ------------------------------- */

export type VerifyResult = { ok: true; pack: ProfilePack } | { ok: false; reason: string };

/** 导入前校验：结构、版本已知性、校验和三关。 */
export function verifyPack(raw: unknown): VerifyResult {
  if (typeof raw !== "object" || raw === null) return { ok: false, reason: "档案包不是有效的 JSON 对象——请确认导出文件完整。" };
  const p = raw as Partial<ProfilePack>;
  if (!Array.isArray(p.profiles)) return { ok: false, reason: "档案包缺少 profiles 数组——文件可能被截断或手改过。" };
  if (typeof p.formatVersion !== "number" || p.formatVersion < 1 || p.formatVersion > PACK_VERSION) {
    return { ok: false, reason: `档案包版本 ${String(p.formatVersion)} 不受支持（支持 1-${PACK_VERSION}）——请用新版 Varix 重新导出。` };
  }
  const sum = packChecksum(p.profiles as ProfileSnapshot[]);
  if (p.checksum !== undefined && p.checksum !== sum) {
    return { ok: false, reason: `校验和不匹配（期望 ${p.checksum}，实得 ${sum}）——文件已损坏或被修改，已拒收。` };
  }
  return { ok: true, pack: { ...(p as ProfilePack), profiles: p.profiles as ProfileSnapshot[], checksum: sum } };
}

/**
 * v1→v2 迁移链：补 appOverrides 空表、curve id 归一（v1 的
 * "Custom" 大写 → "custom"）。链式结构（migrate v(n)→v(n+1) 逐级）
 * ——未来 v3 只加一环，不改旧环（向后兼容纪律）。
 */
export function migrateProfilePack(pack: ProfilePack): ProfilePack {
  let cur = pack;
  while (cur.formatVersion < PACK_VERSION) {
    if (cur.formatVersion === 1) {
      cur = {
        ...cur,
        formatVersion: 2,
        profiles: cur.profiles.map((pr) => ({
          ...pr,
          curve: pr.curve === "Custom" ? "custom" : pr.curve,
          appOverrides: pr.appOverrides ?? {},
        })),
      };
    } else {
      throw new Error(`migrateProfilePack: 无 ${cur.formatVersion}→${cur.formatVersion + 1} 迁移环（不应到达——verifyPack 已挡）`);
    }
  }
  return cur;
}

/* ------------------------------- 冲突合并 ------------------------------- */

export type ConflictStrategy = "skip" | "replace" | "merge";

export interface MergeReport {
  /** 三选各处理了几条。 */
  skipped: number;
  replaced: number;
  merged: number;
  /** 全新入库（无冲突）。 */
  added: number;
  /** merge 中实际补入的字段（审计——「merge 到底动了什么」）。 */
  mergedFields: { id: string; fields: string[] }[];
}

/**
 * 字段级合并（默认策略）：现有档案字段优先；导入包有而现有无的补入。
 * id 与 name 永不覆盖（身份字段——覆盖身份 = 档案消失）。
 */
export function mergeProfiles(existing: ProfileSnapshot[], incoming: ProfileSnapshot[], strategy: ConflictStrategy): { result: ProfileSnapshot[]; report: MergeReport } {
  const result = existing.map((p) => ({ ...p }));
  const report: MergeReport = { skipped: 0, replaced: 0, merged: 0, added: 0, mergedFields: [] };
  for (const inc of incoming) {
    const idx = result.findIndex((p) => p.id === inc.id);
    if (idx === -1) {
      result.push({ ...inc });
      report.added++;
      continue;
    }
    if (strategy === "skip") {
      report.skipped++;
      continue;
    }
    if (strategy === "replace") {
      result[idx] = { ...inc };
      report.replaced++;
      continue;
    }
    // merge
    const cur = result[idx]!;
    const filled: string[] = [];
    for (const k of Object.keys(inc) as (keyof ProfileSnapshot)[]) {
      if (k === "id" || k === "name") continue;
      if (cur[k] === undefined && inc[k] !== undefined) {
        (cur as Record<string, unknown>)[k] = inc[k];
        filled.push(k);
      }
    }
    report.merged++;
    if (filled.length > 0) report.mergedFields.push({ id: inc.id, fields: filled });
  }
  return { result, report };
}
