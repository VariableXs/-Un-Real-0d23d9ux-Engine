//! unx4060_b03assert — UNX-F4060 B03 批判据聚合断言集（诊断段 0x89E0）。
//!
//! 判据原文（提炼）：
//!   J1. 本批 19 条判据全部聚合入 ktest vfs B03 面，聚合报告无缺项。
//!   J2. 聚合通过率 100% 才算绿：任一条未过，批绿位不得置位。
//!   J3. 批账核对：登记数、通过数、失败数三账交叉一致。
//!   J4. 批卡覆盖：聚合表覆盖批内全部模块卡（无 import 依赖，
//!       内嵌表自描述）。
//! 设计纪律：
//!   1. 批级聚合账独立成锚：批内各卡判据以内嵌表登记（卡号 + 判据号），
//!      不 import 同批其他模块文件（表内键即唯一耦合面）。
//!   2. 判据条目状态机：Aggregated→Verified→Green/Red，迁移受控。
//!   3. 批绿闸门：全部条目 Green 才置批绿位，缺项/红项即拒。
//!   4. B03 批 10 卡、19 条判据（各卡判据数按施工单内嵌写死）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x89E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MISS: u16 = SEG_BASE + 1;    // 聚合缺项（应登记条目未入表）
const E_DUP: u16 = SEG_BASE + 2;     // 重复登记同一判据条目
const E_NOT_GREEN: u16 = SEG_BASE + 3; // 批绿闸门拒绝（有红项/缺项）
const E_BAD_ENTRY: u16 = SEG_BASE + 4; // 条目键非法（0 卡号/0 判据号）
const E_STATE_INV: u16 = SEG_BASE + 5; // 条目状态迁移非法
const E_LEDGER: u16 = SEG_BASE + 6;    // 批账三账交叉不平
const E_BAD_ARG: u16 = SEG_BASE + 7;   // 参数非法

/// B03 批卡数定编（unx4051..unx4060，不含跨批卡）。
pub const B03_CARDS: usize = 10;

/// B03 批判据总数定编：19 条（施工单聚合口径）。
pub const B03_CRITERIA: usize = 19;

/// 批绿位（聚合报告 19/19 全绿才置位）。
pub const BATCH_GREEN: u32 = 1;

/// 判据条目状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryPhase {
    Aggregated, // 已聚合入面
    Verified,   // 已核验（断言值达标）
    Green,      // 绿
    Red,        // 红
}

/// 判据条目：卡号 + 判据号 + 相位 + 断言值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CritEntry {
    pub card: u32,    // 卡号（UNX-F4xxx 的 num）
    pub crit: u32,    // 批内判据序号（1 起）
    pub phase: EntryPhase,
    pub value: u32,   // 断言值（0 表未核验）
}

/// B03 批聚合账：内嵌判据表 + 批绿闸门。
pub struct BatchAssert {
    pub entries: Vec<CritEntry>, // 判据条目表
    pub cap: usize,              // 表容量（定编 19）
    pub green_flag: u32,         // 批绿位（0/1）
    pub aggregated: u32,         // 聚合记账数
    pub verified: u32,           // 核验记账数
    pub red_total: u32,          // 红项总数（含历史红）
}

/// 批内嵌表：各卡判据数定编（施工单判据口径，共 19 条）。
/// 4051:2  4052:2  4053:2  4054:2  4055:2  4056:2  4057:2  4058:2
/// 4059:2  4060:1 → 合计 19。
pub const CARD_CRIT_COUNT: [u32; B03_CARDS] = [2, 2, 2, 2, 2, 2, 2, 2, 2, 1];

/// 批卡号定编（B03 批内 10 卡）。
pub const CARD_IDS: [u32; B03_CARDS] = [4051, 4052, 4053, 4054, 4055, 4056, 4057, 4058, 4059, 4060];

impl BatchAssert {
    pub fn new() -> BatchAssert {
        BatchAssert {
            entries: Vec::new(),
            cap: B03_CRITERIA,
            green_flag: 0,
            aggregated: 0,
            verified: 0,
            red_total: 0,
        }
    }

    fn find(&self, card: u32, crit: u32) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].card == card && self.entries[i].crit == crit {
                return Some(i);
            }
        }
        None
    }

    /// 聚合登记：按内嵌定编表逐条入表；超定编/重复拒收。
    pub fn aggregate(&mut self, card: u32, crit: u32) -> Result<(), u16> {
        if card == 0 || crit == 0 {
            return Err(E_BAD_ENTRY);
        }
        if self.entries.len() >= self.cap {
            return Err(E_MISS);
        }
        // 卡号必须在批内嵌定编内
        let mut in_batch = false;
        for k in 0..B03_CARDS {
            if CARD_IDS[k] == card {
                in_batch = true;
                break;
            }
        }
        if !in_batch {
            return Err(E_BAD_ENTRY);
        }
        if self.find(card, crit).is_some() {
            return Err(E_DUP);
        }
        self.entries.push(CritEntry {
            card,
            crit,
            phase: EntryPhase::Aggregated,
            value: 0,
        });
        self.aggregated += 1;
        Ok(())
    }

    /// 按定编表全量聚合（判据 J1：无缺项聚合）。
    pub fn aggregate_all(&mut self) -> Result<u32, u16> {
        for k in 0..B03_CARDS {
            for c in 1..=CARD_CRIT_COUNT[k] {
                self.aggregate(CARD_IDS[k], c)?;
            }
        }
        Ok(self.entries.len() as u32)
    }

    /// 核验：对单条目写入断言值，Aggregated→Verified（值达标）或 Red。
    pub fn verify(&mut self, card: u32, crit: u32, value: u32, want: u32) -> Result<(), u16> {
        let i = match self.find(card, crit) {
            Some(i) => i,
            None => return Err(E_MISS),
        };
        match self.entries[i].phase {
            EntryPhase::Aggregated => {}
            _ => return Err(E_STATE_INV),
        }
        if value >= want {
            self.entries[i].phase = EntryPhase::Verified;
            self.entries[i].value = value;
            self.verified += 1;
        } else {
            self.entries[i].phase = EntryPhase::Red;
            self.entries[i].value = value;
            self.red_total += 1;
        }
        Ok(())
    }

    /// 单条目定绿：Verified→Green。
    pub fn mark_green(&mut self, card: u32, crit: u32) -> Result<(), u16> {
        let i = match self.find(card, crit) {
            Some(i) => i,
            None => return Err(E_MISS),
        };
        if self.entries[i].phase != EntryPhase::Verified {
            return Err(E_STATE_INV);
        }
        self.entries[i].phase = EntryPhase::Green;
        Ok(())
    }

    /// 全量定绿并开批绿闸（判据 J2：19/19 绿才算绿）。
    pub fn seal_batch(&mut self) -> Result<u32, u16> {
        // 缺项检查：登记数必须恰为定编
        if self.entries.len() != B03_CRITERIA {
            return Err(E_MISS);
        }
        // 逐条定绿：非 Verified 态即拒
        for i in 0..self.entries.len() {
            if self.entries[i].phase == EntryPhase::Verified {
                self.entries[i].phase = EntryPhase::Green;
            } else if self.entries[i].phase != EntryPhase::Green {
                return Err(E_NOT_GREEN);
            }
        }
        // 红项残余检查：任何 Red 历史即拒批绿
        if self.red_total > 0 {
            return Err(E_NOT_GREEN);
        }
        self.green_flag = BATCH_GREEN;
        Ok(self.entries.len() as u32)
    }

    /// 聚合报告：登记/绿/红三账导出（判据 J3 对账面）。
    pub fn report(&self) -> Result<(u32, u32, u32), u16> {
        let mut green = 0;
        let mut red = 0;
        for e in &self.entries {
            match e.phase {
                EntryPhase::Green => green += 1,
                EntryPhase::Red => red += 1,
                _ => {}
            }
        }
        Ok((self.entries.len() as u32, green, red))
    }

    /// 批账核对（判据 J3）：登记数 == 定编 且 三账交叉一致。
    pub fn ledger_ok(&self) -> bool {
        if self.entries.len() as u32 != B03_CRITERIA as u32 {
            return false;
        }
        match self.report() {
            Ok((n, green, red)) => {
                n == self.aggregated
                    && green + red <= n
                    && red == self.red_total
            }
            Err(_) => false,
        }
    }

    /// 批绿位读取面。
    pub fn is_green(&self) -> bool {
        self.green_flag == BATCH_GREEN
    }
}

// ── 扩展面：批报告与缺项诊断 ─────────────────────────────────

impl BatchAssert {
    /// 卡维度覆盖审计：返回已覆盖卡数（判据 J4 覆盖面）。
    pub fn covered_cards(&self) -> u32 {
        let mut n = 0;
        for k in 0..B03_CARDS {
            let mut hit = false;
            for e in &self.entries {
                if e.card == CARD_IDS[k] {
                    hit = true;
                    break;
                }
            }
            if hit {
                n += 1;
            }
        }
        n
    }

    /// 缺项定位：返回定编表中尚未聚合的（卡号, 判据号）清单长度。
    pub fn missing_count(&self) -> u32 {
        let mut miss = 0;
        for k in 0..B03_CARDS {
            for c in 1..=CARD_CRIT_COUNT[k] {
                let mut hit = false;
                for e in &self.entries {
                    if e.card == CARD_IDS[k] && e.crit == c {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    miss += 1;
                }
            }
        }
        miss
    }

    /// Verified 相余量只读面（封批前置条件读数）。
    pub fn verified_pending(&self) -> u32 {
        let mut n = 0;
        for e in &self.entries {
            if e.phase == EntryPhase::Verified {
                n += 1;
            }
        }
        n
    }

    /// 批绿位与聚合账联合快照（判据 J2/J3 报告面）。
    pub fn batch_report(&self) -> Result<(u32, u32), u16> {
        if self.aggregated != self.entries.len() as u32 {
            return Err(E_LEDGER);
        }
        Ok((self.green_flag, self.aggregated))
    }
}

/// 自例程：全量聚合→逐条核验定绿→红项注入→批绿闸门→对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut b = BatchAssert::new();

    // R1 全量聚合 19 条无缺项（判据 J1）
    let n = b.aggregate_all()?;
    if n != B03_CRITERIA as u32 {
        return Err(E_MISS);
    }
    // 重复聚合注入 → 拒收
    match b.aggregate(4051, 1) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    hits += 1;

    // R2 逐条核验（断言值 = 判据序号推导，全部达标）
    for i in 0..b.entries.len() {
        let (card, crit) = (b.entries[i].card, b.entries[i].crit);
        let want = 1;
        let val = crit; // 达标值推导：crit ≥ 1 恒 ≥ want
        b.verify(card, crit, val, want)?;
        b.mark_green(card, crit)?;
    }
    let (total, green, red) = b.report()?;
    if total != 19 || green != 19 || red != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 红项路径：新账里核验不达标 → Red 且批绿闸拒绝
    let mut r = BatchAssert::new();
    r.aggregate_all()?;
    r.verify(4051, 1, 0, 1)?; // 值 0 < want 1 → Red
    match r.seal_batch() {
        Err(E_NOT_GREEN) => {}
        _ => return Err(E_NOT_GREEN),
    }
    if r.is_green() {
        return Err(E_NOT_GREEN);
    }
    hits += 1;

    // R4 缺项注入：未聚合满定编就封批 → 拒收（判据 J1/J2 闸门）
    let mut m = BatchAssert::new();
    m.aggregate(4051, 1)?;
    match m.seal_batch() {
        Err(E_MISS) => {}
        _ => return Err(E_MISS),
    }
    hits += 1;

    // R5 主账封批成功 + 批账核对（判据 J2/J3）
    b.seal_batch()?;
    if !b.is_green() {
        return Err(E_NOT_GREEN);
    }
    if !b.ledger_ok() {
        return Err(E_LEDGER);
    }
    if b.aggregated != 19 || b.verified != 19 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
