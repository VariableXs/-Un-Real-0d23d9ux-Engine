//! UNX-F11176 移交接收账（下游消费声明）（0x1f4c0 段）
//!
//! 模型：一次移交拆成「交付」与「消费声明」两本半账。交付侧逐件登记件号、
//! 字节数与受领方；声明侧登记受领方、所领件号、领取额与接收位。规则如下：
//! 交付件号一至十且严格递增，字节非零，受领方在一至三；声明必须指向已交付
//! 的件，受领方须与交付侧一致，接收位必须为真，领取额须等于该件字节，且
//! 同一件只能被声明一次。零悬空即：无指向未交付件的声明、无重复声明、
//! 无未声明件。收交付与收声明各记受理；越界、重复、倒序、零字节、悬空、
//! 未接收位与领取额不符各记拒止。终核要求十件交付与十件声明一一对平，
//! 折叠后冻结；二次冻结返回冻结码。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 下游接收账）。
const SEG_BASE: u32 = 0x1f4c0;

/// 件号或受领方越界。
pub const RX_ENO: u32 = SEG_BASE + 0x01;
/// 件号重复（交付或声明）。
pub const RX_EDUP: u32 = SEG_BASE + 0x02;
/// 交付件号倒序。
pub const RX_EORDER: u32 = SEG_BASE + 0x03;
/// 交付字节为零。
pub const RX_EBYTES: u32 = SEG_BASE + 0x04;
/// 声明指向未交付件或受领方不符。
pub const RX_EDANGLING: u32 = SEG_BASE + 0x05;
/// 接收位未亮。
pub const RX_EACK: u32 = SEG_BASE + 0x06;
/// 领取额不符。
pub const RX_EAMOUNT: u32 = SEG_BASE + 0x07;
/// 两账不齐。
pub const RX_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const RX_EFROZEN: u32 = SEG_BASE + 0x09;

/// 件数。
pub const ITEMS: usize = 10;
/// 受领方数。
pub const CONSUMERS: u8 = 3;
/// 单件字节基数。
pub const BYTES_BASE: u32 = 256;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一件交付。
#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    pub id: u8,
    pub bytes: u32,
    pub consumer: u8,
}

impl Delivery {
    /// 空交付。
    pub const fn blank() -> Delivery {
        Delivery {
            id: 0,
            bytes: 0,
            consumer: 0,
        }
    }

    /// 件号一至十、字节非零、受领方在一至三。
    pub fn ok(&self) -> bool {
        self.id >= 1
            && self.id as usize <= ITEMS
            && self.bytes >= 1
            && self.consumer >= 1
            && self.consumer <= CONSUMERS
    }
}

/// 一条消费声明。
#[derive(Clone, Copy, Debug)]
pub struct Claim {
    pub consumer: u8,
    pub item: u8,
    pub amount: u32,
    pub acked: bool,
}

impl Claim {
    /// 空声明。
    pub const fn blank() -> Claim {
        Claim {
            consumer: 0,
            item: 0,
            amount: 0,
            acked: false,
        }
    }

    /// 受领方在一至三、件号一至十、接收位已亮。
    pub fn label_ok(&self) -> bool {
        self.consumer >= 1
            && self.consumer <= CONSUMERS
            && self.item >= 1
            && self.item as usize <= ITEMS
            && self.acked
    }
}

/// 交付与声明两本半账。
#[derive(Clone, Copy, Debug)]
pub struct ReceiptLedger {
    deliveries: [Delivery; ITEMS],
    claims: [Claim; ITEMS],
    n_del: usize,
    n_claim: usize,
    accepts: u32,
    rejects: u32,
    frozen: bool,
    settled: Option<u64>,
}

impl ReceiptLedger {
    /// 空账。
    pub const fn new() -> ReceiptLedger {
        ReceiptLedger {
            deliveries: [Delivery::blank(); ITEMS],
            claims: [Claim::blank(); ITEMS],
            n_del: 0,
            n_claim: 0,
            accepts: 0,
            rejects: 0,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已交付件数。
    pub fn delivered(&self) -> usize {
        self.n_del
    }

    /// 已声明件数。
    pub fn claimed(&self) -> usize {
        self.n_claim
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 第 i 件交付（交付序）。
    pub fn delivery_at(&self, i: usize) -> Option<Delivery> {
        if i >= self.n_del {
            return None;
        }
        self.deliveries.get(i).copied()
    }

    /// 第 i 条声明（声明序）。
    pub fn claim_at(&self, i: usize) -> Option<Claim> {
        if i >= self.n_claim {
            return None;
        }
        self.claims.get(i).copied()
    }

    /// 按件号查交付。
    pub fn find_delivery(&self, id: u8) -> Option<Delivery> {
        let mut i = 0usize;
        while i < self.n_del {
            if let Some(d) = self.deliveries.get(i) {
                if d.id == id {
                    return Some(*d);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 按件号查声明。
    pub fn find_claim(&self, item: u8) -> Option<Claim> {
        let mut i = 0usize;
        while i < self.n_claim {
            if let Some(c) = self.claims.get(i) {
                if c.item == item {
                    return Some(*c);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收交付：冻结 → 件号受领方 → 字节 → 重复 → 倒序。
    pub fn register(&mut self, d: Delivery) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RX_EFROZEN));
        }
        if d.id == 0
            || d.id as usize > ITEMS
            || d.consumer == 0
            || d.consumer > CONSUMERS
        {
            return Err(self.fail(RX_ENO));
        }
        if d.bytes == 0 {
            return Err(self.fail(RX_EBYTES));
        }
        if self.find_delivery(d.id).is_some() {
            return Err(self.fail(RX_EDUP));
        }
        if self.n_del > 0 {
            if let Some(last) = self.deliveries.get(self.n_del - 1) {
                if d.id <= last.id {
                    return Err(self.fail(RX_EORDER));
                }
            }
        }
        match self.deliveries.get_mut(self.n_del) {
            Some(slot) => *slot = d,
            None => return Err(self.fail(RX_ECOUNT)),
        }
        self.n_del = self.n_del.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 收声明：冻结 → 范围 → 接收位 → 悬空 → 重复 → 领取额。
    pub fn register_claim(&mut self, c: Claim) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RX_EFROZEN));
        }
        if c.consumer == 0
            || c.consumer > CONSUMERS
            || c.item == 0
            || c.item as usize > ITEMS
        {
            return Err(self.fail(RX_ENO));
        }
        if !c.acked {
            return Err(self.fail(RX_EACK));
        }
        let d = match self.find_delivery(c.item) {
            Some(v) => v,
            None => return Err(self.fail(RX_EDANGLING)),
        };
        if d.consumer != c.consumer {
            return Err(self.fail(RX_EDANGLING));
        }
        if self.find_claim(c.item).is_some() {
            return Err(self.fail(RX_EDUP));
        }
        if c.amount != d.bytes {
            return Err(self.fail(RX_EAMOUNT));
        }
        match self.claims.get_mut(self.n_claim) {
            Some(slot) => *slot = c,
            None => return Err(self.fail(RX_ECOUNT)),
        }
        self.n_claim = self.n_claim.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 交付字节合计。
    pub fn delivered_bytes(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n_del {
            if let Some(d) = self.deliveries.get(i) {
                sum = sum.wrapping_add(d.bytes);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 领取额合计。
    pub fn claimed_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n_claim {
            if let Some(c) = self.claims.get(i) {
                sum = sum.wrapping_add(c.amount);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 接收位全亮。
    pub fn all_acked(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_claim {
            match self.claims.get(i) {
                Some(c) => {
                    if !c.acked {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 逐件领取额等于交付字节。
    pub fn amounts_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_claim {
            match self.claims.get(i) {
                Some(c) => match self.find_delivery(c.item) {
                    Some(d) => {
                        if c.amount != d.bytes {
                            return false;
                        }
                    }
                    None => return false,
                },
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 零悬空：每条声明都指向已交付件且受领方一致。
    pub fn no_dangling(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_claim {
            match self.claims.get(i) {
                Some(c) => match self.find_delivery(c.item) {
                    Some(d) => {
                        if d.consumer != c.consumer {
                            return false;
                        }
                    }
                    None => return false,
                },
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 无孤儿交付：每件交付都被声明。
    pub fn no_orphans(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_del {
            match self.deliveries.get(i) {
                Some(d) => {
                    if self.find_claim(d.id).is_none() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 声明按件号互异。
    pub fn claims_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_claim {
            let a = match self.claims.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let mut j = i.wrapping_add(1);
            while j < self.n_claim {
                match self.claims.get(j) {
                    Some(b) => {
                        if a.item == b.item {
                            return false;
                        }
                    }
                    None => return false,
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 两账一一对平。
    pub fn balanced(&self) -> bool {
        self.n_del == ITEMS
            && self.n_claim == ITEMS
            && self.all_acked()
            && self.amounts_ok()
            && self.no_dangling()
            && self.no_orphans()
            && self.claims_unique()
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n_del {
            if let Some(d) = self.deliveries.get(i) {
                h = (h ^ d.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ d.bytes as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ d.consumer as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.n_claim {
            if let Some(c) = self.claims.get(j) {
                h = (h ^ c.consumer as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.item as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.amount as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ if c.acked { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h = (h ^ self.n_del as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.n_claim as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：十件交付、十件声明、零悬空零孤儿、额度全对平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.n_del != ITEMS {
            return Err(RX_ECOUNT);
        }
        let mut i = 0usize;
        while i < self.n_del {
            let d = match self.deliveries.get(i) {
                Some(v) => *v,
                None => return Err(RX_ENO),
            };
            if d.id as usize != i.wrapping_add(1) {
                return Err(RX_EORDER);
            }
            if !d.ok() {
                return Err(RX_EBYTES);
            }
            i = i.wrapping_add(1);
        }
        if self.n_claim != ITEMS {
            return Err(RX_ECOUNT);
        }
        if !self.no_dangling() {
            return Err(RX_EDANGLING);
        }
        if !self.claims_unique() {
            return Err(RX_EDUP);
        }
        if !self.all_acked() {
            return Err(RX_EACK);
        }
        if !self.amounts_ok() {
            return Err(RX_EAMOUNT);
        }
        if !self.no_orphans() {
            return Err(RX_ECOUNT);
        }
        if self.delivered_bytes() != self.claimed_total() {
            return Err(RX_EAMOUNT);
        }
        Ok(self.fold())
    }

    /// 冻结：两账对平才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RX_EFROZEN));
        }
        let v = match self.replay() {
            Ok(v) => v,
            Err(e) => return Err(self.fail(e)),
        };
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
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

/// 第 id 件规格：字节与受领方。
pub fn spec_of(id: usize) -> (u32, u8) {
    if id == 0 || id > ITEMS {
        return (0, 0);
    }
    let bytes = BYTES_BASE.wrapping_add((id as u32) * 32);
    let consumer = ((id % CONSUMERS as usize) as u8).wrapping_add(1);
    (bytes, consumer)
}

/// 规格交付。
pub fn delivery_spec(id: u8) -> Delivery {
    let mut d = Delivery::blank();
    let (bytes, consumer) = spec_of(id as usize);
    d.id = id;
    d.bytes = bytes;
    d.consumer = consumer;
    d
}

/// 规格声明：按交付件全额领取并亮接收位。
pub fn claim_spec(id: u8) -> Claim {
    let mut c = Claim::blank();
    let d = delivery_spec(id);
    c.consumer = d.consumer;
    c.item = id;
    c.amount = d.bytes;
    c.acked = true;
    c
}

/// 构建标准接收账：先十件交付，再逆序十件声明（声明序不受限）。
pub fn build_ledger() -> ReceiptLedger {
    let mut l = ReceiptLedger::new();
    let mut id = 1u8;
    while id <= ITEMS as u8 {
        let _ = l.register(delivery_spec(id));
        id = id.wrapping_add(1);
    }
    let mut k = ITEMS as u8;
    while k >= 1 {
        let _ = l.register_claim(claim_spec(k));
        if k == 1 {
            break;
        }
        k = k.wrapping_sub(1);
    }
    l
}

/// 四探针：越界、重复/倒序、悬空/未接收/额度、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = ReceiptLedger::new();
    let _ = a.register(delivery_spec(1));
    let mut nocons = delivery_spec(2);
    nocons.consumer = 0;
    let mut far = delivery_spec(1);
    far.id = (ITEMS + 1) as u8;
    if a.register(nocons) == Err(RX_ENO) && a.register(far) == Err(RX_ENO) {
        r[0] = true;
    }

    let mut b = ReceiptLedger::new();
    let d1 = delivery_spec(1);
    let _ = b.register(d1);
    if b.register(d1) == Err(RX_EDUP) {
        r[1] = true;
    }
    let mut back = ReceiptLedger::new();
    let _ = back.register(delivery_spec(2));
    if back.register(delivery_spec(1)) != Err(RX_EORDER) {
        r[1] = false;
    }

    let mut c = ReceiptLedger::new();
    let _ = c.register(delivery_spec(1));
    let mut dead = claim_spec(3);
    dead.consumer = spec_of(3).1;
    let mut mute = claim_spec(1);
    mute.acked = false;
    let mut over = claim_spec(1);
    over.amount = over.amount.wrapping_add(1);
    if c.register_claim(dead) == Err(RX_EDANGLING)
        && c.register_claim(mute) == Err(RX_EACK)
        && c.register_claim(over) == Err(RX_EAMOUNT)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(RX_EFROZEN)
        && d.register(delivery_spec(1)) == Err(RX_EFROZEN)
        && d.register_claim(claim_spec(1)) == Err(RX_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11176");
    let base = build_ledger();

    let mut ids = true;
    let mut i = 0usize;
    while i < base.delivered() {
        match base.delivery_at(i) {
            Some(d) => {
                if d.id as usize != i.wrapping_add(1) {
                    ids = false;
                }
            }
            None => ids = false,
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "S1-交付逐件对平",
        base.delivered() == ITEMS
            && base.accepted() == (ITEMS as u32) * 2
            && base.rejected() == 0
            && ids,
        "十件交付加十条声明全收，交付件号连续",
    );

    s.add(
        "S2-接收位逐件对平",
        base.claimed() == ITEMS && base.all_acked(),
        "十件声明齐且接收位全亮",
    );

    s.add(
        "S3-领取额逐件对平",
        base.amounts_ok() && base.delivered_bytes() == base.claimed_total(),
        "逐件领取额等于交付字节，两账合计相等",
    );

    s.add(
        "S4-零悬空",
        base.no_dangling() && base.no_orphans(),
        "无指向未交付件的声明，也无未声明的交付",
    );

    s.add(
        "S5-声明互异",
        base.claims_unique(),
        "十件声明按件号互不重复",
    );

    let mut pairs = true;
    let mut k = 0usize;
    while k < base.delivered() {
        match base.delivery_at(k) {
            Some(d) => match base.find_claim(d.id) {
                Some(c) => {
                    if c.consumer != d.consumer || c.amount != d.bytes || !c.acked {
                        pairs = false;
                    }
                }
                None => pairs = false,
            },
            None => pairs = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S6-受领方对平", pairs, "每件交付的受领方与领取额均与声明一致");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(RX_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放两账逐件对平且指纹一致",
    );

    let mut spec_bytes = 0u32;
    let mut id = 1usize;
    while id <= ITEMS {
        spec_bytes = spec_bytes.wrapping_add(spec_of(id).0);
        id = id.wrapping_add(1);
    }
    s.add(
        "S10-合计对平规格",
        base.delivered_bytes() == spec_bytes && base.claimed_total() == spec_bytes,
        "交付与领取两侧合计均等于规格字节合计",
    );

    let mut la = ReceiptLedger::new();
    let _ = la.register(delivery_spec(1));
    let mut nocons = delivery_spec(2);
    nocons.consumer = 0;
    let mut far = delivery_spec(1);
    far.id = (ITEMS + 1) as u8;
    let r0 = la.register(nocons);
    let r1 = la.register(far);
    s.add(
        "X11-越界拒收",
        r0 == Err(RX_ENO) && r1 == Err(RX_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "受领方零与件号十一均判越界",
    );

    let mut lb = ReceiptLedger::new();
    let d1 = delivery_spec(1);
    let _ = lb.register(d1);
    let dup = lb.register(d1);
    let mut lc = ReceiptLedger::new();
    let _ = lc.register(delivery_spec(2));
    let ord = lc.register(delivery_spec(1));
    s.add(
        "X12-重复倒序",
        dup == Err(RX_EDUP) && ord == Err(RX_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重交一件判重复、二件后补一件判倒序",
    );

    let mut ld = ReceiptLedger::new();
    let _ = ld.register(delivery_spec(1));
    let mut dead = claim_spec(3);
    dead.consumer = spec_of(3).1;
    let mut mute = claim_spec(1);
    mute.acked = false;
    let mut over = claim_spec(1);
    over.amount = over.amount.wrapping_add(1);
    let dg = ld.register_claim(dead);
    let ak = ld.register_claim(mute);
    let am = ld.register_claim(over);
    s.add(
        "X13-悬空未接额度",
        dg == Err(RX_EDANGLING)
            && ak == Err(RX_EACK)
            && am == Err(RX_EAMOUNT)
            && ld.accepted() == 1
            && ld.rejected() == 3,
        "悬空声明、未亮接收位、领取超额分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11176_receipt_flow() {
        let l = build_ledger();
        assert_eq!(l.delivered(), ITEMS);
        assert_eq!(l.claimed(), ITEMS);
        assert_eq!(l.accepted(), (ITEMS as u32) * 2);
        assert_eq!(l.rejected(), 0);
        assert!(l.balanced());
        assert!(l.no_dangling());
        assert!(l.no_orphans());
        assert!(l.all_acked());
        assert!(l.amounts_ok());
        assert_eq!(l.delivered_bytes(), l.claimed_total());
        assert_eq!(l.delivery_at(0).map(|d| d.id), Some(1));
        assert!(l.delivery_at(ITEMS).is_none());
        assert!(l.claim_at(0).map(|c| c.item) == Some(ITEMS as u8));
        assert!(l.find_claim(5).is_some());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11176_spec_claims() {
        assert_eq!(spec_of(0), (0, 0));
        assert_eq!(spec_of(ITEMS + 1), (0, 0));
        let d = delivery_spec(1);
        assert_eq!(d.bytes, BYTES_BASE + 32);
        assert!(d.ok());
        let c = claim_spec(1);
        assert_eq!(c.amount, d.bytes);
        assert_eq!(c.consumer, d.consumer);
        assert!(c.label_ok());
        let mut sum = 0u32;
        let mut id = 1usize;
        while id <= ITEMS {
            sum = sum.wrapping_add(spec_of(id).0);
            id = id.wrapping_add(1);
        }
        assert_eq!(build_ledger().delivered_bytes(), sum);
    }

    #[test]
    fn f11176_gates() {
        let mut a = ReceiptLedger::new();
        assert!(a.register(delivery_spec(1)).is_ok());
        assert_eq!(a.register(delivery_spec(1)), Err(RX_EDUP));
        let mut zero = delivery_spec(2);
        zero.bytes = 0;
        assert_eq!(a.register(zero), Err(RX_EBYTES));
        let mut nocons = delivery_spec(2);
        nocons.consumer = CONSUMERS + 1;
        assert_eq!(a.register(nocons), Err(RX_ENO));
        let mut order = ReceiptLedger::new();
        assert!(order.register(delivery_spec(2)).is_ok());
        assert_eq!(order.register(delivery_spec(1)), Err(RX_EORDER));
        assert_eq!(a.accepted(), 1);
        assert_eq!(a.rejected(), 3);
    }

    #[test]
    fn f11176_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(RX_EFROZEN));
        assert_eq!(l.register(delivery_spec(1)), Err(RX_EFROZEN));
        assert_eq!(l.register_claim(claim_spec(1)), Err(RX_EFROZEN));
    }

    #[test]
    fn f11176_checks_green() {
        let s = run_unx11176_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
