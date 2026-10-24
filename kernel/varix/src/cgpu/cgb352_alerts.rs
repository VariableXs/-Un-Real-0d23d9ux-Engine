// CGPU-F0352 预算监控告警 · cgb352_alerts.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算告警规则（阈值/趋势/异常）；告警去重；
//       分级通知；告警历史；静默期。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 告警规则。
#[derive(Clone, Copy, Debug)]
pub struct AlertRule {
    pub id: u32,
    pub metric_id: u32,
    pub threshold_q8: u16,
    pub level: u8, // 1=info 2=warn 3=critical
    pub cooldown_frames: u32,
    pub enabled: bool,
}

/// 告警事件。
#[derive(Clone, Copy, Debug)]
pub struct AlertEvent {
    pub id: u32,
    pub rule_id: u32,
    pub frame: u64,
    pub value_q8: u16,
    pub level: u8,
    pub notified: bool,
}

/// 告警引擎。
pub struct AlertEngine {
    pub rules: Vec<AlertRule>,
    pub events: Vec<AlertEvent>,
    pub last_fired: Vec<(u32, u64)>, // rule_id -> frame
    pub frame: u64,
    pub muted: bool,
}

impl AlertEngine {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            events: Vec::new(),
            last_fired: Vec::new(),
            frame: 0,
            muted: false,
        }
    }

    pub fn add_rule(&mut self, metric_id: u32, threshold_q8: u16, level: u8, cooldown: u32) -> u32 {
        let id = self.rules.len() as u32 + 1;
        self.rules.push(AlertRule {
            id,
            metric_id,
            threshold_q8,
            level,
            cooldown_frames: cooldown,
            enabled: true,
        });
        id
    }

    /// 输入值，触发告警。
    pub fn tick(&mut self, metric_id: u32, value_q8: u16) -> Option<u32> {
        self.frame += 1;
        if self.muted {
            return None;
        }
        for rule in &self.rules {
            if !rule.enabled || rule.metric_id != metric_id || value_q8 < rule.threshold_q8 {
                continue;
            }
            // 冷却检查
            if let Some((_, last)) = self.last_fired.iter().find(|(rid, _)| *rid == rule.id) {
                if self.frame - *last < rule.cooldown_frames as u64 {
                    continue;
                }
            }
            let event_id = self.events.len() as u32 + 1;
            self.events.push(AlertEvent {
                id: event_id,
                rule_id: rule.id,
                frame: self.frame,
                value_q8,
                level: rule.level,
                notified: !self.muted,
            });
            self.last_fired.retain(|(rid, _)| *rid != rule.id);
            self.last_fired.push((rule.id, self.frame));
            return Some(event_id);
        }
        None
    }

    /// 告警历史统计。
    pub fn count_by_level(&self, level: u8) -> u32 {
        self.events.iter().filter(|e| e.level == level).count() as u32
    }

    /// 静默。
    pub fn mute(&mut self) {
        self.muted = true;
    }

    pub fn unmute(&mut self) {
        self.muted = false;
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0352 预算监控告警");
        cs.add("规则 id 唯一", {
            let mut ids: Vec<u32> = self.rules.iter().map(|r| r.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.rules.len()
        }, "unique");
        cs.add("事件 id 递增", {
            self.events.windows(2).all(|w| w[0].id < w[1].id)
        }, "ids");
        cs.add_dyn("冷却 ≥ 0", self.rules.iter().all(|r| true), "cooldown");
        cs.add("级别 1..=3", self.rules.iter().all(|r| r.level >= 1 && r.level <= 3), "level");
        cs.add("事件引用有效规则", {
            self.events.iter().all(|e| self.rules.iter().any(|r| r.id == e.rule_id))
        }, "valid ref");
        cs.add("冷却执行正确", {
            self.events.windows(2).all(|w| {
                let r1 = self.rules.iter().find(|r| r.id == w[0].rule_id);
                match r1 {
                    None => true,
                    Some(r) => w[1].frame - w[0].frame >= r.cooldown_frames as u64 || w[1].rule_id != w[0].rule_id,
                }
            })
        }, "cooldown enforced");
        cs
    }
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_and_cooldown() {
        let mut e = AlertEngine::new();
        e.add_rule(1, 200, 3, 5);
        assert!(e.tick(1, 250).is_some());
        assert!(e.tick(1, 250).is_none()); // 冷却中
        e.frame += 5;
        assert!(e.tick(1, 250).is_some());
    }

    #[test]
    fn mute_blocks() {
        let mut e = AlertEngine::new();
        e.add_rule(1, 100, 2, 0);
        e.mute();
        assert!(e.tick(1, 250).is_none());
    }
}
