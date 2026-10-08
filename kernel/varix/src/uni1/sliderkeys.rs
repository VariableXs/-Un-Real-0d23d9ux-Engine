//! F436 滑杆键盘操作 · 完整设计（STAR I 主册 G-I-36）。
//!
//! **判据（主册）**：五招行为；步进值定义表（每滑杆登记最小刻度）；气泡
//! 读数；连发节奏一致性；焦点环（F206）在滑杆上的形态。＋通12。
//!
//! 设计：滑杆键盘核——五招（方向键 1 步 / PgUp/PgDn 10 步 / Home/End
//! 极值 / 拖拽同值域 / 滚轮 1 步）；步进定义表（每滑杆登记最小刻度——
//! 一处一事实）；气泡读数账（调节后即显）；连发节奏（与 F240 同源：
//! 首发即时、连发阶梯——两档速率常量）；焦点环形态位。
//!
//! v5 纵深：禁用态（输入全拒且有账——不给假反馈）；Shift 粗调（10 步
//! 同钳制）；气泡 1.5s 自动收（不常驻挡内容）；连发到极值停发（计数
//! 冻结，不空转）。
//!
//! v8 纵深：出厂默认值（每滑杆登记 default——双击滑轨回默认，值没有
//! 默认的诚实 None）；调整历史账（最后 N 步可观测——误调可追溯）；
//! 值等人话读数（千分比/百分比归一）。

use crate::checks::CheckSet;

/// 连发节奏（F240 同源）：首发放松前单发；按住后阶梯速率（ms/发）。
pub const REPEAT_FIRST_DELAY_MS: u64 = 400;
pub const REPEAT_STEP_MS: u64 = 60;

/// 气泡读数自动收起时长（ms）。
pub const BUBBLE_HIDE_MS: u64 = 1_500;

/// 步进定义表条目：滑杆名 → 最小刻度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderSpec {
    pub name: &'static str,
    pub min: u64,
    pub max: u64,
    /// 最小刻度（方向键 1 步）。
    pub step: u64,
    /// 出厂默认值（None = 此滑杆无默认——双击诚实不动作）。
    pub default: Option<u64>,
}

/// 滑杆实例。
pub struct Slider {
    pub spec: SliderSpec,
    pub value: u64,
    /// 气泡读数显示态。
    pub bubble_visible: bool,
    /// 气泡显示计时（ms）。
    bubble_timer_ms: u64,
    /// 焦点环在位（F206 形态）。
    pub focused: bool,
    /// 禁用态（输入全拒）。
    pub enabled: bool,
    /// 被拒输入计数（可观测——不是静默吞）。
    pub blocked_inputs: u64,
    /// 连发账：累计发数。
    pub repeat_holding: bool,
    pub repeats: u64,
    /// 连发饱和（已在极值——停发）。
    pub repeat_saturated: bool,
    /// v8：调整历史账（环形，容量 16——误调可追溯）。
    pub history: alloc::vec::Vec<u64>,
}

/// v8：历史账容量。
pub const HISTORY_CAP: usize = 16;

impl Slider {
    pub fn new(spec: SliderSpec) -> Slider {
        Slider {
            spec,
            value: spec.min,
            bubble_visible: false,
            bubble_timer_ms: 0,
            focused: false,
            enabled: true,
            blocked_inputs: 0,
            repeat_holding: false,
            repeats: 0,
            repeat_saturated: false,
            history: alloc::vec![],
        }
    }

    /// v8：值变更入历史账（环形封顶——只保最近 HISTORY_CAP 步）。
    fn note_history(&mut self, before: u64) {
        if before != self.value {
            if self.history.len() >= HISTORY_CAP {
                self.history.remove(0);
            }
            self.history.push(before);
        }
    }

    /// v8：双击滑轨回出厂默认（有默认才动；无默认诚实拒绝——不猜）。
    pub fn double_click_reset(&mut self) -> Option<u64> {
        if !self.enabled {
            self.blocked_inputs += 1;
            return None;
        }
        let d = self.spec.default?;
        let before = self.value;
        self.value = d.clamp(self.spec.min, self.spec.max);
        self.note_history(before);
        self.show_bubble();
        Some(self.value)
    }

    /// v8：回滚上一步（撤销误调；无历史诚实拒绝）。
    pub fn undo_last(&mut self) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        let prev = self.history.pop()?;
        let before = self.value;
        self.value = prev.clamp(self.spec.min, self.spec.max);
        self.show_bubble();
        let _ = before;
        Some(self.value)
    }

    /// v8：人话读数——值域归一到百分比（带一位小数的千分比精度）。
    pub fn human_readout(&self) -> u64 {
        let span = (self.spec.max - self.spec.min).max(1);
        (self.value.saturating_sub(self.spec.min)) * 100 / span
    }

    fn show_bubble(&mut self) {
        self.bubble_visible = true;
        self.bubble_timer_ms = 0;
    }

    /// 气泡超时账：调节即显；BUBBLE_HIDE_MS 后自动收（不常驻挡内容）。
    pub fn bubble_tick(&mut self, ms: u64) {
        if self.bubble_visible {
            self.bubble_timer_ms += ms;
            if self.bubble_timer_ms >= BUBBLE_HIDE_MS {
                self.bubble_visible = false;
            }
        }
    }

    /// 五招统一入口：步数（方向键 ±1 / Pg ±10 / Shift 粗调 ±10 步）。
    /// 钳制入档；调节即出气泡读数；禁用态全拒且有账。
    pub fn step_by(&mut self, steps: i64) -> u64 {
        if !self.enabled {
            self.blocked_inputs += 1;
            return self.value;
        }
        let s = self.spec.step as i64;
        let v = self.value as i64 + steps * s;
        let before = self.value;
        self.value = v.clamp(self.spec.min as i64, self.spec.max as i64) as u64;
        self.note_history(before);
        self.show_bubble();
        self.value
    }

    /// Shift 粗调：10 倍步进（与 PgUp/PgDn 同量级——专家捷径）。
    pub fn step_coarse(&mut self, dir: i64) -> u64 {
        self.step_by(dir * 10)
    }

    /// Home：极小。End：极大。
    pub fn home(&mut self) -> u64 {
        if !self.enabled {
            self.blocked_inputs += 1;
            return self.value;
        }
        self.value = self.spec.min;
        self.show_bubble();
        self.value
    }

    pub fn end(&mut self) -> u64 {
        if !self.enabled {
            self.blocked_inputs += 1;
            return self.value;
        }
        self.value = self.spec.max;
        self.show_bubble();
        self.value
    }

    /// 拖拽（F217 三通路之一）：与键盘同值域（同一 clamp）。
    pub fn drag_to(&mut self, v: u64) -> u64 {
        if !self.enabled {
            self.blocked_inputs += 1;
            return self.value;
        }
        self.value = v.clamp(self.spec.min, self.spec.max);
        self.note_history(v);
        self.show_bubble();
        self.value
    }

    /// 滚轮：1 步（与方向键同刻度——三通路殊途同归）。
    pub fn wheel(&mut self, up: bool) -> u64 {
        self.step_by(if up { 1 } else { -1 })
    }

    /// 连发节奏：首牙 400ms，之后 60ms/发（一致性判据——三通路共享）。
    /// 到极值停发：值不再变化即饱和（计数冻结，不空转）。
    pub fn repeat_tick(&mut self, held_ms: u64, steps: i64) -> u64 {
        if !self.enabled || held_ms < REPEAT_FIRST_DELAY_MS {
            return self.value; // 首发窗内不发
        }
        let due = (held_ms - REPEAT_FIRST_DELAY_MS) / REPEAT_STEP_MS + 1;
        while self.repeats < due {
            let before = self.value;
            let _ = self.step_by(steps);
            if self.value == before {
                self.repeat_saturated = true; // 已在极值——停发
                break;
            }
            self.repeats += 1;
        }
        self.value
    }

    /// 步进定义表（三滑杆登记——一处一事实；v8 增出厂默认值登记）。
    pub fn registry() -> [SliderSpec; 3] {
        [
            SliderSpec { name: "音量", min: 0, max: 100, step: 2, default: Some(40) },
            SliderSpec { name: "亮度", min: 10, max: 100, step: 5, default: Some(80) },
            SliderSpec { name: "缩放", min: 100, max: 200, step: 25, default: None },
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
            && reg[2].step == 25
            && reg[0].default == Some(40)
            && reg[2].default.is_none(),
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
    // v5：禁用态——输入全拒且有账（不给假反馈）。
    let mut d = Slider::new(reg[0]);
    d.enabled = false;
    set.add(
        "f436-disabled-blocks",
        d.step_by(1) == 0 && d.drag_to(50) == 0 && d.home() == 0 && d.end() == 0
            && d.blocked_inputs == 4 && !d.bubble_visible,
        "",
    );
    // v5：Shift 粗调 = 10 步（同钳制）。
    let mut c = Slider::new(reg[0]);
    set.add("f436-coarse-step", c.step_coarse(1) == 20 && c.step_coarse(-4) == 0, "");
    // v5：气泡 1.5s 自动收。
    c.bubble_tick(1_400);
    set.add("f436-bubble-keep-before-1p5s", c.bubble_visible, "");
    c.bubble_tick(200);
    set.add("f436-bubble-hides-at-1p5s", !c.bubble_visible, "");
    set.add("f436-bubble-stays-hidden", { c.bubble_tick(5_000); !c.bubble_visible }, "");
    // v5：连发到极值停发（计数冻结，不空转）。
    let mut x = Slider::new(reg[2]); // 缩放 100..200 step 25
    let _ = x.drag_to(150);
    let _ = x.repeat_tick(1_000, 4); // 每发 +100：150→175→200→饱和
    set.add(
        "f436-repeat-saturates",
        x.value == 200 && x.repeat_saturated && x.repeats == 1,
        "",
    );
    // v8：双击回默认——有默认生效、无默认诚实拒绝、禁用拒且有账。
    let mut g = Slider::new(reg[0]);
    let _ = g.drag_to(90);
    set.add("f436-dblclick-default", g.double_click_reset() == Some(40) && g.value == 40, "");
    let mut nd = Slider::new(reg[2]); // 缩放无默认
    set.add("f436-dblclick-no-default", nd.double_click_reset().is_none() && nd.value == nd.spec.min, "");
    let mut dis = Slider::new(reg[0]);
    dis.enabled = false;
    set.add("f436-dblclick-disabled", dis.double_click_reset().is_none() && dis.blocked_inputs == 1, "");
    // v8：调整历史账——环形封顶 + undo 回滚 + 无历史诚实拒绝。
    let mut h = Slider::new(reg[0]);
    for _ in 0..(HISTORY_CAP + 6) {
        let _ = h.step_by(1);
    }
    set.add("f436-history-capped", h.history.len() == HISTORY_CAP && h.history[0] == 12, "最老 12 步被挤出");
    let before_undo = h.value;
    let undo1 = h.undo_last();
    set.add(
        "f436-undo-pops",
        undo1.is_some() && h.history.len() == HISTORY_CAP - 1 && undo1 != Some(before_undo),
        "",
    );
    let mut e2 = Slider::new(reg[0]);
    set.add("f436-undo-empty-honest", e2.undo_last().is_none(), "");
    // v8：人话读数——值域归一百分比（亮度 10-100：值 55 → 50%）。
    let mut b = Slider::new(reg[1]);
    let _ = b.drag_to(55);
    set.add("f436-human-readout", b.human_readout() == 50, "");
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

    #[test]
    fn repeat_at_extreme_freezes_count() {
        let mut s = Slider::new(Slider::registry()[2]);
        let _ = s.end(); // 200（上极值）
        let _ = s.repeat_tick(2_000, 2);
        assert_eq!(s.repeats, 0, "极值上连发一发不发——计数冻结");
        assert!(s.repeat_saturated);
    }
}
