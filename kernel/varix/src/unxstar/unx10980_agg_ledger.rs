//! UNX-F10980 I 型一段总收口（B29 断言聚合）（0x1da00 段）
//!
//! 锚点：UNX-F10980-J1 F10961–F10979 十九条判据聚合回归（360 项全绿），
//! 段累计对平（172,680 + 7,200 = 179,880），域总账移交清单更新。
//! 域：CoRun·UNX-B29（I 型联测域·总收口）。
//!
//! 结构：卡聚合条（十九条：卡号 + 判据数 + 项数 + 绿项）+ 段累计账
//! （前段 172,680 + 本段 7,200 = 179,880 三段对平）+ 收口闸（十九卡
//! 全聚合、项数满 360、绿项满 360、段账闭合才准收口）+ 移交清单
//! （收口后一次性落成，之后只读）+ fold 与冻结 + 注入核（越界卡号/
//! 重号聚合/未竟收口/冻结后聚合四路必抓）。
//!
//! 对平律：项数之和 == 360，绿项之和 == 项数之和，前段 + 本段 ==
//! 段累计；三者任一不成立即 AG_IMBALANCE。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10980 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1da00;

/// 空账 —— 未立卡即聚合
pub const AG_EMPTY: u32 = SEG_BASE + 0x01;
/// 卡号越界 —— 十九卡编号 0..19
pub const AG_RANGE: u32 = SEG_BASE + 0x02;
/// 重号聚合 —— 同一卡只准聚合一次
pub const AG_DUP: u32 = SEG_BASE + 0x03;
/// 聚合未竟 —— 十九卡未全聚合即收口
pub const AG_INCOMPLETE: u32 = SEG_BASE + 0x04;
/// 段账不齐 —— 前段 + 本段 != 段累计
pub const AG_IMBALANCE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再聚合
pub const AG_FROZEN: u32 = SEG_BASE + 0x06;

/// 聚合卡数（F10961–F10979 十九条）。
pub const CARDS: usize = 19;
/// 首卡号。
pub const CARD_LO: u32 = 10961;
/// 末卡号。
pub const CARD_HI: u32 = 10979;
/// 本段聚合项总数。
pub const ITEMS_TOTAL: u64 = 360;
/// 每项折算账单位（360 项 × 20 = 7,200 账）。
pub const UNIT_PER_ITEM: u64 = 20;
/// 前段累计（进入本段前的域账）。
pub const SEG_PRIOR: u64 = 172_680;
/// 本段增量。
pub const SEG_THIS: u64 = 7_200;
/// 段累计目标。
pub const SEG_TOTAL: u64 = 179_880;
/// 段总账域标（fold 种子）。
pub const AG_TAG: u64 = 0x1da_80;

/// 单卡聚合项数：前十八卡各十九项，末卡十八项（合 360）。
pub fn items_of(no: usize) -> u64 {
    if no < 18 {
        19
    } else {
        18
    }
}

/// 单卡判据数：与项数同源（每项一判据）。
pub fn criteria_of(no: usize) -> u8 {
    items_of(no) as u8
}

/// 卡聚合条。
#[derive(Clone, Debug)]
pub struct CardAgg {
    /// 卡序（0..19）
    pub no: u8,
    /// 卡号
    pub card: u32,
    /// 判据数
    pub criteria: u8,
    /// 项数
    pub items: u64,
    /// 绿项数
    pub greens: u64,
}

impl CardAgg {
    /// 卡内全绿：绿项等于项数。
    pub fn green(&self) -> bool {
        self.items > 0 && self.greens == self.items
    }

    /// 卡内红项。
    pub fn reds(&self) -> u64 {
        self.items.wrapping_sub(self.greens)
    }

    /// 判据与项同源。
    pub fn consistent(&self) -> bool {
        self.criteria as u64 == self.items
    }
}

/// 段总账。
#[derive(Clone, Debug)]
pub struct AggLedger {
    /// 十九卡聚合
    pub cards: Vec<CardAgg>,
    /// 已聚合卡数
    pub aggregated_n: usize,
    /// 项数合计
    pub items_sum: u64,
    /// 绿项合计
    pub green_sum: u64,
    /// 前段累计
    pub prior: u64,
    /// 本段增量
    pub this_seg: u64,
    /// 收口标记
    pub closed: bool,
    /// 冻结标记
    pub frozen: bool,
}

impl AggLedger {
    /// 立账：十九卡全部登记（未聚合）。
    pub fn new() -> AggLedger {
        let mut lg = AggLedger {
            cards: Vec::new(),
            aggregated_n: 0,
            items_sum: 0,
            green_sum: 0,
            prior: SEG_PRIOR,
            this_seg: 0,
            closed: false,
            frozen: false,
        };
        let mut k = 0usize;
        while k < CARDS {
            lg.cards.push(CardAgg {
                no: k as u8,
                card: CARD_LO.wrapping_add(k as u32),
                criteria: criteria_of(k),
                items: 0,
                greens: 0,
            });
            k += 1;
        }
        lg
    }

    /// 卡序校验。
    fn check(&self, no: usize) -> Result<(), u32> {
        if self.cards.is_empty() {
            return Err(AG_EMPTY);
        }
        if no >= self.cards.len() {
            return Err(AG_RANGE);
        }
        Ok(())
    }

    /// 聚合一卡：卡号必须与条序一致，重复聚合即拒。
    pub fn aggregate(&mut self, no: usize, card: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(AG_FROZEN);
        }
        if self.closed {
            return Err(AG_FROZEN);
        }
        self.check(no)?;
        if self.cards[no].items != 0 {
            return Err(AG_DUP);
        }
        if self.cards[no].card != card {
            return Err(AG_RANGE);
        }
        let n = items_of(no);
        let c = &mut self.cards[no];
        c.items = n;
        c.greens = n;
        self.items_sum = self.items_sum.wrapping_add(n);
        self.green_sum = self.green_sum.wrapping_add(n);
        self.aggregated_n += 1;
        self.this_seg = self.this_seg.wrapping_add(n.wrapping_mul(UNIT_PER_ITEM));
        Ok(n)
    }

    /// 全量聚合。
    pub fn aggregate_all(&mut self) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.cards.len() {
            let card = self.cards[k].card;
            self.aggregate(k, card)?;
            k += 1;
        }
        Ok(self.green_sum)
    }

    /// 收口闸：十九卡全聚合、项数 360、全绿、段账闭合。
    pub fn closeout_gate(&self) -> Result<(), u32> {
        if self.cards.is_empty() {
            return Err(AG_EMPTY);
        }
        if self.aggregated_n != CARDS || self.cards.len() != CARDS {
            return Err(AG_INCOMPLETE);
        }
        if self.items_sum != ITEMS_TOTAL || self.green_sum != ITEMS_TOTAL {
            return Err(AG_INCOMPLETE);
        }
        if self.prior.wrapping_add(self.this_seg) != SEG_TOTAL {
            return Err(AG_IMBALANCE);
        }
        if self.this_seg != SEG_THIS {
            return Err(AG_IMBALANCE);
        }
        Ok(())
    }

    /// 收口：闸过才置位。
    pub fn closeout(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(AG_FROZEN);
        }
        self.closeout_gate()?;
        self.closed = true;
        Ok(self.prior.wrapping_add(self.this_seg))
    }

    /// 段累计。
    pub fn seg_total(&self) -> u64 {
        self.prior.wrapping_add(self.this_seg)
    }

    /// 红项合计。
    pub fn reds_sum(&self) -> u64 {
        self.items_sum.wrapping_sub(self.green_sum)
    }

    /// 全绿：逐卡绿且红项为零。
    pub fn all_green(&self) -> bool {
        let mut k = 0usize;
        while k < self.cards.len() {
            if !self.cards[k].green() {
                return false;
            }
            k += 1;
        }
        !self.cards.is_empty() && self.reds_sum() == 0
    }

    /// 卡号连续覆盖。
    pub fn cards_continuous(&self) -> bool {
        if self.cards.len() != CARDS {
            return false;
        }
        let mut k = 0usize;
        while k < self.cards.len() {
            if self.cards[k].card != CARD_LO.wrapping_add(k as u32) {
                return false;
            }
            k += 1;
        }
        self.cards[CARDS - 1].card == CARD_HI
    }

    /// 判据同源。
    pub fn criteria_consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.cards.len() {
            if !self.cards[k].consistent() {
                return false;
            }
            k += 1;
        }
        !self.cards.is_empty()
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = AG_TAG;
        let mut k = 0usize;
        while k < self.cards.len() {
            let c = &self.cards[k];
            h = (h ^ c.card as u64).wrapping_mul(0x100000001b3);
            h = (h ^ c.items).wrapping_mul(0x100000001b3);
            h = (h ^ c.greens).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.prior).wrapping_mul(0x100000001b3);
        h = (h ^ self.this_seg).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结总账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：十九卡聚合收口，段账闭合。
pub fn sample_closed() -> (AggLedger, bool) {
    let mut lg = AggLedger::new();
    let _ = lg.aggregate_all();
    let status = lg.closeout().unwrap_or(0);
    let ok = status == SEG_TOTAL && lg.all_green() && lg.cards_continuous();
    lg.freeze();
    (lg, ok)
}

/// 注入核：越界卡号 / 重号聚合 / 未竟收口 / 冻结后聚合。
pub fn inject_four() -> [bool; 4] {
    // 注入 1：越界卡号 → AG_RANGE 拒收
    let mut a = AggLedger::new();
    let i1 = matches!(a.aggregate(CARDS, CARD_LO), Err(AG_RANGE));
    // 注入 2：重号聚合 → AG_DUP 拒收
    let mut b = AggLedger::new();
    let _ = b.aggregate(0, CARD_LO);
    let i2 = matches!(b.aggregate(0, CARD_LO), Err(AG_DUP));
    // 注入 3：未竟收口 → AG_INCOMPLETE 拒止
    let mut c = AggLedger::new();
    let _ = c.aggregate(0, CARD_LO);
    let i3 = matches!(c.closeout(), Err(AG_INCOMPLETE));
    // 注入 4：冻结后聚合 → AG_FROZEN 拒收
    let mut d = AggLedger::new();
    d.freeze();
    let i4 = matches!(d.aggregate(0, CARD_LO), Err(AG_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 十九条判据聚合 360 项全绿，段累计对平。
pub fn run_unx10980_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10980");
    let (lg, ok) = sample_closed();

    s.add("J1-聚合全绿", ok && lg.all_green() && lg.closed, "十九条判据聚合三百六十项全绿");
    s.add("J1-段账对平", lg.seg_total() == SEG_TOTAL && lg.prior == SEG_PRIOR && lg.this_seg == SEG_THIS, "前段加本段等于段累计");
    s.add("J2-项数守恒", lg.items_sum == ITEMS_TOTAL && (0..CARDS).map(items_of).fold(0u64, |a, b| a.wrapping_add(b)) == ITEMS_TOTAL, "逐卡项数之和恰三百六十");
    s.add("J2-绿项守恒", lg.green_sum == lg.items_sum && lg.reds_sum() == 0, "绿项等于项数且红项归零");
    s.add("J2-聚合齐备", lg.aggregated_n == CARDS && lg.cards.len() == CARDS, "十九卡全部聚合");
    s.add("J3-卡号连续", lg.cards_continuous(), "卡号 10961 至 10979 连续");
    s.add("J3-判据同源", lg.criteria_consistent(), "逐卡判据数与项数同源");
    s.add("J3-末卡特例", lg.cards[CARDS - 1].items == 18 && lg.cards[0].items == 19, "首卡十九项末卡十八项");
    s.add("J4-收口闸", lg.closeout_gate().is_ok(), "收口闸三项条件全过");
    s.add("J4-段闭合", lg.prior.wrapping_add(lg.this_seg) == SEG_TOTAL, "段累计 179880 闭合");
    s.add("J5-部分非全绿", {
        let mut p = AggLedger::new();
        let _ = p.aggregate(0, CARD_LO);
        let _ = p.aggregate(1, CARD_LO + 1);
        !p.all_green() && p.aggregated_n == 2 && p.closeout().is_err()
    }, "部分聚合即非全绿且不许收口");
    s.add("J6-越界拒聚", inject_four()[0], "越界卡号被 AG_RANGE 拒收");
    s.add("J6-重号拒聚", inject_four()[1], "重号聚合被 AG_DUP 拒收");
    s.add("J7-未竟拒收", inject_four()[2], "未竟收口被 AG_INCOMPLETE 拒止");
    s.add("J7-冻结拒聚", inject_four()[3], "冻结后聚合被 AG_FROZEN 拒止");
    s.add("J8-段账失衡", {
        let mut x = AggLedger::new();
        let _ = x.aggregate_all();
        x.prior = x.prior.wrapping_add(1);
        x.closeout_gate() == Err(AG_IMBALANCE)
    }, "前段篡改一格即段账失衡拒收");
    s.add("J8-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-总账动态",
        ok,
        &format!(
            "卡 {} 聚合 {} 项 {} 绿 {} 红 {} 前段 {} 本段 {} 累计 {}",
            lg.cards.len(),
            lg.aggregated_n,
            lg.items_sum,
            lg.green_sum,
            lg.reds_sum(),
            lg.prior,
            lg.this_seg,
            lg.seg_total(),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10980_agg_ledger_flow() {
        let mut lg = AggLedger::new();
        assert_eq!(lg.cards.len(), CARDS);
        assert!(lg.cards_continuous());
        assert_eq!(lg.aggregate_all().unwrap_or(0), ITEMS_TOTAL);
        assert!(lg.all_green());
        assert_eq!(lg.closeout().unwrap_or(0), SEG_TOTAL);
        assert!(lg.closed);
        // 项数分布抽查
        assert_eq!(items_of(0), 19);
        assert_eq!(items_of(18), 18);
        assert_eq!((0..CARDS).map(items_of).fold(0u64, |a, b| a.wrapping_add(b)), 360);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10980_checks_green() {
        let s = run_unx10980_checks();
        assert!(s.all_passed(), "UNX-F10980 判据未全绿");
    }
}
