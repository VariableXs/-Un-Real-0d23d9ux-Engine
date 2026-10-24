//! VE-F4270 · 一致性预警（U04 治理组）
//! 职责：预警三级（提示/警告/紧急）+ 触发三类（趋势越限/复发率抬升/红线邻界）
//! + 升级链逐级兜底（断点直达主席）+ 误报一键静默反哺规则 + 补报不掩盖。
//! 段位：0x27A0（续表 idx504）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x27A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlCode {
    /// 升级链断点（无兜底）。
    ChainBroken = SEG_BASE + 0x01,
    /// 误报静默后无反哺。
    MuteNoFeedback = SEG_BASE + 0x02,
    /// 补报掩盖旧账。
    BackfillCoverup = SEG_BASE + 0x03,
}

/// 预警级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum AlLevel {
    Hint = 0,
    Warn = 1,
    Urgent = 2,
}

/// 触发源三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlTrigger {
    TrendOver = 0,
    RecurUp = 1,
    RedlineNear = 2,
}

/// 预警事件。
#[derive(Clone, Copy, Debug)]
pub struct Alert {
    pub level: AlLevel,
    pub trigger: AlTrigger,
    pub rule: u16,
    pub muted: bool,
    /// 反哺规则修订（静默后须反哺）。
    pub fed_back: bool,
    pub hash: u64, // 去重哈希
}

/// 预警册。
pub struct AlertBook {
    pub events: Vec<Alert>,
    /// 升级链：角色序（末位兜底=主席）。
    pub chain: Vec<u8>,
    /// 补报账：(规则, 漏报时段) —— 补扫不掩盖，显式追加。
    pub backfills: Vec<(u16, u32)>,
    /// 通知三通道（视觉/听觉/读屏——域本色）。
    pub channels: [bool; 3],
}

fn ev_hash(level: u8, rule: u16) -> u64 {
    (level as u64) << 16 | rule as u64
}

impl AlertBook {
    pub fn new(chain: Vec<u8>) -> Self {
        AlertBook { events: Vec::new(), chain, backfills: Vec::new(), channels: [true; 3] }
    }

    /// 触发：按触发源映射预警级。
    pub fn fire(&mut self, trigger: AlTrigger, rule: u16) -> AlLevel {
        let level = match trigger {
            AlTrigger::TrendOver => AlLevel::Hint,
            AlTrigger::RecurUp => AlLevel::Warn,
            AlTrigger::RedlineNear => AlLevel::Urgent,
        };
        let h = ev_hash(level as u8, rule);
        if !self.events.iter().any(|e| e.hash == h) {
            self.events.push(Alert { level, trigger, rule, muted: false, fed_back: false, hash: h });
        }
        level
    }

    /// 路由：按级取链位；链空→Err。
    pub fn route(&self, level: AlLevel) -> Result<u8, AlCode> {
        let idx = match level {
            AlLevel::Hint => 0,
            AlLevel::Warn => self.chain.len().saturating_sub(2),
            AlLevel::Urgent => self.chain.len().saturating_sub(1),
        };
        self.chain.get(idx).copied().ok_or(AlCode::ChainBroken)
    }

    /// 升级链兜底：Urgent 直达末位（主席）——断点容忍。
    pub fn escalate_bottom(&self) -> Result<u8, AlCode> {
        self.chain.last().copied().ok_or(AlCode::ChainBroken)
    }

    /// 误报静默：一键静默必须反哺规则修订。
    pub fn mute(&mut self, rule: u16, feed_back: bool) -> Result<(), AlCode> {
        let e = self.events.iter_mut().find(|e| e.rule == rule && !e.muted).ok_or(AlCode::MuteNoFeedback)?;
        e.muted = true;
        e.fed_back = feed_back;
        if !feed_back {
            return Err(AlCode::MuteNoFeedback);
        }
        Ok(())
    }

    /// 补报：追加显式记录（不覆盖不掩盖）。
    pub fn backfill(&mut self, rule: u16, period: u32) {
        self.backfills.push((rule, period));
    }

    /// 通知三通道齐备（域本色）。
    pub fn channels_ok(&self) -> bool {
        self.channels.iter().all(|c| *c)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C504-01..12。
pub fn run_ves504_checks() -> CheckSet {
    let mut set = CheckSet::new("ves504");

    let mut ab = AlertBook::new(alloc::vec![1, 2, 3]);
    // 三级映射
    chk(&mut set, "C504-01", ab.fire(AlTrigger::TrendOver, 10) == AlLevel::Hint, "趋势越限→提示级");
    chk(&mut set, "C504-02", ab.fire(AlTrigger::RecurUp, 20) == AlLevel::Warn, "复发抬升→警告级");
    chk(&mut set, "C504-03", ab.fire(AlTrigger::RedlineNear, 30) == AlLevel::Urgent, "红线邻界→紧急级");

    // 去重
    ab.fire(AlTrigger::RedlineNear, 30);
    chk(&mut set, "C504-04", ab.events.len() == 3, "同事件去重");

    // 路由
    chk(&mut set, "C504-05", ab.route(AlLevel::Hint) == Ok(1), "提示级→链首");
    chk(&mut set, "C504-06", ab.route(AlLevel::Urgent) == Ok(3), "紧急级→链末（主席）");
    chk(&mut set, "C504-07", ab.escalate_bottom() == Ok(3), "升级链断点直达主席兜底");

    let empty = AlertBook::new(alloc::vec![]);
    chk(&mut set, "C504-08", empty.route(AlLevel::Hint) == Err(AlCode::ChainBroken), "空链报断点");

    // 静默反哺
    chk(&mut set, "C504-09", ab.mute(10, true).is_ok(), "误报静默+反哺");
    chk(&mut set, "C504-10", ab.mute(20, false) == Err(AlCode::MuteNoFeedback), "静默无反哺拒绝");

    // 补报不掩盖
    ab.backfill(10, 5);
    ab.backfill(10, 5);
    chk(&mut set, "C504-11", ab.backfills.len() == 2, "补报显式追加不覆盖");

    chk(&mut set, "C504-12", ab.channels_ok(), "通知三通道齐备（视觉/听觉/读屏——域本色）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_levels() {
        let mut ab = AlertBook::new(alloc::vec![9]);
        assert_eq!(ab.fire(AlTrigger::RedlineNear, 1), AlLevel::Urgent);
        assert_eq!(ab.route(AlLevel::Urgent), Ok(9));
        assert_eq!(AlertBook::new(alloc::vec![]).escalate_bottom(), Err(AlCode::ChainBroken));
    }
}
