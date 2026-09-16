/**
 * 垫片 invoke 前端层（任务 22 · AI-V 半边）。
 *
 * 协议单源与类型见 tools/shim-protocol.source.json → protocol.ts（AI-B 同源生成）。
 * 本模块只做三件事：传输层注入、三段式 invoke、事件反向通道订阅。
 * 传输层注入式设计：Windows 侧包 Tauri invoke，VARIX 内核侧注入内核传输——
 * 同一份前端代码两个后端，不 fork（总案阶段 3 简洁性红线）。
 */
import {
  SHIM_PROTOCOL_VERSION,
  decodeShimOutcome,
  shimVersionCompatible,
  type ShimCapability,
  type ShimErrorCode,
  type ShimEventChannel,
  type ShimHello,
} from "./protocol";

/**
 * 传输层：收（cmd, args），回传输形态原始值。
 * 传输形态三段式（协议规范 §3）：载荷直达 | {__shim_error:{code,message}} | {__shim_missing:cmd}。
 */
export type ShimTransport = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

export class ShimMappedError extends Error {
  readonly code: ShimErrorCode;
  readonly retryable: boolean;
  constructor(code: ShimErrorCode, message: string, retryable: boolean) {
    super(message);
    this.name = "ShimMappedError";
    this.code = code;
    this.retryable = retryable;
  }
}

export class ShimMissingError extends Error {
  readonly cmd: string;
  constructor(cmd: string) {
    super(`SHIM_MISSING:${cmd}`);
    this.name = "ShimMissingError";
    this.cmd = cmd;
  }
}

let transport: ShimTransport | null = null;
let hello: ShimHello | null = null;

/** 安装传输层（应用启动时调用一次；重复安装视为配置错误，不静默）。 */
export function installShimTransport(t: ShimTransport): void {
  if (transport) throw new ShimMappedError("SHIM_INVALID_ARGS", "垫片传输层已安装", false);
  transport = t;
}

/** 仅供测试重置。 */
export function resetShimTransportForTest(): void {
  transport = null;
  hello = null;
}

function assertTransport(): ShimTransport {
  if (!transport) throw new ShimMappedError("SHIM_BACKEND_DOWN", "垫片传输层未安装", true);
  return transport;
}

/** 版本协商握手：前端启动第一步（协议规范 §5）。不兼容抛 SHIM_VERSION_MISMATCH。 */
export async function shimHello(): Promise<ShimHello> {
  // 注意：局部变量不得叫 t —— audit.cjs 的 i18n 扫描会把 t("...") 误判为词典键。
  const tp = assertTransport();
  const res = await tp("shim_hello", { frontendVersion: SHIM_PROTOCOL_VERSION });
  const decoded = decodeShimOutcome<ShimHello>("shim_hello", res);
  if (decoded.kind !== "ok") throw new ShimMappedError("SHIM_INTERNAL", "shim_hello 应答异常", true);
  if (!shimVersionCompatible(decoded.value)) {
    throw new ShimMappedError(
      "SHIM_VERSION_MISMATCH",
      `协议版本不兼容：前端 ${SHIM_PROTOCOL_VERSION} / 后端 ${decoded.value.protocolVersion}`,
      false,
    );
  }
  hello = decoded.value;
  return hello;
}

/** 已协商能力位查询（未握手时返回空集，调用方按 UNSUPPORTED 降级）。 */
export function shimCapability(flag: ShimCapability): boolean {
  return hello?.capabilities.includes(flag) ?? false;
}

/**
 * 垫片 invoke：与既有 ipc.ts invoke 同签名语义。
 * MAPPED_ERR / MISSING 均以异常抛出，MISSING 错误消息即 cmd（供降级面查映射表）。
 */
export async function shimInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const t = assertTransport();
  const raw = await t(cmd, args ?? {});
  const outcome = decodeShimOutcome<T>(cmd, raw);
  if (outcome.kind === "ok") return outcome.value;
  if (outcome.kind === "missing") throw new ShimMissingError(outcome.cmd);
  throw new ShimMappedError(outcome.code, outcome.message, outcome.retryable);
}

/** 事件反向通道订阅（协议规范 §4），返回退订函数。 */
export type ShimEventListener = (payload: unknown) => void;
const listeners = new Map<ShimEventChannel, Set<ShimEventListener>>();

export function onShimEvent(channel: ShimEventChannel, fn: ShimEventListener): () => void {
  let set = listeners.get(channel);
  if (!set) {
    set = new Set();
    listeners.set(channel, set);
  }
  set.add(fn);
  return () => {
    set.delete(fn);
  };
}

/** 传输层收到事件后调用（不暴露给业务代码）；未识别的 channel 静默忽略由 audit 门禁兜底。 */
export function dispatchShimEvent(channel: ShimEventChannel, payload: unknown): void {
  const set = listeners.get(channel);
  if (!set) return;
  for (const fn of set) fn(payload);
}
