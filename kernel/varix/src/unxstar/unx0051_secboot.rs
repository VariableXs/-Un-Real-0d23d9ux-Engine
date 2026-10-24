//! UNX-F0051 · SecureBoot/SetupMode 状态采集 · 诊断段 0x2040
//! 验收锚点：UNX-F0051-J1 SecureBoot/SetupMode/PK/KEK/db/dbx 六态
//! 联合推导与矛盾显性化，派生规则判据通过；纯功能代码 ≥220 行。
//! 纪律：只读采集——零写入、零固件副作用。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2040;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SB_VAL: u16 = SEG_BASE + 1;    // SecureBoot 变量值非 0/1
const E_SM_VAL: u16 = SEG_BASE + 2;    // SetupMode 值非 0/1
const E_CONTRA: u16 = SEG_BASE + 3;    // 状态组合矛盾（SB=1 且 SM=1 无 PK）
const E_PK_DUP: u16 = SEG_BASE + 4;    // PK 多实体（平台密钥唯一）
const E_KEK_EMPTY: u16 = SEG_BASE + 5; // SecureBoot=1 但 KEK 空
const E_DB_EMPTY: u16 = SEG_BASE + 6;  // SecureBoot=1 但 db 空
const E_SIG_FMT: u16 = SEG_BASE + 7;   // 证书签名类型 GUID 不识别
const E_FROZEN: u16 = SEG_BASE + 8;    // 采集账已冻结
const E_DEP: u16 = SEG_BASE + 9;       // dbx 未采集（吊销账缺失）

/// SecureBoot 变量的两个合法值。
pub const SB_DISABLED: u8 = 0;
pub const SB_ENABLED: u8 = 1;

/// SetupMode 变量语义：0=用户模式（已部署 PK），1=安装模式。
pub const SM_USER: u8 = 0;
pub const SM_SETUP: u8 = 1;

/// 证书签名类型 GUID（efi_signature_data 常见两类，闭集登记）。
pub const SIG_X509: [u8; 16] = [
    0xa5, 0xc2, 0x9d, 0x3f, 0x5b, 0x8c, 0x54, 0x4b,
    0xa6, 0xf8, 0x45, 0x9d, 0xf7, 0xd0, 0x2b, 0x0c, // X509 证书
];
pub const SIG_SHA256: [u8; 16] = [
    0x61, 0x6b, 0x6b, 0xc2, 0x6e, 0xd4, 0x9b, 0x47,
    0x8e, 0xd3, 0x8f, 0xc1, 0xd7, 0xd3, 0x4d, 0x2f, // SHA256 摘要
];

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SB_VAL => "SecureBoot 值非 0/1：固件越纲，登记后按禁用推导",
        E_SM_VAL => "SetupMode 值非 0/1：同上，显性立案不猜",
        E_CONTRA => "状态矛盾：SB=1 而 SM=1 或 PK 缺失，六态互锁判据拦下",
        E_PK_DUP => "PK 多实体：平台密钥全系统唯一，后到拒绝",
        E_KEK_EMPTY => "SecureBoot=1 而 KEK 为空：无法验链，拒绝使能",
        E_DB_EMPTY => "SecureBoot=1 而 db 为空：无授权证书，拒绝使能",
        E_SIG_FMT => "签名类型 GUID 不在闭集（X509/SHA256）：登记未知类型",
        E_FROZEN => "采集账已冻结：只读，重开新账",
        E_DEP => "dbx 吊销账缺失：使能判定前必须采集 dbx",
        _ => "未知诊断码",
    }
}

/// 六态联合视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootSecurityState {
    /// 已部署 PK、用户模式、SecureBoot 使能：全链验证态。
    DeployedEnforcing,
    /// 已部署 PK、用户模式、SecureBoot 显式禁用。
    DeployedAudit,
    /// 无 PK、安装模式：出厂/清密钥态。
    SetupOpen,
    /// 矛盾态：只能显性立案，禁止静默归一。
    Inconsistent(u16),
}

#[derive(Clone, Debug)]
pub struct SecBootSnapshot {
    pub secure_boot: Option<u8>,
    pub setup_mode: Option<u8>,
    pub pk_count: usize,
    pub kek_count: usize,
    pub db_count: usize,
    pub dbx_count: usize,
    pub unknown_sig_types: usize,
    pub state: Option<BootSecurityState>,
    pub frozen: bool,
}

impl SecBootSnapshot {
    pub fn new() -> SecBootSnapshot {
        SecBootSnapshot {
            secure_boot: None,
            setup_mode: None,
            pk_count: 0,
            kek_count: 0,
            db_count: 0,
            dbx_count: 0,
            unknown_sig_types: 0,
            state: None,
            frozen: false,
        }
    }

    /// 变量值采集（只读）。
    pub fn feed_var(&mut self, name: &str, value: &[u8]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        match name {
            "SecureBoot" => {
                if value.len() != 1 || (value[0] != SB_DISABLED && value[0] != SB_ENABLED) {
                    return Err(E_SB_VAL);
                }
                self.secure_boot = Some(value[0]);
            }
            "SetupMode" => {
                if value.len() != 1 || (value[0] != SM_USER && value[0] != SM_SETUP) {
                    return Err(E_SM_VAL);
                }
                self.setup_mode = Some(value[0]);
            }
            _ => {}
        }
        Ok(())
    }

    /// 签名库采集：每个实体一条签名类型 GUID。
    pub fn feed_sigdb(&mut self, which: &str, sig_type: &[u8; 16]) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if *sig_type != SIG_X509 && *sig_type != SIG_SHA256 {
            self.unknown_sig_types += 1;
            return Err(E_SIG_FMT);
        }
        match which {
            "PK" => {
                if self.pk_count >= 1 {
                    return Err(E_PK_DUP);
                }
                self.pk_count += 1;
            }
            "KEK" => self.kek_count += 1,
            "db" => self.db_count += 1,
            "dbx" => self.dbx_count += 1,
            _ => {}
        }
        Ok(())
    }

    /// 六态互锁推导（J1 核心）：
    /// ① SB=1 ⇒ PK=1、SM=用户、KEK≥1、db≥1、dbx 已采集，缺一即矛盾；
    /// ② SM=安装 ⇒ PK=0 且 SB 不得为 1；
    /// ③ 其余组合按 DeployedAudit/SetupOpen 归位。
    pub fn derive(&mut self) -> Result<BootSecurityState, u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        let sb = self.secure_boot.ok_or(E_SB_VAL)?;
        let sm = self.setup_mode.ok_or(E_SM_VAL)?;
        if self.dbx_count == 0 {
            return Err(E_DEP);
        }
        let st = if sb == SB_ENABLED {
            if sm != SM_USER || self.pk_count != 1 || self.kek_count == 0 || self.db_count == 0 {
                self.state = Some(BootSecurityState::Inconsistent(E_CONTRA));
                return Err(E_CONTRA);
            }
            BootSecurityState::DeployedEnforcing
        } else if sm == SM_SETUP {
            if self.pk_count != 0 {
                self.state = Some(BootSecurityState::Inconsistent(E_CONTRA));
                return Err(E_CONTRA);
            }
            BootSecurityState::SetupOpen
        } else {
            BootSecurityState::DeployedAudit
        };
        self.state = Some(st);
        Ok(st)
    }

    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if self.state.is_none() {
            self.derive()?;
        }
        self.frozen = true;
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.secure_boot.unwrap_or(0xFF).to_le_bytes());
        buf.extend_from_slice(&self.setup_mode.unwrap_or(0xFF).to_le_bytes());
        buf.extend_from_slice(&(self.pk_count as u64).to_le_bytes());
        buf.extend_from_slice(&(self.kek_count as u64).to_le_bytes());
        buf.extend_from_slice(&(self.db_count as u64).to_le_bytes());
        buf.extend_from_slice(&(self.dbx_count as u64).to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 状态名（诊断输出用，闭集字符串）。
pub fn state_name(s: &BootSecurityState) -> String {
    match s {
        BootSecurityState::DeployedEnforcing => "已部署·强制验证".to_string(),
        BootSecurityState::DeployedAudit => "已部署·审计".to_string(),
        BootSecurityState::SetupOpen => "安装模式·开放".to_string(),
        BootSecurityState::Inconsistent(c) => format!("矛盾态·码{:04X}", c),
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：部署 + 强制 → DeployedEnforcing。
    let mut s1 = SecBootSnapshot::new();
    s1.feed_var("SecureBoot", &[1])?;
    s1.feed_var("SetupMode", &[0])?;
    s1.feed_sigdb("PK", &SIG_X509)?;
    s1.feed_sigdb("KEK", &SIG_X509)?;
    s1.feed_sigdb("KEK", &SIG_SHA256)?;
    s1.feed_sigdb("db", &SIG_X509)?;
    s1.feed_sigdb("dbx", &SIG_SHA256)?;
    if s1.derive()? != BootSecurityState::DeployedEnforcing {
        return Err(E_CONTRA);
    }
    s1.freeze()?;
    if s1.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R2：部署 + 禁用 → DeployedAudit（db 允许为空）。
    let mut s2 = SecBootSnapshot::new();
    s2.feed_var("SecureBoot", &[0])?;
    s2.feed_var("SetupMode", &[0])?;
    s2.feed_sigdb("PK", &SIG_X509)?;
    s2.feed_sigdb("KEK", &SIG_X509)?;
    s2.feed_sigdb("dbx", &SIG_SHA256)?;
    if s2.derive()? != BootSecurityState::DeployedAudit {
        return Err(E_CONTRA);
    }
    hits += 1;

    // R3：无 PK + 安装模式 → SetupOpen。
    let mut s3 = SecBootSnapshot::new();
    s3.feed_var("SecureBoot", &[0])?;
    s3.feed_var("SetupMode", &[1])?;
    s3.feed_sigdb("dbx", &SIG_SHA256)?;
    if s3.derive()? != BootSecurityState::SetupOpen {
        return Err(E_CONTRA);
    }
    hits += 1;

    // I1：SB=1 而 SM=1 → E_CONTRA。
    let mut i1 = SecBootSnapshot::new();
    i1.feed_var("SecureBoot", &[1])?;
    i1.feed_var("SetupMode", &[1])?;
    i1.feed_sigdb("PK", &SIG_X509)?;
    i1.feed_sigdb("KEK", &SIG_X509)?;
    i1.feed_sigdb("db", &SIG_X509)?;
    i1.feed_sigdb("dbx", &SIG_SHA256)?;
    if i1.derive() != Err(E_CONTRA) {
        return Err(E_CONTRA);
    }
    hits += 1;

    // I2：PK 双实体 → E_PK_DUP。
    let mut i2 = SecBootSnapshot::new();
    i2.feed_sigdb("PK", &SIG_X509)?;
    if i2.feed_sigdb("PK", &SIG_X509) != Err(E_PK_DUP) {
        return Err(E_PK_DUP);
    }
    hits += 1;

    // I3：SB=1 而 KEK 空 → E_CONTRA（互锁同径）。
    let mut i3 = SecBootSnapshot::new();
    i3.feed_var("SecureBoot", &[1])?;
    i3.feed_var("SetupMode", &[0])?;
    i3.feed_sigdb("PK", &SIG_X509)?;
    i3.feed_sigdb("db", &SIG_X509)?;
    i3.feed_sigdb("dbx", &SIG_SHA256)?;
    if i3.derive() != Err(E_CONTRA) {
        return Err(E_CONTRA);
    }
    hits += 1;

    // I4：SecureBoot 值非法 → E_SB_VAL。
    let mut i4 = SecBootSnapshot::new();
    if i4.feed_var("SecureBoot", &[7]) != Err(E_SB_VAL) {
        return Err(E_SB_VAL);
    }
    hits += 1;

    // I5：dbx 未采集 → E_DEP。
    let mut i5 = SecBootSnapshot::new();
    i5.feed_var("SecureBoot", &[0])?;
    i5.feed_var("SetupMode", &[0])?;
    if i5.derive() != Err(E_DEP) {
        return Err(E_DEP);
    }
    hits += 1;

    // I6：未知签名类型 GUID → E_SIG_FMT 且计数留痕。
    let mut i6 = SecBootSnapshot::new();
    let unknown: [u8; 16] = [0xEE; 16];
    if i6.feed_sigdb("db", &unknown) != Err(E_SIG_FMT) || i6.unknown_sig_types != 1 {
        return Err(E_SIG_FMT);
    }
    hits += 1;

    Ok(hits)
}
