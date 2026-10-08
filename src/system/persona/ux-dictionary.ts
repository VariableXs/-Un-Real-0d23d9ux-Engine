/**
 * 十章一致性 + 十一章可发现性 + 八九章进度/剪贴板 · UX 词典与状态模型。
 *
 * 主册判据延伸：
 * - 十章「交互词典：什么动作用什么控件、什么层级用什么浮层、什么状态配
 *   什么颜色文案，改规则必须全局同步改」——词典是**结构化规则表 + 机检**
 *   （违例在数据层就能扫出来，不靠人记忆）；
 * - 十一章「引导只出现一次、可跳过、可找回」——first-run 状态机；
 * - 八/九章「诚实的进度条」「可取消长任务」——进度诚实模型；
 * - 六章「撤销链粒度合理」——undo 分段策略（不是每字符一步也不是一键回解放前）。
 */

// ---------- 交互词典（十章一致性机检的规则面） ----------

export interface DictionaryRule {
  id: string;
  /** 规则域（关闭/浮层/按钮序/加载/空态…）。 */
  domain: "close-key" | "popover-layer" | "button-order" | "loading" | "empty-state" | "accent-usage";
  rule: string;
  /** 机检数据（违例在此登记——静态可扫）。 */
  violations: string[];
}

/** E 域交互词典（新功能先查这里；改规则必须全局同步——规则即数据）。 */
export const UX_DICTIONARY: DictionaryRule[] = [
  { id: "D-CLOSE-01", domain: "close-key", rule: "浮层关闭键统一 Esc；写了 Esc 关闭就必须真的关闭", violations: [] },
  { id: "D-POPOVER-01", domain: "popover-layer", rule: "右键菜单/下拉/气泡 = 弹出层；对话框 = 模态层；抽屉 = 侧滑层——层级固定不混用", violations: [] },
  { id: "D-CONFIRM-01", domain: "button-order", rule: "破坏性确认：取消固定安全侧、确认钮 danger 形态、动词具体", violations: [] },
  { id: "D-LOADING-01", domain: "loading", rule: "长任务必有可动进度与可信剩余时间；不确定时长用分步呈现不假进度", violations: [] },
  { id: "D-EMPTY-01", domain: "empty-state", rule: "空态 = 标题+引导+入口动作三件套（message-catalog 二十页）", violations: [] },
  { id: "D-ACCENT-01", domain: "accent-usage", rule: "强调色只用于可交互语义（主按钮/焦点环/选中态），装饰性用法走 accent-soft", violations: [] },
];

/** 词典机检（消费方数据驱动——新增违例登记即红）。 */
export function auditDictionary(extraViolations: Array<{ ruleId: string; where: string }> = []): { ok: boolean; findings: Array<{ ruleId: string; where: string; rule: string }> } {
  const findings: Array<{ ruleId: string; where: string; rule: string }> = [];
  for (const r of UX_DICTIONARY) {
    for (const v of r.violations) findings.push({ ruleId: r.id, where: v, rule: r.rule });
  }
  for (const e of extraViolations) {
    const r = UX_DICTIONARY.find((x) => x.id === e.ruleId);
    if (r) findings.push({ ruleId: r.id, where: e.where, rule: r.rule });
  }
  return { ok: findings.length === 0, findings };
}

// ---------- first-run 状态机（十一章：只出现一次/可跳过/可找回） ----------

export interface FirstRunEntry {
  key: string;
  /** 引导文案（一句话——不是教程）。 */
  hint: string;
  seen: boolean;
  skipped: boolean;
}

export class FirstRunLedger {
  private entries = new Map<string, FirstRunEntry>();

  register(key: string, hint: string): void {
    if (!this.entries.has(key)) this.entries.set(key, { key, hint, seen: false, skipped: false });
  }

  /** 是否展示（seen=false 且未跳过——只出现一次）。 */
  shouldShow(key: string): boolean {
    const e = this.entries.get(key);
    return e !== undefined && !e.seen && !e.skipped;
  }

  markSeen(key: string): void {
    const e = this.entries.get(key);
    if (e) e.seen = true;
  }

  /** 跳过（可跳过——且不再自动出现）。 */
  skip(key: string): void {
    const e = this.entries.get(key);
    if (e) { e.seen = true; e.skipped = true; }
  }

  /** 可找回（帮助中心清单——跳过的引导不消失，只是不再打扰）。 */
  findable(): FirstRunEntry[] {
    return [...this.entries.values()].filter((e) => e.skipped);
  }

  /** 新手五分钟核心操作清单（学习曲线分层的默认路径——唯一路径选好）。 */
  corePath(): string[] {
    return ["tokens:改一个颜色", "preview:看差异", "archive:导出档案"];
  }
}

// ---------- 诚实进度模型（八/九章：会动的进度 + 可信剩余 + 可取消） ----------

export interface ProgressSample {
  at: number;
  done: number;
}

export interface HonestProgress {
  /** 归一进度 0..1（未知总量 = null——分步呈现不假进度）。 */
  ratio: number | null;
  /** 剩余时间估计 ms（滑窗速率；样本 <2 = null——不猜）。 */
  remainingMs: number | null;
  /** 诚实文案（不装确定的场合直说）。 */
  message: string;
  /** 速率停滞判定（30s 无新完成 = 卡住显性化）。 */
  stalled: boolean;
}

const STALL_WINDOW_MS = 30_000;

/** 滑窗速率估计（最近 10 个样本——旧速率不污染现状）。 */
export function honestProgress(samples: ProgressSample[], total: number, _now: number): HonestProgress {
  if (total <= 0) throw new Error("总量必须为正");
  const sorted = [...samples].sort((a, b) => a.at - b.at).slice(-10);
  if (sorted.length < 2) {
    return { ratio: null, remainingMs: null, message: "准备中——总量确认后显示进度（不装确定的假条）", stalled: false };
  }
  const done = Math.min(total, sorted[sorted.length - 1]!.done);
  const ratio = Math.round((done / total) * 1000) / 1000;
  // 速率 = 窗口首尾差（含停滞检测：窗口跨度内 done 无增长 = 卡住）。
  const span = sorted[sorted.length - 1]!.at - sorted[0]!.at;
  const progressed = sorted[sorted.length - 1]!.done - sorted[0]!.done;
  const stalled = span >= STALL_WINDOW_MS && progressed === 0;
  if (stalled) {
    return { ratio, remainingMs: null, message: `进度停在 ${Math.round(ratio * 100)}% 超过 30 秒——任务可能卡住，可取消后重试`, stalled: true };
  }
  const rate = progressed / span; // 项/ms。
  const remaining = Math.max(0, Math.round((total - done) / rate));
  return { ratio, remainingMs: remaining, message: `剩余约 ${Math.ceil(remaining / 1000)} 秒（按最近速率估计）`, stalled: false };
}

/** 可取消长任务状态机（running→cancelling→cancelled/cancel-failed 显性化）。 */
export type CancelState = "running" | "cancelling" | "cancelled" | "cancel-failed";

export function cancelTransition(cur: CancelState, request: "cancel" | "cancel-ack" | "cancel-fail"): CancelState {
  switch (request) {
    case "cancel":
      return cur === "running" ? "cancelling" : cur;
    case "cancel-ack":
      return cur === "cancelling" ? "cancelled" : cur;
    case "cancel-fail":
      return cur === "cancelling" ? "cancel-failed" : cur;
  }
}

// ---------- undo 分段策略（六章：粒度合理） ----------

export interface UndoGroup {
  /** 组 id（同类连续操作归并）。 */
  kind: string;
  /** 组内操作数。 */
  ops: number;
  at: number;
}

/** 归并策略：同 kind 且间隔 <2s 归并成一步（打字流/滑杆流 = 一步；跨类操作独立成步）。 */
export function coalesceUndo(groups: UndoGroup[]): UndoGroup[] {
  const MERGE_MS = 2000;
  const out: UndoGroup[] = [];
  for (const g of groups) {
    const last = out[out.length - 1];
    if (last && last.kind === g.kind && g.at - last.at < MERGE_MS) {
      last.ops += g.ops;
      last.at = g.at;
    } else {
      out.push({ ...g });
    }
  }
  return out;
}

/** 撤销栈深度契约：50 步封顶 + 内存恒定（最早步淘汰——十二章确定性）。 */
export const UNDO_STACK_MAX = 50;

export function pushUndo(stack: UndoGroup[], g: UndoGroup): UndoGroup[] {
  const next = [...coalesceUndo([...stack, g])];
  return next.length > UNDO_STACK_MAX ? next.slice(next.length - UNDO_STACK_MAX) : next;
}
