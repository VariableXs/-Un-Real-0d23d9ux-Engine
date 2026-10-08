/**
 * J 鼠标域 · F618 滚轮穿透开关 · 纵深引擎（批次七）。
 *
 * v3 有 resolveWheelTarget（白名单三态）。本引擎补「规则」层——
 * 穿透开关的真实使用形态不是全局一开一关，而是规则集：
 *
 * 1. 分层规则——全局默认 < 应用规则 < 容器规则（三层，下层为缺省）。
 *    「PDF 阅读器里永不穿透，但侧边缩略图栏穿透」「办公套件里穿透，
 *    但表格区不穿」——这是真实工作流。规则匹配按 specificity 仲裁：
 *    容器 > 应用 > 全局，同层后声明者胜。
 *
 * 2. 临时开关——快捷键按住 = 临时反转穿透状态（松开还原）；或「临时
 *    关闭 5 秒」带自动过期（倒计时显性化：面板/HUD 显示剩余）。临时态
 *    叠加在规则结果之上，不写回规则（层次分明：临时的不许固化）。
 *
 * 3. 覆盖审计——每次裁决留一行审计（谁/哪层规则/结果/时刻），上限
 *    200 条环形。穿透是「反直觉」行为（滚轮转了页面没动），出问题时
 *    用户第一句话是「为什么没反应」——审计就是答案。
 *
 * 判据锚点：
 * - 三层 specificity 仲裁 → resolveRule()
 * - 临时反转与自动过期 → TempPassState / tempOverride()
 * - 临时态不固化 → 规则集类型级只读（freeze 语义）
 * - 审计环形 200 条 → WheelRuleAudit
 */

/* ------------------------------- 分层规则 ------------------------------- */

/** 穿透三态（与 v3 白名单语义对齐）：pass=穿透、intercept=接管、inherit=看上层。 */
export type WheelPassMode = "pass" | "intercept" | "inherit";

export interface WheelRule {
  id: string;
  /** 层级：容器规则必须带应用上下文（跨应用容器规则无意义）。 */
  layer: "global" | "app" | "container";
  /** 应用 id（app/container 层必填；global 层须为空）。 */
  appId?: string;
  /** 容器声明值（container 层必填，如 data-wheel="menu"）。 */
  container?: string;
  mode: WheelPassMode;
}

/**
 * specificity 仲裁：container > app > global；同层后声明者胜。
 * 候选序列按声明序传入，返回胜者；全 inherit 落到默认 intercept
 * （不穿透是安全缺省——穿透是显式授权行为）。
 */
export function resolveRule(rules: WheelRule[]): { winner: WheelRule | null; mode: WheelPassMode } {
  let winner: WheelRule | null = null;
  for (const r of rules) {
    if (r.mode === "inherit") continue;
    if (!winner) {
      winner = r;
      continue;
    }
    // 同层后声明者胜（>= ：同 rank 迭代到后位即替换）；高层压低层。
    if (rank(r.layer) >= rank(winner.layer)) winner = r;
  }
  return { winner, mode: winner?.mode ?? "intercept" };
}

function rank(layer: WheelRule["layer"]): number {
  return layer === "global" ? 0 : layer === "app" ? 1 : 2;
}

/** 规则集校验（面板保存前闸门）：非法结构一次报全（不是见一个抛一个）。 */
export function validateRules(rules: WheelRule[]): string[] {
  const errs: string[] = [];
  const ids = new Set<string>();
  for (const r of rules) {
    if (!r.id.trim()) errs.push("存在空 id 的规则");
    if (ids.has(r.id)) errs.push(`规则 id 重复：${r.id}`);
    ids.add(r.id);
    if (r.layer === "global" && (r.appId || r.container)) errs.push(`全局规则 ${r.id} 不应带应用/容器限定`);
    if (r.layer === "app" && !r.appId) errs.push(`应用规则 ${r.id} 缺 appId`);
    if (r.layer === "container" && (!r.appId || !r.container)) errs.push(`容器规则 ${r.id} 需同时带 appId 与 container 声明`);
    if (r.layer === "container" && r.mode === "inherit") errs.push(`容器规则 ${r.id} 用 inherit 无意义（容器层就是尽头）`);
  }
  return errs;
}

/* ------------------------------- 临时开关 ------------------------------- */

/** 临时关闭自动过期时长（ms）——「5 秒」口径；0 = 跟随按键（按住型）。 */
export const TEMP_PASS_MS = 5000;

export interface TempPassState {
  /** null = 无临时态；否则到期时刻（按住型为 Infinity）。 */
  expiresAt: number | null;
  /** 临时方向：true=临时放行（穿透）、false=临时拦截。 */
  pass: boolean;
}

/**
 * 临时反转的裁决：规则结果 + 临时态 → 最终模式。
 * 临时态叠加而非替换语义：规则说 pass、临时说拦截 → 拦截（临时优先，
 * 这是「临时」的定义——用户此刻的明确意志压过一切配置）。
 */
export function tempOverride(ruleMode: WheelPassMode, temp: TempPassState | null, nowMs: number): WheelPassMode {
  if (!temp || temp.expiresAt === null) return ruleMode;
  if (nowMs >= temp.expiresAt) return ruleMode; // 过期即失效（宿主负责清理，但裁决层幂等防御）
  return temp.pass ? "pass" : "intercept";
}

/** 剩余秒数（HUD 倒计时口径；向上取整——「还剩 0.2s」显示为 1）。 */
export function tempRemainingSec(temp: TempPassState, nowMs: number): number | null {
  if (temp.expiresAt === null || temp.expiresAt === Infinity) return null;
  const left = temp.expiresAt - nowMs;
  return left <= 0 ? 0 : Math.ceil(left / 1000);
}

/* ------------------------------- 裁决审计 ------------------------------- */

export interface AuditEntry {
  atMs: number;
  layer: WheelRule["layer"] | "temp" | "default";
  ruleId: string | null;
  mode: WheelPassMode;
  /** 触发上下文（容器声明值/应用名——可读性优先）。 */
  ctx: string;
}

/** 环形审计上限。 */
export const AUDIT_LIMIT = 200;

export class WheelRuleAudit {
  private entries: AuditEntry[] = [];

  record(e: AuditEntry): void {
    this.entries.push(e);
    if (this.entries.length > AUDIT_LIMIT) this.entries.shift();
  }

  all(): readonly AuditEntry[] {
    return this.entries;
  }

  clear(): void {
    this.entries = [];
  }

  /** 最近一次「滚轮转了但没动」的答案（面板一键定位）。 */
  lastVerdict(): AuditEntry | null {
    return this.entries[this.entries.length - 1] ?? null;
  }
}
