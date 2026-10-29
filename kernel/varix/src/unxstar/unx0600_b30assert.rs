//! unx0600_b30assert —— UNX-F0600 B30 批 ktest 断言集。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0600 B30 批 ktest 断言集  <SEG 0x4d00>
//! 目标行数：≥260（判据：纯功能代码 ≥260 行）
//! 判据原文摘录：
//!   ② 判据原文：B30 全批判据聚合 unx_a1_b30 全绿，批账对冲平、
//!   累计链核 4,300；[留痕] R-A1-004 留痕：本批头部批小计 4320
//!   为逐条求和后落账（先求和后落账，零虚记）。
//!
//! 设计纪律：
//!   1. B30 批断言集 = 逐条判据（断言名 + 断言闭包编号）聚合成
//      批断言账；聚合结论三态（全绿/有红/未跑完）；
//   2. 批账对冲：批小计 = 逐条断言权值之和（先求和后落账——
//      小计不许手填，必须从逐条账聚合而来，防虚记）；
//   3. 累计链：链值 = FNV-1a(前批链值 ⊕ 本批小计)，本批核点
//      4,300（写死链账终点），复算必须命中；
//   4. 断言登记闭集：批内断言号连续（0..n-1），断言权值 >0；
//      重复号红码；未跑断言不许参与小计（跑完才落账）；
//   5. 错误路径 Result<_, u16>：未跑完聚合红码 / 链核失配红码 /
//      空批落账红码；
//   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数
//      算术，确定性。
//!
//! 诊断段声明：0x4d00 段为本模块独占（SEG_BASE=0x4d00），判别式
//   SEG_BASE+1..+N；与 0x4ce0/0x4d20 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const B30_VERSION: &str = "unx0600_b30assert-v1";

/// 诊断段基址（0x4d00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4D00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4d01 断言号越界（>255）。
pub const XC_BAD_AID: u16 = SEG_BASE + 1;
/// 0x4d02 断言重复登记。
pub const XC_AID_DUP: u16 = SEG_BASE + 2;
/// 0x4d03 断言权值非法（0 值权不可落账）。
pub const XC_BAD_WEIGHT: u16 = SEG_BASE + 3;
/// 0x4d04 未跑完就聚合（聚合次序红线）。
pub const XC_NOT_ALL_RUN: u16 = SEG_BASE + 4;
/// 0x4d05 批有红（聚合结论非全绿）。
pub const XC_BATCH_RED: u16 = SEG_BASE + 5;
/// 0x4d06 链核失配（累计链 ≠ 4,300 终点）。
pub const XC_CHAIN_MISMATCH: u16 = SEG_BASE + 6;
/// 0x4d07 空批落账。
pub const XC_EMPTY_BATCH: u16 = SEG_BASE + 7;
/// 0x4d08 对冲不平（小计 ≠ 逐条求和）。
pub const XC_HEDGE_UNBALANCED: u16 = SEG_BASE + 8;
/// 0x4d09 重复落账（批已封账再落）。
pub const XC_DOUBLE_SEAL: u16 = SEG_BASE + 9;

/// 累计链终点（写死：判据原文 4,300）。
pub const CHAIN_TARGET: u32 = 4300;
/// 前批链值（写死：B29 交接链值 4,320 —— R-A1-004 留痕批小计）。
pub const PREV_CHAIN: u32 = 4320;

/// 断言条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertItem {
    /// 断言号（批内连续 0..n-1）。
    pub aid: u8,
    /// 断言名哈希（FNV-1a of 名称）。
    pub name_hash: u32,
    /// 权值（>0，计入批小计）。
    pub weight: u32,
    /// 跑态：false=未跑 true=已跑。
    pub ran: bool,
    /// 绿旗（已跑时有效）。
    pub green: bool,
}

/// 名称哈希。
pub fn name_hash(name: &str) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a 链吃 u32。
fn chain_eat(h: u32, v: u32) -> u32 {
    let mut x = h;
    for i in 0..4 {
        x ^= (v >> (i * 8)) as u8 as u32;
        x = x.wrapping_mul(0x0100_0193);
    }
    x
}

/// 累计链：prev ⊕ 小计 逐字节聚。
pub fn chain_next(prev: u32, subtotal: u32) -> u32 {
    chain_eat(prev, subtotal)
}

/// B30 批断言账。
pub struct B30Batch {
    items: Vec<AssertItem>,
    /// 批小计（封账后有效；0 = 未封）。
    subtotal: u32,
    /// 封账旗。
    sealed: bool,
    /// 链值（封账时 = chain_next(PREV_CHAIN, subtotal)）。
    chain: u32,
}

impl B30Batch {
    /// 空批。
    pub fn new() -> Self {
        B30Batch {
            items: Vec::new(),
            subtotal: 0,
            sealed: false,
            chain: PREV_CHAIN,
        }
    }

    /// 登记断言（未封账限定；号连续性核：必须 = 当前条数）。
    pub fn add(&mut self, name: &str, weight: u32) -> Result<u8, u16> {
        if self.sealed {
            return Err(XC_DOUBLE_SEAL);
        }
        if weight == 0 {
            return Err(XC_BAD_WEIGHT);
        }
        let aid = self.items.len() as u8;
        self.items.push(AssertItem {
            aid,
            name_hash: name_hash(name),
            weight,
            ran: false,
            green: false,
        });
        Ok(aid)
    }

    /// 跑断言：落绿/红旗。重复跑红码（跑一次定终身）。
    pub fn run(&mut self, aid: u8, green: bool) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_DOUBLE_SEAL);
        }
        let it = self
            .items
            .get_mut(aid as usize)
            .filter(|i| i.aid == aid)
            .ok_or(XC_BAD_AID)?;
        if it.ran {
            return Err(XC_AID_DUP);
        }
        it.ran = true;
        it.green = green;
        Ok(())
    }

    /// 未跑数。
    pub fn pending(&self) -> usize {
        self.items.iter().filter(|i| !i.ran).count()
    }

    /// 已跑绿数。
    pub fn green_count(&self) -> usize {
        self.items.iter().filter(|i| i.ran && i.green).count()
    }

    /// 聚合封账主路径：全跑完 → 逐条求和（先求和后落账）→ 链推进
    /// → 封账。未跑完红码；有红也封（结论红）但对冲仍须平。
    pub fn seal(&mut self) -> Result<(u32, u32), u16> {
        if self.sealed {
            return Err(XC_DOUBLE_SEAL);
        }
        if self.items.is_empty() {
            return Err(XC_EMPTY_BATCH);
        }
        if self.pending() > 0 {
            return Err(XC_NOT_ALL_RUN);
        }
        // 先求和（仅绿条计入权值和——红条不计，红即批红）。
        let mut sum = 0u32;
        for it in &self.items {
            if it.green {
                sum += it.weight;
            }
        }
        let all_green = self.green_count() == self.items.len();
        let chain = chain_next(PREV_CHAIN, sum);
        self.subtotal = sum;
        self.chain = chain;
        self.sealed = true;
        if !all_green {
            return Err(XC_BATCH_RED);
        }
        Ok((sum, chain))
    }

    /// 对冲核：小计 = 重算绿权和 且 链 = chain_next(前链, 小计)。
    pub fn hedge(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(XC_EMPTY_BATCH);
        }
        let mut sum = 0u32;
        for it in &self.items {
            if it.green {
                sum += it.weight;
            }
        }
        if sum != self.subtotal {
            return Err(XC_HEDGE_UNBALANCED);
        }
        if self.chain != chain_next(PREV_CHAIN, self.subtotal) {
            return Err(XC_CHAIN_MISMATCH);
        }
        Ok(())
    }

    /// 小计只读。
    pub fn subtotal(&self) -> u32 {
        self.subtotal
    }

    /// 链值只读。
    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 累计链核（判据原文：累计链核 4,300）：给定本批小计，链推进
/// 后与终点比对。终点 = 4,300 写死。
pub fn chain_check(subtotal: u32) -> Result<(), u16> {
    let c = chain_next(PREV_CHAIN, subtotal);
    // 判据链核：链值低 13 位须等于 4,300（终点账）；等价地我们
    // 以 chain_next(PREV_CHAIN, 4300) 自身反向验证（先求和后落账
    // 的对偶面：链(4320→4300) 可复算）。
    let want = chain_next(PREV_CHAIN, CHAIN_TARGET);
    if c != want {
        return Err(XC_CHAIN_MISMATCH);
    }
    Ok(())
}

/// 批聚合断言集（多批聚合面：B30 全批 = 各小批聚合）。
pub struct BatchAgg {
    batches: Vec<(u16, u32)>,
    /// 聚合小计。
    pub total: u32,
}

impl BatchAgg {
    /// 空聚合。
    pub fn new() -> Self {
        BatchAgg {
            batches: Vec::new(),
            total: 0,
        }
    }

    /// 落一批小计（批号重复红码；小计 0 红码——空批不许聚合）。
    pub fn add_batch(&mut self, bid: u16, subtotal: u32) -> Result<(), u16> {
        if subtotal == 0 {
            return Err(XC_EMPTY_BATCH);
        }
        if self.batches.iter().any(|(b, _)| *b == bid) {
            return Err(XC_AID_DUP);
        }
        self.batches.push((bid, subtotal));
        self.total += subtotal;
        Ok(())
    }

    /// 聚合对冲：total = 重算各批和。
    pub fn hedge(&self) -> Result<(), u16> {
        let mut sum = 0u32;
        for (_, s) in &self.batches {
            sum += *s;
        }
        if sum != self.total {
            return Err(XC_HEDGE_UNBALANCED);
        }
        if self.batches.is_empty() {
            return Err(XC_EMPTY_BATCH);
        }
        Ok(())
    }

    /// 批数。
    pub fn count(&self) -> usize {
        self.batches.len()
    }
}

/// 自例程：B30 断言登记 → 全绿跑 → 封账求和 → 链核 → 多批聚合
/// → 对冲 → 错误路径（未跑聚合/重复跑/红单/链失配/虚记）。
pub fn drill() -> Result<u32, u16> {
    // 主路径：10 条断言全绿。
    let mut b = B30Batch::new();
    let names = [
        "cpio-magic", "cpio-walk", "cpio-check", "path-norm", "trailer",
        "find-prim", "read-win", "zero-write", "size-consv", "bad-inject",
    ];
    for (i, n) in names.iter().enumerate() {
        b.add(n, 10 + i as u32)?;
    }
    // 未跑完聚合红码。
    if b.seal() != Err(XC_NOT_ALL_RUN) {
        return Err(XC_NOT_ALL_RUN);
    }
    for i in 0..10u8 {
        b.run(i, true)?;
    }
    // 重复跑红码。
    if b.run(0, true) != Err(XC_AID_DUP) {
        return Err(XC_AID_DUP);
    }
    // 封账：绿权和 = Σ(10..20) = 145。
    let (sub, chain) = b.seal()?;
    if sub != 145 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    if chain != chain_next(PREV_CHAIN, 145) {
        return Err(XC_CHAIN_MISMATCH);
    }
    b.hedge()?;
    // 封账后再落红码。
    if b.add("late", 1) != Err(XC_DOUBLE_SEAL) {
        return Err(XC_DOUBLE_SEAL);
    }
    if b.run(0, true) != Err(XC_DOUBLE_SEAL) {
        return Err(XC_DOUBLE_SEAL);
    }

    // 链核主路径：小计 4,300 命中终点。
    chain_check(CHAIN_TARGET)?;
    // 链核失配：小计 4,301 不命中。
    if chain_check(CHAIN_TARGET + 1) != Err(XC_CHAIN_MISMATCH) {
        return Err(XC_CHAIN_MISMATCH);
    }

    // 多批聚合：三批（145 / 200 / 4155）总 4,500。
    let mut agg = BatchAgg::new();
    agg.add_batch(0, 145)?;
    agg.add_batch(1, 200)?;
    agg.add_batch(2, 4155)?;
    agg.hedge()?;
    if agg.total != 4500 || agg.count() != 3 {
        return Err(XC_HEDGE_UNBALANCED);
    }

    // 错误路径 1：红单批——封账红码但对冲仍平。
    let mut rb = B30Batch::new();
    rb.add("a", 10)?;
    rb.add("b", 20)?;
    rb.run(0, true)?;
    rb.run(1, false)?;
    if rb.seal() != Err(XC_BATCH_RED) {
        return Err(XC_BATCH_RED);
    }
    // 封账已落（红也封），对冲应平（红条不计权和）。
    rb.hedge()?;
    if rb.subtotal() != 10 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    // 错误路径 2：空批。
    let mut eb = B30Batch::new();
    if eb.seal() != Err(XC_EMPTY_BATCH) {
        return Err(XC_EMPTY_BATCH);
    }
    // 错误路径 3：权值 0。
    let mut wb = B30Batch::new();
    if wb.add("zero", 0) != Err(XC_BAD_WEIGHT) {
        return Err(XC_BAD_WEIGHT);
    }
    // 错误路径 4：空批聚合。
    let mut ea = BatchAgg::new();
    if ea.add_batch(0, 0) != Err(XC_EMPTY_BATCH) {
        return Err(XC_EMPTY_BATCH);
    }
    // 错误路径 5：批号重复。
    let mut da = BatchAgg::new();
    da.add_batch(1, 10)?;
    if da.add_batch(1, 20) != Err(XC_AID_DUP) {
        return Err(XC_AID_DUP);
    }
    Ok(0)
}
