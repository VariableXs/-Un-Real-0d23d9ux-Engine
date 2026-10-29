//! unx4068_hitledger — UNX-F4068 缓存命中率账本可导出（诊断段 0x8AC0）。
//!
//! 判据原文（提炼）：
//!   J1. 命中/未命中/回收三计数账可导出（结构化快照）。
//!   J2. 命中率计算与原始计数复算一致（万次负载对照零差）。
//!   J3. 账守恒：命中 + 未命中 + 回收旁路 == 总请求。
//!   J4. 零请求求率按闭集错误拒收（无意义账不导出）。
//! 设计纪律：
//!   1. 命中率整数化：千分比 hits*1000/reads（无浮点纪律）。
//!   2. 回收旁路：驱逐产生的请求直接计回收账（不入命中/未命中）。
//!   3. 万次负载对照：原始计数逐步累加与账本快照逐项复核。
//!   4. 导出面：LedgerExport 结构只读快照，导出即冻结。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8AC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LEDGER: u16 = SEG_BASE + 1;  // 三账守恒破坏
const E_RATE_FAIL: u16 = SEG_BASE + 2; // 命中率复算不一致
const E_NO_REQ: u16 = SEG_BASE + 3;  // 零请求求率（判据 J4）
const E_BAD_ARG: u16 = SEG_BASE + 4; // 参数非法
const E_SNAP_INV: u16 = SEG_BASE + 5;// 快照与活账不一致
const E_CAP: u16 = SEG_BASE + 6;     // 缓存配额非法
const E_OVERFLOW: u16 = SEG_BASE + 7;// 计数饱和顶

/// 计数饱和顶（u32 纪律上限）。
pub const COUNT_TOP: u32 = 100_000;

/// 账本导出快照（判据 J1 结构化导出面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerExport {
    pub hits: u32,     // 命中数
    pub misses: u32,   // 未命中数
    pub reclaims: u32, // 回收旁路数
    pub reads: u32,    // 总请求数
    pub rate_permille: u32, // 命中率（千分比）
}

/// 请求种类三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqKind {
    Hit,     // 命中
    Miss,    // 未命中
    Reclaim, // 回收旁路
}

/// 命中率账本：三账 + 导出面。
pub struct HitLedger {
    pub hits: u32,     // 命中账
    pub misses: u32,   // 未命中账
    pub reclaims: u32, // 回收账
    pub reads: u32,    // 总请求账（守恒主账）
    pub frozen: Vec<LedgerExport>, // 导出冻结账（历史快照）
}

impl HitLedger {
    pub fn new() -> HitLedger {
        HitLedger {
            hits: 0,
            misses: 0,
            reclaims: 0,
            reads: 0,
            frozen: Vec::new(),
        }
    }

    /// 记一笔请求（判据 J1 三账分录）。
    pub fn record(&mut self, kind: ReqKind) -> Result<(), u16> {
        if self.reads >= COUNT_TOP {
            return Err(E_OVERFLOW);
        }
        self.reads += 1;
        match kind {
            ReqKind::Hit => self.hits += 1,
            ReqKind::Miss => self.misses += 1,
            ReqKind::Reclaim => self.reclaims += 1,
        }
        Ok(())
    }

    /// 命中率千分比复算（判据 J2）：hits*1000/reads。
    /// 零请求拒收（判据 J4）。
    pub fn rate_permille(&self) -> Result<u32, u16> {
        if self.reads == 0 {
            return Err(E_NO_REQ);
        }
        if self.hits + self.misses + self.reclaims != self.reads {
            return Err(E_LEDGER);
        }
        Ok(self.hits * 1000 / self.reads)
    }

    /// 导出快照并冻结（判据 J1：可导出面）。
    pub fn export(&mut self) -> Result<LedgerExport, u16> {
        let rate = self.rate_permille()?;
        let snap = LedgerExport {
            hits: self.hits,
            misses: self.misses,
            reclaims: self.reclaims,
            reads: self.reads,
            rate_permille: rate,
        };
        self.frozen.push(snap);
        Ok(snap)
    }

    /// 万次负载对照复算（判据 J2）：重放快照计数与活账逐项核对。
    pub fn crosscheck_snap(&self, snap: &LedgerExport) -> Result<(), u16> {
        if snap.hits != self.hits
            || snap.misses != self.misses
            || snap.reclaims != self.reclaims
            || snap.reads != self.reads
        {
            return Err(E_SNAP_INV);
        }
        // 复算命中率：与快照值严格一致
        if self.reads == 0 {
            return Err(E_NO_REQ);
        }
        let re = self.hits * 1000 / self.reads;
        if re != snap.rate_permille {
            return Err(E_RATE_FAIL);
        }
        Ok(())
    }

    /// 三账守恒（判据 J3）。
    pub fn ledger_ok(&self) -> bool {
        self.hits + self.misses + self.reclaims == self.reads
    }
}

/// 万次负载生成（确定性序列：每 10 笔 6 hit / 3 miss / 1 reclaim）。
/// 返回（理论 hits, 理论 misses, 理论 reclaims）。
pub fn load_ten_thousand(l: &mut HitLedger) -> Result<(u32, u32, u32), u16> {
    for k in 0..10000u32 {
        let kind = match k % 10 {
            0 | 1 | 2 | 3 | 4 | 5 => ReqKind::Hit,
            6 | 7 | 8 => ReqKind::Miss,
            _ => ReqKind::Reclaim,
        };
        l.record(kind)?;
    }
    Ok((6000, 3000, 1000))
}

// ── 扩展面：账本诊断与快照审计 ───────────────────────────────

impl HitLedger {
    /// 单账读数（判据 J1 三账逐项只读面）。
    pub fn count_of(&self, kind: ReqKind) -> u32 {
        match kind {
            ReqKind::Hit => self.hits,
            ReqKind::Miss => self.misses,
            ReqKind::Reclaim => self.reclaims,
        }
    }

    /// 未命中率千分比（命中率互补面，与 rate_permille 恒和 1000）。
    pub fn miss_rate_permille(&self) -> Result<u32, u16> {
        if self.reads == 0 {
            return Err(E_NO_REQ);
        }
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        Ok((self.misses + self.reclaims) * 1000 / self.reads)
    }

    /// 冻结快照数只读面（导出历史深度）。
    pub fn frozen_count(&self) -> u32 {
        self.frozen.len() as u32
    }

    /// 指定历史快照回读（超界拒收，判据 J1 导出可回放面）。
    pub fn frozen_at(&self, idx: usize) -> Result<LedgerExport, u16> {
        if idx >= self.frozen.len() {
            return Err(E_SNAP_INV);
        }
        Ok(self.frozen[idx])
    }

    /// 快照单调性审计：reads 序列必须非降（导出账时序一致）。
    pub fn frozen_monotonic(&self) -> Result<(), u16> {
        for i in 1..self.frozen.len() {
            if self.frozen[i].reads < self.frozen[i - 1].reads {
                return Err(E_SNAP_INV);
            }
        }
        Ok(())
    }

    /// 命中率与未命中率恒和核对（双向导出一致性面）。
    pub fn rate_pair_consistent(&self) -> Result<(), u16> {
        let r = self.rate_permille()?;
        let m = self.miss_rate_permille()?;
        if r + m != 1000 {
            return Err(E_RATE_FAIL);
        }
        Ok(())
    }
}

// ── 扩展面：负载回放与账本回归 ───────────────────────────────

impl HitLedger {
    /// 小负载确定性回放：每 4 笔 3 hit / 1 miss（单元负载注入面）。
    pub fn replay_small(&mut self, rounds: u32) -> Result<(u32, u32), u16> {
        if rounds == 0 {
            return Err(E_BAD_ARG);
        }
        for _ in 0..rounds {
            self.record(ReqKind::Hit)?;
            self.record(ReqKind::Hit)?;
            self.record(ReqKind::Hit)?;
            self.record(ReqKind::Miss)?;
        }
        Ok((self.hits, self.misses))
    }

    /// 理论分布核对：回放后命中账 == 3*rounds + 前值（回放零差面）。
    pub fn assert_replay(&self, base_hits: u32, rounds: u32) -> Result<(), u16> {
        if self.hits != base_hits + rounds * 3 {
            return Err(E_RATE_FAIL);
        }
        Ok(())
    }

    /// 账本清零（回归重跑面：四账同清，冻结账保留可回放）。
    pub fn reset_running(&mut self) -> Result<(), u16> {
        self.hits = 0;
        self.misses = 0;
        self.reclaims = 0;
        self.reads = 0;
        Ok(())
    }

    /// 冻结账与活账时序审计（导出面总检：单调 + 守恒双通过）。
    pub fn audit_all(&self) -> Result<u32, u16> {
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        for i in 1..self.frozen.len() {
            if self.frozen[i].reads < self.frozen[i - 1].reads {
                return Err(E_SNAP_INV);
            }
        }
        Ok(self.frozen.len() as u32)
    }
}

/// 自例程：万次负载→导出→复算对照→零请求拒收→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut l = HitLedger::new();

    // R1 零请求求率拒收（判据 J4）
    match l.rate_permille() {
        Err(E_NO_REQ) => {}
        _ => return Err(E_NO_REQ),
    }
    hits += 1;

    // R2 万次负载：三账理论分布核对（判据 J1/J2 前置）
    let (h, m, r) = load_ten_thousand(&mut l)?;
    if l.hits != h || l.misses != m || l.reclaims != r {
        return Err(E_LEDGER);
    }
    if l.reads != 10000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 导出快照：命中率 6000*1000/10000 = 600‰
    let snap = l.export()?;
    if snap.rate_permille != 600 {
        return Err(E_RATE_FAIL);
    }
    if l.frozen.len() != 1 {
        return Err(E_SNAP_INV);
    }
    hits += 1;

    // R4 快照对照复算零差（判据 J2）
    l.crosscheck_snap(&snap)?;
    hits += 1;

    // R5 三账守恒 + 追加导出一致性（判据 J3）
    if !l.ledger_ok() {
        return Err(E_LEDGER);
    }
    let snap2 = l.export()?;
    if snap2.reads != 10000 || l.frozen.len() != 2 {
        return Err(E_SNAP_INV);
    }
    hits += 1;

    Ok(hits)
}
