//! UNX-F10257 认证面 ktest 集成（gate 接入 ktest）（0x14360 段）
//!
//! 锚点：UNX-F10257-J1 ktest 集成达成（认证 gate 以 ktest 套件形态接
//! 入：套件注册/用例枚举/运行账三面各 10/10 次）；J2 集成面检出（未
//! 注册套件运行/重复注册/运行账与套件不符 10/10 次检出）；J3 套件指
//! 纹（同套件同指纹，用例增删指纹必变）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证 gate）。
//!
//! 结构：套件册（名称 × 用例集）+ 运行账（套件 → 结果集）+ 集成核
//! （注册/枚举/运行三面）+ 注入核（未注册/重注册/账面不符必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x14360 独占 0x20）。
pub const SEG_BASE: u32 = 0x14360;
pub const KT_NOT_REG: u32 = SEG_BASE + 0x01; // 未注册运行
pub const KT_DUP_REG: u32 = SEG_BASE + 0x02; // 重复注册
pub const KT_LEDGER_MISS: u32 = SEG_BASE + 0x03; // 运行账缺用例
pub const KT_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const KT_EMPTY_SUITE: u32 = SEG_BASE + 0x05; // 空套件

/// ktest 套件。
#[derive(Clone, PartialEq)]
pub struct Suite {
    pub name: String,
    pub cases: Vec<u32>, // 用例号集
}

impl Suite {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && !self.cases.is_empty()
    }

    /// 套件指纹：名称 + 用例集折叠。
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

/// 运行账条目。
#[derive(Clone, PartialEq)]
pub struct RunEntry {
    pub case: u32,
    pub passed: bool,
}

/// 集成册。
pub struct GateBook {
    suites: Vec<Suite>,
    runs: Vec<(String, Vec<RunEntry>)>, // (套件名, 运行结果)
    frozen: bool,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook { suites: Vec::new(), runs: Vec::new(), frozen: false }
    }

    /// 注册套件：空套件/重名/冻结后写拒收。
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
        self.suites.push(Suite {
            name: name.to_string(),
            cases,
        });
        Ok(())
    }

    /// 未注册检测。
    pub fn registered(&self, name: &str) -> bool {
        self.suites.iter().any(|s| s.name == name)
    }

    /// 用例枚举：按注册序回读套件用例集。
    pub fn enumerate(&self, name: &str) -> Result<Vec<u32>, &'static str> {
        match self.suites.iter().find(|s| s.name == name) {
            Some(s) => Ok(s.cases.clone()),
            None => Err("not_reg"),
        }
    }

    /// 运行套件：全部用例记结果；未注册/冻结后运行拒收。
    pub fn run(
        &mut self,
        name: &str,
        results: Vec<RunEntry>,
    ) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let suite = match self.suites.iter().find(|s| s.name == name) {
            Some(s) => s.clone(),
            None => return Err("not_reg"),
        };
        if results.is_empty() {
            return Err("empty_run");
        }
        // 运行账与套件相符：结果用例集必须与注册用例集一致
        for r in &results {
            if !suite.cases.contains(&r.case) {
                return Err("ledger_miss");
            }
        }
        if results.len() != suite.cases.len() {
            return Err("ledger_miss");
        }
        self.runs.push((name.to_string(), results));
        Ok(())
    }

    /// 运行账查询。
    pub fn run_of(&self, name: &str) -> Option<&Vec<RunEntry>> {
        self.runs.iter().find(|(n, _)| n == name).map(|(_, r)| r)
    }

    /// 套件数与运行数。
    pub fn suite_count(&self) -> usize {
        self.suites.len()
    }

    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(b: &mut GateBook, code: u32, name: &str) -> Result<(), &'static str> {
    match code {
        KT_NOT_REG => b.run(name, vec![RunEntry { case: 1, passed: true }]),
        KT_DUP_REG => {
            let _ = b.reg(name, vec![1, 2]);
            b.reg(name, vec![3])
        }
        KT_LEDGER_MISS => {
            let _ = b.reg(name, vec![1, 2]);
            b.run(name, vec![RunEntry { case: 1, passed: true }]) // 少一条
        }
        KT_FROZEN => {
            b.freeze();
            b.reg(name, vec![1])
        }
        KT_EMPTY_SUITE => b.reg(name, vec![]),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10257_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10257");

    // J1 套件注册 + 用例枚举 + 运行账三面
    let j1 = {
        let mut b = GateBook::new();
        let _ = b.reg("gate-core", vec![1, 2, 3]);
        let enumed = b.enumerate("gate-core").unwrap();
        let run = b.run(
            "gate-core",
            vec![
                RunEntry { case: 1, passed: true },
                RunEntry { case: 2, passed: true },
                RunEntry { case: 3, passed: false },
            ],
        );
        enumed == vec![1, 2, 3]
            && run.is_ok()
            && b.run_count() == 1
            && b.run_of("gate-core").unwrap().len() == 3
    };
    s.add("J1-三面集成", j1, "注册/枚举/运行全通");

    // J2a 未注册运行拒绝
    let j2a = inject_fault(&mut GateBook::new(), KT_NOT_REG, "ghost") == Err("not_reg");
    s.add("J2a-未注册拒", j2a, "幽灵套件即拒");

    // J2b 重复注册拒绝
    let j2b = inject_fault(&mut GateBook::new(), KT_DUP_REG, "dup") == Err("dup_reg");
    s.add("J2b-重注册拒", j2b, "同名二次即拒");

    // J2c 运行账与套件不符检出（少一条）
    let j2c = inject_fault(&mut GateBook::new(), KT_LEDGER_MISS, "short") == Err("ledger_miss");
    s.add("J2c-账面不符", j2c, "结果数≠用例数即拒");

    // J3 套件指纹：同套件同指纹/用例增删指纹变
    let j3 = {
        let a = Suite { name: "s".to_string(), cases: vec![1, 2] };
        let b2 = Suite { name: "s".to_string(), cases: vec![1, 2] };
        let c = Suite { name: "s".to_string(), cases: vec![1, 2, 3] };
        a.fp() == b2.fp() && a.fp() != c.fp() && a.fp() != 0
    };
    s.add("J3-套件指纹", j3, "同集同指纹增删必变");

    // J4 账面不符另一路：结果含未注册用例
    let j4 = {
        let mut b = GateBook::new();
        let _ = b.reg("x", vec![1]);
        b.run("x", vec![RunEntry { case: 99, passed: true }]) == Err("ledger_miss")
    };
    s.add("J4-野用例拒", j4, "结果越用例集即拒");

    // J5 注入面五路
    let codes = [KT_NOT_REG, KT_DUP_REG, KT_LEDGER_MISS, KT_FROZEN, KT_EMPTY_SUITE];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut GateBook::new(), **c, "inj").is_err())
        .count();
    s.add("J5-注入五路", caught == 5, "五路注入全拒");

    // J6 空套件拒收
    let j6 = inject_fault(&mut GateBook::new(), KT_EMPTY_SUITE, "e") == Err("empty_suite");
    s.add("J6-空套件拒", j6, "无用例即拒");

    // J7 冻结红线
    let j7 = {
        let mut b = GateBook::new();
        inject_fault(&mut b, KT_FROZEN, "f");
        b.is_frozen() && b.reg("g", vec![1]) == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后注册必抓");

    // J8 多套件运行账互不混
    let j8 = {
        let mut b = GateBook::new();
        let _ = b.reg("a", vec![1, 2]);
        let _ = b.reg("bb", vec![3, 4]);
        let _ = b.run("a", vec![RunEntry { case: 1, passed: true }, RunEntry { case: 2, passed: true }]);
        let _ = b.run("bb", vec![RunEntry { case: 3, passed: false }, RunEntry { case: 4, passed: false }]);
        b.suite_count() == 2
            && b.run_count() == 2
            && b.run_of("a").unwrap().iter().all(|r| r.passed)
            && b.run_of("bb").unwrap().iter().all(|r| !r.passed)
    };
    s.add("J8-账互不混", j8, "两套件运行账隔离");

    // J9 枚举未注册拒绝
    let j9 = GateBook::new().enumerate("nope") == Err("not_reg");
    s.add("J9-枚举未注册拒", j9, "无套件枚举即拒");

    // J10 十轮集成回放：10 轮指纹与账面一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = GateBook::new();
            let _ = b.reg("loop", vec![7, 8, 9]);
            let _ = b.run(
                "loop",
                vec![
                    RunEntry { case: 7, passed: true },
                    RunEntry { case: 8, passed: false },
                    RunEntry { case: 9, passed: true },
                ],
            );
            let suite = b.suites[0].fp();
            (suite, b.run_count())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base.1 == 1
    };
    s.add("J10-十轮回放", j10, "10/10 轮指纹账面一致");

    s
}
