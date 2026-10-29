//! unx0952_execenv — UNX-F0952 bottom half 执行环境（栈/上下文/调度约束三面）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0952 bottom half 执行环境（栈/上下文/调度约束三面）
//! 判据原文摘录（卡面验收行）：执行环境三面（栈余量/上下文能力位/
//! 调度约束）逐面记账可核，越界样本 10/10 次必拦（栈爆/越权睡眠/
//! 违约抢占），三面口径互不自洽必点名。
//! 诊断段 0x7D00（本单独占，XcCode 闭集见下）。
//! 设计纪律：环境三面强耦合——栈面定深、上下文面定权、调度面定时；
//! 任一面违例整体拒绝；三面口径必须互洽（可睡 ⇔ 进程栈 ⇔ 可被抢占）；
//! 全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D00;

// ── 诊断段独占 · 执行环境错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_STACK: u16 = SEG_ERR + 1;  // 栈余量不足或栈爆
const E_CTX: u16 = SEG_ERR + 2;    // 上下文越权（不可睡却睡等）
const E_SCHED: u16 = SEG_ERR + 3;  // 调度约束破约（违约抢占）
const E_MISMATCH: u16 = SEG_ERR + 4; // 三面口径不自洽
const E_STATE: u16 = SEG_ERR + 5;  // 环境状态非法

/// 上下文能力位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtxCaps {
    pub may_sleep: bool,     // 可睡
    pub preemptible: bool,   // 可被抢占
    pub big_stack: bool,     // 大栈（进程栈 4KB 级 vs 中断栈 512B 级）
}

impl CtxCaps {
    /// 口径自洽：可睡 ⇔ 可抢占 ⇔ 大栈（三面一体）。
    pub fn coherent(&self) -> bool {
        self.may_sleep == self.preemptible && self.preemptible == self.big_stack
    }
}

/// 两类执行环境模板。
pub const ENV_INTR: CtxCaps = CtxCaps { may_sleep: false, preemptible: false, big_stack: false };
pub const ENV_PROC: CtxCaps = CtxCaps { may_sleep: true, preemptible: true, big_stack: true };

/// 栈面账：深度预算与哨兵。
#[derive(Clone, Copy, Debug)]
pub struct StackFace {
    pub budget_b: u32, // 预算字节
    pub used_b: u32,   // 已用字节
}

impl StackFace {
    /// 栈面核验：已用不得超预算，且必须留 64B 安全垫。
    pub fn audit(&self) -> Result<(), u16> {
        if self.used_b > self.budget_b { return Err(E_STACK); }
        if self.budget_b.saturating_sub(self.used_b) < 64 { return Err(E_STACK); }
        Ok(())
    }
    pub fn depth_ratio(&self) -> u32 {
        if self.budget_b == 0 { return 100; }
        self.used_b * 100 / self.budget_b
    }
}

/// 调度面账：时长约束与抢占意愿。
#[derive(Clone, Copy, Debug)]
pub struct SchedFace {
    pub run_us: u64,       // 已跑时长
    pub slot_us: u64,      // 时间片
    pub want_preempt: bool,// 希望被抢占
}

impl SchedFace {
    /// 调度面核验：超片必须可被抢占，否则破约。
    pub fn audit(&self, caps: &CtxCaps) -> Result<(), u16> {
        if self.run_us <= self.slot_us { return Ok(()); }
        if self.want_preempt && !caps.preemptible { return Err(E_SCHED); }
        Ok(())
    }
}

/// 执行环境账本：一条记录 = 一次 bottom half 执行的环境三面。
#[derive(Clone, Copy, Debug)]
pub struct EnvRec {
    pub caps: CtxCaps,
    pub stack: StackFace,
    pub sched: SchedFace,
}

/// 环境审计器。
pub struct EnvAuditor {
    pub recs: Vec<EnvRec>,
    pub rejects: Vec<(String, u16)>, // 拦截账（环境名 + 错误码）
}

impl EnvAuditor {
    pub fn new() -> Self { EnvAuditor { recs: Vec::new(), rejects: Vec::new() } }

    /// 三面联审：任一面违例整体拒绝。
    pub fn admit(&mut self, name: &str, rec: EnvRec) -> Result<(), u16> {
        // 第零面：口径自洽。
        if !rec.caps.coherent() {
            self.rejects.push((name.into(), E_MISMATCH));
            return Err(E_MISMATCH);
        }
        // 第一面：栈。
        if let Err(e) = rec.stack.audit() {
            self.rejects.push((name.into(), e));
            return Err(e);
        }
        // 第二面：上下文能力（可睡者用大栈预算已由 caps 声明）。
        if rec.caps.may_sleep && rec.stack.budget_b < 4096 {
            self.rejects.push((name.into(), E_CTX));
            return Err(E_CTX);
        }
        // 第三面：调度。
        if let Err(e) = rec.sched.audit(&rec.caps) {
            self.rejects.push((name.into(), e));
            return Err(e);
        }
        self.recs.push(rec);
        Ok(())
    }

    /// 拦截覆盖核验：给定错误码类至少被拦过 n 次。
    pub fn reject_coverage(&self, code: u16, n: u32) -> Result<(), u16> {
        let c = self.rejects.iter().filter(|(_, e)| *e == code).count() as u32;
        if c < n { return Err(E_STATE); }
        Ok(())
    }

    /// 栈深分布摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("env recs={} rejects={}", self.recs.len(), self.rejects.len());
        let mut total: u32 = 0;
        for r in &self.recs { total += r.stack.depth_ratio(); }
        if !self.recs.is_empty() {
            s = alloc::format!("{} avg-depth={}%", s, total / self.recs.len() as u32);
        }
        s
    }
}

/// 自例程：合法环境全收、越界样本 10/10 全拦返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut aud = EnvAuditor::new();
    // 场景一：中断上下文环境（小栈、不可睡、不抢）。
    let intr = EnvRec {
        caps: ENV_INTR,
        stack: StackFace { budget_b: 512, used_b: 200 },
        sched: SchedFace { run_us: 5, slot_us: 5, want_preempt: false },
    };
    aud.admit("bh-intr", intr)?;
    // 场景二：进程上下文环境（大栈、可睡、可抢）。
    let proc = EnvRec {
        caps: ENV_PROC,
        stack: StackFace { budget_b: 4096, used_b: 1200 },
        sched: SchedFace { run_us: 900, slot_us: 1000, want_preempt: true },
    };
    aud.admit("bh-proc", proc)?;
    if aud.recs.len() != 2 { return Err(E_STATE); }
    // 场景三：栈爆必拦（含安全垫不足）10 样本。
    for i in 0..10u32 {
        let bad = EnvRec {
            caps: ENV_INTR,
            stack: StackFace { budget_b: 512, used_b: 460 + i },
            sched: SchedFace { run_us: 5, slot_us: 5, want_preempt: false },
        };
        if aud.admit("stack-ovf", bad) != Err(E_STACK) { return Err(E_STACK); }
    }
    aud.reject_coverage(E_STACK, 10)?;
    // 场景四：越权睡眠（不可睡环境声明 may_sleep）→ 口径不自洽。
    let sleep_bad = EnvRec {
        caps: CtxCaps { may_sleep: true, preemptible: false, big_stack: true },
        stack: StackFace { budget_b: 4096, used_b: 100 },
        sched: SchedFace { run_us: 1, slot_us: 100, want_preempt: true },
    };
    if aud.admit("ctx-bad", sleep_bad) != Err(E_MISMATCH) { return Err(E_MISMATCH); }
    // 场景五：违约抢占（不可抢占环境却声明 want_preempt）。
    let preempt_bad = EnvRec {
        caps: ENV_INTR,
        stack: StackFace { budget_b: 512, used_b: 100 },
        sched: SchedFace { run_us: 50, slot_us: 5, want_preempt: true },
    };
    if aud.admit("sched-bad", preempt_bad) != Err(E_SCHED) { return Err(E_SCHED); }
    // 场景六：可睡环境配小栈必拦。
    let small_bad = EnvRec {
        caps: ENV_PROC,
        stack: StackFace { budget_b: 512, used_b: 100 },
        sched: SchedFace { run_us: 1, slot_us: 100, want_preempt: true },
    };
    if aud.admit("small-bad", small_bad) != Err(E_CTX) { return Err(E_CTX); }
    if aud.rejects.len() < 13 { return Err(E_STATE); }
    if !aud.summary().is_empty() { /* 摘要可生成 */ }
    // 场景七：栈面比率口径。
    let r = StackFace { budget_b: 512, used_b: 256 }.depth_ratio();
    if r != 50 { return Err(E_STACK); }
    Ok(0)
}
