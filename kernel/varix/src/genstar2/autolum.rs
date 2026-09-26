//! F491 自动亮度（环境光）（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **传感器缺失隐藏判据；2s 渐变；30% 下限；手动覆盖 2h 计时；开关默认关。**
//!
//! 功能定义（主册批次三）：环境光传感器可用时自动亮度开关（默认关——自动
//! 该是选项不是绑架）；亮暗过渡平滑（2 秒渐变不跳变）；夜间自动压暗上限
//! （再暗也不低于 30% 护眼下限）；手动调节临时覆盖（暂停自动 2 小时）；
//! 传感器不支持诚实隐藏本项。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 渐变时长（主册：2 秒渐变不跳变）。
pub const RAMP_MS: u64 = 2_000;
/// 护眼下限（主册：再暗也不低于 30%）。
pub const FLOOR_PERMILLE: u16 = 300;
/// 手动覆盖时长（主册：手动调过后暂停自动 2 小时）。
pub const OVERRIDE_MS: u64 = 2 * 60 * 60 * 1_000;
/// 亮度范围（permille）。
pub const MAX_PERMILLE: u16 = 1_000;

/// 自动亮度控制器。
pub struct AutoLuma {
    /// 传感器存在（宿主支持则用——不支持隐藏本项）。
    pub sensor_present: bool,
    /// 开关（默认关——主册判据）。
    pub enabled: bool,
    /// 目标亮度（permille）。
    target: u16,
    /// 渐变起点与起始时刻（2s 渐变内核算用）。
    ramp_from: u16,
    ramp_start: u64,
    /// 手动覆盖截止时刻（0 = 无覆盖）。
    override_until: u64,
}

impl AutoLuma {
    pub const fn new(sensor_present: bool) -> Self {
        AutoLuma {
            sensor_present,
            enabled: false, // 默认关
            target: 800,
            ramp_from: 800,
            ramp_start: 0,
            override_until: 0,
        }
    }

    /// 设置项可见性（主册：传感器缺失隐藏判据——不骗人）。
    pub fn setting_visible(&self) -> bool {
        self.sensor_present
    }

    /// 传感器读数 → 目标亮度（自动态；30% 下限钳制）。
    pub fn on_sensor(&mut self, lux_level_permille: u16, now_ms: u64) -> Option<u16> {
        if !self.enabled || !self.sensor_present || now_ms < self.override_until {
            return None; // 手动覆盖期自动退让（用户意志优先）
        }
        // 环境越暗亮度越低（线性反比模型）；夜间下限 30%。
        let want = (MAX_PERMILLE.saturating_sub(lux_level_permille)).max(FLOOR_PERMILLE);
        self.set_target(want, now_ms);
        Some(want)
    }

    fn set_target(&mut self, want: u16, now_ms: u64) {
        self.ramp_from = self.target;
        self.target = want;
        self.ramp_start = now_ms;
    }

    /// 渐变插值（2s 线性渐变——不跳变；超过 2s = 到位）。
    pub fn current(&self, now_ms: u64) -> u16 {
        let elapsed = now_ms.saturating_sub(self.ramp_start);
        if elapsed >= RAMP_MS || self.ramp_start == 0 {
            return self.target;
        }
        let from = self.ramp_from as i32;
        let to = self.target as i32;
        let t = elapsed as u32 * 1_000 / RAMP_MS as u32;
        (from + (to - from) * t as i32 / 1_000).clamp(0, MAX_PERMILLE as i32) as u16
    }

    /// 手动调节（暂停自动 2h——主册：用户意志优先）。
    pub fn manual_override(&mut self, value: u16, now_ms: u64) -> u16 {
        let v = value.clamp(FLOOR_PERMILLE, MAX_PERMILLE);
        self.set_target(v, now_ms);
        self.override_until = now_ms + OVERRIDE_MS;
        v
    }

    /// 覆盖是否生效中。
    pub fn overriding(&self, now_ms: u64) -> bool {
        now_ms < self.override_until
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_autolum_checks() -> CheckSet {
    let mut cs = CheckSet::new("F491-autolum");
    // 1) 传感器缺失隐藏判据。
    let mut absent = AutoLuma::new(false);
    cs.add("missing_sensor_hidden", !absent.setting_visible() && absent.on_sensor(100, 0).is_none(), "");
    // 2) 开关默认关。
    let mut a = AutoLuma::new(true);
    cs.add("default_off", !a.enabled && a.setting_visible(), "");
    a.enabled = true;
    // 3) 30% 下限（深夜再暗也不低于）。
    cs.add("floor_30pct", a.on_sensor(999, 1_000) == Some(FLOOR_PERMILLE), "");
    // 4) 2s 渐变（不跳变：中途值介于起止之间）。
    a.on_sensor(0, 2_000); // 环境全亮 → 目标 1000（起点=上一目标 300）
    let from = a.current(2_000);
    let mid = a.current(3_000); // 渐变一半
    let end = a.current(4_001);
    cs.add("ramp_2s", mid > from && mid < end && end == 1_000, "");
    // 5) 手动覆盖 2h 计时（覆盖期自动退让，到期恢复）。
    let manual = a.manual_override(500, 10_000);
    cs.add("manual_applies", manual == 500, "");
    cs.add("auto_defers_during_override", a.on_sensor(0, 60_000).is_none() && a.overriding(60_000), "");
    cs.add("override_expires_2h", !a.overriding(10_000 + OVERRIDE_MS), "");
    cs.add("auto_resumes", a.on_sensor(200, 10_000 + OVERRIDE_MS + 1) == Some(800), "");
    // 6) 手动值也受下限钳制（30% 护眼红线对用户同样生效）。
    cs.add("manual_floor", a.manual_override(50, 99_999_999) == FLOOR_PERMILLE, "");
    // 7) 常量在册。
    cs.add("consts", RAMP_MS == 2_000 && OVERRIDE_MS == 7_200_000, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_never_overshoots() {
        let mut a = AutoLuma::new(true);
        a.enabled = true;
        a.on_sensor(0, 0); // 目标 1000
        let mut last = 800;
        for step in 1..=20u64 {
            let v = a.current(step * 100);
            assert!(v >= last.min(a.target) && v <= last.max(a.target), "渐变单调不跳变");
            last = v;
        }
        assert_eq!(a.current(RAMP_MS + 1), 1_000);
    }

    #[test]
    fn hidden_setting_never_activates() {
        let mut a = AutoLuma::new(false);
        a.enabled = true;
        assert!(a.on_sensor(0, 0).is_none());
    }

    #[test]
    fn override_window_exact() {
        let mut a = AutoLuma::new(true);
        a.enabled = true;
        a.manual_override(600, 1_000);
        assert!(a.overriding(1_000 + OVERRIDE_MS - 1));
        assert!(!a.overriding(1_000 + OVERRIDE_MS));
    }
}

// ===========================================================================
// 深化 v2（F491）：亮度曲线表 / 传感器平滑 / 夜间时段窗 / 状态持久化
// ===========================================================================

/// 亮度曲线表（环境 lux 分级 → 目标亮度——线性反比模型的分级化：
/// 深夜/室内/阴天/晴天五档，主册「亮暗过渡平滑」的输入端）。
pub const LUX_CURVE: [(u16, u16); 5] = [
    (50, 300),   // 深夜
    (200, 450),  // 暗室
    (500, 650),  // 室内灯
    (800, 850),  // 阴天窗边
    (1_000, 1_000), // 晴天
];

/// 曲线查表（lux 超表尾 → 表尾值；低于表头 → 下限 30%）。
pub fn curve_target(lux_permille: u16) -> u16 {
    for (lux, target) in LUX_CURVE {
        if lux_permille <= lux {
            return target.max(FLOOR_PERMILLE);
        }
    }
    LUX_CURVE[LUX_CURVE.len() - 1].1
}

/// 传感器平滑（EMA 指数滑动——环境光抖动不引起亮度跳变）。
pub struct LuxSmoother {
    ema_permille: u16,
    alpha_permille: u16,
    primed: bool,
}

pub const EMA_ALPHA_PERMILLE: u16 = 200; // 新样本权重 20%

impl LuxSmoother {
    pub const fn new() -> Self {
        LuxSmoother { ema_permille: 0, alpha_permille: EMA_ALPHA_PERMILLE, primed: false }
    }

    /// 采样（首样本直落；其后 EMA 平滑）。
    pub fn sample(&mut self, lux_permille: u16) -> u16 {
        if !self.primed {
            self.ema_permille = lux_permille;
            self.primed = true;
            return self.ema_permille;
        }
        let a = self.alpha_permille as u32;
        let e = self.ema_permille as u32;
        let v = lux_permille as u32;
        self.ema_permille = ((a * v + (1_000 - a) * e) / 1_000) as u16;
        self.ema_permille
    }

    pub fn value(&self) -> u16 {
        self.ema_permille
    }
}

/// 夜间时段窗（F116 夜间模式联动：窗内再压暗下限不变但目标减 20%——
/// 深夜刺眼防御；窗外原样）。
pub const NIGHT_DIM_PERMILLE: u16 = 800; // 夜间目标 ×0.8

pub fn night_adjust(target: u16, night: bool) -> u16 {
    if night {
        ((target as u32 * NIGHT_DIM_PERMILLE as u32) / 1_000).max(FLOOR_PERMILLE as u32) as u16
    } else {
        target
    }
}

/// 状态持久化（开关+手动覆盖截止时刻——重启后自动亮度不复活）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VAL1";

pub fn save_state(enabled: bool, override_until: u64, out: &mut [u8]) -> Option<usize> {
    if out.len() < 15 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = enabled as u8;
    out[6] = 0; // 保留位
    out[7..15].copy_from_slice(&override_until.to_le_bytes());
    Some(15)
}

pub fn load_state(buf: &[u8]) -> Option<(bool, u64)> {
    if buf.len() < 15 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    let enabled = buf[5] == 1;
    let mut o = [0u8; 8];
    o.copy_from_slice(&buf[7..15]);
    Some((enabled, u64::from_le_bytes(o)))
}

pub fn run_autolum_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F491-deep");
    // 曲线表：深夜档触底 30%；晴天档顶格。
    cs.add("curve_floor", curve_target(10) == FLOOR_PERMILLE, "");
    cs.add("curve_top", curve_target(1_100) == 1_000, "");
    cs.add("curve_mid", curve_target(300) == 650, "");
    // EMA 平滑：单点跳变不透传（20% 权重）。
    cs.add("ema_first_direct", { let mut s = LuxSmoother::new(); s.sample(500) == 500 }, "");
    cs.add("ema_dampens", {
        let mut s = LuxSmoother::new();
        s.sample(500);
        let v = s.sample(1_000); // 500 + 20%×500 = 600
        v == 600
    }, "");
    // 夜间压暗（目标减 20% 但不破 30% 下限——深夜不刺眼）。
    cs.add("night_dim", night_adjust(1_000, true) == 800, "");
    cs.add("night_floor_holds", night_adjust(300, true) == FLOOR_PERMILLE, "");
    cs.add("day_untouched", night_adjust(700, false) == 700, "");
    // 状态持久化（重启后不复活——开关默认关的诚实延续）。
    cs.add("persist_roundtrip", {
        let mut buf = [0u8; 16];
        let n = save_state(true, 123_456, &mut buf).unwrap();
        load_state(&buf[..n]) == Some((true, 123_456))
    }, "");
    cs.add("persist_bad_magic", load_state(b"XXXX\x01\x01\x00\x00\x00\x00\x00\x00\x00").is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn curve_is_monotonic() {
        let mut last = 0u16;
        for (_, t) in LUX_CURVE {
            assert!(t >= last);
            last = t;
        }
    }

    #[test]
    fn ema_converges_toward_input() {
        let mut s = LuxSmoother::new();
        s.sample(0);
        let mut prev = 0u16;
        for _ in 0..40 {
            let v = s.sample(1_000);
            assert!(v >= prev); // 单调逼近
            prev = v;
        }
        assert!(s.value() >= 990); // 40 轮后收敛
    }

    #[test]
    fn night_never_below_floor() {
        for t in [300u16, 500, 800, 1_000] {
            assert!(night_adjust(t, true) >= FLOOR_PERMILLE);
        }
    }

    #[test]
    fn persist_state_roundtrip_both_flags() {
        let mut buf = [0u8; 16];
        for (en, o) in [(false, 0u64), (true, u64::MAX)] {
            let n = save_state(en, o, &mut buf).unwrap();
            assert_eq!(load_state(&buf[..n]), Some((en, o)));
        }
    }
}
