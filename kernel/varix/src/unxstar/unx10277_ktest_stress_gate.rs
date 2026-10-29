//! UNX-F10277 压力面 ktest 集成（stress gate 入冒烟）（0x16DE0 段）
//!
//! 锚点：UNX-F10277-J1 集成达成（stress gate 以 ktest 套件入冒烟链：
//! 注册/枚举/冒烟运行三面 10/10 次）；J2 集成异常检出（未注册运行/重
//! 注册/冒烟越套件 10/10 次检出）；J3 套件指纹（同套件同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·stress gate）。
//!
//! 结构：stress 套件册 + 冒烟链（套件序）+ 集成核 + 注入核（幽灵套
//! 件/重注册/越链必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x16DE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x16DE0;
pub const SG_GHOST: u32 = SEG_BASE + 0x01; // 未注册运行
pub const SG_DUP_REG: u32 = SEG_BASE + 0x02; // 重注册
pub const SG_OUT_CHAIN: u32 = SEG_BASE + 0x03; // 冒烟越链
pub const SG_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const SG_EMPTY: u32 = SEG_BASE + 0x05; // 空套件

/// stress 套件。
#[derive(Clone, PartialEq)]
pub struct StressSuite {
    pub name: String,
    pub cases: Vec<u32>,
}

impl StressSuite {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && !self.cases.is_empty()
    }

    /// 套件指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in self.name.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        for c in &self.cases {
            for byte in c.to_le_bytes() {
                h ^= byte as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }
}

/// 集成册。
pub struct StressGate {
    suites: Vec<StressSuite>,
    chain: Vec<String>,      // 冒烟链（套件名序）
    runs: Vec<(String, Vec<(u32, bool)>)>,
    frozen: bool,
}

impl StressGate {
    pub fn new() -> StressGate {
        StressGate { suites: Vec::new(), chain: Vec::new(), runs: Vec::new(), frozen: false }
    }

    /// 注册套件：空套件/重名/冻结拒收。
    pub fn reg(&mut self, name: &str, cases: Vec<u32>) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if name.is_empty() {
            return Err("empty_name");
        }
        if cases.is_empty() {
            return Err("empty_suite");
        }
        if self.suites.iter().any(|s| s.name == name) {
            return Err("dup_reg");
        }
        self.suites.push(StressSuite {
            name: name.to_string(),
            cases,
        });
        Ok(())
    }

    /// 挂冒烟链：套件须已注册；链上重复挂拒收。
    pub fn chain_add(&mut self, name: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !self.suites.iter().any(|s| s.name == name) {
            return Err("not_reg");
        }
        if self.chain.iter().any(|n| n == name) {
            return Err("dup_chain");
        }
        self.chain.push(name.to_string());
        Ok(())
    }

    /// 冒烟运行：按链序执行，套件须在链上；结果集须匹配用例集。
    pub fn smoke(&mut self, name: &str, results: Vec<(u32, bool)>) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !self.chain.iter().any(|n| n == name) {
            return Err("out_chain");
        }
        let suite = match self.suites.iter().find(|s| s.name == name) {
            Some(s) => s.clone(),
            None => return Err("ghost"),
        };
        if results.is_empty() {
            return Err("empty_run");
        }
        for (c, _) in &results {
            if !suite.cases.contains(c) {
                return Err("wild_case");
            }
        }
        if results.len() != suite.cases.len() {
            return Err("case_mismatch");
        }
        self.runs.push((name.to_string(), results));
        Ok(())
    }

    /// 冒烟链全绿：链上每套件都有运行且全过。
    pub fn chain_green(&self) -> bool {
        self.chain.iter().all(|n| {
            match self.runs.iter().find(|(rn, _)| rn == n) {
                Some((_, rs)) => rs.iter().all(|(_, p)| *p),
                None => false,
            }
        })
    }

    pub fn suite_count(&self) -> usize {
        self.suites.len()
    }

    pub fn chain_len(&self) -> usize {
        self.chain.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(g: &mut StressGate, code: u32, name: &str) -> Result<(), &'static str> {
    match code {
        SG_GHOST => g.smoke(name, vec![(1, true)]),
        SG_DUP_REG => {
            let _ = g.reg(name, vec![1, 2]);
            g.reg(name, vec![3])
        }
        SG_OUT_CHAIN => {
            let _ = g.reg(name, vec![1]);
            // 未挂链直接冒烟
            g.smoke(name, vec![(1, true)])
        }
        SG_FROZEN => {
            g.freeze();
            g.reg(name, vec![1])
        }
        SG_EMPTY => g.reg(name, vec![]),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10277_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10277");

    // J1 三面集成：注册 + 挂链 + 冒烟运行
    let j1 = {
        let mut g = StressGate::new();
        let r1 = g.reg("stress-core", vec![1, 2, 3]);
        let r2 = g.chain_add("stress-core");
        let r3 = g.smoke("stress-core", vec![(1, true), (2, true), (3, false)]);
        r1 == Ok(()) && r2 == Ok(()) && r3 == Ok(()) && g.chain_len() == 1
    };
    s.add("J1-三面集成", j1, "注册/挂链/冒烟全通");

    // J2a 幽灵套件拒绝
    let j2a = inject_fault(&mut StressGate::new(), SG_GHOST, "ghost") == Err("out_chain");
    s.add("J2a-幽灵拒绝", j2a, "未注册未挂链即拒");

    // J2b 重注册拒绝
    let j2b = inject_fault(&mut StressGate::new(), SG_DUP_REG, "dup") == Err("dup_reg");
    s.add("J2b-重注册拒", j2b, "同名二次即拒");

    // J2c 冒烟越链拒绝（注册但未挂链）
    let j2c = inject_fault(&mut StressGate::new(), SG_OUT_CHAIN, "offline") == Err("out_chain");
    s.add("J2c-越链拒绝", j2c, "未挂链冒烟即拒");

    // J3 套件指纹
    let j3 = {
        let a = StressSuite { name: "s".to_string(), cases: vec![1, 2] };
        let b2 = StressSuite { name: "s".to_string(), cases: vec![1, 2] };
        let c = StressSuite { name: "s".to_string(), cases: vec![1, 3] };
        a.fp() == b2.fp() && a.fp() != c.fp() && a.fp() != 0
    };
    s.add("J3-套件指纹", j3, "同套件同指纹用例变必变");

    // J4 野用例拒绝
    let j4 = {
        let mut g = StressGate::new();
        let _ = g.reg("x", vec![1]);
        let _ = g.chain_add("x");
        g.smoke("x", vec![(99, true)]) == Err("wild_case")
    };
    s.add("J4-野用例拒", j4, "越用例集即拒");

    // J5 用例数不符拒绝
    let j5 = {
        let mut g = StressGate::new();
        let _ = g.reg("y", vec![1, 2]);
        let _ = g.chain_add("y");
        g.smoke("y", vec![(1, true)]) == Err("case_mismatch")
    };
    s.add("J5-数目不符拒", j5, "结果数≠用例数即拒");

    // J6 冒烟链全绿核：全过 true / 一挂 false / 缺运行 false
    let j6 = {
        let mut g = StressGate::new();
        let _ = g.reg("a", vec![1]);
        let _ = g.reg("bb", vec![2]);
        let _ = g.chain_add("a");
        let _ = g.chain_add("bb");
        let _ = g.smoke("a", vec![(1, true)]);
        let partial = g.chain_green();
        let _ = g.smoke("bb", vec![(2, true)]);
        let full = g.chain_green();
        !partial && full
    };
    s.add("J6-链全绿核", j6, "缺运行/一挂都不绿");

    // J7 空套件拒绝
    let j7 = inject_fault(&mut StressGate::new(), SG_EMPTY, "e") == Err("empty_suite");
    s.add("J7-空套件拒", j7, "无用例即拒");

    // J8 注入五路
    let codes = [SG_GHOST, SG_DUP_REG, SG_OUT_CHAIN, SG_FROZEN, SG_EMPTY];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut StressGate::new(), **c, "inj").is_err())
        .count();
    s.add("J8-注入五路", caught == 5, "五路注入全拒");

    // J9 冻结红线
    let j9 = {
        let mut g = StressGate::new();
        inject_fault(&mut g, SG_FROZEN, "f");
        g.is_frozen() && g.reg("g", vec![1]) == Err("frozen")
    };
    s.add("J9-冻结拒收", j9, "印后注册必抓");

    // J10 十轮冒烟回放：10 轮链绿状态一致（确定性）
    let j10 = {
        let mk = || {
            let mut g = StressGate::new();
            let _ = g.reg("loop", vec![7, 8]);
            let _ = g.chain_add("loop");
            let _ = g.smoke("loop", vec![(7, true), (8, true)]);
            (g.chain_green(), g.suite_count(), g.chain_len())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.0
    };
    s.add("J10-十轮冒烟", j10, "10/10 轮链绿一致");

    s
}
