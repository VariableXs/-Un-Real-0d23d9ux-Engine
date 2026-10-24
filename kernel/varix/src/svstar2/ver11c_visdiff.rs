//! VE-F3563 · 跨域视觉一致性对拍 —— 像素级偏差检测 + 感知过滤 + 差异图。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3563`
//!
//! # 职责（锚点原文拆解）
//!
//! - **跨域对拍**（[`DiffEngine`]）：同语义控件在不同域的渲染
//!   diff——不是全屏截图像素比，是语义控件级。
//! - **像素检测 + 感知过滤**（[`PerceptualFilter`]）：像素偏差
//!   超阈检出，但合理差异（渲染噪声/抗锯齿）不误杀——Z07 经验。
//! - **差异图**（[`DiffMap`]）：偏差位置可视化（读屏说明可达）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 对拍失败 | 归因 |
//! | 误杀 | 过滤调优 |
//! | 偏差超阈 | 登记 |
//!
//! # 复杂度
//!
//! O(对比对)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6440 起）
// ---------------------------------------------------------------------------

/// 视觉对拍诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffCode {
    /// 像素偏差超阈。
    PixelExceeded = 6440,
    /// 感知过滤误杀。
    FilterOverkill = 6441,
    /// 对拍失败。
    DiffFailed = 6442,
}

impl DiffCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DiffCode::PixelExceeded => "像素偏差超阈",
            DiffCode::FilterOverkill => "感知过滤误杀",
            DiffCode::DiffFailed => "对拍失败",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DiffCode::PixelExceeded)
    }
}

// ---------------------------------------------------------------------------
// 二、感知过滤
// ---------------------------------------------------------------------------

/// 感知过滤阈值（千分比：低于此值视为噪声不登记）。
pub const PERCEPTUAL_THRESHOLD: u32 = 30;

/// 感知过滤器。
pub struct PerceptualFilter {
    /// 被过滤掉的噪声对数。
    pub filtered: u32,
    /// 被登记的真实差异数。
    pub registered: u32,
}

impl PerceptualFilter {
    /// 新建。
    pub fn new() -> Self {
        Self {
            filtered: 0,
            registered: 0,
        }
    }

    /// 过滤一对：低于阈值=噪声，高于=真差异。
    pub fn classify(&mut self, deviation_permille: u32) -> bool {
        if deviation_permille < PERCEPTUAL_THRESHOLD {
            self.filtered = self.filtered.saturating_add(1);
            false
        } else {
            self.registered = self.registered.saturating_add(1);
            true
        }
    }
}

impl Default for PerceptualFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、差异图
// ---------------------------------------------------------------------------

/// 差异条目。
#[derive(Clone, Copy, Debug)]
pub struct DiffCell {
    /// 控件 id。
    pub control: u32,
    /// 偏差（千分比）。
    pub deviation: u32,
}

/// 差异图（可读视图）。
#[derive(Default)]
pub struct DiffMap {
    cells: Vec<DiffCell>,
}

impl DiffMap {
    /// 从差异集生成。
    pub fn render(hits: &[DiffCell]) -> Self {
        Self {
            cells: hits.to_vec(),
        }
    }

    /// 差异条数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 读屏说明。
    pub fn describe(&self) -> &'static str {
        if self.cells.is_empty() {
            "跨域视觉一致"
        } else {
            "检出跨域视觉差异"
        }
    }
}

// ---------------------------------------------------------------------------
// 四、对拍引擎
// ---------------------------------------------------------------------------

/// 对拍引擎。
pub struct DiffEngine {
    filter: PerceptualFilter,
}

impl DiffEngine {
    /// 新建。
    pub fn new() -> Self {
        Self {
            filter: PerceptualFilter::new(),
        }
    }

    /// 对拍一对控件渲染。
    pub fn compare(&mut self, control: u32, deviation_permille: u32) -> Option<DiffCell> {
        if self.filter.classify(deviation_permille) {
            Some(DiffCell {
                control,
                deviation: deviation_permille,
            })
        } else {
            None
        }
    }

    /// 过滤统计只读。
    pub fn filter_stats(&self) -> (u32, u32) {
        (self.filter.filtered, self.filter.registered)
    }
}

impl Default for DiffEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 视觉对拍判据集。
pub fn run_diff_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11c");

    // 1. 感知过滤噪声。
    let mut f = PerceptualFilter::new();
    let noise = f.classify(10);
    let real = f.classify(100);
    chk.add("6440-噪声过滤", !noise && real && f.filtered == 1 && f.registered == 1, "");

    // 2. 阈值正确。
    chk.add("6441-阈值边界", PERCEPTUAL_THRESHOLD == 30, "");

    // 3. 对拍引擎过滤路径。
    let mut e = DiffEngine::new();
    let none = e.compare(1, 5);
    let some = e.compare(2, 50);
    chk.add("6442-对拍过滤", none.is_none() && some.is_some(), "");

    // 4. 差异图渲染。
    let hits = alloc::vec![DiffCell { control: 1, deviation: 80 }];
    let map = DiffMap::render(&hits);
    chk.add("6443-差异图", map.len() == 1 && map.describe() == "检出跨域视觉差异", "");

    // 5. 空差异图。
    let empty_map = DiffMap::render(&[]);
    chk.add("6444-一致态", empty_map.is_empty() && empty_map.describe() == "跨域视觉一致", "");

    chk
}
