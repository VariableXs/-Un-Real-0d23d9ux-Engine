//! CGPU-F3682 · 硬件演进预测模型（CGPU-X 域 · 十年硬件演进 · 批次 X02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3682`
//!
//! 锚点原文：「模型：硬件演进预测模型（算力/带宽/显存三轴预测——模型实现——
//! 预测表；测试（模型一组）。判据：三轴、预测表、一组、判据。」
//!
//! 判据逐条落位：
//! - **三轴**：[`FORECAST_AXES`]——算力/带宽/显存三轴封闭（锚点原文次序即轴秩）。
//! - **预测表**：[`FORECAST_TABLE`]——每行一条区间化预测
//!   （axis/horizon/low/high/basis 五字段），三轴 × 三视野（2/5/10 年）
//!   共九行封闭；区间倒挂（low ≥ high）判 [`E_X820_RANGE_INVERTED`]——
//!   F3681 纪律「预测给区间不押单点」的可执行形态：单点预测在结构上
//!   无法表达（low < high 是区间存在的最低要求）。
//! - **一组**：判据侧 [`super::vcx82_forecast_checks`] 模型组全量断言。
//!
//! **为什么预测必须带依据**：无依据的区间与瞎猜无法区分——[`BASIS`] 字段
//! 强制每条预测声明其技术依据（已量产节点的延续性/已公布路线图/
//! 产业趋势外推），依据缺席判 [`E_X820_BASIS_MISSING`]。
//!
//! **零静默纪律**：所有倒挂/越轴/依据缺席/视野越界都产出诊断码
//! （0x99C 段 X 域细分续编），由调用方聚合上报。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

// ---------------------------------------------------------------------------
// 一、诊断码（X 域 0x99C 段续编，0x99C8 起）
// ---------------------------------------------------------------------------

/// 区间倒挂（low ≥ high——单点预测或数据错误，区间在结构上不成立）。
pub const E_X820_RANGE_INVERTED: u16 = 0x99C8;
/// 越轴/越视野（axis 超出三轴或 horizon 不在封闭视野集内）。
pub const E_X820_AXIS_UNKNOWN: u16 = 0x99C9;
/// 依据缺席（预测行 basis 空——与瞎猜无法区分）。
pub const E_X820_BASIS_MISSING: u16 = 0x99CA;
/// 版本失配（批次版本与 X02 冻结版不符）。
pub const E_X820_VERSION_MISMATCH: u16 = 0x99CB;

/// 域批次版本。
pub const VCX82_VERSION: &str = "CX82-forecast-v1";

/// 版本校验（双向）：X02 冻结版放行，其余一律 [`E_X820_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCX82_VERSION {
        Ok(())
    } else {
        Err(E_X820_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、三轴封闭（算力/带宽/显存，锚点原文次序即轴秩）
// ---------------------------------------------------------------------------

/// 预测三轴（封闭——锚点「算力/带宽/显存三轴」；次序即锚点原文点名序）。
pub const FORECAST_AXES: [&str; 3] = ["算力", "带宽", "显存"];

/// 预测视野集（封闭三档——2/5/10 年，十年演进的三段观察窗）。
pub const HORIZONS: [u32; 3] = [2, 5, 10];

/// 预测表行（axis=轴下标；horizon=视野年数；low/high=区间上下界，
/// 相对 2025 基线的倍率百分数；basis=技术依据声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastRow {
    /// 所属轴（0=算力 1=带宽 2=显存，对齐 [`FORECAST_AXES`] 下标）。
    pub axis: usize,
    /// 预测视野（年数，必须落在 [`HORIZONS`] 封闭集内）。
    pub horizon: u32,
    /// 区间下界（倍率百分数，如 400 表示 4 倍）。
    pub low: u32,
    /// 区间上界（必须大于 low——单点预测在结构上不可表达）。
    pub high: u32,
    /// 技术依据声明（一句可 grep 的依据）。
    pub basis: &'static str,
}

/// 硬件演进预测表（封闭——三轴 × 三视野共九行；倍率相对 2025 基线）。
pub const FORECAST_TABLE: [ForecastRow; 9] = [
    ForecastRow {
        axis: 0,
        horizon: 2,
        low: 150,
        high: 220,
        basis: "已公布制程与封装路线的延续性外推",
    },
    ForecastRow {
        axis: 0,
        horizon: 5,
        low: 300,
        high: 600,
        basis: "制程 1.5 代延续+先进封装叠层趋势",
    },
    ForecastRow {
        axis: 0,
        horizon: 10,
        low: 600,
        high: 2000,
        basis: "新器件形态（存算一体/光电混合）区间研判",
    },
    ForecastRow {
        axis: 1,
        horizon: 2,
        low: 140,
        high: 200,
        basis: "PCIe 世代交替与 CXL 路线已公布",
    },
    ForecastRow {
        axis: 1,
        horizon: 5,
        low: 250,
        high: 500,
        basis: "新总线世代与光互连早期渗透",
    },
    ForecastRow {
        axis: 1,
        horizon: 10,
        low: 500,
        high: 1800,
        basis: "光互连成熟度区间研判（共封装光学）",
    },
    ForecastRow {
        axis: 2,
        horizon: 2,
        low: 160,
        high: 240,
        basis: "HBM 世代堆叠层数已公布路线",
    },
    ForecastRow {
        axis: 2,
        horizon: 5,
        low: 320,
        high: 640,
        basis: "HBM 堆叠+基板键合工艺延续",
    },
    ForecastRow {
        axis: 2,
        horizon: 10,
        low: 640,
        high: 2400,
        basis: "新显存介质（CXL 内存池/存算融合）区间研判",
    },
];

/// 预测表校验：表满格（三轴 × 三视野不缺格不重复）、区间合法
/// （low < high——倒挂判 [`E_X820_RANGE_INVERTED`]）、轴与视野落封闭集
/// （越界判 [`E_X820_AXIS_UNKNOWN`]）、依据非空
/// （缺席判 [`E_X820_BASIS_MISSING`]）。
pub fn verify_forecast_table() -> Result<usize, u16> {
    if FORECAST_TABLE.len() != 9 {
        return Err(E_X820_AXIS_UNKNOWN);
    }
    // 格唯一性：每格 (axis, horizon) 只出现一次且轴/视野落封闭集。
    let mut i = 0usize;
    while i < FORECAST_TABLE.len() {
        let row = &FORECAST_TABLE[i];
        if row.axis >= 3 {
            return Err(E_X820_AXIS_UNKNOWN);
        }
        let mut in_horizon = false;
        let mut h = 0usize;
        while h < 3 {
            if HORIZONS[h] == row.horizon {
                in_horizon = true;
            }
            h += 1;
        }
        if !in_horizon {
            return Err(E_X820_AXIS_UNKNOWN);
        }
        let mut j = i + 1;
        while j < FORECAST_TABLE.len() {
            let other = &FORECAST_TABLE[j];
            if other.axis == row.axis && other.horizon == row.horizon {
                return Err(E_X820_AXIS_UNKNOWN);
            }
            j += 1;
        }
        i += 1;
    }
    // 区间合法 + 依据非空。
    for row in FORECAST_TABLE.iter() {
        if row.low >= row.high {
            return Err(E_X820_RANGE_INVERTED);
        }
        if row.basis.is_empty() {
            return Err(E_X820_BASIS_MISSING);
        }
    }
    Ok(FORECAST_TABLE.len())
}

// ---------------------------------------------------------------------------
// 三、轴区间守恒（三轴各自三视野单调性——视野越远区间越宽）
// ---------------------------------------------------------------------------

/// 轴单调校验：同一轴内视野越远，区间应越宽（high-low 随视野递增）——
/// 远期预测不确定性更大是预测诚实性的结构表达。
/// 单调破缺判 [`E_X820_RANGE_INVERTED`]（广义倒挂：不确定性倒挂）。
pub fn verify_axis_monotonic(axis: usize) -> Result<(), u16> {
    if axis >= 3 {
        return Err(E_X820_AXIS_UNKNOWN);
    }
    let mut widths: [u32; 3] = [0, 0, 0];
    let mut seen = 0usize;
    for row in FORECAST_TABLE.iter() {
        if row.axis == axis {
            let mut slot = 3usize;
            let mut h = 0usize;
            while h < 3 {
                if HORIZONS[h] == row.horizon {
                    slot = h;
                }
                h += 1;
            }
            if slot < 3 {
                widths[slot] = row.high - row.low;
                seen += 1;
            }
        }
    }
    if seen != 3 {
        return Err(E_X820_AXIS_UNKNOWN);
    }
    if !(widths[0] < widths[1] && widths[1] < widths[2]) {
        return Err(E_X820_RANGE_INVERTED);
    }
    Ok(())
}
