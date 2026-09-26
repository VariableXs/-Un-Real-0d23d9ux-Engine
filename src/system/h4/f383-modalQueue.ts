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
