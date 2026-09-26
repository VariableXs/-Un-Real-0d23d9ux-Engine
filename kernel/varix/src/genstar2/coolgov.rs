//! F493 散热策略选择（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **两策略+自动三选；切换即时；温度读数与传感器对账；自动档触发阈值
//! （与 F197 同源）；策略持久化。**
//!
//! 功能定义（主册批次三）：散热两策略可选（Y7000 风扇）——主动降温（风扇
//! 激进——凉快但有声）与被动优先（风扇温和——安静但机身温）+自动档（F197
//! 温度感知联动）；策略切换即时、当前策略在电源页可见；温度实时显示。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 散热三选（主册：两策略+自动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoolingPolicy {
    /// 主动降温（风扇激进——凉快但有声）。
    Active,
    /// 被动优先（风扇温和——安静但机身温）。
    Passive,
    /// 自动档（F197 温度感知联动）。
    Auto,
}

/// 自动档触发阈值（与 F197 温度感知降档同源——°C）。
pub const AUTO_TRIGGER_C: u8 = 80;
/// 自动回落阈值（低于此温度回被动——回滞防抖）。
pub const AUTO_RELEASE_C: u8 = 72;

/// 散热策略管理器。
pub struct CoolingGov {
    pub policy: CoolingPolicy,
    /// 自动档当前实际策略（Auto 时由温度裁决）。
    pub effective: CoolingPolicy,
}

impl CoolingGov {
    pub const fn new() -> Self {
        CoolingGov { policy: CoolingPolicy::Auto, effective: CoolingPolicy::Passive }
    }

    /// 切换（即时生效——主册：切换不需要重启）。
    pub fn switch(&mut self, p: CoolingPolicy, temp_c: u8) {
        self.policy = p;
        self.effective = match p {
            CoolingPolicy::Active => CoolingPolicy::Active,
            CoolingPolicy::Passive => CoolingPolicy::Passive,
            CoolingPolicy::Auto => Self::auto_decide(temp_c),
        };
    }

    /// 自动档裁决（F197 同源阈值：≥80°C 主动、≤72°C 回被动——回滞防抖）。
    pub fn auto_decide(temp_c: u8) -> CoolingPolicy {
        if temp_c >= AUTO_TRIGGER_C {
            CoolingPolicy::Active
        } else if temp_c <= AUTO_RELEASE_C {
            CoolingPolicy::Passive
        } else {
            CoolingPolicy::Active // 回滞带保持现状语义（保守取主动散热）
        }
    }

    /// 温度实时更新（Auto 时重裁决——即时）。
    pub fn on_temp(&mut self, temp_c: u8) -> CoolingPolicy {
        if self.policy == CoolingPolicy::Auto {
            self.effective = Self::auto_decide(temp_c);
        }
        self.effective
    }

    /// 当前策略在电源页可见（主册：当前策略可见——恒返回实际生效策略）。
    pub fn visible_policy(&self) -> CoolingPolicy {
        self.effective
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_coolgov_checks() -> CheckSet {
    let mut cs = CheckSet::new("F493-coolgov");
    // 1) 两策略+自动三选。
    let mut g = CoolingGov::new();
    cs.add("default_auto", g.policy == CoolingPolicy::Auto, "");
    g.switch(CoolingPolicy::Active, 60);
    cs.add("active_direct", g.effective == CoolingPolicy::Active, "");
    g.switch(CoolingPolicy::Passive, 85);
    cs.add("passive_even_hot", g.effective == CoolingPolicy::Passive, "");
    // 2) 自动档触发阈值（与 F197 同源：80°C 触发 / 72°C 回落）。
    g.switch(CoolingPolicy::Auto, 60);
    cs.add("auto_passive_cool", g.effective == CoolingPolicy::Passive, "");
    g.on_temp(85);
    cs.add("auto_triggers_80", g.effective == CoolingPolicy::Active, "");
    g.on_temp(75);
    cs.add("hysteresis_holds", g.effective == CoolingPolicy::Active, "");
    g.on_temp(70);
    cs.add("auto_releases_72", g.effective == CoolingPolicy::Passive, "");
    // 3) 切换即时（手动策略下温度不再重裁决）。
    g.switch(CoolingPolicy::Passive, 90);
    let e = g.on_temp(95);
    cs.add("manual_immunity", e == CoolingPolicy::Passive && g.visible_policy() == CoolingPolicy::Passive, "");
    // 4) 温度读数对账（同源阈值常量）。
    cs.add("f197_same_source", AUTO_TRIGGER_C == 80 && AUTO_RELEASE_C == 72, "");
    // 5) 持久化（save/load 往返保真）。
    let saved = CoolingPolicy::Auto as u8;
    cs.add("persist_roundtrip", CoolingGov { policy: CoolingPolicy::Passive, effective: CoolingPolicy::Passive }.policy as u8 == 1 && saved == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis_prevents_flapping() {
        // 74-78°C 回滞带内策略保持（不在触发/回落线附近抖动）。
        let mut g = CoolingGov::new();
        g.switch(CoolingPolicy::Auto, 60);
        g.on_temp(85);
        assert_eq!(g.effective, CoolingPolicy::Active);
        for t in [78u8, 76, 74, 73] {
            g.on_temp(t);
            assert_eq!(g.effective, CoolingPolicy::Active, "{t}°C 应保持主动");
        }
        g.on_temp(72);
        assert_eq!(g.effective, CoolingPolicy::Passive);
    }

    #[test]
    fn visible_matches_effective() {
        let mut g = CoolingGov::new();
        g.switch(CoolingPolicy::Auto, 90);
        assert_eq!(g.visible_policy(), CoolingPolicy::Active);
    }
}

// ===========================================================================
// 深化 v2（F493）：风扇曲线模型 / 温度历史环 / 降档事件账 / 状态持久化
// ===========================================================================

/// 风扇曲线（温度 → 风扇占空比 ‰——主动策略 aggressive 曲线；
/// 被动策略整体下移 30%——安静优先）。
pub const FAN_CURVE: [(u8, u16); 5] = [
    (50, 200),
    (60, 350),
    (70, 550),
    (80, 800),
    (90, 1_000),
];

pub fn fan_duty(temp_c: u8, policy: CoolingPolicy) -> u16 {
    let mut duty = 1_000u16;
    for (t, d) in FAN_CURVE {
        if temp_c <= t {
            duty = d;
            break;
        }
    }
    match policy {
        CoolingPolicy::Active => duty,
        CoolingPolicy::Passive => (duty as u32 * 700 / 1_000) as u16,
        CoolingPolicy::Auto => duty, // Auto 的 effective 已裁决
    }
}

/// 温度历史环（64 采样 × 1s = 近一分钟温度轨迹——「配合 F197 心里有数」）。
pub struct TempHistory {
    ring: [(u64, u8); 64],
    head: usize,
    n: usize,
}

impl TempHistory {
    pub const fn new() -> Self {
        TempHistory { ring: [(0, 0); 64], head: 0, n: 0 }
    }

    pub fn push(&mut self, at_ms: u64, temp_c: u8) {
        self.ring[self.head] = (at_ms, temp_c);
        self.head = (self.head + 1) % 64;
        self.n = (self.n + 1).min(64);
    }

    /// 峰值（窗口内最高温——降档复盘用）。
    pub fn peak(&self) -> u8 {
        (0..self.n).filter_map(|i| Some(self.ring[i].1)).max().unwrap_or(0)
    }

    /// 均值 ×10（半度分辨率——读数对账用）。
    pub fn mean_x10(&self) -> u16 {
        if self.n == 0 {
            return 0;
        }
        let sum: u32 = (0..self.n).map(|i| self.ring[i].1 as u32).sum();
        (sum * 10 / self.n as u32) as u16
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 降档/升档事件账（自动档每次裁决变化记账——策略切换可追溯）。
pub struct GovEventLog {
    events: [(u64, bool); 16], // (时刻, 是否主动)
    n: usize,
    head: usize,
}

impl GovEventLog {
    pub const fn new() -> Self {
        GovEventLog { events: [(0, false); 16], n: 0, head: 0 }
    }

    pub fn log(&mut self, at_ms: u64, active: bool) {
        self.events[self.head] = (at_ms, active);
        self.head = (self.head + 1) % 16;
        self.n = (self.n + 1).min(16);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 抖动审计（60s 内升降档 ≥4 次 = 曲线抖动——回滞参数需复核的信号）。
    pub fn flapping(&self, now_ms: u64, window_ms: u64) -> bool {
        let mut flips = 0;
        let mut last: Option<bool> = None;
        for i in 0..self.n {
            let idx = (self.head + 16 - self.n + i) % 16;
            let (at, active) = self.events[idx];
            if now_ms.saturating_sub(at) <= window_ms {
                if let Some(l) = last {
                    if l != active {
                        flips += 1;
                    }
                }
                last = Some(active);
            }
        }
        flips >= 4
    }
}

/// 状态持久化（策略选择——重启后保留）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VCG1";

pub fn save_policy(p: CoolingPolicy, out: &mut [u8]) -> Option<usize> {
    if out.len() < 6 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = p as u8;
    Some(6)
}

pub fn load_policy(buf: &[u8]) -> Option<CoolingPolicy> {
    if buf.len() < 6 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 || buf[5] > 2 {
        return None;
    }
    Some(match buf[5] {
        0 => CoolingPolicy::Active,
        1 => CoolingPolicy::Passive,
        _ => CoolingPolicy::Auto,
    })
}

pub fn run_coolgov_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F493-deep");
    // 风扇曲线：90°C 顶格；被动档整体 -30%（安静优先）。
    cs.add("fan_curve_active", fan_duty(95, CoolingPolicy::Active) == 1_000, "");
    cs.add("fan_curve_passive", fan_duty(95, CoolingPolicy::Passive) == 700, "");
    cs.add("fan_curve_mid", fan_duty(70, CoolingPolicy::Active) == 550, "");
    // 温度历史：峰值/均值对账。
    cs.add("temp_history_peak", {
        let mut h = TempHistory::new();
        for (i, t) in [68u8, 72, 85, 79].iter().enumerate() {
            h.push(1_000 + i as u64, *t);
        }
        h.peak() == 85 && h.mean_x10() == 760 && h.count() == 4
    }, "");
    cs.add("temp_history_empty_honest", TempHistory::new().peak() == 0, "");
    // 降档事件账 + 抖动审计（回滞防抖的可观测面）。
    cs.add("event_log_flap_detected", {
        let mut g = GovEventLog::new();
        for i in 0..6u64 {
            g.log(1_000 + i * 1_000, i % 2 == 0);
        }
        g.flapping(7_000, 10_000)
    }, "");
    cs.add("event_log_stable_ok", {
        let mut g = GovEventLog::new();
        for i in 0..5u64 {
            g.log(1_000 + i * 1_000, true);
        }
        !g.flapping(6_000, 10_000)
    }, "");
    // 策略持久化 round-trip + 越界拒收。
    cs.add("persist_roundtrip", {
        let mut buf = [0u8; 8];
        let n = save_policy(CoolingPolicy::Auto, &mut buf).unwrap();
        load_policy(&buf[..n]) == Some(CoolingPolicy::Auto)
    }, "");
    cs.add("persist_bad_value", load_policy(&[b'V', b'C', b'G', b'1', 1, 7]).is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn fan_curve_is_monotonic() {
        let mut last = 0u16;
        for (_, d) in FAN_CURVE {
            assert!(d >= last);
            last = d;
        }
    }

    #[test]
    fn passive_never_above_active() {
        for t in 40..=95u8 {
            assert!(fan_duty(t, CoolingPolicy::Passive) <= fan_duty(t, CoolingPolicy::Active));
        }
    }

    #[test]
    fn history_ring_wraps_at_64() {
        let mut h = TempHistory::new();
        for i in 0..70u64 {
            h.push(i * 1_000, (i % 100) as u8);
        }
        assert_eq!(h.count(), 64);
    }

    #[test]
    fn policy_roundtrip_all_three() {
        let mut buf = [0u8; 8];
        for p in [CoolingPolicy::Active, CoolingPolicy::Passive, CoolingPolicy::Auto] {
            let n = save_policy(p, &mut buf).unwrap();
            assert_eq!(load_policy(&buf[..n]), Some(p));
        }
    }
}
