//! unx4171_errshort — UNX-F4171 walk 中错误短路传播（诊断段 0x9700）。
//!
//! 判据原文：
//!   J1 中途分量错误（ENOENT/EACCES）立即短路（后续分量零处理，
//!      处理计数=出错位置）。
//!   J2 错误码透传不改写（20 类抽样断言）。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 短路红线：主循环任一分量出错立即返回，出错位置之后的
//!      分量零处理（处理计数 == 出错下标，多一即失守）。
//!   2. 透传不改写：短路返回的错误码与分量层原始码逐位一致，
//!      20 类抽样逐一断言。
//!   3. 短路账：每次短路记（出错位置, 错误码）一账，账数与短路
//!      次数守恒。
//!   4. 成功路径不短路：全部分量处理完计数 = 分量总数。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9700;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SHORT_LEAK: u16 = SEG_BASE + 1;  // 短路后仍有分量被处理（红线失守）
const E_REWRITE: u16 = SEG_BASE + 2;     // 错误码被改写（透传失守）
const E_NO_ENTRY: u16 = SEG_BASE + 3;    // 分量查无（ENOENT 口径）
const E_ACCESS: u16 = SEG_BASE + 4;      // 分量无权（EACCES 口径）
const E_BAL: u16 = SEG_BASE + 5;         // 短路账守恒失守
const E_BAD_STATE: u16 = SEG_BASE + 6;   // 短路状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// POSIX errno 抽样 20 类（Linux 值，透传断言样本）。
pub const SAMPLE_CODES: [u16; 20] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21,
];

/// 短路状态机：Walking 行进中 / Shorted 已短路 / Completed 已走完。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortState {
    Walking,
    Shorted,
    Completed,
}

/// 分量故障表：按下标注入故障码（0=无故障）。
pub struct FaultTable {
    pub faults: Vec<u16>, // faults[i] = 分量 i 的注入码（0=正常）
}

impl FaultTable {
    pub fn new(count: usize) -> FaultTable {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < count {
            v.push(0u16);
            i += 1;
        }
        FaultTable { faults: v }
    }

    pub fn set(&mut self, idx: usize, code: u16) -> Result<(), u16> {
        if idx >= self.faults.len() {
            return Err(E_BAL);
        }
        self.faults[idx] = code;
        Ok(())
    }

    pub fn get(&self, idx: usize) -> u16 {
        if idx >= self.faults.len() {
            return 0;
        }
        self.faults[idx]
    }

    pub fn len(&self) -> usize {
        self.faults.len()
    }
}

/// 一条短路账。
pub struct ShortRecord {
    pub at: usize,  // 出错分量下标（处理计数==此值）
    pub code: u16,  // 原始错误码（透传）
}

/// 短路枢纽：主循环 + 故障注入表 + 短路账 + 透传断言。
pub struct ShortWalker {
    pub state: ShortState,
    ledger: Vec<ShortRecord>,
    pub short_total: u32,    // 短路总数
    pub processed: usize,    // 最近一次处理计数（判据 J1 裁定量）
    pub passthrough_hits: u32, // 透传零改写命中账（判据 J2 面）
}

impl ShortWalker {
    pub fn new() -> ShortWalker {
        ShortWalker {
            state: ShortState::Walking,
            ledger: Vec::new(),
            short_total: 0,
            processed: 0,
            passthrough_hits: 0,
        }
    }

    /// 主循环：逐分量走故障表，遇非零故障码立即短路返回（判据 J1）。
    /// 返回 Ok(处理计数) 或 Err(原始错误码)。
    pub fn walk(&mut self, faults: &FaultTable) -> Result<usize, u16> {
        self.state = ShortState::Walking;
        self.processed = 0;
        let mut i = 0usize;
        while i < faults.len() {
            let code = faults.get(i);
            if code != 0 {
                // 立即短路：后续分量零处理，处理计数 == 出错下标。
                self.processed = i;
                self.ledger.push(ShortRecord { at: i, code });
                self.short_total += 1;
                self.state = ShortState::Shorted;
                return Err(code); // 原码透传，零改写
            }
            self.processed = i + 1;
            i += 1;
        }
        self.state = ShortState::Completed;
        Ok(self.processed)
    }

    /// 短路定位断言：处理计数必须恰等出错下标（判据 J1 断言面）。
    pub fn assert_short_at(&self, expect_at: usize) -> Result<(), u16> {
        if self.processed != expect_at {
            return Err(E_SHORT_LEAK);
        }
        if self.ledger.is_empty() {
            return Err(E_BAL);
        }
        let last = self.ledger.len() - 1;
        if self.ledger[last].at != expect_at {
            return Err(E_SHORT_LEAK);
        }
        Ok(())
    }

    /// 透传抽样断言：20 类码逐一注入短路并核对原码出参（判据 J2）。
    pub fn assert_passthrough(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < SAMPLE_CODES.len() {
            let mut f = FaultTable::new(3);
            f.set(1, SAMPLE_CODES[i])?;
            let out = self.walk(&f);
            match out {
                Err(e) => {
                    if e != SAMPLE_CODES[i] {
                        return Err(E_REWRITE);
                    }
                    self.passthrough_hits += 1;
                }
                Ok(_) => return Err(E_BAL),
            }
            i += 1;
        }
        if self.passthrough_hits != SAMPLE_CODES.len() as u32 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 短路账守恒：账数 = 短路总数，账内定位与透传码全检。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.short_total {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].code == 0 {
                return Err(E_BAL);
            }
            i += 1;
        }
        match self.state {
            ShortState::Shorted | ShortState::Completed => {}
            ShortState::Walking => return Err(E_BAD_STATE),
        }
        Ok(())
    }

    /// 短路账读取（回放核对用）：返回第 i 条（定位, 错误码）。
    pub fn ledger_at(&self, i: usize) -> Result<(usize, u16), u16> {
        if i >= self.ledger.len() {
            return Err(E_BAL);
        }
        Ok((self.ledger[i].at, self.ledger[i].code))
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 短路定位 → R2 边界 → R3 透传 20 类抽样 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：位置 2 注 ENOENT → 短路在 2；位置 1 注 EACCES → 短路在 1。
    let mut w = ShortWalker::new();
    let mut f = FaultTable::new(5);
    f.set(2, E_NO_ENTRY)?;
    if w.walk(&f).is_ok() {
        return Err(E_SHORT_LEAK);
    }
    w.assert_short_at(2)?;
    let mut f2 = FaultTable::new(5);
    f2.set(1, E_ACCESS)?;
    if w.walk(&f2).is_ok() {
        return Err(E_SHORT_LEAK);
    }
    w.assert_short_at(1)?;
    w.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：无故障走完全程（计数=总数）/ 零分量表。
    let mut w = ShortWalker::new();
    let clean = FaultTable::new(6);
    let n = w.walk(&clean)?;
    if n != 6 || w.state != ShortState::Completed {
        return Err(E_BAL);
    }
    let empty = FaultTable::new(0);
    let n0 = w.walk(&empty)?;
    if n0 != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 注入拦截：首分量即故障（计数=0）/ 透传 20 类抽样零改写。
    let mut w = ShortWalker::new();
    let mut f = FaultTable::new(4);
    f.set(0, E_NO_ENTRY)?;
    if w.walk(&f).is_ok() {
        return Err(E_SHORT_LEAK);
    }
    w.assert_short_at(0)?;
    w.assert_passthrough()?;
    if w.passthrough_hits != 20 {
        return Err(E_REWRITE);
    }
    hits += 1;

    // R4 守恒对账：短路账 = 短路总数 + 透传账可回放。
    let mut w = ShortWalker::new();
    let mut f1 = FaultTable::new(3);
    f1.set(2, 5)?;
    w.walk(&f1).is_err();
    let mut f2 = FaultTable::new(3);
    f2.set(0, 13)?;
    w.walk(&f2).is_err();
    w.audit()?;
    if w.ledger_len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 追加回归：多点短路账回放 + 定位单调核对 + 写回。
    let mut w = ShortWalker::new();
    let mut pos = 0usize;
    while pos < 6 {
        let mut f = FaultTable::new(8);
        f.set(pos, E_NO_ENTRY)?;
        if w.walk(&f).is_ok() {
            return Err(E_SHORT_LEAK);
        }
        w.assert_short_at(pos)?;
        pos += 1;
    }
    if w.short_total != 6 || w.ledger_len() != 6 {
        return Err(E_BAL);
    }
    // 账回放：每条定位与注入位置一致、错误码全为 ENOENT。
    let mut i = 0usize;
    while i < 6 {
        let (at, code) = w.ledger_at(i)?;
        if at != i || code != E_NO_ENTRY {
            return Err(E_SHORT_LEAK);
        }
        i += 1;
    }
    w.audit()?;
    w.writeback(true, 0x70)?;
    hits += 1;

    Ok(hits)
}
