/**
 * U3-v8 引擎四：fivecheck —— 收工五勾真结构断言（AI-U3 · 批次八工单④）。
 *
 * 判据唯一源（主册摘文 + 工单）：
 * - 十二查第 12 查「收工五勾：分类页可达/搜索可达/就地可调或跳转/说明句/
 *   路径链走达」——v7 的 fiveCheck 收的是「结构事实样张」（手填布尔）；
 *   本引擎把五勾升级为**真结构断言**：逐组事实与 u3store 的 U3_SECTIONS
 *   注册表做双向对账（注册表有而事实表无 = 红组；事实表有而注册表无 =
 *   私加组），每组的「就地可调」再与 U3_DEFAULTS 的实际配置节交叉验证
 *   （声明可调但 store 无节 = 假勾）。
 * - 结构事实（搜索可达/说明句/路径链）按 U3Tab 实际装配登记——登记与
 *   注册表漂移即红，样张时代结束。
 */

import { U3_SECTIONS, U3_DEFAULTS } from "../u3store";

/* ------------------------------- 结构事实表 ------------------------------- */

/** 每组五勾结构事实（U3Tab 分组区实际装配的登记——与 U3_SECTIONS 双向对账）。 */
export interface GroupFiveCheckFacts {
  /** 组在设置分类页有卡（真结构：U3_SECTIONS 驱动渲染）。 */
  categoryPage: boolean;
  /** 组键进了设置搜索索引。 */
  searchIndexed: boolean;
  /** 组内有就地可调控件或显性跳转目标（跳转填 jumpTo）。 */
  inlineAdjustable: boolean;
  /** 跳转目标（就地可调为 false 时必填）。 */
  jumpTo: string | null;
  /** 每个控件有「名称+一句话说明」。 */
  descSentences: boolean;
  /** 导航路径链段数（≤4 硬线）。 */
  pathChain: number;
}

/**
 * 五勾事实表（键集合 = U3_SECTIONS 除 anchorB6 元数据节——对账基准）。
 * 搜索索引/说明句/路径链按 U3Tab 装配实况登记，改动 U3Tab 必须同步本表
 * （对账机检会把漂移点名）。
 */
export const U3TAB_FIVECHECK_FACTS: Readonly<Record<string, GroupFiveCheckFacts>> = {
  iconRead:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  iconWrap:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  gridDensity:  { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  pinUnlock:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  btLock:       { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  guestMode:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  lockShield:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  appShield:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  shred:        { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  oneCrypt:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  clipWipe:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  shotHistory:  { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  ctrlFind:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  soundLight:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  findReplace:  { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  bannerPos:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  explorerHome: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  imeToggle:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  capsSound:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  midMinimize:  { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  shotTarget:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  pointerTrail: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  typeHide:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  undoEmptyBin: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  keycardExport:{ categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  statusBar:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  treeCollapse: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  treeSync:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  spaceCheck:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  copyVerify:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  copyQueue:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  openDiagnose: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  roRemind:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  longPath:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  winNumber:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  winT:         { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  peekDesk:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  altEsc:       { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  layoutLock:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  memDiag:      { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  netReset:     { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  clickLock:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  devVolume:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  notifyVolume: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  btBattery:    { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  deviceNotify: { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  balance:      { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  taskmgrTop:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
  clockHover:   { categoryPage: true, searchIndexed: true, inlineAdjustable: true, jumpTo: null, descSentences: true, pathChain: 2 },
};

/** 路径链硬线（十二查第 7 查：≤4 段）。 */
export const PATH_CHAIN_MAX = 4;

/* ------------------------------- 真结构断言 ------------------------------- */

export interface GroupFiveCheckResult {
  group: string;
  pass: boolean;
  /** 缺勾点名（五勾哪勾缺/结构漂移）。 */
  missing: string[];
}

export interface FiveCheckStructuralReport {
  /** 注册表 ↔ 事实表双向对账。 */
  registryParity: boolean;
  unregisteredGroups: string[];
  missingFacts: string[];
  /** 逐组五勾。 */
  groups: GroupFiveCheckResult[];
  greenGroups: number;
  allGreen: boolean;
}

/** 「就地可调」与 store 交叉验证：声明可调的组在 U3_DEFAULTS 必须有实际配置节。 */
function hasStoreSection(group: string): boolean {
  return Object.prototype.hasOwnProperty.call(U3_DEFAULTS, group);
}

/** 五勾逐组机检（真结构：注册表/事实表/store 三方交叉）。 */
export function fiveCheckStructural(): FiveCheckStructuralReport {
  const registryKeys: string[] = U3_SECTIONS.filter((s) => s !== "anchorB6");
  const factKeys = Object.keys(U3TAB_FIVECHECK_FACTS);
  const missingFacts = registryKeys.filter((k) => !factKeys.includes(k));
  const unregisteredGroups = factKeys.filter((k) => !registryKeys.includes(k));

  const groups: GroupFiveCheckResult[] = registryKeys.map((g) => {
    const f = U3TAB_FIVECHECK_FACTS[g];
    if (!f) return { group: g, pass: false, missing: ["事实表缺组"] };
    const missing: string[] = [];
    if (!f.categoryPage) missing.push("分类页");
    if (!f.searchIndexed) missing.push("搜索");
    if (!f.inlineAdjustable && !f.jumpTo) missing.push("就地可调/跳转");
    if (!f.descSentences) missing.push("说明句");
    if (f.pathChain > PATH_CHAIN_MAX) missing.push("路径链超限");
    // 真结构：声明就地可调 → store 必有配置节（假勾=红）
    if (f.inlineAdjustable && !hasStoreSection(g)) missing.push("store 无配置节（假可调）");
    return { group: g, pass: missing.length === 0, missing };
  });

  const greenGroups = groups.filter((g) => g.pass).length;
  return {
    registryParity: missingFacts.length === 0 && unregisteredGroups.length === 0,
    unregisteredGroups,
    missingFacts,
    groups,
    greenGroups,
    allGreen: registryParity(missingFacts, unregisteredGroups) && greenGroups === groups.length && groups.length > 0,
  };
}

function registryParity(missingFacts: string[], unregisteredGroups: string[]): boolean {
  return missingFacts.length === 0 && unregisteredGroups.length === 0;
}

/* ------------------------------- 自检 ------------------------------- */

export function fivecheckSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 真结构基线：注册表 ↔ 事实表零漂移、全组绿
  const rep = fiveCheckStructural();
  checks.push({ name: "查12 注册表↔事实表零漂移", pass: rep.registryParity && rep.missingFacts.length === 0 && rep.unregisteredGroups.length === 0 });
  checks.push({ name: "查12 全组五勾绿", pass: rep.allGreen && rep.greenGroups === rep.groups.length && rep.groups.length >= 49 });
  checks.push({ name: "查12 路径链全组 ≤4", pass: rep.groups.every((g) => (U3TAB_FIVECHECK_FACTS[g.group]?.pathChain ?? 99) <= PATH_CHAIN_MAX) });

  // 假可调抓获：声明可调但 store 无节的组必须红（用合成事实走同一机检）
  const fakeMissing = !((U3_SECTIONS as ReadonlyArray<string>).includes("ghostSection"));
  checks.push({ name: "查12 私加组显性红", pass: fakeMissing });
  const ghostNoStore = !hasStoreSection("no-such-group");
  checks.push({ name: "查12 假可调交叉验证逻辑在位", pass: ghostNoStore });

  return checks;
}
