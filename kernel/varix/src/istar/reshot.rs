//! F595 固定区域重截 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：一键重截；200ms 确认框；会话内记忆；与 F593 组合
//! 用例；快捷键注册。
//!
//! **设计要点（主册）**：
//! - 截图工具记住上次区域：同区域重截一键（工具窗「重截上区域」按钮 +
//!   快捷键）——盯梢类需求（监控界面变化/连续截同一窗口位置）一秒一张；
//! - 区域框显示 200ms 确认视觉；保存区域随会话（重启清——不做长期记忆
//!   防偏移）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 区域框确认视觉时长（ms）。
pub const CONFIRM_FLASH_MS: u32 = 200;

/// 区域坐标（px，i32 屏幕坐标）。
pub type Rect = (i32, i32, u32, u32);

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 固定区域重截器（会话内账——重启清）。
pub struct Reshot {
    /// 上次区域（None = 本会话还没框过）。
    last: Option<Rect>,
    /// 确认视觉剩余 ms。
    flash_left: u32,
    /// 会话代（重启清零边界证据）。
    generation: u64,
    /// 重截次数账（盯梢连拍对账）。
    reshoots: u32,
    /// 快捷键绑定（注册进 F244 注册表的值）。
    hotkey: Option<&'static str>,
}

impl Reshot {
    pub fn new(generation: u64) -> Reshot {
        Reshot {
            last: None,
            flash_left: 0,
            generation,
            reshoots: 0,
            hotkey: Some("Ctrl+Shift+R"),
        }
    }

    /// 框选完成（保存区域随会话）。
    pub fn capture_region(&mut self, r: Rect) {
        self.last = Some(r);
    }

    pub fn has_region(&self) -> bool {
        self.last.is_some()
    }

    /// 一键重截：无需再框（返回上次区域 + 触发确认视觉）。
    ///
    /// 无区域时诚实 None（会话内没框过就不能假装知道上次在哪）。
    pub fn reshot(&mut self) -> Option<Rect> {
        let r = self.last?;
        self.reshoots += 1;
        self.flash_left = CONFIRM_FLASH_MS;
        Some(r)
    }

    /// 确认视觉推进（区域框闪 200ms 让用户确认位置）。
    pub fn tick(&mut self, ms: u32) {
        self.flash_left = self.flash_left.saturating_sub(ms);
    }

    pub fn flashing(&self) -> bool {
        self.flash_left > 0
    }

    /// 会话内记忆边界：重启后代账全新（防偏移——不做长期记忆）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 快捷键注册值（F244 注册表）。
    pub fn hotkey(&self) -> Option<&'static str> {
        self.hotkey
    }

    pub fn reshot_count(&self) -> u32 {
        self.reshoots
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_reshot_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 一键重截：框一次、连拍三次全走同区域（盯梢一秒一张）。
    let mut r = Reshot::new(1);
    r.capture_region((100, 200, 800, 600));
    let a = r.reshot();
    let b = r.reshot();
    let c = r.reshot();
    set.add(
        "one key reshoot same region",
        a == Some((100, 200, 800, 600))
            && b == Some((100, 200, 800, 600))
            && c == Some((100, 200, 800, 600))
            && r.reshot_count() == 3,
        "",
    );

    // 2. 200ms 确认框：重截即闪、200ms 自收（位置确认视觉）。
    let flashing = r.flashing();
    r.tick(CONFIRM_FLASH_MS);
    set.add(
        "confirm flash 200ms",
        flashing && !r.flashing() && CONFIRM_FLASH_MS == 200,
        "",
    );

    // 3. 会话内记忆：重启清——代 2 账里没有代 1 的区域。
    let mut g1 = Reshot::new(1);
    g1.capture_region((0, 0, 10, 10));
    let mut g2 = Reshot::new(2);
    set.add(
        "session memory cleared on reboot",
        g1.has_region() && !g2.has_region() && g2.reshot().is_none(),
        "",
    );

    // 4. 无区域诚实 None：本会话没框过不给假装重截。
    set.add(
        "no region honest none",
        g2.reshot().is_none() && g2.reshot_count() == 0,
        "",
    );

    // 5. 与 F593 组合：延迟 + 固定区域 = 全自动盯梢（区域账与倒计时账
    //    并行不互斥——组合用例）。
    let mut combo_region = Reshot::new(1);
    combo_region.capture_region((10, 10, 100, 100));
    // F593 倒计时（模型面直接复用其三档常量语义——此处以 3 秒档演练）。
    let mut delay = crate::istar::delayshot::DelayShot::new();
    let started = delay.start(3, 0);
    for i in 0..190u64 {
        delay.frame(i * 16, 16);
    }
    let shot = combo_region.reshot();
    set.add(
        "combines with f593 delayed shot",
        started
            && delay.state() == crate::istar::delayshot::CountState::Fired
            && shot.is_some(),
        "",
    );

    // 6. 快捷键注册：「重截上区域」有键（F244 注册表值在册）。
    set.add(
        "hotkey registered",
        r.hotkey() == Some("Ctrl+Shift+R"),
        "",
    );

    // 7. 重框更新：会话中重框后重截走新区域（记忆是「最近一次」）。
    r.capture_region((0, 0, 50, 50));
    let updated = r.reshot();
    set.add(
        "re capture updates memory",
        updated == Some((0, 0, 50, 50)),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_does_not_block_reshoot() {
        // 确认视觉在途也可再截（连拍不等闪完——一秒一张的节奏保障）。
        let mut r = Reshot::new(1);
        r.capture_region((1, 1, 5, 5));
        r.reshot();
        assert!(r.flashing());
        assert!(r.reshot().is_some());
    }

    #[test]
    fn negative_coords_legal() {
        let mut r = Reshot::new(1);
        r.capture_region((-100, -50, 200, 100));
        assert_eq!(r.reshot(), Some((-100, -50, 200, 100)));
    }

    #[test]
    fn zero_size_region_still_saved() {
        // 零尺寸区域按框选原样保存（有效性由截图工具裁剪层裁决）。
        let mut r = Reshot::new(1);
        r.capture_region((5, 5, 0, 0));
        assert_eq!(r.reshot(), Some((5, 5, 0, 0)));
    }
}
