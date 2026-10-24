//! ves403_s22t04t06 — VE-F4163 · T04-T06 组自查核验
//! 三组逐项核验：T04 翻译流水线复测（抽取/记忆/机翻/编辑/LQA 五段交接签名 + Critical 清零复核）；
//! T05 资产链复测（打包五检 + 热更新原子性 + 字形覆盖）；
//! T06 地区引擎复测（驻留路由白名单 + 合规库覆盖度 + 格式缓存命中）——证据引用制执行。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x07C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    SigMismatch = SEG_BASE + 0x01,   // 0x07C1
    CriticalBack = SEG_BASE + 0x02,  // 0x07C2
    FiveCheckFail = SEG_BASE + 0x03, // 0x07C3
    CoverageShort = SEG_BASE + 0x04, // 0x07C4
}

/// T04 五段流水线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransStage { Extract = 0, TmMemory = 1, MtEngine = 2, Edit = 3, Lqa = 4 }

impl TransStage {
    pub const ALL: [TransStage; 5] = [
        TransStage::Extract, TransStage::TmMemory, TransStage::MtEngine, TransStage::Edit, TransStage::Lqa];
    pub fn name(self) -> &'static str {
        match self {
            TransStage::Extract => "extract", TransStage::TmMemory => "tm", TransStage::MtEngine => "mt",
            TransStage::Edit => "edit", TransStage::Lqa => "lqa",
        }
    }
}

/// T05 五检。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiveCheck { Manifest = 0, Checksum = 1, GlyphCover = 2, RefClosure = 3, A11yAsset = 4 }

impl FiveCheck {
    pub const ALL: [FiveCheck; 5] = [
        FiveCheck::Manifest, FiveCheck::Checksum, FiveCheck::GlyphCover,
        FiveCheck::RefClosure, FiveCheck::A11yAsset];
    pub fn name(self) -> &'static str {
        match self {
            FiveCheck::Manifest => "manifest", FiveCheck::Checksum => "checksum",
            FiveCheck::GlyphCover => "glyph-cover", FiveCheck::RefClosure => "ref-closure",
            FiveCheck::A11yAsset => "a11y-asset",
        }
    }
}

/// T06 核验三项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T06Check { ResidencyRoute = 0, ComplianceCover = 1, FormatCacheHit = 2 }

impl T06Check {
    pub const ALL: [T06Check; 3] = [
        T06Check::ResidencyRoute, T06Check::ComplianceCover, T06Check::FormatCacheHit];
    pub fn name(self) -> &'static str {
        match self {
            T06Check::ResidencyRoute => "residency-route", T06Check::ComplianceCover => "compliance-cover",
            T06Check::FormatCacheHit => "format-cache-hit",
        }
    }
}

/// 三组核验矩阵。
pub struct VerifyT04T06 {
    pub t04_sigs: Vec<(TransStage, u32)>, // 五段交接签名
    pub t04_critical: u32,                // Critical 残留（须 0）
    pub t05_checks: Vec<(FiveCheck, bool)>,
    pub t05_atomic: bool,                 // 热更新原子性
    pub t06: Vec<(T06Check, bool)>,
    pub compliance_cover_bp: u32,         // 合规覆盖度（万分比）
    pub cache_hit_bp: u32,
}

impl VerifyT04T06 {
    pub fn new() -> Self {
        VerifyT04T06 {
            t04_sigs: Vec::new(), t04_critical: 0,
            t05_checks: FiveCheck::ALL.iter().map(|c| (*c, false)).collect(),
            t05_atomic: false, t06: T06Check::ALL.iter().map(|c| (*c, false)).collect(),
            compliance_cover_bp: 0, cache_hit_bp: 0,
        }
    }

    /// T04 五段签名链一致：全段同值。
    pub fn check_t04(&mut self, sigs: [u32; 5]) -> bool {
        self.t04_sigs = TransStage::ALL.iter().zip(sigs.iter()).map(|(s, v)| (*s, *v)).collect();
        sigs.iter().all(|v| *v == sigs[0])
    }
    pub fn t04_ok(&self) -> bool { self.t04_sigs.len() == 5 && self.t04_critical == 0 }

    /// T05 五检 + 原子性。
    pub fn mark_t05(&mut self, c: FiveCheck, pass: bool) {
        if let Some(x) = self.t05_checks.iter_mut().find(|x| x.0 == c) { x.1 = pass; }
    }
    pub fn t05_ok(&self) -> bool {
        self.t05_checks.iter().all(|x| x.1) && self.t05_atomic
    }
    pub fn five_missing(&self) -> Vec<&'static str> {
        self.t05_checks.iter().filter(|x| !x.1).map(|x| x.0.name()).collect()
    }

    /// T06 三项。
    pub fn mark_t06(&mut self, c: T06Check, pass: bool) {
        if let Some(x) = self.t06.iter_mut().find(|x| x.0 == c) { x.1 = pass; }
    }
    /// T06 通过：三标记过 + 覆盖度 ≥ 9500bp + 缓存命中 ≥ 9000bp。
    pub fn t06_ok(&self) -> bool {
        self.t06.iter().all(|x| x.1) && self.compliance_cover_bp >= 9500 && self.cache_hit_bp >= 9000
    }

    pub fn all_ok(&self) -> bool { self.t04_ok() && self.t05_ok() && self.t06_ok() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves403_checks() -> CheckSet {
    let mut set = CheckSet::new("ves403");
    let mut m = VerifyT04T06::new();

    // C403-01 T04 五段交接签名一致。
    chk(&mut set, "C403-01", m.check_t04([0x77AA; 5]) && m.t04_ok(), "五段签名一致");

    // C403-02 Critical 清零复核：残留即红。
    m.t04_critical = 1;
    chk(&mut set, "C403-02", !m.t04_ok(), "Critical 复发红");
    m.t04_critical = 0;

    // C403-03 五段签名失配检出。
    let mut m2 = VerifyT04T06::new();
    chk(&mut set, "C403-03", !m2.check_t04([1, 1, 2, 1, 1]), "MT 段签名漂移");

    // C403-04 T05 五检全过 + 原子性。
    for c in FiveCheck::ALL.iter() { m.mark_t05(*c, true); }
    m.t05_atomic = true;
    chk(&mut set, "C403-04", m.t05_ok(), "五检+原子性绿");

    // C403-05 缺检定位。
    let mut m3 = VerifyT04T06::new();
    m3.mark_t05(FiveCheck::Manifest, true);
    m3.mark_t05(FiveCheck::Checksum, true);
    chk(&mut set, "C403-05", m3.five_missing() == vec!["glyph-cover", "ref-closure", "a11y-asset"], "缺三检定位");

    // C403-06 原子性缺 → T05 红。
    m3.t05_checks.iter_mut().for_each(|x| x.1 = true);
    chk(&mut set, "C403-06", !m3.t05_ok(), "原子性缺红");

    // C403-07 T06 三项 + 覆盖度门槛。
    for c in T06Check::ALL.iter() { m.mark_t06(*c, true); }
    m.compliance_cover_bp = 9600;
    m.cache_hit_bp = 9100;
    chk(&mut set, "C403-07", m.t06_ok(), "T06 三项+门槛绿");

    // C403-08 覆盖度不足红。
    let mut m4 = VerifyT04T06::new();
    for c in T06Check::ALL.iter() { m4.mark_t06(*c, true); }
    m4.compliance_cover_bp = 9400;
    m4.cache_hit_bp = 9500;
    chk(&mut set, "C403-08", !m4.t06_ok(), "覆盖 9400bp 红线");

    // C403-09 缓存命中不足红。
    m4.compliance_cover_bp = 9800;
    m4.cache_hit_bp = 8900;
    chk(&mut set, "C403-09", !m4.t06_ok(), "命中 8900bp 红线");

    // C403-10 三组全过判定。
    chk(&mut set, "C403-10", m.all_ok(), "T04-T06 全绿");

    // C403-11 名单单源（5+5+3）。
    chk(&mut set, "C403-11", TransStage::ALL.len() == 5 && FiveCheck::ALL.len() == 5
        && T06Check::ALL.len() == 3, "名单长度单源");

    // C403-12 名单名核验。
    chk(&mut set, "C403-12", TransStage::Lqa.name() == "lqa"
        && FiveCheck::A11yAsset.name() == "a11y-asset"
        && T06Check::ResidencyRoute.name() == "residency-route", "名单名正确");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves403_smoke() {
        let set = run_ves403_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves403 red: {:?}", red);
    }
}
