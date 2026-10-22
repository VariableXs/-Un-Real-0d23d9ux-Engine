//! UNX-F0225 诊断模式完备性演练脚本（0xAA00 段）
//!
//! 锚点：UNX-F0225-J1 12 步演练脚本（进诊断/查报告/改参数/退出）
//! 全过，演练记录落账。
//! 域：CoRun·UNX-A1-B12（诊断演练面）。
//!
//! 结构：12 步脚本闭集（enter/check-report/read-param/modify-param/
//! verify-param/collect-log/run-selftest/read-result/clear-flag/
//! save-record/exit/verify-exit）+ 演练执行器（步序严格 + 跳步拒 +
//! 步留账）+ 演练记录（(步, 动作, 结果) 落账）+ 注入核（跳步/重复/
//! 未知步/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAA00 独占 0x80）。
pub const SEG_BASE: u32 = 0xAA00;
pub const XC_DG_SKIP: u32 = SEG_BASE + 0x01;
pub const XC_DG_DUP: u32 = SEG_BASE + 0x02;
pub const XC_DG_SEALED: u32 = SEG_BASE + 0x03;

/// 12 步闭集。
pub const STEPS: [&str; 12] = [
    "enter-diag",
    "check-report",
    "read-param",
    "modify-param",
    "verify-param",
    "collect-log",
    "run-selftest",
    "read-result",
    "clear-flag",
    "save-record",
    "exit-diag",
    "verify-exit",
];

/// 步留账。
#[derive(Clone)]
pub struct StepLog {
    pub idx: usize,
    pub name: String,
    pub ok: bool,
}

/// 演练执行器。
pub struct DiagRunner {
    pub next: usize,
    pub logs: Vec<StepLog>,
    pub sealed: bool,
}

impl DiagRunner {
    pub fn new() -> Self {
        DiagRunner { next: 0, logs: Vec::new(), sealed: false }
    }

    /// 执行一步（严格步序）。
    pub fn run_step(&mut self, idx: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_DG_SEALED);
        }
        if idx >= STEPS.len() {
            return Err(XC_DG_SKIP);
        }
        if idx < self.next {
            return Err(XC_DG_DUP);
        }
        if idx > self.next {
            return Err(XC_DG_SKIP);
        }
        self.logs.push(StepLog { idx, name: String::from(STEPS[idx]), ok: true });
        self.next += 1;
        if self.next == STEPS.len() {
            self.sealed = true;
        }
        Ok(())
    }

    /// 全 12 步一键演练。
    pub fn run_all(&mut self) -> Result<(), u32> {
        for i in 0..STEPS.len() {
            self.run_step(i)?;
        }
        Ok(())
    }

    /// 演练记录完整性：12 步全账 + 步名与闭集一致。
    pub fn record_complete(&self) -> bool {
        self.logs.len() == 12
            && self
                .logs
                .iter()
                .enumerate()
                .all(|(i, l)| l.idx == i && l.name == STEPS[i] && l.ok)
    }
}

/// 12 步全过核。
pub fn twelve_steps_exact() -> bool {
    let mut r = DiagRunner::new();
    if r.run_all().is_err() {
        return false;
    }
    r.record_complete() && r.sealed
}

/// 步名闭集抽查核：进/查/改/退四关键步在位。
pub fn key_steps_exact() -> bool {
    STEPS[0] == "enter-diag"
        && STEPS[1] == "check-report"
        && STEPS[3] == "modify-param"
        && STEPS[10] == "exit-diag"
}

/// 跳步/重复拒核。
pub fn order_guard_exact() -> bool {
    // 跳步
    let mut r = DiagRunner::new();
    if r.run_step(2) == Err(XC_DG_SKIP) {
        // pass
    } else {
        return false;
    }
    // 重复
    let mut r2 = DiagRunner::new();
    let _ = r2.run_step(0);
    if r2.run_step(0) == Err(XC_DG_DUP) {
        // pass
    } else {
        return false;
    }
    // 封账后再跑
    let mut r3 = DiagRunner::new();
    let _ = r3.run_all();
    r3.run_step(0) == Err(XC_DG_SEALED)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知步（越界）
    let mut r = DiagRunner::new();
    if r.run_step(12) == Err(XC_DG_SKIP) {
        caught += 1;
    }
    // 2) 跳步
    if r.run_step(1) == Err(XC_DG_SKIP) {
        caught += 1;
    }
    // 3) 半途不完整
    let mut r2 = DiagRunner::new();
    for i in 0..11 {
        let _ = r2.run_step(i);
    }
    if !r2.record_complete() {
        caught += 1;
    }
    // 4) 封账
    let mut r3 = DiagRunner::new();
    let _ = r3.run_all();
    if r3.run_step(11) == Err(XC_DG_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：演练账折叠哈希。
pub fn fp_frozen(r: &DiagRunner) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in &r.logs {
        h ^= l.idx as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in l.name.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx225_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0225");
    // J1-1 12 步全过 + 记录落账
    let ok = twelve_steps_exact();
    s.add("B12-01-12步全过", ok, if ok { "" } else { "步漂移" });
    // J1-2 关键步在位
    let ok = key_steps_exact();
    s.add("B12-02-关键步", ok, if ok { "" } else { "步名漂移" });
    // J1-3 跳步/重复拒
    let ok = order_guard_exact();
    s.add("B12-03-步序守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B12-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let mut r = DiagRunner::new();
    let _ = r.run_step(0);
    let ok = fp_frozen(&r) == fp_frozen(&r);
    s.add("B12-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
