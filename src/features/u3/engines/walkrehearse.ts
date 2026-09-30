/**
 * U3-v7 引擎四：walkrehearse —— 十二查 manual-walk 类查项的**预演面**
 * 深化引擎（AI-U3 · 批次七）。
 *
 * 判据唯一源（分工书《AI 分工完成图》通用验收标准十二查 + walkcheck 引擎）：
 * 「主册判据与十二查同时全绿才收工」——v4 起 manual-walk 类查项（4K 走查/
 * 可调三通则/三落位/路径链/说明句/台账证据/最丑角落/收工五勾）一直显性
 * pending 不冒领；本引擎把 pending 变成**可执行的预演工位**：
 *
 * 1. 走查脚本生成器：每条 manual-walk 查项给出具体步骤（做什么/看什么/
 *    什么算过）——真机走查不再是「凭感觉走一遍」；
 * 2. 显性 pending 登记表：50 域 × 8 条 manual-walk 查项的记录槽，未执行
 *    即 pending（不冒领全绿）；
 * 3. 工单导出：阻塞清单按多 AI 并行纪律④格式导出（卡在哪/需要什么/
 *    谁能解）——卡住的永远是任务，不是人；
 * 4. 收工五勾机检（查 12）：分类页/搜索/就地可调/说明句/路径链五勾
 *    从结构事实断言，不靠自述。
 *
 * 与 walkcheck 的分工：walkcheck 管「查项定义与清单生成」，本引擎管
 * 「manual-walk 查项怎么执行、执行到哪、还差谁」。
 */

/* ------------------------------- 走查脚本 ------------------------------- */

/** 一条走查步骤：做什么、看什么、什么算过。 */
export interface WalkStep {
  readonly action: string;
  readonly expect: string;
}

/** manual-walk 查项号（walkcheck TWELVE_QUERIES 中的 4/5/6/7/8/10/11/12）。 */
export const MANUAL_WALK_QUERIES: ReadonlyArray<number> = [4, 5, 6, 7, 8, 10, 11, 12];

/** 4K 走查四档 DPI（Windows 常用缩放四档——判据「四档 DPI 截图全绿」）。 */
export const DPI_SCALE_TIERS: ReadonlyArray<number> = [100, 125, 150, 200];

/** 走查脚本生成器（判据：每条 manual-walk 查项有具体步骤）。 */
export function rehearsalScript(queryNo: number, domain: string): ReadonlyArray<WalkStep> {
  switch (queryNo) {
    case 4: // 4K 走查
      return [
        { action: `${domain} 在 ${DPI_SCALE_TIERS.join("%/")}% 四档缩放下逐屏截图`, expect: "文字不糊不锯齿，图标矢量清晰，虚线焦点环四档都可见" },
        { action: "放大 300% 检查间距与对齐", expect: "1px 级错位为零（放大三倍看不尴尬）" },
        { action: "暗色主题下重复四档截图", expect: "对比度达标，状态色不靠色相单通道区分" },
      ];
    case 5: // 可调三通则
      return [
        { action: `枚举 ${domain} 全部可调参数并登记清单`, expect: "参数清单与实现一一对应，无隐藏参数" },
        { action: "每参数调到极值（最小/最大）观察排布", expect: "极值下不破版不遮挡，回默认恢复原状" },
        { action: "核对每参数双入口（设置页 + 就地）", expect: "两入口改同一值即时同步（一处一事实）" },
      ];
    case 6: // 三落位
      return [
        { action: `对 ${domain} 每个参数登记 A 直调/B 跳转/C 免调节三落位`, expect: "登记表覆盖全部参数，无第四类" },
        { action: "B 类跳转逐条走达", expect: "≤4 段路径链到目标面板" },
        { action: "C 类免调节逐条核对默认值", expect: "默认值即最佳值，无需用户动手" },
      ];
    case 7: // 导航路径链
      return [
        { action: `从主界面出发走 ${domain} 的功能路径并记段数`, expect: "每功能 ≤4 段到" },
        { action: "用键盘 Tab 顺序重走同一路径", expect: "焦点顺序与视觉一致，不迷路" },
        { action: "搜索/索引反查同一功能", expect: "能通过搜索一步定位" },
      ];
    case 8: // 说明句
      return [
        { action: `核对 ${domain} 每个控件的名称+说明句+调节控件三件套`, expect: "三件套齐全，说明句一句话说清不堆术语" },
        { action: "检查文案双语（zh/en）", expect: "零缺键、标点统一、无硬编码字面量" },
      ];
    case 10: // 台账与证据
      return [
        { action: `核对 ${domain} 数据、复现命令、日期三件套`, expect: "台账三件齐且可复现（跑命令得同数）" },
        { action: "抽 1 条判据按台账命令实跑", expect: "结果与台账登记一致" },
      ];
    case 11: // 最丑角落
      return [
        { action: `自记 ${domain} 最不满意的一处并登记改进方向`, expect: "有具体位置+为什么丑+怎么改，不是空话" },
      ];
    case 12: // 收工五勾
      return [
        { action: `对 ${domain} 跑收工五勾机检（分类页/搜索/就地可调/说明句/路径链）`, expect: "五勾全绿——任一缺勾不收工" },
      ];
    default:
      return [{ action: `查项 ${queryNo} 非 manual-walk 类`, expect: "走 walkcheck 自动核验通道" }];
  }
}

/* ------------------------------- 登记表 ------------------------------- */

export type WalkResult = "pass" | "fail" | "pending";

export interface WalkRecord {
  queryNo: number;
  domain: string;
  /** 4K 查项的档位（其余查项 null）。 */
  dpiPct: number | null;
  result: WalkResult;
  executedAt: number | null;
  note: string;
}

/** 生成全域显性 pending 登记表（判据：未执行即 pending——不冒领全绿）。 */
export function pendingLedger(domains: ReadonlyArray<string>): WalkRecord[] {
  const out: WalkRecord[] = [];
  for (const domain of domains) {
    for (const q of MANUAL_WALK_QUERIES) {
      if (q === 4) {
        for (const dpi of DPI_SCALE_TIERS) {
          out.push({ queryNo: q, domain, dpiPct: dpi, result: "pending", executedAt: null, note: "四档走查未执行——随闸门补测" });
        }
      } else {
        out.push({ queryNo: q, domain, dpiPct: null, result: "pending", executedAt: null, note: "真机走查 pending——不冒领" });
      }
    }
  }
  return out;
}

/** 记录执行（只允许改 pending → pass/fail，且必须带 note——防洗账；
 *  无匹配槽时原账返回（同一引用），不改写任何已入账记录）。 */
export function recordWalk(
  ledger: WalkRecord[],
  queryNo: number, domain: string, dpiPct: number | null,
  result: Exclude<WalkResult, "pending">, note: string,
): { ledger: WalkRecord[]; changed: boolean } {
  let changed = false;
  const next = ledger.map((r) => {
    if (r.queryNo === queryNo && r.domain === domain && r.dpiPct === dpiPct && r.result === "pending") {
      changed = true;
      return { ...r, result, executedAt: Date.now(), note };
    }
    return r;
  });
  return changed ? { ledger: next, changed } : { ledger, changed };
}

/* ------------------------------- 工单导出 ------------------------------- */

export interface BlockedItem {
  domain: string;
  queryNo: number;
  what: string;
  need: string;
  who: string;
}

/** 阻塞清单 → 结构化工单（多 AI 并行纪律④：卡在哪/需要什么/谁能解）。 */
export function blockedWorkorder(ledger: ReadonlyArray<WalkRecord>): { total: number; items: BlockedItem[]; text: string } {
  const pending = ledger.filter((r) => r.result === "pending");
  const items: BlockedItem[] = pending.map((r) => ({
    domain: r.domain,
    queryNo: r.queryNo,
    what: r.queryNo === 4 ? `${r.domain} ${r.dpiPct}% 档 4K 截图走查` : `查项 ${r.queryNo}（${r.domain}）真机走查`,
    need: r.queryNo === 4 ? "真机四档 DPI 环境 + 截图归档" : "真机环境 + 走查执行人",
    who: "Variable 派真机闸门（或具备真机通道的 AI 窗口）",
  }));
  const text = items
    .map((b, i) => `${i + 1}. [${b.domain}·查${b.queryNo}] 卡在哪：${b.what}｜需要什么：${b.need}｜谁能解：${b.who}`)
    .join("\n");
  return { total: items.length, items, text };
}

/* ------------------------------- 收工五勾机检 ------------------------------- */

/** 收工五勾结构事实（查 12 判据：五勾全绿才收工——从结构断言不靠自述）。 */
export interface FiveCheckFacts {
  /** 分类页在位（设置页有该域分类入口）。 */
  categoryPage: boolean;
  /** 搜索可达（搜索能定位该域功能）。 */
  searchReachable: boolean;
  /** 就地可调（面板内可直接改参数）。 */
  inlineAdjustable: boolean;
  /** 说明句齐全（名称+一句话说明）。 */
  descSentences: boolean;
  /** 路径链登记（≤4 段）。 */
  pathChainRegistered: boolean;
}

export const FIVE_CHECK_NAMES: ReadonlyArray<string> = ["分类页", "搜索", "就地可调", "说明句", "路径链"];

/** 五勾机检（判据：五勾全绿才收工——缺勾显性，红哪勾说哪勾）。 */
export function fiveCheck(f: FiveCheckFacts): { pass: boolean; missing: string[] } {
  const vals = [f.categoryPage, f.searchReachable, f.inlineAdjustable, f.descSentences, f.pathChainRegistered];
  const missing = FIVE_CHECK_NAMES.filter((_, i) => !vals[i]);
  return { pass: missing.length === 0, missing };
}

/* ------------------------------- 自检 ------------------------------- */

export function walkrehearseSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 脚本生成器：8 条 manual-walk 查项全有非空脚本
  const allScripts = MANUAL_WALK_QUERIES.map((q) => rehearsalScript(q, "deskicons"));
  checks.push({ name: "八查项脚本全在册", pass: allScripts.length === 8 && allScripts.every((s) => s.length > 0 && s.every((st) => st.action.length > 0 && st.expect.length > 0)) });
  checks.push({ name: "非 manual-walk 查项导流自动核验", pass: rehearsalScript(1, "x").length === 1 && rehearsalScript(1, "x")[0]!.expect.includes("自动核验") });
  checks.push({ name: "四档 DPI 常量在册", pass: DPI_SCALE_TIERS.length === 4 && DPI_SCALE_TIERS[0] === 100 && DPI_SCALE_TIERS[3] === 200 });

  // 登记表：pending 显性、4K 按档拆槽
  const ledger = pendingLedger(["deskicons", "copyops"]);
  const q4 = ledger.filter((r) => r.queryNo === 4);
  checks.push({ name: "pending 台账显性", pass: ledger.every((r) => r.result === "pending" && r.executedAt === null) });
  checks.push({ name: "4K 按档拆槽", pass: q4.length === 8 && new Set(q4.map((r) => r.dpiPct)).size === 4 });
  checks.push({ name: "台账槽位总数", pass: ledger.length === 2 * (7 + 4) });

  // 执行记录：pending→pass 带 note；重复执行不洗账
  const r1 = recordWalk(ledger, 5, "deskicons", null, "pass", "双入口同步验证过");
  checks.push({ name: "执行入账", pass: r1.changed && r1.ledger.find((r) => r.queryNo === 5 && r.domain === "deskicons")?.result === "pass" });
  const r2 = recordWalk(r1.ledger, 5, "deskicons", null, "fail", "再执行");
  checks.push({ name: "已入账不可改判（防洗账）", pass: !r2.changed && r2.ledger === r1.ledger });

  // 工单导出：三件齐（卡在哪/需要什么/谁能解）
  const wo = blockedWorkorder(r1.ledger);
  checks.push({ name: "工单三件套格式", pass: wo.total === ledger.length - 1 && wo.items.every((b) => b.what.length > 0 && b.need.length > 0 && b.who.length > 0) && wo.text.includes("卡在哪") && wo.text.includes("谁能解") });

  // 五勾机检：全绿/缺勾显性
  const all: FiveCheckFacts = { categoryPage: true, searchReachable: true, inlineAdjustable: true, descSentences: true, pathChainRegistered: true };
  const lacking: FiveCheckFacts = { ...all, pathChainRegistered: false, searchReachable: false };
  checks.push({ name: "收工五勾全绿", pass: fiveCheck(all).pass && fiveCheck(all).missing.length === 0 });
  checks.push({ name: "缺勾显性点名", pass: !fiveCheck(lacking).pass && fiveCheck(lacking).missing.join("/") === "搜索/路径链" });

  return checks;
}
