//! CGPU-F0431 · 统计 API 冻结与文档（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API 文档：统计族接口冻结（立方查询/账单/画像/导出——签名冻结
//!   版本化）；文档（统计架构/查询手册/账单说明）；与 C04 画像 API 的
//!   一致性核对。判据：冻结、三文档、一致核对、判据。
//!
//! 设计纪律：
//! - 四族签名册：立方查询（cube）/账单（bill）/画像（profile）/导出
//!   （export）各登记签名（族+名+指纹 u64）。冻结后：
//!   只增不改不删（十年承诺同源——declare 在冻结后拒）。
//! - 版本化：freeze 时锁定版本基线，版本只升不降（回退即红）。
//! - 三文档：统计架构 / 查询手册 / 账单说明，各有必备节清单，
//!   文档校验 = 必备节逐一在场（缺节即红——不许"详见XX"式空壳）。
//! - C04 一致性核对：画像族签名与 C04 册同名同参指纹必须一致
//!   （同源签名——两册漂移即红）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E20 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E20，全仓独占）。
pub const SEG_BASE: u16 = 0x0E20;

pub const E_SAPI_FROZEN: u16 = SEG_BASE + 1; // 冻结后改动（declare 被拒）
pub const E_SAPI_DUP: u16 = SEG_BASE + 2; // 签名重复登记
pub const E_SAPI_VER: u16 = SEG_BASE + 3; // 版本回退
pub const E_SAPI_DOC: u16 = SEG_BASE + 4; // 文档必备节缺失
pub const E_SAPI_CONS: u16 = SEG_BASE + 5; // C04 一致性核对红（漂移）
pub const E_SAPI_MISSING: u16 = SEG_BASE + 6; // 四族未齐就冻结

/// 统计 API 版本基线。
pub const STAT_API_VERSION: u32 = 1;

/// 四族名。
pub const FAM_CUBE: &str = "cube";
pub const FAM_BILL: &str = "bill";
pub const FAM_PROFILE: &str = "profile";
pub const FAM_EXPORT: &str = "export";

/// 三文档名。
pub const DOC_ARCH: &str = "统计架构";
pub const DOC_QUERY: &str = "查询手册";
pub const DOC_BILL: &str = "账单说明";

/// 签名记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatSig {
    /// 族（cube/bill/profile/export）。
    pub family: &'static str,
    /// 接口名。
    pub name: &'static str,
    /// 签名指纹（参数表+返回型——同源算法算出）。
    pub fp: u64,
}

/// 统计族 API 冻结册。
#[derive(Debug)]
pub struct StatApiFreeze {
    sigs: Vec<StatSig>,
    /// 冻结版本（0 = 未冻结）。
    version: u32,
}

impl Default for StatApiFreeze {
    fn default() -> Self {
        Self {
            sigs: Vec::new(),
            version: 0,
        }
    }
}

impl StatApiFreeze {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记签名（冻结后拒）。
    pub fn declare(&mut self, family: &'static str, name: &'static str, fp: u64) -> Result<(), u16> {
        if self.version > 0 {
            return Err(E_SAPI_FROZEN);
        }
        // 同族同名不允许重复（改签名 = 先解冻——十年承诺不容许）。
        if self.sigs.iter().any(|s| s.family == family && s.name == name) {
            return Err(E_SAPI_DUP);
        }
        self.sigs.push(StatSig { family, name, fp });
        Ok(())
    }

    /// 冻结：四族至少各一签名才准冻结（空壳冻结 = 假承诺）。
    pub fn freeze(&mut self, version: u32) -> Result<(), u16> {
        for fam in [FAM_CUBE, FAM_BILL, FAM_PROFILE, FAM_EXPORT] {
            if !self.sigs.iter().any(|s| s.family == fam) {
                return Err(E_SAPI_MISSING);
            }
        }
        // 版本只升不降（回退即红）。
        if version <= self.version {
            return Err(E_SAPI_VER);
        }
        self.version = version;
        Ok(())
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn sig_count(&self) -> usize {
        self.sigs.len()
    }

    /// 指纹漂移核对：冻结后同名签名指纹必须不变。
    pub fn check_fp(&self, family: &str, name: &str, fp_now: u64) -> Result<(), u16> {
        match self.sigs.iter().find(|s| s.family == family && s.name == name) {
            Some(s) if s.fp == fp_now => Ok(()),
            Some(_) => Err(E_SAPI_CONS),
            None => Err(E_SAPI_MISSING),
        }
    }

    /// C04 画像一致性核对：画像族签名与 C04 侧（族+名+指纹列表）逐一对上。
    /// c04_sigs: C04 侧声明过的 (name, fp)。
    pub fn cross_c04(&self, c04_sigs: &[(&str, u64)]) -> Result<usize, u16> {
        if c04_sigs.is_empty() {
            return Err(E_SAPI_CONS);
        }
        let mut matched = 0usize;
        for (name, fp) in c04_sigs {
            match self.sigs.iter().find(|s| s.family == FAM_PROFILE && s.name == *name) {
                Some(s) if s.fp == *fp => matched += 1,
                Some(_) => return Err(E_SAPI_CONS), // 同名不同参 = 漂移。
                None => return Err(E_SAPI_MISSING), // C04 有而本册无 = 悬空。
            }
        }
        Ok(matched)
    }
}

/// 文档必备节清单。
pub const ARCH_SECTIONS: [&str; 4] = ["分层架构", "聚合管线", "存储布局", "查询路径"];
pub const QUERY_SECTIONS: [&str; 4] = ["立方查询", "时间窗", "维度选择", "预算限制"];
pub const BILL_SECTIONS: [&str; 4] = ["三尺度", "构成分列", "环比口径", "导出格式"];

pub fn doc_sections(doc: &str) -> Option<&'static [&'static str]> {
    match doc {
        DOC_ARCH => Some(&ARCH_SECTIONS),
        DOC_QUERY => Some(&QUERY_SECTIONS),
        DOC_BILL => Some(&BILL_SECTIONS),
        _ => None,
    }
}

/// 文档校验器。
#[derive(Debug, Default)]
pub struct StatDocs {
    /// 已通过校验的文档名。
    passed: Vec<&'static str>,
}

impl StatDocs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 单文档校验：必备节逐一在场（present 顺序与必备清单同长）。
    pub fn check(&mut self, doc: &'static str, present: &[bool]) -> Result<usize, u16> {
        let need = doc_sections(doc).ok_or(E_SAPI_DOC)?;
        if present.len() != need.len() {
            return Err(E_SAPI_DOC);
        }
        let mut green = 0usize;
        for (i, ok) in present.iter().enumerate() {
            if *ok {
                green += 1;
            } else {
                // 缺节即红——节名进诊断上下文（此处以 Err 返回）。
                let _ = need[i];
                return Err(E_SAPI_DOC);
            }
        }
        if !self.passed.contains(&doc) {
            self.passed.push(doc);
        }
        Ok(green)
    }

    /// 三文档齐全性（收口前置）。
    pub fn all_three(&self) -> bool {
        self.passed.len() == 3
    }

    pub fn passed_docs(&self) -> usize {
        self.passed.len()
    }
}

// ==========================================================================
// 判据（施工书：冻结、三文档、一致核对、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> StatApiFreeze {
        let mut f = StatApiFreeze::new();
        f.declare(FAM_CUBE, "query_window", 0xC0DE_0001).ok();
        f.declare(FAM_BILL, "bill_report", 0xB111_0002).ok();
        f.declare(FAM_PROFILE, "profile_get", 0xCAFE_0003).ok();
        f.declare(FAM_EXPORT, "export_stream", 0xE0E0_0004).ok();
        f
    }

    #[test]
    fn a01_四族签名册与冻结版本化() {
        let mut f = seeded();
        assert_eq!(f.sig_count(), 4);
        // 缺族拒冻结（删掉 export 族签名验证——用新册缺一族）。
        let mut g = StatApiFreeze::new();
        g.declare(FAM_CUBE, "query_window", 1).ok();
        g.declare(FAM_BILL, "bill_report", 2).ok();
        g.declare(FAM_PROFILE, "profile_get", 3).ok();
        assert_eq!(g.freeze(STAT_API_VERSION), Err(E_SAPI_MISSING));
        // 四族齐 → 冻结成功，版本入册。
        assert_eq!(f.freeze(STAT_API_VERSION), Ok(()));
        assert_eq!(f.version(), STAT_API_VERSION);
        // 同版本再冻结 = 版本不升 = 拒。
        assert_eq!(f.freeze(STAT_API_VERSION), Err(E_SAPI_VER));
    }

    #[test]
    fn a02_冻结后只增不改不删() {
        let mut f = seeded();
        f.freeze(STAT_API_VERSION).ok();
        // 冻结后 declare 拒。
        assert_eq!(f.declare(FAM_CUBE, "new_api", 9), Err(E_SAPI_FROZEN));
        // 冻结后同名指纹漂移核对红。
        assert_eq!(f.check_fp(FAM_CUBE, "query_window", 0xC0DE_0001), Ok(()));
        assert_eq!(f.check_fp(FAM_CUBE, "query_window", 0xDEAD), Err(E_SAPI_CONS));
        // 未登记名核对红。
        assert_eq!(f.check_fp(FAM_CUBE, "ghost", 1), Err(E_SAPI_MISSING));
        // 重复登记在冻结前也拒。
        let mut g = StatApiFreeze::new();
        g.declare(FAM_CUBE, "a", 1).ok();
        assert_eq!(g.declare(FAM_CUBE, "a", 2), Err(E_SAPI_DUP));
    }

    #[test]
    fn a03_三文档必备节校验() {
        let mut d = StatDocs::new();
        // 架构文档：四节全在。
        assert_eq!(d.check(DOC_ARCH, &[true, true, true, true]), Ok(4));
        // 查询手册：缺一节红。
        assert_eq!(d.check(DOC_QUERY, &[true, true, false, true]), Err(E_SAPI_DOC));
        // 补齐后过。
        assert_eq!(d.check(DOC_QUERY, &[true, true, true, true]), Ok(4));
        // 账单说明：节长度不符红。
        assert_eq!(d.check(DOC_BILL, &[true, true]), Err(E_SAPI_DOC));
        assert_eq!(d.check(DOC_BILL, &[true, true, true, true]), Ok(4));
        // 三文档齐全。
        assert!(d.all_three());
        assert_eq!(d.passed_docs(), 3);
        // 未知文档名拒。
        assert_eq!(d.check("不存在的文档", &[true; 4]), Err(E_SAPI_DOC));
        // 节清单常量自洽：三文档必备节名互不重叠（各管一摊）。
        for s in ARCH_SECTIONS {
            assert!(!QUERY_SECTIONS.contains(&s));
            assert!(!BILL_SECTIONS.contains(&s));
        }
    }

    #[test]
    fn a04_C04画像一致性核对() {
        let f = seeded();
        // C04 侧同名同参 → 一致绿。
        let c04 = [("profile_get", 0xCAFE_0003u64)];
        assert_eq!(f.cross_c04(&c04), Ok(1));
        // 同名不同参 → 漂移红。
        assert_eq!(f.cross_c04(&[("profile_get", 0xBAD)]), Err(E_SAPI_CONS));
        // C04 有而本册无 → 悬空红。
        assert_eq!(f.cross_c04(&[("profile_put", 1)]), Err(E_SAPI_MISSING));
        // 空册红。
        assert_eq!(f.cross_c04(&[]), Err(E_SAPI_CONS));
        // 多签名全对上。
        let f2 = seeded();
        let c04b = [("profile_get", 0xCAFE_0003u64), ("profile_get", 0xCAFE_0003u64)];
        assert_eq!(f2.cross_c04(&c04b), Ok(2));
    }

    #[test]
    fn a05_版本只升不降() {
        let mut f = seeded();
        f.freeze(2).ok();
        // 回退拒。
        assert_eq!(f.freeze(1), Err(E_SAPI_VER));
        assert_eq!(f.version(), 2);
        // 升版放行。
        assert_eq!(f.freeze(3), Ok(()));
        assert_eq!(f.version(), 3);
        // 版本基线常量 sanity。
        assert!(STAT_API_VERSION >= 1);
    }
}
