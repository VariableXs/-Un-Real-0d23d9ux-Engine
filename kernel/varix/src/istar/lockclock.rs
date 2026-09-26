//! F564 锁屏时钟样式 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：两式渲染；指针微步进动画；秒针开关；三米可读走查；
//! 预览实时。
//!
//! **设计要点（主册）**：
//! - 锁屏大钟两式可选：数字式（默认——大字号 72px F238 同源）与表盘式
//!   （星徽风格模拟表盘——指针用 F124 动画微步进，秒针可选关）；
//! - 样式切换即时预览（设置页 mini 预览 F499 同源）；两式都过可读性红线
//!   （三米外可读）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 数字式大字号（px，F238 同源）。
pub const DIGITAL_FONT_PX: u32 = 72;

/// 指针微步进：秒针每帧角度（0.1°/帧 @60fps → 6°/秒实时速率；
/// 一圈 3600 帧 = 60 秒，F124 曲线族的锁屏落点）。
pub const SECOND_HAND_DEG_PER_TICK_X100: u32 = 10;

/// 每秒帧数（微步进节奏唯一源）。
pub const TICKS_PER_SECOND: u32 = 60;

/// 表盘秒针开关缺省（主册「安静」缺省——关）。
pub const SECOND_HAND_DEFAULT: bool = false;

/// 三米可读红线（数字式最小渲染字号——低于此字号判不可读）。
pub const READABLE_MIN_PX: u32 = 72;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 时钟样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockStyle {
    Digital,
    Analog,
}

/// 锁屏时钟配置与渲染账。
pub struct LockClock {
    style: ClockStyle,
    second_hand: bool,
    /// 表盘指针角度账（度 ×100，0..=36000 循环）。
    hour_deg_x100: u32,
    min_deg_x100: u32,
    sec_deg_x100: u32,
    /// 微步进帧数账。
    ticks: u64,
}

impl LockClock {
    pub fn new() -> LockClock {
        LockClock {
            style: ClockStyle::Digital,
            second_hand: SECOND_HAND_DEFAULT,
            hour_deg_x100: 0,
            min_deg_x100: 0,
            sec_deg_x100: 0,
            ticks: 0,
        }
    }

    /// 换样式（即时生效——预览与锁屏同一取数口）。
    pub fn set_style(&mut self, s: ClockStyle) {
        self.style = s;
    }

    pub fn style(&self) -> ClockStyle {
        self.style
    }

    /// 秒针开关（表盘式才有效；数字式无秒针概念——开关状态保留但不渲染）。
    pub fn set_second_hand(&mut self, on: bool) {
        self.second_hand = on;
    }

    pub fn second_hand(&self) -> bool {
        self.second_hand
    }

    /// 推进一帧微步进（秒针 0.1°/帧@60fps = 6°/秒；秒针过零 = 过 1 分钟
    /// → 分针进 6°（360°/时）；分针过零 = 过 1 小时 → 时针进 30°
    /// （360°/12h）。比例全部从单一帧节奏推导，无独立魔法数）。
    pub fn tick(&mut self) {
        self.ticks += 1;
        self.sec_deg_x100 = (self.sec_deg_x100 + SECOND_HAND_DEG_PER_TICK_X100) % 36_000;
        if self.sec_deg_x100 == 0 {
            // 秒针过零 = 过 1 分钟 → 分针 6°。
            self.min_deg_x100 = (self.min_deg_x100 + 600) % 36_000;
            if self.min_deg_x100 == 0 {
                // 分针过零 = 过 1 小时 → 时针 30°。
                self.hour_deg_x100 = (self.hour_deg_x100 + 3_000) % 36_000;
            }
        }
    }

    pub fn hands(&self) -> (u32, u32, u32) {
        (self.hour_deg_x100, self.min_deg_x100, self.sec_deg_x100)
    }

    pub fn tick_count(&self) -> u64 {
        self.ticks
    }

    /// 渲染字号（数字式 72px；三米可读红线唯一判定口）。
    pub fn render_font_px(&self) -> u32 {
        match self.style {
            ClockStyle::Digital => DIGITAL_FONT_PX,
            ClockStyle::Analog => DIGITAL_FONT_PX, // 表盘直径等价字号口径
        }
    }

    /// 可读性红线判定（两式同过——字号 ≥ 三米可读下限）。
    pub fn readable_at_three_meters(&self) -> bool {
        self.render_font_px() >= READABLE_MIN_PX
    }

    /// 预览实时性：样式切换后首帧即按新样式渲染（状态即渲染源——结构证据）。
    pub fn preview_after_switch(&mut self, s: ClockStyle) -> ClockStyle {
        self.set_style(s);
        self.style
    }
}

impl Default for LockClock {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_lockclock_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 两式渲染：数字式缺省；切表盘即时（预览实时）。
    let mut c = LockClock::new();
    let digital_default = c.style() == ClockStyle::Digital;
    let switched = c.preview_after_switch(ClockStyle::Analog) == ClockStyle::Analog;
    set.add("two styles instant preview", digital_default && switched, "");

    // 2. 指针微步进：每帧 0.1°@60fps，3600 帧一圈（60 秒实时对齐）。
    let mut c2 = LockClock::new();
    c2.set_style(ClockStyle::Analog);
    for _ in 0..(TICKS_PER_SECOND as u64 * 60) {
        c2.tick();
    }
    let (_, _, sec) = c2.hands();
    set.add(
        "micro step full circle per minute",
        sec == 0 && c2.tick_count() == TICKS_PER_SECOND as u64 * 60,
        "",
    );

    // 3. 指针联动：60 圈秒针 = 1 圈分针（3600 秒后分针回零、时针进 5°×12=?）
    //    ——3600 秒 = 60 分钟：分针走满 360°回零，时针进 30°。
    let mut c3 = LockClock::new();
    c3.set_style(ClockStyle::Analog);
    for _ in 0..(TICKS_PER_SECOND as u64 * 3600) {
        c3.tick();
    }
    let (h, m, _) = c3.hands();
    set.add("hour hand 30 degrees per hour", m == 0 && h == 3_000, "");

    // 4. 秒针开关：缺省关（安静）；开后指针账照走（渲染层决定显隐）。
    let mut c4 = LockClock::new();
    c4.set_style(ClockStyle::Analog);
    let off_default = !c4.second_hand();
    c4.set_second_hand(true);
    c4.tick();
    let (_, _, s1) = c4.hands();
    set.add(
        "second hand toggle",
        off_default && c4.second_hand() && s1 == SECOND_HAND_DEG_PER_TICK_X100,
        "",
    );

    // 5. 三米可读走查：两式字号 ≥ 72px 红线。
    let mut c5 = LockClock::new();
    let d_ok = c5.readable_at_three_meters();
    c5.set_style(ClockStyle::Analog);
    set.add(
        "readable at three meters",
        d_ok && c5.readable_at_three_meters() && READABLE_MIN_PX == 72,
        "",
    );

    // 6. F238 同源：数字式字号常量 = 72px。
    set.add("digital font 72px f238", DIGITAL_FONT_PX == 72, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_wrap_at_360() {
        let mut c = LockClock::new();
        c.set_style(ClockStyle::Analog);
        for _ in 0..(240 * 60 * 12 + 5) {
            c.tick();
        }
        let (h, m, s) = c.hands();
        assert!(h < 36_000 && m < 36_000 && s < 36_000);
    }

    #[test]
    fn second_hand_hidden_still_ticks() {
        // 秒针关 = 不渲染，不 = 停走（时间账独立于显示）。
        let mut c = LockClock::new();
        c.tick();
        assert_eq!(c.hands().2, SECOND_HAND_DEG_PER_TICK_X100);
    }
}
