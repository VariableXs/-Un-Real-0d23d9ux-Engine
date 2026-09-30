/**
 * 垫片协议类型同源生成器（任务22 初版，S2.01 三体定版 v2 · AI-3）。
 * 单一事实源 tools/shim-protocol.source.json → 三产物：
 *   ① src/lib/shim/protocol.ts（前端类型与常量；AI-V 运行时 shimInvoke.ts 依赖）
 *   ② src-tauri/src/shim_protocol.rs（后端 serde 类型 + 三段式用例）
 *   ③ kernel/varix/src/vport/shim_protocol.rs（内核零依赖常量表：错误码线缆编码/
 *      能力位/事件频道静态表——no_std、无堆分配，vport 域纪律）
 * 防漂移：三产物只允许由本脚本生成，手工改动会被下次生成覆盖；
 * 一致性由 tools/audit.cjs 的 SHIM PROTOCOL GATE 段看护（内存再生 vs 磁盘逐字节比对）。
 * 用法：node tools/gen-shim-protocol.cjs（或被 audit.cjs 以 buildArtifacts 导入）。
 */
"use strict";
const fs = require("fs");
const path = require("path");

const SRC_PATH = path.join(__dirname, "shim-protocol.source.json");
const DOC_REF = "docs/垫片协议规范.md";

/** SHIM_INVALID_ARGS → InvalidArgs（后端 enum 变体名）。 */
function variantOf(code) {
  const body = code.replace(/^SHIM_/, "");
  return body.split("_").map((w) => w.charAt(0) + w.slice(1).toLowerCase()).join("");
}

/** SHIM_INVALID_ARGS → INVALID_ARGS（内核线缆常量名）。 */
function constOf(code) {
  return code.replace(/^SHIM_/, "");
}

function buildArtifacts(src) {
  // ---------- ① TS 侧 ----------
  const tsCodes = src.errorCodes.map((e) => `  | "${e.code}"`).join("\n");
  const tsEvents = src.events.map((e) => `  | "${e.channel}"`).join("\n");
  const tsCaps = src.capabilities.map((c) => `  | "${c.flag}"`).join("\n");
  const ts = `// 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
// 协议规范见 ${DOC_REF}（任务22 定版，S2.01 v2 · AI-3）。

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

  // ---------- ② 后端（src-tauri）侧 ----------
  const rustCodes = src.errorCodes
    .map((e) => `    #[serde(rename = "${e.code}")]\n    ${variantOf(e.code)},`)
    .join("\n");
  const rustCaps = src.capabilities
    .map((c) => {
      const v = c.flag.charAt(0).toUpperCase() + c.flag.slice(1);
      return `    #[serde(rename = "${c.flag}")]\n    ${v},`;
    })
    .join("\n");
  // retryable 由单源 retryable 标志推导（禁手写清单防漂移）。
  const retryableArms = src.errorCodes
    .filter((e) => e.retryable)
    .map((e) => `ShimErrorCode::${variantOf(e.code)}`)
    .join("\n            | ");
  const retryableBody = retryableArms
    ? `matches!(\n            self,\n            ${retryableArms}\n        )`
    : "let _ = self;\n        false";
  const rs = `//! 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
//! 垫片协议类型（任务22 定版，S2.01 v2 · AI-3）：三段式错误 + 版本协商 + 能力位。

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
    /// 是否可重试（同源自 source.json retryable 标志）。
    pub fn retryable(self) -> bool {
        ${retryableBody}
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
    fn kv_full_code_is_first_class() {
        // S2.03：KV 满容量是一等错误码，不可重试、线缆名稳定。
        let body = ShimErrorBody {
            code: ShimErrorCode::KvFull,
            message: "KV 账本/溢出区已满".into(),
        };
        let raw = serde_json::to_value(ShimReply::MappedErr { err: body }).unwrap();
        assert_eq!(raw["__shim_error"]["code"], "SHIM_KV_FULL");
        let back: ShimReply = serde_json::from_value(raw).unwrap();
        if let ShimReply::MappedErr { err } = back {
            assert_eq!(err.code, ShimErrorCode::KvFull);
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
            backend_version: ${src.protocol.version},
            capabilities: vec![ShimCapability::KvStore],
        };
        let raw = serde_json::to_value(&hello).unwrap();
        assert_eq!(raw["protocolVersion"], SHIM_PROTOCOL_VERSION);
        let back: ShimHello = serde_json::from_value(raw).unwrap();
        assert!(back.capabilities.contains(&ShimCapability::KvStore));
    }
}
`;

  // ---------- ③ 内核侧（vport 域，零依赖无堆分配） ----------
  const kErrConsts = src.errorCodes
    .map((e, i) => `    pub const ${constOf(e.code)}: u8 = ${i};`)
    .join("\n");
  const kErrNames = src.errorCodes.map((e) => `"${e.code}"`).join(", ");
  const kRetryableArms = src.errorCodes
    .filter((e) => e.retryable)
    .map((e) => constOf(e.code))
    .join("\n            | ");
  const kRetryableBody = kRetryableArms
    ? `matches!(\n            code,\n            ${kRetryableArms}\n        )`
    : "let _ = code;\n        false";
  const kCaps = src.capabilities
    .map((c, i) => {
      // camelCase → UPPER_SNAKE：kvStore → KV_STORE。
      const name = c.flag.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase();
      return `    pub const ${name}: u8 = ${i};`;
    })
    .join("\n");
  const kCapsNames = src.capabilities.map((c) => `"${c.flag}"`).join(", ");
  const kEvents = src.events.map((e) => `"${e.channel}"`).join(", ");
  const krs = `//! 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
//! 内核侧垫片协议常量表（任务22 定版，S2.01 v2 · AI-3）：零依赖、无堆分配（vport 域纪律）。
//! 错误码 u8 序号即线缆编码；能力位/事件频道静态表供内核发射端与命令面校验。

/// 垫片协议版本（版本协商基准）。
pub const SHIM_PROTOCOL_VERSION: u32 = ${src.protocol.version};
/// 后端要求的前端最低协议版本。
pub const SHIM_MIN_FRONTEND_VERSION: u32 = ${src.protocol.minFrontendVersion};

/// 三段式应答状态字节（协议规范 §3：OK | MAPPED_ERR | MISSING）。
pub const REPLY_OK: u8 = 0;
pub const REPLY_MAPPED_ERR: u8 = 1;
pub const REPLY_MISSING: u8 = 2;

/// 映射错误码：u8 序号 = 线缆编码，NAMES 静态表同源协议名。
pub mod err {
${kErrConsts}
    /// 错误码总数（越界码一律按 INVALID_ARGS 兜底，不猜测）。
    pub const COUNT: u8 = ${src.errorCodes.length};

    /// 协议名表（与 TS/后端同源，下标即线缆编码）。
    pub const NAMES: [&str; COUNT as usize] = [${kErrNames}];

    /// 线缆码 → 协议名（越界返回 None，调用方不得猜测）。
    pub fn name(code: u8) -> Option<&'static str> {
        NAMES.get(code as usize).copied()
    }

    /// 是否可重试（同源自 source.json retryable 标志）。
    pub fn is_retryable(code: u8) -> bool {
        if code >= COUNT {
            return false;
        }
        ${kRetryableBody}
    }
}

/// 能力位：u8 位序 + 协议名表（shim_hello 能力位图）。
pub mod caps {
${kCaps}

    /// 协议名表（与 TS/后端同源，下标即位序）。
    pub const NAMES: [&str; ${src.capabilities.length}] = [${kCapsNames}];

    /// 位序 → 协议名（越界返回 None，调用方不得猜测）。
    pub fn name(flag: u8) -> Option<&'static str> {
        NAMES.get(flag as usize).copied()
    }
}

/// 事件反向通道频道名表（后端→前端 emit；内核发射端校验用，语义与 Tauri event 同构）。
pub mod events {
    pub const CHANNELS: [&str; ${src.events.length}] = [${kEvents}];

    /// 频道名是否在协议表内（未知频道拒绝发射，不静默透传）。
    pub fn is_known(channel: &str) -> bool {
        CHANNELS.contains(&channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_consts_match_source() {
        assert_eq!(SHIM_PROTOCOL_VERSION, ${src.protocol.version});
        assert_eq!(SHIM_MIN_FRONTEND_VERSION, ${src.protocol.minFrontendVersion});
    }

    #[test]
    fn err_names_roundtrip_and_bounds() {
        for i in 0..err::COUNT {
            let n = err::name(i).unwrap_or_else(|| panic!("err name missing at {}", i));
            assert!(n.starts_with("SHIM_"), "协议名必须带 SHIM_ 前缀: {}", n);
        }
        assert!(err::name(err::COUNT).is_none(), "COUNT 本身越界");
        assert!(err::name(200).is_none(), "野码不得猜名");
    }

    #[test]
    fn retryable_flags_match_source() {
        assert!(err::is_retryable(err::TIMEOUT));
        assert!(err::is_retryable(err::BACKEND_DOWN));
        assert!(err::is_retryable(err::INTERNAL));
        assert!(!err::is_retryable(err::KV_FULL), "KV 满容量明确拒绝，不可重试");
        assert!(!err::is_retryable(err::INVALID_ARGS));
        assert!(!err::is_retryable(err::PERM_DENIED));
        assert!(!err::is_retryable(200), "越界码不可重试");
    }

    #[test]
    fn caps_table_roundtrip() {
        assert_eq!(caps::name(caps::KV_STORE), Some("kvStore"));
        assert!(caps::name(200).is_none());
    }

    #[test]
    fn events_table_membership() {
        assert!(events::is_known("boot://event"));
        assert!(events::is_known("settings://changed"));
        assert!(events::is_known("shim://input"));
        assert!(events::is_known("engine://state"));
        assert!(!events::is_known("bogus://channel"));
    }

    #[test]
    fn reply_status_consts_distinct() {
        assert_ne!(REPLY_OK, REPLY_MAPPED_ERR);
        assert_ne!(REPLY_OK, REPLY_MISSING);
        assert_ne!(REPLY_MAPPED_ERR, REPLY_MISSING);
    }
}
`;

  return { ts, rs, krs };
}

function writeArtifacts(src) {
  const arts = buildArtifacts(src);
  fs.mkdirSync(path.join(__dirname, "..", "src", "lib", "shim"), { recursive: true });
  fs.writeFileSync(path.join(__dirname, "..", "src", "lib", "shim", "protocol.ts"), arts.ts, "utf8");
  fs.writeFileSync(path.join(__dirname, "..", "src-tauri", "src", "shim_protocol.rs"), arts.rs, "utf8");
  fs.writeFileSync(
    path.join(__dirname, "..", "kernel", "varix", "src", "vport", "shim_protocol.rs"),
    arts.krs,
    "utf8",
  );
  console.log(
    "generated: src/lib/shim/protocol.ts, src-tauri/src/shim_protocol.rs, kernel/varix/src/vport/shim_protocol.rs",
  );
}

module.exports = { buildArtifacts };

if (require.main === module) {
  const src = JSON.parse(fs.readFileSync(SRC_PATH, "utf8"));
  writeArtifacts(src);
}
