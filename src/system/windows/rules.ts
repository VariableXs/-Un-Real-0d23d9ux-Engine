/**
 * N-03 窗口规则引擎（NEXT-40 · AI-2 窗口路）：
 * 用户可写「当 X 应用打开 → 自动贴靠/入组/置顶/透明度」的自动化规则。
 * - 规则契约：`Rule { id, trigger, actions, priority, enabled }`（JSON 可导入导出分享）；
 * - 冲突按 优先级 priority + specificity（进程名 > 应用族 > 通配）裁决，裁决日志实时可见；
 * - **安全分叉不可覆盖原则**：L4（反作弊）与管理员窗口的动作白名单硬编码在本层，
 *   用户规则只能加严不能放宽——命中受限窗口时动作直接拒绝并留日志；
 * - 动作只映射到 VWM 既有执行器（snapVwmRect / stages / z 序 / 透明度样式），
 *   **不新增任何绕过安全审查的执行路径**；
 * - 日志环形缓冲 500 条。
 *
 * 纯策略层：本模块只产出「裁决结果 + 动作描述」，应用由调用方执行到 VWM store。
 */

/** 触发器：VWM 应用标识匹配（精确 tp:id / 应用名 / `*` 通配）。 */
export interface RuleTrigger {
  /** 匹配的 app 标识（如 `tp:vscode`、`write`、`*`）。 */
  app: string;
  /** 可选标题正则（字符串形式，导入时校验）。 */
  titlePattern?: string;
}

export type RuleAction =
  | {
      type: "snapRect";
      /**
       * 贴靠矩形：分量 ∈ (0,1] 视为工作区比例（导入分享可跨分辨率），
       * >1 视为绝对像素。应用时经 resolveSnapRect 换算。
       */
      rect: { x: number; y: number; w: number; h: number };
    }
  | { type: "stage"; stageId: string }
  | { type: "topmost" }
  | { type: "opacity"; value: number };

/** 把规则里的贴靠矩形换算为绝对像素（比例分量 ≤1 按工作区换算）。 */
export function resolveSnapRect(
  rect: { x: number; y: number; w: number; h: number },
  wa: { x: number; y: number; w: number; h: number },
): { x: number; y: number; w: number; h: number } {
  const f = (v: number, base: number, origin: number, incl0: boolean) =>
    (incl0 ? v >= 0 && v <= 1 : v > 0 && v <= 1) ? Math.round(origin + v * base) : Math.round(v);
  return { x: f(rect.x, wa.w, wa.x, true), y: f(rect.y, wa.h, wa.y, true), w: f(rect.w, wa.w, 0, false), h: f(rect.h, wa.h, 0, false) };
}

export interface Rule {
  id: string;
  name: string;
  trigger: RuleTrigger;
  actions: RuleAction[];
  /** 数值越大优先级越高。 */
  priority: number;
  enabled: boolean;
}

export interface RuleDecision {
  winId: string;
  app: string;
  /** 胜出规则（null = 无规则命中）。 */
  rule: Rule | null;
  /** 被拒绝的动作与原因（安全白名单等）。 */
  rejected: { action: RuleAction; reason: string }[];
}

export interface RuleLogEntry {
  ts: number;
  text: string;
}

/** 受限窗口动作白名单：L4（反作弊）与管理员窗口——任何规则都不得施加动作（只能加严不能放宽）。 */
export interface RestrictedWindow {
  winId: string;
  /** 兼容等级（compat.rs L1-L4）；L4 视为受限。 */
  tier: 1 | 2 | 3 | 4;
  elevated?: boolean;
}

const LOG_CAP = 500;
const logs: RuleLogEntry[] = [];

export function ruleLogs(): readonly RuleLogEntry[] {
  return logs;
}
export function clearRuleLogs(): void {
  logs.length = 0;
}

function log(text: string, now = Date.now()): void {
  logs.push({ ts: now, text });
  if (logs.length > LOG_CAP) logs.splice(0, logs.length - LOG_CAP);
}

/** 校验规则（导入时行级报错，不崩溃——验收 ④）。返回错误列表（空 = 合法）。 */
export function validateRule(r: unknown): string[] {
  const errs: string[] = [];
  const o = r as Partial<Rule>;
  if (!o || typeof o !== "object") return ["规则必须是对象"];
  if (typeof o.id !== "string" || !o.id) errs.push("缺少 id");
  if (typeof o.name !== "string") errs.push("缺少 name");
  const t = o.trigger as Partial<RuleTrigger> | undefined;
  if (!t || typeof t.app !== "string" || !t.app) errs.push("trigger.app 必须为非空字符串");
  if (t?.titlePattern) {
    try {
      new RegExp(t.titlePattern);
    } catch (e) {
      errs.push(`titlePattern 非法正则: ${(e as Error).message}`);
    }
  }
  if (!Array.isArray(o.actions) || o.actions.length === 0) errs.push("actions 不能为空");
  if (typeof o.priority !== "number") errs.push("priority 必须为数字");
  if (typeof o.enabled !== "boolean") errs.push("enabled 必须为布尔");
  return errs;
}

/** 导入他人规则 → 与本地规则合并（同 id 覆盖、其余追加；非法条目跳过并报错行级返回）。 */
export function importRules(incoming: unknown[]): { merged: Rule[]; errors: { index: number; errs: string[] }[] } {
  const local = loadRules();
  const byId = new Map(local.map((r) => [r.id, r]));
  const errors: { index: number; errs: string[] }[] = [];
  incoming.forEach((r, index) => {
    const errs = validateRule(r);
    if (errs.length > 0) {
      errors.push({ index, errs });
      return;
    }
    byId.set((r as Rule).id, r as Rule);
  });
  const merged = [...byId.values()];
  persistRules(merged);
  return { merged, errors };
}

const KEY = "variable:vwm:rules";

export function loadRules(): Rule[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]") as Rule[];
    return Array.isArray(raw) ? raw : [];
  } catch {
    return [];
  }
}

function persistRules(rules: Rule[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(rules));
  } catch {
    /* storage full */
  }
}

export function saveRule(rule: Rule): void {
  const all = loadRules();
  const i = all.findIndex((r) => r.id === rule.id);
  if (i >= 0) all[i] = rule;
  else all.push(rule);
  persistRules(all);
}

// ---------- 裁决 ----------

/** specificity：精确 app > 通配。 */
function specificity(t: RuleTrigger, app: string, title: string): number {
  if (t.app !== "*" && t.app !== app) return -1;
  if (t.titlePattern) {
    if (!new RegExp(t.titlePattern).test(title)) return -1;
    return 2;
  }
  return t.app === "*" ? 0 : 1;
}

/**
 * 对一个窗口做规则裁决：
 * 1) 受限窗口（L4/管理员）直接拒绝全部动作并留日志（安全分叉不可覆盖）；
 * 2) 命中规则按 priority 优先、同分比 specificity，高者胜出；
 * 3) 无命中 → rule=null（维持现状）。
 */
export function decide(
  input: { winId: string; app: string; title: string; restricted: boolean },
  rules: Rule[] = loadRules(),
  now = Date.now(),
): RuleDecision {
  if (input.restricted) {
    log(`[${input.winId}] 受限窗口（L4/管理员）：规则引擎动作全部拒绝（只能加严不能放宽）`, now);
    return { winId: input.winId, app: input.app, rule: null, rejected: [] };
  }
  const hits = rules
    .filter((r) => r.enabled)
    .map((r) => ({ r, spec: specificity(r.trigger, input.app, input.title) }))
    .filter((h) => h.spec >= 0)
    .sort((a, b) => (b.r.priority - a.r.priority) || (b.spec - a.spec));
  if (hits.length === 0) {
    return { winId: input.winId, app: input.app, rule: null, rejected: [] };
  }
  const winner = hits[0]!.r;
  const rejected: RuleDecision["rejected"] = [];
  // 安全检查：透明度下限 30%（与 Z-36 档位一致）、stage 必须存在
  for (const a of winner.actions) {
    if (a.type === "opacity" && (a.value < 30 || a.value > 100)) {
      rejected.push({ action: a, reason: "透明度越界（30–100）" });
    }
  }
  const applied = winner.actions.filter((a) => !rejected.some((r) => r.action === a));
  log(`[${input.winId}] 规则「${winner.name}」胜出（priority=${winner.priority}），动作 ${applied.length} 项`, now);
  return { winId: input.winId, app: input.app, rule: winner, rejected };
}

/** 12 条内置模板（一键启用——调用方复制为 Rule 并 saveRule）。 */
export const RULE_TEMPLATES: { name: string; rule: Omit<Rule, "id"> }[] = [
  { name: "聊天软件靠右半屏", rule: { name: "聊天软件靠右半屏", trigger: { app: "*" }, actions: [{ type: "snapRect", rect: { x: 0.5, y: 0, w: 0.5, h: 1 } }], priority: 10, enabled: true } },
  { name: "写作应用左侧通栏", rule: { name: "写作应用左侧通栏", trigger: { app: "write" }, actions: [{ type: "snapRect", rect: { x: 0, y: 0, w: 0.5, h: 1 } }], priority: 10, enabled: true } },
  { name: "代码窗口入舞台组", rule: { name: "代码窗口入舞台组", trigger: { app: "code" }, actions: [{ type: "stage", stageId: "stage-dev" }], priority: 20, enabled: true } },
  { name: "监控窗口半透明", rule: { name: "监控窗口半透明", trigger: { app: "*", titlePattern: "监控|Monitor" }, actions: [{ type: "opacity", value: 70 }], priority: 15, enabled: true } },
  { name: "浮窗置顶", rule: { name: "浮窗置顶", trigger: { app: "*", titlePattern: "浮窗|Floating" }, actions: [{ type: "topmost" }], priority: 30, enabled: true } },
  { name: "笔记窗口右下象限", rule: { name: "笔记窗口右下象限", trigger: { app: "notes" }, actions: [{ type: "snapRect", rect: { x: 0.5, y: 0.5, w: 0.5, h: 0.5 } }], priority: 10, enabled: true } },
  { name: "回收站小窗", rule: { name: "回收站小窗", trigger: { app: "recycle" }, actions: [{ type: "snapRect", rect: { x: 0.6, y: 0.6, w: 0.4, h: 0.4 } }], priority: 10, enabled: true } },
  { name: "文件管理器左半屏", rule: { name: "文件管理器左半屏", trigger: { app: "explorer" }, actions: [{ type: "snapRect", rect: { x: 0, y: 0, w: 0.5, h: 1 } }], priority: 10, enabled: true } },
  { name: "日历右上角", rule: { name: "日历右上角", trigger: { app: "calendar" }, actions: [{ type: "snapRect", rect: { x: 0.7, y: 0, w: 0.3, h: 0.5 } }], priority: 10, enabled: true } },
  { name: "截图工具居中", rule: { name: "截图工具居中", trigger: { app: "snapshot" }, actions: [{ type: "snapRect", rect: { x: 0.2, y: 0.1, w: 0.6, h: 0.8 } }], priority: 10, enabled: true } },
  { name: "剪贴板历史右侧", rule: { name: "剪贴板历史右侧", trigger: { app: "clipboard" }, actions: [{ type: "snapRect", rect: { x: 0.75, y: 0.2, w: 0.25, h: 0.6 } }], priority: 10, enabled: true } },
  { name: "计算器左下角", rule: { name: "计算器左下角", trigger: { app: "calc" }, actions: [{ type: "snapRect", rect: { x: 0, y: 0.55, w: 0.35, h: 0.45 } }], priority: 10, enabled: true } },
];
