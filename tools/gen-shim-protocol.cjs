/**
 * 任务22（AI-B）：垫片协议类型同源生成器。
 * 单一事实源 tools/shim-protocol.source.json →
 *   ① src/lib/shim/protocol.ts（前端类型与常量；AI-V 运行时 shimInvoke.ts 依赖）
 *   ② src-tauri/src/shim_protocol.rs（后端 serde 类型 + 三段式用例）
 * 防漂移：两侧只允许由本脚本生成，手工改动会被下次生成覆盖。
 * 用法：node tools/gen-shim-protocol.cjs
 */
const fs = require("fs");
const path = require("path");

const src = JSON.parse(fs.readFileSync(path.join(__dirname, "shim-protocol.source.json"), "utf8"));

// ---------- TS 侧 ----------
const tsCodes = src.errorCodes.map((e) => `  | "${e.code}"`).join("\n");
const tsEvents = src.events.map((e) => `  | "${e.channel}"`).join("\n");
const tsCaps = src.capabilities.map((c) => `  | "${c.flag}"`).join("\n");
const ts = `// 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
// 协议规范见 docs/双域-垫片协议规范-v1.md（任务22，AI-V+AI-B）。

/** 垫片协议版本（版本协商基准，见 shim_hello）。 */
export const SHIM_PROTOCOL_VERSION = ${src.protocol.version};
/** 后端必须支持的最低前端协议版本。 */
export const SHIM_MIN_BACKEND_VERSION = ${src.protocol.minBackendVersion};

/** 三段式错误分类：OK | MAPPED_ERR(inner) | MISSING(cmd)。 */
export type ShimOutcome<T> =
  | { kind: "ok"; value: T }
  | { kind: "mapped_err"; code: ShimErrorCode; message: string; retryable: boolean }
  | { kind: "missing"; cmd: string };

/** 映射错误码（内圈错误，语义见协议规范 §3）。 */
export type ShimErrorCode =
${tsCodes};

/** 事件反向通道（后端→前端 emit，语义与 Tauri event 同构）。 */
export type ShimEventChannel =
${tsEvents};

/** 能力位（版本协商返回，见 SHIM_HELLO）。 */
export type ShimCapability =
${tsCaps};

/** 各错误码是否可重试（同源自 source.json）。 */
export const SHIM_RETRYABLE: Record<ShimErrorCode, boolean> = {
${src.errorCodes.map((e) => `  "${e.code}": ${e.retryable},`).join("\n")}
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
`;
fs.mkdirSync(path.join(__dirname, "..", "src", "lib", "shim"), { recursive: true });
fs.writeFileSync(path.join(__dirname, "..", "src", "lib", "shim", "protocol.ts"), ts, "utf8");

// ---------- Rust 侧 ----------
const rustCodes = src.errorCodes
  .map((e) => {
    const body = e.code.replace(/^SHIM_/, "");
    const v = body.split("_").map((w) => w.charAt(0) + w.slice(1).toLowerCase()).join("");
    return `    #[serde(rename = "${e.code}")]\n    ${v},`;
  })
  .join("\n");
const rustCaps = src.capabilities
  .map((c) => {
    const v = c.flag.charAt(0).toUpperCase() + c.flag.slice(1);
    return `    #[serde(rename = "${c.flag}")]\n    ${v},`;
  })
  .join("\n");
const rs = `//! 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
//! 垫片协议类型（任务22，AI-B）：三段式错误 + 版本协商 + 能力位。

use serde::{Deserialize, Serialize};

/// 垫片协议版本（版本协商基准）。
pub const SHIM_PROTOCOL_VERSION: u32 = ${src.protocol.version};
/// 前端必须支持的最低后端协议版本。
pub const SHIM_MIN_FRONTEND_VERSION: u32 = ${src.protocol.minFrontendVersion};

/// 映射错误码（三段式第二段 MAPPED_ERR 的内圈错误）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShimErrorCode {
${rustCodes}
}

impl ShimErrorCode {
    /// 是否可重试（同源自 source.json）。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ShimErrorCode::Timeout | ShimErrorCode::BackendDown | ShimErrorCode::Internal
        )
    }
}

/// 能力位（shim_hello 能力位图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShimCapability {
${rustCaps}
}

/// shim_hello 应答：版本协商 + 能力位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimHello {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "backendVersion")]
    pub backend_version: u32,
    pub capabilities: Vec<ShimCapability>,
}

/// 三段式传输形态（垫片层统一编码）。
/// 注意：untagged 按序匹配，Ok(Value) 兜底必须放最后，否则吞掉 MappedErr/Missing。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShimReply {
    /// 映射错误。
    MappedErr {
        #[serde(rename = "__shim_error")]
        err: ShimErrorBody,
    },
    /// 命令不存在。
    Missing {
        #[serde(rename = "__shim_missing")]
        cmd: String,
    },
    /// 正常载荷（JSON 透传）。
    Ok(serde_json::Value),
}

/// 映射错误体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimErrorBody {
    pub code: ShimErrorCode,
    pub message: String,
}

impl ShimReply {
    /// 三段式判定：ok / mapped_err / missing。
    pub fn kind(&self) -> &'static str {
        match self {
            ShimReply::Ok(_) => "ok",
            ShimReply::MappedErr { .. } => "mapped_err",
            ShimReply::Missing { .. } => "missing",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_path_passes_through() {
        let r = ShimReply::Ok(serde_json::json!({ "n": 1 }));
        assert_eq!(r.kind(), "ok");
    }

    #[test]
    fn mapped_err_roundtrip() {
        let body = ShimErrorBody {
            code: ShimErrorCode::PermDenied,
            message: "白名单拒绝".into(),
        };
        let raw = serde_json::to_value(ShimReply::MappedErr { err: body }).unwrap();
        assert!(raw.get("__shim_error").is_some());
        let back: ShimReply = serde_json::from_value(raw).unwrap();
        assert_eq!(back.kind(), "mapped_err");
        if let ShimReply::MappedErr { err } = back {
            assert_eq!(err.code, ShimErrorCode::PermDenied);
            assert!(!err.code.retryable());
        } else {
            unreachable!();
        }
    }

    #[test]
    fn missing_path_carries_cmd() {
        let raw = serde_json::to_value(ShimReply::Missing { cmd: "nope".into() }).unwrap();
        let back: ShimReply = serde_json::from_value(raw).unwrap();
        assert_eq!(back.kind(), "missing");
        if let ShimReply::Missing { cmd } = back {
            assert_eq!(cmd, "nope");
        } else {
            unreachable!();
        }
    }

    #[test]
    fn version_negotiation_fields_present() {
        let hello = ShimHello {
            protocol_version: SHIM_PROTOCOL_VERSION,
            backend_version: 1,
            capabilities: vec![ShimCapability::KvStore],
        };
        let raw = serde_json::to_value(&hello).unwrap();
        assert_eq!(raw["protocolVersion"], SHIM_PROTOCOL_VERSION);
        let back: ShimHello = serde_json::from_value(raw).unwrap();
        assert!(back.capabilities.contains(&ShimCapability::KvStore));
    }
}
`;
fs.writeFileSync(path.join(__dirname, "..", "src-tauri", "src", "shim_protocol.rs"), rs, "utf8");

console.log("generated: src/lib/shim/protocol.ts, src-tauri/src/shim_protocol.rs");
