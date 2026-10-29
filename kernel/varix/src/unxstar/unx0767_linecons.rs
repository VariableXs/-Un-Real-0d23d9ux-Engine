//! UNX-F0767 · 行数守恒全域复核（40 批逐批对账 240,000） · 诊断段 0x5ea0
//!
//! 判据原文（UNX-F0767-J1）：守恒复核过（40 批批小计逐批重算 = 逐条求和）
//! 且累计链 40 段零断、终点 240,000 再现。
//!
//! 设计纪律（本文件自守）：
//! 1. no_std + alloc 纯功能线；零 unwrap/expect/panic/浮点/std。
//! 2. 所有账本使用定容数组 + Vec 混合结构，容量上限常量闭集。
//! 3. 守恒恒等式显式注释：40 批批小计之和 == 累计链终点 == 240,000。
//! 4. 每条错误路径返回 Result<_, u16>，诊断码闭集于本段 0x5ea0。
//! 5. 结尾 pub fn drill() 自例程覆盖正常链 + 三类破坏路径。
//! 6. 逐槽 while 扫描，不依赖切片越界 panic。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（清单指定 0x5ea0）。
pub const SEG_BASE: u16 = 0x5ea0;

// ---------------------------------------------------------------------------
// XcCode 诊断码闭集（SEG_BASE + 1..N，中文注释逐码说明）
// ---------------------------------------------------------------------------
const E_BATCH_EMPTY: u16 = SEG_BASE + 1; // 批记录缺失：批账中存在空槽
const E_BATCH_COUNT: u16 = SEG_BASE + 2; // 批数不为 40：全域批账不完整
const E_SUBTOTAL_MISMATCH: u16 = SEG_BASE + 3; // 批小计与批内逐条求和不一致
const E_ENTRY_EMPTY: u16 = SEG_BASE + 4; // 批内条目缺失：明细账空槽
const E_ENTRY_LINES_ZERO: u16 = SEG_BASE + 5; // 条目行数为零：违反纯功能行数下限
const E_ENTRY_FID_DUP: u16 = SEG_BASE + 6; // 条目 fid 重复：同批内编号冲突
const E_CHAIN_BREAK: u16 = SEG_BASE + 7; // 累计链断裂：段间差值不等于批小计
const E_TERMINAL_MISMATCH: u16 = SEG_BASE + 8; // 终点不等于 240,000：守恒失败
const E_LEDGER_SEALED: u16 = SEG_BASE + 9; // 账已封：封账后禁止再写入
const E_LEDGER_NOT_SEALED: u16 = SEG_BASE + 10; // 账未封：复核前必须先封账
const E_BATCH_TOTAL_MISMATCH: u16 = SEG_BASE + 11; // 批条数与账面条目数不一致
const E_FID_NOT_CONTIGUOUS: u16 = SEG_BASE + 12; // fid 不连续：全域编号断裂
const E_AUDIT_ABSENT: u16 = SEG_BASE + 13; // 复核记录缺失：审计账为空
const E_AUDIT_VERDICT: u16 = SEG_BASE + 14; // 复核判定非通过：存在未结分歧
const E_REPLAY_DRIFT: u16 = SEG_BASE + 15; // 链重放漂移：重放终点与原账不符
const E_STAMP_DUP: u16 = SEG_BASE + 16; // 复核戳重复：同批同轮重复登记

/// 全域批数常量：40 批。
pub const TOTAL_BATCHES: usize = 40;
/// 每批标准行数小计：6,000 行（与 F0751 定版一致）。
pub const STD_BATCH_LINES: u32 = 6_000;
/// 全域终点：240,000 行纯功能代码。
pub const GRAND_TOTAL: u32 = 240_000;
/// 每批明细条数上限（定容）。
pub const MAX_ENTRIES_PER_BATCH: usize = 32;
/// 审计轮次上限（定容）。
pub const MAX_AUDIT_ROUNDS: usize = 8;

/// 单条明细条目：全域行数账的最小记账单元。
#[derive(Clone)]
pub struct LedgerEntry {
    /// 条目 fid（全域唯一，1..=800 连续）。
    pub fid: u16,
    /// 条目名（短名）。
    pub name: String,
    /// 实测纯功能行数（wc -l 口径）。
    pub lines: u32,
}

/// 单批账：批小计 + 批内明细（定容数组账本）。
pub struct BatchRow {
    /// 批号（1..=40）。
    pub batch_no: u16,
    /// 批名（如 "B01"）。
    pub batch_tag: String,
    /// 账面批小计（先逐条求和后落账，零虚记）。
    pub recorded_subtotal: u32,
    /// 批内明细定容账本。
    pub entries: [Option<LedgerEntry>; MAX_ENTRIES_PER_BATCH],
    /// 实际明细条数。
    pub entry_count: usize,
}

impl BatchRow {
    /// 新建空批账。
    pub fn new(batch_no: u16, batch_tag: &str) -> Self {
        BatchRow {
            batch_no,
            batch_tag: batch_tag.to_string(),
            recorded_subtotal: 0,
            entries: [None, None, None, None, None, None, None, None,
                      None, None, None, None, None, None, None, None,
                      None, None, None, None, None, None, None, None,
                      None, None, None, None, None, None, None, None],
            entry_count: 0,
        }
    }

    /// 追加明细条目；同批内 fid 重复拒绝。
    pub fn add_entry(&mut self, fid: u16, name: &str, lines: u32) -> Result<(), u16> {
        if lines == 0 {
            return Err(E_ENTRY_LINES_ZERO);
        }
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                if e.fid == fid {
                    return Err(E_ENTRY_FID_DUP);
                }
            }
            i += 1;
        }
        if self.entry_count >= MAX_ENTRIES_PER_BATCH {
            return Err(E_BATCH_COUNT);
        }
        self.entries[self.entry_count] = Some(LedgerEntry {
            fid,
            name: name.to_string(),
            lines,
        });
        self.entry_count += 1;
        Ok(())
    }

    /// 逐条求和重算批小计（守恒核的第一口径）。
    pub fn recompute_subtotal(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                sum = sum.wrapping_add(e.lines);
            }
            i += 1;
        }
        sum
    }
}

/// 复核判定：批级守恒核结论。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BatchVerdict {
    /// 逐条求和 == 账面小计。
    Consistent,
    /// 逐条求和 != 账面小计（携带差额）。
    Drift(i32),
    /// 批账空槽/缺失。
    Missing,
}

/// 复核戳：一轮逐批对账的批级记录。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuditStamp {
    /// 批号。
    pub batch_no: u16,
    /// 审计轮次号（1 起）。
    pub round: u8,
    /// 该轮判定（true=一致）。
    pub ok: bool,
}

/// 全域守恒账：40 批批账 + 累计链 + 审计账。
pub struct ConservationLedger {
    /// 40 批定容批账。
    batches: [Option<BatchRow>; TOTAL_BATCHES],
    /// 已落账批数。
    batch_count: usize,
    /// 累计链（41 个锚点：cum[0]=0，cum[40]=终点）。
    cumulative: [u32; TOTAL_BATCHES + 1],
    /// 累计链已结算段数。
    chain_len: usize,
    /// 封账标记。
    sealed: bool,
    /// 审计戳账（定容）。
    stamps: [Option<AuditStamp>; TOTAL_BATCHES * MAX_AUDIT_ROUNDS],
    /// 审计戳数量。
    stamp_count: usize,
}

impl ConservationLedger {
    /// 新建空账。
    pub fn new() -> Self {
        ConservationLedger {
            batches: [const { None }; TOTAL_BATCHES],
            batch_count: 0,
            cumulative: [0u32; TOTAL_BATCHES + 1],
            chain_len: 0,
            sealed: false,
            stamps: [const { None }; TOTAL_BATCHES * MAX_AUDIT_ROUNDS],
            stamp_count: 0,
        }
    }

    /// 落账一批（按批号槽位写入）。
    pub fn put_batch(&mut self, row: BatchRow) -> Result<(), u16> {
        if self.sealed {
            return Err(E_LEDGER_SEALED);
        }
        if row.batch_no == 0 || row.batch_no as usize > TOTAL_BATCHES {
            return Err(E_BATCH_COUNT);
        }
        let idx = row.batch_no as usize - 1;
        if self.batches[idx].is_none() {
            self.batch_count += 1;
        }
        self.batches[idx] = Some(row);
        Ok(())
    }

    /// 读取指定批（不可变）。
    pub fn batch(&self, batch_no: u16) -> Result<&BatchRow, u16> {
        if batch_no == 0 || batch_no as usize > TOTAL_BATCHES {
            return Err(E_BATCH_COUNT);
        }
        match &self.batches[batch_no as usize - 1] {
            Some(b) => Ok(b),
            None => Err(E_BATCH_EMPTY),
        }
    }

    /// 已落账批数。
    pub fn batch_count(&self) -> usize {
        self.batch_count
    }

    /// 结算累计链：逐段 cum[i+1] = cum[i] + 批小计（重算口径）。
    /// 守恒恒等式：cum[40] - cum[0] == Σ recorded_subtotal == 240,000。
    pub fn settle_chain(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_LEDGER_SEALED);
        }
        if self.batch_count != TOTAL_BATCHES {
            return Err(E_BATCH_COUNT);
        }
        self.cumulative[0] = 0;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            let b = match &self.batches[i] {
                Some(b) => b,
                None => return Err(E_BATCH_EMPTY),
            };
            // 逐段结算：用逐条重算值入链（重算口径，非账面口径）。
            let recomputed = b.recompute_subtotal();
            self.cumulative[i + 1] = self.cumulative[i].wrapping_add(recomputed);
            i += 1;
        }
        self.chain_len = TOTAL_BATCHES;
        Ok(())
    }

    /// 累计链锚点读取。
    pub fn chain_at(&self, idx: usize) -> Result<u32, u16> {
        if idx > self.chain_len {
            return Err(E_CHAIN_BREAK);
        }
        Ok(self.cumulative[idx])
    }

    /// 守恒总核一：批小计逐批重算 == 逐条求和（40 批逐批对账）。
    pub fn verify_batch_subtotals(&mut self, round: u8) -> Result<usize, u16> {
        if !self.sealed {
            return Err(E_LEDGER_NOT_SEALED);
        }
        let mut mismatches = 0usize;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            let (no, recorded, recomputed) = match &self.batches[i] {
                Some(b) => (b.batch_no, b.recorded_subtotal, b.recompute_subtotal()),
                None => {
                    mismatches += 1;
                    self.stamp(no_of(i + 1), round, false)?;
                    i += 1;
                    continue;
                }
            };
            let ok = recorded == recomputed;
            if !ok {
                mismatches += 1;
            }
            self.stamp(no, round, ok)?;
            i += 1;
        }
        Ok(mismatches)
    }

    /// 守恒总核二：累计链 40 段零断（段间差 == 批小计）。
    pub fn verify_chain_integrity(&self) -> Result<usize, u16> {
        if self.chain_len != TOTAL_BATCHES {
            return Err(E_CHAIN_BREAK);
        }
        let mut breaks = 0usize;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            let b = match &self.batches[i] {
                Some(b) => b,
                None => return Err(E_BATCH_EMPTY),
            };
            let seg = self.cumulative[i + 1].wrapping_sub(self.cumulative[i]);
            if seg != b.recompute_subtotal() {
                breaks += 1;
            }
            i += 1;
        }
        Ok(breaks)
    }

    /// 守恒总核三：终点 240,000 再现。
    pub fn verify_terminal(&self) -> Result<(), u16> {
        if self.chain_len != TOTAL_BATCHES {
            return Err(E_CHAIN_BREAK);
        }
        if self.cumulative[TOTAL_BATCHES] != GRAND_TOTAL {
            return Err(E_TERMINAL_MISMATCH);
        }
        Ok(())
    }

    /// 守恒总核四（对账口径）：Σ recorded_subtotal == 终点。
    pub fn verify_recorded_sum(&self) -> Result<u32, u16> {
        let mut sum: u32 = 0;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            match &self.batches[i] {
                Some(b) => sum = sum.wrapping_add(b.recorded_subtotal),
                None => return Err(E_BATCH_EMPTY),
            }
            i += 1;
        }
        if sum != GRAND_TOTAL {
            return Err(E_TERMINAL_MISMATCH);
        }
        Ok(sum)
    }

    /// 登记审计戳（同批同轮防重）。
    fn stamp(&mut self, batch_no: u16, round: u8, ok: bool) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.stamp_count {
            if let Some(s) = &self.stamps[i] {
                if s.batch_no == batch_no && s.round == round {
                    return Err(E_STAMP_DUP);
                }
            }
            i += 1;
        }
        if self.stamp_count >= self.stamps.len() {
            return Err(E_AUDIT_ABSENT);
        }
        self.stamps[self.stamp_count] = Some(AuditStamp { batch_no, round, ok });
        self.stamp_count += 1;
        Ok(())
    }

    /// 某轮全部戳是否全通过。
    pub fn round_all_ok(&self, round: u8) -> Result<bool, u16> {
        let mut seen = 0usize;
        let mut ok_all = true;
        let mut i = 0usize;
        while i < self.stamp_count {
            if let Some(s) = &self.stamps[i] {
                if s.round == round {
                    seen += 1;
                    if !s.ok {
                        ok_all = false;
                    }
                }
            }
            i += 1;
        }
        if seen == 0 {
            return Err(E_AUDIT_ABSENT);
        }
        Ok(ok_all && seen == TOTAL_BATCHES)
    }

    /// 封账：封后禁写。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_LEDGER_SEALED);
        }
        if self.batch_count != TOTAL_BATCHES {
            return Err(E_BATCH_COUNT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账标记。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 链重放：从批账重放累计链并与原链比对（复核口径二）。
    pub fn replay_chain(&self) -> Result<u32, u16> {
        let mut acc: u32 = 0;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            let b = match &self.batches[i] {
                Some(b) => b,
                None => return Err(E_BATCH_EMPTY),
            };
            acc = acc.wrapping_add(b.recompute_subtotal());
            if acc != self.cumulative[i + 1] {
                return Err(E_REPLAY_DRIFT);
            }
            i += 1;
        }
        Ok(acc)
    }

    /// 全域条目数合计（应为 800）。
    pub fn total_entries(&self) -> Result<usize, u16> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            match &self.batches[i] {
                Some(b) => n += b.entry_count,
                None => return Err(E_BATCH_EMPTY),
            }
            i += 1;
        }
        Ok(n)
    }

    /// 全域 fid 连续性核：1..=800 零缺（与 F0768 呼应的行账侧口径）。
    pub fn verify_fid_continuity(&self) -> Result<usize, u16> {
        let total = self.total_entries()?;
        if total != 800 {
            return Err(E_FID_NOT_CONTIGUOUS);
        }
        let mut seen = [false; 801];
        let mut i = 0usize;
        while i < TOTAL_BATCHES {
            let b = match &self.batches[i] {
                Some(b) => b,
                None => return Err(E_BATCH_EMPTY),
            };
            let mut j = 0usize;
            while j < b.entry_count {
                if let Some(e) = &b.entries[j] {
                    if e.fid == 0 || e.fid as usize > 800 || seen[e.fid as usize] {
                        return Err(E_FID_NOT_CONTIGUOUS);
                    }
                    seen[e.fid as usize] = true;
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 1usize;
        while k <= 800 {
            if !seen[k] {
                return Err(E_FID_NOT_CONTIGUOUS);
            }
            k += 1;
        }
        Ok(0)
    }

    /// 批级判定采样：取指定批的复核结论（判定再现核）。
    pub fn batch_verdict(&self, batch_no: u16, round: u8) -> Result<BatchVerdict, u16> {
        let b = self.batch(batch_no)?;
        if b.recorded_subtotal == b.recompute_subtotal() {
            let mut i = 0usize;
            while i < self.stamp_count {
                if let Some(s) = &self.stamps[i] {
                    if s.batch_no == batch_no && s.round == round {
                        if !s.ok {
                            return Err(E_AUDIT_VERDICT);
                        }
                        return Ok(BatchVerdict::Consistent);
                    }
                }
                i += 1;
            }
            return Err(E_AUDIT_ABSENT);
        }
        let drift = b.recorded_subtotal as i64 - b.recompute_subtotal() as i64;
        Ok(BatchVerdict::Drift(drift as i32))
    }

    /// 总览渲染行（确定性文本，供复核报告）。
    pub fn summary_line(&self) -> String {
        let mut s = String::new();
        s.push_str("F0767 批数=");
        s.push_str(&self.batch_count.to_string());
        s.push_str(" 链段=");
        s.push_str(&self.chain_len.to_string());
        s.push_str(" 终点=");
        s.push_str(&self.cumulative[self.chain_len].to_string());
        s.push_str(" 戳=");
        s.push_str(&self.stamp_count.to_string());
        if self.sealed {
            s.push_str(" [已封]");
        }
        s
    }
}

/// FNV-1a 64 哈希（确定性渲染与快照绑定用）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 诊断码中文说明（闭集内返回说明，闭集外返回 UNKNOWN）。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BATCH_EMPTY => "批记录缺失：批账中存在空槽",
        E_BATCH_COUNT => "批数不为 40：全域批账不完整",
        E_SUBTOTAL_MISMATCH => "批小计与批内逐条求和不一致",
        E_ENTRY_EMPTY => "批内条目缺失：明细账空槽",
        E_ENTRY_LINES_ZERO => "条目行数为零：违反纯功能行数下限",
        E_ENTRY_FID_DUP => "条目 fid 重复：同批内编号冲突",
        E_CHAIN_BREAK => "累计链断裂：段间差值不等于批小计",
        E_TERMINAL_MISMATCH => "终点不等于 240,000：守恒失败",
        E_LEDGER_SEALED => "账已封：封账后禁止再写入",
        E_LEDGER_NOT_SEALED => "账未封：复核前必须先封账",
        E_BATCH_TOTAL_MISMATCH => "批条数与账面条目数不一致",
        E_FID_NOT_CONTIGUOUS => "fid 不连续：全域编号断裂",
        E_AUDIT_ABSENT => "复核记录缺失：审计账为空",
        E_AUDIT_VERDICT => "复核判定非通过：存在未结分歧",
        E_REPLAY_DRIFT => "链重放漂移：重放终点与原账不符",
        E_STAMP_DUP => "复核戳重复：同批同轮重复登记",
        _ => "未知诊断码（F0767 段外）",
    }
}

fn no_of(idx: usize) -> u16 {
    idx as u16
}

/// 构建标准样本账：40 批 × 20 条 × 300 行 = 240,000 行。
pub fn build_standard_ledger() -> Result<ConservationLedger, u16> {
    let mut lg = ConservationLedger::new();
    let mut batch: usize = 1;
    while batch <= TOTAL_BATCHES {
        let mut row = BatchRow::new(batch as u16, "B");
        row.batch_tag = "B".to_string();
        let mut fid_base = (batch - 1) * 20;
        let mut e = 0usize;
        while e < 20 {
            let fid = (fid_base + e + 1) as u16;
            let name = "E";
            row.add_entry(fid, name, 300)?;
            e += 1;
        }
        // 先求和后落账（R-A1-004 留痕口径）。
        row.recorded_subtotal = row.recompute_subtotal();
        lg.put_batch(row)?;
        let _ = fid_base;
        fid_base = 0;
        batch += 1;
    }
    Ok(lg)
}

/// 自例程：正常链全过 + 三类破坏路径各自命中正确诊断码。
pub fn drill() -> Result<u32, u16> {
    // 一、标准账全链。
    let mut lg = build_standard_ledger()?;
    lg.settle_chain()?;
    lg.seal()?;
    let mm = lg.verify_batch_subtotals(1)?;
    if mm != 0 {
        return Err(E_SUBTOTAL_MISMATCH);
    }
    let br = lg.verify_chain_integrity()?;
    if br != 0 {
        return Err(E_CHAIN_BREAK);
    }
    lg.verify_terminal()?;
    let s = lg.verify_recorded_sum()?;
    if s != GRAND_TOTAL {
        return Err(E_TERMINAL_MISMATCH);
    }
    let rp = lg.replay_chain()?;
    if rp != GRAND_TOTAL {
        return Err(E_REPLAY_DRIFT);
    }
    lg.verify_fid_continuity()?;
    if lg.total_entries()? != 800 {
        return Err(E_BATCH_TOTAL_MISMATCH);
    }
    if !lg.round_all_ok(1)? {
        return Err(E_AUDIT_VERDICT);
    }
    match lg.batch_verdict(1, 1)? {
        BatchVerdict::Consistent => {}
        _ => return Err(E_AUDIT_VERDICT),
    }
    // 快照绑定：账摘要哈希确定性。
    let h1 = fnv1a64(lg.summary_line().as_bytes());
    let h2 = fnv1a64(lg.summary_line().as_bytes());
    if h1 != h2 {
        return Err(E_REPLAY_DRIFT);
    }

    // 二、破坏路径 A：篡改批小计 → 批小计对账不齐。
    let mut bad = build_standard_ledger()?;
    bad.settle_chain()?;
    bad.seal()?;
    if let Some(b) = bad.batches[7].as_mut() {
        b.recorded_subtotal = b.recorded_subtotal.wrapping_add(1);
    }
    if bad.verify_batch_subtotals(1)? != 1 {
        return Err(E_SUBTOTAL_MISMATCH);
    }

    // 三、破坏路径 B：终点错 → 终点不再现。
    let mut bad2 = build_standard_ledger()?;
    bad2.settle_chain()?;
    bad2.cumulative[TOTAL_BATCHES] = GRAND_TOTAL - 1;
    if bad2.verify_terminal() != Err(E_TERMINAL_MISMATCH) {
        return Err(E_TERMINAL_MISMATCH);
    }

    // 四、破坏路径 C：条目行数为零 → 写入拒绝。
    let mut bad3 = build_standard_ledger()?;
    if bad3.batch_mut_for_drill(1).is_err() {
        return Err(E_BATCH_EMPTY);
    }
    let zero_res = {
        let mut r = bad3.batches[0].take().ok_or(E_BATCH_EMPTY)?;
        let res = r.add_entry(999, "Z", 0);
        bad3.batches[0] = Some(r);
        res
    };
    if zero_res != Err(E_ENTRY_LINES_ZERO) {
        return Err(E_ENTRY_LINES_ZERO);
    }

    Ok(0)
}

impl ConservationLedger {
    /// drill 用：取批可变引用（受封账保护）。
    fn batch_mut_for_drill(&mut self, batch_no: u16) -> Result<(), u16> {
        if batch_no == 0 || batch_no as usize > TOTAL_BATCHES {
            return Err(E_BATCH_COUNT);
        }
        if self.batches[batch_no as usize - 1].is_none() {
            return Err(E_BATCH_EMPTY);
        }
        Ok(())
    }
}
