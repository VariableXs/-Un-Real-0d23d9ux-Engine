//! CGPU-F1718 · 第三方引擎框架接入
//!
//! 判据锚点（施工书原文摘录）：
//! 接入：接入层（第三方引擎（游戏引擎/推理框架接入异构——接入接口（K01 API
//! 之上封装——封装层；适配矩阵（ONNX Runtime/DirectML/主流引擎插件——适配矩阵
//! （适配深度分级（完整/基础/预留——分级表；兼容（版本兼容（框架版本跟随策略
//! ——跟随策略（兼容承诺边界显性——边界声明；示例（每适配一个最小示例——示例
//! 对齐；治理（第三方适配审核（复用 F3109 模式——治理复用；测试（封装/矩阵/
//! 跟随/示例/治理五组）。判据：封装层、适配矩阵、跟随边界、示例、治理、五组、判据。
//!
//! 诊断码段 0xA101~0xA105（独占）：
//!   0xA101 封装层失格（接入接口未声明在 K01 之上/封装自建第二真相）
//!   0xA102 适配矩阵越表（框架不在册/深度越三级）
//!   0xA103 跟随边界失信（承诺边界未声明/版本越跟随窗）
//!   0xA104 示例失配（适配无最小示例/示例与适配名不对齐）
//!   0xA105 段越界（判码落 0xA100..0xA200 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const THIRD_FRAMEWORK_VERSION: &str = "K37-3pframework-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA101~0xA105）
// ---------------------------------------------------------------------------

pub const XC_WRAP_BROKEN: u32 = 0xA101;
pub const XC_MATRIX_OOB: u32 = 0xA102;
pub const XC_FOLLOW_BROKEN: u32 = 0xA103;
pub const XC_SAMPLE_MISMATCH: u32 = 0xA104;
pub const XC_SEG_OOR: u32 = 0xA105;

// ---------------------------------------------------------------------------
// 一、封装层（K01 API 之上）
// ---------------------------------------------------------------------------

/// 封装声明：接入接口必须声明为 K01 API 之上薄封装（不自建第二套语义——
/// vck07 同步适配层同纪律）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WrapDecl {
    /// 封装声明在册。
    pub declared: bool,
    /// 底座源单（K01 API）。
    pub base: &'static str,
    /// 薄封装承诺（只做名字翻译/参数搬运，不加语义）。
    pub thin_only: bool,
}

impl WrapDecl {
    pub const fn build() -> Self {
        WrapDecl { declared: true, base: "CGPU-K01-API", thin_only: true }
    }

    pub fn verified(&self) -> Result<(), u32> {
        if !self.declared || self.base.is_empty() || !self.thin_only {
            return Err(XC_WRAP_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、适配矩阵（深度三级）
// ---------------------------------------------------------------------------

/// 适配深度三级闭集。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AdaptDepth {
    /// 完整：全点表接通+认证过。
    Full,
    /// 基础：核心点接通。
    Basic,
    /// 预留：接口位声明未实现。
    Reserved,
}

/// 适配行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdaptRow {
    /// 框架名。
    pub framework: &'static str,
    pub depth: AdaptDepth,
}

/// 适配矩阵（判据点名三框架）。
pub struct AdaptMatrix {
    rows: [AdaptRow; 3],
}

impl AdaptMatrix {
    pub const fn build() -> Self {
        AdaptMatrix {
            rows: [
                AdaptRow { framework: "onnx-runtime", depth: AdaptDepth::Basic },
                AdaptRow { framework: "directml", depth: AdaptDepth::Reserved },
                AdaptRow { framework: "engine-plugin", depth: AdaptDepth::Reserved },
            ],
        }
    }

    /// 矩阵闭合：三框架在册 + 深度合法（枚举闭集）+ 名非空（0xA102）。
    pub fn closed(&self) -> Result<(), u32> {
        for r in &self.rows {
            if r.framework.is_empty() {
                return Err(XC_MATRIX_OOB);
            }
        }
        Ok(())
    }

    /// 查行。
    pub fn row(&self, idx: usize) -> Option<&AdaptRow> {
        self.rows.get(idx)
    }
}

// ---------------------------------------------------------------------------
// 三、版本跟随策略与兼容边界
// ---------------------------------------------------------------------------

/// 跟随策略：框架版本跟随窗（跟随 N-1 代——不追最新不弃旧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FollowPolicy {
    /// 跟随窗宽（代数）。
    pub window_gens: u8,
    /// 边界声明在册（兼容承诺边界显性）。
    pub boundary_declared: bool,
    /// 不承诺项（诚实清单）。
    pub not_promised: &'static str,
}

impl FollowPolicy {
    pub const fn build() -> Self {
        FollowPolicy {
            window_gens: 2,
            boundary_declared: true,
            not_promised: "nightly/pre-release 不承诺",
        }
    }

    /// 跟随校验：窗宽 1..=3 + 边界声明 + 不承诺清单非空（0xA103）。
    pub fn verified(&self) -> Result<(), u32> {
        if self.window_gens == 0 || self.window_gens > 3 {
            return Err(XC_FOLLOW_BROKEN);
        }
        if !self.boundary_declared || self.not_promised.is_empty() {
            return Err(XC_FOLLOW_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、示例对齐 + 治理复用
// ---------------------------------------------------------------------------

/// 示例行：每适配一个最小示例（名对齐框架）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRow {
    pub framework: &'static str,
    pub sample: &'static str,
}

/// 治理审核（F3109 模式复用）。
pub struct GovReview {
    /// 适配名。
    pub framework: &'static str,
    /// 审核过。
    pub reviewed: bool,
    /// 复用源单。
    pub origin: &'static str,
}

/// 治理账。
pub struct GovBook;

impl GovBook {
    /// 示例对齐：每个非预留适配必须有同名示例（0xA104）。
    pub fn samples_aligned(matrix: &AdaptMatrix, samples: &[SampleRow]) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            let r = match matrix.row(i) {
                Some(r) => r,
                None => return Err(XC_SAMPLE_MISMATCH),
            };
            // 预留深度不要求示例。
            if r.depth != AdaptDepth::Reserved {
                let mut found = false;
                for s in samples {
                    if s.framework == r.framework && !s.sample.is_empty() {
                        found = true;
                    }
                }
                if !found {
                    return Err(XC_SAMPLE_MISMATCH);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 治理闭合：审核过 + 复用源单非空。
    pub fn reviewed(g: &GovReview) -> Result<(), u32> {
        if !g.reviewed || g.origin.is_empty() {
            return Err(XC_WRAP_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组）
// ---------------------------------------------------------------------------

/// 第三方接入引擎。
pub struct ThirdPartyEngine;

impl ThirdPartyEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：封装层。
        WrapDecl::build().verified()?;
        // 组二：适配矩阵。
        let m = AdaptMatrix::build();
        m.closed()?;
        // 组三：跟随边界。
        FollowPolicy::build().verified()?;
        let mut bad = FollowPolicy::build();
        bad.window_gens = 0;
        if bad.verified().is_ok() {
            return Err(XC_FOLLOW_BROKEN);
        }
        // 组四：示例对齐——ONNX（Basic）必须带示例。
        let samples = [SampleRow { framework: "onnx-runtime", sample: "min-onnx-infer" }];
        GovBook::samples_aligned(&m, &samples)?;
        // 缺示例反例。
        if GovBook::samples_aligned(&m, &[]).is_ok() {
            return Err(XC_SAMPLE_MISMATCH);
        }
        // 组五：治理。
        let g = GovReview { framework: "onnx-runtime", reviewed: true, origin: "CGPU-F3109" };
        GovBook::reviewed(&g)?;
        Ok(())
    }

    /// 段对账：五码全落 0xA100..0xA200 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_WRAP_BROKEN,
            XC_MATRIX_OOB,
            XC_FOLLOW_BROKEN,
            XC_SAMPLE_MISMATCH,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA100 || codes[i] >= 0xA200 {
                return Err(XC_WRAP_BROKEN);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_WRAP_BROKEN);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
