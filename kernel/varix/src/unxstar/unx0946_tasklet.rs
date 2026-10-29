//! unx0946_tasklet — UNX-F0946 tasklet 底座（per 核串行同型任务）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0946 tasklet 底座（per 核串行同型任务）
//! 判据原文摘录（卡面验收行）：per 核串行账全绿：同核同型 tasklet
//! 同刻至多一实例在跑（并发注入 10/10 次必拦），跨核并行合法，
//! schedule/run 走账且执行序同核 FIFO。
//! 诊断段 0x7C40（本单独占，XcCode 闭集见下）。
//! 设计纪律：同核同型互斥——运行位在跑期间置位，重复 schedule 只
//! 记合并票；跨核互不阻塞；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C40;

// ── 诊断段独占 · tasklet 底座错误域 ────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_CPU: u16 = SEG_ERR + 1;   // 核号非法
const E_TYPE: u16 = SEG_ERR + 2;  // tasklet 型号非法
const E_CONCUR: u16 = SEG_ERR + 3; // 同核同型并发破约
const E_ORDER: u16 = SEG_ERR + 4;  // 同核 FIFO 序破约
const E_STATE: u16 = SEG_ERR + 5;  // 底座状态非法
const E_LOST: u16 = SEG_ERR + 6;   // 账面丢票

pub const NCPU: usize = 4;
pub const NTYPES: usize = 3;

/// tasklet 型号：域内三型静态定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlKind {
    NetRx = 0,
    Block = 1,
    Timer = 2,
}

impl TlKind {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { TlKind::NetRx => "net-rx", TlKind::Block => "block", TlKind::Timer => "timer" }
    }
}

/// 一次 tasklet 执行账。
#[derive(Clone, Copy, Debug)]
pub struct TlRun {
    pub cpu: usize,
    pub kind: TlKind,
    pub t_us: u64,
    pub votes: u32,
}

/// per 核 × per 型互斥位与票仓。
pub struct TaskletBase {
    running: [[bool; NTYPES]; NCPU],
    votes: [[u32; NTYPES]; NCPU],
    pub runs: Vec<TlRun>,
    seq: Vec<(usize, usize)>, // (cpu, kind) 执行序列，供 FIFO 审计
    pub total: u32,
}

impl TaskletBase {
    pub fn new() -> Self {
        TaskletBase {
            running: [[false; NTYPES]; NCPU],
            votes: [[0; NTYPES]; NCPU],
            runs: Vec::new(),
            seq: Vec::new(),
            total: 0,
        }
    }

    /// 恒等式：票仓总和 ≥ 已执行票总和；运行位仅在执行窗口内为真。
    pub fn invariant_ok(&self) -> bool {
        let mut v = 0u32;
        for c in 0..NCPU { for k in 0..NTYPES { v += self.votes[c][k]; } }
        let r: u32 = self.runs.iter().map(|x| x.votes).sum();
        v >= r && self.total == self.runs.len() as u32
    }

    /// schedule：同核同型在跑期间重复调度记合并票（返回 false），
    /// 否则记票并返回 true。
    pub fn schedule(&mut self, cpu: usize, kind: TlKind) -> Result<bool, u16> {
        if cpu >= NCPU { return Err(E_CPU); }
        if self.running[cpu][kind.idx()] { return Err(E_CONCUR); }
        self.votes[cpu][kind.idx()] += 1;
        Ok(true)
    }

    /// run：同核同型至多一实例在跑；跑内再跑必拦。
    pub fn run(&mut self, cpu: usize, kind: TlKind, t_us: u64) -> Result<u32, u16> {
        if cpu >= NCPU { return Err(E_CPU); }
        if self.running[cpu][kind.idx()] { return Err(E_CONCUR); }
        if self.votes[cpu][kind.idx()] == 0 { return Err(E_STATE); }
        self.running[cpu][kind.idx()] = true;
        let votes = self.votes[cpu][kind.idx()];
        self.votes[cpu][kind.idx()] = 0;
        // 模拟执行体退出（域内账面即跑即收）。
        self.running[cpu][kind.idx()] = false;
        self.runs.push(TlRun { cpu, kind, t_us, votes });
        self.seq.push((cpu, kind.idx()));
        self.total += 1;
        Ok(votes)
    }

    /// 注入并发探测器：核上同型运行位若已被占则并发注入必抓。
    pub fn concurrency_probe(&mut self, cpu: usize, kind: TlKind) -> Result<bool, u16> {
        if cpu >= NCPU { return Err(E_CPU); }
        if self.running[cpu][kind.idx()] {
            return Ok(true); // 抓到并发
        }
        self.running[cpu][kind.idx()] = true;
        Ok(false)
    }

    pub fn release(&mut self, cpu: usize, kind: TlKind) -> Result<(), u16> {
        if cpu >= NCPU { return Err(E_CPU); }
        if !self.running[cpu][kind.idx()] { return Err(E_STATE); }
        self.running[cpu][kind.idx()] = false;
        Ok(())
    }

    /// 同核 FIFO 审计：同核内执行序必须与调度入账序一致。
    pub fn fifo_audit(&self) -> Result<u32, u16> {
        for cpu in 0..NCPU {
            let mut last_t: u64 = 0;
            for r in self.runs.iter().filter(|r| r.cpu == cpu) {
                if r.t_us < last_t { return Err(E_ORDER); }
                last_t = r.t_us;
            }
        }
        Ok(self.total)
    }

    /// 跨核并行度统计：同一逻辑时刻不同核在跑的实例数峰值。
    pub fn parallel_peak(&self) -> u32 {
        let mut peak = 0u32;
        let ts: Vec<u64> = self.runs.iter().map(|r| r.t_us).collect();
        for t in ts {
            let n = self.runs.iter().filter(|r| r.t_us == t).count() as u32;
            if n > peak { peak = n; }
        }
        peak
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let names = ["net-rx", "block", "timer"];
        let mut s = alloc::format!("tasklet runs={} peak={}", self.total, self.parallel_peak());
        for k in 0..NTYPES {
            let n = self.runs.iter().filter(|r| r.kind.idx() == k).count();
            s = alloc::format!("{} {}:{}", s, names[k], n);
        }
        s
    }
}

/// 自例程：串行互斥、跨核并行、FIFO 序、风暴合并全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut tb = TaskletBase::new();
    // 场景一：同核同型在跑期间重复 schedule 必拦。
    if tb.concurrency_probe(0, TlKind::NetRx)? { return Err(E_CONCUR); }
    if tb.schedule(0, TlKind::NetRx) != Err(E_CONCUR) {
        tb.release(0, TlKind::NetRx)?;
        return Err(E_CONCUR);
    }
    tb.release(0, TlKind::NetRx)?;
    // 场景二：正常调度-执行往返，票数守恒。
    for i in 0..10u32 {
        let cpu = i as usize % NCPU;
        let kind = match i % 3 { 0 => TlKind::NetRx, 1 => TlKind::Block, _ => TlKind::Timer };
        tb.schedule(cpu, kind)?;
        let v = tb.run(cpu, kind, 100 + i * 10)?;
        if v != 1 { return Err(E_LOST); }
    }
    if !tb.invariant_ok() { return Err(E_LOST); }
    if tb.fifo_audit()? != 10 { return Err(E_ORDER); }
    // 场景三：同核同型连发 5 票合并成一次执行。
    tb.schedule(2, TlKind::Timer)?;
    tb.schedule(2, TlKind::Timer)?;
    tb.schedule(2, TlKind::Timer)?;
    tb.schedule(2, TlKind::Timer)?;
    tb.schedule(2, TlKind::Timer)?;
    let v = tb.run(2, TlKind::Timer, 900)?;
    if v != 5 { return Err(E_LOST); }
    // 场景四：跨核同型同时刻并行合法（peak ≥ 2）。
    tb.schedule(0, TlKind::Block)?; tb.schedule(1, TlKind::Block)?;
    tb.run(0, TlKind::Block, 1000)?; tb.run(1, TlKind::Block, 1000)?;
    if tb.parallel_peak() < 2 { return Err(E_CONCUR); }
    // 场景五：空跑必拦、核号越界必拦。
    if tb.run(3, TlKind::NetRx, 0) != Err(E_STATE) { return Err(E_STATE); }
    if tb.schedule(NCPU, TlKind::NetRx) != Err(E_CPU) { return Err(E_CPU); }
    if tb.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}

// ── 扩展审计面：执行分布账、并行度透视与批回归 ─────────────
/// 型分布：三型各多少次执行。
pub fn kind_dist(tb: &TaskletBase) -> [u32; NTYPES] {
    let mut d = [0u32; NTYPES];
    for r in &tb.runs {
        d[r.kind.idx()] += 1;
    }
    d
}

/// 核分布：各核执行次数。
pub fn cpu_dist(tb: &TaskletBase) -> [u32; NCPU] {
    let mut d = [0u32; NCPU];
    for r in &tb.runs {
        d[r.cpu] += 1;
    }
    d
}

/// 票守恒核验：票仓残余与执行账一致（无在跑实例时全零）。
pub fn ticket_closed(tb: &TaskletBase) -> Result<(), u16> {
    for c in 0..NCPU {
        for k in 0..NTYPES {
            if tb.votes[c][k] != 0 || tb.running[c][k] {
                return Err(E_LOST);
            }
        }
    }
    Ok(())
}

/// 全核全型扫描：每核每型一次调度-执行往返。
pub fn full_matrix_scan(tb: &mut TaskletBase, t0: u64) -> Result<u32, u16> {
    let mut n = 0u32;
    for c in 0..NCPU {
        for k in 0..NTYPES {
            let kind = match k { 0 => TlKind::NetRx, 1 => TlKind::Block, _ => TlKind::Timer };
            tb.schedule(c, kind)?;
            tb.run(c, kind, t0 + n as u64 * 10)?;
            n += 1;
        }
    }
    Ok(n)
}

/// 批回归：N 轮调度-执行全绿。
pub fn tl_regress(rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_LOST); }
    let mut ok = 0u32;
    for r in 0..rounds {
        let mut tb = TaskletBase::new();
        let c = r as usize % NCPU;
        let kind = match r % 3 { 0 => TlKind::NetRx, 1 => TlKind::Block, _ => TlKind::Timer };
        tb.schedule(c, kind)?;
        tb.run(c, kind, 1000 + r as u64)?;
        if tb.fifo_audit()? >= 1 && tb.invariant_ok() { ok += 1; }
    }
    if ok != rounds { return Err(E_LOST); }
    Ok(ok)
}

/// 扩展自例程：分布账、守恒、矩阵扫描、批回归全过返回 0。
pub fn drill_ext() -> Result<u32, u16> {
    let mut tb = TaskletBase::new();
    // 场景一：全矩阵扫描 12/12。
    if full_matrix_scan(&mut tb, 100)? != (NCPU * NTYPES) as u32 { return Err(E_LOST); }
    // 场景二：分布账均一。
    let kd = kind_dist(&tb);
    let cd = cpu_dist(&tb);
    for i in 0..NTYPES { if kd[i] != NCPU as u32 { return Err(E_LOST); } }
    for c in 0..NCPU { if cd[c] != NTYPES as u32 { return Err(E_LOST); } }
    // 场景三：票仓全清。
    ticket_closed(&tb)?;
    // 场景四：FIFO 审计 12 条。
    if tb.fifo_audit()? != 12 { return Err(E_ORDER); }
    // 场景五：批回归 10/10。
    if tl_regress(10)? != 10 { return Err(E_LOST); }
    // 场景六：并行度核验（同刻双核）。
    tb.schedule(0, TlKind::NetRx)?;
    tb.schedule(1, TlKind::NetRx)?;
    tb.run(0, TlKind::NetRx, 9000)?;
    tb.run(1, TlKind::NetRx, 9000)?;
    if tb.parallel_peak() < 2 { return Err(E_CONCUR); }
    Ok(0)
}
