//! 深化层 · F559 屏幕坏点检测（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F559 节）：
//! ①五色循环的**节奏账**——五色 + 灰阶共 [`TOTAL_PAGES`] 页、30 秒扫完
//!   一轮的自动推进换算（不点也能走完的扫场节奏，每页驻留时长唯一源）；
//! ②灰阶渐变页的**逐行生成器**——第 step 档第 row 行亮度的纯整数计算
//!   （0-255 全程覆盖、档内逐行单调递增；越界档诚实拒绝）；
//! ③F360 网格叠加的**定位换算**——屏幕坐标 → 第几行第几列（用户报修
//!   口径），越出屏界拒绝；整屏网格行列总数一并给出；
//! ④**零 UI 纯净性判定**——检测态任何部件（横幅/按钮/提示框）出现即
//!   违规记红；网格叠加是唯一许可的例外（不计违规）。

use crate::checks::CheckSet;
use crate::istar::deadpixel::{DeadPixelStage, GRAY_STEPS, GRID_STEP_PX, SOLID_CYCLE, SolidColor};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// ① 扫场节奏账
// ---------------------------------------------------------------------------

/// 扫描窗口（30 秒扫完一轮——主册定档）。
pub const SCAN_WINDOW_MS: u32 = 30_000;
/// 全轮页数 = 五色 + 16 档灰阶。
pub const TOTAL_PAGES: usize = SOLID_CYCLE.len() + GRAY_STEPS as usize;

/// 扫场节奏账（每页驻留时长 = 窗口 ÷ 页数，整数除唯一源）。
pub struct ScanPace {
    dwell_ms: u32,
}

impl ScanPace {
    pub fn new() -> ScanPace {
        ScanPace { dwell_ms: SCAN_WINDOW_MS / TOTAL_PAGES as u32 }
    }

    pub fn dwell_ms(&self) -> u32 {
        self.dwell_ms
    }

    /// elapsed 毫秒内应推进的页数（自动扫场——不点也能走完）。
    pub fn pages_due(&self, elapsed_ms: u32) -> u32 {
        elapsed_ms / self.dwell_ms.max(1)
    }

    /// 节奏合同：30 秒窗口恰好走完一轮（21 页）。
    pub fn completes_in_window(&self) -> bool {
        self.pages_due(SCAN_WINDOW_MS) == TOTAL_PAGES as u32
    }
}

impl Default for ScanPace {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ② 灰阶逐行生成器
// ---------------------------------------------------------------------------

/// 档内行量化步长（256 级 ÷ 16 档）。
pub const GRAY_ROW_QUANTUM: u32 = 256 / GRAY_STEPS as u32;

/// 第 step 档、第 row 行的亮度（纯整数；档内逐行递增，跨档整段抬升）。
pub fn gray_row_luma(step: u8, row: u32) -> u8 {
    let base = step as u32 * GRAY_ROW_QUANTUM;
    (base + row % GRAY_ROW_QUANTUM) as u8
}

/// 越界诚实版：step ≥ [`GRAY_STEPS`] 拒绝（不给默认值，防脏档静默变脸）。
pub fn try_gray_row_luma(step: u8, row: u32) -> Option<u8> {
    if step >= GRAY_STEPS {
        return None;
    }
    Some(gray_row_luma(step, row))
}

// ---------------------------------------------------------------------------
// ③ 网格定位换算
// ---------------------------------------------------------------------------

/// 屏幕坐标 → (行, 列)（报修口径：坏点在第几行第几列；每格
/// [`GRID_STEP_PX`] px）。越出屏界返回 None。
pub fn grid_cell(x: u32, y: u32, screen_w: u32, screen_h: u32) -> Option<(u32, u32)> {
    if x >= screen_w || y >= screen_h {
        return None;
    }
    Some((y / GRID_STEP_PX, x / GRID_STEP_PX))
}

/// 整屏网格 (行总数, 列总数)（不足一格的边缘余量向上取整）。
pub fn grid_dims(screen_w: u32, screen_h: u32) -> (u32, u32) {
    (
        (screen_h + GRID_STEP_PX - 1) / GRID_STEP_PX,
        (screen_w + GRID_STEP_PX - 1) / GRID_STEP_PX,
    )
}

// ---------------------------------------------------------------------------
// ④ 零 UI 纯净性判定
// ---------------------------------------------------------------------------

/// 纯净性审计：检测态部件出现即记账（网格叠加除外——唯一许可叠加）。
pub struct PurityAudit {
    violations: u8,
}

impl PurityAudit {
    pub fn new() -> PurityAudit {
        PurityAudit { violations: 0 }
    }

    /// 部件出现上报（横幅/按钮/提示框……宿主渲染层注入）。
    pub fn report_element(&mut self) {
        self.violations = self.violations.saturating_add(1);
    }

    /// 纯净判定：零部件违规。
    pub fn verdict(&self) -> bool {
        self.violations == 0
    }

    pub fn violations(&self) -> u8 {
        self.violations
    }
}

impl Default for PurityAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f559_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 节奏账：21 页 / 30 秒，驻留 1428ms，窗口末恰好扫完一轮。
    let pace = ScanPace::new();
    cs.add(
        "21 pages 30s cadence",
        TOTAL_PAGES == 21 && pace.completes_in_window() && pace.dwell_ms() == 1428,
        "",
    );

    // 2) 自动推进：3 个驻留周期 → 推 3 页，与手动 advance 同轨。
    //    循环线 [R,G,B,W,Black]→灰阶：推 3 页后当前停在 W（R→G→B 已扫过）；
    //    推 2 页的中间锚 = B（对账手动换色的逐页语义）。
    let mut s = DeadPixelStage::new();
    let due = pace.pages_due(pace.dwell_ms() * 3);
    for _ in 0..due {
        s.advance();
    }
    let mut mid = DeadPixelStage::new();
    mid.advance();
    mid.advance();
    cs.add(
        "auto advance matches manual",
        due == 3
            && s.color() == SolidColor::White
            && s.switch_count() == 3
            && mid.color() == SolidColor::Blue,
        "",
    );

    // 3) 灰阶角点：step0/row0 = 纯黑 0；step15/row15 = 纯白 255。
    cs.add(
        "gray luma corners",
        try_gray_row_luma(0, 0) == Some(0) && try_gray_row_luma(15, 15) == Some(255),
        "",
    );

    // 4) 档内逐行单调递增（背光不均判读的前提——渐变不许回头）。
    let mut mono = true;
    let mut prev: Option<u8> = None;
    for r in 0..GRAY_ROW_QUANTUM {
        let v = match try_gray_row_luma(8, r) {
            Some(v) => v,
            None => {
                mono = false;
                break;
            }
        };
        if let Some(p) = prev {
            if v <= p {
                mono = false;
            }
        }
        prev = Some(v);
    }
    cs.add("gray ramp monotonic 16 rows", mono && prev == Some(143), "");

    // 5) 越界档诚实拒绝：step 16 / 255 一律 None。
    cs.add(
        "gray step out of range none",
        try_gray_row_luma(GRAY_STEPS, 0).is_none() && try_gray_row_luma(255, 0).is_none(),
        "",
    );

    // 6) 网格定位：(150,250) @1920×1080 → 第 2 行第 1 列（报修口径）。
    cs.add("grid cell row col", grid_cell(150, 250, 1920, 1080) == Some((2, 1)), "");

    // 7) 屏界拒绝 + 行列总数：出界 None；1080p → 11 行 20 列。
    cs.add(
        "grid bounds and dims",
        grid_cell(1920, 0, 1920, 1080).is_none()
            && grid_cell(0, 1080, 1920, 1080).is_none()
            && grid_dims(1920, 1080) == (11, 20),
        "",
    );

    // 8) 纯净性：检测态零部件 = 干净；横幅部件出现即红；网格叠加
    //    是许可例外（部件账不因开网格增减）。
    let mut s2 = DeadPixelStage::new();
    let mut pa = PurityAudit::new();
    let clean0 = pa.verdict() && s2.pristine();
    pa.report_element();
    let dirty = !pa.verdict() && pa.violations() == 1;
    s2.toggle_grid();
    cs.add(
        "purity audit banner red grid tolerated",
        clean0 && dirty && s2.grid_on() && pa.violations() == 1,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_window_auto_scan_returns_to_red() {
        // 30 秒自动扫完 21 页回到纯色首页——节奏账闭环。
        let pace = ScanPace::new();
        let mut s = DeadPixelStage::new();
        for _ in 0..pace.pages_due(SCAN_WINDOW_MS) {
            s.advance();
        }
        assert_eq!(s.color(), SolidColor::Red);
        assert_eq!(s.switch_count(), 21);
    }

    #[test]
    fn gray_sweep_quantum_boundaries() {
        assert_eq!(gray_row_luma(0, 0), 0);
        assert_eq!(gray_row_luma(1, 0), 16);
        assert_eq!(gray_row_luma(0, 1), 1);
        assert_eq!(gray_row_luma(15, 0), 240);
    }

    #[test]
    fn grid_last_pixel_maps_to_last_cell() {
        assert_eq!(grid_cell(1919, 1079, 1920, 1080), Some((10, 19)));
    }
}
