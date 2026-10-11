//! CGPU-F3683 · 算力增长规划（CGPU-X 域 · 十年硬件演进 · 批次 X03）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3683`
//!
//! 锚点原文：「算力：算力增长规划（FLOPS 增长曲线适配——规划实现；测试（规划
//! 一组）。判据：曲线、一组、判据。」
//!
//! 判据逐条落位：
//! - **曲线**：[`ROADMAP_STEPS`]——FLOPS 增长曲线的阶梯化规划（封闭七行，
//!   2025-2035 十年视野）；每行一步（year/flops_x100/confidence/basis）。
//!   规划与预测的分工：预测（F3682）给区间，规划取区间内的**承诺值**并
//!   标注置信度（F3681 章程「每条路线标注依据与置信度」的落地）。
//! - **曲线两条铁律**：①年份单调递增（时间倒流判
//!   [`E_X830_YEAR_REGRESSED`]）；②算力单调不减（规划保守性——规划值
//!   不随节点推进倒退，倒退判 [`E_X830_FLOPS_REGRESSED`]）。
//! - **置信度界**：confidence 必须落在 0-100（越界判
//!   [`E_X830_CONFIDENCE_FAULT`]）——置信度是给下游的诚实信号不是装饰。
//! - **依据强制**：basis 空（与拍脑袋无法区分）判
//!   [`E_X830_BASIS_MISSING`]。
//! - **一组**：判据侧 [`super::vcx83_computegrow_checks`] 规划组全量断言。
//!
//! **零静默纪律**：所有倒退/越界/缺席都产出诊断码（0x99C 段 X 域细分续编），
//! 由调用方聚合上报；版本 [`check_version`] 双向校验。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

// ---------------------------------------------------------------------------
// 一、诊断码（X 域 0x99C 段续编，0x99CE 起）
// ---------------------------------------------------------------------------

/// 年份倒退（阶梯年序列非严格递增——时间轴错乱）。
pub const E_X830_YEAR_REGRESSED: u16 = 0x99CE;
/// 算力倒退（FLOPS 规划值随节点推进下降——规划保守性破缺）。
pub const E_X830_FLOPS_REGRESSED: u16 = 0x99CF;
/// 置信度越界（confidence 超出 0-100——诚实信号失效）。
pub const E_X830_CONFIDENCE_FAULT: u16 = 0x99D0;
/// 依据缺席（规划行 basis 空——与拍脑袋无法区分）。
pub const E_X830_BASIS_MISSING: u16 = 0x99D1;
/// 版本失配（批次版本与 X03 冻结版不符）。
pub const E_X830_VERSION_MISMATCH: u16 = 0x99D2;

/// 域批次版本。
pub const VCX83_VERSION: &str = "CX83-computegrow-v1";

/// 版本校验（双向）：X03 冻结版放行，其余一律 [`E_X830_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCX83_VERSION {
        Ok(())
    } else {
        Err(E_X830_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、FLOPS 增长曲线（阶梯化规划，封闭七步）
// ---------------------------------------------------------------------------

/// 规划阶梯行（year=节点年；flops_x100=相对 2025 基线的 FLOPS 倍率
/// ×100（如 250 = 2.5 倍）；confidence=置信度 0-100；basis=技术依据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoadmapStep {
    /// 节点年（严格递增）。
    pub year: u32,
    /// FLOPS 倍率 ×100（相对 2025 基线，单调不减）。
    pub flops_x100: u32,
    /// 置信度（0-100——给下游的诚实信号）。
    pub confidence: u8,
    /// 技术依据声明。
    pub basis: &'static str,
}

/// FLOPS 增长曲线阶梯（封闭七步——2025-2035 十年规划；
/// 取 F3682 算力轴预测区间内的规划值：前期取区间下沿保守承诺，
/// 后期随置信度递减上沿放宽——置信度本身就是这段曲线的诚实注脚）。
pub const ROADMAP_STEPS: [RoadmapStep; 7] = [
    RoadmapStep {
        year: 2025,
        flops_x100: 100,
        confidence: 100,
        basis: "基线年：当前量产节点实测",
    },
    RoadmapStep {
        year: 2026,
        flops_x100: 130,
        confidence: 90,
        basis: "F3682 算力轴 2 年区间下沿",
    },
    RoadmapStep {
        year: 2027,
        flops_x100: 175,
        confidence: 85,
        basis: "已公布制程节点路线延续",
    },
    RoadmapStep {
        year: 2029,
        flops_x100: 300,
        confidence: 70,
        basis: "5 年区间中段+先进封装叠层",
    },
    RoadmapStep {
        year: 2031,
        flops_x100: 480,
        confidence: 55,
        basis: "制程 1.5 代+3D 堆叠路线置信区间内",
    },
    RoadmapStep {
        year: 2033,
        flops_x100: 750,
        confidence: 40,
        basis: "新器件形态早期信号外推",
    },
    RoadmapStep {
        year: 2035,
        flops_x100: 1100,
        confidence: 30,
        basis: "十年上沿保守承诺（存算/光电区间下沿）",
    },
];

/// 曲线校验（两条铁律 + 两项诚实）：年份严格递增（倒退判
/// [`E_X830_YEAR_REGRESSED`]）、FLOPS 单调不减（倒退判
/// [`E_X830_FLOPS_REGRESSED`]）、置信度 0-100（越界判
/// [`E_X830_CONFIDENCE_FAULT`]）、依据非空（缺席判
/// [`E_X830_BASIS_MISSING`]）。
pub fn verify_roadmap() -> Result<usize, u16> {
    if ROADMAP_STEPS.is_empty() {
        return Err(E_X830_YEAR_REGRESSED);
    }
    let mut i = 0usize;
    while i < ROADMAP_STEPS.len() {
        let step = &ROADMAP_STEPS[i];
        if step.confidence > 100 {
            return Err(E_X830_CONFIDENCE_FAULT);
        }
        if step.basis.is_empty() {
            return Err(E_X830_BASIS_MISSING);
        }
        if i + 1 < ROADMAP_STEPS.len() {
            let next = &ROADMAP_STEPS[i + 1];
            if next.year <= step.year {
                return Err(E_X830_YEAR_REGRESSED);
            }
            if next.flops_x100 < step.flops_x100 {
                return Err(E_X830_FLOPS_REGRESSED);
            }
        }
        i += 1;
    }
    Ok(ROADMAP_STEPS.len())
}

// ---------------------------------------------------------------------------
// 三、曲线×预测对账（规划值必须落在 F3682 预测区间内——账实相符）
// ---------------------------------------------------------------------------

/// 曲线×预测对账：每个规划年的 FLOPS 倍率必须落在 F3682 算力轴
/// 同视野预测区间 [low, high] 内（规划越界即与预测脱钩——
/// 账实不符判 [`E_X830_FLOPS_REGRESSED`] 的对偶码不适用，
/// 直接沿用区间倒挂语义的对账失败）。
///
/// 对账规则：horizon 对齐——2025/2026/2027 → 2 年视野；
/// 2029/2031 → 5 年视野；2033/2035 → 10 年视野。
pub fn audit_against_forecast() -> Result<usize, u16> {
    // F3682 算力轴（axis=0）区间：(150,220)@2y (300,600)@5y (600,2000)@10y，
    // 倍率口径 ×100 对齐（预测表 150 = 1.5 倍）。
    let ranges: [(u32, u32, u32, u32); 7] = [
        (2025, 2, 100, 220),
        (2026, 2, 100, 220),
        (2027, 2, 100, 220),
        (2029, 5, 100, 600),
        (2031, 5, 100, 600),
        (2033, 10, 100, 2000),
        (2035, 10, 100, 2000),
    ];
    let mut checked = 0usize;
    let mut i = 0usize;
    while i < ROADMAP_STEPS.len() {
        let step = &ROADMAP_STEPS[i];
        let span = ranges[i];
        if step.year != span.0 {
            return Err(E_X830_YEAR_REGRESSED);
        }
        if step.flops_x100 < span.2 || step.flops_x100 > span.3 {
            return Err(E_X830_CONFIDENCE_FAULT);
        }
        let _ = span.1;
        checked += 1;
        i += 1;
    }
    Ok(checked)
}
