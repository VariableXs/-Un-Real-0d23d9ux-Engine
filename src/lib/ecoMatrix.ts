/**
 * AI-14 Z-53/Z-56 扩展兼容承诺与版本矩阵（前端层）。
 *
 * 三类第三方资源：theme（.vxs 主题）/ script（.vxsc 脚本）/ config（分享信封）。
 * 承诺：当前引擎版本向前兼容 N-1 个 minor；弃用策略 = 标记废弃 ≥2 个 minor 后移除。
 * 矩阵随版本发布更新；本模块提供运行时一致性校验（防矩阵与承诺打架）。
 */

export type EcoKind = "theme" | "script" | "config";

/** 版本矩阵条目：每个资源类型一份承诺。 */
export interface EcoMatrixEntry {
  kind: EcoKind;
  /** 资源格式版本（资源内自声明） */
  formatVersion: string;
  /** 引擎侧最低接受的资源格式版本 */
  minSupported: string;
  /** 引擎侧最高接受的资源格式版本 */
  maxSupported: string;
  /** 弃用策略：deprecated 资源仍可导入但提示迁移 */
  deprecated: boolean;
  /** 计划移除的引擎 minor（弃用时非空） */
  removalAt?: string;
}

export const ENGINE_COMPAT_BASE = "1.5";
/** 承诺：向前兼容 N-1 个 minor（当前 minor -1）。 */
export const COMPAT_MINOR_DEPTH = 1;

export const ECO_MATRIX: EcoMatrixEntry[] = [
  {
    kind: "theme",
    formatVersion: "1",
    minSupported: "1",
    maxSupported: "1",
    deprecated: false,
  },
  {
    kind: "script",
    formatVersion: "1",
    minSupported: "1",
    maxSupported: "1",
    deprecated: false,
  },
  {
    kind: "config",
    formatVersion: "1",
    minSupported: "1",
    maxSupported: "1",
    deprecated: false,
  },
];

export function entryOf(kind: EcoKind): EcoMatrixEntry {
  const hit = ECO_MATRIX.find((e) => e.kind === kind);
  if (!hit) throw new Error(`ecoMatrix 缺少 ${kind} 条目`);
  return hit;
}

export type CompatVerdict = "supported" | "deprecated" | "unsupported";

export interface CompatVerdictResult {
  verdict: CompatVerdict;
  message: string;
}

function compareVer(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

/** 资源兼容判定：≤minSupported 不接受（太老）；deprecated 标记但不拦截；>maxSupported 拒绝。 */
export function checkEcoCompat(kind: EcoKind, formatVersion: string): CompatVerdictResult {
  let e: EcoMatrixEntry;
  try {
    e = entryOf(kind);
  } catch {
    return { verdict: "unsupported", message: `未知资源类型 ${kind}` };
  }
  if (compareVer(formatVersion, e.minSupported) < 0) {
    return { verdict: "unsupported", message: `格式过旧（${formatVersion} < ${e.minSupported}），请重新导出` };
  }
  if (compareVer(formatVersion, e.maxSupported) > 0) {
    return { verdict: "unsupported", message: `格式过新（${formatVersion} > ${e.maxSupported}），请升级引擎` };
  }
  if (e.deprecated) {
    return {
      verdict: "deprecated",
      message: `该格式已标记弃用${e.removalAt ? `，将在 ${e.removalAt} 移除` : ""}，仍可导入，建议迁移`,
    };
  }
  return { verdict: "supported", message: "兼容" };
}

/** 矩阵自检：条目齐全、min ≤ max、deprecated 必须带 removalAt。测试与启动各跑一次。 */
export function matrixSelfCheck(): string[] {
  const errs: string[] = [];
  const kinds: EcoKind[] = ["theme", "script", "config"];
  for (const k of kinds) {
    try {
      const e = entryOf(k);
      if (compareVer(e.minSupported, e.maxSupported) > 0) errs.push(`${k}: minSupported > maxSupported`);
      if (e.deprecated && !e.removalAt) errs.push(`${k}: deprecated 缺 removalAt`);
    } catch (err) {
      errs.push(err instanceof Error ? err.message : String(err));
    }
  }
  return errs;
}
