//! UNX-F11167 风险台账终版（R-C4-001/002/003 终态落账）（0x1f430 段）
//!
//! 纯功能模型：域风险台账终版对 R-C4-001、R-C4-002、R-C4-003 三条风险逐条
//! 落终态。终态口径为全部结清、责任人在册、结清证据非空；终版收口时零在途
//! 断言必须成立，锁值后不得再改态或补记。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f430;

/// 风险号越界。
pub const RK_ENO: u32 = SEG_BASE + 0x01;
/// 风险号重复。
pub const RK_EDUP: u32 = SEG_BASE + 0x02;
/// 风险号倒序。
pub const RK_EORDER: u32 = SEG_BASE + 0x03;
/// 终态值越出三态。
pub const RK_ESTATE: u32 = SEG_BASE + 0x04;
/// 责任人越出责任域（形状违规）。
pub const RK_EOWNER: u32 = SEG_BASE + 0x05;
/// 结清证据缺失。
pub const RK_EEVID: u32 = SEG_BASE + 0x06;
/// 台账未闭合（在途或不全）。
pub const RK_EOPEN: u32 = SEG_BASE + 0x07;
/// 终值已锁定。
pub const RK_EFROZEN: u32 = SEG_BASE + 0x08;

/// 风险条数。
pub const RISKS: usize = 3;
/// 终态：结清。
pub const ST_CLOSED: u8 = 0;
/// 终态：在途。
pub const ST_OPEN: u8 = 1;
/// 终态：豁免。
pub const ST_WAIVED: u8 = 2;
/// 三态宽度。
pub const STATES: u8 = 3;
/// 责任人数。
pub const OWNERS: u8 = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条风险。
#[derive(Clone, Copy, Debug)]
pub struct RiskItem {
    pub id: u8,
    pub state: u8,
    pub owner: u8,
    pub evidence: u8,
}

impl RiskItem {
    /// 终态值在域内。
    pub fn state_ok(&self) -> bool {
        self.state < STATES
    }

    /// 责任人在域内。
    pub fn owner_ok(&self) -> bool {
        self.owner < OWNERS
    }

    /// 是否结清。
    pub fn closed(&self) -> bool {
        self.state == ST_CLOSED
    }

    /// 结清证据是否留痕。
    pub fn evidenced(&self) -> bool {
        self.evidence > 0
    }
}

/// 风险台账。
#[derive(Clone, Debug)]
pub struct RiskLedger {
    items: Vec<RiskItem>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl RiskLedger {
    /// 空账。
    pub fn new() -> RiskLedger {
        RiskLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已落条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按号取风险。
    pub fn item(&self, id: usize) -> Option<RiskItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 登记一条：冻结 → 越界 → 重记 → 倒序 → 终态 → 责任 → 证据。
    pub fn record(&mut self, id: usize, state: u8, owner: u8, evidence: u8) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RK_EFROZEN);
        }
        if id == 0 || id > RISKS {
            return self.fail(RK_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(RK_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(RK_EORDER);
            }
        }
        let it = RiskItem {
            id: id as u8,
            state,
            owner,
            evidence,
        };
        if !it.state_ok() {
            return self.fail(RK_ESTATE);
        }
        if !it.owner_ok() {
            return self.fail(RK_EOWNER);
        }
        if it.closed() && !it.evidenced() {
            return self.fail(RK_EEVID);
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

    /// 缺号表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 1usize;
        while id <= RISKS {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 风险号严格递增。
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

    /// 在途（未结清）条数。
    pub fn open_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].closed() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 结清且证据留痕条数。
    pub fn closed_evidenced(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].closed() && self.items[i].evidenced() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
}

impl RiskLedger {
    /// 台账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.state as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.owner as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.evidence as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终版：三条齐 → 零在途 → 结清留痕。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(RK_EFROZEN);
        }
        if self.items.len() != RISKS || !self.gaps().is_empty() || !self.order_strict() {
            return self.fail(RK_EOPEN);
        }
        if self.open_count() != 0 {
            return self.fail(RK_EOPEN);
        }
        if self.closed_evidenced() != RISKS as u32 {
            return self.fail(RK_EEVID);
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

/// 构建标准风险台账：三条全结清、责任人轮转、证据各一。
pub fn build_final() -> RiskLedger {
    let mut l = RiskLedger::new();
    let mut id = 1usize;
    while id <= RISKS {
        let owner = ((id - 1) % OWNERS as usize) as u8;
        let _ = l.record(id, ST_CLOSED, owner, 1);
        id += 1;
    }
    let _ = l.settle();
    l
}

/// 四探针：越界、重复/倒序、终态/责任/证据违例、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = RiskLedger::new();
    let _ = a.record(1, ST_CLOSED, 0, 1);
    if a.record(0, ST_CLOSED, 0, 1) == Err(RK_ENO)
        && a.record(RISKS + 1, ST_CLOSED, 0, 1) == Err(RK_ENO)
    {
        r[0] = true;
    }

    let mut b = RiskLedger::new();
    let _ = b.record(2, ST_CLOSED, 0, 1);
    if b.record(1, ST_CLOSED, 0, 1) == Err(RK_EORDER)
        && b.record(2, ST_CLOSED, 0, 1) == Err(RK_EDUP)
    {
        r[1] = true;
    }

    let mut c = RiskLedger::new();
    let _ = c.record(1, ST_CLOSED, 0, 1);
    if c.record(2, 7, 0, 1) == Err(RK_ESTATE)
        && c.record(2, ST_CLOSED, 7, 1) == Err(RK_EOWNER)
        && c.record(2, ST_CLOSED, 0, 0) == Err(RK_EEVID)
    {
        r[2] = true;
    }

    let mut d = build_final();
    if d.is_frozen()
        && d.record(RISKS + 1, ST_CLOSED, 0, 1) == Err(RK_EFROZEN)
        && d.settle() == Err(RK_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11167");

    let base = build_final();
    s.add(
        "X1-终版落账",
        base.len() == RISKS && base.accepts == 3 && base.rejects == 0 && base.is_frozen(),
        "三条全收、零拒收且已锁定",
    );
    s.add(
        "X2-风险号连续",
        base.gaps().is_empty() && base.order_strict(),
        "R-C4-001/002/003 无缺且严格递增",
    );
    s.add(
        "X3-零在途",
        base.open_count() == 0,
        "终版在途风险计数为零",
    );
    s.add(
        "X4-结清留痕",
        base.closed_evidenced() == 3,
        "三条结清均带证据",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = RiskLedger::new();
    let _ = fl.record(1, ST_CLOSED, 0, 1);
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复倒序/终态责任证据/冻结后写四码可达",
    );

    let mut lr = RiskLedger::new();
    let _ = lr.record(1, ST_CLOSED, 0, 1);
    let r0 = lr.record(0, ST_CLOSED, 0, 1);
    let r1 = lr.record(RISKS + 1, ST_CLOSED, 0, 1);
    s.add(
        "X8-越界拒收",
        r0 == Err(RK_ENO) && r1 == Err(RK_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "风险号 0 与 4 均判越界",
    );

    let mut lo = RiskLedger::new();
    let _ = lo.record(2, ST_CLOSED, 0, 1);
    let o = lo.record(1, ST_CLOSED, 0, 1);
    let d = lo.record(2, ST_CLOSED, 0, 1);
    s.add(
        "X9-倒序与重复",
        o == Err(RK_EORDER) && d == Err(RK_EDUP) && lo.accepts == 1 && lo.rejects == 2,
        "2 后写 1 判倒序、重写 2 判重复",
    );

    let mut lv = RiskLedger::new();
    let st = lv.record(1, 7, 0, 1);
    let ow = lv.record(1, ST_CLOSED, 7, 1);
    let ev = lv.record(1, ST_CLOSED, 0, 0);
    s.add(
        "X10-终态责任证据",
        st == Err(RK_ESTATE)
            && ow == Err(RK_EOWNER)
            && ev == Err(RK_EEVID)
            && lv.accepts == 0
            && lv.rejects == 3,
        "终态越界、责任人越界、证据缺失分别拒收",
    );

    let mut lop = RiskLedger::new();
    let _ = lop.record(1, ST_CLOSED, 0, 1);
    let _ = lop.record(2, ST_OPEN, 0, 0);
    let _ = lop.record(3, ST_CLOSED, 2, 1);
    let eo = lop.settle();
    s.add(
        "X11-零在途断言",
        eo == Err(RK_EOPEN) && lop.open_count() == 1 && lop.accepts == 3 && lop.rejects == 1,
        "一条在途即判未闭合",
    );

    let mut lsh = RiskLedger::new();
    let _ = lsh.record(1, ST_CLOSED, 0, 1);
    let _ = lsh.record(2, ST_CLOSED, 1, 1);
    let es = lsh.settle();
    s.add(
        "X12-条数不全",
        es == Err(RK_EOPEN) && lsh.len() == 2 && lsh.rejects == 1,
        "缺第三条即判未闭合",
    );

    let mut z = build_final();
    let zf = z.freeze();
    let ez = z.settle();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && ez == Err(RK_EFROZEN) && z.accepts == 3,
        &format!("acc={} rej={} settled={}", z.accepts, z.rejects, zf.is_some()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11167_flow() {
        let mut l = build_final();
        assert_eq!(l.len(), 3);
        assert_eq!(l.open_count(), 0);
        assert_eq!(l.closed_evidenced(), 3);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.is_frozen());
        assert_eq!((l.accepts, l.rejects), (3, 0));
        let v = l.settled_value();
        assert!(v.is_some());
        assert_eq!(l.freeze(), v);
        assert_eq!(l.freeze(), v);
        assert_eq!(l.item(1).map(|i| i.owner), Some(0));
        assert_eq!(l.item(2).map(|i| i.owner), Some(1));
        assert_eq!(l.item(3).map(|i| i.owner), Some(2));
        assert!(l.item(4).is_none());
        assert!(l.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11167_gates() {
        let mut a = RiskLedger::new();
        assert!(a.record(2, ST_CLOSED, 0, 1).is_ok());
        assert_eq!(a.record(1, ST_CLOSED, 0, 1), Err(RK_EORDER));
        assert_eq!(a.record(2, ST_CLOSED, 0, 1), Err(RK_EDUP));
        assert_eq!((a.accepts, a.rejects), (1, 2));

        let mut b = RiskLedger::new();
        assert_eq!(b.record(1, 7, 0, 1), Err(RK_ESTATE));
        assert_eq!(b.record(1, ST_CLOSED, 7, 1), Err(RK_EOWNER));
        assert_eq!(b.record(1, ST_CLOSED, 0, 0), Err(RK_EEVID));
        assert_eq!(b.record(0, ST_CLOSED, 0, 1), Err(RK_ENO));
        assert_eq!(b.record(RISKS + 1, ST_CLOSED, 0, 1), Err(RK_ENO));
        assert_eq!((b.accepts, b.rejects), (0, 5));

        let mut c = RiskLedger::new();
        assert!(c.record(1, ST_CLOSED, 0, 1).is_ok());
        assert!(c.record(2, ST_CLOSED, 1, 1).is_ok());
        assert_eq!((c.accepts, c.rejects), (2, 0));
        assert_eq!(c.settle(), Err(RK_EOPEN));
        assert_eq!((c.accepts, c.rejects), (2, 1));

        let mut d = RiskLedger::new();
        assert!(d.record(1, ST_CLOSED, 0, 1).is_ok());
        assert!(d.record(2, ST_OPEN, 0, 0).is_ok());
        assert!(d.record(3, ST_CLOSED, 2, 1).is_ok());
        assert_eq!(d.open_count(), 1);
        assert_eq!(d.settle(), Err(RK_EOPEN));
        assert_eq!((d.accepts, d.rejects), (3, 1));

        let mut e = build_final();
        assert_eq!(e.settle(), Err(RK_EFROZEN));
        assert_eq!(e.record(RISKS + 1, ST_CLOSED, 0, 1), Err(RK_EFROZEN));
        assert_eq!((e.accepts, e.rejects), (3, 2));
    }

    #[test]
    fn f11167_checks_green() {
        let s = run_unx11167_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
