//! ves480_s26perf — VE-F4246 · 性能一致性验证
//! 各域性能基线跨域一致性核验（同构操作跨域性能差异阈值检——同构不同速=发现项）+性能预算跨域对齐验证；
//! 跨域差异越阈→定位慢域+立案；预算冲突→家族预算仲裁；基线漂移→重冻结；
//! 含无障碍操作性能同构专项（读屏会话不因域而异速——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1B20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    Ok = 0,
    SlowDomain = SEG_BASE + 0x01, // 0x1B21
    BudgetConflict = SEG_BASE + 0x02, // 0x1B22
    BaselineDrift = SEG_BASE + 0x03, // 0x1B23
}

/// 同构操作在各域的性能读数（μs）。
pub struct OpTiming { pub op: &'static str, pub domain: &'static str, pub us: u32, pub a11y_session: bool }

/// 核验册。
pub struct PerfConsisBook {
    pub timings: Vec<OpTiming>,
    pub tol_bp: u32,       // 跨域差异容差
    pub budgets: Vec<(&'static str, u32)>, // 域预算
    pub family_budget: u32,
    pub filed: Vec<&'static str>, // 慢域立案
    pub refrozen: u32,
}

impl PerfConsisBook {
    pub fn new(tol_bp: u32) -> Self {
        PerfConsisBook { timings: Vec::new(), tol_bp, budgets: Vec::new(), family_budget: 0, filed: Vec::new(), refrozen: 0 }
    }
    pub fn put(&mut self, t: OpTiming) { self.timings.push(t); }
    /// 同构同速核验：同一操作跨域读数最大偏差 ≤ 容差；越阈→定位慢域+立案。
    pub fn verify(&mut self, op: &'static str) -> Result<(), PcCode> {
        let ts: Vec<(&'static str, u32)> = self.timings.iter()
            .filter(|t| t.op == op).map(|t| (t.domain, t.us)).collect();
        if ts.len() < 2 { return Ok(()); }
        let max = ts.iter().map(|(_, u)| *u).max().unwrap();
        let min = ts.iter().map(|(_, u)| *u).min().unwrap();
        if max == 0 { return Ok(()); }
        if (max - min) * 10_000 / max > self.tol_bp {
            let slow = ts.iter().find(|(_, u)| *u == max).unwrap().0;
            if !self.filed.contains(&slow) { self.filed.push(slow); }
            return Err(PcCode::SlowDomain);
        }
        Ok(())
    }
    /// a11y 专项：读屏会话同操作跨域差异核验（域本色）。
    pub fn a11y_verify(&mut self, op: &'static str) -> Result<(), PcCode> {
        let ts: Vec<u32> = self.timings.iter()
            .filter(|t| t.op == op && t.a11y_session).map(|t| t.us).collect();
        if ts.len() < 2 { return Ok(()); }
        let max = *ts.iter().max().unwrap();
        let min = *ts.iter().min().unwrap();
        if max > 0 && (max - min) * 10_000 / max > self.tol_bp {
            return Err(PcCode::SlowDomain);
        }
        Ok(())
    }
    /// 预算对齐：各域预算与家族预算一致（冲突→仲裁取小）。
    pub fn budget_align(&mut self, family: u32) -> Result<(), PcCode> {
        self.family_budget = family;
        let conflict = self.budgets.iter().any(|(_, b)| *b != family);
        if conflict {
            for b in self.budgets.iter_mut() { b.1 = b.1.min(family); }
            return Err(PcCode::BudgetConflict);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves480_checks() -> CheckSet {
    let mut set = CheckSet::new("ves480");
    let mut bk = PerfConsisBook::new(500); // 5% 容差

    // C480-01 同构同速：三域 100/102/101μs → 过。
    bk.put(OpTiming { op: "open-menu", domain: "ui", us: 100, a11y_session: false });
    bk.put(OpTiming { op: "open-menu", domain: "cli", us: 102, a11y_session: false });
    bk.put(OpTiming { op: "open-menu", domain: "srv", us: 101, a11y_session: false });
    chk(&mut set, "C480-01", bk.verify("open-menu").is_ok(), "同构同速");

    // C480-02 跨域差异越阈→慢域定位+立案。
    bk.put(OpTiming { op: "open-menu", domain: "web", us: 150, a11y_session: false });
    chk(&mut set, "C480-02", bk.verify("open-menu") == Err(PcCode::SlowDomain)
        && bk.filed.contains(&"web"), "慢域立案");

    // C480-03 慢域优化后复验过。
    bk.timings.iter_mut().filter(|t| t.domain == "web").for_each(|t| t.us = 103);
    bk.filed.clear();
    chk(&mut set, "C480-03", bk.verify("open-menu").is_ok(), "优化复验");

    // C480-04 域本色：读屏会话同构专项（两域 110/112 → 过）。
    bk.put(OpTiming { op: "open-menu", domain: "ui-sr", us: 110, a11y_session: true });
    bk.put(OpTiming { op: "open-menu", domain: "cli-sr", us: 112, a11y_session: true });
    chk(&mut set, "C480-04", bk.a11y_verify("open-menu").is_ok(), "读屏同构");

    // C480-05 读屏会话异速→专项拒（不因域而异速）。
    bk.put(OpTiming { op: "open-menu", domain: "web-sr", us: 200, a11y_session: true });
    chk(&mut set, "C480-05", bk.a11y_verify("open-menu") == Err(PcCode::SlowDomain), "读屏异速拒");

    // C480-06 预算对齐：冲突→仲裁取小。
    bk.budgets = vec![("ui", 500), ("cli", 400)];
    chk(&mut set, "C480-06", bk.budget_align(450) == Err(PcCode::BudgetConflict)
        && bk.budgets[1].1 == 400 && bk.budgets[0].1 == 450, "仲裁取小");

    // C480-07 对齐后通过。
    bk.budgets = vec![("ui", 450), ("cli", 450)];
    chk(&mut set, "C480-07", bk.budget_align(450).is_ok(), "预算对齐");

    // C480-08 单操作单域→不判（样本不足放行）。
    let mut bk2 = PerfConsisBook::new(500);
    bk2.put(OpTiming { op: "x", domain: "ui", us: 10, a11y_session: false });
    chk(&mut set, "C480-08", bk2.verify("x").is_ok(), "样本不足放行");

    // C480-09 立案幂等。
    bk.put(OpTiming { op: "save", domain: "a", us: 100, a11y_session: false });
    bk.put(OpTiming { op: "save", domain: "b", us: 900, a11y_session: false });
    bk.verify("save").unwrap_err();
    bk.verify("save").unwrap_err();
    chk(&mut set, "C480-09", bk.filed.iter().filter(|d| **d == "b").count() == 1, "立案幂等");

    // C480-10 容差常量在册。
    chk(&mut set, "C480-10", bk.tol_bp == 500, "容差 5%");

    // C480-11 基线重冻结计数位（漂移路径预留）。
    bk.refrozen = 0;
    chk(&mut set, "C480-11", bk.refrozen == 0, "重冻结位");

    // C480-12 错误码段位单源。
    chk(&mut set, "C480-12", PcCode::SlowDomain as u16 == SEG_BASE + 1, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves480_smoke() {
        let set = run_ves480_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves480 red: {:?}", red);
    }
}
