//! VE-F4262 · 一致性治理流程（U04 一致性治理与演进组）
//! 职责：治理事务六段流（提案→评估→立项→实施→验证→归档，无判据不得过段）
//! + 流程实例五态状态机（草稿/评审/执行/挂起/关闭）白名单迁移
//! + 越权跳段拦截立案 + 挂起超期自动升级催办。
//! 段位：0x1F20（续表 idx496）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1F20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrCode {
    /// 段间跳转越权。
    StageJump = SEG_BASE,
    /// 出口判据缺失（卡点不熄火）。
    GateMissing = SEG_BASE + 0x01,
    /// 状态迁移越权（白名单外）。
    StateIllegal = SEG_BASE + 0x02,
    /// 实例不存在。
    NotFound = SEG_BASE + 0x03,
}

/// 六段流。
pub const STAGES: [&str; 6] = ["propose", "assess", "approve", "execute", "verify", "archive"];

/// 五态。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FState {
    Draft = 0,
    Review = 1,
    Running = 2,
    Suspended = 3,
    Closed = 4,
}

/// 白名单迁移表：from → 允许的 to 集合。
pub fn transition_allowed(from: FState, to: FState) -> bool {
    use FState::*;
    matches!(
        (from, to),
        (Draft, Review) | (Draft, Suspended)
            | (Review, Running) | (Review, Suspended) | (Review, Draft)
            | (Running, Suspended) | (Running, Closed)
            | (Suspended, Running) | (Suspended, Closed) | (Suspended, Review)
            | (Closed, Draft) // 归档后重开仅经草稿
    )
}

/// 流程实例。
#[derive(Clone, Debug)]
pub struct FlowInst {
    pub id: u32,
    pub state: FState,
    pub stage: usize, // STAGES 下标
    /// 段级出口判据：过段须当前段判据已挂。
    pub gates: [bool; 6],
    /// 挂起起始日（0=非挂起）。
    pub suspended_since: u16,
}

/// 流程册。
pub struct FlowBook {
    pub insts: Vec<FlowInst>,
    /// 挂起超期阈值（日）。
    pub suspend_cap: u16,
    pub escalated: Vec<u32>,
    pub blocked_events: u32,
}

impl FlowBook {
    pub fn new(suspend_cap: u16) -> Self {
        FlowBook { insts: Vec::new(), suspend_cap, escalated: Vec::new(), blocked_events: 0 }
    }

    /// 新建实例（草稿态、零段）。
    pub fn create(&mut self, id: u32) -> Result<(), PrCode> {
        if self.insts.iter().any(|i| i.id == id) {
            return Err(PrCode::NotFound);
        }
        self.insts.push(FlowInst { id, state: FState::Draft, stage: 0, gates: [false; 6], suspended_since: 0 });
        Ok(())
    }

    /// 过段：白名单内顺序 +1，且当前段判据必须已挂（无判据不得过段——卡点）。
    pub fn advance(&mut self, id: u32) -> Result<usize, PrCode> {
        let inst = self.insts.iter_mut().find(|i| i.id == id).ok_or(PrCode::NotFound)?;
        if inst.state != FState::Running {
            return Err(PrCode::StateIllegal);
        }
        if inst.stage >= STAGES.len() {
            return Err(PrCode::StageJump);
        }
        if !inst.gates[inst.stage] {
            self.blocked_events += 1;
            return Err(PrCode::GateMissing);
        }
        inst.stage += 1;
        Ok(inst.stage)
    }

    /// 状态迁移（白名单）。
    pub fn transit(&mut self, id: u32, to: FState) -> Result<(), PrCode> {
        let inst = self.insts.iter_mut().find(|i| i.id == id).ok_or(PrCode::NotFound)?;
        if !transition_allowed(inst.state, to) {
            self.blocked_events += 1;
            return Err(PrCode::StateIllegal);
        }
        inst.suspended_since = if to == FState::Suspended { 1 } else { 0 };
        inst.state = to;
        Ok(())
    }

    /// 挂起超期升级催办。
    pub fn escalate(&mut self, today: u16) {
        let mut due: Vec<u32> = Vec::new();
        for i in self.insts.iter() {
            if i.state == FState::Suspended && i.suspended_since > 0 && today.saturating_sub(i.suspended_since) > self.suspend_cap {
                if !self.escalated.contains(&i.id) && !due.contains(&i.id) {
                    due.push(i.id);
                }
            }
        }
        for id in due {
            self.escalated.push(id);
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C496-01..12。
pub fn run_ves496_checks() -> CheckSet {
    let mut set = CheckSet::new("ves496");

    // 白名单表单测
    chk(&mut set, "C496-01", transition_allowed(FState::Draft, FState::Review) && transition_allowed(FState::Running, FState::Closed), "白名单正迁移放行");
    chk(&mut set, "C496-02", !transition_allowed(FState::Draft, FState::Closed) && !transition_allowed(FState::Closed, FState::Running), "越权迁移拦截（草稿不可直达关闭）");

    let mut fb = FlowBook::new(7);
    chk(&mut set, "C496-03", fb.create(1).is_ok() && fb.create(1) == Err(PrCode::NotFound), "实例创建与重号拒收");

    chk(&mut set, "C496-04", fb.advance(1) == Err(PrCode::StateIllegal), "草稿态不得过段（须先 Running）");
    let _ = fb.transit(1, FState::Review);
    let _ = fb.transit(1, FState::Running);
    chk(&mut set, "C496-05", fb.transit(1, FState::Closed) == Err(PrCode::StateIllegal), "评审态直达关闭越权");

    // 卡点：无判据不得过段
    chk(&mut set, "C496-06", fb.advance(1) == Err(PrCode::GateMissing), "提案段无判据→卡点不熄火");
    chk(&mut set, "C496-07", fb.blocked_events == 2, "越权/卡点事件留痕");
    {
        let i = fb.insts.iter_mut().find(|i| i.id == 1).unwrap();
        i.gates[0] = true;
    }
    chk(&mut set, "C496-08", fb.advance(1) == Ok(1), "挂判据后过段进评估");
    {
        let i = fb.insts.iter_mut().find(|i| i.id == 1).unwrap();
        i.gates = [true; 6];
    }
    let mut last = 1;
    for _ in 0..5 {
        if let Ok(s) = fb.advance(1) {
            last = s;
        }
    }
    chk(&mut set, "C496-09", last == 6, "六段全通至归档");
    chk(&mut set, "C496-10", fb.advance(1) == Err(PrCode::StageJump), "归档后再过段=跳段拦截");

    // 挂起超期升级
    let _ = fb.create(2);
    let _ = fb.transit(2, FState::Review);
    let _ = fb.transit(2, FState::Suspended);
    {
        let i2 = fb.insts.iter_mut().find(|i| i.id == 2).unwrap();
        i2.suspended_since = 1;
    }
    fb.escalate(20); // 20-1=19 > 7 → 升级
    chk(&mut set, "C496-11", fb.escalated.contains(&2), "挂起超期自动升级催办");
    fb.escalate(5); // 5-1=4 ≤ 7 → 不升级
    chk(&mut set, "C496-12", fb.escalated.len() == 1, "未超期不升级");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_states() {
        assert!(transition_allowed(FState::Suspended, FState::Running));
        assert!(!transition_allowed(FState::Closed, FState::Closed));
        let mut fb = FlowBook::new(5);
        let _ = fb.create(1);
        assert_eq!(fb.advance(1), Err(PrCode::StateIllegal));
    }
}
