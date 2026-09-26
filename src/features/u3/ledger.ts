/**
 * F550 内核账册与编号空间（ledger.ts · 零依赖数据层）。
 *
 * 从 reconcile.ts 抽出——消除 anchor ↔ reconcile 循环依赖（anchor 需要账册
 * 自证、reconcile 需要 anchorRuntime，两者都只依赖本文件）。
 *
 * 数据源（一处一事实）：kernel/varix/src/ustar3/ 50 域 CheckSet（v1 收口，
 * 数字登记自 docs/AI-U3-完成报告.md §2 域对账总表；单测数对源码
 * `grep -c '#\[test\]'` 实测核对——copyops 实测 43，v1 报告 §2 表笔误 42，
 * 偏差已登记报告 §8）。
 */

/** 内核 ustar3 域检查项数（cargo 宿主实测口径）。 */
export const KERNEL_DOMAIN_CHECKS: Record<string, { fRange: string; checks: number; unitTests: number }> = {
  deskicons: { fRange: "F501-F503", checks: 37, unitTests: 21 },
  locksec: { fRange: "F504-F508", checks: 58, unitTests: 33 },
  filesec: { fRange: "F509-F512", checks: 53, unitTests: 28 },
  pointerfx: { fRange: "F513/F514/F519/F520/F522/F523", checks: 70, unitTests: 38 },
  explorerx: { fRange: "F515/F517/F521/F525/F526/F527/F528", checks: 82, unitTests: 39 },
  copyops: { fRange: "F524/F529-F534", checks: 93, unitTests: 43 },
  winkeys: { fRange: "F516/F518/F535-F539/F548", checks: 95, unitTests: 47 },
  sysdev: { fRange: "F540-F547", checks: 91, unitTests: 49 },
  clockcal: { fRange: "F549", checks: 12, unitTests: 7 },
  anchor: { fRange: "F550", checks: 25, unitTests: 2 },
};

/** 内核账册合计自证（616 检查项 / 307 单测——账册可信性基线）。 */
export function kernelLedgerSelfCheck(): { checksTotal: number; testsTotal: number; ledgerOk: boolean } {
  let checksTotal = 0;
  let testsTotal = 0;
  for (const d of Object.keys(KERNEL_DOMAIN_CHECKS)) {
    checksTotal += KERNEL_DOMAIN_CHECKS[d]?.checks ?? 0;
    testsTotal += KERNEL_DOMAIN_CHECKS[d]?.unitTests ?? 0;
  }
  return { checksTotal, testsTotal, ledgerOk: checksTotal === 616 && testsTotal === 307 };
}

/** 相邻锚点域的编号空间（判据：与 F400/F575 脚本合并无冲突——区间不相交即无冲突）。 */
export const NEIGHBOR_ANCHOR_SPACES: Array<{ anchor: string; range: [number, number]; owner: string }> = [
  { anchor: "F400", range: [201, 400], owner: "AI-H4 · H 域收官登记" },
  { anchor: "F550", range: [501, 550], owner: "AI-U3 · I 域批次六" },
  { anchor: "F575", range: [551, 575], owner: "AI-U4 · 批次七" },
  { anchor: "F600", range: [401, 600], owner: "AI-U4 · I 域收官登记" },
];

/** 编号空间重叠检测（合并无冲突 = 本域 501-550 与他人区间交集为空；
 *  F600 是 I 域总登记超集，豁免）。 */
export function anchorSpaceConflictFree(): { conflictFree: boolean; overlaps: string[] } {
  const overlaps: string[] = [];
  for (const a of NEIGHBOR_ANCHOR_SPACES) {
    for (const b of NEIGHBOR_ANCHOR_SPACES) {
      if (a.anchor >= b.anchor) continue; // 无序对去重
      const lo = Math.max(a.range[0], b.range[0]);
      const hi = Math.min(a.range[1], b.range[1]);
      if (lo <= hi && !(a.anchor === "F600" || b.anchor === "F600")) {
        overlaps.push(`${a.anchor}∩${b.anchor} = [${lo},${hi}]`);
      }
    }
  }
  return { conflictFree: overlaps.length === 0, overlaps };
}
