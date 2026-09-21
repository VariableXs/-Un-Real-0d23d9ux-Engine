// 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
// 协议规范见 docs/垫片协议规范.md（任务22 定版，S2.01 v2 · AI-3）。

/** 垫片协议版本（版本协商基准，见 shim_hello）。 */
export const SHIM_PROTOCOL_VERSION = 2;
/** 后端必须支持的最低前端协议版本。 */
export const SHIM_MIN_BACKEND_VERSION = 2;

/** 三段式错误分类：OK | MAPPED_ERR(inner) | MISSING(cmd)。 */
export type ShimOutcome<T> =
  | { kind: "ok"; value: T }
  | { kind: "mapped_err"; code: ShimErrorCode; message: string; retryable: boolean }
  | { kind: "missing"; cmd: string };

/** 映射错误码（内圈错误，语义见协议规范 §3）。 */
export type ShimErrorCode =
  | "SHIM_INVALID_ARGS"
  | "SHIM_UNSUPPORTED"
  | "SHIM_TIMEOUT"
  | "SHIM_BACKEND_DOWN"
  | "SHIM_VERSION_MISMATCH"
  | "SHIM_PERM_DENIED"
  | "SHIM_KV_FULL"
  | "SHIM_INTERNAL";

/** 事件反向通道（后端→前端 emit，语义与 Tauri event 同构）。 */
export type ShimEventChannel =
  | "boot://event"
  | "settings://changed"
  | "sys://applog"
  | "sys://curtain"
  | "sys://quit-request"
  | "sys://shortcut"
  | "sys://taskbar-yield"
  | "sys://display-changed"
  | "sys://anticheat"
  | "embed://state"
  | "embed://popup"
  | "embed://native-geo"
  | "embed://native-max"
  | "embed://native-min"
  | "usb://progress"
  | "usb://removed"
  | "taskbar://state"
  | "watch://escape"
  | "shim://input"
  | "engine://state";

/** 能力位（版本协商返回，见 SHIM_HELLO）。 */
export type ShimCapability =
  | "kvStore"
  | "fsWorkspace"
  | "fsShared"
  | "windowMgr"
  | "embed"
  | "inputBus"
  | "clipboard"
  | "processCtl"
  | "vault"
  | "applog"
  | "netStack"
  | "audioOut"
  | "displayCtl"
  | "powerCtl"
  | "taskQueue"
  | "scheduler"
  | "extLoader"
  | "openhubGateway"
  | "wineChannel"
  | "engineVm";

/** 各错误码是否可重试（同源自 source.json）。 */
export const SHIM_RETRYABLE: Record<ShimErrorCode, boolean> = {
  "SHIM_INVALID_ARGS": false,
  "SHIM_UNSUPPORTED": false,
  "SHIM_TIMEOUT": true,
  "SHIM_BACKEND_DOWN": true,
  "SHIM_VERSION_MISMATCH": false,
  "SHIM_PERM_DENIED": false,
  "SHIM_KV_FULL": false,
  "SHIM_INTERNAL": true,
};

/** shim_hello 应答：版本协商 + 能力位。 */
export interface ShimHello {
  protocolVersion: number;
  backendVersion: number;
  capabilities: ShimCapability[];
}

/** 版本协商判定：不满足最低版本即降级面。 */
export function shimVersionCompatible(hello: ShimHello): boolean {
  return (
    hello.protocolVersion === SHIM_PROTOCOL_VERSION &&
    hello.backendVersion >= SHIM_MIN_BACKEND_VERSION
  );
}

/**
 * 三段式解码：把垫片应答归一为 ShimOutcome。
 * - 正常载荷 → ok
 * - 形如 { __shim_error: { code, message } } → mapped_err
 * - 形如 { __shim_missing: cmd } → missing
 */
export function decodeShimOutcome<T>(cmd: string, raw: unknown): ShimOutcome<T> {
  if (typeof raw === "object" && raw !== null) {
    const obj = raw as Record<string, unknown>;
    if ("__shim_error" in obj) {
      const e = obj["__shim_error"] as Record<string, unknown>;
      const code = e["code"];
      if (typeof code === "string" && code in SHIM_RETRYABLE) {
        return {
          kind: "mapped_err",
          code: code as ShimErrorCode,
          message: typeof e["message"] === "string" ? e["message"] : "",
          retryable: SHIM_RETRYABLE[code as ShimErrorCode],
        };
      }
      return { kind: "mapped_err", code: "SHIM_INTERNAL", message: JSON.stringify(e), retryable: true };
    }
    if ("__shim_missing" in obj) {
      return { kind: "missing", cmd: typeof obj["__shim_missing"] === "string" ? obj["__shim_missing"] : cmd };
    }
  }
  return { kind: "ok", value: raw as T };
}

/** 三段式编码（后端应答 → 传输形态；测试与降级面共用）。 */
export function encodeShimMappedError(code: ShimErrorCode, message: string): unknown {
  return { __shim_error: { code, message } };
}
export function encodeShimMissing(cmd: string): unknown {
  return { __shim_missing: cmd };
}
