//! ves277_s12nlayout — VE-F4037 · RTL 与 N 域布局
//! N03 布局引擎 RTL 镜像执行协同终审：镜像执行归属（N 执行/T 规则——分工复述单源）+ RTL 布局断言双方对拍 + T 规则×N 执行×真机三方对拍。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7140;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum NLayoutCode {
    Ok = 0,
    Diverged = SEG_BASE + 0x01,
    Disagree = SEG_BASE + 0x02,
    ExecMissing = SEG_BASE + 0x03,
    RuleMissing = SEG_BASE + 0x04,
    Unowned = SEG_BASE + 0x05,
}

/// 分工归属：镜像执行 = N 侧执行，镜像规则 = T 侧规则。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner { NExec = 0, TRule = 1 }

/// 分工单源表：职责 → 归属。唯一真源，双方各自复述比对。
pub struct DivisionBook {
    pub entries: Vec<(&'static str, Owner)>,
}

impl DivisionBook {
    pub fn canonical() -> Self {
        DivisionBook {
            entries: vec![
                ("布局镜像执行", Owner::NExec),
                ("镜像规则表", Owner::TRule),
                ("方向切换执行", Owner::NExec),
                ("方向语义判定", Owner::TRule),
                ("焦点序镜像执行", Owner::NExec),
                ("焦点语义判定", Owner::TRule),
            ],
        }
    }
    pub fn owner_of(&self, duty: &str) -> Option<Owner> {
        for e in self.entries.iter() {
            if e.0 == duty { return Some(e.1); }
        }
        None
    }
    /// 分工分叉归一：N 侧与 T 侧复述必须一致。
    pub fn retell_matches(&self, n_retell: &[(&str, Owner)], t_retell: &[(&str, Owner)]) -> bool {
        if n_retell.len() != self.entries.len() || t_retell.len() != self.entries.len() { return false; }
        for e in self.entries.iter() {
            let mut n_ok = false;
            let mut t_ok = false;
            for r in n_retell.iter() {
                if r.0 == e.0 { n_ok = r.1 == e.1; }
            }
            for r in t_retell.iter() {
                if r.0 == e.0 { t_ok = r.1 == e.1; }
            }
            if !n_ok || !t_ok { return false; }
        }
        true
    }
}

/// 布局断言对拍：T 侧规则期望 与 N 侧执行实测 对拍。
#[derive(Clone)]
pub struct LayoutClaim {
    pub widget: u32,
    /// T 侧期望镜像（规则推得）。
    pub t_expect: bool,
    /// N 侧实测镜像。
    pub n_actual: bool,
}

pub struct PairChecker {
    pub claims: Vec<LayoutClaim>,
}

impl PairChecker {
    pub fn new() -> Self { PairChecker { claims: Vec::new() } }
    pub fn add(&mut self, c: LayoutClaim) { self.claims.push(c); }
    /// 对拍：全部一致 → 绿；分歧集合返回。
    pub fn judge(&self) -> (bool, Vec<u32>) {
        let mut bad = Vec::new();
        for c in self.claims.iter() {
            if c.t_expect != c.n_actual { bad.push(c.widget); }
        }
        (bad.is_empty(), bad)
    }
}

/// 三方对拍：T 规则 × N 执行 × 真机。
#[derive(Clone)]
pub struct TripleClaim {
    pub widget: u32,
    pub t_rule: bool,
    pub n_exec: bool,
    pub device: bool,
}

impl TripleClaim {
    pub fn agree(&self) -> bool {
        self.t_rule == self.n_exec && self.n_exec == self.device
    }
}

pub struct TripleChecker {
    pub claims: Vec<TripleClaim>,
}

impl TripleChecker {
    pub fn new() -> Self { TripleChecker { claims: Vec::new() } }
    pub fn add(&mut self, c: TripleClaim) { self.claims.push(c); }
    pub fn judge(&self) -> (bool, Vec<u32>) {
        let mut bad = Vec::new();
        for c in self.claims.iter() {
            if !c.agree() { bad.push(c.widget); }
        }
        (bad.is_empty(), bad)
    }
    /// 归属判责：分歧者按分工单源判责侧。
    pub fn blame(&self, div: &DivisionBook, widget: u32) -> Option<Owner> {
        for c in self.claims.iter() {
            if c.widget == widget {
                if c.n_exec == c.device && c.t_rule != c.n_exec { return Some(Owner::TRule); }
                if c.t_rule == c.device && c.n_exec != c.t_rule { return Some(Owner::NExec); }
                return None;
            }
        }
        None
    }
}

/// N 侧执行登记：每个镜像执行必须有 N 侧执行记录（执行漏 → N 侧立案）。
pub struct ExecLedger {
    pub done: Vec<u32>,
}

impl ExecLedger {
    pub fn new() -> Self { ExecLedger { done: Vec::new() } }
    pub fn mark(&mut self, w: u32) { self.done.push(w); }
    pub fn executed(&self, w: u32) -> bool {
        self.done.iter().any(|x| *x == w)
    }
}

pub fn run_ves277_checks() -> CheckSet {
    let mut set = CheckSet::new("ves277_s12nlayout");
    let div = DivisionBook::canonical();

    // 1) 分工单源：六职责归属明确。
    set.add("C4037-01", div.entries.len() == 6
        && div.owner_of("布局镜像执行") == Some(Owner::NExec)
        && div.owner_of("镜像规则表") == Some(Owner::TRule), "分工单源");

    // 2) 双侧复述一致。
    let retell: Vec<(&str, Owner)> = div.entries.clone();
    set.add("C4037-02", div.retell_matches(&retell, &retell), "双侧复述一致");

    // 3) 复述分叉检出。
    let mut bad_retell = div.entries.clone();
    bad_retell[0].1 = Owner::TRule;
    set.add("C4037-03", !div.retell_matches(&bad_retell, &retell), "复述分叉检出");

    // 4) 复述缺项检出。
    let short: Vec<(&str, Owner)> = retell[1..].to_vec();
    set.add("C4037-04", !div.retell_matches(&short, &retell), "复述缺项检出");

    // 5) 双方对拍全绿。
    let mut pc = PairChecker::new();
    pc.add(LayoutClaim { widget: 1, t_expect: true, n_actual: true });
    pc.add(LayoutClaim { widget: 2, t_expect: false, n_actual: false });
    set.add("C4037-05", pc.judge() == (true, Vec::new()), "双方对拍绿");

    // 6) 对拍分歧检出。
    pc.add(LayoutClaim { widget: 3, t_expect: true, n_actual: false });
    let (ok, bad) = pc.judge();
    set.add("C4037-06", !ok && bad == [3], "对拍分歧检出");

    // 7) 三方对拍全绿。
    let mut tc = TripleChecker::new();
    tc.add(TripleClaim { widget: 1, t_rule: true, n_exec: true, device: true });
    set.add("C4037-07", tc.judge() == (true, Vec::new()), "三方对拍绿");

    // 8) 三方分歧检出。
    tc.add(TripleClaim { widget: 4, t_rule: true, n_exec: true, device: false });
    let (ok2, bad2) = tc.judge();
    set.add("C4037-08", !ok2 && bad2 == [4], "三方分歧检出");

    // 9) 判责：规则侧错。
    set.add("C4037-09", tc.blame(&div, 4) == Some(Owner::TRule), "判责规则侧");

    // 10) 判责：执行侧错。
    tc.add(TripleClaim { widget: 5, t_rule: true, n_exec: false, device: true });
    set.add("C4037-10", tc.blame(&div, 5) == Some(Owner::NExec), "判责执行侧");

    // 11) 三方全分歧 → 无法单侧判责。
    tc.add(TripleClaim { widget: 6, t_rule: false, n_exec: true, device: false });
    set.add("C4037-11", tc.blame(&div, 6) == None, "三方分歧无单责");

    // 12) 执行漏立案：N 侧未登记。
    let mut el = ExecLedger::new();
    el.mark(1);
    set.add("C4037-12", el.executed(1) && !el.executed(2), "执行漏立案点");

    // 13) 职责缺 → 单源查无。
    set.add("C4037-13", div.owner_of("不存在职责") == None, "职责缺返回 None");

    // 14) 段位自查。
    set.add("C4037-14", SEG_BASE == 0x7140, "段位 0x7140");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_agree() {
        let c = TripleClaim { widget: 9, t_rule: true, n_exec: true, device: true };
        assert!(c.agree());
        let d = TripleClaim { widget: 10, t_rule: true, n_exec: false, device: true };
        assert!(!d.agree());
    }
}
