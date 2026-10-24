//! UNX-F0213 · 报告完整性签名
//!
//! 锚点判据（施工书原文）：UNX-F0213-J1 报告哈希签名落账，
//! 篡改检出 100%。
//!
//! 设计：报告正文（逐字段序）→ FNV 内容哈希 → 签名落账
//! （版本 + 哈希 + 签名时刻）→ 验签器（重算哈希对拍）。红线：
//! ①任何单字节篡改必检出（含字段值/字段序/字段增删）；②未签名
//! 报告验签必拒；③账内签名不可被静默覆盖（重复签名拒）。
//!
//! 错误码段：0x2C50（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0213_VERSION: &str = "F0213-repsign-v1";

/// 诊断码段基址（UNX-F0213 独占）。
pub const SEG_BASE: u16 = 0x2C50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 篡改检出（哈希不符）。
    TamperFound = SEG_BASE + 1,
    /// 未签名验签。
    UnsignedVerify = SEG_BASE + 2,
    /// 重复签名。
    DupSign = SEG_BASE + 3,
    /// 空正文签名。
    EmptyBody = SEG_BASE + 4,
    /// 账内签名与报告版本不符。
    VersionMismatch = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// FNV-1a 64 位。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 报告正文。
#[derive(Debug, Default, Clone)]
pub struct ReportBody {
    /// 字段（名, 值）。
    pub fields: Vec<(String, String)>,
}

impl ReportBody {
    /// 规范化字节流：名\0值\n 顺序拼接（字段序参与签名）。
    pub fn canon(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (k, v) in self.fields.iter() {
            out.extend_from_slice(k.as_bytes());
            out.push(0);
            out.extend_from_slice(v.as_bytes());
            out.push(b'\n');
        }
        out
    }

    /// 内容哈希。
    pub fn content_hash(&self) -> u64 {
        fnv1a(&self.canon())
    }
}

/// 签名账项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigRecord {
    /// 报告版本。
    pub version: String,
    /// 内容哈希。
    pub hash: u64,
    /// 签名时刻（tick）。
    pub at: u64,
}

/// 签名器 + 验签器。
#[derive(Debug, Default)]
pub struct SignBook {
    /// 账（一报告一签）。
    pub recs: Vec<SigRecord>,
}

impl SignBook {
    /// 签名落账。
    pub fn sign(&mut self, version: &str, body: &ReportBody, at: u64) -> Result<u64, u16> {
        if body.fields.is_empty() {
            return Err(E::EmptyBody.code());
        }
        if self.recs.iter().any(|r| r.version == version) {
            return Err(E::DupSign.code());
        }
        let h = body.content_hash();
        self.recs.push(SigRecord {
            version: String::from(version),
            hash: h,
            at,
        });
        Ok(h)
    }

    /// 验签：重算对拍。
    pub fn verify(&self, version: &str, body: &ReportBody) -> Result<(), u16> {
        let r = self
            .recs
            .iter()
            .find(|r| r.version == version)
            .ok_or(E::UnsignedVerify.code())?;
        if body.content_hash() != r.hash {
            return Err(E::TamperFound.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：哈希签名落账，篡改检出 100%）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：8 字段报告。
    let body = ReportBody {
        fields: [
            ("boot-ok", "yes"),
            ("heap", "64MiB"),
            ("gpu", "probe-done"),
            ("fs", "mounted"),
            ("krand", "seed-42"),
            ("ucode", "0xA5"),
            ("smp", "4-cpu"),
            ("tsc", "calibrated"),
        ]
        .iter()
        .map(|(k, v)| (String::from(*k), String::from(*v)))
        .collect(),
    };

    // 二、签名落账 + 验签绿。
    let mut book = SignBook::default();
    let h1 = book.sign("v1", &body, 100)?;
    book.verify("v1", &body)?;

    // 三、篡改检出 100%：逐字段单点篡改全检出（J1 主体全扫）。
    for i in 0..body.fields.len() {
        let mut forged = body.clone();
        forged.fields[i].1.push('X');
        if book.verify("v1", &forged).is_ok() {
            return Err(E::TamperFound.code());
        }
    }
    // 字段序篡改（换序 → canon 变 → 检出）。
    let mut swapped = body.clone();
    swapped.fields.swap(0, 1);
    if book.verify("v1", &swapped).is_ok() {
        return Err(E::TamperFound.code());
    }
    // 删字段篡改。
    let mut short = body.clone();
    short.fields.pop();
    if book.verify("v1", &short).is_ok() {
        return Err(E::TamperFound.code());
    }

    // 四、红线逐项：重复签名 / 空正文 / 未签名验签 / 哈希确定性。
    if book.sign("v1", &body, 200).is_ok() {
        return Err(E::DupSign.code());
    }
    let empty = ReportBody::default();
    if book.sign("v2", &empty, 0).is_ok() {
        return Err(E::EmptyBody.code());
    }
    if book.verify("v-missing", &body).is_ok() {
        return Err(E::UnsignedVerify.code());
    }
    let h2 = body.content_hash();
    if h1 != h2 {
        return Err(E::TamperFound.code());
    }

    // 五、判别式段位对拍。
    if E::TamperFound.code() != SEG_BASE + 1 || E::VersionMismatch.code() != SEG_BASE + 5 {
        return Err(E::TamperFound.code());
    }
    Ok(())
}
