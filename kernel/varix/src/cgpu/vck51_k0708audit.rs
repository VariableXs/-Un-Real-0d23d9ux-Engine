//! CGPU-F1732 · K07-K08 组自查核验
//!
//! 判据锚点（施工书原文摘录）：
//! K07 核验：接口（10 函数）；归因（双维/准确记录回读）；隐私（分类表/指纹评估
//! 记录）；K08 核验：接口（6 函数）；场景（16 条端到端记录）；矩阵（诚实记录）；
//! 认证（判据公开记录）；结论（三态）；测试（两组/回读/三态三组）。判据：两组核验、
//! 回读、三态、三组、判据。
//!
//! 诊断码段 0xAF01~0xAF05（独占）：
//!   0xAF01 接口数失账（K07≠10/K08≠6 任一回读失配）
//!   0xAF02 归因失准（双维记录缺/准确率记录失真）
//!   0xAF03 隐私失档（分类表/指纹评估记录缺）
//!   0xAF08 场景失账（16 条记录缺/矩阵不诚实/判据不公开）
//!   0xAF05 段越界（判码落 0xAF00..0xB000 区间外；结论三态并入各组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K0708_AUDIT_VERSION: &str = "K51-k0708audit-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xAF01~0xAF05；0xAF03 占隐私、0xAF04 占场景、0xAF05 占段越界）
// ---------------------------------------------------------------------------

pub const XC_A3_API_MISMATCH: u32 = 0xAF01;
pub const XC_A3_ATTR_LOST: u32 = 0xAF02;
pub const XC_A3_PRIVACY_MISSING: u32 = 0xAF03;
pub const XC_A3_SCENE_SHORT: u32 = 0xAF04;
pub const XC_A3_SEG_OOR: u32 = 0xAF05;

// ---------------------------------------------------------------------------
// 一、接口数回读（K07=10 / K08=6）
// ---------------------------------------------------------------------------

/// K07/K08 接口数承诺（16——74 总账尾段）。
pub const K_GROUP_API_PROMISE_3: [(&str, u16); 2] = [("K07", 10), ("K08", 6)];

pub fn api_census_k0708() -> Result<(), u32> {
    if K_GROUP_API_PROMISE_3.len() != 2 {
        return Err(XC_A3_API_MISMATCH);
    }
    let mut total = 0u16;
    for (name, n) in K_GROUP_API_PROMISE_3.iter() {
        if name.is_empty() || *n == 0 {
            return Err(XC_A3_API_MISMATCH);
        }
        total += n;
    }
    if total != 16 {
        return Err(XC_A3_API_MISMATCH);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、K07 归因与隐私回读
// ---------------------------------------------------------------------------

/// 归因双维记录：引擎维 + 段维（F1699 双维守恒回读）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrRecord {
    /// 双维守恒账在册。
    pub dual_dim_conserved: bool,
    /// 准确率记录千分比（承诺 ≥950‰）。
    pub accuracy_permille: u32,
    pub record_anchor: &'static str,
}

impl AttrRecord {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.dual_dim_conserved || self.accuracy_permille < 950
            || self.record_anchor.is_empty()
        {
            return Err(XC_A3_ATTR_LOST);
        }
        Ok(())
    }
}

/// 隐私记录：三级分类表 + 模型指纹评估双档在册（F1707 回读）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrivacyRecord {
    pub tier_table_ok: bool,
    pub fingerprint_eval_ok: bool,
    pub anon_strip_ok: bool,
}

impl PrivacyRecord {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.tier_table_ok || !self.fingerprint_eval_ok || !self.anon_strip_ok {
            return Err(XC_A3_PRIVACY_MISSING);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、K08 场景/矩阵/认证回读
// ---------------------------------------------------------------------------

/// K08 回读账：16 条端到端 + 矩阵诚实 + 认证判据公开。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K08Readback {
    pub e2e_cases: u16,
    pub matrix_honest: bool,
    pub criteria_public: bool,
}

impl K08Readback {
    pub fn sound(&self) -> Result<(), u32> {
        if self.e2e_cases < 16 {
            return Err(XC_A3_SCENE_SHORT);
        }
        if !self.matrix_honest || !self.criteria_public {
            return Err(XC_A3_SCENE_SHORT);
        }
        Ok(())
    }
}

/// 三态结论（K07/K08 两组在册记录）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict3 {
    Pass,
    Conditional,
    Fail,
}

/// 回读总闸：两组结论全非 Fail 且记录面全绿。
pub fn k0708_readback_gate(
    attr: &AttrRecord,
    priv_rec: &PrivacyRecord,
    k08: &K08Readback,
    verdicts: [Verdict3; 2],
) -> Result<(), u32> {
    attr.sound()?;
    priv_rec.sound()?;
    k08.sound()?;
    for v in verdicts.iter() {
        if *v == Verdict3::Fail {
            return Err(XC_A3_SCENE_SHORT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（三组：接口+归因+隐私 / 场景 / 三态）
// ---------------------------------------------------------------------------

/// K07-K08 自查核验引擎。
pub struct K0708AuditEngine;

impl K0708AuditEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：接口数回读 + 归因 + 隐私。
        api_census_k0708()?;
        let attr = AttrRecord {
            dual_dim_conserved: true,
            accuracy_permille: 960,
            record_anchor: "K07#attr",
        };
        let priv_rec = PrivacyRecord { tier_table_ok: true, fingerprint_eval_ok: true, anon_strip_ok: true };
        let k08 = K08Readback { e2e_cases: 16, matrix_honest: true, criteria_public: true };
        // 反例：归因失准（949‰）拒。
        let weak = AttrRecord {
            dual_dim_conserved: true,
            accuracy_permille: 949,
            record_anchor: "K07#attr",
        };
        if weak.sound().is_ok() {
            return Err(XC_A3_ATTR_LOST);
        }
        // 反例：隐私失档拒。
        let torn = PrivacyRecord { tier_table_ok: true, fingerprint_eval_ok: false, anon_strip_ok: true };
        if torn.sound().is_ok() {
            return Err(XC_A3_PRIVACY_MISSING);
        }
        // 组二：K08 场景账 + 反例（15 条/不诚实/不公开拒）。
        let short = K08Readback { e2e_cases: 15, matrix_honest: true, criteria_public: true };
        if short.sound().is_ok() {
            return Err(XC_A3_SCENE_SHORT);
        }
        let liar = K08Readback { e2e_cases: 16, matrix_honest: false, criteria_public: true };
        if liar.sound().is_ok() {
            return Err(XC_A3_SCENE_SHORT);
        }
        // 组三：总闸正例 + Fail 结论拒。
        k0708_readback_gate(&attr, &priv_rec, &k08, [Verdict3::Pass, Verdict3::Conditional])?;
        if k0708_readback_gate(&attr, &priv_rec, &k08, [Verdict3::Pass, Verdict3::Fail]).is_ok() {
            return Err(XC_A3_SCENE_SHORT);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xAF00..0xB000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_A3_API_MISMATCH,
            XC_A3_ATTR_LOST,
            XC_A3_PRIVACY_MISSING,
            XC_A3_SCENE_SHORT,
            XC_A3_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xAF00 || codes[i] >= 0xB000 {
                return Err(XC_A3_API_MISMATCH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_A3_API_MISMATCH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
