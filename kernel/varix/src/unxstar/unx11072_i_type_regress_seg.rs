//! UNX-F11072 I 型回归段（0x1e900 段）
//!
//! B29–B33 联测判据在此重放：十个联合步骤按 prepare(0) → act(1) →
//! assert(2) → teardown(3) 四相编排，每步登记前置依赖位图、后置结果
//! 与相位。执行必须满足：依赖已全部完成、assert 前至少一个 act 完成、
//! teardown 前全部 assert 完成、后置检查失败即阻断。
//!
//! 不变式：executed == 已完成位数；阻断数必须为 0；步骤号段内唯一且
//! 前置依赖只能指向已登记步骤。冻结后 add/execute/rollback 全拒。
//!
//! 结构：JointStep（联合步骤）/ RunPlan（联测执行计划 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11072 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e900;

/// 空账：步骤号为 0 或查无此步
pub const IT_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：相位不在 0..=3
pub const IT_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：步骤号重复登记或重复执行
pub const IT_DUP: u32 = SEG_BASE + 0x03;
/// 失配：后置检查失败、依赖被回滚或相位越序
pub const IT_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：依赖未完成、步骤不足十席或 teardown 早于 assert
pub const IT_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：计划冻结后的编排/执行/回滚
pub const IT_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x72 标记 11072 号卡
pub const IT_TAG: u64 = 0x1e80_72;

/// 本段固定十席
const SEATS: usize = 10;
/// 相位：准备
const PHASE_PREPARE: u8 = 0;
/// 相位：执行
const PHASE_ACT: u8 = 1;
/// 相位：断言
const PHASE_ASSERT: u8 = 2;
/// 相位：善后
const PHASE_TEARDOWN: u8 = 3;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 单个联合步骤
#[derive(Clone, Debug)]
pub struct JointStep {
    /// 步骤号（1..=10）
    pub step: u16,
    /// 相位 0..=3
    pub phase: u8,
    /// 前置依赖位图（bit(step-1)）
    pub deps: u16,
    /// 0 = 未定/通过，1 = 后置失败
    pub outcome: u8,
    /// 后置检查是否通过
    pub post_ok: bool,
}

impl JointStep {
    /// 步骤号 → 位
    pub fn bit(step: u16) -> u16 {
        if step == 0 || step > 16 {
            0
        } else {
            1u16 << (step - 1)
        }
    }

    /// 新建步骤（后置默认为通过）
    pub fn new(step: u16, phase: u8, deps: u16) -> JointStep {
        JointStep { step, phase, deps, outcome: 0, post_ok: true }
    }

    /// 依赖是否已全部完成
    pub fn satisfied(&self, completed: u16) -> bool {
        self.deps & !completed == 0
    }

    /// 步骤指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.step);
        h = fnv_byte(h, self.phase);
        h = fnv_u16(h, self.deps);
        h = fnv_byte(h, self.outcome);
        fnv_byte(h, if self.post_ok { 1 } else { 0 })
    }
}

/// B29–B33 联测执行计划
#[derive(Clone, Debug)]
pub struct RunPlan {
    /// 步骤表
    pub steps: Vec<JointStep>,
    /// 已完成位图
    pub completed: u16,
    /// 已登记位图（依赖合法性用）
    pub added: u16,
    /// 已执行步数
    pub executed: u16,
    /// 被阻断步数
    pub blocked: u16,
    frozen: Option<u64>,
}

impl RunPlan {
    /// 空计划
    pub fn new() -> RunPlan {
        RunPlan { steps: Vec::new(), completed: 0, added: 0, executed: 0, blocked: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(IT_FROZEN);
        }
        Ok(())
    }

    fn index(&self, step: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].step == step {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 编排一步：0 号 / 非法相位 / 重号 / 依赖未来步 四道闸
    pub fn add(&mut self, step: JointStep) -> Result<(), u32> {
        self.guard()?;
        if step.step == 0 {
            return Err(IT_EMPTY);
        }
        if step.phase > PHASE_TEARDOWN {
            return Err(IT_RANGE);
        }
        if self.index(step.step).is_some() {
            return Err(IT_DUP);
        }
        if step.deps & !self.added != 0 {
            return Err(IT_SHORT);
        }
        self.added |= JointStep::bit(step.step);
        self.steps.push(step);
        Ok(())
    }

    fn act_mask(&self) -> u16 {
        let mut m = 0u16;
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].phase == PHASE_ACT {
                m |= JointStep::bit(self.steps[k].step);
            }
            k += 1;
        }
        m
    }

    fn assert_total(&self) -> u16 {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].phase == PHASE_ASSERT {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    fn assert_done(&self) -> u16 {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.steps.len() {
            let s = &self.steps[k];
            if s.phase == PHASE_ASSERT && (self.completed & JointStep::bit(s.step)) != 0 {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 执行一步：依赖/相位/后置三关
    pub fn execute(&mut self, step: u16) -> Result<u8, u32> {
        self.guard()?;
        let idx = match self.index(step) {
            Some(i) => i,
            None => return Err(IT_EMPTY),
        };
        if !self.steps[idx].satisfied(self.completed) {
            return Err(IT_SHORT);
        }
        let bit = JointStep::bit(step);
        if self.completed & bit != 0 {
            return Err(IT_DUP);
        }
        if self.steps[idx].phase == PHASE_ASSERT && (self.completed & self.act_mask()) == 0 {
            return Err(IT_MISMATCH);
        }
        if self.steps[idx].phase == PHASE_TEARDOWN && self.assert_done() < self.assert_total() {
            return Err(IT_SHORT);
        }
        if !self.steps[idx].post_ok {
            self.steps[idx].outcome = 1;
            self.blocked = self.blocked.wrapping_add(1);
            return Err(IT_MISMATCH);
        }
        self.completed |= bit;
        self.executed = self.executed.wrapping_add(1);
        Ok(self.steps[idx].phase)
    }

    /// 回滚一步：无已完成后继才能撤
    pub fn rollback(&mut self, step: u16) -> Result<(), u32> {
        self.guard()?;
        let bit = JointStep::bit(step);
        if bit == 0 || (self.completed & bit) == 0 {
            return Err(IT_EMPTY);
        }
        let mut k = 0usize;
        while k < self.steps.len() {
            let s = &self.steps[k];
            if s.step != step && (self.completed & JointStep::bit(s.step)) != 0 {
                if s.deps & bit != 0 {
                    return Err(IT_MISMATCH);
                }
            }
            k += 1;
        }
        if self.executed == 0 {
            return Err(IT_SHORT);
        }
        self.completed &= !bit;
        self.executed = self.executed.wrapping_sub(1);
        let idx = match self.index(step) {
            Some(i) => i,
            None => return Err(IT_EMPTY),
        };
        self.steps[idx].outcome = 0;
        Ok(())
    }

    /// 相位统计：(prepare, act, assert, teardown)
    pub fn phase_counts(&self) -> (u16, u16, u16, u16) {
        let mut p = [0u16; 4];
        let mut k = 0usize;
        while k < self.steps.len() {
            let ph = self.steps[k].phase as usize;
            if ph < 4 {
                p[ph] = p[ph].wrapping_add(1);
            }
            k += 1;
        }
        (p[0], p[1], p[2], p[3])
    }

    /// 联合不变量：全完成、无阻断、executed 对齐
    pub fn joint_invariant(&self) -> bool {
        if self.completed.count_ones() as usize != self.steps.len() {
            return false;
        }
        if self.executed as usize != self.steps.len() || self.blocked != 0 {
            return false;
        }
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].outcome != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 步骤号唯一且递增
    pub fn ids_ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.steps.len() {
            if self.steps[k].step <= self.steps[k - 1].step {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 重放：不足十席 IT_SHORT，未全完成 IT_MISMATCH
    pub fn verify(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.steps.len() < SEATS {
            return Err(IT_SHORT);
        }
        let mut ok = 0u16;
        let mut k = 0usize;
        while k < self.steps.len() {
            let s = &self.steps[k];
            if (self.completed & JointStep::bit(s.step)) != 0 && s.outcome == 0 {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        if ok as usize != self.steps.len() || !self.joint_invariant() {
            return Err(IT_MISMATCH);
        }
        Ok(ok)
    }

    /// 计划指纹
    pub fn fold(&self) -> u64 {
        let mut h = IT_TAG;
        h = fnv_u32(h, self.completed as u32);
        h = fnv_u32(h, self.added as u32);
        h = fnv_u16(h, self.executed);
        h = fnv_u16(h, self.blocked);
        let mut k = 0usize;
        while k < self.steps.len() {
            h = self.steps[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结计划
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：零步骤 / 非法相位 / 重步骤 / 依赖未来步
pub fn inject_four() -> [bool; 4] {
    let mut p0 = RunPlan::new();
    let empty = matches!(p0.add(JointStep::new(0, PHASE_ACT, 0)), Err(IT_EMPTY));
    let mut p1 = RunPlan::new();
    let range = matches!(p1.add(JointStep::new(1, 4, 0)), Err(IT_RANGE));
    let mut p2 = RunPlan::new();
    let first = p2.add(JointStep::new(1, PHASE_ACT, 0)).is_ok();
    let dup = first && matches!(p2.add(JointStep::new(1, PHASE_ACT, 0)), Err(IT_DUP));
    let mut p3 = RunPlan::new();
    let future = matches!(p3.add(JointStep::new(1, PHASE_ACT, JointStep::bit(9))), Err(IT_SHORT));
    [empty, range, dup, future]
}

/// 构造十步联测样本（2 准备 + 3 执行 + 2 断言 + 3 善后）
fn sample_plan() -> Result<RunPlan, u32> {
    let mut p = RunPlan::new();
    p.add(JointStep::new(1, PHASE_PREPARE, 0))?;
    p.add(JointStep::new(2, PHASE_PREPARE, 0))?;
    p.add(JointStep::new(3, PHASE_ACT, JointStep::bit(1)))?;
    p.add(JointStep::new(4, PHASE_ACT, JointStep::bit(1) | JointStep::bit(2)))?;
    p.add(JointStep::new(5, PHASE_ACT, JointStep::bit(3)))?;
    p.add(JointStep::new(6, PHASE_ASSERT, JointStep::bit(4)))?;
    p.add(JointStep::new(7, PHASE_ASSERT, JointStep::bit(5)))?;
    p.add(JointStep::new(
        8,
        PHASE_TEARDOWN,
        JointStep::bit(6) | JointStep::bit(7),
    ))?;
    p.add(JointStep::new(9, PHASE_TEARDOWN, JointStep::bit(8)))?;
    p.add(JointStep::new(10, PHASE_TEARDOWN, JointStep::bit(9)))?;
    let mut s = 1u16;
    while s <= 10 {
        p.execute(s)?;
        s = s.wrapping_add(1);
    }
    Ok(p)
}

/// UNX-F11072 判据入口
pub fn run_unx11072_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11072");
    let mut plan = match sample_plan() {
        Ok(v) => v,
        Err(_) => RunPlan::new(),
    };
    let hits = plan.verify().unwrap_or(0);
    s.add(
        "J1a-I型判据全量重放",
        hits == 10 && plan.steps.len() == SEATS,
        "B29–B33 十步必须全部执行且后置通过",
    );
    let (prep, act, assert, tear) = plan.phase_counts();
    s.add(
        "J1b-联测语义一致",
        plan.joint_invariant() && prep == 2 && act == 3 && assert == 2 && tear == 3,
        "2/3/2/3 相位账与联合不变量必须成立",
    );
    s.add(
        "J2-执行守恒",
        plan.executed == 10
            && plan.completed.count_ones() == 10
            && plan.executed as usize == plan.steps.len(),
        "executed 必须等于已完成位数与步数",
    );
    let inj = inject_four();
    s.add("J3a-零步骤拒止", inj[0], "step=0 编排必须 IT_EMPTY");
    s.add("J3b-非法相位拒", inj[1], "相位 4 必须 IT_RANGE");
    s.add("J3c-重步骤拒止", inj[2], "同号二次编排必须 IT_DUP");
    s.add("J3d-未来依赖拒", inj[3], "依赖未登记步骤必须 IT_SHORT");
    let mut fz = match sample_plan() {
        Ok(v) => v,
        Err(_) => RunPlan::new(),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.execute(1), Err(IT_FROZEN))
        && matches!(fz.add(JointStep::new(11, PHASE_ACT, 0)), Err(IT_FROZEN))
        && matches!(fz.rollback(10), Err(IT_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后执行/编排/回滚全拒且指纹不变",
    );
    let fp_a = plan.clone().fold();
    let mut b = match sample_plan() {
        Ok(v) => v,
        Err(_) => RunPlan::new(),
    };
    let sens = b.rollback(10).is_ok() && b.fold() != fp_a;
    s.add("J5-指纹敏感性", sens, "回滚末步必须改变 fold");
    let dep_short = {
        let mut g = RunPlan::new();
        g.add(JointStep::new(1, PHASE_ACT, 0)).is_ok()
            && g.add(JointStep::new(2, PHASE_ASSERT, JointStep::bit(1))).is_ok()
            && matches!(g.execute(2), Err(IT_SHORT))
    };
    let assert_early = {
        let mut g = RunPlan::new();
        g.add(JointStep::new(1, PHASE_PREPARE, 0)).is_ok()
            && g.add(JointStep::new(2, PHASE_ASSERT, 0)).is_ok()
            && matches!(g.execute(2), Err(IT_MISMATCH))
    };
    let post_fail = {
        let mut g = RunPlan::new();
        let mut st = JointStep::new(1, PHASE_ACT, 0);
        st.post_ok = false;
        match g.add(st) {
            Ok(()) => matches!(g.execute(1), Err(IT_MISMATCH)) && g.blocked == 1,
            Err(_) => false,
        }
    };
    s.add(
        "J6-相位与后置拒止",
        dep_short && assert_early && post_fail,
        "依赖未完成/断言早于执行/后置失败三路都必须拒",
    );
    let mut unique = plan.ids_ordered();
    let mut k = 0usize;
    while k < plan.steps.len() {
        if plan.steps[k].step == 0 {
            unique = false;
        }
        k += 1;
    }
    let dup_reject = {
        let mut g = RunPlan::new();
        g.add(JointStep::new(1, PHASE_PREPARE, 0)).is_ok()
            && matches!(g.add(JointStep::new(1, PHASE_ACT, 0)), Err(IT_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        unique && dup_reject,
        "步骤号必须唯一递增且重号当场拒",
    );
    let mut partial = match sample_plan() {
        Ok(v) => v,
        Err(_) => RunPlan::new(),
    };
    while partial.steps.len() > 5 {
        if !partial.steps.is_empty() {
            partial.steps.pop();
        }
    }
    let negative = partial.steps.len() == 5 && matches!(partial.verify(), Err(IT_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五步半账必须 IT_SHORT，不得伪装成绿",
    );
    let teardown_early = {
        let mut g = RunPlan::new();
        let ok_add = g.add(JointStep::new(1, PHASE_ACT, 0)).is_ok()
            && g.add(JointStep::new(2, PHASE_ASSERT, JointStep::bit(1))).is_ok()
            && g.add(JointStep::new(3, PHASE_TEARDOWN, 0)).is_ok();
        ok_add && g.execute(1).is_ok() && matches!(g.execute(3), Err(IT_SHORT))
    };
    let rollback_block = {
        let mut g = RunPlan::new();
        let ok_add = g.add(JointStep::new(1, PHASE_ACT, 0)).is_ok()
            && g.add(JointStep::new(2, PHASE_ASSERT, JointStep::bit(1))).is_ok();
        ok_add
            && g.execute(1).is_ok()
            && g.execute(2).is_ok()
            && matches!(g.rollback(1), Err(IT_MISMATCH))
    };
    s.add(
        "J10-teardown与回滚纪律",
        teardown_early && rollback_block,
        "assert 未完禁 teardown、有后继禁回滚",
    );
    s.add_dyn(
        "J9-动态账",
        hits == 10 && plan.blocked == 0,
        &format!(
            "hits={} executed={} 相位={}/{}/{}/{}",
            hits, plan.executed, prep, act, assert, tear
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        plan.fold() != 0,
        &format!("fold={:016x} tag={:x}", plan.fold(), IT_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11072_flow() {
        let mut plan = sample_plan().unwrap_or(RunPlan::new());
        assert_eq!(plan.verify().unwrap_or(0), 10);
        assert!(plan.joint_invariant());
        assert_eq!(plan.phase_counts(), (2, 3, 2, 3));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11072_checks_green() {
        assert!(run_unx11072_checks().all_passed(), "UNX-F11072 判据未全绿");
    }
}
