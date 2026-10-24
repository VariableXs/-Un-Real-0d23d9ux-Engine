//! VE-F3715 · 质量数据隐私
//!
//! 扫描数据隐私：内容分析不出本地（本地红线 P0）。
//! 云端扫描必须显式 opt-in + 本地优先。IP 泄露 P0。质量数据与身份关联脱敏。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA3xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA3Code(pub u16);

impl CA3Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const CLOUD_DEFAULT: Self = Self(SEG_BASE + 1);
    pub const OPTIN_FORCED: Self = Self(SEG_BASE + 2);
    pub const IP_LEAK: Self = Self(SEG_BASE + 3);
    pub const AGG_FAKE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CLOUD_DEFAULT => "云端默认扫描（P0 红线）：内容分析未留在本地",
            Self::OPTIN_FORCED => "强制 opt-in（P1 红线）：无 opt-in 不可发布检查",
            Self::IP_LEAK => "IP 泄露（P0 红线）",
            Self::AGG_FAKE => "聚合数据虚报，需透明修正",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 扫描模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Local,
    CloudOptIn,
}

/// 扫描配置
#[derive(Debug, Clone)]
pub struct PrivacyConfig {
    pub mode: ScanMode,
    pub opt_in_granted: bool,
    pub anonymize: bool,
}

impl PrivacyConfig {
    pub fn local_default() -> Self {
        Self { mode: ScanMode::Local, opt_in_granted: false, anonymize: true }
    }

    /// 本地优先断言 P0
    pub fn assert_local_first(&self) -> Result<(), CA3Code> {
        if self.mode == ScanMode::CloudOptIn && !self.opt_in_granted {
            Err(CA3Code::CLOUD_DEFAULT)
        } else {
            Ok(())
        }
    }

    /// opt-in 不可强迫 P1
    pub fn assert_optin_voluntary(&self) -> Result<(), CA3Code> {
        if self.mode == ScanMode::CloudOptIn && !self.opt_in_granted {
            Err(CA3Code::OPTIN_FORCED)
        } else {
            Ok(())
        }
    }
}

/// 质量数据记录（脱敏后）
#[derive(Debug, Clone)]
pub struct QualityRecord {
    pub asset_id: u32,
    pub creator_hash: u64,
    pub score: u16,
    pub raw_identity: Option<u32>,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualPrivacy {
    config: PrivacyConfig,
    records: Vec<QualityRecord>,
    network_calls: u32,
}

impl QualPrivacy {
    pub fn new(config: PrivacyConfig) -> Self {
        Self { config, records: Vec::new(), network_calls: 0 }
    }

    /// 本地扫描：不产生网络调用
    pub fn local_scan(&mut self, rec: QualityRecord) -> Result<(), CA3Code> {
        self.config.assert_local_first()?;
        self.records.push(rec);
        Ok(())
    }

    /// 云端扫描：需 opt-in
    pub fn cloud_scan(&mut self, rec: QualityRecord) -> Result<(), CA3Code> {
        self.config.assert_optin_voluntary()?;
        self.network_calls += 1;
        self.records.push(rec);
        Ok(())
    }

    /// IP 泄露断言 P0：local 模式下 network_calls 必须为 0
    pub fn assert_no_ip_leak(&self) -> Result<(), CA3Code> {
        if self.config.mode == ScanMode::Local && self.network_calls > 0 {
            Err(CA3Code::IP_LEAK)
        } else {
            Ok(())
        }
    }

    /// 脱敏断言：记录中 raw_identity 必须为 None
    pub fn assert_anonymized(&self) -> Result<(), CA3Code> {
        if self.config.anonymize && self.records.iter().any(|r| r.raw_identity.is_some()) {
            Err(CA3Code::IP_LEAK)
        } else {
            Ok(())
        }
    }

    /// 聚合公开
    pub fn aggregate(&self) -> (usize, u16) {
        let n = self.records.len();
        let avg = if n == 0 {
            0
        } else {
            (self.records.iter().map(|r| r.score as u32).sum::<u32>() / n as u32) as u16
        };
        (n, avg)
    }

    pub fn network_call_count(&self) -> u32 {
        self.network_calls
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves115_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves115_qualprivacy");

    // 01 云端默认 P0
    {
        let cfg = PrivacyConfig { mode: ScanMode::CloudOptIn, opt_in_granted: false, anonymize: true };
        let mut p = QualPrivacy::new(cfg);
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: None };
        chk(&mut set, "C3715-01", p.local_scan(rec) == Err(CA3Code::CLOUD_DEFAULT), "err");
    }
    // 02 本地正常
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: None };
        chk(&mut set, "C3715-02", p.local_scan(rec).is_ok(), "err");
    }
    // 03 opt-in 强迫 P1
    {
        let cfg = PrivacyConfig { mode: ScanMode::CloudOptIn, opt_in_granted: false, anonymize: true };
        let mut p = QualPrivacy::new(cfg);
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: None };
        chk(&mut set, "C3715-03", p.cloud_scan(rec) == Err(CA3Code::OPTIN_FORCED), "err");
    }
    // 04 opt-in 后云端可用
    {
        let cfg = PrivacyConfig { mode: ScanMode::CloudOptIn, opt_in_granted: true, anonymize: true };
        let mut p = QualPrivacy::new(cfg);
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: None };
        chk(&mut set, "C3715-04", p.cloud_scan(rec).is_ok(), "err");
    }
    // 05 IP 泄露 P0
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        // simulate forced network
        p.network_calls = 1;
        chk(&mut set, "C3715-05", p.assert_no_ip_leak() == Err(CA3Code::IP_LEAK), "err");
    }
    // 06 本地无网络
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: None };
        p.local_scan(rec).unwrap();
        chk(&mut set, "C3715-06", p.assert_no_ip_leak().is_ok() && p.network_call_count() == 0, "err");
    }
    // 07 脱敏断言
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        let rec = QualityRecord { asset_id: 1, creator_hash: 0, score: 80, raw_identity: Some(42) };
        p.records.push(rec);
        chk(&mut set, "C3715-07", p.assert_anonymized() == Err(CA3Code::IP_LEAK), "err");
    }
    // 08 脱敏正常
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        let rec = QualityRecord { asset_id: 1, creator_hash: 99, score: 80, raw_identity: None };
        p.local_scan(rec).unwrap();
        chk(&mut set, "C3715-08", p.assert_anonymized().is_ok(), "err");
    }
    // 09 聚合
    {
        let mut p = QualPrivacy::new(PrivacyConfig::local_default());
        p.local_scan(QualityRecord { asset_id: 1, creator_hash: 0, score: 70, raw_identity: None }).unwrap();
        p.local_scan(QualityRecord { asset_id: 2, creator_hash: 0, score: 90, raw_identity: None }).unwrap();
        let (n, avg) = p.aggregate();
        chk(&mut set, "C3715-09", n == 2 && avg == 80, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3715-10", !CA3Code::CLOUD_DEFAULT.explain().is_empty(), "err");
    }

    set
}
