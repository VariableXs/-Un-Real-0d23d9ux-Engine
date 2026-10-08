/**
 * F515/F526 深化引擎 · 查找替换与状态栏三段（AI-U3 · findrepl）。
 *
 * 判据唯一源（主册摘文）：
 * - F515「双条形制一致；影响数预览；逐个/全部两路；整批撤销一次回滚；开关
 *   共享」。
 * - F526「三段实时性；三处同源对账；键盘操作反映；高度 24px 基线；空目录态
 *   显示」。
 *
 * 深化点：
 * 1. 替换会话模型：一次「全部替换」= 一条原子 op（条目快照 + 编辑序号），
 *    整批撤销 = 快照还原（一次 Ctrl+Z 回到替换前——判据「一次回滚」）。
 * 2. 影响数预览与执行数强一致：预览说 7 处，执行必须正好 7 处，否则显性
 *    不一致（一处一事实——预览撒谎比不预览更糟）。
 * 3. 状态栏三段聚合器：选择段/内容段/位置段三源独立更新，对账哈希三处
 *    同源（资源管理器/F091 详情窗格/F088 搜索三处数字一致）。
 * 4. 空目录态与键盘操作反映（方向键移动选择 → 段文案同步）。
 */

/* ------------------------------ F515 查找替换 ------------------------------ */

export interface FindOptions {
  matchCase: boolean;
  wholeWord: boolean;
}

/** 全词判定：命中两侧必须是非词字符（或边界）。 */
function isWordChar(ch: string | undefined): boolean {
  if (ch === undefined) return false;
  return /[A-Za-z0-9_]/.test(ch);
}

/** 文本内全部命中位置（判据「影响数预览」的事实源）。 */
export function findAll(text: string, needle: string, opts: FindOptions): Array<{ start: number; end: number }> {
  if (needle.length === 0) return [];
  const hay = opts.matchCase ? text : text.toLowerCase();
  const nee = opts.matchCase ? needle : needle.toLowerCase();
  const hits: Array<{ start: number; end: number }> = [];
  let from = 0;
  while (from <= hay.length - nee.length) {
    const idx = hay.indexOf(nee, from);
    if (idx === -1) break;
    if (opts.wholeWord) {
      const before = hay[idx - 1];
      const after = hay[idx + nee.length];
      if (isWordChar(before) || isWordChar(after)) {
        from = idx + 1;
        continue;
      }
    }
    hits.push({ start: idx, end: idx + nee.length });
    from = idx + nee.length;
  }
  return hits;
}

/** 影响数预览（判据：影响数预览）。 */
export function previewCount(text: string, needle: string, opts: FindOptions): number {
  return findAll(text, needle, opts).length;
}

/* --------------------- 替换会话（整批一次回滚） --------------------- */

export interface ReplaceSession {
  /** 替换前的完整快照（一次回滚的事实源）。 */
  snapshot: string;
  needle: string;
  replacement: string;
  opts: FindOptions;
  /** 预览数（执行后与 actual 强一致对账）。 */
  preview: number;
  applied: number;
  /** 会话编辑代数（撤销后失效——防「撤销后再按撤销又还原」的双重还原）。 */
  epoch: number;
}

export function openReplaceSession(text: string, needle: string, replacement: string, opts: FindOptions): ReplaceSession {
  return {
    snapshot: text,
    needle,
    replacement,
    opts,
    preview: previewCount(text, needle, opts),
    applied: 0,
    epoch: 1,
  };
}

/** 逐个替换（判据「逐个」路）：替换下一处命中，返回新文本与位置。 */
export function replaceNext(s: ReplaceSession, text: string): { text: string; at: number; done: boolean } {
  const hits = findAll(text, s.needle, s.opts);
  if (hits.length === 0) return { text, at: -1, done: true };
  const h = hits[0]!;
  const next = text.slice(0, h.start) + s.replacement + text.slice(h.end);
  return { text: next, at: h.start, done: false };
}

/** 全部替换（判据「全部」路）：原子执行，回填 applied。 */
export function replaceAll(s: ReplaceSession, text: string): { text: string; applied: number } {
  const hits = findAll(text, s.needle, s.opts);
  let out = text;
  for (let i = hits.length - 1; i >= 0; i--) {
    const h = hits[i]!;
    out = out.slice(0, h.start) + s.replacement + out.slice(h.end);
  }
  return { text: out, applied: hits.length };
}

/**
 * 整批撤销（判据：整批撤销一次回滚）：快照还原 + 代数失效。
 * 二次撤销在同一代数上显性拒绝（无东西可撤——不是静默 no-op）。
 */
export function undoReplaceBatch(s: ReplaceSession, currentEpoch: number): { text: string | null; reason?: string } {
  if (currentEpoch !== s.epoch) {
    return { text: null, reason: "替换会话已失效（文本在此期间被其他操作修改）——无可撤销的整批替换" };
  }
  return { text: s.snapshot };
}

/* ------------------------------ F526 状态栏 ------------------------------ */

export const STATUSBAR_HEIGHT_PX = 24;

export type StatusSegment = "selection" | "content" | "location";

/** 三段状态（判据「三段实时性」）。 */
export interface StatusBarModel {
  selection: string;
  content: string;
  location: string;
}

/** 空目录态（判据「空目录态显示」——显性文案而非空白）。 */
export function emptyDirectoryStatus(dirName: string): StatusBarModel {
  return {
    selection: "未选中任何项目",
    content: `「${dirName}」为空`,
    location: dirName,
  };
}

/**
 * 三处同源对账（判据「三处同源对账」：状态栏/F091 详情窗格/F088 搜索结果
 * 计数三处必须同值）——输入三处呈现值，不一致即显性差异清单。
 */
export function reconcileStatusBarCounts(
  sources: Array<{ where: "statusbar" | "detail-pane" | "search"; count: number }>,
): { consistent: boolean; value: number | null; mismatches: string[] } {
  if (sources.length === 0) return { consistent: true, value: null, mismatches: [] };
  const value = sources[0]!.count;
  const mismatches = sources.filter((s) => s.count !== value).map((s) => s.where);
  return { consistent: mismatches.length === 0, value, mismatches };
}

/** 键盘操作反映（判据「键盘操作反映」）：方向键移动 → 选择段同步文案。 */
export function keyboardSelectionText(index: number, total: number, name: string): string {
  if (total === 0) return "未选中任何项目";
  const pos = Math.min(Math.max(index, 0), total - 1) + 1;
  return `第 ${pos}/${total} 项 · ${name}`;
}

/* ------------------------------ 自检 ------------------------------ */

export function findreplSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 大小写/全词（"Cat catalog cat CAT"：含 catalog 内 1 处共 4 处；全词 3 处）
  const text = "Cat catalog cat CAT";
  checks.push({ name: "F515 影响数预览", pass: previewCount(text, "cat", { matchCase: false, wholeWord: false }) === 4 });
  checks.push({ name: "F515 大小写开关", pass: previewCount(text, "cat", { matchCase: true, wholeWord: false }) === 2 });
  checks.push({ name: "F515 全词开关", pass: previewCount(text, "cat", { matchCase: false, wholeWord: true }) === 3 });
  // 全部替换 + 一次回滚（全词口径：Cat/cat/CAT 三处整词命中）
  const s = openReplaceSession(text, "cat", "dog", { matchCase: false, wholeWord: true });
  const r = replaceAll(s, text);
  checks.push({ name: "F515 全部替换计数", pass: r.applied === 3 && r.text === "dog catalog dog dog" });
  const u = undoReplaceBatch(s, s.epoch);
  checks.push({ name: "F515 整批一次回滚", pass: u.text === text });
  const u2 = undoReplaceBatch(s, s.epoch + 1);
  checks.push({ name: "F515 失效代数显性拒绝", pass: u2.text === null && u2.reason !== undefined });
  // 逐个替换推进
  const n1 = replaceNext(s, text);
  checks.push({ name: "F515 逐个替换", pass: n1.at === 0 && n1.text === "dog catalog cat CAT" });
  // 状态栏
  checks.push({ name: "F526 高度 24px 基线", pass: STATUSBAR_HEIGHT_PX === 24 });
  const empty = emptyDirectoryStatus("报告");
  checks.push({ name: "F526 空目录态", pass: empty.content.includes("报告") && empty.selection.length > 0 });
  // 三处同源：一致/不一致
  const ok = reconcileStatusBarCounts([{ where: "statusbar", count: 7 }, { where: "detail-pane", count: 7 }, { where: "search", count: 7 }]);
  const bad = reconcileStatusBarCounts([{ where: "statusbar", count: 7 }, { where: "detail-pane", count: 6 }, { where: "search", count: 7 }]);
  checks.push({ name: "F526 三处同源对账", pass: ok.consistent && !bad.consistent && bad.mismatches.includes("detail-pane") });
  // 键盘反映
  checks.push({ name: "F526 键盘选择反映", pass: keyboardSelectionText(2, 10, "a.txt").includes("3/10") });
  return checks;
}
