/**
 * F394 清理建议收口页（H 域 · AI-H4）：
 * 存储工具「清理建议」页汇总所有可清项：回收站（F267 已过 30 天的）、重复文件（F365
 * 扫描结果）、旧版本快照（F325 超保留期的）、缓存（各应用声明可清缓存）——每项给
 * 「可释放 X MB」与勾选框，全部勾选执行前一次性列出总账与不可逆项标注。
 * 判据（主册 F394）：四来源汇总对账（各项数字与源头一致）；总账准确；不可逆标注与
 * 二次确认；执行后空间实测回收 ≥90% 账面值。
 * 依赖锚点：F267 回收站纪律 / F325 版本快照 / F365 重复查找。
 */

/** 四来源（判据「四来源汇总」——口径源）。 */
export const CLEANUP_SOURCES = ["recycleBin", "duplicates", "oldSnapshots", "appCaches"] as const;
export type CleanupSource = (typeof CLEANUP_SOURCES)[number];

export interface CleanupItem {
  id: string;
  source: CleanupSource;
  title: string;
  /** 可释放字节（账面值）。 */
  reclaimableBytes: number;
  /** 不可逆项（缓存清了要重下、快照清了回不去）——诚实成本声明（判据）。 */
  irreversible: boolean;
  /** 不可逆项的人话代价说明。 */
  costNote: string | null;
  checked: boolean;
}

export interface CleanupLedger {
  items: CleanupItem[];
}

export function buildLedger(items: CleanupItem[]): CleanupLedger {
  return { items: items.map((i) => ({ ...i, checked: i.irreversible ? i.checked : i.checked })) };
}

/** 总账（判据「总账准确」）：勾选项字节之和；不可逆项单独列示。 */
export function summary(ledger: CleanupLedger): { totalBytes: number; checkedCount: number; irreversibleChecked: Array<{ id: string; costNote: string }> } {
  const checked = ledger.items.filter((i) => i.checked);
  return {
    totalBytes: checked.reduce((s, i) => s + i.reclaimableBytes, 0),
    checkedCount: checked.length,
    irreversibleChecked: checked.filter((i) => i.irreversible).map((i) => ({ id: i.id, costNote: i.costNote ?? "（缺代价说明——不可逆项必须给）" })),
  };
}

/** 不可逆项二次确认（判据）：含不可逆勾选项时必须显式确认才可执行。 */
export function requiresConfirmation(ledger: CleanupLedger): boolean {
  return ledger.items.some((i) => i.checked && i.irreversible);
}

export interface ExecutionResult {
  executed: CleanupItem[];
  /** 实际回收字节（执行器上报）。 */
  actuallyFreedBytes: number;
  /** 回收率 = 实收/账面（判据 ≥90% 才合格）。 */
  recoveryRate: number;
  pass: boolean;
}

/** 执行勾选项并核对回收率（判据「实测回收 ≥90% 账面值」）。 */
export function execute(ledger: CleanupLedger, freedByItem: Record<string, number>): ExecutionResult {
  const checked = ledger.items.filter((i) => i.checked);
  const claimed = checked.reduce((s, i) => s + i.reclaimableBytes, 0);
  const freed = checked.reduce((s, i) => s + (freedByItem[i.id] ?? 0), 0);
  const rate = claimed === 0 ? 1 : freed / claimed;
  return { executed: checked, actuallyFreedBytes: freed, recoveryRate: rate, pass: rate >= 0.9 };
}

/** 四来源汇总对账（判据）：页面数字必须与源头逐项一致（差一处即红）。 */
export function auditSourceReconciliation(page: CleanupItem[], sourceTotals: Record<CleanupSource, number>): { pass: boolean; mismatches: string[] } {
  const mismatches: string[] = [];
  for (const src of CLEANUP_SOURCES) {
    const pageSum = page.filter((i) => i.source === src).reduce((s, i) => s + i.reclaimableBytes, 0);
    if (pageSum !== (sourceTotals[src] ?? 0)) mismatches.push(`${src}: 页面 ${pageSum} ≠ 源头 ${sourceTotals[src] ?? 0}`);
  }
  return { pass: mismatches.length === 0, mismatches };
}

/** 可逆/不可逆分色标注（判据「可逆项和不可逆项分色标注」的模型面）。 */
export function visualClass(item: CleanupItem): "safe" | "caution" {
  return item.irreversible ? "caution" : "safe";
}

/** 默认勾选策略：可逆项默认勾、不可逆项默认不勾（防手滑——诚实成本前置）。 */
export function defaultChecked(item: Omit<CleanupItem, "checked">): CleanupItem {
  return { ...item, checked: !item.irreversible };
}
