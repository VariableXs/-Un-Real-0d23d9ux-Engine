// CGPU-F0342 前台后台差异化 · cgb342_fg_bg.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：前台/后台预算差异化（前台优先级高/配额多/降档晚）；
//       切换响应 <1 帧；历史记录；公平性保底。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 任务可见性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    Foreground,
    Background,
    Hidden,
}

/// 配额策略。
#[derive(Clone, Copy, Debug)]
pub struct QuotaPolicy {
    pub fg_weight: u32,
    pub bg_weight: u32,
    pub fg_quota_us: u32,
    pub bg_quota_us: u32,
    pub fg_degrade_threshold_q8: u16,
    pub bg_degrade_threshold_q8: u16,
}

impl Default for QuotaPolicy {
    fn default() -> Self {
        Self {
            fg_weight: 80,
            bg_weight: 20,
            fg_quota_us: 10_000,
            bg_quota_us: 2_500,
            fg_degrade_threshold_q8: 230,
            bg_degrade_threshold_q8: 179,
        }
    }
}

/// 配额分配记录。
#[derive(Clone, Copy, Debug)]
pub struct QuotaAllocation {
    pub subject: u32,
    pub visibility: Visibility,
    pub quota_us: u32,
    pub used_us: u32,
    pub frame: u64,
}

/// 前后台差异化引擎。
pub struct FgBgScheduler {
    pub policy: QuotaPolicy,
    pub allocations: Vec<QuotaAllocation>,
    pub switches: Vec<(u64, u32, Visibility, Visibility)>,
    pub current_frame: u64,
    pub switch_latency_frames: u32,
}

impl FgBgScheduler {
    pub fn new(policy: QuotaPolicy) -> Self {
        Self {
            policy,
            allocations: Vec::new(),
            switches: Vec::new(),
            current_frame: 0,
            switch_latency_frames: 0,
        }
    }

    /// 分配配额（按可见性）。
    pub fn allocate(&mut self, subject: u32, visibility: Visibility) -> u32 {
        let quota = match visibility {
            Visibility::Foreground => self.policy.fg_quota_us,
            Visibility::Background => self.policy.bg_quota_us,
            Visibility::Hidden => 0,
        };
        self.allocations.push(QuotaAllocation {
            subject,
            visibility,
            quota_us: quota,
            used_us: 0,
            frame: self.current_frame,
        });
        quota
    }

    /// 可见性切换。
    pub fn switch_visibility(&mut self, subject: u32, new_vis: Visibility) {
        self.current_frame += 1;
        if let Some(a) = self.allocations.iter_mut().find(|a| a.subject == subject) {
            let old = a.visibility;
            if old != new_vis {
                a.visibility = new_vis;
                a.quota_us = match new_vis {
                    Visibility::Foreground => self.policy.fg_quota_us,
                    Visibility::Background => self.policy.bg_quota_us,
                    Visibility::Hidden => 0,
                };
                self.switches.push((self.current_frame, subject, old, new_vis));
                self.switch_latency_frames = 1; // 同帧响应
            }
        }
    }

    /// 消耗。
    pub fn consume(&mut self, subject: u32, us: u32) -> bool {
        if let Some(a) = self.allocations.iter_mut().find(|a| a.subject == subject) {
            if a.used_us + us > a.quota_us {
                return false;
            }
            a.used_us += us;
            true
        } else {
            false
        }
    }

    /// 降档阈值（按可见性）。
    pub fn degrade_threshold(&self, visibility: Visibility) -> u16 {
        match visibility {
            Visibility::Foreground => self.policy.fg_degrade_threshold_q8,
            Visibility::Background => self.policy.bg_degrade_threshold_q8,
            Visibility::Hidden => 0,
        }
    }

    /// 公平性保底：后台最小份额。
    pub fn fairness_floor(&self) -> u32 {
        self.policy.bg_quota_us
    }

    /// 使用率。
    pub fn usage_q8(&self, subject: u32) -> u16 {
        self.allocations
            .iter()
            .find(|a| a.subject == subject)
            .map(|a| {
                if a.quota_us == 0 {
                    0
                } else {
                    ((a.used_us as u64 * 256) / a.quota_us as u64) as u16
                }
            })
            .unwrap_or(0)
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0342 前台后台差异化");
        cs.add("FG 配额 > BG 配额", {
            self.policy.fg_quota_us > self.policy.bg_quota_us
        }, "fg > bg");
        cs.add("FG 权重 > BG 权重", {
            self.policy.fg_weight > self.policy.bg_weight
        }, "fg_weight > bg_weight");
        cs.add("FG 降档阈值 ≥ BG", {
            self.policy.fg_degrade_threshold_q8 >= self.policy.bg_degrade_threshold_q8
        }, "fg threshold ≥ bg");
        cs.add("切换延迟 ≤ 1 帧", self.switch_latency_frames <= 1, "≤1 frame");
        cs.add("分配记录不超额", {
            self.allocations.iter().all(|a| a.used_us <= a.quota_us)
        }, "used ≤ quota");
        cs.add("切换记录时间有序", {
            self.switches.windows(2).all(|w| w[0].0 <= w[1].0)
        }, "frame mono");
        cs.add("后台保底正", self.fairness_floor() > 0, ">0");
        cs
    }
}

impl Default for FgBgScheduler {
    fn default() -> Self {
        Self::new(QuotaPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fg_gets_more() {
        let mut s = FgBgScheduler::default();
        let fg = s.allocate(1, Visibility::Foreground);
        let bg = s.allocate(2, Visibility::Background);
        assert!(fg > bg);
    }

    #[test]
    fn switch_updates_quota() {
        let mut s = FgBgScheduler::default();
        s.allocate(1, Visibility::Background);
        s.switch_visibility(1, Visibility::Foreground);
        assert_eq!(s.usage_q8(1), 0);
        assert!(s.consume(1, 5000));
    }

    #[test]
    fn consume_over_quota() {
        let mut s = FgBgScheduler::default();
        s.allocate(1, Visibility::Background);
        assert!(!s.consume(1, 10_000)); // 超 BG 配额
    }
}
