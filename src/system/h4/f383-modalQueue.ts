/**
 * F383 弹窗排队不叠罗汉（H 域 · AI-H4）：
 * 同一时刻多个弹层（对话框/通知横幅/OSD）按队列错开呈现：对话框居中只允许一个模态、
 * 后续弹窗排队（前一个关闭 200ms 后下一个淡入——F124 节奏），通知横幅纵向堆叠最多 3 条
 * （超出进通知中心）；禁止两个对话框叠在一起各抢焦点。
 * 判据（主册 F383）：模态唯一性审计（并发模态=0）；排队 200ms 间隔；横幅 3 条上限与溢流；
 * 焦点唯一；队列公平（先到先出）。
 * 依赖锚点：F124 动画曲线总谱。
 */

/** 模态关闭到下一个淡入的间隔（判据 200ms）。 */
export const DEQUEUE_GAP_MS = 200;
/** 横幅堆叠上限（判据 3 条）。 */
export const BANNER_CAP = 3;

export type LayerKind = "modal" | "banner" | "osd";

export interface LayerRequest {
  id: string;
  kind: LayerKind;
  /** 入队时刻（ms）。 */
  at: number;
}

export interface ActiveLayer {
  id: string;
  kind: LayerKind;
  /** 持有焦点（判据「焦点唯一」——同一时刻全系统仅一层）。 */
  hasFocus: boolean;
}

export interface LayerEngineState {
  active: ActiveLayer | null;
  /** 模态 FIFO 队列（判据「队列公平」）。 */
  modalQueue: string[];
  /** 横幅堆叠（最多 3）。 */
  banners: string[];
  /** 上一个模态关闭时刻（200ms 间隔判据）。 */
  lastModalClosedAt: number | null;
  /** 溢流进通知中心的横幅（判据「溢流」）。 */
  overflowedToCenter: string[];
}

export function initialEngine(): LayerEngineState {
  return { active: null, modalQueue: [], banners: [], lastModalClosedAt: null, overflowedToCenter: [] };
}

/**
 * 弹层请求入队：模态唯一性——无活动模态且间隔已过即呈现，否则排队；
 * 横幅进堆叠（超限溢流）；OSD 短暂非模态呈现（不抢焦点）。
 */
export function enqueue(state: LayerEngineState, req: LayerRequest): { state: LayerEngineState; presented: boolean } {
  switch (req.kind) {
    case "modal": {
      const canPresent = state.active === null && (state.lastModalClosedAt === null || req.at - state.lastModalClosedAt >= DEQUEUE_GAP_MS);
      if (canPresent) {
        return { state: { ...state, active: { id: req.id, kind: "modal", hasFocus: true } }, presented: true };
      }
      return { state: { ...state, modalQueue: [...state.modalQueue, req.id] }, presented: false };
    }
    case "banner": {
      if (state.banners.length >= BANNER_CAP) {
        return { state: { ...state, overflowedToCenter: [...state.overflowedToCenter, req.id] }, presented: false };
      }
      return { state: { ...state, banners: [...state.banners, req.id] }, presented: true };
    }
    case "osd":
      return { state, presented: true }; // OSD 由浮层系统自管；此处仅放行
  }
}

/** 模态关闭：清活动层、记时刻、间隔后队首递补（FIFO 判据）。 */
export function closeModal(state: LayerEngineState, now: number): { state: LayerEngineState; promoted: string | null } {
  const closed = state.active?.kind === "modal" ? state.active.id : null;
  const next: LayerEngineState = { ...state, active: null, lastModalClosedAt: now };
  if (!closed) return { state, promoted: null };
  return { state: next, promoted: null }; // 递补由 afterGap 显式触发（200ms 判据的结构保证）
}

/** 关闭后 200ms 到点：队首模态淡入（判据「前一个关闭 200ms 后下一个淡入」）。 */
export function afterGap(state: LayerEngineState, now: number): { state: LayerEngineState; promoted: string | null } {
  if (state.lastModalClosedAt === null || now - state.lastModalClosedAt < DEQUEUE_GAP_MS || state.active !== null || state.modalQueue.length === 0) {
    return { state, promoted: null };
  }
  const [head, ...rest] = state.modalQueue;
  return { state: { ...state, active: { id: head!, kind: "modal", hasFocus: true }, modalQueue: rest }, promoted: head! };
}

/** 横幅 dismissal：堆叠收缩（新到横幅可回填）。 */
export function dismissBanner(state: LayerEngineState, id: string): LayerEngineState {
  return { ...state, banners: state.banners.filter((b) => b !== id) };
}

/** 焦点唯一审计（判据）：全系统持焦层 ≤1。 */
export function auditFocusUniqueness(layers: ActiveLayer[]): boolean {
  return layers.filter((l) => l.hasFocus).length <= 1;
}

/** 模态唯一性审计（判据「并发模态=0」）：活动层中模态数 ≤1。 */
export function auditModalUniqueness(layers: ActiveLayer[]): boolean {
  return layers.filter((l) => l.kind === "modal").length <= 1;
}

/** 队列公平审计：递补顺序 == 入队顺序（FIFO）。 */
export function auditFifo(enqueued: string[], promoted: string[]): boolean {
  return enqueued.join("|") === promoted.join("|");
}

/* ================= v4 深化批次四：焦点归还账 / 溢流摘要 / 弹层生命周期日志 ================= */

/** 焦点归还账（判据「焦点唯一」的关闭侧）：每层记录唤起者，关闭时焦点回家（F206 联动）。 */
export class FocusReturnLedger {
  private readonly invokerByLayer = new Map<string, string>();

  register(layerId: string, invokerElementId: string): void {
    this.invokerByLayer.set(layerId, invokerElementId);
  }

  /** 层关闭 → 焦点归还唤起者；未登记的层如实返回 null（不编造归还目标）。 */
  returnTargetOf(layerId: string): string | null {
    return this.invokerByLayer.get(layerId) ?? null;
  }

  forget(layerId: string): void {
    this.invokerByLayer.delete(layerId);
  }

  get size(): number {
    return this.invokerByLayer.size;
  }
}

/** 递补时机审计（判据「排队 200ms 间隔」的机检面）：早于间隔 = 违规。 */
export function requeueTimingOk(prevClosedAt: number, presentedAt: number): boolean {
  return presentedAt - prevClosedAt >= DEQUEUE_GAP_MS;
}

/** 溢流摘要（判据「溢流进通知中心」的人话面）：N 条溢流并成一条摘要行。 */
export function overflowDigest(overflowed: string[]): { count: number; text: string } | null {
  if (overflowed.length === 0) return null;
  return { count: overflowed.length, text: `${overflowed.length} 条通知未展开——已收入通知中心` };
}

/** 弹层生命周期行（体验日志十三章：每次出现/消失都留时间轴，谁关的、怎么关的可回放）。 */
export interface LayerLifecycleRow {
  layerId: string;
  kind: LayerKind;
  openedAt: number;
  closedAt: number | null;
  closedBy: "user" | "system" | "timeout" | null;
}

export class LayerLifecycleLog {
  /** 同层可多次开闭——按实例存数组，不以 id 键控（v4 修复：Map 键控会让多次开闭互相覆盖）。 */
  private readonly rows: LayerLifecycleRow[] = [];

  open(layerId: string, kind: LayerKind, at: number): void {
    this.rows.push({ layerId, kind, openedAt: at, closedAt: null, closedBy: null });
  }

  close(layerId: string, at: number, by: NonNullable<LayerLifecycleRow["closedBy"]>): void {
    const r = [...this.rows].reverse().find((x) => x.layerId === layerId && x.closedAt === null);
    if (r) {
      r.closedAt = at;
      r.closedBy = by;
    }
  }

  /** 挫败信号（十三章）：同层 30s 窗口内反复开闭 ≥3 次 = 指纹事件。 */
  frustrationSignals(windowMs = 30_000): Array<{ layerId: string; cycles: number }> {
    const all = [...this.rows].sort((a, b) => a.openedAt - b.openedAt);
    const counts = new Map<string, number>();
    for (const r of all) {
      const prevClosed = all.filter((x) => x.layerId === r.layerId && x.closedAt !== null && x.closedAt < r.openedAt).at(-1);
      if (prevClosed && r.openedAt - prevClosed.openedAt <= windowMs) {
        counts.set(r.layerId, (counts.get(r.layerId) ?? 0) + 1);
      }
    }
    return [...counts.entries()].filter(([, n]) => n >= 2).map(([layerId, n]) => ({ layerId, cycles: n + 1 }));
  }

  all(): LayerLifecycleRow[] {
    return [...this.rows].sort((a, b) => a.openedAt - b.openedAt);
  }
}
