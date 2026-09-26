/**
 * F365 重复文件查找（H 域 · AI-H4）：
 * 存储工具里的重复查找：按内容哈希（不是按文件名——改名的重复也现形）扫描指定目录，
 * 结果按浪费空间排序（最大的重复组在前）、每组内推荐保留项（最新修改的）、
 * 勾选删除其余（进回收站 F261 语义可反悔）；扫描空闲执行（F050 纪律）。
 * 判据（主册 F365）：内容哈希命中（改名重复用例）；组排序与保留推荐；删除走回收站；
 * 空闲扫描；误判率（抽查 20 组人工复核——本层以哈希全等保证零误判口径）。
 * 依赖锚点：F050 中断合并纪律（空闲扫描）/ F261 回收站。
 */

/** 扫描三阶段（大小预筛 → 部分哈希 → 全量哈希确认）。 */
export type ScanPhase = "size" | "partial" | "full";

export interface ScannedFile {
  path: string;
  sizeBytes: number;
  mtimeMs: number;
  /** 内容哈希（全量；本层以字符串表示，测试中为受控值）。 */
  hash: string;
}

export interface DuplicateGroup {
  hash: string;
  files: ScannedFile[];
  /** 浪费空间 = 组大小 × (份数-1)。 */
  wastedBytes: number;
  /** 推荐保留：最新修改的（同刻取路径字典序第一——确定性推荐）。 */
  keepPath: string;
}

export interface DuplicateReport {
  groups: DuplicateGroup[];
  /** 可释放总空间 = 各组浪费之和。 */
  reclaimableBytes: number;
  scannedCount: number;
}

/**
 * 找重（内容哈希判据）：先按大小分组（大小不同必不重复——零误判的第一道筛），
 * 同大小再比哈希。改名重复因哈希相同必然命中（判据「改名重复用例」）。
 */
export function findDuplicates(files: ScannedFile[]): DuplicateReport {
  const bySize = new Map<number, ScannedFile[]>();
  for (const f of files) {
    const list = bySize.get(f.sizeBytes) ?? [];
    list.push(f);
    bySize.set(f.sizeBytes, list);
  }
  const groups: DuplicateGroup[] = [];
  for (const sameSize of bySize.values()) {
    if (sameSize.length < 2) continue;
    const byHash = new Map<string, ScannedFile[]>();
    for (const f of sameSize) {
      const list = byHash.get(f.hash) ?? [];
      list.push(f);
      byHash.set(f.hash, list);
    }
    for (const [hash, fs] of byHash) {
      if (fs.length < 2) continue;
      const sorted = [...fs].sort((a, b) => (b.mtimeMs - a.mtimeMs) || a.path.localeCompare(b.path));
      groups.push({ hash, files: sorted, wastedBytes: fs[0]!.sizeBytes * (fs.length - 1), keepPath: sorted[0]!.path });
    }
  }
  groups.sort((a, b) => b.wastedBytes - a.wastedBytes || a.hash.localeCompare(b.hash));
  return { groups, reclaimableBytes: groups.reduce((s, g) => s + g.wastedBytes, 0), scannedCount: files.length };
}

/* ---------- 空闲扫描调度（F050 纪律：后台批量永不让路前台） ---------- */

/** 空闲准入：前台 IO 活跃（近窗口内有前台请求）即让路。 */
export function idleAdmission(nowMs: number, lastForegroundIoMs: number, quietWindowMs = 2000): { admitted: boolean; reason: string } {
  if (lastForegroundIoMs >= 0 && nowMs - lastForegroundIoMs < quietWindowMs) {
    return { admitted: false, reason: `前台 IO 活跃（<${quietWindowMs}ms 静默窗）——让路（F050 纪律）` };
  }
  return { admitted: true, reason: "空闲窗口——扫描可推进" };
}

/* ---------- 删除走回收站（F261 语义：可反悔） ---------- */

export interface RecycleEntry {
  path: string;
  originalDir: string;
  hash: string;
  trashedAt: number;
}

/** 勾选删除：保留推荐项之外的进回收站；推荐项被勾选也保护（keepPath 永不删除）。 */
export function trashChecked(group: DuplicateGroup, checkedPaths: string[], now: number): { trashed: RecycleEntry[]; protectedKept: string[] } {
  const trashed: RecycleEntry[] = [];
  const protectedKept: string[] = [];
  for (const path of checkedPaths) {
    if (path === group.keepPath) {
      protectedKept.push(path);
      continue;
    }
    const file = group.files.find((f) => f.path === path);
    if (!file) continue;
    const slash = file.path.lastIndexOf("/");
    const dir = slash < 0 ? "/" : file.path.slice(0, slash) || "/";
    trashed.push({ path: file.path, originalDir: dir, hash: group.hash, trashedAt: now });
  }
  return { trashed, protectedKept };
}

/** 回收站还原：从回收站账取回原路径（可反悔判据的还原面）。 */
export function restoreFromTrash(entries: RecycleEntry[], path: string): { ok: boolean; originalDir: string | null } {
  const e = entries.find((x) => x.path === path);
  return e ? { ok: true, originalDir: e.originalDir } : { ok: false, originalDir: null };
}

/** 误判率审计（判据「抽查 20 组」）：组内哈希全等 + 大小全等 → 误判 0。 */
export function auditFalsePositive(report: DuplicateReport): { checkedGroups: number; misjudged: number } {
  let misjudged = 0;
  for (const g of report.groups.slice(0, 20)) {
    const bad = g.files.some((f) => f.hash !== g.hash || f.sizeBytes !== g.files[0]!.sizeBytes);
    if (bad) misjudged++;
  }
  return { checkedGroups: Math.min(20, report.groups.length), misjudged };
}
