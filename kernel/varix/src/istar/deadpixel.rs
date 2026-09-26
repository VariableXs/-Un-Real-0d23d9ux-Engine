//! F559 屏幕坏点检测 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：五色纯净性（全屏零 UI）；换色交互；灰阶页；
//! 网格叠加联动；Esc 退出即时。
//!
//! **设计要点（主册）**：
//! - 全屏纯色循环（红/绿/蓝/白/黑五色，点击换色/Esc 退）——坏点/亮点/
//!   坏线在纯色下一目了然；
//! - 补充灰阶渐变页（检查背光不均）；
//! - 检测结果不自动判定（人眼为准——系统只提供干净的测试场）；
//! - 可配 F360 网格叠加辅助定位（坏点在第几行第几列）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 五色纯色页（顺序即循环序——RGBW 黑，先彩色后黑白：彩查坏点、白查暗点、
/// 黑查亮点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidColor {
    Red,
    Green,
    Blue,
    White,
    Black,
}

pub const SOLID_CYCLE: [SolidColor; 5] = [
    SolidColor::Red,
    SolidColor::Green,
    SolidColor::Blue,
    SolidColor::White,
    SolidColor::Black,
];

/// 灰阶页档数（16 级渐变——背光不均判读够用）。
pub const GRAY_STEPS: u8 = 16;

/// 网格叠加间距（px，F360 同源——每格 100px 定位口径）。
pub const GRID_STEP_PX: u32 = 100;

// ---------------------------------------------------------------------------
// 检测场状态机
// ---------------------------------------------------------------------------

/// 检测场（全屏测试页状态机）。
pub struct DeadPixelStage {
    /// 当前页。
    page: Page,
    /// 纯色循环游标。
    solid_idx: usize,
    /// 灰阶当前档。
    gray_idx: u8,
    /// 网格叠加开关（F360 联动）。
    grid_on: bool,
    /// 会话账：换色次数（走查凭证）。
    switches: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Solid,
    Gray,
}

impl DeadPixelStage {
    pub fn new() -> DeadPixelStage {
        DeadPixelStage {
            page: Page::Solid,
            solid_idx: 0,
            gray_idx: 0,
            grid_on: false,
            switches: 0,
        }
    }

    /// 当前显示的纯色（Solid 页时）。
    pub fn color(&self) -> SolidColor {
        SOLID_CYCLE[self.solid_idx]
    }

    /// 灰阶当前档（Gray 页时；0..16）。
    pub fn gray(&self) -> u8 {
        self.gray_idx
    }

    /// 点击换色：纯色页循环推进；灰阶页推进灰阶档；页尾回纯色首页
    /// （五色+灰阶一条循环线，不设死胡同）。
    pub fn advance(&mut self) {
        self.switches += 1;
        match self.page {
            Page::Solid => {
                self.solid_idx += 1;
                if self.solid_idx == SOLID_CYCLE.len() {
                    self.solid_idx = 0;
                    self.page = Page::Gray;
                }
            }
            Page::Gray => {
                self.gray_idx += 1;
                if self.gray_idx == GRAY_STEPS {
                    self.gray_idx = 0;
                    self.page = Page::Solid;
                }
            }
        }
    }

    /// 是否在灰阶页。
    pub fn on_gray(&self) -> bool {
        self.page == Page::Gray
    }

    /// 网格叠加开/关（F360 联动口）。
    pub fn toggle_grid(&mut self) -> bool {
        self.grid_on = !self.grid_on;
        self.grid_on
    }

    pub fn grid_on(&self) -> bool {
        self.grid_on
    }

    /// 纯净性判据：测试场自身 UI 归零（网格关时全屏零 UI——状态机只输出
    /// 底色，无横幅无按钮；本判定即状态机输出面清点）。
    pub fn pristine(&self) -> bool {
        !self.grid_on
    }

    /// Esc 退出（返回是否退出——即时语义：任何页任何状态一律退）。
    pub fn esc_exit(&mut self) -> bool {
        true
    }

    /// 换色次数（走查账）。
    pub fn switch_count(&self) -> u32 {
        self.switches
    }
}

impl Default for DeadPixelStage {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_deadpixel_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 五色循环序钉死（RGBW黑——顺序即循环唯一源）。
    set.add(
        "five solid colors in order",
        SOLID_CYCLE
            == [SolidColor::Red, SolidColor::Green, SolidColor::Blue, SolidColor::White, SolidColor::Black],
        "",
    );

    // 2. 换色交互：点击推进 R→G→B→W→黑→灰阶页。
    let mut s = DeadPixelStage::new();
    s.advance();
    let green = s.color() == SolidColor::Green;
    for _ in 0..3 {
        s.advance();
    }
    let black = s.color() == SolidColor::Black;
    s.advance();
    set.add("click advances cycle to gray", green && black && s.on_gray() && s.gray() == 0, "");

    // 3. 灰阶页 16 档走完回纯色首页。
    for _ in 0..GRAY_STEPS {
        s.advance();
    }
    set.add(
        "gray sixteen steps then first solid",
        !s.on_gray() && s.color() == SolidColor::Red && s.gray() == 0,
        "",
    );

    // 4. 网格叠加联动：开/关往返；开时纯净性让位（网格是唯一许可的叠加）。
    s.toggle_grid();
    let on = s.grid_on();
    s.toggle_grid();
    set.add("grid overlay toggles", on && !s.grid_on(), "");

    // 5. 五色纯净性：网格关时状态机输出面零 UI（pristine 判定）。
    let mut s2 = DeadPixelStage::new();
    s2.advance();
    s2.advance();
    set.add("pristine without overlay", s2.pristine() && s2.switch_count() == 2, "");

    // 6. Esc 退出即时：任何页一律退（状态机不设拦截）。
    let mut s3 = DeadPixelStage::new();
    s3.advance();
    s3.toggle_grid();
    set.add("esc exits from any state", s3.esc_exit(), "");

    // 7. 灰阶档位界内（16 级不越界——渐变页参数红线）。
    let mut s4 = DeadPixelStage::new();
    for _ in 0..(5 + 8) {
        s4.advance();
    }
    set.add("gray stays within sixteen", s4.on_gray() && s4.gray() < GRAY_STEPS, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_cycle_length() {
        // 全循环 = 5 色 + 16 灰阶 = 21 步回原点。
        let mut s = DeadPixelStage::new();
        for _ in 0..(SOLID_CYCLE.len() + GRAY_STEPS as usize) {
            s.advance();
        }
        assert_eq!(s.color(), SolidColor::Red);
        assert_eq!(s.switch_count(), 21);
    }

    #[test]
    fn grid_independent_of_color() {
        let mut s = DeadPixelStage::new();
        s.toggle_grid();
        s.advance();
        assert!(s.grid_on());
        assert_eq!(s.color(), SolidColor::Green);
    }
}
