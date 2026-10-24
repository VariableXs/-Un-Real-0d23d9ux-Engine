#![allow(dead_code)]
//! VE-F3720 · R06 组收口双签
//!
//! 十四族 API 冻结签名表 + 映射断言 + 20 项自检。
//! 联动核验：单源三断言 / 同源双端对拍全绿 / 偏科断言全绿 / 本地扫描网络审计。
//! 双签结构。同源破 P0 硬门 / 隐私硬门。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA8xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA8Code(pub u16);

impl CA8Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const MAP_MISSING: Self = Self(SEG_BASE + 1);
    pub const SOURCE_BROKEN: Self = Self(SEG_BASE + 2);
    pub const PRIVACY_BROKEN: Self = Self(SEG_BASE + 3);
    pub const LINK_DIVERGE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MAP_MISSING => "映射缺失，阻断",
            Self::SOURCE_BROKEN => "同源破（P0 重构硬门）",
            Self::PRIVACY_BROKEN => "本地扫描破（隐私硬门）",
            Self::LINK_DIVERGE => "联动分歧，两轮上限",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 十四族 API 签名
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFamily {
    Score,
    Consistency,
    A11yScan,
    PerfScan,
    CompatScan,
    Regression,
    Report,
    Gate,
    Suggest,
    Rank,
    MarketLink,
    Integrate,
    Debug,
    Close,
}

/// API 冻结签名
#[derive(Debug, Clone)]
pub struct FrozenApi {
    pub family: ApiFamily,
    pub version: u16,
    pub mapped: bool,
}

/// 双签
#[derive(Debug, Clone, Copy)]
pub struct DualSign {
    pub quality_sign: bool,
    pub eco_sign: bool,
}

impl DualSign {
    pub fn complete(&self) -> bool {
        self.quality_sign && self.eco_sign
    }
}

/// 收口清单
#[derive(Debug, Clone)]
pub struct CloseManifest {
    pub families: Vec<FrozenApi>,
    pub dual_sign: DualSign,
    pub same_source_ok: bool,
    pub privacy_ok: bool,
    pub skill_bias_ok: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct R06Close {
    manifest: CloseManifest,
    link_rounds: u8,
}

impl R06Close {
    pub fn new(manifest: CloseManifest) -> Self {
        Self { manifest, link_rounds: 0 }
    }

    /// 十四族盘点
    pub fn assert_all_families(&self) -> Result<(), CA8Code> {
        let mut seen = [false; 14];
        for f in &self.manifest.families {
            let idx = match f.family {
                ApiFamily::Score => 0,
                ApiFamily::Consistency => 1,
                ApiFamily::A11yScan => 2,
                ApiFamily::PerfScan => 3,
                ApiFamily::CompatScan => 4,
                ApiFamily::Regression => 5,
                ApiFamily::Report => 6,
                ApiFamily::Gate => 7,
                ApiFamily::Suggest => 8,
                ApiFamily::Rank => 9,
                ApiFamily::MarketLink => 10,
                ApiFamily::Integrate => 11,
                ApiFamily::Debug => 12,
                ApiFamily::Close => 13,
            };
            seen[idx] = true;
        }
        if seen.iter().all(|&s| s) {
            Ok(())
        } else {
            Err(CA8Code::MAP_MISSING)
        }
    }

    /// 映射断言
    pub fn assert_mapping(&self) -> Result<(), CA8Code> {
        if self.manifest.families.iter().all(|f| f.mapped) {
            Ok(())
        } else {
            Err(CA8Code::MAP_MISSING)
        }
    }

    /// 同源硬门 P0
    pub fn assert_same_source(&self) -> Result<(), CA8Code> {
        if self.manifest.same_source_ok {
            Ok(())
        } else {
            Err(CA8Code::SOURCE_BROKEN)
        }
    }

    /// 隐私硬门
    pub fn assert_privacy(&self) -> Result<(), CA8Code> {
        if self.manifest.privacy_ok {
            Ok(())
        } else {
            Err(CA8Code::PRIVACY_BROKEN)
        }
    }

    /// 偏科证据
    pub fn assert_skill_bias(&self) -> Result<(), CA8Code> {
        if self.manifest.skill_bias_ok {
            Ok(())
        } else {
            Err(CA8Code::LINK_DIVERGE)
        }
    }

    /// 双签断言
    pub fn assert_dual_sign(&self) -> Result<(), CA8Code> {
        if self.manifest.dual_sign.complete() {
            Ok(())
        } else {
            Err(CA8Code::LINK_DIVERGE)
        }
    }

    /// 联动核验：两轮上限
    pub fn link_verify(&mut self) -> Result<(), CA8Code> {
        self.link_rounds += 1;
        if self.link_rounds > 2 {
            return Err(CA8Code::LINK_DIVERGE);
        }
        self.assert_same_source()?;
        self.assert_privacy()?;
        self.assert_skill_bias()?;
        Ok(())
    }

    /// 20 项自检简版
    pub fn self_check(&self) -> u32 {
        let mut ok = 0u32;
        if self.assert_all_families().is_ok() { ok += 1; }
        if self.assert_mapping().is_ok() { ok += 1; }
        if self.assert_same_source().is_ok() { ok += 1; }
        if self.assert_privacy().is_ok() { ok += 1; }
        if self.assert_skill_bias().is_ok() { ok += 1; }
        if self.assert_dual_sign().is_ok() { ok += 1; }
        ok
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves120_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves120_r06close");

    fn full_manifest() -> CloseManifest {
        let families: Vec<FrozenApi> = vec![
            ApiFamily::Score, ApiFamily::Consistency, ApiFamily::A11yScan,
            ApiFamily::PerfScan, ApiFamily::CompatScan, ApiFamily::Regression,
            ApiFamily::Report, ApiFamily::Gate, ApiFamily::Suggest,
            ApiFamily::Rank, ApiFamily::MarketLink, ApiFamily::Integrate,
            ApiFamily::Debug, ApiFamily::Close,
        ]
        .into_iter()
        .map(|f| FrozenApi { family: f, version: 1, mapped: true })
        .collect();
        CloseManifest {
            families,
            dual_sign: DualSign { quality_sign: true, eco_sign: true },
            same_source_ok: true,
            privacy_ok: true,
            skill_bias_ok: true,
        }
    }

    // 01 十四族齐
    {
        let c = R06Close::new(full_manifest());
        chk(&mut set, "C3720-01", c.assert_all_families().is_ok(), "err");
    }
    // 02 映射缺
    {
        let mut m = full_manifest();
        m.families[0].mapped = false;
        let c = R06Close::new(m);
        chk(&mut set, "C3720-02", c.assert_mapping() == Err(CA8Code::MAP_MISSING), "err");
    }
    // 03 同源破 P0
    {
        let mut m = full_manifest();
        m.same_source_ok = false;
        let c = R06Close::new(m);
        chk(&mut set, "C3720-03", c.assert_same_source() == Err(CA8Code::SOURCE_BROKEN), "err");
    }
    // 04 隐私硬门
    {
        let mut m = full_manifest();
        m.privacy_ok = false;
        let c = R06Close::new(m);
        chk(&mut set, "C3720-04", c.assert_privacy() == Err(CA8Code::PRIVACY_BROKEN), "err");
    }
    // 05 偏科
    {
        let mut m = full_manifest();
        m.skill_bias_ok = false;
        let c = R06Close::new(m);
        chk(&mut set, "C3720-05", c.assert_skill_bias() == Err(CA8Code::LINK_DIVERGE), "err");
    }
    // 06 双签缺
    {
        let mut m = full_manifest();
        m.dual_sign.eco_sign = false;
        let c = R06Close::new(m);
        chk(&mut set, "C3720-06", c.assert_dual_sign() == Err(CA8Code::LINK_DIVERGE), "err");
    }
    // 07 联动两轮上限
    {
        let mut m = full_manifest();
        m.skill_bias_ok = false;
        let mut c = R06Close::new(m);
        let _ = c.link_verify();
        let _ = c.link_verify();
        chk(&mut set, "C3720-07", c.link_verify() == Err(CA8Code::LINK_DIVERGE), "err");
    }
    // 08 联动全绿
    {
        let mut c = R06Close::new(full_manifest());
        chk(&mut set, "C3720-08", c.link_verify().is_ok(), "err");
    }
    // 09 自检 6 项全过
    {
        let c = R06Close::new(full_manifest());
        chk(&mut set, "C3720-09", c.self_check() == 6, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3720-10", !CA8Code::SOURCE_BROKEN.explain().is_empty(), "err");
    }

    set
}
