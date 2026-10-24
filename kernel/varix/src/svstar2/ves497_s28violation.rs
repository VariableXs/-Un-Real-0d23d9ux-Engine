//! VE-F4263 · 违例处置流程（U04 一致性治理与演进组）
//! 职责：违例六步全链（发现→定级→派单→修复→复核→销账）+ 红黄绿三级
//! （即修/包前修/记账攒批，与主工程缺陷账本口径同源）+ 红线单列即时阻断
//! + 销账未复核账目锁死 + 定级争议仲裁升级。
//! 段位：0x1F60（续表 idx497）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1F60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VlCode {
    /// 定级争议（升级仲裁）。
    LevelDispute = SEG_BASE,
    /// 销账未复核（账目锁死）。
    CloseUnreviewed = SEG_BASE + 0x01,
    /// 红线违例（即时阻断永不入池）。
    RedlineHit = SEG_BASE + 0x02,
    /// 工单步骤越序。
    StepJump = SEG_BASE + 0x03,
    /// 账目不存在。
    NotFound = SEG_BASE + 0x04,
}

/// 三级分级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Level {
    Green = 0, // 记账攒批
    Yellow = 1, // 包前修
    Red = 2,   // 即修
}

/// 违例条目（账本）。
#[derive(Clone, Copy, Debug)]
pub struct Violation {
    pub id: u32,
    pub level: Level,
    pub redline: bool,
    pub step: u8, // 六步 0..6
    pub reviewed: bool,
    pub closed: bool,
    pub locked: bool,
}

/// 违例账本。
pub struct ViolationBook {
    pub items: Vec<Violation>,
    /// 仲裁升级账。
    pub disputes: Vec<u32>,
    /// 红线通报三要素齐备标记。
    pub redline_reports: u32,
}

pub const STEPS: [&str; 6] = ["found", "graded", "assigned", "fixed", "reviewed", "closed"];

impl ViolationBook {
    pub fn new() -> Self {
        ViolationBook { items: Vec::new(), disputes: Vec::new(), redline_reports: 0 }
    }

    /// 发现并定级（红线单列：即时阻断不入池——返回 Err 并通报）。
    pub fn found(&mut self, id: u32, level: Level, redline: bool) -> Result<(), VlCode> {
        if self.items.iter().any(|v| v.id == id) {
            return Err(VlCode::NotFound);
        }
        if redline {
            self.redline_reports += 1; // 通报三要素齐备即记
            return Err(VlCode::RedlineHit);
        }
        self.items.push(Violation { id, level, redline: false, step: 1, reviewed: false, closed: false, locked: false });
        Ok(())
    }

    /// 定级争议升级。
    pub fn dispute(&mut self, id: u32) -> Result<(), VlCode> {
        let v = self.items.iter().find(|v| v.id == id).ok_or(VlCode::NotFound)?;
        let _ = v.level;
        if !self.disputes.contains(&id) {
            self.disputes.push(id);
        }
        Err(VlCode::LevelDispute)
    }

    /// 步推进（六步顺序：发现→定级→派单→修复→复核→销账）。
    pub fn step(&mut self, id: u32) -> Result<u8, VlCode> {
        let v = self.items.iter_mut().find(|v| v.id == id).ok_or(VlCode::NotFound)?;
        if v.locked {
            return Err(VlCode::CloseUnreviewed);
        }
        if v.step >= 6 {
            return Err(VlCode::StepJump);
        }
        v.step += 1;
        if v.step == 5 {
            v.reviewed = true;
        }
        if v.step == 6 {
            // 销账必须先复核
            if !v.reviewed {
                v.locked = true;
                v.step = 5;
                return Err(VlCode::CloseUnreviewed);
            }
            v.closed = true;
        }
        Ok(v.step)
    }

    /// 批量派单：绿级攒批计数。
    pub fn batch_greens(&self) -> usize {
        self.items.iter().filter(|v| v.level == Level::Green && v.step < 4).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C497-01..12。
pub fn run_ves497_checks() -> CheckSet {
    let mut set = CheckSet::new("ves497");

    let mut bk = ViolationBook::new();

    // 红线单列
    chk(&mut set, "C497-01", bk.found(1, Level::Red, true) == Err(VlCode::RedlineHit), "红线违例即时阻断永不入池");
    chk(&mut set, "C497-02", bk.redline_reports == 1 && bk.items.is_empty(), "通报三要素记账、池内零残留");

    // 三级入池
    let _ = bk.found(2, Level::Red, false);
    let _ = bk.found(3, Level::Yellow, false);
    let _ = bk.found(4, Level::Green, false);
    chk(&mut set, "C497-03", bk.items.len() == 3, "三级入池（红/黄/绿）");
    chk(&mut set, "C497-04", bk.batch_greens() == 1, "绿级攒批计数");

    // 六步链：绿级全链
    for _ in 0..6 {
        let _ = bk.step(4);
    }
    chk(&mut set, "C497-05", bk.items.iter().find(|v| v.id == 4).unwrap().closed, "绿级六步全通销账");
    chk(&mut set, "C497-06", bk.step(4) == Err(VlCode::StepJump), "销账后再步进越序");

    // 销账未复核锁死：直接从 step 1 跳到销账不可能（顺序推进），构造未复核销账——手动置 step=5 但未复核
    let v = bk.items.iter_mut().find(|v| v.id == 3).unwrap();
    v.step = 4; // 已修复未复核
    drop(v);
    // 修复账目语义：step(3) 推进到 5（复核）→ 6（销账）正常；改为直接锁死测试：
    let v = bk.items.iter_mut().find(|v| v.id == 3).unwrap();
    v.reviewed = false;
    v.step = 5;
    drop(v);
    chk(&mut set, "C497-07", bk.step(3) == Err(VlCode::CloseUnreviewed), "销账未复核→账目锁死");
    chk(&mut set, "C497-08", bk.items.iter().find(|v| v.id == 3).unwrap().locked, "锁死位在册");
    chk(&mut set, "C497-09", bk.step(3) == Err(VlCode::CloseUnreviewed), "锁死账目拒绝一切步进");

    // 定级争议仲裁升级
    chk(&mut set, "C497-10", bk.dispute(2) == Err(VlCode::LevelDispute), "红级定级争议升级");
    let _ = bk.dispute(3);
    chk(&mut set, "C497-11", bk.disputes == alloc::vec![2, 3], "争议账保序去重");

    chk(&mut set, "C497-12", bk.step(99) == Err(VlCode::NotFound), "未知账目拒收");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn violation_flow() {
        let mut bk = ViolationBook::new();
        assert!(bk.found(1, Level::Green, true).is_err());
        assert!(bk.found(2, Level::Green, false).is_ok());
        for _ in 0..6 {
            assert!(bk.step(2).is_ok());
        }
        assert!(bk.items[0].closed);
        assert!(bk.step(2).is_err());
    }
}
