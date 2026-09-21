//! 本文件由 tools/gen-shim-protocol.cjs 从 tools/shim-protocol.source.json 生成，禁止手改。
//! 内核侧垫片协议常量表（任务22 定版，S2.01 v2 · AI-3）：零依赖、无堆分配（vport 域纪律）。
//! 错误码 u8 序号即线缆编码；能力位/事件频道静态表供内核发射端与命令面校验。

/// 垫片协议版本（版本协商基准）。
pub const SHIM_PROTOCOL_VERSION: u32 = 2;
/// 后端要求的前端最低协议版本。
pub const SHIM_MIN_FRONTEND_VERSION: u32 = 2;

/// 三段式应答状态字节（协议规范 §3：OK | MAPPED_ERR | MISSING）。
pub const REPLY_OK: u8 = 0;
pub const REPLY_MAPPED_ERR: u8 = 1;
pub const REPLY_MISSING: u8 = 2;

/// 映射错误码：u8 序号 = 线缆编码，NAMES 静态表同源协议名。
pub mod err {
    pub const INVALID_ARGS: u8 = 0;
    pub const UNSUPPORTED: u8 = 1;
    pub const TIMEOUT: u8 = 2;
    pub const BACKEND_DOWN: u8 = 3;
    pub const VERSION_MISMATCH: u8 = 4;
    pub const PERM_DENIED: u8 = 5;
    pub const KV_FULL: u8 = 6;
    pub const INTERNAL: u8 = 7;
    /// 错误码总数（越界码一律按 INVALID_ARGS 兜底，不猜测）。
    pub const COUNT: u8 = 8;

    /// 协议名表（与 TS/后端同源，下标即线缆编码）。
    pub const NAMES: [&str; COUNT as usize] = ["SHIM_INVALID_ARGS", "SHIM_UNSUPPORTED", "SHIM_TIMEOUT", "SHIM_BACKEND_DOWN", "SHIM_VERSION_MISMATCH", "SHIM_PERM_DENIED", "SHIM_KV_FULL", "SHIM_INTERNAL"];

    /// 线缆码 → 协议名（越界返回 None，调用方不得猜测）。
    pub fn name(code: u8) -> Option<&'static str> {
        NAMES.get(code as usize).copied()
    }

    /// 是否可重试（同源自 source.json retryable 标志）。
    pub fn is_retryable(code: u8) -> bool {
        if code >= COUNT {
            return false;
        }
        matches!(
            code,
            TIMEOUT
            | BACKEND_DOWN
            | INTERNAL
        )
    }
}

/// 能力位：u8 位序 + 协议名表（shim_hello 能力位图）。
pub mod caps {
    pub const KV_STORE: u8 = 0;
    pub const FS_WORKSPACE: u8 = 1;
    pub const FS_SHARED: u8 = 2;
    pub const WINDOW_MGR: u8 = 3;
    pub const EMBED: u8 = 4;
    pub const INPUT_BUS: u8 = 5;
    pub const CLIPBOARD: u8 = 6;
    pub const PROCESS_CTL: u8 = 7;
    pub const VAULT: u8 = 8;
    pub const APPLOG: u8 = 9;
    pub const NET_STACK: u8 = 10;
    pub const AUDIO_OUT: u8 = 11;
    pub const DISPLAY_CTL: u8 = 12;
    pub const POWER_CTL: u8 = 13;
    pub const TASK_QUEUE: u8 = 14;
    pub const SCHEDULER: u8 = 15;
    pub const EXT_LOADER: u8 = 16;
    pub const OPENHUB_GATEWAY: u8 = 17;
    pub const WINE_CHANNEL: u8 = 18;
    pub const ENGINE_VM: u8 = 19;

    /// 协议名表（与 TS/后端同源，下标即位序）。
    pub const NAMES: [&str; 20] = ["kvStore", "fsWorkspace", "fsShared", "windowMgr", "embed", "inputBus", "clipboard", "processCtl", "vault", "applog", "netStack", "audioOut", "displayCtl", "powerCtl", "taskQueue", "scheduler", "extLoader", "openhubGateway", "wineChannel", "engineVm"];

    /// 位序 → 协议名（越界返回 None，调用方不得猜测）。
    pub fn name(flag: u8) -> Option<&'static str> {
        NAMES.get(flag as usize).copied()
    }
}

/// 事件反向通道频道名表（后端→前端 emit；内核发射端校验用，语义与 Tauri event 同构）。
pub mod events {
    pub const CHANNELS: [&str; 20] = ["boot://event", "settings://changed", "sys://applog", "sys://curtain", "sys://quit-request", "sys://shortcut", "sys://taskbar-yield", "sys://display-changed", "sys://anticheat", "embed://state", "embed://popup", "embed://native-geo", "embed://native-max", "embed://native-min", "usb://progress", "usb://removed", "taskbar://state", "watch://escape", "shim://input", "engine://state"];

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
        assert_eq!(SHIM_PROTOCOL_VERSION, 2);
        assert_eq!(SHIM_MIN_FRONTEND_VERSION, 2);
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
