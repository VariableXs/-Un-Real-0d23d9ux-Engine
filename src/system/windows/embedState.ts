import { createStore, useStore } from "../../lib/store";

/**
 * 批次W-3：嵌入会话状态机（前端镜像）：
 * - running：原生窗口正常呈现（占位透明层）
 * - exited：进程树已退出（Rust 监护线程 embed://state 上报）→ 占位卡
 * - orphaned：窗口消失但进程仍在（应用回到自身窗口，如启动器回壳）→ 占位卡
 * - failed：批次C-2 捕获失败（未找到可嵌窗口；应用独立运行）→ 占位卡 + 框选兜底
 * 状态只展示一次（重新打开走新会话，新 embed_id）。
 */

export type EmbedSessionState = "running" | "exited" | "orphaned" | "failed";

/** 批次C-2：失败/收编上下文（tpId + 根 pid，框选窗口后 embed_adopt 用）。 */
export type EmbedMeta = { tpId: string; rootPid: number };

export const embedStateStore = createStore<{
  states: Record<string, EmbedSessionState>;
  /** 批次C-1：重嵌重同步计数（EmbedBridge 依赖此值重发 embed_bounds）。 */
  resync: Record<string, number>;
  /** 批次C-2：embedId → 会话上下文。 */
  meta: Record<string, EmbedMeta>;
}>({ states: {}, resync: {}, meta: {} });

export function setEmbedSessionState(embedId: string, state: EmbedSessionState): void {
  embedStateStore.setState((s) => ({ states: { ...s.states, [embedId]: state } }));
}

/** 批次C-2：登记会话上下文（启动时写入；框选收编后随状态清除）。 */
export function setEmbedMeta(embedId: string, meta: EmbedMeta): void {
  embedStateStore.setState((s) => ({ meta: { ...s.meta, [embedId]: meta } }));
}

export function clearEmbedSessionState(embedId: string): void {
  embedStateStore.setState((s) => {
    if (!(embedId in s.states)) return s;
    const states = { ...s.states };
    delete states[embedId];
    return { states };
  });
}

/** 批次C-2：会话收编/复归成功 → 状态与上下文一并清除。 */
export function clearEmbedSessionAll(embedId: string): void {
  embedStateStore.setState((s) => {
    const states = { ...s.states };
    const meta = { ...s.meta };
    delete states[embedId];
    delete meta[embedId];
    return { states, meta };
  });
}

/** 批次C-1：重嵌成功后触发边界重同步（EmbedBridge 重发 embed_bounds + embed_visible）。 */
export function bumpEmbedResync(embedId: string): void {
  embedStateStore.setState((s) => ({
    resync: { ...s.resync, [embedId]: (s.resync[embedId] ?? 0) + 1 },
  }));
}

export function useEmbedSessionState(embedId: string): EmbedSessionState {
  return useStore(embedStateStore, (s) => s.states[embedId] ?? "running");
}
