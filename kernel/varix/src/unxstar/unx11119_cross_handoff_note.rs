//! UNX-F11119 跨域移交注账（联签面移交声明）（0x1f599 段）
//!
//! 联签面把跨域物件按件移交：每件声明来源域与去向域、应交付数与实交数，
//! 接收方逐件回执。逐件对平且全部回执后，移交声明才允许落账。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11119 独占 0x1f599。
pub const SEG_BASE: u32 = 0x1f599;

/// 移交件号越界。
pub const TR_RANGE: u32 = SEG_BASE + 0x01;
/// 来源域与去向域相同。
pub const TR_DOMAIN: u32 = SEG_BASE + 0x02;
/// 移交件重复登记。
pub const TR_DUP: u32 = SEG_BASE + 0x03;
/// 实交数与应交付数不齐。
pub const TR_UNBAL: u32 = SEG_BASE + 0x04;
/// 接收方未回执。
pub const TR_OPEN: u32 = SEG_BASE + 0x05;
/// 移交账已冻结。
pub const TR_FROZEN: u32 = SEG_BASE + 0x06;
/// 移交件数不足。
pub const TR_THIN: u32 = SEG_BASE + 0x07;
/// 容量溢出。
pub const TR_OVER: u32 = SEG_BASE + 0x08;

/// 移交件总数。
pub const ITEMS: usize = 6;
/// 每件应交付数。
pub const ARTIFACTS: usize = 5;
/// 容量。
pub const CAP: usize = 8;
/// 折叠种子。
const TR_TAG: u64 = 0x0000_1f5_9999;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 单件移交通道。
#[derive(Clone, Debug)]
pub struct TransferItem {
    /// 件号（1..=6）。
    pub no: usize,
    /// 来源域。
    pub from: String,
    /// 去向域。
    pub to: String,
    /// 应交付数。
    pub artifacts: usize,
    /// 实交付数。
    pub delivered: usize,
    /// 接收方是否回执。
    pub acked: bool,
}

impl TransferItem {
    /// 件是否逐件对平。
    pub fn balanced(&self) -> bool {
        self.artifacts > 0 && self.delivered == self.artifacts
    }

    /// 件是否闭合：对平且已回执。
    pub fn closed(&self) -> bool {
        self.balanced() && self.acked
    }

    /// 件渲染，形如 T1|C1>C2|5/5|A。
    pub fn render(&self) -> String {
        let flag = if self.acked { "A" } else { "N" };
        format!(
            "T{}|{}>{}|{}/{}|{}",
            self.no, self.from, self.to, self.delivered, self.artifacts, flag
        )
    }
}

/// 跨域移交账。
#[derive(Clone, Debug)]
pub struct TransferBook {
    items: Vec<TransferItem>,
    frozen: bool,
    root: u64,
}

impl TransferBook {
    /// 建空账。
    pub fn new() -> Self {
        Self { items: Vec::new(), frozen: false, root: 0 }
    }

    /// 定位件下标。
    fn index_of(&self, no: usize) -> usize {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        idx
    }

    /// 登记一件：件域、域差、重复、容量、冻结五道闸。
    pub fn enroll(&mut self, no: usize, from: &str, to: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(TR_FROZEN);
        }
        if no == 0 || no > ITEMS {
            return Err(TR_RANGE);
        }
        if from == to {
            return Err(TR_DOMAIN);
        }
        if self.items.len() >= CAP {
            return Err(TR_OVER);
        }
        if self.index_of(no) != usize::MAX {
            return Err(TR_DUP);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].from == from && self.items[k].to == to {
                return Err(TR_DOMAIN);
            }
            k = k.wrapping_add(1);
        }
        let src = String::from(from);
        let dst = String::from(to);
        self.items.push(TransferItem {
            no,
            from: src,
            to: dst,
            artifacts: ARTIFACTS,
            delivered: 0,
            acked: false,
        });
        Ok(())
    }

    /// 交付一件：逐件累计但不得超过应交付数。
    pub fn deliver(&mut self, no: usize, count: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(TR_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(TR_RANGE);
        }
        if let Some(it) = self.items.get_mut(idx) {
            let next = it.delivered.wrapping_add(count);
            if next > it.artifacts {
                return Err(TR_OVER);
            }
            it.delivered = next;
            return Ok(());
        }
        Err(TR_RANGE)
    }

    /// 接收方回执。
    pub fn ack(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(TR_FROZEN);
        }
        let idx = self.index_of(no);
        if idx == usize::MAX {
            return Err(TR_RANGE);
        }
        if let Some(it) = self.items.get_mut(idx) {
            it.acked = true;
            return Ok(());
        }
        Err(TR_RANGE)
    }

    /// 件数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 对平件数。
    pub fn balanced_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].balanced() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 闭合件数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].closed() {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 悬空件号清单。
    pub fn dangling(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].closed() && out.len() < 16 {
                out.push(self.items[k].no);
            }
            k = k.wrapping_add(1);
        }
        out.sort_by_key(|n| *n);
        out
    }

    /// 移交声明：件齐、逐件对平、全部回执、零悬空。
    pub fn transfer(&self) -> Result<(), u32> {
        if self.items.len() < ITEMS {
            return Err(TR_THIN);
        }
        if self.balanced_count() != ITEMS {
            return Err(TR_UNBAL);
        }
        if self.closed_count() != ITEMS || !self.dangling().is_empty() {
            return Err(TR_OPEN);
        }
        Ok(())
    }

    /// 冻结：返回折叠根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        self.root = self.fold();
        self.root
    }

    /// 已冻结否。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = TR_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.delivered as u64).wrapping_mul(FP_STEP);
            let b = if it.acked { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h
    }
}

/// 标准移交账：六件四域互移，逐件交满并回执。
pub fn build_transfer() -> TransferBook {
    let mut book = TransferBook::new();
    let _ = book.enroll(1, "C1", "C2");
    let _ = book.enroll(2, "C2", "C3");
    let _ = book.enroll(3, "C3", "C4");
    let _ = book.enroll(4, "C4", "C1");
    let _ = book.enroll(5, "C1", "C3");
    let _ = book.enroll(6, "C2", "C4");
    let mut no = 1usize;
    while no <= ITEMS {
        let _ = book.deliver(no, ARTIFACTS);
        let _ = book.ack(no);
        no = no.wrapping_add(1);
    }
    book
}

/// 四路对抗：件号越界、同域移交、交付不齐、未回执。
pub fn inject_four() -> [bool; 4] {
    let mut a_book = TransferBook::new();
    let a = a_book.enroll(0, "C1", "C2") == Err(TR_RANGE) && a_book.item_count() == 0;
    let mut b_book = TransferBook::new();
    let b = b_book.enroll(1, "C1", "C1") == Err(TR_DOMAIN) && b_book.item_count() == 0;
    let mut c_book = TransferBook::new();
    let _ = c_book.enroll(1, "C1", "C2");
    let _ = c_book.enroll(2, "C2", "C3");
    let _ = c_book.enroll(3, "C3", "C4");
    let _ = c_book.enroll(4, "C4", "C1");
    let _ = c_book.enroll(5, "C1", "C3");
    let _ = c_book.enroll(6, "C2", "C4");
    let mut no = 1usize;
    while no <= ITEMS {
        let cnt = if no == 3 { ARTIFACTS - 1 } else { ARTIFACTS };
        let _ = c_book.deliver(no, cnt);
        let _ = c_book.ack(no);
        no = no.wrapping_add(1);
    }
    let c = c_book.transfer() == Err(TR_UNBAL) && c_book.balanced_count() == ITEMS - 1;
    let mut d_book = TransferBook::new();
    let _ = d_book.enroll(1, "C1", "C2");
    let _ = d_book.enroll(2, "C2", "C3");
    let _ = d_book.enroll(3, "C3", "C4");
    let _ = d_book.enroll(4, "C4", "C1");
    let _ = d_book.enroll(5, "C1", "C3");
    let _ = d_book.enroll(6, "C2", "C4");
    let mut j = 1usize;
    while j <= ITEMS {
        let _ = d_book.deliver(j, ARTIFACTS);
        j = j.wrapping_add(1);
    }
    let mut m = 1usize;
    while m < ITEMS {
        let _ = d_book.ack(m);
        m = m.wrapping_add(1);
    }
    let d = d_book.transfer() == Err(TR_OPEN) && d_book.dangling() == [6];
    [a, b, c, d]
}

/// F11119 全部检查：移交账逐件对平与零悬空。
pub fn run_unx11119_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11119 跨域移交注账");
    let book = build_transfer();
    let four = inject_four();

    // J1 六件在册。
    s.add(
        "J1-六件在册",
        book.item_count() == ITEMS,
        "C1 至 C4 四域之间六件移交全部登记在册",
    );
    // J2 逐件对平。
    s.add(
        "J2-逐件对平",
        book.balanced_count() == ITEMS && book.closed_count() == ITEMS,
        "六件应交付 5 实交付 5，逐件对平且回执齐全",
    );
    // J3 移交声明成立。
    s.add(
        "J3-移交成立",
        book.transfer().is_ok() && book.dangling().is_empty(),
        "零悬空条件下移交声明通过",
    );
    // J4 件域闸。
    let mut range_book = TransferBook::new();
    s.add(
        "J4-件域闸",
        range_book.enroll(0, "C1", "C2") == Err(TR_RANGE)
            && range_book.enroll(7, "C1", "C2") == Err(TR_RANGE),
        "件号 0 与 7 越出 1..=6，登记报 TR_RANGE",
    );
    // J5 同域闸。
    let mut domain_book = TransferBook::new();
    s.add(
        "J5-同域闸",
        domain_book.enroll(1, "C2", "C2") == Err(TR_DOMAIN) && domain_book.item_count() == 0,
        "来源域与去向域相同报 TR_DOMAIN，不入账",
    );
    // J6 重件闸。
    let mut dup_book = TransferBook::new();
    let _ = dup_book.enroll(1, "C1", "C2");
    s.add(
        "J6-重件闸",
        dup_book.enroll(1, "C3", "C4") == Err(TR_DUP) && dup_book.item_count() == 1,
        "同一件号二次登记报 TR_DUP",
    );
    // J7 通道闸。
    let mut lane_book = TransferBook::new();
    let _ = lane_book.enroll(1, "C1", "C2");
    s.add(
        "J7-通道闸",
        lane_book.enroll(2, "C1", "C2") == Err(TR_DOMAIN) && lane_book.item_count() == 1,
        "同一来源去向通道重复占用报 TR_DOMAIN",
    );
    // J8 交付上限闸。
    let mut over_book = TransferBook::new();
    let _ = over_book.enroll(1, "C1", "C2");
    s.add(
        "J8-交付上限",
        over_book.deliver(1, ARTIFACTS + 1) == Err(TR_OVER) && over_book.balanced_count() == 0,
        "单件交付超过应交付数报 TR_OVER，件保持未对平",
    );
    // J9 交付不齐闸。
    let unbal = build_short();
    s.add(
        "J9-交付不齐",
        unbal.transfer() == Err(TR_UNBAL) && unbal.balanced_count() == ITEMS - 1,
        "第 3 件少交 1 件时仅 5 件对平，移交报 TR_UNBAL",
    );
    // J10 未回执闸。
    let mut open_book = TransferBook::new();
    let _ = open_book.enroll(1, "C1", "C2");
    let _ = open_book.enroll(2, "C2", "C3");
    let _ = open_book.enroll(3, "C3", "C4");
    let _ = open_book.enroll(4, "C4", "C1");
    let _ = open_book.enroll(5, "C1", "C3");
    let _ = open_book.enroll(6, "C2", "C4");
    let mut k = 1usize;
    while k <= ITEMS {
        let _ = open_book.deliver(k, ARTIFACTS);
        k = k.wrapping_add(1);
    }
    let mut j = 1usize;
    while j < ITEMS {
        let _ = open_book.ack(j);
        j = j.wrapping_add(1);
    }
    s.add(
        "J10-回执闸",
        open_book.transfer() == Err(TR_OPEN) && open_book.dangling() == [6],
        "第 6 件未回执时移交报 TR_OPEN，悬空清单为 6",
    );
    // J11 冻结拒写。
    let mut sealed = build_transfer();
    let root = sealed.seal();
    s.add(
        "J11-冻结拒写",
        sealed.is_frozen()
            && sealed.enroll(1, "C1", "C2") == Err(TR_FROZEN)
            && sealed.deliver(1, 1) == Err(TR_FROZEN)
            && sealed.ack(1) == Err(TR_FROZEN),
        "冻结后登记、交付、回执均报 TR_FROZEN",
    );
    // J12 冻结根。
    s.add(
        "J12-冻结根",
        sealed.fold() == root && root != TR_TAG,
        "冻结根等于冻结时刻的折叠值且不等于种子",
    );
    // J13 四路对抗。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "件号越界、同域移交、交付不齐、未回执四路全部被拒",
    );
    // J14 件渲染。
    let item = TransferItem {
        no: 1,
        from: String::from("C1"),
        to: String::from("C2"),
        artifacts: ARTIFACTS,
        delivered: ARTIFACTS,
        acked: true,
    };
    s.add(
        "J14-件渲染",
        item.render() == "T1|C1>C2|5/5|A" && item.closed(),
        "首件渲染为 T1|C1>C2|5/5|A",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-移交明细",
        book.transfer().is_ok(),
        &format!(
            "件={} 对平={} 闭合={} 悬空={}",
            book.item_count(),
            book.balanced_count(),
            book.closed_count(),
            book.dangling().len()
        ),
    );
    s
}

/// 欠交一件的移交账：第 3 件只交 4 件。
pub fn build_short() -> TransferBook {
    let mut book = TransferBook::new();
    let _ = book.enroll(1, "C1", "C2");
    let _ = book.enroll(2, "C2", "C3");
    let _ = book.enroll(3, "C3", "C4");
    let _ = book.enroll(4, "C4", "C1");
    let _ = book.enroll(5, "C1", "C3");
    let _ = book.enroll(6, "C2", "C4");
    let mut no = 1usize;
    while no <= ITEMS {
        let cnt = if no == 3 { ARTIFACTS - 1 } else { ARTIFACTS };
        let _ = book.deliver(no, cnt);
        let _ = book.ack(no);
        no = no.wrapping_add(1);
    }
    book
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11119_transfer_flow() {
        let mut book = build_transfer();
        assert_eq!(book.item_count(), ITEMS);
        assert_eq!(book.balanced_count(), ITEMS);
        assert!(book.transfer().is_ok());
        let root = book.seal();
        assert_eq!(book.fold(), root);
        assert_eq!(book.deliver(1, 1), Err(TR_FROZEN));
    }

    #[test]
    fn unx11119_checks_green() {
        let s = run_unx11119_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
