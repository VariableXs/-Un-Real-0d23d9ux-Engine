//! mech_thermal — 热节流状态机本体（AI-K1 深化批次五 · F048）。
//!
//! 主册依据：
//! - F048【设计细节】「温度联动」——域内有温度上限信号（temp_over_limit
//!   粘滞），但**多档热状态的进出算法**缺席：几个档位、跨越迟滞怎么算、
//!   抖动怎么防、critical 怎么粘住。本件按 Linux thermal trip point 语义
//!   实现：normal/warm/hot/critical 四档、进入需连续 2 次采样确认（防
//!   单点毛刺）、退出需低于 (trip − hysteresis)（防临界振荡）、critical
//!   触发即粘滞（唯一出口是复位）、各档频率封顶表。
//! - 锚点：F048「性能档与功耗联动」的热侧执行机构；F047 预算约束的
//!   环境降级路径（热了就降频，预算表随之收紧）。
//! - 零堆、零浮点（温度用 0.1°C 定点整数）。

// ---------------------------------------------------------------------------
// 1. 档位与参数
// ---------------------------------------------------------------------------

/// 温度单位 0.1°C（例：700 = 70.0°C）。
pub type DegaC = i32;

/// 各档进入阈值（升序）与退出迟滞。
pub const TRIP_WARM: DegaC = 700; // 70.0°C
pub const TRIP_HOT: DegaC = 800; // 80.0°C
pub const TRIP_CRITICAL: DegaC = 850; // 85.0°C
pub const HYSTERESIS: DegaC = 50; // 5.0°C 退出迟滞
/// 进入确认：连续 N 次采样越过阈值才生效（防毛刺）。
pub const CONFIRM_SAMPLES: u32 = 2;

/// 热档位（封顶档指 governor 频率表的最大允许档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleState {
    /// 全速。
    Normal,
    /// 封顶到档 4。
    Warm,
    /// 封顶到档 2。
    Hot,
    /// 关机请求（粘滞——只有复位能离开）。
    Critical,
}

impl ThrottleState {
    /// 该档允许的最高 governor 频率档（0..=5）。
    pub fn freq_cap(&self) -> usize {
        match self {
            ThrottleState::Normal => 5,
            ThrottleState::Warm => 4,
            ThrottleState::Hot => 2,
            ThrottleState::Critical => 0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ThrottleState::Normal => "normal",
            ThrottleState::Warm => "warm",
            ThrottleState::Hot => "hot",
            ThrottleState::Critical => "critical",
        }
    }
}

// ---------------------------------------------------------------------------
// 2. 状态机
// ---------------------------------------------------------------------------

/// 热节流状态机（每温度区实例化一份）。
pub struct ThermalGov {
    pub state: ThrottleState,
    /// 升档确认计数（连续越线采样数）。
    up_streak: u32,
    /// 降档确认计数（连续回落采样数）。
    down_streak: u32,
    /// critical 是否已触发（粘滞）。
    pub critical_fired: bool,
    /// 统计面：档位切换次数（normal↔warm↔hot；critical 单向）。
    pub transitions: u32,
}

impl ThermalGov {
    pub const fn new() -> Self {
        ThermalGov {
            state: ThrottleState::Normal,
            up_streak: 0,
            down_streak: 0,
            critical_fired: false,
            transitions: 0,
        }
    }

    fn next_up(&self) -> Option<ThrottleState> {
        match self.state {
            ThrottleState::Normal => Some(ThrottleState::Warm),
            ThrottleState::Warm => Some(ThrottleState::Hot),
            ThrottleState::Hot => Some(ThrottleState::Critical),
            ThrottleState::Critical => None,
        }
    }

    fn next_down(&self) -> Option<ThrottleState> {
        match self.state {
            ThrottleState::Normal => None,
            ThrottleState::Warm => Some(ThrottleState::Normal),
            ThrottleState::Hot => Some(ThrottleState::Warm),
            ThrottleState::Critical => None, // 粘滞
        }
    }

    fn trip_of(&self, s: ThrottleState) -> DegaC {
        match s {
            ThrottleState::Warm => TRIP_WARM,
            ThrottleState::Hot => TRIP_HOT,
            ThrottleState::Critical => TRIP_CRITICAL,
            ThrottleState::Normal => 0,
        }
    }

    /// 采样一次温度（0.1°C 定点），返回当前档。
    pub fn sample(&mut self, temp: DegaC) -> ThrottleState {
        if self.state == ThrottleState::Critical {
            // 粘滞：任何输入不再离开。
            self.critical_fired = true;
            return self.state;
        }

        // 升档路径：连续 CONFIRM_SAMPLES 次越过下一档 trip。
        let up_trip = self.next_up().map(|s| self.trip_of(s));
        if let Some(t) = up_trip {
            if temp >= t {
                self.up_streak += 1;
                if self.up_streak >= CONFIRM_SAMPLES {
                    if let Some(next) = self.next_up() {
                        self.state = next;
                        self.transitions += 1;
                        if next == ThrottleState::Critical {
                            self.critical_fired = true;
                        }
                        self.up_streak = 0;
                        self.down_streak = 0;
                    }
                }
                return self.state;
            }
        }
        self.up_streak = 0;

        // 降档路径：连续 CONFIRM_SAMPLES 次低于 (本档 trip − 迟滞)。
        if self.state != ThrottleState::Normal {
            let exit_at = self.trip_of(self.state) - HYSTERESIS;
            if temp < exit_at {
                self.down_streak += 1;
                if self.down_streak >= CONFIRM_SAMPLES {
                    if let Some(next) = self.next_down() {
                        self.state = next;
                        self.transitions += 1;
                        self.down_streak = 0;
                        self.up_streak = 0;
                    }
                }
                return self.state;
            }
            self.down_streak = 0;
        }
        self.state
    }

    /// 复位（仅 critical 粘滞的合法出口；模拟重启）。
    pub fn reset(&mut self) {
        *self = ThermalGov::new();
    }
}

// ---------------------------------------------------------------------------
// 3. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_thermal");

    // 1) 确认进入：单次毛刺不升档，回落重置后须重新数满两次。
    {
        let mut t = ThermalGov::new();
        t.sample(TRIP_WARM + 10);
        let after_spike = t.state;
        t.sample(TRIP_WARM - 200); // 回落重置 streak
        t.sample(TRIP_WARM + 10); // streak=1
        let after_one = t.state;
        t.sample(TRIP_WARM + 10); // streak=2 → Warm
        let after_two = t.state;
        cs.add(
            "th_debounce",
            after_spike == ThrottleState::Normal
                && after_one == ThrottleState::Normal
                && after_two == ThrottleState::Warm,
            "",
        );
    }

    // 2) 迟滞防振：先两连越线进入 Warm，之后 690/705 永久交替——
    //    阈值持续被穿越数十次，但迟滞带（退出线 650）+ 确认期使档位
    //    只进一次、不再翻动（裸阈值每两拍翻一次的对照面）。
    {
        let mut t = ThermalGov::new();
        t.sample(705);
        t.sample(705); // 确认期满足 → Warm
        let mut flips = 0u32;
        let mut crossings = 0u32;
        let mut last = t.state;
        let mut prev_temp = 0;
        for i in 0..40i32 {
            let temp = if i % 2 == 0 { 690 } else { 705 };
            if (prev_temp < TRIP_WARM) != (temp < TRIP_WARM) {
                crossings += 1;
            }
            prev_temp = temp;
            let s = t.sample(temp);
            if s != last {
                flips += 1;
                last = s;
            }
        }
        cs.add(
            "th_hysteresis_no_flap",
            crossings >= 18 && flips == 0 && last == ThrottleState::Warm,
            "",
        );
    }

    // 3) critical 粘滞：逐档确认爬到 critical 后，降温也不离开，复位才回。
    {
        let mut t = ThermalGov::new();
        // 860 连续 6 采样：Warm(2)→Hot(4)→Critical(6)。
        for _ in 0..6 {
            t.sample(TRIP_CRITICAL + 10);
        }
        let fired = t.critical_fired && t.state == ThrottleState::Critical;
        t.sample(200); // 大幅降温
        let stays = t.state == ThrottleState::Critical;
        t.reset();
        let back = t.state == ThrottleState::Normal && !t.critical_fired;
        cs.add("th_critical_sticky", fired && stays && back, "");
    }

    // 4) 封顶单调：档位越高频率封顶越紧。
    {
        let caps = [
            ThrottleState::Normal.freq_cap(),
            ThrottleState::Warm.freq_cap(),
            ThrottleState::Hot.freq_cap(),
            ThrottleState::Critical.freq_cap(),
        ];
        cs.add("th_caps_monotonic", caps[0] > caps[1] && caps[1] > caps[2] && caps[2] > caps[3], "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 4. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_two_samples_to_enter() {
        let mut t = ThermalGov::new();
        assert_eq!(t.sample(TRIP_WARM + 1), ThrottleState::Normal);
        assert_eq!(t.sample(TRIP_WARM + 1), ThrottleState::Warm);
        // 折返重置后必须重新数。
        t.sample(TRIP_WARM - 300);
        assert_eq!(t.sample(TRIP_WARM + 1), ThrottleState::Normal);
        assert_eq!(t.sample(TRIP_WARM + 1), ThrottleState::Warm);
    }

    #[test]
    fn exit_requires_below_trip_minus_hysteresis() {
        let mut t = ThermalGov::new();
        t.sample(TRIP_WARM + 1);
        t.sample(TRIP_WARM + 1);
        assert_eq!(t.state, ThrottleState::Warm);
        // 降到 trip 以下但未到退出线 → 不降。
        t.sample(TRIP_WARM - 10);
        t.sample(TRIP_WARM - 10);
        assert_eq!(t.state, ThrottleState::Warm);
        // 降到退出线下连续两次 → 降。
        t.sample(TRIP_WARM - HYSTERESIS - 1);
        assert_eq!(t.state, ThrottleState::Warm);
        t.sample(TRIP_WARM - HYSTERESIS - 1);
        assert_eq!(t.state, ThrottleState::Normal);
    }

    #[test]
    fn full_ladder_up_and_down() {
        let mut t = ThermalGov::new();
        // 逐档爬升（每档两次确认）。
        t.sample(TRIP_WARM + 1);
        t.sample(TRIP_WARM + 1);
        assert_eq!(t.state, ThrottleState::Warm);
        t.sample(TRIP_HOT + 1);
        t.sample(TRIP_HOT + 1);
        assert_eq!(t.state, ThrottleState::Hot);
        t.sample(TRIP_CRITICAL + 1);
        t.sample(TRIP_CRITICAL + 1);
        assert_eq!(t.state, ThrottleState::Critical);
        assert_eq!(t.transitions, 3);
        // 粘滞后复位重来，逐档下降。
        t.reset();
        t.sample(TRIP_HOT + 1);
        t.sample(TRIP_HOT + 1);
        t.sample(TRIP_WARM - HYSTERESIS - 1);
        t.sample(TRIP_WARM - HYSTERESIS - 1);
        t.sample(0);
        t.sample(0);
        assert_eq!(t.state, ThrottleState::Normal);
    }

    #[test]
    fn oscillation_around_trip_does_not_flap() {
        // 前置两连越线进 Warm；随后 690/705 永久交替：无迟滞的裸阈值
        // 会在边界附近反复翻动，本状态机因退出线 650（迟滞）永不退出、
        // 因确认期永不升 Hot——零翻动。
        let mut t = ThermalGov::new();
        t.sample(705);
        t.sample(705);
        assert_eq!(t.state, ThrottleState::Warm);
        let mut flips = 0;
        for i in 0..40 {
            let temp = if i % 2 == 0 { 690 } else { 705 };
            let s = t.sample(temp);
            if s != ThrottleState::Warm {
                flips += 1;
            }
        }
        assert_eq!(flips, 0);
        // 对照：同样的交替但无前置进入 → streak 永远 1 → 永不升档。
        let mut t2 = ThermalGov::new();
        for i in 0..40 {
            let temp = if i % 2 == 0 { 690 } else { 705 };
            t2.sample(temp);
        }
        assert_eq!(t2.state, ThrottleState::Normal);
    }

    #[test]
    fn critical_fires_even_without_graduation() {
        // 温度从未在 warm/hot 区间停留过，一路 950：每两拍升一档，
        // 第 6 拍到 critical（逐档爬升语义——跳档不允许，防状态跳变毛刺）。
        let mut t = ThermalGov::new();
        for i in 1..=6 {
            t.sample(TRIP_CRITICAL + 100);
            let expect = [
                ThrottleState::Normal,
                ThrottleState::Warm,
                ThrottleState::Warm,
                ThrottleState::Hot,
                ThrottleState::Hot,
                ThrottleState::Critical,
            ][(i - 1) as usize];
            assert_eq!(t.state, expect, "第 {} 拍", i);
        }
        assert!(t.critical_fired);
        // 之后永不离场。
        for temp in [0, 100, 500, 700, 799] {
            t.sample(temp);
            assert_eq!(t.state, ThrottleState::Critical);
        }
    }

    #[test]
    fn caps_match_expected_table() {
        assert_eq!(ThrottleState::Normal.freq_cap(), 5);
        assert_eq!(ThrottleState::Warm.freq_cap(), 4);
        assert_eq!(ThrottleState::Hot.freq_cap(), 2);
        assert_eq!(ThrottleState::Critical.freq_cap(), 0);
    }
}
