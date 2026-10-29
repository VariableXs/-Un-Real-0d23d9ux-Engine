//! UNX-F11140 B37 批收口（清账一段总断言）（0x1f280 段）
//!
//! 纯功能模型：B37 批是一段清账的最后一个批次，收口时不看单卡而看批：
//! 十九道判据（J1~J19）分挂六张卡（F11134~F11139），每道判据终重放三轮，
//! 三轮全过才许闭；再叠两条总断言：
//!   一、行数守恒——十九道判据的声明行数合计等于批行数 6420，重放行数
//!       合计又必须等于声明合计，一格不许挪；
//!   二、域累计对平——按卡分组，每张卡名下判据的行数合计等于该卡在册
//!       负载（4/3/3/3/3/3），六卡相加仍回 6420。
//!
//! 判据号与卡的挂接是硬约束：J1~J4 属 11134、J5~J7 属 11135、J8~J10 属
//! 11136、J11~J13 属 11137、J14~J16 属 11138、J17~J19 属 11139。挂错卡
//! 当场拒止，不许「先记后改」。
//!
//! 拒止面：判据号越界、卡挂接错位、判据号倒挂、行数低于地板、轮次未满、
//! 判据有失败、行数守恒不成立、域累计不平、终重放缺账、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f280;
/// 判据号越界
pub const BC_NO: u32 = SEG_BASE + 0x01;
/// 判据挂错卡
pub const BC_CARD: u32 = SEG_BASE + 0x02;
/// 判据号倒挂或重复登记
pub const BC_ORDER: u32 = SEG_BASE + 0x03;
/// 行数低于地板
pub const BC_LINES: u32 = SEG_BASE + 0x04;
/// 轮次未满即收口
pub const BC_ROUNDS: u32 = SEG_BASE + 0x05;
/// 判据有失败轮
pub const BC_FAIL: u32 = SEG_BASE + 0x06;
/// 行数守恒不成立
pub const BC_SUM: u32 = SEG_BASE + 0x07;
/// 域累计不平
pub const BC_DOMAIN: u32 = SEG_BASE + 0x08;
/// 终重放缺账
pub const BC_REPLAY: u32 = SEG_BASE + 0x09;
/// 批账已封盘
pub const BC_FROZEN: u32 = SEG_BASE + 0x0a;

/// 批内判据数。
pub const CRITS: usize = 19;
/// 每判据重放轮数。
pub const ROUNDS: usize = 3;
/// 批卡数。
pub const CARDS: usize = 6;
/// 批卡号。
pub const CARD_IDS: [u32; CARDS] = [11134, 11135, 11136, 11137, 11138, 11139];
/// 每卡判据负载。
pub const CARD_LOAD: [u32; CARDS] = [4, 3, 3, 3, 3, 3];
/// 每卡行数累计（与判据行数表同源）。
pub const CARD_ACC: [u32; CARDS] = [1320, 1000, 1080, 960, 1040, 1020];
/// 批行数合计。
pub const LINE_TOTAL: u32 = 6420;
/// 判据行数地板。
pub const LINE_MIN: u32 = 260;
/// 判据行数表（十九道，合计 6420）。
pub const DECL_LINES: [u32; CRITS] = [
    300, 320, 340, 360, 380, 300, 320, 340, 360, 380, 300, 320, 340, 360, 380, 300, 320, 340, 360,
];
/// 判据名。
pub const CRIT_NAMES: [&str; CRITS] = [
    "J1", "J2", "J3", "J4", "J5", "J6", "J7", "J8", "J9", "J10", "J11", "J12", "J13", "J14", "J15",
    "J16", "J17", "J18", "J19",
];
/// 批名。
pub const BATCH: &str = "B37";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 判据号所挂的卡。
pub fn card_for(no: usize) -> u32 {
    if no == 0 || no > CRITS {
        return 0;
    }
    let mut acc = 0usize;
    let mut c = 0usize;
    while c < CARDS {
        let load = match CARD_LOAD.get(c) {
            Some(v) => *v as usize,
            None => 0,
        };
        if no <= acc.wrapping_add(load) {
            return match CARD_IDS.get(c) {
                Some(v) => *v,
                None => 0,
            };
        }
        acc = acc.wrapping_add(load);
        c = c.wrapping_add(1);
    }
    0
}

/// 判据号所在的卡序（0..6）。
pub fn card_slot(no: usize) -> usize {
    if no == 0 || no > CRITS {
        return CARDS;
    }
    let mut acc = 0usize;
    let mut c = 0usize;
    while c < CARDS {
        let load = match CARD_LOAD.get(c) {
            Some(v) => *v as usize,
            None => 0,
        };
        if no <= acc.wrapping_add(load) {
            return c;
        }
        acc = acc.wrapping_add(load);
        c = c.wrapping_add(1);
    }
    CARDS
}

/// 声明行数表合计。
pub fn decl_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CRITS {
        if let Some(v) = DECL_LINES.get(i) {
            n = n.wrapping_add(*v);
        }
        i = i.wrapping_add(1);
    }
    n
}

/// 卡负载合计。
pub fn load_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CARDS {
        if let Some(v) = CARD_LOAD.get(i) {
            n = n.wrapping_add(*v);
        }
        i = i.wrapping_add(1);
    }
    n
}

/// 判据记录。
#[derive(Clone, Copy, Debug)]
pub struct CritRec {
    /// 判据号（1..=19）。
    pub no: usize,
    /// 所挂卡号。
    pub card: u32,
    declared: u32,
    replayed_lines: u32,
    rounds: [u8; ROUNDS],
    replayed: usize,
    fails: usize,
    has_decl: bool,
    closed: bool,
}

impl CritRec {
    /// 空白判据。
    pub const fn new(no: usize) -> CritRec {
        CritRec {
            no,
            card: 0,
            declared: 0,
            replayed_lines: 0,
            rounds: [0u8; ROUNDS],
            replayed: 0,
            fails: 0,
            has_decl: false,
            closed: false,
        }
    }

    /// 立判据账：卡须挂接、行数须下地板。
    pub fn declare(&mut self, card: u32, lines: u32) -> Result<(), u32> {
        if self.has_decl || self.closed {
            return Err(BC_ORDER);
        }
        if card != card_for(self.no) {
            return Err(BC_CARD);
        }
        if lines < LINE_MIN {
            return Err(BC_LINES);
        }
        self.card = card;
        self.declared = lines;
        self.has_decl = true;
        Ok(())
    }

    /// 单轮重放：只收通过/失败；行数随轮记入。
    pub fn replay(&mut self, verdict: u8, lines: u32) -> Result<(), u32> {
        if !self.has_decl || self.closed {
            return Err(BC_ORDER);
        }
        if self.replayed >= ROUNDS {
            return Err(BC_ROUNDS);
        }
        if verdict != 1 && verdict != 2 {
            return Err(BC_FAIL);
        }
        if lines > self.declared {
            return Err(BC_SUM);
        }
        if let Some(slot) = self.rounds.get_mut(self.replayed) {
            *slot = verdict;
        }
        self.replayed = self.replayed.wrapping_add(1);
        if verdict == 2 {
            self.fails = self.fails.wrapping_add(1);
        }
        self.replayed_lines = self.replayed_lines.wrapping_add(lines);
        Ok(())
    }

    /// 收口：三轮走满、零失败。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if self.replayed != ROUNDS {
            return Err(BC_ROUNDS);
        }
        if self.fails != 0 {
            return Err(BC_FAIL);
        }
        self.closed = true;
        Ok(())
    }

    /// 是否已立账。
    pub fn declared(&self) -> bool {
        self.has_decl
    }

    /// 声明行数。
    pub fn declared_lines(&self) -> u32 {
        self.declared
    }

    /// 重放行数合计。
    pub fn replayed_lines(&self) -> u32 {
        self.replayed_lines
    }

    /// 已重放轮数。
    pub fn replayed(&self) -> usize {
        self.replayed
    }

    /// 失败轮数。
    pub fn fails(&self) -> usize {
        self.fails
    }

    /// 是否收口。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 判据指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.card as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.declared as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.replayed_lines as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.fails as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.closed { 1 } else { 0 };
        h
    }
}

/// B37 批账。
#[derive(Clone, Debug)]
pub struct BatchLedger {
    /// 段内基址。
    pub base: u32,
    crits: [CritRec; CRITS],
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl BatchLedger {
    /// 新建十九道空白判据。
    pub fn new(base: u32) -> BatchLedger {
        let mut crits = [CritRec::new(1); CRITS];
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = crits.get_mut(i) {
                *c = CritRec::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        BatchLedger {
            base,
            crits,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 判据只读。
    pub fn crit(&self, no: usize) -> Option<CritRec> {
        if no == 0 || no > CRITS {
            return None;
        }
        self.crits.get(no - 1).copied()
    }

    /// 立判据账（判据号须紧接游标）。
    pub fn declare(&mut self, no: usize, card: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(BC_FROZEN));
        }
        if no == 0 || no > CRITS {
            return Err(self.fail(BC_NO));
        }
        if let Some(c) = self.crits.get(no - 1) {
            if c.declared() {
                return Err(self.fail(BC_ORDER));
            }
        }
        if no != self.cursor.wrapping_add(1) {
            return Err(self.fail(BC_ORDER));
        }
        let r = match self.crits.get_mut(no - 1) {
            Some(c) => c.declare(card, lines),
            None => Err(BC_NO),
        };
        match r {
            Ok(()) => {
                self.cursor = no;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 单轮重放。
    pub fn replay_round(&mut self, no: usize, verdict: u8, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(BC_FROZEN));
        }
        if no == 0 || no > CRITS {
            return Err(self.fail(BC_NO));
        }
        let r = match self.crits.get_mut(no - 1) {
            Some(c) => c.replay(verdict, lines),
            None => Err(BC_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 判据收口。
    pub fn close(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(BC_FROZEN));
        }
        if no == 0 || no > CRITS {
            return Err(self.fail(BC_NO));
        }
        let r = match self.crits.get_mut(no - 1) {
            Some(c) => c.close(),
            None => Err(BC_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 声明行数合计。
    pub fn decl_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                n = n.wrapping_add(c.declared_lines());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 重放行数合计。
    pub fn replay_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                n = n.wrapping_add(c.replayed_lines());
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 行数守恒：声明合计等于批行数，重放合计等于声明 × 轮数。
    pub fn conservation(&self) -> bool {
        let d = self.decl_sum();
        d == LINE_TOTAL && self.replay_sum() == d.wrapping_mul(ROUNDS as u32)
    }

    /// 按卡的域累计。
    pub fn card_sums(&self) -> [u32; CARDS] {
        let mut out = [0u32; CARDS];
        let mut i = 1usize;
        while i <= CRITS {
            let slot = card_slot(i);
            if slot < CARDS {
                if let Some(c) = self.crits.get(i - 1) {
                    if let Some(v) = out.get_mut(slot) {
                        *v = v.wrapping_add(c.declared_lines());
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 域累计对平：六卡逐卡等于在册负载行数。
    pub fn domain_balanced(&self) -> bool {
        let got = self.card_sums();
        let mut i = 0usize;
        while i < CARDS {
            let (g, w) = match (got.get(i), CARD_ACC.get(i)) {
                (Some(g), Some(w)) => (*g, *w),
                _ => return false,
            };
            if g != w {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 已收口判据数。
    pub fn closed_crits(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                if c.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 失败轮合计。
    pub fn total_fails(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                n = n.wrapping_add(c.fails());
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
        while i < CRITS {
            if let Some(c) = self.crits.get(i) {
                h ^= c.digest();
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

    /// 终重放：十九闭、零失败、行数守恒、域累计对平，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.closed_crits() != CRITS {
            return Err(self.fail(BC_REPLAY));
        }
        if self.total_fails() != 0 {
            return Err(self.fail(BC_FAIL));
        }
        if !self.conservation() {
            return Err(self.fail(BC_SUM));
        }
        if !self.domain_balanced() {
            return Err(self.fail(BC_DOMAIN));
        }
        self.freeze()
    }
}

/// 搭建 B37 批账：十九判据各三轮全过，终重放。
pub fn build_batch(base: u32) -> (BatchLedger, Result<u64, u32>) {
    let mut g = BatchLedger::new(base);
    let mut no = 1usize;
    while no <= CRITS {
        let card = card_for(no);
        let lines = match DECL_LINES.get(no.wrapping_sub(1)) {
            Some(v) => *v,
            None => LINE_MIN,
        };
        if g.declare(no, card, lines).is_err() {
            return (g, Err(BC_CARD));
        }
        let mut r = 0usize;
        while r < ROUNDS {
            if g.replay_round(no, 1, lines).is_err() {
                return (g, Err(BC_SUM));
            }
            r = r.wrapping_add(1);
        }
        if g.close(no).is_err() {
            return (g, Err(BC_FAIL));
        }
        no = no.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：挂错卡、行数不足、失败轮、重放超声明。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① J1 挂到 11135 上落 BC_CARD。
    let mut a = BatchLedger::new(0x3c00);
    out[0] = a.declare(1, 11135, 300).err() == Some(BC_CARD) && a.rejects() == 1;

    // ② 行数 259 落 BC_LINES。
    let mut b = BatchLedger::new(0x3c01);
    out[1] = b.declare(1, 11134, LINE_MIN - 1).err() == Some(BC_LINES) && b.rejects() == 1;

    // ③ 三轮里有一轮失败，收口落 BC_FAIL。
    let mut c = BatchLedger::new(0x3c02);
    let mut ok3 = c.declare(1, 11134, 300).is_ok();
    ok3 = ok3 && c.replay_round(1, 1, 300).is_ok();
    ok3 = ok3 && c.replay_round(1, 1, 300).is_ok();
    ok3 = ok3 && c.replay_round(1, 2, 300).is_ok();
    out[2] = ok3 && c.close(1).err() == Some(BC_FAIL) && c.rejects() == 1;

    // ④ 重放行数超过声明落 BC_SUM。
    let mut d = BatchLedger::new(0x3c03);
    let ok4 = d.declare(1, 11134, 300).is_ok();
    out[3] = ok4 && d.replay_round(1, 1, 301).err() == Some(BC_SUM) && d.rejects() == 1;

    out
}

/// 判据集：G1~G13。
pub fn run_unx11140_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11140.b37_batch_close");
    let (g, r) = build_batch(0x1140);

    // G1 常量面：十九判据、六卡、行数 6420。
    let mut acc = 0u32;
    let mut i1 = 0usize;
    while i1 < CARDS {
        if let Some(v) = CARD_ACC.get(i1) {
            acc = acc.wrapping_add(*v);
        }
        i1 = i1.wrapping_add(1);
    }
    let c1 = SEG_BASE == 0x1f280
        && CRITS == 19
        && ROUNDS == 3
        && CARDS == 6
        && decl_total() == LINE_TOTAL
        && load_total() == CRITS as u32
        && acc == LINE_TOTAL
        && CARD_IDS.len() == CARDS
        && CRIT_NAMES.len() == CRITS
        && BATCH == "B37";
    s.add("G1-常量面", c1, "19 判据 / 3 轮 / 6 卡 / 行数 6420");

    // G2 十九判据在册且挂卡正确。
    let mut c2 = 0usize;
    let mut i2 = 1usize;
    while i2 <= CRITS {
        if let Some(c) = g.crit(i2) {
            if c.declared() && c.card == card_for(i2) {
                c2 = c2.wrapping_add(1);
            }
        }
        i2 = i2.wrapping_add(1);
    }
    s.add_dyn("G2-判据挂卡", c2 == CRITS, &format!("mounted={}/{}", c2, CRITS));

    // G3 十九判据全闭。
    s.add_dyn(
        "G3-判据闭合",
        g.closed_crits() == CRITS,
        &format!("closed={}/{}", g.closed_crits(), CRITS),
    );

    // G4 重放轮次 57/57。
    let mut rounds = 0usize;
    let mut i4 = 1usize;
    while i4 <= CRITS {
        if let Some(c) = g.crit(i4) {
            rounds = rounds.wrapping_add(c.replayed());
        }
        i4 = i4.wrapping_add(1);
    }
    s.add_dyn(
        "G4-轮次 57/57",
        rounds == CRITS.wrapping_mul(ROUNDS),
        &format!("rounds={}/{}", rounds, CRITS * ROUNDS),
    );

    // G5 行数守恒。
    s.add_dyn(
        "G5-行数守恒",
        g.conservation() && g.decl_sum() == LINE_TOTAL,
        &format!(
            "decl={}/{} replay={}/{}",
            g.decl_sum(),
            LINE_TOTAL,
            g.replay_sum(),
            LINE_TOTAL * ROUNDS as u32
        ),
    );

    // G6 域累计六卡对平。
    s.add_dyn(
        "G6-域累计对平",
        g.domain_balanced() && g.card_sums() == CARD_ACC,
        &format!("cards={:?}", g.card_sums()),
    );

    // G7 零失败轮。
    s.add_dyn(
        "G7-零失败轮",
        g.total_fails() == 0,
        &format!("fails={}", g.total_fails()),
    );

    // G8 终重放成立并封盘。
    s.add("G8-批收口封盘", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // G9 重放与封盘幂等。
    let mut a9 = g.clone();
    let before = a9.fold();
    let again = a9.replay();
    let mut b9 = BatchLedger::new(0x1140);
    let f1 = b9.freeze();
    let f2 = b9.freeze();
    s.add(
        "G9-幂等",
        again.is_ok() && a9.fold() == before && f1.is_ok() && f1 == f2,
        "重放二次同指纹；封盘二次同指纹",
    );

    // G10 拒止优先序。
    let mut c10 = g.clone();
    let z1 = c10.declare(CRITS + 1, 11134, 300).err();
    let mut c10b = BatchLedger::new(0x1141);
    let z2 = c10b.declare(CRITS + 1, 11134, 300).err();
    s.add(
        "G10-拒止优先序",
        z1 == Some(BC_FROZEN) && z2 == Some(BC_NO),
        "封盘优先于判据号；未封盘则 BC_NO",
    );

    // G11 四路注入。
    let inj = inject_four();
    s.add(
        "G11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "挂卡/行数/失败/超声明四路各自发火",
    );

    // G12 指纹灵敏度。
    let (g2, _) = build_batch(0x1142);
    let (g3, _) = build_batch(0x1140);
    s.add(
        "G12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址相异、同基址相同",
    );

    // G13 逐例归零。
    s.add(
        "G13-逐例归零",
        g.rejects() == 0
            && g.closed_crits() == CRITS
            && g.conservation()
            && g.domain_balanced(),
        "rejects=0，十九闭、守恒、域平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11140_flow() {
        let (g, r) = build_batch(0x9000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.closed_crits(), CRITS);
        assert_eq!(g.total_fails(), 0);
        assert_eq!(g.decl_sum(), LINE_TOTAL);
        assert_eq!(g.replay_sum(), LINE_TOTAL * ROUNDS as u32);
        assert!(g.conservation());
        assert_eq!(g.card_sums(), CARD_ACC);
        assert!(g.domain_balanced());
        assert_eq!(g.rejects(), 0);
        // 首末判据的挂卡。
        assert_eq!(card_for(1), 11134);
        assert_eq!(card_for(CRITS), 11139);
        let last = match g.crit(CRITS) {
            Some(c) => c,
            None => return,
        };
        assert_eq!(last.card, 11139);
        assert!(last.closed());
    }

    #[test]
    fn f11140_gates() {
        let mut g = BatchLedger::new(0x9001);
        assert_eq!(g.declare(2, 11134, 300).err(), Some(BC_ORDER));
        assert_eq!(g.declare(1, 11135, 300).err(), Some(BC_CARD));
        assert_eq!(g.declare(1, 11134, 259).err(), Some(BC_LINES));
        assert!(g.declare(1, 11134, 300).is_ok());
        assert_eq!(g.declare(1, 11134, 300).err(), Some(BC_ORDER));
        assert_eq!(g.close(1).err(), Some(BC_ROUNDS));
        assert!(g.replay_round(1, 1, 300).is_ok());
        assert!(g.replay_round(1, 1, 300).is_ok());
        assert!(g.replay_round(1, 1, 300).is_ok());
        assert_eq!(g.replay_round(1, 1, 300).err(), Some(BC_ROUNDS));
        assert!(g.close(1).is_ok());
        assert_eq!(g.rejects(), 6);
        assert!(g.freeze().is_ok());
        assert_eq!(g.declare(2, 11134, 300).err(), Some(BC_FROZEN));
        assert_eq!(g.rejects(), 7);
        // 终重放：只闭一道，缺账。
        assert_eq!(g.replay().err(), Some(BC_REPLAY));
        assert_eq!(g.rejects(), 8);
    }

    #[test]
    fn f11140_rules() {
        assert_eq!(card_for(0), 0);
        assert_eq!(card_for(CRITS + 1), 0);
        assert_eq!(card_for(1), 11134);
        assert_eq!(card_for(4), 11134);
        assert_eq!(card_for(5), 11135);
        assert_eq!(card_for(7), 11135);
        assert_eq!(card_for(8), 11136);
        assert_eq!(card_for(10), 11136);
        assert_eq!(card_for(11), 11137);
        assert_eq!(card_for(13), 11137);
        assert_eq!(card_for(14), 11138);
        assert_eq!(card_for(16), 11138);
        assert_eq!(card_for(17), 11139);
        assert_eq!(card_slot(4), 0);
        assert_eq!(card_slot(19), 5);
        assert_eq!(card_slot(0), CARDS);
        assert_eq!(decl_total(), LINE_TOTAL);
        assert_eq!(load_total(), CRITS as u32);
        // 判据状态机：未立账不得重放。
        let mut c = CritRec::new(1);
        assert_eq!(c.replay(1, 300).err(), Some(BC_ORDER));
        assert!(c.declare(11134, 300).is_ok());
        assert_eq!(c.declare(11134, 300).err(), Some(BC_ORDER));
        assert_eq!(c.replay(3, 300).err(), Some(BC_FAIL));
        assert_eq!(c.replay(1, 301).err(), Some(BC_SUM));
    }

    #[test]
    fn f11140_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11140_checks_green() {
        let cs = run_unx11140_checks();
        assert!(cs.all_passed(), "checks red in unx11140");
    }
}
