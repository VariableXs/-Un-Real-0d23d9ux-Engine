//! CGPU-F0813 · 认证基准（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0813，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证基准——认证操作（签发/校验/重认证/吊销）的
//!   吞吐基准：整数操作计数 + 刻度窗口 → 每千刻度吞吐，
//!   逐操作与预算比对，基准档位闭集裁定。
//!
//! 设计纪律：
//! - 基准操作闭集四类：IssueBench / VerifyBench / RenewBench /
///   RevokeBench；每类独立预算（ops/千刻度）。
//! - 吞吐口径：throughput = ops * 1000 / ticks（整数除法，
///   ticks 不得为 0）；吞吐 ≥ 预算 → 达标。
//! - 判定闭集三态：Hit（≥ 预算）/ Near（≥ 90%）/ Miss（其余）。
//! - 结果账本定容：BENCH_CAP=48 条，超容逐出最旧并计数，
///   守恒恒等式：total_runs == held + evicted。
//! - 总评：全部 Hit → Gold；允许 ≤1 个 Near → Silver；
///   其余 → Bronze（无 Fail 档——认证基准只保底不淘汰）。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
///   诊断段 0x7020 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x7020，批内 CGPU-F0813 独占）。
pub const SEG_BASE: u16 = 0x7020;

/// 操作类别重复登记。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 结果账本已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 口径非法（刻度为 0 / 预算为 0）。
pub const E_PARAM: u16 = SEG_BASE + 3;
/// 操作未全部登记即总评。
pub const E_PARTIAL: u16 = SEG_BASE + 4;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 5;

/// 结果账本容量。
pub const BENCH_CAP: usize = 48;
/// Near 阈值（千分比）。
pub const NEAR_PM: u64 = 900;

/// 基准操作类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchOp {
    IssueBench,
    VerifyBench,
    RenewBench,
    RevokeBench,
}

impl BenchOp {
    pub fn ord(self) -> u8 {
        match self {
            BenchOp::IssueBench => 0,
            BenchOp::VerifyBench => 1,
            BenchOp::RenewBench => 2,
            BenchOp::RevokeBench => 3,
        }
    }
}

/// 单项预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpBudget {
    pub op: BenchOp,
    /// 预算（ops/千刻度）。
    pub budget: u64,
}

impl OpBudget {
    pub fn new(op: BenchOp, budget: u64) -> Result<Self, u16> {
        if budget == 0 {
            return Err(E_PARAM);
        }
        Ok(OpBudget { op, budget })
    }
}

/// 判定（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchVerdict {
    Hit,
    Near,
    Miss,
}

impl BenchVerdict {
    pub fn ord(self) -> u8 {
        match self {
            BenchVerdict::Hit => 0,
            BenchVerdict::Near => 1,
            BenchVerdict::Miss => 2,
        }
    }
}

/// 吞吐与判定：throughput = ops*1000/ticks。
pub fn measure(budget: u64, ops: u64, ticks: u64) -> Result<(u64, BenchVerdict), u16> {
    if ticks == 0 {
        return Err(E_PARAM);
    }
    let tp = ops * 1000 / ticks;
    // 判定：tp ≥ budget → Hit；tp*1000 ≥ budget*NEAR_PM → Near；否则 Miss。
    let v = if tp >= budget {
        BenchVerdict::Hit
    } else if tp * 1000 >= budget * NEAR_PM {
        BenchVerdict::Near
    } else {
        BenchVerdict::Miss
    };
    Ok((tp, v))
}

/// 基准册。
#[derive(Clone, Debug, Default)]
pub struct BenchBook {
    budgets: Vec<OpBudget>,
}

impl BenchBook {
    pub fn new() -> Self {
        BenchBook {
            budgets: Vec::new(),
        }
    }
    pub fn add(&mut self, b: OpBudget) -> Result<(), u16> {
        if self.budgets.iter().any(|o| o.op == b.op) {
            return Err(E_DUP);
        }
        self.budgets.push(b);
        Ok(())
    }
    pub fn budget_of(&self, op: BenchOp) -> Result<u64, u16> {
        self.budgets
            .iter()
            .find(|o| o.op == op)
            .map(|o| o.budget)
            .ok_or(E_PARTIAL)
    }
    pub fn len(&self) -> usize {
        self.budgets.len()
    }
}

/// 结果记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchRec {
    pub op: BenchOp,
    pub throughput: u64,
    pub verdict: BenchVerdict,
}

/// 结果账本（环形定容）。
#[derive(Clone, Debug)]
pub struct BenchLedger {
    ring: Vec<BenchRec>,
    evicted: u64,
    total: u64,
}

impl BenchLedger {
    pub fn new() -> Self {
        BenchLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, r: BenchRec) {
        if self.ring.len() >= BENCH_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(r);
        self.total += 1;
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
}

/// 总评档位（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchTier {
    Gold,
    Silver,
    Bronze,
}

/// 总评：四类操作各取最新一条记录，必须全部在册；
/// 全 Hit → Gold；Miss==0 且 Near ≤ 1 → Silver；其余 Bronze。
pub fn overall(book: &BenchBook, led: &BenchLedger) -> Result<BenchTier, u16> {
    if book.len() != 4 {
        return Err(E_PARTIAL);
    }
    let mut nears = 0u32;
    let mut misses = 0u32;
    let mut found = 0u32;
    for b in &book.budgets {
        let rec = led
            .ring
            .iter()
            .rev()
            .find(|r| r.op == b.op)
            .ok_or(E_PARTIAL)?;
        found += 1;
        match rec.verdict {
            BenchVerdict::Hit => {}
            BenchVerdict::Near => nears += 1,
            BenchVerdict::Miss => misses += 1,
        }
    }
    if found != 4 {
        return Err(E_PARTIAL);
    }
    Ok(if misses == 0 && nears == 0 {
        BenchTier::Gold
    } else if misses == 0 && nears <= 1 {
        BenchTier::Silver
    } else {
        BenchTier::Bronze
    })
}

/// 自例程：主路径（Gold）+ Silver/Bronze 路径 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut book = BenchBook::new();
    book.add(OpBudget::new(BenchOp::IssueBench, 1000)?)?;
    book.add(OpBudget::new(BenchOp::VerifyBench, 5000)?)?;
    book.add(OpBudget::new(BenchOp::RenewBench, 800)?)?;
    book.add(OpBudget::new(BenchOp::RevokeBench, 200)?)?;
    if book.add(OpBudget::new(BenchOp::IssueBench, 1)?) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if OpBudget::new(BenchOp::IssueBench, 0) != Err(E_PARAM) {
        return Err(E_PARAM);
    }
    // 主路径 1：全达标 → Gold。
    // throughput：Issue 1000 ops/100 ticks = 10000 ≥ 1000 → Hit；
    // Verify 5000/100 = 50000 ≥ 5000 → Hit；
    // Renew 800/100 = 8000 ≥ 800 → Hit；Revoke 200/100=2000 ≥ 200 → Hit。
    let mut led = BenchLedger::new();
    for (op, ops) in [
        (BenchOp::IssueBench, 1000u64),
        (BenchOp::VerifyBench, 5000),
        (BenchOp::RenewBench, 800),
        (BenchOp::RevokeBench, 200),
    ]
    .iter()
    {
        let budget = book.budget_of(*op)?;
        let (tp, v) = measure(budget, *ops, 100)?;
        if v != BenchVerdict::Hit || tp != ops * 10 {
            return Err(E_PARAM);
        }
        led.push(BenchRec {
            op: *op,
            throughput: tp,
            verdict: v,
        });
    }
    led.audit()?;
    if overall(&book, &led)? != BenchTier::Gold {
        return Err(E_PARTIAL);
    }
    // 主路径 2：一个 Near（≥90%）→ Silver。
    // Renew：ops=760/100 → 7600；7600*1000=7_600_000 ≥ 800*900=720_000 → Near。
    let mut led2 = BenchLedger::new();
    for (op, ops) in [
        (BenchOp::IssueBench, 1000u64),
        (BenchOp::VerifyBench, 5000),
        (BenchOp::RenewBench, 760),
        (BenchOp::RevokeBench, 200),
    ]
    .iter()
    {
        let budget = book.budget_of(*op)?;
        let (tp, v) = measure(budget, *ops, 100)?;
        led2.push(BenchRec {
            op: *op,
            throughput: tp,
            verdict: v,
        });
    }
    if overall(&book, &led2)? != BenchTier::Silver {
        return Err(E_PARTIAL);
    }
    // 边界：恰好 90% → Near（720 ops → tp 7200；7200*1000=7_200_000 == 720_000*10? 
    // budget*NEAR_PM = 800*900 = 720_000；tp*1000 = 7_200_000 ≥ 720_000 → Near。
    let (_, v90) = measure(800, 720, 100)?;
    if v90 != BenchVerdict::Near {
        return Err(E_PARAM);
    }
    // 89% 以下 → Miss（719 ops → 7190）。
    let (_, v89) = measure(800, 719, 100)?;
    if v89 != BenchVerdict::Miss {
        return Err(E_PARAM);
    }
    // 主路径 3：一个 Miss → Bronze；两个 Near → Bronze。
    let mut led3 = BenchLedger::new();
    for (op, ops) in [
        (BenchOp::IssueBench, 1000u64),
        (BenchOp::VerifyBench, 5000),
        (BenchOp::RenewBench, 700),
        (BenchOp::RevokeBench, 100),
    ]
    .iter()
    {
        let budget = book.budget_of(*op)?;
        let (tp, v) = measure(budget, *ops, 100)?;
        led3.push(BenchRec {
            op: *op,
            throughput: tp,
            verdict: v,
        });
    }
    if overall(&book, &led3)? != BenchTier::Bronze {
        return Err(E_PARTIAL);
    }
    // 错误路径 1：刻度为 0。
    if measure(100, 10, 0) != Err(E_PARAM) {
        return Err(E_PARAM);
    }
    // 错误路径 2：册不满 4 类即总评。
    let mut book2 = BenchBook::new();
    book2.add(OpBudget::new(BenchOp::IssueBench, 1)?)?;
    if overall(&book2, &led) != Err(E_PARTIAL) {
        return Err(E_PARTIAL);
    }
    // 错误路径 3：缺一类操作的结果记录。
    let mut led4 = BenchLedger::new();
    for (op, ops) in [
        (BenchOp::IssueBench, 1000u64),
        (BenchOp::VerifyBench, 5000),
        (BenchOp::RenewBench, 800),
    ]
    .iter()
    {
        let budget = book.budget_of(*op)?;
        let (tp, v) = measure(budget, *ops, 100)?;
        led4.push(BenchRec {
            op: *op,
            throughput: tp,
            verdict: v,
        });
    }
    if overall(&book, &led4) != Err(E_PARTIAL) {
        return Err(E_PARTIAL);
    }
    // 定容守恒：52 条记录进 48 容账本。
    let mut led5 = BenchLedger::new();
    for i in 0..52u32 {
        led5.push(BenchRec {
            op: BenchOp::VerifyBench,
            throughput: i as u64,
            verdict: BenchVerdict::Hit,
        });
    }
    led5.audit()?;
    if led5.held() != BENCH_CAP || led5.evicted != 4 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
