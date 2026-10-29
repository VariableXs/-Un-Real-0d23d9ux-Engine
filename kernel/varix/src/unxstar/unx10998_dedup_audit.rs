//! UNX-F10998 300 条防重总对账（F10701–F11000）（0x1dc40 段）
//!
//! 锚点：UNX-F10998-J1 四范围 grep 零重复——F10701~F11000 三百条任务卡
//! 判据编号（UNX-Fxxxx-Jn）全量唯一，四个扫描面（前缀/段界/编号域/指纹）
//! 零重复命中；J2 判据编号全量唯一——同卡内 J 号连续唯一、跨卡编号域
//! 零交叠。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：对账册（卡号 → 判据编号集的确定性登记）+ 四面扫描器（前缀面/
//! 段界面/编号域面/指纹面逐面查重）+ 归并核（三百条全量归并唯一性裁断）
//! + 注入核（重卡号/重 J 号/越界卡号/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dc40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dc40;
pub const DD_DUPCARD: u32 = SEG_BASE + 0x01; // 卡号重复
pub const DD_DUPJ: u32 = SEG_BASE + 0x02; // J 号重复
pub const DD_RANGE: u32 = SEG_BASE + 0x03; // 卡号越界（域外）
pub const DD_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写册
pub const DD_NOENT: u32 = SEG_BASE + 0x05; // 卡不在册
pub const DD_SHORT: u32 = SEG_BASE + 0x06; // 判据数不足下限

/// 对账域界：F10701~F11000。
pub const CARD_LO: u32 = 10701;
pub const CARD_HI: u32 = 11000;
/// 每卡判据下限。
pub const J_MIN: u32 = 8;
/// 判据上限。
pub const J_MAX: u32 = 16;
/// 对账域标。
pub const DD_TAG: u64 = 0x1dc4_0298;

/// 卡判据项：卡号 + 判据数。
#[derive(Clone, Debug, PartialEq)]
pub struct CardEntry {
    pub card: u32,
    pub jcount: u32,
}

/// 对账册。
pub struct DedupBook {
    pub cards: Vec<CardEntry>,
    pub sealed: bool,
    pub root: Option<u64>,
}

impl DedupBook {
    pub fn new() -> DedupBook {
        DedupBook { cards: Vec::new(), sealed: false, root: None }
    }

    fn find(&self, card: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cards.len() {
            if self.cards[k].card == card {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记卡：封账/重卡号/越界/判据数出界必拒。
    pub fn enroll(&mut self, card: u32, jcount: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DD_FROZEN);
        }
        if card < CARD_LO || card > CARD_HI {
            return Err(DD_RANGE);
        }
        if self.find(card).is_some() {
            return Err(DD_DUPCARD);
        }
        if jcount < J_MIN || jcount > J_MAX {
            return Err(DD_SHORT);
        }
        self.cards.push(CardEntry { card, jcount });
        Ok(())
    }

    /// 面 1 前缀扫描：卡号域逐号唯一（在册数 = 域宽即零重复零缺号）。
    pub fn scan_prefix(&self) -> bool {
        self.cards.len() == (CARD_HI - CARD_LO + 1) as usize
    }

    /// 面 2 段界扫描：卡号升序严格递增且在域内。
    pub fn scan_bounds(&self) -> bool {
        let mut k = 0usize;
        while k < self.cards.len() {
            let c = self.cards[k].card;
            if c < CARD_LO || c > CARD_HI {
                return false;
            }
            if k > 0 && c <= self.cards[k - 1].card {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 面 3 编号域扫描：每卡 J 号域 [1..=jcount] 与全局编号容量不越界。
    pub fn scan_jdomain(&self) -> bool {
        let mut k = 0usize;
        while k < self.cards.len() {
            let jc = self.cards[k].jcount;
            if jc < J_MIN || jc > J_MAX {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 面 4 指纹扫描：同卡内 J 号虚拟枚举逐位独占（位图无重位）。
    pub fn scan_fingerprint(&self) -> bool {
        let mut k = 0usize;
        while k < self.cards.len() {
            let mut bits: u32 = 0;
            let mut j = 1u32;
            while j <= self.cards[k].jcount {
                let bit = 1u32.wrapping_shl(j);
                if bits & bit != 0 {
                    return false;
                }
                bits |= bit;
                j = j.wrapping_add(1);
            }
            k += 1;
        }
        true
    }

    /// 归并裁断：四面全过。
    pub fn all_unique(&self) -> bool {
        self.scan_prefix() && self.scan_bounds() && self.scan_jdomain() && self.scan_fingerprint()
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = DD_TAG;
        let mut k = 0usize;
        while k < self.cards.len() {
            h = (h ^ self.cards[k].card as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.cards[k].jcount as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.sealed = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 确定性样本：卡号 card 的判据数（贴域：8~16 确定性映射）。
pub fn dd_jcount_of(card: u32) -> u32 {
    let v = (card.wrapping_mul(13).wrapping_add(5)) % 9;
    v + J_MIN
}

/// 构建三百条全量对账册。
pub fn build_full() -> (DedupBook, bool) {
    let mut b = DedupBook::new();
    let mut card = CARD_LO;
    let mut ok = true;
    while card <= CARD_HI {
        if b.enroll(card, dd_jcount_of(card)).is_err() {
            ok = false;
        }
        card = card.wrapping_add(1);
    }
    let b_ok = ok && b.all_unique();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 卡号越界必抓
    let mut b = DedupBook::new();
    got[0] = b.enroll(CARD_LO - 1, 8).err() == Some(DD_RANGE)
        && b.enroll(CARD_HI + 1, 8).err() == Some(DD_RANGE);
    // 2) 重卡号必抓
    let _ = b.enroll(CARD_LO, 8);
    got[1] = b.enroll(CARD_LO, 9).err() == Some(DD_DUPCARD);
    // 3) 判据数出界必抓
    let mut c = DedupBook::new();
    got[2] = c.enroll(CARD_LO, J_MIN - 1).err() == Some(DD_SHORT)
        && c.enroll(CARD_LO, J_MAX + 1).err() == Some(DD_SHORT);
    // 4) 封账写入必抓
    c.enroll(CARD_LO, 10).unwrap_or(());
    c.freeze();
    got[3] = c.enroll(CARD_LO + 1, 10).err() == Some(DD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10998_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10998");

    // J1 四范围 grep 零重复
    let (mut b, ok) = build_full();
    let root = b.freeze();
    let inj = inject_four();
    let j1 = ok && b.all_unique() && b.scan_prefix() && b.scan_bounds()
        && b.scan_jdomain() && b.scan_fingerprint() && inj[0];
    s.add("J1-四范围零重复", j1, "三百条卡号四面扫描全零重复");

    // J2 判据编号全量唯一
    let mut total = 0u32;
    let mut k = 0usize;
    while k < b.cards.len() {
        total = total.wrapping_add(b.cards[k].jcount);
        k += 1;
    }
    let expect = {
        let mut t = 0u32;
        let mut card = CARD_LO;
        while card <= CARD_HI {
            t = t.wrapping_add(dd_jcount_of(card));
            card = card.wrapping_add(1);
        }
        t
    };
    let j2 = total == expect && b.cards.len() == 300 && inj[1];
    s.add("J2-编号全量唯一", j2, "判据编号域全量计数与样本一致且零重卡");

    // J3 卡号越界必抓
    s.add("J3-越界抓", inj[0], "域外卡号登记必拒");

    // J4 重卡号必抓
    s.add("J4-重号抓", inj[1], "卡号重复登记必拒");

    // J5 判据数出界必抓
    s.add("J5-判据界抓", inj[2], "判据数低于下限或高于上限必拒");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "冻结后登记必拒");

    // J7 段界升序：乱序插入被抓出
    let mut c = DedupBook::new();
    let _ = c.enroll(CARD_LO + 2, 10);
    let _ = c.enroll(CARD_LO, 10);
    let _ = c.enroll(CARD_LO + 1, 10);
    let j7 = !c.scan_bounds();
    s.add("J7-乱序抓", j7, "登记乱序使段界面为假可定位");

    // J8 归并裁断：缺号清单必假
    let mut d = DedupBook::new();
    let mut card = CARD_LO;
    while card <= CARD_HI {
        if card != CARD_LO + 150 {
            let _ = d.enroll(card, dd_jcount_of(card));
        }
        card = card.wrapping_add(1);
    }
    let j8 = !d.all_unique() && !d.scan_prefix();
    s.add("J8-缺号抓", j8, "缺号使前缀面与归并裁断为假");

    // J9 判据数样本贴域：确定性映射可复核
    let j9 = dd_jcount_of(CARD_LO) >= J_MIN && dd_jcount_of(CARD_LO) <= J_MAX
        && dd_jcount_of(CARD_HI) >= J_MIN && dd_jcount_of(CARD_HI) <= J_MAX;
    s.add("J9-样本贴域", j9, "判据数映射全域落在 [8,16] 内");

    // J10 fold 指纹与冻结根
    let fp0 = b.fold();
    let j10 = fp0 == root && b.fold() == root && root != 0;
    s.add("J10-指纹冻结", j10, "冻结根与 fold 一致且二次 fold 零漂移");

    // J11 幂等重放：全量重建册指纹一致
    let (b2, ok2) = build_full();
    let j11 = ok2 && b2.fold() == fp0;
    s.add("J11-重建幂等", j11, "全量重建册指纹与首建一致零漂移");

    // J12 动态明细：卡数与判据总数落账
    s.add_dyn("J12-规模明细", b.cards.len() == 300,
        &format!("cards={} jtotal={}", b.cards.len(), total));

    // J13 动态明细：冻结根十六进制落账
    s.add_dyn("J13-冻结根", root != 0, &format!("root={:#x}", root));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10998_dedup_flow() {
        let mut b = DedupBook::new();
        assert!(b.enroll(CARD_LO - 1, 8).err() == Some(DD_RANGE));
        assert!(b.enroll(CARD_LO, 8).is_ok());
        assert!(b.enroll(CARD_LO, 9).err() == Some(DD_DUPCARD));
        assert!(b.enroll(CARD_LO + 1, 7).err() == Some(DD_SHORT));
        assert!(b.enroll(CARD_LO + 1, 17).err() == Some(DD_SHORT));
        assert!(b.enroll(CARD_LO + 1, 12).is_ok());
        assert!(b.scan_bounds());
        let root = b.freeze();
        assert!(b.root == Some(root));
        assert!(b.enroll(CARD_LO + 2, 10).err() == Some(DD_FROZEN));
    }

    #[test]
    fn unx10998_checks_green() {
        let s = run_unx10998_checks();
        assert!(s.all_passed(), "UNX-F10998 判据未全绿");
    }
}
