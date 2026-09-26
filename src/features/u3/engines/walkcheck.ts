/**
 * U3-v4 十二查走查对账引擎（AI-U3 · walkcheck）。
 *
 * 纪律唯一源（分工书《AI 分工完成图》通用验收标准十二查）：
 * 「每项验收 = 主册判据摘文 + 十二查。主册判据与十二查同时全绿才收工」。
 *
 * 本引擎把十二查从表格纪律变成**可执行对账**：50 项 × 12 查的清单生成器
 * + 每查的证据装载器 + F550 对账增强（v4 引擎自检并入锚点域表）。
 * 对齐 F125「走查脚本工具化」先例（vx-walkcheck 形态）。
 */

/* ------------------------------ 十二查定义 ------------------------------ */

export interface WalkQuery {
  no: number;
  what: string;
  /** 证据装载方式（代码可自动核的项在此登记自动核验函数）。 */
  auto: "domain-selfcheck" | "manual-walk" | "constants";
}

export const TWELVE_QUERIES: readonly WalkQuery[] = [
  { no: 1, what: "功能完整——主册功能定义逐条实现，无降级、无占位", auto: "domain-selfcheck" },
  { no: 2, what: "无感标准——无感标准段逐条实测通过", auto: "domain-selfcheck" },
  { no: 3, what: "性能线——帧率/延迟/内存数值全达标", auto: "constants" },
  { no: 4, what: "4K 走查——四档 DPI 截图全绿，模糊=缺陷", auto: "manual-walk" },
  { no: 5, what: "可调三通则——参数清单+排布舒适+双入口可达", auto: "manual-walk" },
  { no: 6, what: "三落位登记——A 直调/B 跳转/C 免调节登记入表", auto: "manual-walk" },
  { no: 7, what: "导航路径链——≤4 段路径链登记且逐步走达", auto: "manual-walk" },
  { no: 8, what: "说明句——名称+一句话说明+调节控件三件套齐全", auto: "manual-walk" },
  { no: 9, what: "回归零破坏——既有判据全绿，无新增魔法数", auto: "domain-selfcheck" },
  { no: 10, what: "台账与证据——数据、复现命令、日期三件齐", auto: "manual-walk" },
  { no: 11, what: "最丑角落——自记本项最不满意处", auto: "manual-walk" },
  { no: 12, what: "收工五勾——分类页/搜索/就地可调/说明句/路径链", auto: "manual-walk" },
];

/* ------------------------------ F 编号 × 分工线映射 ------------------------------ */

/** U3 五十项与分工线域映射（对账的事实源——与 anchor 域表同源）。 */
export const U3_DOMAIN_MAP: ReadonlyArray<{ domain: string; fRange: string; items: readonly number[] }> = [
  { domain: "deskicons", fRange: "F501-F503", items: [501, 502, 503] },
  { domain: "locksec", fRange: "F504-F508", items: [504, 505, 506, 507, 508] },
  { domain: "filesec", fRange: "F509-F512", items: [509, 510, 511, 512] },
  { domain: "pointerfx", fRange: "F513/F514/F519/F520/F522/F523", items: [513, 514, 519, 520, 522, 523] },
  { domain: "explorerx", fRange: "F515/F517/F521/F525-F528", items: [515, 517, 521, 525, 526, 527, 528] },
  { domain: "copyops", fRange: "F524/F529-F534", items: [524, 529, 530, 531, 532, 533, 534] },
  { domain: "winkeys", fRange: "F516/F518/F535-F539/F548", items: [516, 518, 535, 536, 537, 538, 539, 548] },
  { domain: "sysdev", fRange: "F540-F547", items: [540, 541, 542, 543, 544, 545, 546, 547] },
  { domain: "clockcal", fRange: "F549", items: [549] },
  { domain: "anchor", fRange: "F550", items: [550] },
];

/** 全部 F 编号覆盖核对（50 项不重不漏）。 */
export function u3ItemCoverage(): { total: number; unique: number; missing: number[]; duplicated: number[] } {
  const all = U3_DOMAIN_MAP.flatMap((d) => d.items);
  const seen = new Set<number>();
  const duplicated: number[] = [];
  for (const n of all) {
    if (seen.has(n)) duplicated.push(n);
    seen.add(n);
  }
  const missing: number[] = [];
  for (let n = 501; n <= 550; n++) {
    if (!seen.has(n)) missing.push(n);
  }
  return { total: all.length, unique: seen.size, missing, duplicated };
}

/* ------------------------------ 对账引擎 ------------------------------ */

export type QueryEvidence =
  | { no: number; auto: "domain-selfcheck"; pass: boolean; detail: string }
  | { no: number; auto: "constants"; pass: boolean; detail: string }
  | { no: number; auto: "manual-walk"; pass: boolean; detail: string };

/** 自动核验函数注册：domain-selfcheck 类查项由引擎自检覆盖。 */
export type SelfcheckRunner = () => Array<{ name: string; pass: boolean }>;

/**
 * 生成单域十二查清单（判据：主册判据 + 十二查同时全绿才收工）。
 * domain-selfcheck 查项的 pass = 该域自检全绿（引擎化证据）；
 * constants 查项 pass = 常量登记齐全（具名常量纪律）；
 * manual-walk 查项 pass 由登记表给出（真机走查不冒领——显性 pending）。
 */
export function buildDomainChecklist(
  _domain: string,
  opts: {
    runSelfcheck: SelfcheckRunner | null;
    constantsRegistered: boolean;
    manualWalkResults: Partial<Record<number, { pass: boolean; note: string }>>;
  },
): Array<QueryEvidence> {
  const sc = opts.runSelfcheck ? opts.runSelfcheck() : [];
  const scAllPass = opts.runSelfcheck !== null && sc.every((c) => c.pass);
  return TWELVE_QUERIES.map((q): QueryEvidence => {
    if (q.auto === "domain-selfcheck") {
      return { no: q.no, auto: q.auto, pass: scAllPass, detail: scAllPass ? `${sc.length} 条引擎自检全绿` : opts.runSelfcheck === null ? "该域无引擎自检——显性缺证" : `${sc.filter((c) => !c.pass).length} 条自检红` };
    }
    if (q.auto === "constants") {
      return { no: q.no, auto: q.auto, pass: opts.constantsRegistered, detail: opts.constantsRegistered ? "判据常量全部具名登记" : "存在未具名魔法数——回炉" };
    }
    const r = opts.manualWalkResults[q.no];
    return { no: q.no, auto: q.auto, pass: r?.pass ?? false, detail: r ? r.note : "真机走查 pending——不冒领" };
  });
}

/** 全域对账汇总（50 项 × 12 查 → 收工判定）。 */
export function reconcileTwelveQueries(
  perDomain: Array<{ domain: string; evidence: QueryEvidence[] }>,
): { domains: number; greenDomains: number; blocked: Array<{ domain: string; queryNo: number; what: string }> } {
  const blocked: Array<{ domain: string; queryNo: number; what: string }> = [];
  let greenDomains = 0;
  for (const d of perDomain) {
    const bad = d.evidence.filter((e) => !e.pass);
    if (bad.length === 0) greenDomains++;
    for (const b of bad) {
      const q = TWELVE_QUERIES.find((x) => x.no === b.no);
      blocked.push({ domain: d.domain, queryNo: b.no, what: q?.what ?? `查项 ${b.no}` });
    }
  }
  return { domains: perDomain.length, greenDomains, blocked };
}

/* ------------------------------ 自检 ------------------------------ */

export function walkcheckSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 十二查在册
  checks.push({ name: "十二查清单在册", pass: TWELVE_QUERIES.length === 12 && TWELVE_QUERIES.every((q, i) => q.no === i + 1) });
  // 50 项覆盖不重不漏
  const cov = u3ItemCoverage();
  checks.push({ name: "F501-F550 覆盖不重不漏", pass: cov.unique === 50 && cov.missing.length === 0 && cov.duplicated.length === 0 });
  // 清单生成：自检绿域的 1/2/9/3 查全绿；manual-walk 未登记显性 pending
  const list = buildDomainChecklist("deskicons", {
    runSelfcheck: () => [{ name: "a", pass: true }],
    constantsRegistered: true,
    manualWalkResults: {},
  });
  const pending = list.filter((e) => e.auto === "manual-walk" && !e.pass);
  checks.push({ name: "走查 pending 显性", pass: list.find((e) => e.no === 1)?.pass === true && pending.length === 8 });
  // 无自检域显性缺证
  const list2 = buildDomainChecklist("x", { runSelfcheck: null, constantsRegistered: true, manualWalkResults: { 4: { pass: true, note: "四档走查绿" } } });
  checks.push({ name: "缺自检显性缺证", pass: list2.find((e) => e.no === 1)?.pass === false });
  // 汇总：全绿域计数与阻塞清单
  const summary = reconcileTwelveQueries([
    { domain: "good", evidence: list },
    { domain: "bad", evidence: list2 },
  ]);
  checks.push({ name: "对账汇总阻塞显性", pass: summary.domains === 2 && summary.greenDomains === 0 && summary.blocked.some((b) => b.domain === "good" && b.queryNo === 4) });
  return checks;
}
