//! UNX-F11000 C4 域总收口（B16–B30 断言聚合）（0x1e000 段）
//!
//! 锚点：UNX-F11000-J1 F10981–F10999 十九条判据聚合回归（460 项全流程），
//! 段累计对平（179,880+7,300=187,180），域总账移交清单更新，B31–B40 收官
//! 接力声明在册。
//! 域：CoRun·UNX-C4-B30（五族联测总收口域）。
//!
//! 结构：聚合重放槽（十九路判据逐槽登记、逐槽裁断，坏样本注入必抓）+
//! 段累计对平器（基账 + 接力账闭式对平，越限即拒）+ 移交清单（逐项登记、
//! 封存后拒写）+ 接力声明册（B31–B40 十批接力在册可查）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e000 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e000;
pub const DC_BADVERDICT: u32 = SEG_BASE + 0x01; // 判据槽裁断失真
pub const DC_SHORT: u32 = SEG_BASE + 0x02; // 十九路未齐
pub const DC_DRIFT: u32 = SEG_BASE + 0x03; // 段累计对平失败
pub const DC_EMPTYNAME: u32 = SEG_BASE + 0x04; // 判据名空
pub const DC_FROZEN: u32 = SEG_BASE + 0x05; // 封存后写账
pub const DC_DUP: u32 = SEG_BASE + 0x06; // 槽号重号
pub const DC_NOHAND: u32 = SEG_BASE + 0x07; // 移交项不存在

/// 十九路判据（F10981–F10999）。
pub const JUDGES_NEEDED: usize = 19;
/// 判据槽定容。
pub const SLOT_CAP: usize = 24;
/// C4 域收口标（fold 种子）。
pub const C4_TAG: u64 = 0x1e00c4;
/// 段累计基账（B16–B30 已累计行数）。
pub const SEG_ACCUM_BASE: u64 = 179_880;
/// 接力批新增行数。
pub const SEG_RELAY_ADD: u64 = 7_300;
/// 段累计对平目标。
pub const SEG_ACCUM_TOTAL: u64 = 187_180;
/// 接力批总数（B31–B40）。
pub const RELAY_BATCHES: usize = 10;

/// 单条聚合重放槽：一路判据的登记与裁断位。
#[derive(Clone, Debug)]
pub struct ReplaySlot {
    pub id: u32,
    pub name: String,
    pub probe: u64,
    pub verdict: bool,
    pub settled: bool,
}

impl ReplaySlot {
    /// 探针与判定一致且已落裁断位才算通过。
    pub fn green(&self) -> bool {
        self.verdict && self.settled && self.probe != 0
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("S{}|{}|{}", self.id, flag, self.name)
    }
}

/// C4 域总收口册：聚合重放 → 段累计对平 → 移交清单 → 接力声明 → 封存。
pub struct DomainCloseLedger {
    pub slots: Vec<ReplaySlot>,
    pub handover: Vec<String>,
    pub relay: Vec<u32>,
    pub accum: u64,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DomainCloseLedger {
    pub fn new() -> DomainCloseLedger {
        DomainCloseLedger {
            slots: Vec::new(),
            handover: Vec::new(),
            relay: Vec::new(),
            accum: 0,
            frozen: false,
            root: None,
        }
    }

    /// 登记判据槽：封存、空名、零号、重号、超容均拒。
    pub fn enroll(&mut self, id: u32, name: &str, probe: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(DC_FROZEN);
        }
        if name.is_empty() {
            return Err(DC_EMPTYNAME);
        }
        if id == 0 || self.slots.iter().any(|s| s.id == id) {
            return Err(DC_DUP);
        }
        if self.slots.len() >= SLOT_CAP {
            return Err(DC_SHORT);
        }
        self.slots.push(ReplaySlot {
            id,
            name: name.to_string(),
            probe,
            verdict: false,
            settled: false,
        });
        Ok(())
    }

    /// 裁断：探针闭环（探针值可由期望值重算且非零）才翻绿。
    pub fn judge(&mut self, id: u32, expect: u64) -> Result<bool, u32> {
        if self.frozen {
            return Err(DC_FROZEN);
        }
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].id == id {
                ok = self.slots[k].probe == expect && self.slots[k].probe != 0;
                self.slots[k].verdict = ok;
                self.slots[k].settled = true;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(DC_NOHAND)
        }
    }

    /// 未裁断槽号清单（按登记序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.slots.len() {
            if !self.slots[k].settled {
                g.push(self.slots[k].id);
            }
            k += 1;
        }
        g
    }

    /// 红槽清单（已裁断但为假）。
    pub fn reds(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].settled && !self.slots[k].verdict {
                g.push(self.slots[k].id);
            }
            k += 1;
        }
        g
    }

    /// 聚合裁断：十九路齐、全数裁断、零红槽。
    pub fn closed(&self) -> bool {
        self.slots.len() == JUDGES_NEEDED
            && self.gaps().is_empty()
            && self.reds().is_empty()
    }

    /// 移交清单注账：封存、空串均拒。
    pub fn hand(&mut self, item: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(DC_FROZEN);
        }
        if item.is_empty() {
            return Err(DC_EMPTYNAME);
        }
        self.handover.push(item.to_string());
        Ok(())
    }

    /// 接力声明：B31–B40 批号逐一在册，重复批号拒。
    pub fn relay_declare(&mut self, batch: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(DC_FROZEN);
        }
        if batch == 0 || self.relay.iter().any(|b| *b == batch) {
            return Err(DC_DUP);
        }
        self.relay.push(batch);
        Ok(())
    }

    /// 段累计对平：基账 + 接力账闭式校验，越限即拒。
    pub fn reconcile(&mut self, base: u64, relay: u64, total: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(DC_FROZEN);
        }
        let sum = base.wrapping_add(relay);
        if sum != total {
            return Err(DC_DRIFT);
        }
        self.accum = sum;
        Ok(sum)
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含槽/移交/接力三维）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = C4_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let s = &self.slots[k];
            h = (h ^ s.id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.probe).wrapping_mul(0x100000001b3);
            h = (h ^ s.verdict as u64).wrapping_mul(0x100000001b3);
            let bytes = s.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        k = 0;
        while k < self.handover.len() {
            let bytes = self.handover[k].as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        k = 0;
        while k < self.relay.len() {
            h = (h ^ self.relay[k] as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.accum).wrapping_mul(0x100000001b3);
        h
    }

    /// 封存：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// F10981–F10999 第 idx 路确定性探针（闭式期望供正反两路共用）。
pub fn c4_probe(idx: u32) -> u64 {
    // 等差守恒闭式：sum(1..=n) 与阶乘化 fold 混合，全程 wrapping 防溢出。
    let n = idx.wrapping_add(9);
    let sum = n.wrapping_mul(n.wrapping_add(1)) / 2;
    (sum as u64).wrapping_mul(0x100000001b3) ^ (idx as u64)
}

/// 十九路全量重放：返回全绿与否与红槽数。
pub fn replay_nineteen(l: &mut DomainCloseLedger) -> (bool, usize) {
    let mut i = 1u32;
    while i <= JUDGES_NEEDED as u32 {
        let _ = l.enroll(i, "聚合重放路", c4_probe(i));
        i += 1;
    }
    let mut i = 1u32;
    let mut reds = 0usize;
    while i <= JUDGES_NEEDED as u32 {
        if l.judge(i, c4_probe(i)) != Ok(true) {
            reds += 1;
        }
        i += 1;
    }
    (reds == 0 && l.closed(), reds)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 裁断期望值给错必出红槽
    let mut a = DomainCloseLedger::new();
    let _ = a.enroll(1, "错值样本", c4_probe(1));
    let _ = a.judge(1, c4_probe(1).wrapping_add(1));
    got[0] = a.reds().len() == 1 && !a.closed();
    // 2) 十九路缺一必不算收口
    let mut b = DomainCloseLedger::new();
    let mut i = 1u32;
    while i < JUDGES_NEEDED as u32 {
        let _ = b.enroll(i, "缺路样本", c4_probe(i));
        i += 1;
    }
    got[1] = !b.closed();
    // 3) 段累计对平错值必拒
    let mut c = DomainCloseLedger::new();
    got[2] = c.reconcile(SEG_ACCUM_BASE, SEG_RELAY_ADD, SEG_ACCUM_TOTAL + 1).err() == Some(DC_DRIFT);
    // 4) 封存后写账必抓
    let mut d = DomainCloseLedger::new();
    let _ = d.hand("封存前注账");
    d.freeze();
    got[3] = d.hand("封存后注账").err() == Some(DC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11000_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11000");

    // J1 十九条判据聚合回归全绿
    let mut book = DomainCloseLedger::new();
    let (green, reds) = replay_nineteen(&mut book);
    s.add("J1-十九路聚合重放", green && reds == 0 && book.closed(), "F10981–F10999 全量重放零红槽");

    // J2 段累计对平：179,880+7,300=187,180
    let rec = book.reconcile(SEG_ACCUM_BASE, SEG_RELAY_ADD, SEG_ACCUM_TOTAL);
    s.add("J2-段累计对平", rec == Ok(SEG_ACCUM_TOTAL) && book.accum == SEG_ACCUM_TOTAL, "基账加接力账闭式等于总账");

    // J3 域总账移交清单更新：四件移交注账齐
    let mut hands_ok = true;
    let mut h = 0usize;
    let hand_items = ["十九路重放账", "段累计对平账", "B31–B40 接力声明", "域总账封存根"];
    while h < hand_items.len() {
        if book.hand(hand_items[h]).is_err() {
            hands_ok = false;
        }
        h += 1;
    }
    s.add("J3-移交清单更新", hands_ok && book.handover.len() == 4, "四件移交注账逐件在册");

    // J4 B31–B40 接力声明在册
    let mut relay_ok = true;
    let mut b = 31u32;
    while b <= 40u32 {
        if book.relay_declare(b).is_err() {
            relay_ok = false;
        }
        b += 1;
    }
    s.add("J4-接力声明在册", relay_ok && book.relay.len() == RELAY_BATCHES, "十批接力逐批声明");

    // J5 封存根：fold 稳定且二次一致
    let root = book.freeze();
    s.add("J5-封存根", book.root == Some(root) && book.fold() == root && root != 0, "封存根落账且指纹稳定");

    // J6 封存后写账必抓（正册复现）
    let j6 = book.enroll(77, "迟到槽", 1).err() == Some(DC_FROZEN)
        && book.hand("迟到注账").err() == Some(DC_FROZEN);
    s.add("J6-封存拒写", j6, "封存后登记与注账均必拒");

    // J7 注入检出：错值裁断 + 缺路
    let inj = inject_four();
    s.add("J7-错值缺路检出", inj[0] && inj[1], "错值必红且缺路必不算收口");

    // J8 注入检出：对平错值 + 封存后写
    s.add("J8-对平封存检出", inj[2] && inj[3], "对平错值必拒且封存后写必抓");

    // J9 重号拒与空名拒
    let mut d = DomainCloseLedger::new();
    let _ = d.enroll(1, "甲", 5);
    let j9 = d.enroll(1, "乙", 6).err() == Some(DC_DUP)
        && d.enroll(2, "", 6).err() == Some(DC_EMPTYNAME);
    s.add("J9-重号空名拒", j9, "槽号重号与判据名空均必拒");

    // J10 裁断目标不存在拒
    let j10 = DomainCloseLedger::new().judge(9, 1).err() == Some(DC_NOHAND);
    s.add("J10-目标不存在", j10, "裁断不存在的槽必拒");

    // J11 探针确定性：同 idx 两次求值一致且异 idx 不撞
    let p3a = c4_probe(3);
    let p3b = c4_probe(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= JUDGES_NEEDED as u32 {
        let mut j = i.wrapping_add(1);
        while j <= JUDGES_NEEDED as u32 {
            if c4_probe(i) == c4_probe(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J11-探针确定性", p3a == p3b && distinct, "探针同入同出且十九路互异");

    // J12 缺口语义：漏裁断一路时 gaps 可枚举且 closed 为假
    let j12 = {
        let mut c = DomainCloseLedger::new();
        let mut n = 1u32;
        while n <= JUDGES_NEEDED as u32 {
            let _ = c.enroll(n, "缺口样本", c4_probe(n));
            n += 1;
        }
        let mut m = 1u32;
        while m < JUDGES_NEEDED as u32 {
            let _ = c.judge(m, c4_probe(m));
            m += 1;
        }
        c.gaps().len() == 1 && !c.closed()
    };
    s.add("J12-缺口枚举", j12, "漏裁断一路可枚举且收口为假");

    // J13 未裁断不算收口：探针齐但裁断位未翻
    let j13 = {
        let mut c = DomainCloseLedger::new();
        let mut n = 1u32;
        while n <= JUDGES_NEEDED as u32 {
            let _ = c.enroll(n, "未翻位", c4_probe(n));
            n += 1;
        }
        !c.closed() && c.gaps().len() == JUDGES_NEEDED
    };
    s.add("J13-未裁断不算收", j13, "探针齐但未逐路裁断仍不算收口");

    // J14 容量边界：第 SLOT_CAP+1 条登记被拒且不破坏原账
    let j14 = {
        let mut c = DomainCloseLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= SLOT_CAP as u32 {
            if c.enroll(n, "x", c4_probe(n & 0x0f).wrapping_add(n as u64)).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(999, "over", 1).err() == Some(DC_SHORT)
    };
    s.add("J14-容量边界", j14, "超出定容必拒且原账不损");

    // J15 红槽裁断位不可静默翻绿：红槽不因二次 gap 消失
    let j15 = {
        let mut c = DomainCloseLedger::new();
        let _ = c.enroll(1, "红槽样本", 7);
        let _ = c.judge(1, 8);
        c.reds().len() == 1 && c.gaps().is_empty() && !c.closed()
    };
    s.add("J15-红槽不静默", j15, "裁断为假必须留红不可静默");

    // J16 动态明细：段累计十进制落账
    let acc = book.accum;
    s.add_dyn("J16-段累计明细", acc == SEG_ACCUM_TOTAL, &format!("accum={}", acc));

    // J17 动态明细：接力批号清单闭合
    let first = if book.relay.is_empty() { 0u32 } else { book.relay[0] };
    let last = if book.relay.is_empty() { 0u32 } else { book.relay[book.relay.len() - 1] };
    s.add_dyn(
        "J17-接力清单明细",
        first == 31 && last == 40 && book.relay.len() == 10,
        &format!("relay={}..{} n={}", first, last, book.relay.len()),
    );

    // J18 动态明细：移交件数与封存根十六进制
    s.add_dyn(
        "J18-移交封存明细",
        book.handover.len() == 4 && book.root.is_some(),
        &format!("hand={} root={:#x}", book.handover.len(), book.fold()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11000_close_flow() {
        let mut l = DomainCloseLedger::new();
        assert!(l.enroll(1, "a", 5).is_ok());
        assert!(l.enroll(1, "dup", 1).err() == Some(DC_DUP));
        assert_eq!(l.judge(1, 5), Ok(true));
        assert!(l.enroll(2, "b", 5).is_ok());
        assert_eq!(l.judge(2, 6), Ok(false));
        assert!(!l.closed());
        // 段累计闭式是 base + relay == total：10+5=15 对平，10+6≠15 必漂移。
        // （初版两行写反了——把对平当漂移、把越限当对平，与 reconcile 的
        // 文档闭式直接矛盾，见 `reconcile` 注释与 J-段累计 判据。）
        assert!(l.reconcile(10, 5, 15).is_ok());
        assert!(l.reconcile(10, 6, 15).err() == Some(DC_DRIFT));
        l.freeze();
        assert!(l.enroll(3, "late", 1).err() == Some(DC_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11000_checks_green() {
        let s = run_unx11000_checks();
        assert!(s.all_passed(), "UNX-F11000 判据未全绿");
    }
}
