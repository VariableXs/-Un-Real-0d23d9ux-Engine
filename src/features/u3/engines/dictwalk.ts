/**
 * U3-v6 引擎二：dictwalk —— 十章「全局一致性」交互词典的 U3 领地
 * 走查面（AI-U3 · 批次六）。
 *
 * 判据覆盖（深化面）：
 * - 十章交互词典：U3 领地每个浮层/面板的「关闭路径清单」机检——
 *   点外部、Esc、再点触发钮、失焦四条出路必须齐全；
 * - 四章键盘与焦点：浮层打开焦点落入、关闭焦点归还触发元素的
 *   归还链机检；
 * - 十三章体验日志挂钩：每个词典条目绑定埋点域（体验可回放）。
 *
 * 与 AI-E1 ux-dictionary 的边界：E1 管跨域通用词典（全局规则源），
 * 本引擎是 U3 领地的**条目级走查账本**——逐浮层登记出路清单并机检，
 * 红项显性化（不冒领全绿）。
 */

/* ------------------------------- 词典模型 ------------------------------- */

/** 浮层关闭出路四条（二章公理 + 四章焦点）。 */
export type DismissalPath = "outside-click" | "escape" | "re-trigger" | "blur";

/** 词典条目：U3 领地一个浮层/面板的交互契约。 */
export interface DictEntry {
  /** 稳定 id（埋点 targetId 同源）。 */
  id: string;
  /** 层级：popover < modal < system（十章：什么层级用什么浮层）。 */
  layer: "popover" | "modal" | "system";
  /** 声明的关闭出路（机检要求四条全齐）。 */
  dismissals: ReadonlyArray<DismissalPath>;
  /** 焦点归还链：打开时焦点自哪个元素来（关闭时还回去）。 */
  focusReturnTo: string | null;
  /** 体验日志埋点域（十三章同源）。 */
  logDomain: string;
}

/** 四条出路齐全判据（二章：完整消失路径）。 */
export function dismissalsComplete(d: DictEntry): boolean {
  const need: ReadonlyArray<DismissalPath> = ["outside-click", "escape", "re-trigger", "blur"];
  return need.every((p) => d.dismissals.includes(p));
}

/** 焦点链完整判据（四章：不许丢在宇宙里）。 */
export function focusChainComplete(d: DictEntry): boolean {
  return d.focusReturnTo !== null && d.focusReturnTo.length > 0;
}

/** 层级冲突检测：同刻打开的两个浮层，低层不得盖高层（十章一套规则）。 */
export function layerConflict(
  a: { id: string; layer: DictEntry["layer"] },
  b: { id: string; layer: DictEntry["layer"] },
): string | null {
  const rank: Record<DictEntry["layer"], number> = { popover: 0, modal: 1, system: 2 };
  if (rank[a.layer] > rank[b.layer]) return `${a.id}(${a.layer}) 压 ${b.id}(${b.layer})——低层盖高层`;
  return null;
}

/* ------------------------------- U3 领地条目账本 ------------------------------- */

/** U3 领地浮层/面板条目（走查对象登记表——新增浮层必须先入账再写码）。 */
export const U3_DICT_ENTRIES: ReadonlyArray<DictEntry> = [
  { id: "u3-lab-run",          layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "lab-run-all",  logDomain: "lab" },
  { id: "u3-walkcheck-detail", layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "walkcheck-row", logDomain: "walkcheck" },
  { id: "u3-lockmount-panel",  layer: "modal",   dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "lockmount-row", logDomain: "lab" },
  { id: "u3-desk-context",     layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "desk-surface",  logDomain: "f501" },
  { id: "u3-exp-tree-menu",    layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "exp-tree",      logDomain: "f527" },
  { id: "u3-clock-tooltip",    layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "clock-slot",    logDomain: "f549" },
  { id: "u3-tray-overflow",    layer: "popover", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "tray-arrow",    logDomain: "f516" },
  { id: "u3-action-center",    layer: "modal",   dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: "clock-slot",    logDomain: "f516" },
  { id: "u3-pin-entry",        layer: "system",  dismissals: ["escape", "re-trigger", "blur"],                   focusReturnTo: "lockmount-row", logDomain: "f504" },
];

/* ------------------------------- 走查引擎 ------------------------------- */

export interface DictRow {
  id: string;
  dismissalsOk: boolean;
  missing: ReadonlyArray<DismissalPath>;
  focusOk: boolean;
  logOk: boolean;
}

/** 逐条走查（红项带缺失清单——不冒领全绿）。 */
export function walkDictEntries(entries: ReadonlyArray<DictEntry> = U3_DICT_ENTRIES): Array<DictRow> {
  return entries.map((d) => {
    const missing = (["outside-click", "escape", "re-trigger", "blur"] as ReadonlyArray<DismissalPath>)
      .filter((p) => !d.dismissals.includes(p));
    return {
      id: d.id,
      dismissalsOk: missing.length === 0,
      missing,
      focusOk: focusChainComplete(d),
      logOk: d.logDomain.length > 0,
    };
  });
}

/** 账本级红绿（系统层锁屏 PIN 允许无 outside-click——锁屏语义内无「外」）。 */
export function dictWalkVerdict(rows: ReadonlyArray<DictRow>, entries: ReadonlyArray<DictEntry>): Array<{ id: string; pass: boolean; reason: string }> {
  return rows.map((r, i) => {
    const layer = entries[i]?.layer;
    if (layer === "system") {
      // 系统层例外：outside-click 豁免（锁屏内不存在「点击外部」语义），其余三条必须齐
      const nonOutside = r.missing.filter((m) => m !== "outside-click");
      return { id: r.id, pass: nonOutside.length === 0 && r.focusOk, reason: nonOutside.length === 0 ? "系统层豁免 outside-click，余三条齐全" : `缺 ${nonOutside.join("/")}` };
    }
    return { id: r.id, pass: r.dismissalsOk && r.focusOk, reason: r.dismissalsOk && r.focusOk ? "四出路+焦点归还齐全" : `缺 ${(r.missing.length ? r.missing : ["焦点归还"]).join("/")}` };
  });
}

/* ------------------------------- 自检 ------------------------------- */

export function dictwalkSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 全账本走查：除系统层 PIN 外四出路全齐
  const rows = walkDictEntries();
  const verdicts = dictWalkVerdict(rows, U3_DICT_ENTRIES);
  checks.push({ name: "词典账本 9 条在册", pass: rows.length === 9 });
  checks.push({ name: "非系统层四出路全齐", pass: rows.filter((_r, i) => U3_DICT_ENTRIES[i]!.layer !== "system").every((r) => r.dismissalsOk) });
  checks.push({ name: "系统层豁免后全绿", pass: verdicts.every((v) => v.pass) });
  checks.push({ name: "焦点归还链全非空", pass: rows.every((r) => r.focusOk) });
  checks.push({ name: "埋点域全覆盖", pass: rows.every((r) => r.logOk) });

  // 破坏注入：出路残缺 → 显性红
  const broken: DictEntry = { id: "bad", layer: "popover", dismissals: ["escape"], focusReturnTo: "x", logDomain: "x" };
  const br = dictWalkVerdict(walkDictEntries([broken]), [broken]);
  checks.push({ name: "破坏注入：缺出路显性红", pass: br[0]!.pass === false && br[0]!.reason.includes("outside-click") });

  // 破坏注入：焦点丢宇宙 → 红
  const noFocus: DictEntry = { id: "bad2", layer: "modal", dismissals: ["outside-click", "escape", "re-trigger", "blur"], focusReturnTo: null, logDomain: "x" };
  const nf = dictWalkVerdict(walkDictEntries([noFocus]), [noFocus]);
  checks.push({ name: "破坏注入：焦点链断显性红", pass: nf[0]!.pass === false });

  // 层级冲突
  checks.push({ name: "低层压高层显性", pass: layerConflict({ id: "a", layer: "system" }, { id: "b", layer: "popover" }) !== null });
  checks.push({ name: "同层不冲突", pass: layerConflict({ id: "a", layer: "popover" }, { id: "b", layer: "popover" }) === null });

  return checks;
}
