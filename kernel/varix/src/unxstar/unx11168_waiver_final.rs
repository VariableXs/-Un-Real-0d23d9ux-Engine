//! UNX-F11168 豁免账终版（域豁免清单终核）（0x1f440 段）
//!
//! 纯功能模型：域豁免账终版逐例登记六条豁免，例号连续、类别与范围在域、
//! 权重一或二。终核以六十条判据为分母折算豁免率（千分比），率值必须压在
//! 闸线一百‰以内；锁值后不得再登记或重核。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f440;

/// 豁免例号越界。
pub const WA_ENO: u32 = SEG_BASE + 0x01;
/// 豁免例号重复。
pub const WA_EDUP: u32 = SEG_BASE + 0x02;
/// 豁免例号倒序。
pub const WA_EORDER: u32 = SEG_BASE + 0x03;
/// 类别或范围越界（形状违规）。
pub const WA_EKIND: u32 = SEG_BASE + 0x04;
/// 豁免权重越界。
pub const WA_EWEIGHT: u32 = SEG_BASE + 0x05;
/// 豁免例数不齐。
pub const WA_ECOUNT: u32 = SEG_BASE + 0x06;
/// 豁免率越出闸线。
pub const WA_ERATE: u32 = SEG_BASE + 0x07;
/// 终值已锁定。
pub const WA_EFROZEN: u32 = SEG_BASE + 0x08;

/// 豁免例数。
pub const WAIVERS: usize = 6;
/// 判据总分母。
pub const CRITERIA: u32 = 60;
/// 类别数。
pub const KINDS: u8 = 3;
/// 范围数。
pub const SCOPES: u8 = 4;
/// 权重上限。
pub const MAX_WEIGHT: u8 = 2;
/// 豁免率闸线（千分比）。
pub const MAX_RATE: u32 = 100;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条豁免。
#[derive(Clone, Copy, Debug)]
pub struct WaiverItem {
    pub id: u8,
    pub kind: u8,
    pub scope: u8,
    pub weight: u8,
}

impl WaiverItem {
    /// 类别与范围均在域内。
    pub fn kind_ok(&self) -> bool {
        self.kind < KINDS && self.scope < SCOPES
    }

    /// 权重为一或二。
    pub fn weight_ok(&self) -> bool {
        self.weight >= 1 && self.weight <= MAX_WEIGHT
    }
}

/// 豁免账。
#[derive(Clone, Debug)]
pub struct WaiverLedger {
    items: Vec<WaiverItem>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl WaiverLedger {
    /// 空账。
    pub fn new() -> WaiverLedger {
        WaiverLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已登例数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按例号取豁免。
    pub fn item(&self, id: usize) -> Option<WaiverItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 登记一例：冻结 → 越界 → 重记 → 倒序 → 类别范围 → 权重。
    pub fn record(&mut self, id: usize, kind: u8, scope: u8, weight: u8) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(WA_EFROZEN);
        }
        if id == 0 || id > WAIVERS {
            return self.fail(WA_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(WA_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(WA_EORDER);
            }
        }
        let it = WaiverItem {
            id: id as u8,
            kind,
            scope,
            weight,
        };
        if !it.kind_ok() {
            return self.fail(WA_EKIND);
        }
        if !it.weight_ok() {
            return self.fail(WA_EWEIGHT);
        }
        self.items.push(it);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 缺例表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 1usize;
        while id <= WAIVERS {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 例号严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].id <= self.items[i - 1].id {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 权重总和。
    pub fn weight_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].weight as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 豁免率（千分比，以六十条判据为分母）。
    pub fn rate_per_mille(&self) -> u32 {
        self.weight_sum()
            .wrapping_mul(1000)
            .wrapping_div(CRITERIA)
    }

    /// 豁免率是否在闸内。
    pub fn rate_within_gate(&self) -> bool {
        self.rate_per_mille() <= MAX_RATE
    }
}

impl WaiverLedger {
    /// 豁免账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.kind as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.scope as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.weight as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.weight_sum() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终核：六例齐 → 豁免率闸内 → 锁值。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(WA_EFROZEN);
        }
        if self.items.len() != WAIVERS || !self.gaps().is_empty() || !self.order_strict() {
            return self.fail(WA_ECOUNT);
        }
        if !self.rate_within_gate() {
            return self.fail(WA_ERATE);
        }
        let v = self.fold();
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 构建标准豁免账：六例、类别范围轮转、权重各一，率恰一百‰。
pub fn build_final() -> WaiverLedger {
    let mut l = WaiverLedger::new();
    let mut id = 1usize;
    while id <= WAIVERS {
        let kind = (id % KINDS as usize) as u8;
        let scope = (id % SCOPES as usize) as u8;
        let _ = l.record(id, kind, scope, 1);
        id += 1;
    }
    let _ = l.settle();
    l
}

/// 四探针：越界、重复/倒序、类别权重违例、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = WaiverLedger::new();
    let _ = a.record(1, 0, 0, 1);
    if a.record(0, 0, 0, 1) == Err(WA_ENO)
        && a.record(WAIVERS + 1, 0, 0, 1) == Err(WA_ENO)
    {
        r[0] = true;
    }

    let mut b = WaiverLedger::new();
    let _ = b.record(2, 0, 0, 1);
    if b.record(1, 0, 0, 1) == Err(WA_EORDER)
        && b.record(2, 0, 0, 1) == Err(WA_EDUP)
    {
        r[1] = true;
    }

    let mut c = WaiverLedger::new();
    let _ = c.record(1, 0, 0, 1);
    if c.record(2, 3, 0, 1) == Err(WA_EKIND)
        && c.record(2, 0, SCOPES, 1) == Err(WA_EKIND)
        && c.record(2, 0, 0, 3) == Err(WA_EWEIGHT)
    {
        r[2] = true;
    }

    let mut d = build_final();
    if d.is_frozen()
        && d.record(WAIVERS + 1, 0, 0, 1) == Err(WA_EFROZEN)
        && d.settle() == Err(WA_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11168");

    let base = build_final();
    s.add(
        "X1-终版构建",
        base.len() == WAIVERS && base.accepts == 6 && base.rejects == 0 && base.is_frozen(),
        "六例全收、零拒收且已锁定",
    );
    s.add_dyn(
        "X2-豁免率闸线",
        base.rate_per_mille() == MAX_RATE && base.rate_within_gate(),
        &format!("rate={} max={}", base.rate_per_mille(), MAX_RATE),
    );
    s.add(
        "X3-例号连续",
        base.gaps().is_empty() && base.order_strict(),
        "例号 1..=6 无缺且严格递增",
    );
    s.add(
        "X4-权重口径",
        base.weight_sum() == 6,
        "六例权重各一，合计为六",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = WaiverLedger::new();
    let _ = fl.record(1, 0, 0, 1);
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复倒序/类别权重/冻结后写四码可达",
    );

    let mut lr = WaiverLedger::new();
    let _ = lr.record(1, 0, 0, 1);
    let r0 = lr.record(0, 0, 0, 1);
    let r1 = lr.record(WAIVERS + 1, 0, 0, 1);
    s.add(
        "X8-越界拒收",
        r0 == Err(WA_ENO) && r1 == Err(WA_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "例号 0 与 7 均判越界",
    );

    let mut lo = WaiverLedger::new();
    let _ = lo.record(2, 0, 0, 1);
    let o = lo.record(1, 0, 0, 1);
    let d = lo.record(2, 0, 0, 1);
    s.add(
        "X9-倒序与重复",
        o == Err(WA_EORDER) && d == Err(WA_EDUP) && lo.accepts == 1 && lo.rejects == 2,
        "2 后写 1 判倒序、重写 2 判重复",
    );

    let mut lv = WaiverLedger::new();
    let k = lv.record(1, 3, 0, 1);
    let sc = lv.record(1, 0, SCOPES, 1);
    let w = lv.record(1, 0, 0, 3);
    s.add(
        "X10-类别与权重",
        k == Err(WA_EKIND)
            && sc == Err(WA_EKIND)
            && w == Err(WA_EWEIGHT)
            && lv.accepts == 0
            && lv.rejects == 3,
        "类别越界、范围越界、权重越界分别拒收",
    );

    let mut lc = WaiverLedger::new();
    let mut id = 1usize;
    while id < WAIVERS {
        let _ = lc.record(id, 0, 0, 1);
        id += 1;
    }
    let ec = lc.settle();
    s.add(
        "X11-例数不全",
        ec == Err(WA_ECOUNT) && lc.len() == 5 && lc.rejects == 1,
        "缺第六例即判例数不齐",
    );

    let mut lrate = WaiverLedger::new();
    let mut j = 1usize;
    while j <= WAIVERS {
        let weight = if j == WAIVERS { 2 } else { 1 };
        let _ = lrate.record(j, 0, 0, weight);
        j += 1;
    }
    let er = lrate.settle();
    s.add(
        "X12-率越闸线",
        er == Err(WA_ERATE)
            && lrate.weight_sum() == 7
            && lrate.rate_per_mille() == 116
            && lrate.rejects == 1,
        "权重七即率 116‰ 越闸",
    );

    let mut z = build_final();
    let zf = z.freeze();
    let ez = z.settle();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && ez == Err(WA_EFROZEN) && z.accepts == 6,
        &format!("acc={} rej={} rate={}", z.accepts, z.rejects, z.rate_per_mille()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11168_flow() {
        let mut l = build_final();
        assert_eq!(l.len(), 6);
        assert_eq!(l.weight_sum(), 6);
        assert_eq!(l.rate_per_mille(), 100);
        assert!(l.rate_within_gate());
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.is_frozen());
        assert_eq!((l.accepts, l.rejects), (6, 0));
        let v = l.settled_value();
        assert!(v.is_some());
        assert_eq!(l.freeze(), v);
        assert_eq!(l.freeze(), v);
        assert_eq!(l.item(1).map(|i| i.kind), Some(1));
        assert_eq!(l.item(6).map(|i| i.scope), Some(2));
        assert!(l.item(7).is_none());
        assert!(l.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11168_gates() {
        let mut a = WaiverLedger::new();
        assert!(a.record(2, 0, 0, 1).is_ok());
        assert_eq!(a.record(1, 0, 0, 1), Err(WA_EORDER));
        assert_eq!(a.record(2, 0, 0, 1), Err(WA_EDUP));
        assert_eq!((a.accepts, a.rejects), (1, 2));

        let mut b = WaiverLedger::new();
        assert!(b.record(1, 0, 0, 1).is_ok());
        assert_eq!(b.record(2, 3, 0, 1), Err(WA_EKIND));
        assert_eq!(b.record(2, 0, SCOPES, 1), Err(WA_EKIND));
        assert_eq!(b.record(2, 0, 0, 3), Err(WA_EWEIGHT));
        assert_eq!(b.record(0, 0, 0, 1), Err(WA_ENO));
        assert_eq!(b.record(WAIVERS + 1, 0, 0, 1), Err(WA_ENO));
        assert_eq!((b.accepts, b.rejects), (1, 5));

        let mut c = WaiverLedger::new();
        let mut id = 1usize;
        while id < WAIVERS {
            assert!(c.record(id, 0, 0, 1).is_ok());
            id += 1;
        }
        assert_eq!(c.settle(), Err(WA_ECOUNT));
        assert_eq!((c.accepts, c.rejects), (5, 1));

        let mut d = WaiverLedger::new();
        let mut j = 1usize;
        while j <= WAIVERS {
            let weight = if j == WAIVERS { 2 } else { 1 };
            assert!(d.record(j, 0, 0, weight).is_ok());
            j += 1;
        }
        assert_eq!(d.weight_sum(), 7);
        assert_eq!(d.settle(), Err(WA_ERATE));
        assert_eq!((d.accepts, d.rejects), (6, 1));

        let mut e = build_final();
        assert_eq!(e.settle(), Err(WA_EFROZEN));
        assert_eq!(e.record(WAIVERS + 1, 0, 0, 1), Err(WA_EFROZEN));
        assert_eq!((e.accepts, e.rejects), (6, 2));
    }

    #[test]
    fn f11168_checks_green() {
        let s = run_unx11168_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
