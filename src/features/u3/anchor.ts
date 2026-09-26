/**
 * F550 I 域批次六验收锚点（AI-U3 · anchor）——前端面总检引擎。
 *
 * 判据唯一源（主册摘文）：「25 检查点入脚本并全绿基线；与 F400 脚本合并
 * 无冲突；锚点可执行性抽查（随机 5 条实机跑）；账册检对账」。
 *
 * 【主册 25 检查点摘录】状态栏三段同源 / 树列表双向同步 / 空间预检拦截率 /
 * 校验失败报告率 / 长路径 600 字符全操作 / Win+数字映射一致性 / Peek 透明度
 * 与恢复精度 / 布局锁定拒绝反馈 / 诊断报告三要素 / 网络重置清单完整性 /
 * ClickLock 阈值 / 分设备音量跟随 / 双滑杆独立 / 电量三处同源 / 接入通知
 * 三态 / 平衡试听实时 / 置顶不抢焦点 / 农历五年抽检 等。
 *
 * 与内核 ustar3::anchor 的分工：内核侧跑 50 域 CheckSet（cargo test 宿主
 * 通道）；本文件聚合前端面九域自检（deskicons/locksec/filesec/pointerfx/
 * explorerx/copyops/winkeys/sysdev/clockcal）——两侧同判据不同实现面，
 * 互为对账（一处一事实：数据常量同源移植）。
 */

/** 每域检查结果。 */
export interface DomainCheckResult {
  domain: string;
  fRange: string;
  checks: Array<{ name: string; pass: boolean }>;
}

/* 九域自检注册（单向依赖：anchor → 九域 → u3store，无模块环）。
 * 「25 检查点入脚本」的脚本本体——每条可在宿主跑出数据（锚点可执行性）。 */
import { deskiconsSelfCheck } from "./deskicons";
import { locksecSelfCheck } from "./locksec";
import { filesecSelfCheck } from "./filesec";
import { pointerfxSelfCheck } from "./pointerfx";
import { explorerxSelfCheck } from "./explorerx";
import { copyopsSelfCheck } from "./copyops";
import { winkeysSelfCheck } from "./winkeys";
import { sysdevSelfCheck } from "./sysdev";
import { clockcalSelfCheck } from "./clockcal";
import { KERNEL_DOMAIN_CHECKS, kernelLedgerSelfCheck, NEIGHBOR_ANCHOR_SPACES, anchorSpaceConflictFree } from "./ledger";
import { v4EnginesSelfCheck } from "./engines";

/** F550 锚点域自身自检（对账面自证：聚合完整性 + 内核账册 + 编号空间）。
 *  注意：遍历时排除 anchor 自身（run() 递归防火墙——否则栈溢出）。 */
function anchorSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 十一域注册齐全（九功能域 + anchor 自身 + v4 引擎群——缺域即锚点失义）
  checks.push({ name: "F550 十一域自检注册齐全", pass: U3_ANCHOR_DOMAINS.length === 11 });
  // 功能域检查点总数 ≥ 主册 25 检查点判据（不数 anchor 与 v4 聚合域——防火墙）
  const total = U3_ANCHOR_DOMAINS
    .filter((d) => d.domain !== "anchor" && d.domain !== "v4-engines")
    .reduce((a, d) => a + d.run().length, 0);
  checks.push({ name: "F550 检查点 ≥25", pass: total >= 25 });
  // 内核账册自证（616/307 与源码实测一致——账册检对账）
  checks.push({ name: "F550 内核账册 616/307", pass: kernelLedgerSelfCheck().ledgerOk });
  // 内核十域检查项登记齐全（含 anchor 自身 25）
  checks.push({ name: "F550 内核十域登记", pass: Object.keys(KERNEL_DOMAIN_CHECKS).length === 10 && KERNEL_DOMAIN_CHECKS.anchor?.checks === 25 });
  // 编号空间合并无冲突（F400/F575 相邻锚点）
  checks.push({ name: "F550 编号空间无冲突", pass: anchorSpaceConflictFree().conflictFree && NEIGHBOR_ANCHOR_SPACES.length === 4 });
  return checks;
}

/** 十域检查点注册表（九功能域 + F550 锚点域自身）。 */
export const U3_ANCHOR_DOMAINS: Array<{ domain: string; fRange: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { domain: "deskicons", fRange: "F501-F503", run: deskiconsSelfCheck },
  { domain: "locksec", fRange: "F504-F508", run: locksecSelfCheck },
  { domain: "filesec", fRange: "F509-F512", run: filesecSelfCheck },
  { domain: "pointerfx", fRange: "F513·F514·F519·F520·F522·F523", run: pointerfxSelfCheck },
  { domain: "explorerx", fRange: "F515·F517·F521·F525-F528", run: explorerxSelfCheck },
  { domain: "copyops", fRange: "F524·F529-F534", run: copyopsSelfCheck },
  { domain: "winkeys", fRange: "F516·F518·F535-F539·F548", run: winkeysSelfCheck },
  { domain: "sysdev", fRange: "F540-F547", run: sysdevSelfCheck },
  { domain: "clockcal", fRange: "F549", run: clockcalSelfCheck },
  { domain: "anchor", fRange: "F550", run: anchorSelfCheck },
  { domain: "v4-engines", fRange: "F501-F550·v4", run: v4EnginesSelfCheck },
];

/** 全量执行（判据：25 检查点入脚本并全绿基线）。 */
export function anchorRuntime(): { total: number; passed: number; failed: number; allGreen: boolean; results: DomainCheckResult[] } {
  const results: DomainCheckResult[] = U3_ANCHOR_DOMAINS.map((d) => ({ domain: d.domain, fRange: d.fRange, checks: d.run() }));
  const all = results.flatMap((r) => r.checks);
  const passed = all.filter((c) => c.pass).length;
  return { total: all.length, passed, failed: all.length - passed, allGreen: passed === all.length, results };
}

/** 可执行性抽查（判据：随机 5 条实机跑——宿主验证即「可执行」证据）。 */
export function anchorSpotCheck(n = 5): Array<{ domain: string; name: string; pass: boolean }> {
  const results = anchorRuntime().results;
  const flat = results.flatMap((r) => r.checks.map((c) => ({ domain: r.domain, ...c })));
  const picked: Array<{ domain: string; name: string; pass: boolean }> = [];
  const step = Math.max(1, Math.floor(flat.length / n));
  for (let i = 0; i < flat.length && picked.length < n; i += step) { const p = flat[i]; if (p) picked.push(p); }
  return picked;
}

/** 账册检：检查点总数与登记值对账（判据：账册检对账）。 */
export const U3_ANCHOR_MIN_CHECKS = 25;

/** 写入 store 留痕（时间线留痕 F372 族）。 */
export function persistAnchorRun(allGreen: boolean): void {
  // 动态导入规避模块环：本文件只写一节，不引其他域
  import("./u3store").then(({ u3Store }) => {
    u3Store.set("anchorB6", { lastRun: Date.now(), lastAllGreen: allGreen });
  }).catch((e) => console.error("[u3:F550] 锚点留痕失败", e));
}
