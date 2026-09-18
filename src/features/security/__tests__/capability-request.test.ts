import { beforeEach, describe, expect, it } from "vitest";
import {
  capabilityStore,
  decideCapabilityRequest,
  pushCapabilityRequest,
  recordLatencySample,
} from "../capabilityRequest";
import {
  cancelEngineApp,
  applyEngineMsg,
  engineStore,
  requestEngineApp,
  resetEngineSession,
} from "../../../system/engine/engineSessions";

/** 每用例前复位两个全局 store（vitest 单文件实例内共享）。 */
beforeEach(() => {
  resetEngineSession();
  capabilityStore.setState({ requests: [], latencyLog: [] });
});

describe("任务 61UI · 申请式授权（默认拒绝 + 会话级授权）", () => {
  it("登记申请：未申报理由如实标注，状态 pending", () => {
    const r = pushCapabilityRequest({ subject: "wine:notepad.exe", scope: "net", reason: "  " });
    expect(r.status).toBe("pending");
    expect(r.reason).toBe("未申报理由");
    expect(capabilityStore.getState().requests).toHaveLength(1);
  });

  it("裁决：允许 = 本次会话有效；拒绝立即生效；进程重启即失效（不持久化）", () => {
    const r = pushCapabilityRequest({ subject: "engine:app.x", scope: "host-disk", reason: "读取工作目录" });
    capabilityStore.setState({
      requests: decideCapabilityRequest(capabilityStore.getState().requests, r.id, "granted"),
    });
    expect(capabilityStore.getState().requests[0]).toMatchObject({ status: "granted", grantSession: true });
    const r2 = pushCapabilityRequest({ subject: "wine:game.exe", scope: "net", reason: "联机" });
    capabilityStore.setState({
      requests: decideCapabilityRequest(capabilityStore.getState().requests, r2.id, "denied"),
    });
    expect(capabilityStore.getState().requests[1]).toMatchObject({ status: "denied", grantSession: false });
  });

  it("申请表上限 20 条防膨胀（保留最近）", () => {
    for (let i = 0; i < 25; i += 1) {
      pushCapabilityRequest({ subject: `wine:p${i}.exe`, scope: "net", reason: "测试" });
    }
    const all = capabilityStore.getState().requests;
    expect(all).toHaveLength(20);
    expect(all[all.length - 1]!.subject).toBe("wine:p24.exe");
  });

  it("延迟样本入账：上限 64 条", () => {
    for (let i = 0; i < 70; i += 1) {
      recordLatencySample({ captureMs: 1, encodeMs: 2, transmitMs: 3, decodeMs: 4, composeMs: 5 });
    }
    expect(capabilityStore.getState().latencyLog).toHaveLength(64);
  });
});

describe("任务 50/51 · 引擎会话 store 集成（副作用接线）", () => {
  it("requestEngineApp 未就绪 → 登记请求；applyEngineMsg(ready) → 会话开窗计数递增", () => {
    const before = engineStore.getState().session;
    requestEngineApp("app.foobar", 1000);
    expect(engineStore.getState().session.pending).toHaveLength(1);
    applyEngineMsg({ seq: 1, kind: "ready" });
    const after = engineStore.getState().session;
    expect(after.lifecycle).toBe("ready");
    expect(after.openedWindows).toBe(before.openedWindows + 1);
    expect(after.pending).toEqual([]);
  });

  it("applyEngineMsg 幂等：重复 seq 不产生二次开窗", () => {
    requestEngineApp("app.foobar", 1000);
    applyEngineMsg({ seq: 1, kind: "ready" });
    const once = engineStore.getState().session.openedWindows;
    applyEngineMsg({ seq: 1, kind: "ready" });
    expect(engineStore.getState().session.openedWindows).toBe(once);
  });

  it("cancelEngineApp：取消后 ready 只撤卡不开窗", () => {
    requestEngineApp("app.foobar", 1000);
    cancelEngineApp("app.foobar");
    applyEngineMsg({ seq: 1, kind: "ready" });
    expect(engineStore.getState().session.openedWindows).toBe(0);
  });

  it("异常三场景经 store：crashed 撤卡 + usb-removed 复位 closed", () => {
    requestEngineApp("app.foobar", 1000);
    applyEngineMsg({ seq: 1, kind: "crashed", reason: "VM 崩溃" });
    expect(engineStore.getState().session).toMatchObject({ lifecycle: "crashed", pending: [] });
    applyEngineMsg({ seq: 2, kind: "usb-removed", reason: "强拔" });
    expect(engineStore.getState().session).toMatchObject({ lifecycle: "closed", reason: "强拔" });
    // 下次插入恢复：reset 后可重新拉起
    resetEngineSession();
    requestEngineApp("app.foobar", 2000);
    expect(engineStore.getState().session.pending).toHaveLength(1);
  });
});
