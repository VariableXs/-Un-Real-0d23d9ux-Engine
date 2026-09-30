import { beforeEach, describe, expect, it } from "vitest";
import {
  bumpEmbedResync,
  clearEmbedSessionAll,
  clearEmbedSessionState,
  embedStateStore,
  setEmbedMeta,
  setEmbedSessionState,
} from "../embedState";

/**
 * M5：占位卡三态状态机（embedStateStore 前端镜像）回归——
 * VirtualWindowManager 的 embed://state 处理器按 payload.state 分派：
 * running → clearEmbedSessionState + bumpEmbedResync；
 * exited → closeVwmWinSafe（R7：不占位，直接关窗）；
 * orphaned → setEmbedSessionState（占位卡）。本文件锁定 store 语义。
 */

beforeEach(() => {
  embedStateStore.setState({ states: {}, resync: {}, meta: {} });
});

describe("embedState 占位卡状态机（M5）", () => {
  it("orphaned：状态写入并对默认渲染态（running）可见", () => {
    setEmbedSessionState("w1", "orphaned");
    expect(embedStateStore.getState().states.w1).toBe("orphaned");
  });

  it("running 复归：清状态 + resync 自增（EmbedBridge 据此重发 embed_bounds）", () => {
    setEmbedSessionState("w1", "orphaned");
    clearEmbedSessionState("w1");
    bumpEmbedResync("w1");
    const s = embedStateStore.getState();
    expect(s.states.w1).toBeUndefined();
    expect(s.resync.w1).toBe(1);
    bumpEmbedResync("w1");
    expect(embedStateStore.getState().resync.w1).toBe(2);
  });

  it("clear 对不存在的会话是无操作（不扰动其它会话）", () => {
    setEmbedSessionState("w1", "orphaned");
    setEmbedSessionState("w2", "orphaned");
    clearEmbedSessionState("w9");
    expect(embedStateStore.getState().states).toEqual({ w1: "orphaned", w2: "orphaned" });
  });

  it("clearEmbedSessionAll：状态与收编上下文一并清除（重新打开走新会话）", () => {
    setEmbedSessionState("w1", "exited");
    setEmbedMeta("w1", { tpId: "notepad", rootPid: 4242 });
    clearEmbedSessionAll("w1");
    const s = embedStateStore.getState();
    expect(s.states.w1).toBeUndefined();
    expect(s.meta.w1).toBeUndefined();
  });
});
