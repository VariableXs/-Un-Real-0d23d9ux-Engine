//! UNREAL-X AI-02 · 族0015 唤醒源治理（X00351~X00375）。
//!
//! 唤醒源白名单 + 仲裁 + 记录：只有白名单内的唤醒源可以点亮系统；
//! 多个候选同时到达时按固定优先级仲裁出唯一赢家；每次唤醒写入环形记录。
//! 纯逻辑 + 固定数组；非法输入钳制回默认，绝不 panic。

use crate::checks::CheckSet;

/// 唤醒源目录（固定 8 类）。
pub const WAKE_SRC_COUNT: usize = 8;

/// 唤醒记录环形容量。
pub const WAKE_LOG_CAPACITY: usize = 8;

/// 唤醒源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeSrc {
    PowerButton = 0,
    Keyboard = 1,
    Mouse = 2,
    RtcTimer = 3,
    Lan = 4,
    Lid = 5,
    Usb = 6,
    Unknown = 7,
}

impl WakeSrc {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            WakeSrc::PowerButton => "pwrbtn",
            WakeSrc::Keyboard => "kbd",
            WakeSrc::Mouse => "mouse",
            WakeSrc::RtcTimer => "rtc",
            WakeSrc::Lan => "lan",
            WakeSrc::Lid => "lid",
            WakeSrc::Usb => "usb",
            WakeSrc::Unknown => "unknown",
        }
    }

    /// 仲裁优先级（越小越优先；电源键恒最优先）。
    pub fn priority(self) -> u8 {
        match self {
            WakeSrc::PowerButton => 0,
            WakeSrc::Lid => 1,
            WakeSrc::Keyboard => 2,
            WakeSrc::RtcTimer => 3,
            WakeSrc::Lan => 4,
            WakeSrc::Usb => 5,
            WakeSrc::Mouse => 6,
            WakeSrc::Unknown => 7,
        }
    }

    pub const ALL: [WakeSrc; WAKE_SRC_COUNT] = [
        WakeSrc::PowerButton,
        WakeSrc::Keyboard,
        WakeSrc::Mouse,
        WakeSrc::RtcTimer,
        WakeSrc::Lan,
        WakeSrc::Lid,
        WakeSrc::Usb,
        WakeSrc::Unknown,
    ];
}

/// 唤醒治理档位：≥5 档独立可交付。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakePolicy {
    /// 关闭治理 = 现状：一切唤醒源放行。
    Off,
    /// 只认电源键/盖盖。
    Essential,
    /// 标准：外加键盘/RTC。
    Standard,
    /// 宽松：外加 LAN/USB（远程唤醒场景）。
    Permissive,
    /// 自审计：全放行但逐条记录审计行。
    Audit,
}

impl WakePolicy {
    pub fn from_id(id: &str) -> WakePolicy {
        match id {
            "off" => WakePolicy::Off,
            "essential" => WakePolicy::Essential,
            "standard" => WakePolicy::Standard,
            "permissive" => WakePolicy::Permissive,
            "audit" => WakePolicy::Audit,
            _ => WakePolicy::Off,
        }
    }

    /// 该档位下的唤醒源白名单。
    pub fn whitelist(self) -> [bool; WAKE_SRC_COUNT] {
        let mut w = [false; WAKE_SRC_COUNT];
        for s in WakeSrc::ALL {
            w[s.index()] = match self {
                WakePolicy::Off | WakePolicy::Audit => true,
                WakePolicy::Essential => {
                    matches!(s, WakeSrc::PowerButton | WakeSrc::Lid)
                }
                WakePolicy::Standard => {
                    matches!(s, WakeSrc::PowerButton | WakeSrc::Lid | WakeSrc::Keyboard | WakeSrc::RtcTimer)
                }
                WakePolicy::Permissive => s != WakeSrc::Unknown,
            };
        }
        w
    }
}

/// 一条唤醒记录。
#[derive(Clone, Copy, Debug)]
pub struct WakeRecord {
    pub src: WakeSrc,
    /// 唤醒时刻（调用方时钟，纯计数）。
    pub stamp: u64,
    /// 是否被白名单拒绝后仍到达（审计线索）。
    pub rejected: bool,
}

/// 唤醒源治理器。
#[derive(Clone, Copy, Debug)]
pub struct WakeGovernor {
    pub policy: WakePolicy,
    /// 用户对单源的开/关覆盖（叠加在档位白名单之上）。
    overrides: [Option<bool>; WAKE_SRC_COUNT],
    log: [Option<WakeRecord>; WAKE_LOG_CAPACITY],
    log_head: usize,
    log_len: usize,
    /// 本次唤醒的仲裁赢家。
    pub last_wake: Option<WakeSrc>,
    /// 被钳制/拒绝次数（护栏可观测）。
    pub clamped: u32,
}

impl WakeGovernor {
    pub const fn new(policy: WakePolicy) -> WakeGovernor {
        WakeGovernor {
            policy,
            overrides: [None; WAKE_SRC_COUNT],
            log: [None; WAKE_LOG_CAPACITY],
            log_head: 0,
            log_len: 0,
            last_wake: None,
            clamped: 0,
        }
    }

    /// 生效白名单 = 档位白名单 ⊕ 用户覆盖。
    pub fn effective(&self, src: WakeSrc) -> bool {
        let base = self.policy.whitelist()[src.index()];
        match self.overrides[src.index()] {
            Some(v) => v,
            None => base,
        }
    }

    /// 用户覆盖单源：true 允许 / false 禁止（非法档位记钳制）。
    pub fn set_override(&mut self, src: WakeSrc, allow: bool) {
        if src == WakeSrc::Unknown {
            self.clamped += 1;
            return;
        }
        self.overrides[src.index()] = Some(allow);
    }

    /// 清除单源覆盖（回到档位默认）。
    pub fn clear_override(&mut self, src: WakeSrc) {
        self.overrides[src.index()] = None;
    }

    /// 仲裁：多个候选同时到达，选出唯一赢家。
    /// 全部被白名单拒绝 → None（保持睡眠），并把拒绝记入日志。
    pub fn arbitrate(&mut self, candidates: &[WakeSrc], stamp: u64) -> Option<WakeSrc> {
        if self.policy == WakePolicy::Off {
            // 现状档：无治理，直接按优先级挑第一个。
            let mut best: Option<WakeSrc> = None;
            for &c in candidates {
                best = match best {
                    Some(b) if b.priority() <= c.priority() => Some(b),
                    _ => Some(c),
                };
            }
            if let Some(w) = best {
                self.record(w, stamp, false);
                self.last_wake = Some(w);
            }
            return best;
        }
        let mut best: Option<WakeSrc> = None;
        for &c in candidates {
            if !self.effective(c) {
                self.record(c, stamp, true);
                continue;
            }
            best = match best {
                Some(b) if b.priority() <= c.priority() => Some(b),
                _ => Some(c),
            };
        }
        if let Some(w) = best {
            self.record(w, stamp, false);
            self.last_wake = Some(w);
        } else {
            self.clamped += 1;
        }
        best
    }

    /// 记录入环形缓冲（容量 8，覆盖最旧）。
    fn record(&mut self, src: WakeSrc, stamp: u64, rejected: bool) {
        self.log[self.log_head] = Some(WakeRecord { src, stamp, rejected });
        self.log_head = (self.log_head + 1) % WAKE_LOG_CAPACITY;
        if self.log_len < WAKE_LOG_CAPACITY {
            self.log_len += 1;
        }
    }

    /// 读取记录（旧 → 新）。
    pub fn records(&self) -> [Option<WakeRecord>; WAKE_LOG_CAPACITY] {
        let mut out: [Option<WakeRecord>; WAKE_LOG_CAPACITY] = [None; WAKE_LOG_CAPACITY];
        for i in 0..self.log_len {
            let idx = (self.log_head + WAKE_LOG_CAPACITY - self.log_len + i) % WAKE_LOG_CAPACITY;
            out[i] = self.log[idx];
        }
        out
    }

    /// 净身：清空记录与覆盖。
    pub fn reset(&mut self) {
        self.log = [None; WAKE_LOG_CAPACITY];
        self.log_head = 0;
        self.log_len = 0;
        self.overrides = [None; WAKE_SRC_COUNT];
        self.last_wake = None;
        self.clamped = 0;
    }
}

/// 族0015 域自检。
pub fn run_wakesrc_checks() -> CheckSet {
    let mut set = CheckSet::new("power.x2wakesrc");
    let mut g = WakeGovernor::new(WakePolicy::Standard);
    set.add("whitelist essential narrower than permissive", {
        let e = WakePolicy::Essential.whitelist().iter().filter(|w| **w).count();
        let p = WakePolicy::Permissive.whitelist().iter().filter(|w| **w).count();
        e < p
    }, "");
    set.add("arbitration picks priority winner", {
        g.arbitrate(&[WakeSrc::Mouse, WakeSrc::Keyboard], 10) == Some(WakeSrc::Keyboard)
    }, "");
    set.add("rejected source stays asleep", {
        let mut h = WakeGovernor::new(WakePolicy::Essential);
        h.arbitrate(&[WakeSrc::Lan], 1).is_none() && h.clamped == 1
    }, "");
    set.add("power button always wins", {
        g.arbitrate(&[WakeSrc::Keyboard, WakeSrc::PowerButton, WakeSrc::Lan], 20) == Some(WakeSrc::PowerButton)
    }, "");
    set.add("override can block whitelisted src", {
        let mut h = WakeGovernor::new(WakePolicy::Standard);
        h.set_override(WakeSrc::Keyboard, false);
        h.arbitrate(&[WakeSrc::Keyboard], 2).is_none()
    }, "");
    set.add("override on unknown clamps", {
        let mut h = WakeGovernor::new(WakePolicy::Standard);
        h.set_override(WakeSrc::Unknown, true);
        h.clamped == 1
    }, "");
    set.add("ring log capacity 8", {
        let mut h = WakeGovernor::new(WakePolicy::Off);
        let mut n = 0u64;
        while n < 12 {
            h.arbitrate(&[WakeSrc::RtcTimer], n);
            n += 1;
        }
        let recs = h.records();
        recs.iter().flatten().count() == WAKE_LOG_CAPACITY
            && recs[0].unwrap().stamp == 4
            && recs[7].unwrap().stamp == 11
    }, "");
    set.add("off policy is passthrough", WakePolicy::Off.whitelist().iter().all(|w| *w), "");
    set.add("policy lookup unknown falls to off", WakePolicy::from_id("nope") == WakePolicy::Off, "");
    set.add("reset wipes log and overrides", {
        g.reset();
        g.records().iter().flatten().count() == 0 && g.last_wake.is_none() && g.clamped == 0
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x00351_min_loop_whitelist_and_arbitration() {
        let mut g = WakeGovernor::new(WakePolicy::Standard);
        assert_eq!(g.arbitrate(&[WakeSrc::Mouse, WakeSrc::RtcTimer], 5), Some(WakeSrc::RtcTimer));
        assert!(g.arbitrate(&[WakeSrc::Lan], 6).is_none());
    }

    #[test]
    fn x00356_invalid_inputs_never_panic() {
        let mut g = WakeGovernor::new(WakePolicy::Essential);
        g.set_override(WakeSrc::Unknown, false);
        assert_eq!(g.clamped, 1);
        assert!(!g.arbitrate(&[], 0).is_some());
    }

    #[test]
    fn x00364_arbitration_log_ring() {
        let mut g = WakeGovernor::new(WakePolicy::Permissive);
        for i in 0..10u64 {
            g.arbitrate(&[WakeSrc::Usb], i);
        }
        let recs = g.records();
        assert_eq!(recs[0].unwrap().stamp, 2);
        assert_eq!(recs[WAKE_LOG_CAPACITY - 1].unwrap().stamp, 9);
    }

    #[test]
    fn x00370_reset_clean() {
        let mut g = WakeGovernor::new(WakePolicy::Audit);
        g.set_override(WakeSrc::Mouse, true);
        g.arbitrate(&[WakeSrc::Mouse], 1);
        g.reset();
        assert!(g.records().iter().flatten().count() == 0 && g.overrides.iter().all(|o| o.is_none()));
    }

    #[test]
    fn x00351_run_checks_pass() {
        assert!(run_wakesrc_checks().all_passed());
    }
}
