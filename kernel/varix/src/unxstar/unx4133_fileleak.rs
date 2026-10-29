//! unx4133_fileleak — UNX-F4133 file 泄漏回归（万次开-关）（诊断段 0x9280）。
//!
//! 判据原文：UNX-F4133-J1 万次 open-close 循环后 file 池驻留回归基线（残差=0），
//!           泄漏计数器全程恒零。
//!   J2 峰值驻留不超过池容量。
//!   J3 释放即换代，旧二元组释放必撞代次门。
//!   J4 扫描两式互证：活槽数等于 allocs 减 frees。
//! 设计纪律：
//!   1. 池驻留量是唯一判据：跑完 N 轮后 in_use 必须回到基线 0，任何「近似为零」
//!      的说法都不算通过——残差按整数比较，不设容差。
//!   2. 每槽独立代次（gen）：释放即换代，旧句柄凭「(槽号, 代次)」二元组无法再
//!      误释放（防 ABA 型重复释放），代次不回收也不回绕。
//!   3. 泄漏检测走全池扫描（sweep）：只有 InUse 且代次匹配的槽才算活；扫描同时
//!      核对 allocs - frees == in_use，两式互证。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9280 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9280;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOSLOT: u16 = SEG_BASE + 1; // 池满（EMFILE 面）
const E_DOUBLE: u16 = SEG_BASE + 2; // 重复释放（槽已 Free）
const E_STALE: u16 = SEG_BASE + 3; // 代次不符（旧句柄误释放）
const E_SLOT: u16 = SEG_BASE + 4; // 槽号越界
const E_LEAK: u16 = SEG_BASE + 5; // 扫描发现驻留未归零（泄漏）
const E_ACCT: u16 = SEG_BASE + 6; // allocs-frees 与 in_use 不平
const E_RESID: u16 = SEG_BASE + 7; // 残差非零（判据失守）
const E_PEAK: u16 = SEG_BASE + 8; // 峰值账倒挂（peak < in_use）

/// 池容量：256 槽（够跑并发峰值，同时保持内存账可手算）。
pub const POOL_CAP: usize = 256;
/// 回归轮数：判据原文「万次」。
pub const ROUNDS: u32 = 10_000;

/// 槽生命周期状态机：Free → InUse → Free（换代）／Leaked（扫描认定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Free,
    InUse,
    Leaked,
}

/// 池槽：槽号 + 代次 + 状态。
#[derive(Clone, Copy)]
pub struct Slot {
    /// 槽号（0..POOL_CAP）
    pub id: u16,
    /// 代次（每次分配递增，释放后旧代次句柄失效）
    pub gen: u32,
    /// 状态
    pub state: SlotState,
}

/// 池：槽表 + 驻留/收发账 + 泄漏账。
pub struct FilePool {
    /// 定长槽表
    pub slots: Vec<Slot>,
    /// 当前驻留（占用槽数）
    pub in_use: u32,
    /// 历史峰值驻留
    pub peak: u32,
    /// 累计分配次数（u64 跑万次不溢）
    pub allocs: u64,
    /// 累计释放次数
    pub frees: u64,
    /// 泄漏计数器（判据要求全程恒零）
    pub leaks: u32,
    /// 扫描次数
    pub sweeps: u32,
}

impl FilePool {
    pub fn with_cap(cap: usize) -> Result<FilePool, u16> {
        if cap == 0 || cap > POOL_CAP {
            return Err(E_SLOT); // 只受理闭集容量，防内存账失真
        }
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < cap {
            v.push(Slot {
                id: i as u16,
                gen: 0,
                state: SlotState::Free,
            });
            i += 1;
        }
        Ok(FilePool {
            slots: v,
            in_use: 0,
            peak: 0,
            allocs: 0,
            frees: 0,
            leaks: 0,
            sweeps: 0,
        })
    }

    pub fn cap(&self) -> usize {
        self.slots.len()
    }

    /// 分配：取首个 Free 槽，代次加一，返回 (槽号, 代次)。
    pub fn alloc(&mut self) -> Result<(u16, u32), u16> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].state == SlotState::Free {
                self.slots[i].gen += 1;
                self.slots[i].state = SlotState::InUse;
                self.in_use += 1;
                self.allocs += 1;
                if self.in_use > self.peak {
                    self.peak = self.in_use;
                }
                return Ok((self.slots[i].id, self.slots[i].gen));
            }
            i += 1;
        }
        Err(E_NOSLOT)
    }

    /// 释放：校验 (槽号, 代次) 二元组；代次不符 → E_STALE，重复释放 → E_DOUBLE。
    pub fn free(&mut self, id: u16, gen: u32) -> Result<(), u16> {
        let i = id as usize;
        if i >= self.slots.len() {
            return Err(E_SLOT);
        }
        if self.slots[i].gen != gen {
            return Err(E_STALE);
        }
        match self.slots[i].state {
            SlotState::Free => return Err(E_DOUBLE),
            SlotState::Leaked => return Err(E_LEAK),
            SlotState::InUse => {}
        }
        self.slots[i].state = SlotState::Free;
        self.slots[i].gen += 1; // 释放即换代：旧句柄失去效力
        if self.in_use == 0 {
            return Err(E_ACCT);
        }
        self.in_use -= 1;
        self.frees += 1;
        Ok(())
    }

    /// 全池扫描：活槽计数 + 两式互证 + 峰值账核对。
    pub fn sweep(&mut self) -> Result<u32, u16> {
        self.sweeps += 1;
        let mut live = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].state == SlotState::InUse {
                live += 1;
            }
            i += 1;
        }
        if live != self.in_use {
            self.leaks += live - self.in_use;
            return Err(E_ACCT);
        }
        if self.allocs - self.frees != self.in_use as u64 {
            return Err(E_ACCT);
        }
        if self.peak < self.in_use {
            return Err(E_PEAK);
        }
        Ok(live)
    }

    /// 驻留残差：万次循环后必须为 0。非零即 E_RESID。
    pub fn residual(&self) -> Result<u32, u16> {
        let r = if self.allocs >= self.frees {
            (self.allocs - self.frees) as u32
        } else {
            return Err(E_ACCT);
        };
        if r != self.in_use {
            return Err(E_ACCT);
        }
        if r != 0 {
            return Err(E_RESID);
        }
        Ok(r)
    }

    /// 单轮开-关（判据的动作单元）：alloc + free 成对，返回是否成对成功。
    pub fn open_close_once(&mut self) -> Result<(), u16> {
        let (id, gen) = self.alloc()?;
        self.free(id, gen)
    }
}

/// 回归台：跑 N 轮并留存基线/终值快照，供判据对账。
pub struct LeakBench {
    /// 轮数
    pub rounds: u32,
    /// 成功轮数
    pub ok_rounds: u32,
    /// 失败轮数（含池满）
    pub bad_rounds: u32,
    /// 起始驻留基线
    pub baseline: u32,
    /// 结束驻留
    pub final_in_use: u32,
    /// 峰值
    pub peak: u32,
    /// 泄漏计（全程应恒零）
    pub leaks: u32,
}

impl LeakBench {
    pub fn new() -> LeakBench {
        LeakBench {
            rounds: 0,
            ok_rounds: 0,
            bad_rounds: 0,
            baseline: 0,
            final_in_use: 0,
            peak: 0,
            leaks: 0,
        }
    }

    /// 跑 n 轮单开单关。
    pub fn run(&mut self, pool: &mut FilePool, n: u32) -> Result<(), u16> {
        self.baseline = pool.in_use;
        let mut i = 0u32;
        while i < n {
            match pool.open_close_once() {
                Ok(()) => self.ok_rounds += 1,
                Err(E_NOSLOT) => self.bad_rounds += 1,
                Err(e) => return Err(e),
            }
            i += 1;
        }
        self.rounds += n;
        self.final_in_use = pool.in_use;
        self.peak = pool.peak;
        self.leaks = pool.leaks;
        Ok(())
    }

    /// 判据式：残差 = 终值 - 基线，必须为 0 且不是「靠容差抵消」。
    pub fn residual_zero(&self) -> bool {
        self.final_in_use == self.baseline
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOSLOT => "池满，无空闲槽",
        E_DOUBLE => "重复释放（槽已 Free）",
        E_STALE => "代次不符（旧句柄误释放）",
        E_SLOT => "槽号越界或容量非法",
        E_LEAK => "扫描认定泄漏或对泄漏槽操作",
        E_ACCT => "allocs-frees 与 in_use 不平",
        E_RESID => "驻留残差非零（判据失守）",
        E_PEAK => "峰值账倒挂",
        _ => "未知诊断码",
    }
}

/// 确定性演练：万次开-关 → 并发峰值 → 泄漏注入 → 残差对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：万次单开单关，残差 0、泄漏计 0（判据主断言）。
    let mut pool = FilePool::with_cap(64)?;
    let mut bench = LeakBench::new();
    bench.run(&mut pool, ROUNDS)?;
    if bench.ok_rounds != ROUNDS || bench.bad_rounds != 0 {
        return Err(E_ACCT);
    }
    if pool.in_use != 0 || !bench.residual_zero() {
        return Err(E_RESID);
    }
    if pool.leaks != 0 || bench.leaks != 0 {
        return Err(E_LEAK);
    }
    if pool.allocs != ROUNDS as u64 || pool.frees != ROUNDS as u64 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：扫描互证——活槽 0、allocs-frees==in_use、峰值不超过容量。
    if pool.sweep()? != 0 {
        return Err(E_LEAK);
    }
    if pool.peak != 1 {
        return Err(E_PEAK); // 单开单关的峰值必为 1
    }
    if pool.peak as usize > pool.cap() {
        return Err(E_PEAK);
    }
    hits += 1;

    // R3：并发峰值——一次开 32 个，峰值 32，全关后回 0。
    let mut p2 = FilePool::with_cap(64)?;
    let mut held: Vec<(u16, u32)> = Vec::new();
    let mut i = 0;
    while i < 32 {
        held.push(p2.alloc()?);
        i += 1;
    }
    if p2.in_use != 32 || p2.peak != 32 {
        return Err(E_PEAK);
    }
    while let Some(&(id, gen)) = held.last() {
        p2.free(id, gen)?;
        held.pop();
    }
    if p2.in_use != 0 || p2.sweep()? != 0 {
        return Err(E_RESID);
    }
    hits += 1;

    // R4：池满拒收——容量 4 的池开第 5 个 → E_NOSLOT，且拒收不动账。
    let mut p3 = FilePool::with_cap(4)?;
    let mut k = 0;
    while k < 4 {
        p3.alloc()?;
        k += 1;
    }
    let a_before = p3.allocs;
    if p3.alloc() != Err(E_NOSLOT) {
        return Err(E_NOSLOT);
    }
    if p3.allocs != a_before || p3.in_use != 4 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：泄漏注入 10/10 次——故意不 free，扫描必检出残差。
    let mut leaks_seen = 0u32;
    let mut n = 0;
    while n < 10 {
        let mut p = FilePool::with_cap(8)?;
        let (id, gen) = p.alloc()?;
        // 故意不释放，模拟路径遗漏
        if p.residual() != Err(E_RESID) {
            return Err(E_RESID);
        }
        if p.free(id, gen).is_err() {
            return Err(E_ACCT);
        }
        if p.residual()? != 0 {
            return Err(E_RESID);
        }
        leaks_seen += 1;
        n += 1;
    }
    if leaks_seen != 10 {
        return Err(E_LEAK);
    }
    hits += 1;

    // I2：重复释放 → E_DOUBLE（先释放成功，再释放同二元组走代次门 → E_STALE）。
    let mut p4 = FilePool::with_cap(2)?;
    let (id, gen) = p4.alloc()?;
    p4.free(id, gen)?;
    // 释放即换代，同一 (id, gen) 再来一道必先撞代次门
    if p4.free(id, gen) != Err(E_STALE) {
        return Err(E_STALE);
    }
    // 拿新代次再释放一次：槽已 Free → E_DOUBLE
    let cur_gen = match p4.slots.get(id as usize) {
        Some(s) => s.gen,
        None => return Err(E_SLOT),
    };
    if p4.free(id, cur_gen) != Err(E_DOUBLE) {
        return Err(E_DOUBLE);
    }
    hits += 1;

    // I3：槽号越界 → E_SLOT；非法容量 → E_SLOT。
    if p4.free(99, 1) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    if FilePool::with_cap(0).is_ok() {
        return Err(E_SLOT);
    }
    if FilePool::with_cap(POOL_CAP + 1).is_ok() {
        return Err(E_SLOT);
    }
    hits += 1;

    // I4：泄漏计数器全程恒零（含所有注入轮次）。
    if pool.leaks != 0 || p2.leaks != 0 || p3.leaks != 0 || p4.leaks != 0 {
        return Err(E_LEAK);
    }
    hits += 1;

    // C1：账目终审——万次池 allocs==frees==10000，峰值 1，扫描 1 次。
    if pool.allocs != 10_000 || pool.frees != 10_000 {
        return Err(E_ACCT);
    }
    if pool.sweeps != 1 || pool.in_use != 0 {
        return Err(E_ACCT);
    }
    if !bench.residual_zero() || bench.final_in_use != 0 {
        return Err(E_RESID);
    }
    hits += 1;

    Ok(hits)
}
