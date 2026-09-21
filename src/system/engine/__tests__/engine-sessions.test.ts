import { beforeEach, describe, expect, it } from "vitest";
import {
  applyEngineMsg,
  cancelEngineApp,
  engineStore,
  requestEngineApp,
  resetEngineSession,
  setEngineNotify,
  setEngineWake,
  setEngineWindowOps,
} from "../engineSessions";
import { EMPTY_ENGINE_SESSION } from "../engineModel";

/**
 * S3.7 拉起协议闭环（三体 AI-2）：占位卡 + 唤醒 + 就绪自动开/撤卡。
 * 全部经可注入缝（wake / windowOps / notify），零 Tauri/VWM 依赖。
 * 副作用均为同步（与 engineSessions 的同步接线一致）。
 */

function freshHarness() {
  let opens = 0;
  let focuses = 0;
  let wakes = 0;
  let wakeErr: unknown = null;
  const notes: Array<{ level: string; message: string }> = [];
  setEngineWindowOps({
    open: () => {
      opens += 1;
    },
    focusExisting: () => {
      focuses += 1;
      return opens > 0; // 有占位窗（已 open）→ 聚焦
    },
  });
  setEngineWake(() => {
    wakes += 1;
    if (wakeErr !== null) return Promise.reject(wakeErr);
    return Promise.resolve({});
  });
  setEngineNotify((level, message) => notes.push({ level, message }));
  return {
    get opens() {
      return opens;
    },
    get focuses() {
      return focuses;
    },
    get wakes() {
      return wakes;
    },
    failNextWake(err: unknown) {
      wakeErr = err;
    },
    notes,
  };
}

describe("S3.7 引擎拉起协议闭环（engineSessions）", () => {
  beforeEach(() => {
    resetEngineSession();
    engineStore.setState({ session: { ...EMPTY_ENGINE_SESSION } });
  });

  it("closed 态点击：占位卡立即出现 + 登记请求 + 唤醒后端", () => {
    freshHarness();
    requestEngineApp("app.foobar");
    expect(engineStore.getState().session.openedWindows).toBe(1);
    expect(engineStore.getState().session.pending).toHaveLength(1);
    expect(engineStore.getState().session.pending[0]?.appKey).toBe("app.foobar");
    expect(engineStore.getState().session.lifecycle).toBe("closed");
  });

  it("ready 态点击：直接开流窗，不唤醒不登记", () => {
    freshHarness();
    applyEngineMsg({ seq: 1, kind: "ready" });
    requestEngineApp("app.foobar");
    expect(engineStore.getState().session.openedWindows).toBe(1);
    expect(engineStore.getState().session.pending).toHaveLength(0);
  });

  it("就绪事件：占位窗已存在 → 聚焦自然过渡（撤卡），不双开", () => {
    const h = freshHarness();
    requestEngineApp("app.foobar"); // 占位窗 opens=1
    expect(h.opens).toBe(1);
    applyEngineMsg({ seq: 2, kind: "ready" });
    expect(h.focuses).toBe(1);
    expect(h.opens).toBe(1);
    expect(engineStore.getState().session.pending).toHaveLength(0);
  });

  it("就绪事件但无占位窗（外部驱动）：新开流窗并计数", () => {
    const h = freshHarness();
    // 不点击，直接外部就绪（如另一入口唤醒）。
    applyEngineMsg({ seq: 1, kind: "boot-stage", stage: "vhdx-mount" });
    applyEngineMsg({ seq: 2, kind: "ready" });
    // pending 为空 → 无 open-app effect；本用例验证点击路径才开窗。
    expect(h.opens).toBe(0);
    expect(h.focuses).toBe(0);
    // 现在点击：ready 态直开。
    requestEngineApp("app.foobar");
    expect(h.opens).toBe(1);
  });

  it("取消路径：等待中取消 → 就绪时不自动开窗（撤卡）", () => {
    const h = freshHarness();
    requestEngineApp("app.foobar");
    cancelEngineApp("app.foobar");
    expect(engineStore.getState().session.pending[0]?.cancelled).toBe(true);
    applyEngineMsg({ seq: 2, kind: "ready" });
    expect(h.focuses).toBe(0);
    expect(h.opens).toBe(1);
    expect(engineStore.getState().session.pending).toHaveLength(0);
  });

  it("崩溃路径：撤卡 + 如实通知，窗口保持诚实状态卡由用户关闭", () => {
    const h = freshHarness();
    requestEngineApp("app.foobar");
    applyEngineMsg({ seq: 2, kind: "crashed", reason: "虚拟机异常退出" });
    expect(engineStore.getState().session.lifecycle).toBe("crashed");
    expect(h.notes.some((n) => n.level === "warn" && n.message.includes("崩溃"))).toBe(true);
  });

  it("唤醒失败：如实错误提示（三要素文案来自后端）", async () => {
    const h = freshHarness();
    h.failNextWake({ code: "ENGINE_WAKE", message: "引擎差分链未就绪：未找到 WIN_ENGINE 卷。" });
    requestEngineApp("app.foobar");
    await Promise.resolve();
    await Promise.resolve();
    expect(
      h.notes.some((n) => n.level === "error" && n.message.includes("WIN_ENGINE")),
    ).toBe(true);
  });

  it("防双击双窗：同一软件等待中重复点击不重复登记/开窗", () => {
    const h = freshHarness();
    requestEngineApp("app.foobar");
    requestEngineApp("app.foobar");
    requestEngineApp("app.foobar");
    expect(h.opens).toBe(1);
    expect(engineStore.getState().session.pending).toHaveLength(1);
  });

  it("crashed 后重新点击：重新登记并唤醒（后端复位重拉）", () => {
    const h = freshHarness();
    requestEngineApp("app.foobar");
    applyEngineMsg({ seq: 2, kind: "crashed", reason: "x" });
    requestEngineApp("app.foobar");
    expect(h.wakes).toBe(2);
    expect(engineStore.getState().session.pending).toHaveLength(1);
  });

  it("乱序/重复事件：seq 水位幂等（旧事件不回卷状态）", () => {
    freshHarness();
    applyEngineMsg({ seq: 5, kind: "boot-stage", stage: "vm-create" });
    applyEngineMsg({ seq: 3, kind: "boot-stage", stage: "vhdx-mount" }); // 旧事件
    expect(engineStore.getState().session.stage).toBe("vm-create");
    expect(engineStore.getState().session.lastSeq).toBe(5);
  });
});
