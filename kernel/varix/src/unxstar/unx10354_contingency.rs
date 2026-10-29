//! UNX-F10354 演习应急预案（故障处置预置）（0x1b6c0 段）
//!
//! 锚点：UNX-F10354-J1 演习应急预案——四类故障（挂起/泄漏/失速/失真）
//! 各预置一条处置（动作码+时长预算），预算不得超帽且不得为零，预案齐
//! 后逐类演练签押，未预置不得演练，四类全预置全演练方算预案成账。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：预案册（类+动作+预算三字段行）+ 预算帽核（零预算/超帽即拒）+
//! 演练核（未预置不演/重复不演）+ 成账核（四类全预置全演练）+
//! 注入核（重类/无预演/超帽/坏类四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b6c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b6c0;
pub const CF_BUDGET: u32 = SEG_BASE + 0x01; // 预算越界（零或超帽）
pub const CF_CLASS: u32 = SEG_BASE + 0x02; // 故障类越界
pub const CF_ACTION: u32 = SEG_BASE + 0x03; // 动作码越界
pub const CF_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const CF_DUP: u32 = SEG_BASE + 0x05; // 类重复预置
pub const CF_NOPLAN: u32 = SEG_BASE + 0x06; // 未预置先演练
pub const CF_REDRILL: u32 = SEG_BASE + 0x07; // 重复演练
pub const CF_NOTREADY: u32 = SEG_BASE + 0x08; // 预案未成即封

/// 故障四类。
pub const FC_HANG: u8 = 1;
pub const FC_LEAK: u8 = 2;
pub const FC_STALL: u8 = 3;
pub const FC_CORRUPT: u8 = 4;
pub const FC_N: u8 = 5;

/// 处置动作三型。
pub const ACT_KILL: u8 = 1;
pub const ACT_RECLAIM: u8 = 2;
pub const ACT_DEGRADE: u8 = 3;
pub const ACT_N: u8 = 4;

/// 预算帽（演练时长上限，tick）。
pub const BUDGET_MAX: u32 = 5000;
/// 类数锚（四类全预置）。
pub const CLASSES_NEEDED: usize = 4;
/// 册定容。
pub const CAP: usize = 8;

/// 预案行：故障类 + 处置动作 + 时长预算。
#[derive(Clone)]
pub struct PlanRow {
    pub fclass: u8,
    pub action: u8,
    pub budget: u32,
    pub drilled: bool,
}

/// 应急预案册。
#[derive(Clone)]
pub struct ContingencyPlan {
    pub rows: Vec<PlanRow>,
    pub sealed: bool,
}

impl ContingencyPlan {
    /// 空册。
    pub fn new() -> Self {
        ContingencyPlan {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 找类。
    pub fn find(&self, fclass: u8) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].fclass == fclass {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 预置一条：类/动作/预算三重核。
    pub fn preset(&mut self, fclass: u8, action: u8, budget: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_NOTREADY);
        }
        if fclass < FC_HANG || fclass >= FC_N {
            return Err(CF_CLASS);
        }
        if action < ACT_KILL || action >= ACT_N {
            return Err(CF_ACTION);
        }
        if budget == 0 || budget > BUDGET_MAX {
            return Err(CF_BUDGET);
        }
        if self.rows.len() >= CAP {
            return Err(CF_CAP);
        }
        if self.find(fclass).is_some() {
            return Err(CF_DUP);
        }
        self.rows.push(PlanRow {
            fclass,
            action,
            budget,
            drilled: false,
        });
        Ok(())
    }

    /// 演练签押：未预置不演，重复不演。
    pub fn drill(&mut self, fclass: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_NOTREADY);
        }
        let idx = match self.find(fclass) {
            Some(i) => i,
            None => return Err(CF_NOPLAN),
        };
        if self.rows[idx].drilled {
            return Err(CF_REDRILL);
        }
        self.rows[idx].drilled = true;
        Ok(())
    }

    /// 某类剩余预算。
    pub fn remaining(&self, fclass: u8) -> u32 {
        match self.find(fclass) {
            Some(i) => BUDGET_MAX - self.rows[i].budget,
            None => 0,
        }
    }

    /// 未预置类数。
    pub fn unpreset(&self) -> usize {
        let mut n = 0;
        let mut c = FC_HANG;
        while c < FC_N {
            if self.find(c).is_none() {
                n += 1;
            }
            c += 1;
        }
        n
    }

    /// 未演练类数。
    pub fn undrilled(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if !self.rows[i].drilled {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 预案成账：四类全预置且全演练。
    pub fn plan_ok(&self) -> bool {
        self.unpreset() == 0 && self.undrilled() == 0 && self.rows.len() == CLASSES_NEEDED
    }

    /// 总预算。
    pub fn budget_total(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < self.rows.len() {
            t += self.rows[i].budget;
            i += 1;
        }
        t
    }

    /// 封账：预案未成即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_NOTREADY);
        }
        if !self.plan_ok() {
            return Err(CF_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：类/动作/预算/演练位逐行叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6c01b6;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ (r.fclass as u32)).wrapping_mul(0x01000193);
            h = (h ^ (r.action as u32)).wrapping_mul(0x01000193);
            h = (h ^ r.budget).wrapping_mul(0x01000193);
            let bit = if r.drilled { 1u32 } else { 0 };
            h = (h ^ bit).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

/// 预案样例：四类各一条（类, 动作, 预算）。
pub fn plan_samples() -> [(u8, u8, u32); 4] {
    [
        (FC_HANG, ACT_KILL, 1200),
        (FC_LEAK, ACT_RECLAIM, 2400),
        (FC_STALL, ACT_DEGRADE, 800),
        (FC_CORRUPT, ACT_KILL, 3600),
    ]
}

/// 注入核：四路坏账必抓。
/// 0: 类重复预置；1: 未预置先演练；2: 预算超帽；3: 坏类。
pub fn inject_four() -> [bool; 4] {
    let mut p = ContingencyPlan::new();
    let mut k = 0;
    while k < plan_samples().len() {
        let (c, a, b) = plan_samples()[k];
        let _ = p.preset(c, a, b);
        k += 1;
    }
    let d0 = p.preset(FC_HANG, ACT_KILL, 600).err() == Some(CF_DUP);
    let d1 = p.drill(FC_N).err() == Some(CF_NOPLAN);
    let d2 = p.preset(FC_N + 9, ACT_KILL, BUDGET_MAX + 1).err() == Some(CF_CLASS);
    let d3 = {
        let mut q = ContingencyPlan::new();
        q.preset(FC_HANG, ACT_KILL, BUDGET_MAX + 1).err() == Some(CF_BUDGET)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10354_checks。
pub fn run_unx10354_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10354");
    // J1 正路径：四类全预置。
    let mut p = ContingencyPlan::new();
    let mut j1 = true;
    let mut k = 0;
    while k < plan_samples().len() {
        let (c, a, b) = plan_samples()[k];
        if p.preset(c, a, b).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-四类预置", j1 && p.rows.len() == 4, "四类各一条处置入册");
    // J2 边界：零预算拒。
    let mut p2 = ContingencyPlan::new();
    let j2 = p2.preset(FC_HANG, ACT_KILL, 0).err() == Some(CF_BUDGET);
    s.add("J2-零预拒", j2, "零预算即 CF_BUDGET");
    // J3 边界：坏动作拒。
    let mut p3 = ContingencyPlan::new();
    let j3 = p3.preset(FC_HANG, 0, 100).err() == Some(CF_ACTION)
        && p3.preset(FC_HANG, 4, 100).err() == Some(CF_ACTION);
    s.add("J3-坏动拒", j3, "动作码越界即 CF_ACTION");
    // J4 注入：预算超帽必抓。
    let mut p4 = ContingencyPlan::new();
    let j4 = p4
        .preset(FC_LEAK, ACT_RECLAIM, BUDGET_MAX + 1)
        .err()
        == Some(CF_BUDGET);
    s.add("J4-超帽抓", j4, "预算超帽即 CF_BUDGET");
    // J5 注入：类重复预置必抓。
    let mut p5 = ContingencyPlan::new();
    let _ = p5.preset(FC_STALL, ACT_DEGRADE, 800);
    let j5 = p5.preset(FC_STALL, ACT_KILL, 500).err() == Some(CF_DUP);
    s.add("J5-重类抓", j5, "一类两预置即 CF_DUP");
    // J6 注入：未预置先演练必抓。
    let mut p6 = ContingencyPlan::new();
    let _ = p6.preset(FC_HANG, ACT_KILL, 1200);
    let j6 = p6.drill(FC_CORRUPT).err() == Some(CF_NOPLAN);
    s.add("J6-无演抓", j6, "未预置先演练即 CF_NOPLAN");
    // J7 注入：重复演练必抓。
    let mut p7 = ContingencyPlan::new();
    let _ = p7.preset(FC_HANG, ACT_KILL, 1200);
    let _ = p7.drill(FC_HANG);
    let j7 = p7.drill(FC_HANG).err() == Some(CF_REDRILL);
    s.add("J7-重演抓", j7, "重复演练即 CF_REDRILL");
    // J8 边界：坏类号拒。
    let mut p8 = ContingencyPlan::new();
    let j8 = p8.preset(0, ACT_KILL, 100).err() == Some(CF_CLASS)
        && p8.preset(5, ACT_KILL, 100).err() == Some(CF_CLASS);
    s.add("J8-坏类拒", j8, "类号越界即 CF_CLASS");
    // J9 正路径：四类全演练后成账。
    let mut p9 = p.clone();
    let mut c9 = FC_HANG;
    let mut ok9 = true;
    while c9 < FC_N {
        if p9.drill(c9).is_err() {
            ok9 = false;
        }
        c9 += 1;
    }
    let j9 = ok9 && p9.plan_ok() && p9.undrilled() == 0;
    s.add("J9-预案成", j9, "四类全预置全演练");
    // J10 正路径：成账后封账。
    let mut p10 = p9.clone();
    let j10 = p10.seal().is_ok() && p10.sealed;
    s.add("J10-封账成", j10, "预案成后封账通过");
    // J11 边界：未成账封账拒。
    let mut p11 = p.clone();
    let j11 = p11.seal().err() == Some(CF_NOTREADY) && p11.undrilled() == 4;
    s.add("J11-缺封拒", j11, "未演练封账即 CF_NOTREADY");
    // J12 边界：剩余预算核。
    let rem = p.remaining(FC_HANG) + p.remaining(FC_LEAK) + p.remaining(FC_STALL) + p.remaining(FC_CORRUPT);
    let j12 = rem == 4 * BUDGET_MAX - p.budget_total();
    s.add("J12-余预核", j12, "四类剩余预算之和与总账对平");
    // J13 动态：预算分布与指纹。
    let j13 = p.budget_total() == 8000 && p.fold() != 0;
    s.add_dyn(
        "J13-预算出数",
        j13,
        &format!(
            "四类预算合计 {} tick（帽 {}），指纹 {:08x}",
            p.budget_total(),
            BUDGET_MAX,
            p.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重类/无演/超帽/坏类四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut p = ContingencyPlan::new();
        let mut k = 0;
        while k < plan_samples().len() {
            let (c, a, b) = plan_samples()[k];
            assert!(p.preset(c, a, b).is_ok());
            k += 1;
        }
        assert!(p.preset(FC_HANG, ACT_KILL, 600).err() == Some(CF_DUP));
        let mut c = FC_HANG;
        while c < FC_N {
            assert!(p.drill(c).is_ok());
            c += 1;
        }
        assert!(p.plan_ok());
        assert!(p.seal().is_ok());
        assert!(p.drill(FC_LEAK).err() == Some(CF_NOTREADY));
        assert!(p.fold() != 0);
        assert_eq!(p.budget_total(), 8000);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10354_checks();
        assert!(s.all_passed());
    }
}
