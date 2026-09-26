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
