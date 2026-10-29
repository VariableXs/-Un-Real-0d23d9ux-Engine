//! unx0951_coalesce — UNX-F0951 bottom half 合并与去重（同型多触发合并纪律）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0951 bottom half 合并与去重（同型多触发合并纪律）
//! 判据原文摘录（卡面验收行）：同型 N 次触发合并执行恰一次（N=1..256
//! 全档 10/10 次账平），去重账不翻倍不丢失，跨型不串合并。
//! 诊断段 0x7CE0（本单独占，XcCode 闭集见下）。
//! 设计纪律：合并即票仓——同型多触发只累积计数不排队；执行一次清
//! 票仓一次；跨型边界绝对隔离；守恒恒等式（累计触发数 = 已执行票数
//! + 在仓票数）贯穿全程；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7CE0;

// ── 诊断段独占 · 合并去重错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_DUP: u16 = SEG_ERR + 1;   // 去重失败（同型执行翻倍）
const E_LOST: u16 = SEG_ERR + 2;  // 合并丢票
const E_MIX: u16 = SEG_ERR + 3;   // 跨型串合并
const E_KIND: u16 = SEG_ERR + 4;  // 型号非法
const E_STATE: u16 = SEG_ERR + 5; // 合并器状态非法

pub const MAX_TICKS: u32 = 256;

/// 同型标识（域内四型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BhKind {
    Net = 0,
    Block = 1,
    Timer = 2,
    Tty = 3,
}

pub const NKIND: usize = 4;

impl BhKind {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { BhKind::Net => "net", BhKind::Block => "block", BhKind::Timer => "timer", BhKind::Tty => "tty" }
    }
}

/// 合并执行账：一次执行吞掉多少票。
#[derive(Clone, Copy, Debug)]
pub struct CoRun {
    pub kind: BhKind,
    pub t_us: u64,
    pub tickets: u32,
}

/// 合并器：per 型票仓 + 执行账。
pub struct Coalescer {
    tickets: [u32; NKIND],
    pub raised_tot: [u32; NKIND],
    pub runs: Vec<CoRun>,
    pub exec_tot: u32,
}

impl Coalescer {
    pub fn new() -> Self {
        Coalescer { tickets: [0; NKIND], raised_tot: [0; NKIND], runs: Vec::new(), exec_tot: 0 }
    }

    /// 守恒恒等式：per 型 raised_tot == 已执行票 + 在仓票。
    pub fn invariant_ok(&self) -> bool {
        for k in 0..NKIND {
            let done: u32 = self.runs.iter().filter(|r| r.kind.idx() == k).map(|r| r.tickets).sum();
            if self.raised_tot[k] != done + self.tickets[k] { return false; }
        }
        self.exec_tot == self.runs.len() as u32
    }

    /// 同型触发：只累票不排队。超 MAX_TICKS 报满（防票仓溢出）。
    pub fn raise(&mut self, kind: BhKind) -> Result<u32, u16> {
        let k = kind.idx();
        if self.tickets[k] >= MAX_TICKS { return Err(E_STATE); }
        self.tickets[k] += 1;
        self.raised_tot[k] += 1;
        Ok(self.tickets[k])
    }

    pub fn in_stock(&self, kind: BhKind) -> u32 { self.tickets[kind.idx()] }

    /// 同型执行：一次吞掉全部在仓票，恰一次执行。
    /// 空仓执行属状态错（无事可做却在跑）。
    pub fn run(&mut self, kind: BhKind, t_us: u64) -> Result<u32, u16> {
        let k = kind.idx();
        if self.tickets[k] == 0 { return Err(E_STATE); }
        let n = self.tickets[k];
        self.tickets[k] = 0;
        self.runs.push(CoRun { kind, t_us, tickets: n });
        self.exec_tot += 1;
        Ok(n)
    }

    /// 同型多触发合并审计：N 次触发后单次执行吞 N 票。
    pub fn coalesce_audit(&mut self, kind: BhKind, n: u32) -> Result<u32, u16> {
        if n == 0 || n > MAX_TICKS { return Err(E_KIND); }
        for _ in 0..n { self.raise(kind)?; }
        if self.in_stock(kind) != n { return Err(E_LOST); }
        let got = self.run(kind, 0)?;
        if got != n { return Err(E_LOST); }
        // 二次执行必拦（去重：不翻倍）。
        if self.run(kind, 1) != Err(E_STATE) { return Err(E_DUP); }
        Ok(n)
    }

    /// 跨型隔离审计：对 A 型连发后，B 型仓必须仍空。
    pub fn isolation_audit(&mut self, a: BhKind, b: BhKind) -> Result<(), u16> {
        if a == b { return Err(E_MIX); }
        for _ in 0..10 { self.raise(a)?; }
        if self.in_stock(b) != 0 { return Err(E_MIX); }
        self.run(a, 0)?;
        if self.in_stock(b) != 0 { return Err(E_MIX); }
        Ok(())
    }

    /// 全型清仓：按固定序把各型票全跑一遍。
    pub fn drain_all(&mut self, t0: u64) -> Result<u32, u16> {
        let mut n = 0u32;
        for k in 0..NKIND {
            if self.tickets[k] > 0 {
                let kind = idx_kind(k);
                let t = self.tickets[k];
                self.run(kind, t0 + n as u64 * 10)?;
                if t == 0 { return Err(E_LOST); }
                n += 1;
            }
        }
        Ok(n)
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("coalescer runs={} exec={}", self.runs.len(), self.exec_tot);
        for k in 0..NKIND {
            s = alloc::format!("{} {}:{}", s, idx_kind(k).as_str(), self.tickets[k]);
        }
        s
    }
}

fn idx_kind(i: usize) -> BhKind {
    match i { 0 => BhKind::Net, 1 => BhKind::Block, 2 => BhKind::Timer, _ => BhKind::Tty }
}

/// 自例程：N=1..256 全档合并、跨型隔离、翻倍拦截全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut co = Coalescer::new();
    // 场景一：全档抽验 N ∈ {1,2,3,7,64,127,255,256}。
    let ns = [1u32, 2, 3, 7, 64, 127, 255, 256];
    for (i, n) in ns.iter().enumerate() {
        let kind = idx_kind(i % NKIND);
        if co.coalesce_audit(kind, *n)? != *n { return Err(E_LOST); }
        if !co.invariant_ok() { return Err(E_LOST); }
    }
    // 场景二：跨型隔离四两两组合。
    for a in 0..NKIND {
        for b in 0..NKIND {
            if a != b {
                co.isolation_audit(idx_kind(a), idx_kind(b))?;
            }
        }
    }
    // 场景三：满仓必拦（>256 票）。
    let mut co2 = Coalescer::new();
    for _ in 0..MAX_TICKS { co2.raise(BhKind::Net)?; }
    if co2.raise(BhKind::Net) != Err(E_STATE) { return Err(E_STATE); }
    // 场景四：空仓执行必拦。
    if co2.run(BhKind::Block, 0) != Err(E_STATE) { return Err(E_STATE); }
    // 场景五：全型清仓守恒。
    co.raise(BhKind::Tty)?; co.raise(BhKind::Tty)?; co.raise(BhKind::Timer)?;
    let n = co.drain_all(500)?;
    if n != 2 { return Err(E_LOST); }
    if !co.invariant_ok() { return Err(E_LOST); }
    for k in 0..NKIND {
        if co.tickets[k] != 0 { return Err(E_LOST); }
    }
    if co.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
