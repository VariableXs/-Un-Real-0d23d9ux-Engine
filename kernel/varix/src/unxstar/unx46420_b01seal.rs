//! UNX-F46420 B01 批收官印（20 条冻结/6,000 行满额/批册归档）（0x40130 段）
//!
//! 锚点：UNX-F46420-J1 批印入册：20/20 冻结、域账累计 6,000/240,000；
//! 轻门禁输出三断言全绿。
//! 域：CoRun·UNX-S1-B01（工具链域·批收官印面）。
//!
//! 结构：批册账（20 单逐单登记：单号/行数/冻结位）+ 域账（累计行数核：
//! 6,000/240,000）+ 轻门禁（三断言：满额/冻结齐/归档齐）+ 批印（三断言
//! 全绿才落印，印 = 确定性指纹）+ 注入核（缺单/行数不足/未冻结/域账
//! 漂移/封印改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40130 独占 0x10）。
pub const SEG_BASE: u32 = 0x40130;
pub const BS_CARD_MISS: u32 = SEG_BASE + 0x01; // 批册缺单
pub const BS_LINES_SHORT: u32 = SEG_BASE + 0x02; // 行数不足
pub const BS_NOT_FROZEN: u32 = SEG_BASE + 0x03; // 未冻结
pub const BS_DOMAIN_DRIFT: u32 = SEG_BASE + 0x04; // 域账漂移
pub const BS_SEALED: u32 = SEG_BASE + 0x05; // 封印改写

/// 批常量：B01 = 20 单 / 6,000 行 / 域账总盘 240,000 行。
pub const BATCH_CARDS: usize = 20;
pub const BATCH_LINES: u32 = 6000;
pub const DOMAIN_LINES: u32 = 240000;

/// 批册单目。
#[derive(Clone)]
pub struct BatchCard {
    pub seq: u32,
    pub lines: u32,
    pub frozen: bool,
}

/// 批收官印账。
pub struct BatchSeal {
    pub cards: Vec<BatchCard>,
    pub domain_used: u32,
    pub stamped: bool,
}

impl BatchSeal {
    pub fn new() -> Self {
        BatchSeal { cards: Vec::new(), domain_used: 0, stamped: false }
    }

    /// 单目登记（序号连续递增；行数 ≥200；封印拒写）。
    pub fn put(&mut self, seq: u32, lines: u32) -> Result<(), u32> {
        if self.stamped {
            return Err(BS_SEALED);
        }
        if seq as usize != self.cards.len() + 1 {
            return Err(BS_CARD_MISS);
        }
        if lines < 200 {
            return Err(BS_LINES_SHORT);
        }
        self.cards.push(BatchCard { seq, lines, frozen: false });
        Ok(())
    }

    /// 冻结（逐单；未登记拒绝）。
    pub fn freeze(&mut self, seq: u32) -> Result<(), u32> {
        if self.stamped {
            return Err(BS_SEALED);
        }
        match self.cards.iter_mut().find(|c| c.seq == seq) {
            None => Err(BS_NOT_FROZEN),
            Some(c) => {
                c.frozen = true;
                Ok(())
            }
        }
    }

    /// 域账登记（累计行数入库）。
    pub fn commit_domain(&mut self, lines: u32) -> Result<(), u32> {
        if self.stamped {
            return Err(BS_SEALED);
        }
        self.domain_used += lines;
        if self.domain_used > DOMAIN_LINES {
            return Err(BS_DOMAIN_DRIFT);
        }
        Ok(())
    }

    /// 满额断言：20 单齐 + 行数合计 = 6,000。
    pub fn full_batch(&self) -> bool {
        self.cards.len() == BATCH_CARDS
            && self.cards.iter().map(|c| c.lines).sum::<u32>() == BATCH_LINES
    }

    /// 冻结齐断言：20/20 冻结。
    pub fn all_frozen(&self) -> bool {
        self.cards.len() == BATCH_CARDS && self.cards.iter().all(|c| c.frozen)
    }

    /// 域账断言：累计 6,000 且 ≤ 总盘。
    pub fn domain_ok(&self) -> bool {
        self.domain_used == BATCH_LINES && self.domain_used <= DOMAIN_LINES
    }

    /// 轻门禁：三断言全绿才落印。
    pub fn stamp(&mut self) -> Result<u32, u32> {
        if self.stamped {
            return Err(BS_SEALED);
        }
        if !self.full_batch() {
            return Err(BS_CARD_MISS);
        }
        if !self.all_frozen() {
            return Err(BS_NOT_FROZEN);
        }
        if !self.domain_ok() {
            return Err(BS_DOMAIN_DRIFT);
        }
        self.stamped = true;
        Ok(self.seal_fingerprint())
    }

    /// 批印指纹（20 单序 + 行数 + 域账复合 FNV）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for c in self.cards.iter() {
            for b in format!("{}|{}|{};", c.seq, c.lines, c.frozen).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        for b in format!("domain={}", self.domain_used).as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 标准夹具：20 单（300 行/单）全冻结 + 域账 6,000 + 落印。
pub fn standard_seal() -> (BatchSeal, u32) {
    let mut b = BatchSeal::new();
    for i in 1..=BATCH_CARDS as u32 {
        b.put(i, 300).expect("fixture card");
    }
    for i in 1..=BATCH_CARDS as u32 {
        b.freeze(i).expect("fixture freeze");
    }
    b.commit_domain(BATCH_LINES).expect("fixture domain");
    let fp = b.stamp().expect("fixture stamp");
    (b, fp)
}

/// 注入核：6 注入必抓（缺单/行数不足/未冻结落印/域账漂移/封印改写/序断档）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺单落印必抓（19 单）
    let mut b = BatchSeal::new();
    for i in 1..BATCH_CARDS as u32 {
        b.put(i, 300).expect("partial");
        b.freeze(i).expect("partial freeze");
    }
    b.commit_domain(5700).expect("partial domain");
    if b.stamp() == Err(BS_CARD_MISS) {
        caught += 1;
    }
    // 2) 行数不足必抓
    let mut b2 = BatchSeal::new();
    if b2.put(1, 100) == Err(BS_LINES_SHORT) {
        caught += 1;
    }
    // 3) 未冻结落印必抓（20 单但冻结 19）
    let mut b3 = BatchSeal::new();
    for i in 1..=BATCH_CARDS as u32 {
        b3.put(i, 300).expect("cards");
    }
    for i in 1..BATCH_CARDS as u32 {
        b3.freeze(i).expect("freezes");
    }
    b3.commit_domain(BATCH_LINES).expect("domain");
    if b3.stamp() == Err(BS_NOT_FROZEN) {
        caught += 1;
    }
    // 4) 域账漂移必抓（累计 7,000 超 B01 应计入的 6,000 → domain_ok 假）
    let mut b4 = BatchSeal::new();
    for i in 1..=BATCH_CARDS as u32 {
        b4.put(i, 300).expect("c");
        b4.freeze(i).expect("f");
    }
    b4.commit_domain(7000).expect("over domain");
    if !b4.domain_ok() && b4.stamp() == Err(BS_DOMAIN_DRIFT) {
        caught += 1;
    }
    // 5) 封印改写必抓
    let (mut std_b, _fp) = standard_seal();
    if std_b.put(21, 300) == Err(BS_SEALED) {
        caught += 1;
    }
    // 6) 序断档必抓（跳号）
    let mut b5 = BatchSeal::new();
    b5.put(1, 300).expect("seq1");
    if b5.put(3, 300) == Err(BS_CARD_MISS) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46420_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46420");
    // J1-1 批印入册：20/20 冻结、6,000/240,000
    let (b, fp) = standard_seal();
    let ok = b.full_batch() && b.all_frozen() && b.domain_ok() && b.stamped;
    s.add("46420-J1-批印入册", ok, if ok { "" } else { "批印漂移" });
    // J1-2 批印指纹确定（两份夹具同印）
    let (_b2, fp2) = standard_seal();
    let ok = fp == fp2;
    s.add("46420-J1-批印指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 域账余量核（240,000 - 6,000 = 234,000）
    let ok = DOMAIN_LINES - b.domain_used == 234000;
    s.add("46420-J1-域账余量", ok, if ok { "" } else { "余量漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46420-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46420_all_checks_green() {
        let set = run_unx46420_checks();
        assert!(set.all_passed(), "UNX-F46420 checks must be green: {}", set.red_items_text());
    }
}
