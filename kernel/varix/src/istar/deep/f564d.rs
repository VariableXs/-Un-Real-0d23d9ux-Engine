//! 深化层 · F564 锁屏时钟样式（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F564 节）：
//! ①**表盘渲染参数引擎**——半径/刻度/指针长度全部从数字式 72px 单一源
//!   （[`DIGITAL_FONT_PX`]）派生，无独立魔法数；秒针角度 → 60 刻度索引、
//!   时针角度 → 12 刻度索引的纯整数换算（渲染层取数的几何唯一源）；
//! ②**微步进时序账**——逐帧审计秒针角度序列：环上差恒等于
//!   [`SECOND_HAND_DEG_PER_TICK_X100`]（0.1°/帧@60fps），任何一帧偏离
//!   即跳格违约（连续角度序列，不跳格）；
//! ③**秒针安静模式的层级账**——关秒针 = 表盘重绘清单剔除秒针层
//!   （4 层 → 3 层），数字式只有字面 1 层；
//! ④**三米可读换算账**——字号 × 线宽 × 对比度三分账，任一不过线即
//!   不可读（两式同过一条红线）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::lockclock::{
    ClockStyle, LockClock, DIGITAL_FONT_PX, READABLE_MIN_PX, SECOND_HAND_DEFAULT,
    SECOND_HAND_DEG_PER_TICK_X100, TICKS_PER_SECOND,
};

// ---------------------------------------------------------------------------
// ① 表盘渲染参数引擎
// ---------------------------------------------------------------------------

/// 表盘半径（数字式 72px + 24px 余量——单一源派生）。
pub const DIAL_RADIUS_PX: u32 = DIGITAL_FONT_PX + 24;
/// 时刻度数。
pub const HOUR_TICKS: u32 = 12;
/// 每时刻度间的小刻度数（12×5 = 60 秒刻度）。
pub const MINOR_TICKS_PER_HOUR: u32 = 5;

/// 表盘几何（渲染层取数唯一源）。
pub struct DialGeometry;

impl DialGeometry {
    pub fn radius() -> u32 {
        DIAL_RADIUS_PX
    }

    /// 秒刻度总数（60——每刻 6°）。
    pub fn tick_count() -> u32 {
        HOUR_TICKS * MINOR_TICKS_PER_HOUR
    }

    /// 指针长度：时针半径、分针 3/4 径、秒针 7/8 径（经典表盘比例）。
    pub fn hour_hand_len() -> u32 {
        DIAL_RADIUS_PX / 2
    }

    pub fn minute_hand_len() -> u32 {
        DIAL_RADIUS_PX * 3 / 4
    }

    pub fn second_hand_len() -> u32 {
        DIAL_RADIUS_PX * 7 / 8
    }

    /// 角度 x100 → 秒针刻度索引（每刻 6° = 600 x100）。
    pub fn sec_tick_index(deg_x100: u32) -> u8 {
        ((deg_x100 / 600) % 60) as u8
    }

    /// 角度 x100 → 时针刻度索引（每刻 30° = 3000 x100）。
    pub fn hour_tick_index(deg_x100: u32) -> u8 {
        ((deg_x100 / 3_000) % 12) as u8
    }
}

// ---------------------------------------------------------------------------
// ② 微步进时序账
// ---------------------------------------------------------------------------

/// 微步进审计：逐帧对账秒针角度序列。
pub struct MicroStepAudit {
    samples: u64,
    max_delta: u32,
    off_cadence: u32,
}

impl MicroStepAudit {
    /// 跑 frames 帧并逐帧记账：环上差（含过零回绕）恒等于 0.1°/帧。
    pub fn run(frames: u64) -> MicroStepAudit {
        let mut c = LockClock::new();
        c.set_style(ClockStyle::Analog);
        let mut prev = c.hands().2;
        let mut a = MicroStepAudit { samples: 0, max_delta: 0, off_cadence: 0 };
        for _ in 0..frames {
            c.tick();
            let cur = c.hands().2;
            let d = if cur >= prev { cur - prev } else { 36_000 - (prev - cur) };
            if d != SECOND_HAND_DEG_PER_TICK_X100 {
                a.off_cadence += 1;
            }
            if d > a.max_delta {
                a.max_delta = d;
            }
            a.samples += 1;
            prev = cur;
        }
        a
    }

    /// 不跳格：全帧无一处偏离 0.1°/帧。
    pub fn seamless(&self) -> bool {
        self.off_cadence == 0
    }

    pub fn max_delta(&self) -> u32 {
        self.max_delta
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }
}

// ---------------------------------------------------------------------------
// ③ 秒针安静模式的层级账
// ---------------------------------------------------------------------------

/// 表盘渲染层级清单（重绘面 = true 的层）。
pub struct LayerPlan {
    pub face: bool,
    pub hour: bool,
    pub minute: bool,
    pub second: bool,
}

impl LayerPlan {
    /// 表盘式：字面 + 时针 + 分针恒在；秒针层随开关（安静模式剔除）。
    pub fn analog(second_hand: bool) -> LayerPlan {
        LayerPlan { face: true, hour: true, minute: true, second: second_hand }
    }

    /// 数字式：只有字面一层（无指针概念）。
    pub fn digital() -> LayerPlan {
        LayerPlan { face: true, hour: false, minute: false, second: false }
    }

    pub fn redraw_layers(&self) -> u8 {
        self.face as u8 + self.hour as u8 + self.minute as u8 + self.second as u8
    }
}

// ---------------------------------------------------------------------------
// ④ 三米可读换算账
// ---------------------------------------------------------------------------

/// 最小线宽（指针/笔画——细线三米外糊掉）。
pub const MIN_STROKE_PX: u32 = 6;
/// 最小对比度（0-255 口径的 3/4）。
pub const MIN_CONTRAST_Q8: u32 = 192;

/// 三分账：字号 × 线宽 × 对比度，任一不过线即不可读。
pub fn readable(font_px: u32, stroke_px: u32, contrast_q8: u32) -> bool {
    font_px >= READABLE_MIN_PX && stroke_px >= MIN_STROKE_PX && contrast_q8 >= MIN_CONTRAST_Q8
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f564_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 参数引擎单一源：半径 = 72 + 24；秒刻度 60。
    cs.add(
        "dial derived from font source",
        DialGeometry::radius() == DIAL_RADIUS_PX
            && DIAL_RADIUS_PX == DIGITAL_FONT_PX + 24
            && DialGeometry::tick_count() == 60,
        "",
    );

    // 2) 指针长度经典比例：48 / 72 / 84。
    cs.add(
        "hand length ratios",
        DialGeometry::hour_hand_len() == 48
            && DialGeometry::minute_hand_len() == 72
            && DialGeometry::second_hand_len() == 84,
        "",
    );

    // 3) 刻度索引换算：0°→0、6°→1、359.9°→59；时针 30°→1、330°→11。
    cs.add(
        "tick index mapping",
        DialGeometry::sec_tick_index(0) == 0
            && DialGeometry::sec_tick_index(600) == 1
            && DialGeometry::sec_tick_index(35_990) == 59
            && DialGeometry::hour_tick_index(3_000) == 1
            && DialGeometry::hour_tick_index(33_000) == 11,
        "",
    );

    // 4) 微步进一分钟审计：3600 帧逐帧 0.1°，环上差无一处偏离。
    let minute = MicroStepAudit::run(TICKS_PER_SECOND as u64 * 60);
    cs.add(
        "micro step seamless per minute",
        minute.samples() == 3_600 && minute.seamless(),
        "",
    );

    // 5) 不跳格红线：全程最大单帧增量恰为 0.1°（无跳格无滞后）。
    cs.add(
        "micro step max delta exact",
        minute.max_delta() == SECOND_HAND_DEG_PER_TICK_X100,
        "",
    );

    // 6) 安静模式：秒针缺省关 → 3 层重绘；开 → 4 层。
    cs.add(
        "quiet mode drops second layer",
        !SECOND_HAND_DEFAULT
            && LayerPlan::analog(SECOND_HAND_DEFAULT).redraw_layers() == 3
            && LayerPlan::analog(true).redraw_layers() == 4,
        "",
    );

    // 7) 数字式最省：只有字面 1 层。
    cs.add("digital plan single layer", LayerPlan::digital().redraw_layers() == 1, "");

    // 8) 三米可读三分账：72/6/192 过线；字号、线宽、对比度任一缺一不可。
    cs.add(
        "readability three way ledger",
        readable(72, 6, 192)
            && !readable(48, 6, 192)
            && !readable(72, 5, 192)
            && !readable(72, 6, 191),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micro_audit_sixty_frames() {
        let a = MicroStepAudit::run(60);
        assert_eq!(a.samples(), 60);
        assert!(a.seamless());
        assert_eq!(a.max_delta(), SECOND_HAND_DEG_PER_TICK_X100);
    }

    #[test]
    fn hour_tick_index_near_full_circle() {
        // 359.9° → 11 号时刻度（不满一圈不回 0）。
        assert_eq!(DialGeometry::hour_tick_index(35_990), 11);
        assert_eq!(DialGeometry::sec_tick_index(35_990), 59);
    }

    #[test]
    fn layer_plans_disjoint() {
        let d = LayerPlan::digital();
        let a = LayerPlan::analog(true);
        assert!(d.face && !d.hour && !d.minute && !d.second);
        assert!(a.face && a.hour && a.minute && a.second);
    }
}
