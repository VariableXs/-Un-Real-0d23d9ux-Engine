//! VE-F4272 · 一致性自动化治理（U04 治理组）
//! 职责：自动化四项（定级建议/派单/催办/销账复核预检）+ 人机分工铁律
//! （机器建议人终审——终审权永不委托）+ 置信度低转人工 + 误操作回放撤销
//! + 自动化风暴熔断降级全人工 + 自动化率上限。
//! 段位：0x2800（续表 idx506）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2800;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuCode {
    /// 置信度低转人工。
    LowConfidence = SEG_BASE,
    /// 自动化率越上限。
    RateOver = SEG_BASE + 0x01,
    /// 熔断触发（风暴降级全人工）。
    CircuitOpen = SEG_BASE + 0x02,
    /// 撤销失败（回放不可逆）。
    UndoFail = SEG_BASE + 0x03,
}

/// 自动化项四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoKind {
    Grading = 0,
    Assign = 1,
    Urge = 2,
    PreReview = 3,
}
pub const AUTO_N: usize = 4;

/// 自动化操作（可回放撤销）。
#[derive(Clone, Copy, Debug)]
pub struct AutoOp {
    pub kind: AutoKind,
    pub target: u16,
    pub confidence_bp: u16, // 置信度 bp
    pub undone: bool,
}

/// 自动化治理器。
pub struct AutoGov {
    pub ops: Vec<AutoOp>,
    /// 终审队列（机器建议人终审）。
    pub review_queue: Vec<u16>,
    /// 自动化率上限（0-10000 bp）。
    pub rate_cap_bp: u16,
    /// 熔断：连续误操作阈值。
    pub fuse_threshold: u32,
    pub fuse_blows: u32,
    /// 撤销反向日志。
    pub undo_log: Vec<u16>,
}

impl AutoGov {
    pub fn new(rate_cap_bp: u16, fuse_threshold: u32) -> Self {
        AutoGov { ops: Vec::new(), review_queue: Vec::new(), rate_cap_bp, fuse_threshold, fuse_blows: 0, undo_log: Vec::new() }
    }

    /// 建议生成：置信度低于线→转人工队列（人终审）。
    pub fn suggest(&mut self, kind: AutoKind, target: u16, confidence_bp: u16, floor_bp: u16) -> Result<(), AuCode> {
        if confidence_bp < floor_bp {
            if !self.review_queue.contains(&target) {
                self.review_queue.push(target);
            }
            return Err(AuCode::LowConfidence);
        }
        // 自动化率上限检
        let done = self.ops.len() as u32;
        let total = done + self.review_queue.len() as u32 + 1;
        if total > 0 && (done * 10_000 / total) as u16 > self.rate_cap_bp {
            if !self.review_queue.contains(&target) {
                self.review_queue.push(target);
            }
            return Err(AuCode::RateOver);
        }
        self.ops.push(AutoOp { kind, target, confidence_bp, undone: false });
        Ok(())
    }

    /// 误操作回放撤销（O(1) 反向日志）。
    pub fn undo(&mut self, target: u16) -> Result<(), AuCode> {
        let op = self.ops.iter_mut().rev().find(|o| o.target == target && !o.undone).ok_or(AuCode::UndoFail)?;
        op.undone = true;
        self.undo_log.push(target);
        Ok(())
    }

    /// 风暴熔断：连续撤销数达阈值→熔断降级全人工。
    pub fn storm_check(&mut self) -> bool {
        let recent = self.undo_log.len() as u32;
        if recent >= self.fuse_threshold {
            self.fuse_blows += 1;
            self.review_queue.extend(self.ops.iter().filter(|o| !o.undone).map(|o| o.target));
            return true;
        }
        false
    }

    /// 自动化率现值。
    pub fn rate_bp(&self) -> u32 {
        let done = self.ops.iter().filter(|o| !o.undone).count() as u32;
        let total = done + self.review_queue.len() as u32;
        if total == 0 {
            0
        } else {
            done * 10_000 / total
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C506-01..12。
pub fn run_ves506_checks() -> CheckSet {
    let mut set = CheckSet::new("ves506");

    let mut ag = AutoGov::new(9_000, 3);
    // 四项自动化
    for (i, k) in [AutoKind::Grading, AutoKind::Assign, AutoKind::Urge, AutoKind::PreReview].iter().enumerate() {
        let r = ag.suggest(*k, 100 + i as u16, 9_500, 8_000);
        debug_assert!(r.is_ok());
    }
    chk(&mut set, "C506-01", ag.ops.len() == 4, "四项自动化全过（高置信）");

    // 低置信转人工
    chk(&mut set, "C506-02", ag.suggest(AutoKind::Grading, 200, 5_000, 8_000) == Err(AuCode::LowConfidence), "低置信转人工队列");
    chk(&mut set, "C506-03", ag.review_queue == alloc::vec![200], "终审队列在册（终审权在人）");

    // 自动化率上限
    let mut ag2 = AutoGov::new(500, 99); // 上限 5%
    let _ = ag2.suggest(AutoKind::Assign, 1, 9_900, 8_000); // 1/2=50% > 5%？
    // done=1, total=2 → 5000bp > 500bp → RateOver
    chk(&mut set, "C506-04", ag2.ops.is_empty() && ag2.review_queue.contains(&1), "自动化率超上限转人工");

    // 回放撤销
    chk(&mut set, "C506-05", ag.undo(100).is_ok(), "误操作撤销成功");
    chk(&mut set, "C506-06", ag.undo_log == alloc::vec![100], "反向日志留痕");
    chk(&mut set, "C506-07", ag.undo(100) == Err(AuCode::UndoFail), "重复撤销拒绝");
    chk(&mut set, "C506-08", ag.undo(999) == Err(AuCode::UndoFail), "未知目标撤销失败");

    // 风暴熔断
    for i in 0..3u16 {
        let _ = ag.undo(101 + i);
    }
    chk(&mut set, "C506-09", ag.storm_check(), "连续撤销 3 次=风暴熔断");
    chk(&mut set, "C506-10", ag.fuse_blows == 1 && ag.review_queue.contains(&103), "熔断降级：未撤销项全转人工");

    // 率仪表
    let r = ag.rate_bp();
    chk(&mut set, "C506-11", r == 0, "熔断后有效自动化率 0（全人工）");
    chk(&mut set, "C506-12", AutoGov::new(10_000, 1).rate_bp() == 0, "空册率为 0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_gov() {
        let mut ag = AutoGov::new(10_000, 2);
        assert!(ag.suggest(AutoKind::Urge, 1, 9_000, 8_000).is_ok());
        assert!(ag.suggest(AutoKind::Urge, 2, 1_000, 8_000).is_err());
        assert!(ag.undo(1).is_ok());
        let _ = ag.undo(1);
        // undo_log 只有 1 条（1 成功撤销）
        assert_eq!(ag.undo_log.len(), 1);
    }
}
