//! F436 滑杆键盘操作 · 完整设计（STAR I 主册 G-I-36）。
//!
//! **判据（主册）**：五招行为；步进值定义表（每滑杆登记最小刻度）；气泡
//! 读数；连发节奏一致性；焦点环（F206）在滑杆上的形态。＋通12。
//!
//! 设计：滑杆键盘核——五招（方向键 1 步 / PgUp/PgDn 10 步 / Home/End
//! 极值 / 拖拽同值域 / 滚轮 1 步）；步进定义表（每滑杆登记最小刻度——
//! 一处一事实）；气泡读数账（调节后即显）；连发节奏（与 F240 同源：
//! 首发即时、连发阶梯——两档速率常量）；焦点环形态位。

use crate::checks::CheckSet;

/// 连发节奏（F240 同源）：首发放松前单发；按住后阶梯速率（ms/发）。
pub const REPEAT_FIRST_DELAY_MS: u64 = 400;
pub const REPEAT_STEP_MS: u64 = 60;

/// 步进定义表条目：滑杆名 → 最小刻度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderSpec {
    pub name: &'static str,
    pub min: u64,
    pub max: u64,
    /// 最小刻度（方向键 1 步）。
    pub step: u64,
}

/// 滑杆实例。
pub struct Slider {
    pub spec: SliderSpec,
    pub value: u64,
    /// 气泡读数显示态。
    pub bubble_visible: bool,
    /// 焦点环在位（F206 形态）。
    pub focused: bool,
    /// 连发账：首牙时刻与累计发数。
    pub repeat_holding: bool,
    pub repeats: u64,
}

impl Slider {
    pub fn new(spec: SliderSpec) -> Slider {
        Slider {
            spec,
            value: spec.min,
            bubble_visible: false,
            focused: false,
            repeat_holding: false,
            repeats: 0,
        }
    }

    /// 五招统一入口：步数（方向键 ±1 / Pg ±10 / Home -∞ / End +∞）。
    /// 钳制入档；调节即出气泡读数。
    pub fn step_by(&mut self, steps: i64) -> u64 {
        let s = self.spec.step as i64;
        let v = self.value as i64 + steps * s;
        self.value = v.clamp(self.spec.min as i64, self.spec.max as i64) as u64;
        self.bubble_visible = true;
        self.value
    }

    /// Home：极小。End：极大。
    pub fn home(&mut self) -> u64 {
        self.value = self.spec.min;
        self.bubble_visible = true;
        self.value
    }

    pub fn end(&mut self) -> u64 {
        self.value = self.spec.max;
        self.bubble_visible = true;
        self.value
    }

    /// 拖拽（F217 三通路之一）：与键盘同值域（同一 clamp）。
    pub fn drag_to(&mut self, v: u64) -> u64 {
        self.value = v.clamp(self.spec.min, self.spec.max);
        self.bubble_visible = true;
        self.value
    }

    /// 滚轮：1 步（与方向键同刻度——三通路殊途同归）。
    pub fn wheel(&mut self, up: bool) -> u64 {
        self.step_by(if up { 1 } else { -1 })
    }

    /// 连发节奏：首牙 400ms，之后 60ms/发（一致性判据——三通路共享）。
    pub fn repeat_tick(&mut self, held_ms: u64, steps: i64) -> u64 {
        if held_ms < REPEAT_FIRST_DELAY_MS {
            return self.value; // 首发窗内不发
        }
        let due = (held_ms - REPEAT_FIRST_DELAY_MS) / REPEAT_STEP_MS + 1;
        while self.repeats < due {
            let _ = self.step_by(steps);
            self.repeats += 1;
        }
        self.value
    }

    /// 步进定义表（三滑杆登记——一处一事实）。
    pub fn registry() -> [SliderSpec; 3] {
        [
            SliderSpec { name: "音量", min: 0, max: 100, step: 2 },
            SliderSpec { name: "亮度", min: 10, max: 100, step: 5 },
            SliderSpec { name: "缩放", min: 100, max: 200, step: 25 },
        ]
    }
}

pub fn run_sliderkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F436");
    let reg = Slider::registry();
    // 步进定义表登记在册。
    set.add(
        "f436-registry",
        reg.len() == 3
            && reg[0].step == 2
            && reg[1].step == 5
            && reg[1].min == 10
            && reg[2].step == 25,
        "",
    );
    // 五招：方向键。
    let mut s = Slider::new(reg[0]);
    s.focused = true;
    set.add("f436-arrow-step", s.step_by(1) == 2, "");
    set.add("f436-arrow-negative", s.step_by(-1) == 0, "");
    set.add("f436-arrow-floor", s.step_by(-5) == 0, "");
    // PgUp/PgDn = 10 步。
    set.add("f436-pgup", s.step_by(10) == 20, "");
    // Home/End。
    set.add("f436-end", s.end() == 100, "");
    set.add("f436-home", s.home() == 0, "");
    // 拖拽同值域。
    set.add("f436-drag-clamped", s.drag_to(150) == 100 && s.drag_to(64) == 64, "");
    // 滚轮 1 步同刻度。
    set.add("f436-wheel", s.wheel(true) == 66 && s.wheel(false) == 64, "");
    // 气泡读数：调节即显；焦点环在位。
    set.add("f436-bubble", s.bubble_visible && s.focused, "");
    // 连发节奏：400ms 首牙 + 60ms 阶梯。
    let mut r = Slider::new(reg[0]);
    r.repeat_tick(399, 1);
    set.add("f436-repeat-first-window", r.repeats == 0 && r.value == 0, "");
    r.repeat_tick(400, 1);
    set.add("f436-repeat-first-fire", r.repeats == 1 && r.value == 2, "");
    r.repeat_tick(880, 1); // (880-400)/60+1 = 9 发累计
    set.add("f436-repeat-ladder", r.repeats == 9 && r.value == 18, "");
    // 连发与单击殊途同归：18 = 9 次方向键 ×2。
    let mut m = Slider::new(reg[0]);
    for _ in 0..9 {
        let _ = m.step_by(1);
    }
    set.add("f436-paths-agree", m.value == r.value, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_floor_10() {
        let mut s = Slider::new(Slider::registry()[1]);
        s.drag_to(50);
        assert_eq!(s.step_by(-10), 10, "50-10×5≤0 → 钳亮度下限 10（F239 同源）");
        assert_eq!(s.value, 10, "亮度下限 10（F239 同源）");
    }

    #[test]
    fn zoom_step_25() {
        let mut s = Slider::new(Slider::registry()[2]);
        assert_eq!(s.step_by(2), 150);
        assert_eq!(s.end(), 200);
    }
}
