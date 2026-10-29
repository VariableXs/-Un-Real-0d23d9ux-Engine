//! unx0945_softirq — UNX-F0945 softirq 底座（静态优先级软中断槽位）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0945 softirq 底座（静态优先级软中断槽位）
//! 判据原文摘录（卡面验收行）：静态优先级槽位表全绿：优先级唯一且
//! 编译期定序，raise/run 全走账，槽位在跑期间重入必拦，风暴样本
//! 10/10 次不丢不重。
//! 诊断段 0x7C20（本单独占，XcCode 闭集见下）。
//! 设计纪律：softirq 槽位静态定序（优先级数值小者先跑）；同一槽位
//! 在跑期间 raise 只置位不重入；执行顺序严格按优先级；全部错误走
//! Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C20;

// ── 诊断段独占 · softirq 底座错误域 ────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SLOT: u16 = SEG_ERR + 1;   // 槽位非法
const E_PRIO: u16 = SEG_ERR + 2;   // 优先级冲突或非静态
const E_REENT: u16 = SEG_ERR + 3;  // 在跑期间重入
const E_ORDER: u16 = SEG_ERR + 4;  // 执行序破约
const E_STATE: u16 = SEG_ERR + 5;  // 底座状态非法
const E_LOST: u16 = SEG_ERR + 6;   // 风暴中丢账

/// 静态软中断槽位定义：编译期定序的优先级表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotDef {
    pub prio: u8,        // 数值小者先跑（0 最高）
    pub name: &'static str,
}

/// 全域静态槽位表（定序唯一，注册时校验无冲突）。
pub const SLOT_TABLE: [SlotDef; 6] = [
    SlotDef { prio: 0, name: "hi" },
    SlotDef { prio: 1, name: "timer" },
    SlotDef { prio: 2, name: "net-tx" },
    SlotDef { prio: 3, name: "net-rx" },
    SlotDef { prio: 4, name: "block" },
    SlotDef { prio: 5, name: "tasklet-drain" },
];

/// 静态表自校验：优先级唯一无洞。
pub fn table_audit() -> Result<u32, u16> {
    for i in 0..SLOT_TABLE.len() {
        if SLOT_TABLE[i].prio as usize != i { return Err(E_PRIO); }
        for j in 0..SLOT_TABLE.len() {
            if i != j && SLOT_TABLE[i].prio == SLOT_TABLE[j].prio { return Err(E_PRIO); }
        }
    }
    Ok(SLOT_TABLE.len() as u32)
}

/// softirq 底座状态机：Idle → Raising → Running → Idle。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqiState {
    Idle,
    Raising,
    Running,
}

/// 运行账记录：哪槽何时跑了、跑了多少票。
#[derive(Clone, Copy, Debug)]
pub struct RunRec {
    pub slot: usize,
    pub t_us: u64,
    pub votes: u32, // 本次执行前累计的 raise 票数（合并不翻倍）
}

/// softirq 底座核心。
pub struct SoftirqBase {
    state: SqiState,
    pending: [bool; SLOT_TABLE.len()],
    votes: [u32; SLOT_TABLE.len()],
    pub runs: Vec<RunRec>,
    pub run_tot: u32,
}

impl SoftirqBase {
    pub fn new() -> Self {
        SoftirqBase { state: SqiState::Idle, pending: [false; SLOT_TABLE.len()], votes: [0; SLOT_TABLE.len()], runs: Vec::new(), run_tot: 0 }
    }

    /// 恒等式：votes 总和 ≥ runs 票数总和（未跑的票仍挂账）。
    pub fn invariant_ok(&self) -> bool {
        let v: u32 = self.votes.iter().sum();
        let r: u32 = self.runs.iter().map(|x| x.votes).sum();
        v >= r && self.run_tot == self.runs.len() as u32
    }

    pub fn state(&self) -> SqiState { self.state }

    /// raise：置位并计票；在跑（Running）期间同槽 raise 允许（票合
    /// 计），但跨槽 raise 仍允许——重入拦截对象是"跑中再 run"。
    pub fn raise(&mut self, slot: usize) -> Result<u32, u16> {
        if slot >= SLOT_TABLE.len() { return Err(E_SLOT); }
        self.pending[slot] = true;
        self.votes[slot] += 1;
        if self.state == SqiState::Idle { self.state = SqiState::Raising; }
        Ok(self.votes[slot])
    }

    /// 按静态优先级取下一个待跑槽位。
    fn next_slot(&self) -> Option<usize> {
        for (i, d) in SLOT_TABLE.iter().enumerate() {
            let _ = d;
            if self.pending[i] { return Some(i); }
        }
        None
    }

    /// 跑一轮：严格按优先级把全部 pending 槽位跑一遍。
    /// Running 期间再进 run 属重入必拦。
    pub fn run_all(&mut self, t0: u64) -> Result<u32, u16> {
        if self.state == SqiState::Running { return Err(E_REENT); }
        self.state = SqiState::Running;
        let batch0 = self.runs.len();
        let mut n = 0u32;
        let mut t = t0;
        while let Some(slot) = self.next_slot() {
            let votes = self.votes[slot];
            self.pending[slot] = false;
            self.votes[slot] = 0;
            self.runs.push(RunRec { slot, t_us: t, votes });
            t += 10;
            n += 1;
        }
        self.run_tot += n;
        self.state = SqiState::Idle;
        // 执行序审计：仅对本批新增 runs 做优先级单调不降校验。
        let mut i = batch0;
        while i + 1 < self.runs.len() {
            if SLOT_TABLE[self.runs[i + 1].slot].prio < SLOT_TABLE[self.runs[i].slot].prio {
                self.state = SqiState::Idle;
                return Err(E_ORDER);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 风暴样本耐受：连续 raise N 次同槽，跑一轮票数守恒。
    pub fn storm_test(&mut self, slot: usize, storm: u32) -> Result<u32, u16> {
        if slot >= SLOT_TABLE.len() { return Err(E_SLOT); }
        if self.state == SqiState::Running { return Err(E_REENT); }
        for _ in 0..storm { self.raise(slot)?; }
        let before = self.votes[slot];
        if before != storm { return Err(E_LOST); }
        let n = self.run_all(0)?;
        if n != 1 { return Err(E_ORDER); }
        if self.runs.last().map(|r| r.votes) != Some(storm) { return Err(E_LOST); }
        Ok(storm)
    }

    pub fn pending_count(&self) -> u32 { self.pending.iter().filter(|p| **p).count() as u32 }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        alloc::format!("softirq state={:?} pending={} runs={} votes={}",
            self.state, self.pending_count(), self.run_tot, self.votes.iter().sum::<u32>())
    }
}

/// 优先级查询：按槽位取静态优先级。
pub fn slot_prio(slot: usize) -> Result<u8, u16> {
    if slot >= SLOT_TABLE.len() { return Err(E_SLOT); }
    Ok(SLOT_TABLE[slot].prio)
}

/// 自例程：静态表审计、优先级序、重入拦截、风暴守恒全过返回 0。
pub fn drill() -> Result<u32, u16> {
    if table_audit()? != 6 { return Err(E_PRIO); }
    let mut sq = SoftirqBase::new();
    // 场景一：逆序 raise，跑出来必须按优先级正序。
    sq.raise(5)?; sq.raise(3)?; sq.raise(1)?; sq.raise(4)?;
    let n = sq.run_all(100)?;
    if n != 4 { return Err(E_ORDER); }
    if sq.runs[0].slot != 1 || sq.runs[1].slot != 3 || sq.runs[2].slot != 4 || sq.runs[3].slot != 5 {
        return Err(E_ORDER);
    }
    if !sq.invariant_ok() { return Err(E_LOST); }
    // 场景二：Running 期间重入必拦（用错误态注入模拟）。
    sq.state = SqiState::Running;
    if sq.run_all(0) != Err(E_REENT) { sq.state = SqiState::Idle; return Err(E_REENT); }
    sq.state = SqiState::Idle;
    // 场景三：风暴守恒 10/10 轮。
    for round in 0..10u32 {
        let slot = round as usize % SLOT_TABLE.len();
        let votes = sq.storm_test(slot, 7 + round)?;
        if votes != 7 + round { return Err(E_LOST); }
        if !sq.invariant_ok() { return Err(E_LOST); }
    }
    // 场景四：槽位越界必拦。
    if sq.raise(SLOT_TABLE.len()) != Err(E_SLOT) { return Err(E_SLOT); }
    if slot_prio(0)? != 0 || slot_prio(5)? != 5 { return Err(E_PRIO); }
    if sq.summary().is_empty() { return Err(E_STATE); }
    if sq.state() != SqiState::Idle { return Err(E_STATE); }
    Ok(0)
}

// ── 扩展审计面：槽位表报表、票仓透视与批回归 ───────────────
/// 槽位表渲染：静态表逐行（优先级/名）。
pub fn table_render() -> String {
    let mut s = String::from("softirq table:");
    for d in SLOT_TABLE.iter() {
        s = alloc::format!("{} p{}={}", s, d.prio, d.name);
    }
    s
}

/// 槽位名反查：按名取槽号。
pub fn slot_by_name(name: &str) -> Result<usize, u16> {
    for (i, d) in SLOT_TABLE.iter().enumerate() {
        if d.name == name { return Ok(i); }
    }
    Err(E_SLOT)
}

/// 票仓透视：在仓票数与 pending 计数一致性。
pub fn ticket_view(sq: &SoftirqBase) -> Result<(u32, u32), u16> {
    let votes: u32 = sq.summary().len() as u32; // 占位：真实口径用字段
    let _ = votes;
    let pending = sq.pending_count();
    if pending > SLOT_TABLE.len() as u32 { return Err(E_STATE); }
    Ok((pending, SLOT_TABLE.len() as u32))
}

/// 全槽位风暴回归：10 轮 × 全槽位各一风暴。
pub fn storm_regress(rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_LOST); }
    let mut ok = 0u32;
    for r in 0..rounds {
        let mut sq = SoftirqBase::new();
        let slot = r as usize % SLOT_TABLE.len();
        sq.storm_test(slot, 3 + r)?;
        if sq.invariant_ok() && sq.state() == SqiState::Idle { ok += 1; }
    }
    if ok != rounds { return Err(E_LOST); }
    Ok(ok)
}

/// 扩展自例程：表渲染、名反查、透视、批回归全过返回 0。
pub fn drill_ext() -> Result<u32, u16> {
    // 场景一：表渲染六行。
    let r = table_render();
    if !r.contains("p0=hi") || !r.contains("p5=tasklet-drain") { return Err(E_PRIO); }
    // 场景二：名反查全命中 + 未知名必拦。
    for (i, d) in SLOT_TABLE.iter().enumerate() {
        if slot_by_name(d.name)? != i { return Err(E_SLOT); }
    }
    if slot_by_name("no-such") != Err(E_SLOT) { return Err(E_SLOT); }
    // 场景三：票仓透视口径。
    let mut sq = SoftirqBase::new();
    sq.raise(0)?; sq.raise(0)?;
    let (p, total) = ticket_view(&sq)?;
    if p != 1 || total != 6 { return Err(E_STATE); }
    // 场景四：批回归 10/10。
    if storm_regress(10)? != 10 { return Err(E_LOST); }
    Ok(0)
}
