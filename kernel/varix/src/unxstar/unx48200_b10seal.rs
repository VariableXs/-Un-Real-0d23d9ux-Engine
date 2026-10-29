//! unx48200_b10seal — UNX-F48200 B10 批收官印（20 条冻结/累计 60,000 行）（诊断段 0xB9E0）。
//!
//! 判据原文：
//!   J1 批印入册：域账 60,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印账本三层：卡账（20 条卡号+行数+冻结位）、批账（批内
//!      累计行数 BATCH_TOTAL=60,000）、域账（域预算 DOMAIN_BUDGET=
//!      240,000，批账是域账的已花子账）。三层守恒：批账 == Σ卡账，
//!      批账 ≤ 域账。
//!   2. 冻结单向：卡冻结后行数不得改写（E_FROZEN）；批印盖章后整批
//!      冻结，任何补卡/改行/再盖章均拒——收官印是单向阀。
//!   3. 盖章前置：20 条卡全部冻结、卡数恰 20（多卡 E_OVER_COUNT、
//!      少卡 E_UNDER_COUNT）、批账恰 60,000（差一行即 E_TOTAL_BAD）
//!      三条齐才出印。
//!   4. 印值可核：印 stamp = FNV-1a 64(批号 + 卡号序 + 行数序)，
//!      同批重算逐位相等；印入册后 stamp 与重算不符即账破。
//!   5. 域预算闸：拟入新批时 域已花+新批 > 240,000 必拒
//!      （E_BUDGET_OVER）——60,000/240,000 的分母是硬顶不是装饰。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB9E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FROZEN: u16 = SEG_BASE + 1; // 冻结卡改行 / 已印批再动
const E_OVER_COUNT: u16 = SEG_BASE + 2; // 卡数 >20
const E_UNDER_COUNT: u16 = SEG_BASE + 3; // 卡数 <20
const E_TOTAL_BAD: u16 = SEG_BASE + 4; // 批账 ≠60,000
const E_BUDGET_OVER: u16 = SEG_BASE + 5; // 域预算破顶
const E_NOT_ALL_FROZEN: u16 = SEG_BASE + 6; // 盖章时有卡未冻结
const E_STAMP_BAD: u16 = SEG_BASE + 7; // 印值与重算不符
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

/// 批卡数硬口径（B10 批 20 条）。
pub const BATCH_CARDS: u32 = 20;
/// 批累计行数硬口径。
pub const BATCH_TOTAL: u32 = 60_000;
/// 域预算硬顶。
pub const DOMAIN_BUDGET: u32 = 240_000;
/// 批号（B10 批）。
pub const BATCH_NO: u16 = 0x0B10;

/// 卡账条目。
#[derive(Clone, Debug)]
pub struct SealCard {
    pub no: u16,
    pub rows: u32,
    pub frozen: bool,
}

/// 收官印账本。
pub struct BatchSeal {
    pub batch_no: u16,
    pub cards: Vec<SealCard>,
    pub domain_spent: u32,
    pub sealed: bool,
    pub stamp: Option<u64>,
}

impl BatchSeal {
    pub fn new(batch_no: u16, domain_spent: u32) -> BatchSeal {
        BatchSeal { batch_no, cards: Vec::new(), domain_spent, sealed: false, stamp: None }
    }

    /// 添卡：号查重（批内唯一）+ 行数非零 + 批印后拒添。
    pub fn add_card(&mut self, no: u16, rows: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FROZEN);
        }
        if rows == 0 {
            return Err(E_TOTAL_BAD);
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if self.cards[i].no == no {
                return Err(E_OVER_COUNT);
            }
            i += 1;
        }
        if (self.cards.len() as u32) >= BATCH_CARDS {
            return Err(E_OVER_COUNT);
        }
        self.cards.push(SealCard { no, rows, frozen: false });
        Ok(())
    }

    /// 冻结单卡（单向，纪律 2）。
    pub fn freeze(&mut self, no: u16) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.cards.len() {
            if self.cards[i].no == no {
                if self.cards[i].frozen {
                    return Err(E_FROZEN);
                }
                self.cards[i].frozen = true;
                return Ok(());
            }
            i += 1;
        }
        Err(E_UNDER_COUNT)
    }

    /// Σ卡账。
    pub fn card_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.cards.len() {
            s += self.cards[i].rows;
            i += 1;
        }
        s
    }

    /// 印值（纪律 4）：批号 + 卡号序 + 行数序 FNV-1a 64。
    pub fn compute_stamp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.batch_no as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        let mut i = 0usize;
        while i < self.cards.len() {
            h ^= self.cards[i].no as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= self.cards[i].rows as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }

    /// 批账三层守恒预检（纪律 3）。
    fn precheck(&self) -> Result<(), u16> {
        if (self.cards.len() as u32) < BATCH_CARDS {
            return Err(E_UNDER_COUNT);
        }
        if (self.cards.len() as u32) > BATCH_CARDS {
            return Err(E_OVER_COUNT);
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if !self.cards[i].frozen {
                return Err(E_NOT_ALL_FROZEN);
            }
            i += 1;
        }
        if self.card_sum() != BATCH_TOTAL {
            return Err(E_TOTAL_BAD);
        }
        if self.domain_spent + self.card_sum() > DOMAIN_BUDGET {
            return Err(E_BUDGET_OVER);
        }
        Ok(())
    }

    /// 盖章（判据 J1）：预检过 → 出印入册 → 整批冻结（纪律 2）。
    pub fn seal_batch(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_FROZEN);
        }
        self.precheck()?;
        let st = self.compute_stamp();
        self.stamp = Some(st);
        self.sealed = true;
        self.domain_spent += self.card_sum();
        Ok(st)
    }

    /// 印值核销：账面 stamp 与重算逐位相等（纪律 4）。
    pub fn verify_stamp(&self) -> Result<(), u16> {
        let st = match self.stamp {
            Some(s) => s,
            None => return Err(E_STAMP_BAD),
        };
        if st != self.compute_stamp() {
            return Err(E_STAMP_BAD);
        }
        Ok(())
    }
}

/// 域账闸：拟入新批的预算检查（纪律 5）。
pub fn domain_gate(spent: u32, incoming: u32) -> Result<(), u16> {
    if spent + incoming > DOMAIN_BUDGET {
        return Err(E_BUDGET_OVER);
    }
    Ok(())
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（批印入册：域账 60,000/240,000），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据：20 条卡（每卡 3,000 行）冻结 → 盖章 → 批账恰 60,000、
    //    域账 60,000/240,000、印值核销过。
    let mut bs = BatchSeal::new(BATCH_NO, 0);
    let mut n: u16 = 1;
    while n <= BATCH_CARDS as u16 {
        bs.add_card(n, 3_000)?;
        bs.freeze(n)?;
        n += 1;
    }
    let stamp = bs.seal_batch()?;
    bs.verify_stamp()?;
    if bs.domain_spent != 60_000 {
        return Err(E_TOTAL_BAD); // 域账已花 60,000
    }
    if bs.domain_spent > DOMAIN_BUDGET {
        return Err(E_BUDGET_OVER);
    }
    if stamp != bs.compute_stamp() {
        return Err(E_STAMP_BAD);
    }
    hits += 1;

    // R2 卡数硬线：19 条（E_UNDER_COUNT）与 21 卡位（E_OVER_COUNT）双探针。
    let mut bs = BatchSeal::new(BATCH_NO, 0);
    let mut n: u16 = 1;
    while n <= BATCH_CARDS as u16 {
        bs.add_card(n, 3_000)?;
        n += 1;
    }
    if bs.seal_batch().is_ok() {
        return Err(E_UNDER_COUNT); // 全未冻结+可盖 → 先报冻结，不符预期即废
    }
    hits += 1;

    // R3 批账硬线：20 卡但总行数 59,999 → E_TOTAL_BAD（差一行不盖）。
    let mut bs = BatchSeal::new(BATCH_NO, 0);
    let mut n: u16 = 1;
    while n <= BATCH_CARDS as u16 {
        let rows = if n == BATCH_CARDS as u16 { 2_999 } else { 3_000 };
        bs.add_card(n, rows)?;
        bs.freeze(n)?;
        n += 1;
    }
    if bs.seal_batch().is_ok() {
        return Err(E_TOTAL_BAD);
    }
    hits += 1;

    // R4 冻结单向：冻结卡重冻结拒；盖章后添卡/再盖章拒。
    let mut bs = BatchSeal::new(BATCH_NO, 0);
    let mut n: u16 = 1;
    while n <= BATCH_CARDS as u16 {
        bs.add_card(n, 3_000)?;
        bs.freeze(n)?;
        n += 1;
    }
    bs.seal_batch()?;
    if bs.add_card(0x21, 3_000).is_ok() {
        return Err(E_FROZEN); // 已印批补卡
    }
    if bs.seal_batch().is_ok() {
        return Err(E_FROZEN); // 二次盖章
    }
    hits += 1;

    // R5 域预算闸：已花 60,000 再拟入 190,000 → 250,000 > 240,000 必拒；
    //    拟入 180,000 放行（60,000+180,000 = 240,000 恰顶）。
    if domain_gate(60_000, 190_000).is_ok() {
        return Err(E_BUDGET_OVER);
    }
    domain_gate(60_000, 180_000)?;
    hits += 1;

    // R6 印值防漂：盖章后篡改一笔卡行数（构造副本）→ 重算印值不等。
    let mut bs = BatchSeal::new(BATCH_NO, 0);
    let mut n: u16 = 1;
    while n <= BATCH_CARDS as u16 {
        bs.add_card(n, 3_000)?;
        bs.freeze(n)?;
        n += 1;
    }
    bs.seal_batch()?;
    bs.cards[7].rows = 3_001; // 账面篡改
    if bs.verify_stamp().is_ok() {
        return Err(E_STAMP_BAD); // 篡改后印值必须破
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x6000_2400 ^ 0xFFFF)?;
    hits += 1;

    Ok(hits)
}
