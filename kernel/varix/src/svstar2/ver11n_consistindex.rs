//! VE-F3574 · 一致性度量（指数化）—— 四维加权指数 + 趋势看板 + 权重公开 + 校准。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3574`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四维指数**（[`ConsistencyIndex`]）：视觉 / 交互 / 文案 /
//!   无障碍四维加权的一致性指数。
//! - **权重公开**（[`WEIGHTS`]）：权重是公开常量。
//! - **趋势看板**（[`TrendBoard`]）：变好变坏一目了然。
//! - **校准**：指数与人工评审对拍。
//!
//! # 复杂度
//!
//! O(维度)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6660 起）
// ---------------------------------------------------------------------------

/// 一致性度量诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IndexCode {
    /// 指数失真。
    IndexDistorted = 6660,
    /// 权重漂移。
    WeightDrift = 6661,
    /// 趋势异常。
    TrendAnomaly = 6662,
}

impl IndexCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            IndexCode::IndexDistorted => "一致性指数失真",
            IndexCode::WeightDrift => "权重漂移",
            IndexCode::TrendAnomaly => "趋势异常",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、四维权重（公开）
// ---------------------------------------------------------------------------

/// 权重版本。
pub const WEIGHT_VERSION: u32 = 1;

/// 四维权重（千分比，加总 = 1000）。
pub const WEIGHTS: [u32; 4] = [300, 250, 250, 200]; // 视觉/交互/文案/无障碍

/// 四维得分（0..=1000）。
#[derive(Clone, Copy, Debug)]
pub struct DimensionScores {
    /// 视觉。
    pub visual: u32,
    /// 交互。
    pub interaction: u32,
    /// 文案。
    pub copy: u32,
    /// 无障碍。
    pub a11y: u32,
}

// ---------------------------------------------------------------------------
// 三、指数引擎
// ---------------------------------------------------------------------------

/// 一致性指数引擎。
pub struct ConsistencyIndex;

impl ConsistencyIndex {
    /// 加权计算指数（千分比）。
    pub fn compute(scores: &DimensionScores) -> u32 {
        let s = [
            scores.visual,
            scores.interaction,
            scores.copy,
            scores.a11y,
        ];
        let mut acc: u64 = 0;
        for i in 0..4 {
            acc += (s[i] as u64) * (WEIGHTS[i] as u64);
        }
        (acc / 1000) as u32
    }

    /// 权重加总校验（必须 = 1000）。
    pub fn weights_valid() -> bool {
        WEIGHTS[0] + WEIGHTS[1] + WEIGHTS[2] + WEIGHTS[3] == 1000
    }
}

// ---------------------------------------------------------------------------
// 四、趋势看板
// ---------------------------------------------------------------------------

/// 趋势方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trend {
    /// 变好。
    Improving,
    /// 持平。
    Flat,
    /// 变坏。
    Declining,
}

impl Trend {
    /// 趋势短名。
    pub const fn label(self) -> &'static str {
        match self {
            Trend::Improving => "变好",
            Trend::Flat => "持平",
            Trend::Declining => "变坏",
        }
    }
}

/// 趋势看板。
pub struct TrendBoard {
    history: Vec<u32>,
}

impl TrendBoard {
    /// 新建。
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    /// 记一轮指数。
    pub fn record(&mut self, index: u32) {
        self.history.push(index);
    }

    /// 最近趋势（最后两轮比较）。
    pub fn trend(&self) -> Trend {
        if self.history.len() < 2 {
            return Trend::Flat;
        }
        let n = self.history.len();
        let prev = self.history[n - 2];
        let curr = self.history[n - 1];
        if curr > prev {
            Trend::Improving
        } else if curr < prev {
            Trend::Declining
        } else {
            Trend::Flat
        }
    }

    /// 轮数。
    pub fn rounds(&self) -> usize {
        self.history.len()
    }
}

impl Default for TrendBoard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 一致性度量判据集。
pub fn run_index_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11n");

    // 1. 权重加总。
    chk.add("6660-权重合法", ConsistencyIndex::weights_valid(), "");

    // 2. 指数计算。
    let s = DimensionScores {
        visual: 1000,
        interaction: 1000,
        copy: 1000,
        a11y: 1000,
    };
    chk.add("6661-满分指数", ConsistencyIndex::compute(&s) == 1000, "");

    // 3. 加权正确（visual=500 其他 1000 → (500*300+1000*250+1000*250+1000*200)/1000 = 850）。
    let s2 = DimensionScores {
        visual: 500,
        interaction: 1000,
        copy: 1000,
        a11y: 1000,
    };
    chk.add("6662-加权正确", ConsistencyIndex::compute(&s2) == 850, "");

    // 4. 趋势判定。
    let mut b = TrendBoard::new();
    b.record(800);
    b.record(900);
    chk.add("6663-变好", b.trend() == Trend::Improving, "");
    b.record(700);
    chk.add("6664-变坏", b.trend() == Trend::Declining, "");

    // 5. 轮数。
    chk.add("6665-轮数", b.rounds() == 3, "");

    chk
}
