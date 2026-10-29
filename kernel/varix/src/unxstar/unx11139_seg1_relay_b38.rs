//! UNX-F11139 清账段一接力账（向 B38 段二接力）（0x1f270 段）
//!
//! 纯功能模型：段一（B37）收口后把二十张卡的账接力给段二（B38）。接力
//! 不是整本搬走，而是逐项交付、逐项接收、逐项回执：
//!   交（hand）→ 接（receive）→ 回执（ack）→ 收口（close）。
//! 一项只许交一次、接一次、回执一次；交接量与承接量必须逐位相等；
//! 接收方不得抢接未交付项。
//!
//! 项内基线：一张卡一卡、十道判据、不少于 260 行。二十项合计必须对平
//! 段一总账的三个数：20 卡 / 200 判据 / 6400 行——这正是「接力断言」：
//! 段二期初 = 段一期末，三项逐项相等，在途为零。
//!
//! 拒止面：项号越界、项序倒挂、卡号与项号不挂接、交接量不合基线、重复
//! 交接、未交接即接收、重复接收、承接与交接不符、未回执即收口、合计
//! 不平、在途未清、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f270;
/// 项号或卡号越界
pub const RL_NO: u32 = SEG_BASE + 0x01;
/// 项序倒挂或卡号与项号不挂接
pub const RL_ORDER: u32 = SEG_BASE + 0x02;
/// 交接量不合项内基线
pub const RL_CARRY: u32 = SEG_BASE + 0x03;
/// 未交接即接收、未交接即收口
pub const RL_SHORT: u32 = SEG_BASE + 0x04;
/// 承接与交接不符
pub const RL_MISMATCH: u32 = SEG_BASE + 0x05;
/// 重复交接或重复接收
pub const RL_DUP: u32 = SEG_BASE + 0x06;
/// 未回执即收口
pub const RL_ACK: u32 = SEG_BASE + 0x07;
/// 合计不平
pub const RL_SUM: u32 = SEG_BASE + 0x08;
/// 在途未清
pub const RL_INFLIGHT: u32 = SEG_BASE + 0x09;
/// 接力账已封盘
pub const RL_FROZEN: u32 = SEG_BASE + 0x0a;

/// 接力项数（段一二十张卡）。
pub const ITEMS: usize = 20;
/// 数位数：卡 / 判据 / 行。
pub const THREE: usize = 3;
/// 卡号基底（F11100 起）。
pub const CARD_BASE: u32 = 11100;
/// 每项卡数。
pub const ITEM_CARDS: u32 = 1;
/// 每项判据数。
pub const ITEM_CRITS: u32 = 10;
/// 每项行数。
pub const ITEM_LINES: u32 = 320;
/// 每项行数地板。
pub const LINE_MIN: u32 = 260;
/// 合计：卡。
pub const TOTAL_CARDS: u32 = ITEMS as u32 * ITEM_CARDS;
/// 合计：判据。
pub const TOTAL_CRITS: u32 = ITEMS as u32 * ITEM_CRITS;
/// 合计：行。
pub const TOTAL_LINES: u32 = ITEMS as u32 * ITEM_LINES;
/// 段二期初应接合计（三项）。
pub const OPENING: [u32; THREE] = [TOTAL_CARDS, TOTAL_CRITS, TOTAL_LINES];
/// 接力方向。
pub const FROM_SEG: &str = "B37";
/// 接力去向。
pub const TO_SEG: &str = "B38";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 项内基线是否成立：一卡、十判据、行数不下地板。
pub fn carry_ok(cards: u32, crits: u32, lines: u32) -> bool {
    cards == ITEM_CARDS && crits == ITEM_CRITS && lines >= LINE_MIN
}

/// 卡号是否与项号挂接（第 n 项 = F11100 + n - 1）。
pub fn card_of(no: usize) -> u32 {
    CARD_BASE.wrapping_add(no.wrapping_sub(1) as u32)
}

/// 卡号反查项号；不在接力范围内返回 0。
pub fn item_of(card: u32) -> usize {
    if card < CARD_BASE {
        return 0;
    }
    let no = card.wrapping_sub(CARD_BASE).wrapping_add(1) as usize;
    if no > ITEMS {
        return 0;
    }
    no
}

/// 三项逐位相等。
pub fn triple_eq(a: &[u32; THREE], b: &[u32; THREE]) -> bool {
    let mut i = 0usize;
    while i < THREE {
        let (x, y) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (*x, *y),
            _ => return false,
        };
        if x != y {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 一项接力账。
#[derive(Clone, Copy, Debug)]
pub struct RelayItem {
    /// 项号（1..=20）。
    pub no: usize,
    /// 卡号（F11100 + 项号）。
    pub card: u32,
    carry: [u32; THREE],
    recv: [u32; THREE],
    has_carry: bool,
    has_recv: bool,
    acked: bool,
    closed: bool,
}

impl RelayItem {
    /// 空白项。
    pub const fn new(no: usize) -> RelayItem {
        RelayItem {
            no,
            card: 0,
            carry: [0u32; THREE],
            recv: [0u32; THREE],
            has_carry: false,
            has_recv: false,
            acked: false,
            closed: false,
        }
    }

    /// 交接：卡号须与项号挂接，量须合基线。
    pub fn hand(&mut self, card: u32, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.has_carry || self.closed {
            return Err(RL_DUP);
        }
        if card != card_of(self.no) {
            return Err(RL_ORDER);
        }
        if !carry_ok(cards, crits, lines) {
            return Err(RL_CARRY);
        }
        self.card = card;
        self.carry = [cards, crits, lines];
        self.has_carry = true;
        Ok(())
    }

    /// 接收：须先交接，且三项逐位等于交接量。
    pub fn receive(&mut self, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.has_recv {
            return Err(RL_DUP);
        }
        if !self.has_carry {
            return Err(RL_SHORT);
        }
        if self.closed {
            return Err(RL_DUP);
        }
        let got = [cards, crits, lines];
        if !triple_eq(&got, &self.carry) {
            return Err(RL_MISMATCH);
        }
        self.recv = got;
        self.has_recv = true;
        Ok(())
    }

    /// 回执。
    pub fn ack(&mut self) -> Result<(), u32> {
        if !self.has_recv {
            return Err(RL_SHORT);
        }
        if self.acked {
            return Err(RL_DUP);
        }
        self.acked = true;
        Ok(())
    }

    /// 项收口：交、接、回执三者齐，且逐位相等。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if !self.has_carry {
            return Err(RL_SHORT);
        }
        if !self.has_recv {
            return Err(RL_SHORT);
        }
        if !self.acked {
            return Err(RL_ACK);
        }
        if !triple_eq(&self.carry, &self.recv) {
            return Err(RL_MISMATCH);
        }
        self.closed = true;
        Ok(())
    }

    /// 是否已交接。
    pub fn handed(&self) -> bool {
        self.has_carry
    }

    /// 是否已接收。
    pub fn received(&self) -> bool {
        self.has_recv
    }

    /// 是否已回执。
    pub fn acked(&self) -> bool {
        self.acked
    }

    /// 是否收口。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 交接量。
    pub fn carry(&self) -> [u32; THREE] {
        self.carry
    }

    /// 承接量。
    pub fn recv(&self) -> [u32; THREE] {
        self.recv
    }

    /// 是否在途（交而未收口）。
    pub fn inflight(&self) -> bool {
        self.has_carry && !self.closed
    }

    /// 逐位对平。
    pub fn balanced(&self) -> bool {
        self.has_carry && self.has_recv && triple_eq(&self.carry, &self.recv)
    }

    /// 项指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.card as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < THREE {
            if let (Some(c), Some(r)) = (self.carry.get(i), self.recv.get(i)) {
                h ^= *c as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= *r as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h ^= if self.acked { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.closed { 1 } else { 0 };
        h
    }
}

/// 段一→段二接力总账。
#[derive(Clone, Debug)]
pub struct RelayLedger {
    /// 段内基址。
    pub base: u32,
    items: [RelayItem; ITEMS],
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl RelayLedger {
    /// 新建二十个空白项。
    pub fn new(base: u32) -> RelayLedger {
        let mut items = [RelayItem::new(1); ITEMS];
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = items.get_mut(i) {
                *x = RelayItem::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        RelayLedger {
            base,
            items,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 项只读。
    pub fn item(&self, no: usize) -> Option<RelayItem> {
        if no == 0 || no > ITEMS {
            return None;
        }
        self.items.get(no - 1).copied()
    }

    /// 交接（项序须紧接游标）。
    pub fn hand(&mut self, no: usize, card: u32, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(RL_FROZEN));
        }
        if no == 0 || no > ITEMS {
            return Err(self.fail(RL_NO));
        }
        if let Some(x) = self.items.get(no - 1) {
            if x.handed() {
                return Err(self.fail(RL_DUP));
            }
        }
        if no != self.cursor.wrapping_add(1) {
            return Err(self.fail(RL_ORDER));
        }
        let r = match self.items.get_mut(no - 1) {
            Some(x) => x.hand(card, cards, crits, lines),
            None => Err(RL_NO),
        };
        match r {
            Ok(()) => {
                self.cursor = no;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 接收。
    pub fn receive(&mut self, no: usize, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(RL_FROZEN));
        }
        if no == 0 || no > ITEMS {
            return Err(self.fail(RL_NO));
        }
        let r = match self.items.get_mut(no - 1) {
            Some(x) => x.receive(cards, crits, lines),
            None => Err(RL_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 回执。
    pub fn ack(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(RL_FROZEN));
        }
        if no == 0 || no > ITEMS {
            return Err(self.fail(RL_NO));
        }
        let r = match self.items.get_mut(no - 1) {
            Some(x) => x.ack(),
            None => Err(RL_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 项收口。
    pub fn close(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(RL_FROZEN));
        }
        if no == 0 || no > ITEMS {
            return Err(self.fail(RL_NO));
        }
        let r = match self.items.get_mut(no - 1) {
            Some(x) => x.close(),
            None => Err(RL_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 某数位合计：[交接合计, 承接合计, 项数]。
    pub fn sums(&self, idx: usize) -> [u32; 3] {
        let mut out = [0u32; 3];
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = self.items.get(i) {
                if let Some(c) = x.carry.get(idx) {
                    out[0] = out[0].wrapping_add(*c);
                }
                if let Some(r) = x.recv.get(idx) {
                    out[1] = out[1].wrapping_add(*r);
                }
                out[2] = out[2].wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 段二期初三项（承接合计）。
    pub fn opening(&self) -> [u32; THREE] {
        let mut out = [0u32; THREE];
        let mut i = 0usize;
        while i < THREE {
            if let Some(slot) = out.get_mut(i) {
                *slot = match self.sums(i).get(1) {
                    Some(v) => *v,
                    None => 0,
                };
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 段一期末三项（交接合计）。
    pub fn closing(&self) -> [u32; THREE] {
        let mut out = [0u32; THREE];
        let mut i = 0usize;
        while i < THREE {
            if let Some(slot) = out.get_mut(i) {
                *slot = match self.sums(i).get(0) {
                    Some(v) => *v,
                    None => 0,
                };
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 接力断言：段二期初 = 段一期末 = 应接合计。
    pub fn relay_assertion(&self) -> bool {
        let o = self.opening();
        let c = self.closing();
        triple_eq(&o, &OPENING) && triple_eq(&c, &OPENING) && triple_eq(&o, &c)
    }

    /// 已收口项数。
    pub fn closed_items(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = self.items.get(i) {
                if x.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 在途项数。
    pub fn inflight(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = self.items.get(i) {
                if x.inflight() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已回执项数。
    pub fn acked_items(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = self.items.get(i) {
                if x.acked() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= if self.frozen { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.cursor as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < ITEMS {
            if let Some(x) = self.items.get(i) {
                h ^= x.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 封盘，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 终重放：零在途、二十项收口、接力断言成立，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.inflight() != 0 {
            return Err(self.fail(RL_INFLIGHT));
        }
        if self.closed_items() != ITEMS {
            return Err(self.fail(RL_SUM));
        }
        if !self.relay_assertion() {
            return Err(self.fail(RL_SUM));
        }
        self.freeze()
    }
}

/// 搭建接力账：二十项交接、接收、回执、收口，最后终重放。
pub fn build_relay(base: u32) -> (RelayLedger, Result<u64, u32>) {
    let mut g = RelayLedger::new(base);
    let mut no = 1usize;
    while no <= ITEMS {
        let card = card_of(no);
        if g.hand(no, card, ITEM_CARDS, ITEM_CRITS, ITEM_LINES).is_err() {
            return (g, Err(RL_CARRY));
        }
        if g.receive(no, ITEM_CARDS, ITEM_CRITS, ITEM_LINES).is_err() {
            return (g, Err(RL_MISMATCH));
        }
        if g.ack(no).is_err() {
            return (g, Err(RL_ACK));
        }
        if g.close(no).is_err() {
            return (g, Err(RL_SHORT));
        }
        no = no.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：卡号不挂接、基线不合、承接不符、在途未清。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 卡号与项号不挂接落 RL_ORDER。
    let mut a = RelayLedger::new(0x3b00);
    out[0] = a.hand(1, card_of(2), 1, 10, 320).err() == Some(RL_ORDER) && a.rejects() == 1;

    // ② 判据数不合基线落 RL_CARRY。
    let mut b = RelayLedger::new(0x3b01);
    out[1] = b.hand(1, card_of(1), 1, 9, 320).err() == Some(RL_CARRY) && b.rejects() == 1;

    // ③ 承接与交接不符落 RL_MISMATCH。
    let mut c = RelayLedger::new(0x3b02);
    let ok3 = c.hand(1, card_of(1), 1, 10, 320).is_ok();
    out[2] = ok3 && c.receive(1, 1, 10, 300).err() == Some(RL_MISMATCH) && c.rejects() == 1;

    // ④ 二十项交接齐、十九项回执，终重放因在途落 RL_INFLIGHT。
    let mut d = RelayLedger::new(0x3b03);
    let mut no = 1usize;
    let mut ok4 = true;
    while no <= ITEMS {
        if d.hand(no, card_of(no), ITEM_CARDS, ITEM_CRITS, ITEM_LINES).is_err() {
            ok4 = false;
        }
        if d.receive(no, ITEM_CARDS, ITEM_CRITS, ITEM_LINES).is_err() {
            ok4 = false;
        }
        if no < ITEMS && d.ack(no).is_err() {
            ok4 = false;
        }
        no = no.wrapping_add(1);
    }
    out[3] = ok4 && d.replay().err() == Some(RL_INFLIGHT);

    out
}

/// 判据集：F1~F13。
pub fn run_unx11139_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11139.seg1_relay_b38");
    let (g, r) = build_relay(0x1139);

    // F1 常量面。
    let c1 = SEG_BASE == 0x1f270
        && ITEMS == 20
        && THREE == 3
        && CARD_BASE == 11100
        && card_of(1) == 11100
        && card_of(ITEMS) == 11119
        && item_of(11100) == 1
        && item_of(11119) == ITEMS
        && item_of(0) == 0
        && OPENING == [20, 200, 6400]
        && FROM_SEG == "B37"
        && TO_SEG == "B38";
    s.add("F1-常量面", c1, "二十项对 F11100–F11119，应接 20/200/6400");

    // F2 二十项交接齐。
    let mut c2 = 0usize;
    let mut i2 = 1usize;
    while i2 <= ITEMS {
        if let Some(x) = g.item(i2) {
            if x.handed() && x.card == card_of(i2) {
                c2 = c2.wrapping_add(1);
            }
        }
        i2 = i2.wrapping_add(1);
    }
    s.add_dyn("F2-交接齐", c2 == ITEMS, &format!("handed={}/{}", c2, ITEMS));

    // F3 接收齐、回执齐。
    s.add_dyn(
        "F3-接收回执",
        g.acked_items() == ITEMS,
        &format!("acked={}/{}", g.acked_items(), ITEMS),
    );

    // F4 逐项对平 20/20。
    let mut c4 = 0usize;
    let mut i4 = 1usize;
    while i4 <= ITEMS {
        if let Some(x) = g.item(i4) {
            if x.balanced() && x.closed() {
                c4 = c4.wrapping_add(1);
            }
        }
        i4 = i4.wrapping_add(1);
    }
    s.add_dyn("F4-逐项对平", c4 == ITEMS, &format!("balanced={}/{}", c4, ITEMS));

    // F5 接力断言：期初 = 期末 = 应接。
    s.add_dyn(
        "F5-接力断言",
        g.relay_assertion() && triple_eq(&g.opening(), &OPENING) && triple_eq(&g.closing(), &OPENING),
        &format!("open={:?} close={:?}", g.opening(), g.closing()),
    );

    // F6 三项合计对平。
    let c6 = g.sums(0)[0] == TOTAL_CARDS
        && g.sums(1)[0] == TOTAL_CRITS
        && g.sums(2)[0] == TOTAL_LINES;
    s.add_dyn(
        "F6-三项合计",
        c6,
        &format!("{:?}/{:?}/{:?}", g.sums(0), g.sums(1), g.sums(2)),
    );

    // F7 零在途。
    s.add_dyn("F7-零在途", g.inflight() == 0, &format!("inflight={}", g.inflight()));

    // F8 终重放成立并封盘。
    s.add("F8-接力封盘", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // F9 重放与封盘幂等。
    let mut a9 = g.clone();
    let before = a9.fold();
    let again = a9.replay();
    let mut b9 = RelayLedger::new(0x1139);
    let f1 = b9.freeze();
    let f2 = b9.freeze();
    s.add(
        "F9-幂等",
        again.is_ok() && a9.fold() == before && f1.is_ok() && f1 == f2,
        "重放二次同指纹；封盘二次同指纹",
    );

    // F10 拒止优先序。
    let mut c10 = g.clone();
    let z1 = c10.hand(ITEMS + 1, 0, 1, 10, 320).err();
    let mut c10b = RelayLedger::new(0x113a);
    let z2 = c10b.hand(ITEMS + 1, 0, 1, 10, 320).err();
    s.add(
        "F10-拒止优先序",
        z1 == Some(RL_FROZEN) && z2 == Some(RL_NO),
        "封盘优先于项号；未封盘则 RL_NO",
    );

    // F11 四路注入。
    let inj = inject_four();
    s.add(
        "F11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "卡号/基线/承接/在途四路各自发火",
    );

    // F12 指纹灵敏度。
    let (g2, _) = build_relay(0x113b);
    let (g3, _) = build_relay(0x1139);
    s.add(
        "F12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址相异、同基址相同",
    );

    // F13 逐例归零。
    s.add(
        "F13-逐例归零",
        g.rejects() == 0 && g.closed_items() == ITEMS && g.relay_assertion(),
        "rejects=0，二十项全闭，接力断言成立",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11139_flow() {
        let (g, r) = build_relay(0x8000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.closed_items(), ITEMS);
        assert_eq!(g.inflight(), 0);
        assert_eq!(g.acked_items(), ITEMS);
        assert_eq!(g.rejects(), 0);
        assert_eq!(g.opening(), OPENING);
        assert_eq!(g.closing(), OPENING);
        assert!(g.relay_assertion());
        assert_eq!(g.sums(0)[0], TOTAL_CARDS);
        assert_eq!(g.sums(1)[0], TOTAL_CRITS);
        assert_eq!(g.sums(2)[0], TOTAL_LINES);
        let first = match g.item(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(first.card, 11100);
        assert!(first.closed() && first.balanced());
    }

    #[test]
    fn f11139_gates() {
        let mut g = RelayLedger::new(0x8001);
        assert_eq!(g.hand(1, card_of(1), 1, 9, 320).err(), Some(RL_CARRY));
        assert_eq!(g.hand(1, card_of(2), 1, 10, 320).err(), Some(RL_ORDER));
        assert!(g.hand(1, card_of(1), 1, 10, 320).is_ok());
        assert_eq!(g.hand(1, card_of(1), 1, 10, 320).err(), Some(RL_DUP));
        assert_eq!(g.receive(2, 1, 10, 320).err(), Some(RL_SHORT));
        assert_eq!(g.receive(1, 2, 10, 320).err(), Some(RL_MISMATCH));
        assert_eq!(g.ack(1).err(), Some(RL_SHORT));
        assert!(g.receive(1, 1, 10, 320).is_ok());
        assert_eq!(g.close(1).err(), Some(RL_ACK));
        assert!(g.ack(1).is_ok());
        assert!(g.close(1).is_ok());
        assert_eq!(g.receive(1, 1, 10, 320).err(), Some(RL_DUP));
        assert_eq!(g.rejects(), 8);
        assert!(g.freeze().is_ok());
        assert_eq!(g.hand(2, card_of(2), 1, 10, 320).err(), Some(RL_FROZEN));
        assert_eq!(g.rejects(), 9);
    }

    #[test]
    fn f11139_rules() {
        assert!(carry_ok(1, 10, 260));
        assert!(!carry_ok(2, 10, 320));
        assert!(!carry_ok(1, 11, 320));
        assert!(!carry_ok(1, 10, 259));
        assert_eq!(card_of(1), 11100);
        assert_eq!(card_of(20), 11119);
        assert_eq!(item_of(11100), 1);
        assert_eq!(item_of(11119), 20);
        assert_eq!(item_of(11120), 0);
        assert_eq!(item_of(0), 0);
        let a = [20u32, 200, 6400];
        let b = [20u32, 200, 6400];
        let c = [20u32, 200, 6399];
        assert!(triple_eq(&a, &b));
        assert!(!triple_eq(&a, &c));
        // 项状态机：未交接即接收/回执拒。
        let mut it = RelayItem::new(1);
        assert_eq!(it.receive(1, 10, 320).err(), Some(RL_SHORT));
        assert_eq!(it.ack().err(), Some(RL_SHORT));
        assert_eq!(it.close().err(), Some(RL_SHORT));
        assert!(it.hand(11100, 1, 10, 320).is_ok());
        assert!(!it.acked());
        assert!(it.inflight());
        assert_eq!(it.hand(11100, 1, 10, 320).err(), Some(RL_DUP));
    }

    #[test]
    fn f11139_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11139_checks_green() {
        let cs = run_unx11139_checks();
        assert!(cs.all_passed(), "checks red in unx11139");
    }
}
