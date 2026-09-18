import { useStore } from "../../lib/store";
import { createStore } from "../../lib/store";

/**
 * 任务 61（AI-S UI 侧）：申请式授权模型。
 * Wine / 引擎进程默认能力收敛（无网络/无宿主盘，内核侧任务 61K 强制）；
 * 越权能力必须由进程显式申请 → 用户逐条裁决 → 默认拒绝。
 * 本模块是前端镜像：内核经系统事件上报申请，UI 呈现卡片，裁决回写事件总线。
 */

/** 可申请的越权能力范围（与内核 capbits 同名同序，禁止私加）。 */
export type CapabilityScope = "net" | "host-disk" | "usb-shared" | "clipboard" | "audio";

export interface CapabilityRequest {
  id: string;
  /** 申请方（如 wine:notepad.exe / engine:app.foobar）。 */
  subject: string;
  scope: CapabilityScope;
  /** 申请理由（进程如实申报；缺失时显示"未申报理由"，仍需用户裁决）。 */
  reason: string;
  at: number;
  status: "pending" | "granted" | "denied";
  /** granted 时本次会话有效期（进程重启即失效，绝不持久化）。 */
  grantSession: boolean;
}

export interface LatencySample {
  seg: LatencySegments;
  at: number;
}

export type LatencySegments = import("../../system/engine/engineModel").LatencySegments;

export const capabilityStore = createStore<{
  requests: CapabilityRequest[];
  /** 任务 53：引擎代理实测延迟样本（engine://state latency 事件追加）。 */
  latencyLog: LatencySample[];
}>({ requests: [], latencyLog: [] });

let seq = 0;

/** 登记一条能力申请（内核事件驱动；UI 演示亦可调用）。 */
export function pushCapabilityRequest(init: {
  subject: string;
  scope: CapabilityScope;
  reason: string;
}): CapabilityRequest {
  const req: CapabilityRequest = {
    id: `cap-${Date.now().toString(36)}-${(seq += 1)}`,
    subject: init.subject,
    scope: init.scope,
    reason: init.reason.trim() || "未申报理由",
    at: Date.now(),
    status: "pending",
    grantSession: false,
  };
  capabilityStore.setState((s) => ({
    requests: [...s.requests.slice(-19), req],
  }));
  return req;
}

/** 用户裁决（granted = 仅本次会话有效；denied 立即生效）。 */
export function decideCapabilityRequest(
  requests: CapabilityRequest[],
  id: string,
  decision: "granted" | "denied",
): CapabilityRequest[] {
  return requests.map((r) =>
    r.id === id ? { ...r, status: decision, grantSession: decision === "granted" } : r,
  );
}

/** 引擎代理延迟样本入账（engine://state kind=latency 时调用；上限 64 条防膨胀）。 */
export function recordLatencySample(seg: LatencySegments): void {
  capabilityStore.setState((s) => ({
    latencyLog: [...s.latencyLog.slice(-63), { seg, at: Date.now() }],
  }));
}

export function useCapabilityRequests(): CapabilityRequest[] {
  return useStore(capabilityStore, (s) => s.requests);
}

export function useCapabilityLatency(): LatencySample[] {
  return useStore(capabilityStore, (s) => s.latencyLog);
}
