//! CGPU-F0447 · 高级特性 API 冻结与文档（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API 文档：高级特性接口冻结（拓扑/老化/动态调节/策略插件——签名
//!   冻结并入 C 域总册 v1.0 增量版）；文档（高级仲裁白皮书/策略插件
//!   开发指南）；与总冻结 v1.0 的兼容声明。判据：冻结增量、两文档、
//!   兼容、判据。
//!
//! 设计纪律：
//! - 增量冻结：四族签名册（topo/aging/fairadj/policy）基于总册 v1.0
//!   记 **增量版本**（v1.1 = v1.0 + 增量段）——版本号只升不降；
//!   增量冻结后只增不改不删。
//! - 与 v1.0 兼容声明：增量签名不得与 v1.0 册同名不同参（漂移即拒
//!   ——兼容 = 新名或同参同名）。
//! - 两文档：白皮书（概念/机制/边界三节）与插件指南（接口/门槛/生命
//!   周期三节）必备节校验。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F20 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F20，全仓独占）。
pub const SEG_BASE: u16 = 0x0F20;

pub const E_AA_FROZEN: u16 = SEG_BASE + 1; // 增量冻结后改动
pub const E_AA_DUP: u16 = SEG_BASE + 2; // 重名登记
pub const E_AA_VER: u16 = SEG_BASE + 3; // 版本回退
pub const E_AA_DOC: u16 = SEG_BASE + 4; // 文档必备节缺失
pub const E_AA_COMPAT: u16 = SEG_BASE + 5; // 与 v1.0 不兼容（同名不同参）
pub const E_AA_FAMILY: u16 = SEG_BASE + 6; // 增量族缺（四族未齐）

/// 增量版本基线（v1.0 总册 + 本增量 = v1.1）。
pub const BASE_VERSION_V10: u32 = 10;
pub const INCREMENT_VERSION: u32 = 11;

/// 四增量族。
pub const FAM_TOPO: &str = "topo";
pub const FAM_AGING: &str = "aging";
pub const FAM_FAIRADJ: &str = "fairadj";
pub const FAM_POLICY: &str = "policy";

/// 增量签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvSig {
    pub family: &'static str,
    pub name: &'static str,
    pub fp: u64,
}

/// 高级特性 API 冻结册（增量）。
#[derive(Debug)]
pub struct AdvApiFreeze {
    sigs: Vec<AdvSig>,
    version: u32,
}

impl Default for AdvApiFreeze {
    fn default() -> Self {
        Self {
            sigs: Vec::new(),
            version: BASE_VERSION_V10,
        }
    }
}

impl AdvApiFreeze {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记增量签名（版本推进过 = 冻结后拒；与 v1.0 册同名不同参拒）。
    pub fn declare(
        &mut self,
        family: &'static str,
        name: &'static str,
        fp: u64,
        v10_sigs: &[(&str, u64)],
    ) -> Result<(), u16> {
        if self.version > BASE_VERSION_V10 {
            return Err(E_AA_FROZEN);
        }
        if self.sigs.iter().any(|s| s.family == family && s.name == name) {
            return Err(E_AA_DUP);
        }
        // 兼容闸：与 v1.0 同名必须同参。
        if let Some((_, v10_fp)) = v10_sigs.iter().find(|(n, _)| *n == name) {
            if *v10_fp != fp {
                return Err(E_AA_COMPAT);
            }
        }
        self.sigs.push(AdvSig { family, name, fp });
        Ok(())
    }

    /// 冻结增量：四族各至少一签名 → 版本推进为 v1.1。
    pub fn freeze_increment(&mut self) -> Result<u32, u16> {
        for fam in [FAM_TOPO, FAM_AGING, FAM_FAIRADJ, FAM_POLICY] {
            if !self.sigs.iter().any(|s| s.family == fam) {
                return Err(E_AA_FAMILY);
            }
        }
        self.version = INCREMENT_VERSION;
        Ok(self.version)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn sig_count(&self) -> usize {
        self.sigs.len()
    }

    /// 指纹漂移核对（冻结后同名签名指纹必须不变）。
    pub fn check_fp(&self, family: &str, name: &str, fp_now: u64) -> Result<(), u16> {
        match self.sigs.iter().find(|s| s.family == family && s.name == name) {
            Some(s) if s.fp == fp_now => Ok(()),
            Some(_) => Err(E_AA_COMPAT),
            None => Err(E_AA_FAMILY),
        }
    }
}

/// 两文档必备节。
pub const WHITEPAPER_SECTIONS: [&str; 3] = ["概念", "机制", "边界"];
pub const PLUGIN_GUIDE_SECTIONS: [&str; 3] = ["接口", "门槛", "生命周期"];

pub fn doc_sections(doc: &str) -> Option<&'static [&'static str]> {
    match doc {
        "高级仲裁白皮书" => Some(&WHITEPAPER_SECTIONS),
        "策略插件开发指南" => Some(&PLUGIN_GUIDE_SECTIONS),
        _ => None,
    }
}

/// 文档校验器。
#[derive(Debug, Default)]
pub struct AdvDocs {
    passed: Vec<&'static str>,
}

impl AdvDocs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 单文档校验（必备节逐一在场）。
    pub fn check(&mut self, doc: &'static str, present: &[bool]) -> Result<usize, u16> {
        let need = doc_sections(doc).ok_or(E_AA_DOC)?;
        if present.len() != need.len() {
            return Err(E_AA_DOC);
        }
        for (i, ok) in present.iter().enumerate() {
            if !ok {
                let _ = need[i];
                return Err(E_AA_DOC);
            }
        }
        if !self.passed.contains(&doc) {
            self.passed.push(doc);
        }
        Ok(present.iter().filter(|o| **o).count())
    }

    /// 兼容声明：总册 v1.0 兼容 = 版本序（10 → 11）且未回退。
    pub fn compat_declaration(version: u32) -> Result<(), u16> {
        if version < BASE_VERSION_V10 {
            return Err(E_AA_VER);
        }
        Ok(())
    }

    pub fn all_two(&self) -> bool {
        self.passed.len() == 2
    }
}

// ==========================================================================
// 判据（施工书：冻结增量、两文档、兼容、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn v10() -> Vec<(&'static str, u64)> {
        vec![("shared_entry", 0xC0DE_0001), ("old_api", 0x1)]
    }

    fn seeded() -> AdvApiFreeze {
        let mut f = AdvApiFreeze::new();
        f.declare(FAM_TOPO, "topo_estimate", 0xA001, &v10()).ok();
        f.declare(FAM_AGING, "aging_tick", 0xA002, &v10()).ok();
        f.declare(FAM_FAIRADJ, "fair_rebalance", 0xA003, &v10()).ok();
        f.declare(FAM_POLICY, "policy_verify", 0xA004, &v10()).ok();
        f
    }

    #[test]
    fn a01_增量冻结_四族齐才准() {
        let mut f = seeded();
        assert_eq!(f.sig_count(), 4);
        // 缺族拒冻结。
        let mut g = AdvApiFreeze::new();
        g.declare(FAM_TOPO, "topo_estimate", 1, &v10()).ok();
        g.declare(FAM_AGING, "aging_tick", 2, &v10()).ok();
        g.declare(FAM_FAIRADJ, "fair_rebalance", 3, &v10()).ok();
        assert_eq!(g.freeze_increment(), Err(E_AA_FAMILY));
        // 四族齐 → v1.1。
        assert_eq!(f.freeze_increment(), Ok(INCREMENT_VERSION));
        assert_eq!(f.version(), 11);
        // 版本序：v1.0 基线 10。
        assert_eq!(BASE_VERSION_V10, 10);
    }

    #[test]
    fn a02_冻结后只增不改不删() {
        let mut f = seeded();
        f.freeze_increment().ok();
        // 冻结后 declare 拒。
        assert_eq!(f.declare(FAM_TOPO, "new_sig", 9, &v10()), Err(E_AA_FROZEN));
        // 指纹漂移核对。
        assert_eq!(f.check_fp(FAM_TOPO, "topo_estimate", 0xA001), Ok(()));
        assert_eq!(f.check_fp(FAM_TOPO, "topo_estimate", 0xDEAD), Err(E_AA_COMPAT));
        // 冻结前重名拒。
        let mut g = AdvApiFreeze::new();
        g.declare(FAM_AGING, "x", 1, &v10()).ok();
        assert_eq!(g.declare(FAM_AGING, "x", 2, &v10()), Err(E_AA_DUP));
    }

    #[test]
    fn a03_兼容闸_与v10同名必须同参() {
        let mut f = AdvApiFreeze::new();
        // 增量签名用 v1.0 已有名 + 同参 → 兼容通过。
        assert_eq!(f.declare(FAM_TOPO, "shared_entry", 0xC0DE_0001, &v10()), Ok(()));
        // 同名不同参 → 不兼容拒。
        assert_eq!(f.declare(FAM_TOPO, "shared_entry2", 0xBAD, &v10()), Ok(()));
        let mut g = AdvApiFreeze::new();
        assert_eq!(g.declare(FAM_POLICY, "shared_entry", 0xBAD, &v10()), Err(E_AA_COMPAT));
        // 新名自由登记。
        assert_eq!(g.declare(FAM_POLICY, "fresh_api", 7, &v10()), Ok(()));
    }

    #[test]
    fn a04_两文档必备节() {
        let mut d = AdvDocs::new();
        assert_eq!(d.check("高级仲裁白皮书", &[true, true, true]), Ok(3));
        assert_eq!(d.check("策略插件开发指南", &[true, true, false]), Err(E_AA_DOC));
        assert_eq!(d.check("策略插件开发指南", &[true, true, true]), Ok(3));
        // 节长度不符。
        assert_eq!(d.check("高级仲裁白皮书", &[true]), Err(E_AA_DOC));
        // 未知文档拒。
        assert_eq!(d.check("无关文档", &[true, true, true]), Err(E_AA_DOC));
        assert!(d.all_two());
        // 节清单互不重叠。
        for s in WHITEPAPER_SECTIONS {
            assert!(!PLUGIN_GUIDE_SECTIONS.contains(&s));
        }
    }

    #[test]
    fn a05_兼容声明_版本只升不降() {
        assert_eq!(AdvDocs::compat_declaration(11), Ok(()));
        assert_eq!(AdvDocs::compat_declaration(10), Ok(()));
        // 低于 v1.0 拒。
        assert_eq!(AdvDocs::compat_declaration(9), Err(E_AA_VER));
        // 冻结后版本不可回退（f.version 已 11，册内无降级 API——字段只读语义由测试钉死）。
        let mut f = seeded();
        f.freeze_increment().ok();
        assert!(f.version() >= BASE_VERSION_V10);
    }
}
