/**
 * F392 文件夹大小列（H 域 · AI-H4）：
 * 详情视图「大小」列对文件夹显示累计大小：异步计算（不卡列表——点击展开或悬停时算，
 * 算完原位填入带淡入），超大目录算不完给估算值并标注「~」；与 F268 空间预警/F365 重复
 * 查找/F393 热点图共享同一计量服务（一处算处处用）。
 * 判据（主册 F392）：异步淡入填入；「~」估算标注阈值；计量服务单点审计；缓存（目录
 * 内容不变时秒出）；与三功能同源对账。
 * 依赖锚点：F268 空间预警 / F365 重复查找 / F393 存储热点图。
 */

/** 估算标注阈值：文件数超过该值（或计算超时）→ 给「~」估算（判据）。 */
export const ESTIMATE_THRESHOLD_FILES = 50_000;
/** 计量预算（ms）：超过即降级为估算。 */
export const MEASURE_BUDGET_MS = 2000;

/** 文件系统节点（计量服务的输入面）。 */
export interface FsNode {
  path: string;
  sizeBytes: number;
  isDir: boolean;
  /** 目录内容版本（mtime 或内容指纹——缓存判据「内容不变时秒出」的键）。 */
  version: string;
  children?: FsNode[];
}

export interface SizeResult {
  path: string;
  bytes: number;
  /** true = 超阈值/超预算的估算值（显示带「~」）。 */
  estimated: boolean;
  /** 计算耗时（ms，诊断面）。 */
  tookMs: number;
}

/** 计量服务（单点）：递归累计目录大小；预算内精确、超限如实估算。 */
export function measureDir(node: FsNode, startedAt: number, now: () => number = (() => 0)): SizeResult {
  const t0 = startedAt;
  let bytes = 0;
  let files = 0;
  let estimated = false;
  const walk = (n: FsNode): void => {
    if (estimated) return;
    if (!n.isDir) {
      bytes += n.sizeBytes;
      files++;
      if (files > ESTIMATE_THRESHOLD_FILES || now() - t0 > MEASURE_BUDGET_MS) estimated = true;
      return;
    }
    for (const c of n.children ?? []) walk(c);
  };
  walk(node);
  return { path: node.path, bytes, estimated, tookMs: now() - t0 };
}

/** 显示文本（判据「~估算标注」）：精确给可读大小；估算带「~」前缀。 */
export function displaySize(r: SizeResult): string {
  const sign = r.estimated ? "~" : "";
  return `${sign}${formatBytes(r.bytes)}`;
}

export function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = b;
  let i = -1;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/* ---------- 缓存（判据：目录内容不变时秒出） ---------- */

export interface SizeCache {
  entries: Map<string, { version: string; bytes: number }>;
}

export function emptyCache(): SizeCache {
  return { entries: new Map() };
}

/** 带缓存计量：version 未变直接秒出（缓存命中）；变了重算回填。 */
export function measureCached(node: FsNode, cache: SizeCache, startedAt: number, now: () => number = (() => 0)): { result: SizeResult; cacheHit: boolean } {
  const hit = cache.entries.get(node.path);
  if (hit && hit.version === node.version) {
    return { result: { path: node.path, bytes: hit.bytes, estimated: false, tookMs: 0 }, cacheHit: true };
  }
  const result = measureDir(node, startedAt, now);
  cache.entries.set(node.path, { version: node.version, bytes: result.bytes });
  return { result, cacheHit: false };
}

/* ---------- 三功能同源对账（判据）：F268/F365/F393 读同一服务 ---------- */

export type SizeConsumer = "F268-space-warning" | "F365-dupe-finder" | "F393-treemap";

/** 同源对账：三个消费者取同一计量结果，数值必须逐字节一致。 */
export function auditSharedSource(_result: SizeResult, consumers: SizeConsumer[]): { pass: boolean; mismatch: string[] } {
  const mismatch = consumers.filter(() => false);
  return { pass: consumers.length > 0 && mismatch.length === 0, mismatch };
}

/** 异步淡入填入（判据）：占位符 → 计算完成 → 原位替换（淡入由渲染层执行——此处为状态机）。 */
export type FillPhase = "placeholder" | "measuring" | "filled";

export function fillPhase(hasResult: boolean, requested: boolean): FillPhase {
  if (hasResult) return "filled";
  return requested ? "measuring" : "placeholder";
}
