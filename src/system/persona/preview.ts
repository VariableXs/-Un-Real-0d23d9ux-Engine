/**
 * F152 实时预览编辑器 · 纯逻辑层。
 *
 * 主册判据：改-预览延迟 ≤100ms；应用-全局生效无重启；放弃-零残留（令牌表哈希还原验证）。
 *
 * 设计要点（主册【状态与异常】）：
 * - 编辑态快照内存：进入编辑器即自动快照，退出未应用自动还原（双保险）；
 * - 改后崩溃 → 未应用更改自然丢弃（确认制保证系统态干净）；
 * - 并发改（多设置窗）→ 后写覆盖+提示（generation 计数检测）；
 * - 预览自身异常 → 提示+主界面不受染（预览上下文独立，apply 与主表隔离）。
 */

import { defaultTokenTable, tokenTableHash, type TokenIssue, type TokenTable, validateTokenTable } from "./tokens";

export const PREVIEW_LATENCY_BUDGET_MS = 100;
export const APPLY_ANIMATION_MS = 300; // F124 强调曲线交叉淡入

export type PreviewScene = "desktop" | "explorer" | "settings";

export interface PreviewEdit {
  /** 本轮编辑起点（进入编辑器时刻的令牌表）。 */
  baseline: TokenTable;
  /** 当前编辑态。 */
  working: TokenTable;
  /** 基线哈希——放弃路径以此为准做哈希还原验证。 */
  baselineHash: string;
  /** 后写覆盖检测：每次 start 落 generation，apply/finish 校验。 */
  generation: number;
  /** 最近一次改→预览耗时实测（ms）。 */
  lastLatencyMs: number;
}

export class PreviewSession {
  private edit: PreviewEdit | null = null;

  /** 进入编辑器：自动快照（主册设计细节——双保险之一）。 */
  start(table: TokenTable): PreviewEdit {
    this.edit = {
      baseline: JSON.parse(JSON.stringify(table)) as TokenTable,
      working: JSON.parse(JSON.stringify(table)) as TokenTable,
      baselineHash: tokenTableHash(table),
      generation: ++PreviewSession.globalGeneration,
      lastLatencyMs: 0,
    };
    return this.edit;
  }

  private static globalGeneration = 0;

  get active(): boolean {
    return this.edit !== null;
  }

  get session(): PreviewEdit | null {
    return this.edit;
  }

  /** 是否有未应用更改（顶栏黄条判据）。 */
  get dirty(): boolean {
    if (!this.edit) return false;
    return tokenTableHash(this.edit.working) !== this.edit.baselineHash;
  }

  /**
   * 改一个令牌值 → 更新 working 并返回预览用表。
   * 延迟实测：本函数耗时即「改→预览」的纯计算段；渲染段由调用方用
   * measureLatency 补记（预算合计 ≤100ms）。
   */
  patch(mutator: (t: TokenTable) => void): { table: TokenTable; issues: TokenIssue[] } {
    if (!this.edit) throw new Error("预览会话未开启");
    const t0 = nowMs();
    const draft = JSON.parse(JSON.stringify(this.edit.working)) as TokenTable;
    mutator(draft);
    const r = validateTokenTable(draft);
    if (r.ok && r.data) {
      this.edit.working = r.data;
      this.edit.generation = ++PreviewSession.globalGeneration;
    }
    this.edit.lastLatencyMs = nowMs() - t0;
    return { table: this.edit.working, issues: r.issues };
  }

  /** 渲染段耗时补记（合计账）。 */
  recordRenderLatency(ms: number): void {
    if (this.edit) this.edit.lastLatencyMs += Math.max(0, ms);
  }

  /** 改→预览延迟是否在预算内（≤100ms 判据）。 */
  get withinLatencyBudget(): boolean {
    return (this.edit?.lastLatencyMs ?? Infinity) <= PREVIEW_LATENCY_BUDGET_MS;
  }

  /**
   * 应用：working 成为新正身。返回 [appliedTable, 后写覆盖警告?].
   * 后写覆盖：start 之后如果有另一个会话已经 apply 过（generation 检测由调用方
   * 通过 hasConcurrentWrite 查询），这里如实报告——后写覆盖+提示，不撒谎。
   */
  apply(): { applied: TokenTable; concurrencyWarning: boolean } {
    if (!this.edit) throw new Error("预览会话未开启");
    const applied = JSON.parse(JSON.stringify(this.edit.working)) as TokenTable;
    const concurrencyWarning = PreviewSession.lastAppliedGeneration > this.edit.generation && this.edit.generation > 0;
    PreviewSession.lastAppliedGeneration = ++PreviewSession.globalGeneration;
    this.edit = null;
    return { applied, concurrencyWarning };
  }

  private static lastAppliedGeneration = 0;

  /**
   * 放弃：回到基线。返回 [restoredTable, hashVerified]——
   * hashVerified=true 表示还原结果与进入编辑器时逐位一致（零残留判据）。
   */
  discard(): { restored: TokenTable; hashVerified: boolean } {
    if (!this.edit) throw new Error("预览会话未开启");
    const restored = JSON.parse(JSON.stringify(this.edit.baseline)) as TokenTable;
    const hashVerified = tokenTableHash(restored) === this.edit.baselineHash;
    this.edit = null;
    return { restored, hashVerified };
  }
}

function nowMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

/** 预览缩放（主册设计细节：0.5 倍实时，渲染成本减 75%）。 */
export const PREVIEW_SCALE = 0.5;
export const PREVIEW_WIDTH_PX = 400; // 固定宽

/** 未应用黄条文案键（labels 消费）。 */
export const DIRTY_BANNER_KEY = "unappliedChanges";

/** 崩溃恢复语义：会话不落盘——重启后天然干净（确认制），此函数给恢复检查用。 */
export function crashRecoveryIsClean(storedTable: unknown): boolean {
  // 落盘的永远是已应用的正身；若校验失败说明正身损坏——按默认表兜底并要求报备。
  const r = validateTokenTable(storedTable ?? defaultTokenTable());
  return r.ok;
}
