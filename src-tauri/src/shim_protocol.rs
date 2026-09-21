//! 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
//! 垫片协议类型（任务22 定版，S2.01 v2 · AI-3）：三段式错误 + 版本协商 + 能力位。

use serde::{Deserialize, Serialize};

/// 垫片协议版本（版本协商基准）。
pub const SHIM_PROTOCOL_VERSION: u32 = 2;
/// 前端必须支持的最低后端协议版本。
pub const SHIM_MIN_FRONTEND_VERSION: u32 = 2;

/// 映射错误码（三段式第二段 MAPPED_ERR 的内圈错误）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShimErrorCode {
    #[serde(rename = "SHIM_INVALID_ARGS")]
    InvalidArgs,
    #[serde(rename = "SHIM_UNSUPPORTED")]
    Unsupported,
    #[serde(rename = "SHIM_TIMEOUT")]
    Timeout,
    #[serde(rename = "SHIM_BACKEND_DOWN")]
    BackendDown,
    #[serde(rename = "SHIM_VERSION_MISMATCH")]
    VersionMismatch,
    #[serde(rename = "SHIM_PERM_DENIED")]
    PermDenied,
    #[serde(rename = "SHIM_KV_FULL")]
    KvFull,
    #[serde(rename = "SHIM_INTERNAL")]
    Internal,
}

impl ShimErrorCode {
    /// 是否可重试（同源自 source.json retryable 标志）。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ShimErrorCode::Timeout
            | ShimErrorCode::BackendDown
            | ShimErrorCode::Internal
        )
    }
}

/// 能力位（shim_hello 能力位图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShimCapability {
    #[serde(rename = "kvStore")]
    KvStore,
    #[serde(rename = "fsWorkspace")]
    FsWorkspace,
    #[serde(rename = "fsShared")]
    FsShared,
    #[serde(rename = "windowMgr")]
    WindowMgr,
    #[serde(rename = "embed")]
    Embed,
    #[serde(rename = "inputBus")]
    InputBus,
    #[serde(rename = "clipboard")]
    Clipboard,
    #[serde(rename = "processCtl")]
    ProcessCtl,
    #[serde(rename = "vault")]
    Vault,
    #[serde(rename = "applog")]
    Applog,
    #[serde(rename = "netStack")]
    NetStack,
    #[serde(rename = "audioOut")]
    AudioOut,
    #[serde(rename = "displayCtl")]
    DisplayCtl,
    #[serde(rename = "powerCtl")]
    PowerCtl,
    #[serde(rename = "taskQueue")]
    TaskQueue,
    #[serde(rename = "scheduler")]
    Scheduler,
    #[serde(rename = "extLoader")]
    ExtLoader,
    #[serde(rename = "openhubGateway")]
    OpenhubGateway,
    #[serde(rename = "wineChannel")]
    WineChannel,
    #[serde(rename = "engineVm")]
    EngineVm,
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
            backend_version: 2,
            capabilities: vec![ShimCapability::KvStore],
        };
        let raw = serde_json::to_value(&hello).unwrap();
        assert_eq!(raw["protocolVersion"], SHIM_PROTOCOL_VERSION);
        let back: ShimHello = serde_json::from_value(raw).unwrap();
        assert!(back.capabilities.contains(&ShimCapability::KvStore));
    }
}
