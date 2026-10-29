//! UNX-F11141 清账场二搭建（B21–B36 执行序与范围勘误注记）（0x1f290 段）
//!
//! 纯功能模型：清账场二要铺 B21~B36 十六批。铺场不是点名，是排执行序：
//! 每批有前置批（B22 须待 B21、B35 须待 B34 与 B27 等），执行序必须是
//! 十六批的一个置换，且任一槽位落批时其全部前置已在更早槽位执行完。
//! 前置未到就落批，当场拒止——这是场二最常见的假进度。
//!
//! 同场还有一本「范围勘误注记」：场一收口后发现的范围口径偏差，在册后
//! 逐条应用到目标批，按类改该批的行数额度：
//!   0 范围扩（+delta）、1 范围缩（-delta）、2 序号勘误（+delta）、
//!   3 口径勘误（-delta，绝对值）。
//! 注记必须在目标批已执行之后才许应用（免得改写已在途的账），每条只许
//! 应用一次；应用后该批额度不得低于地板 200 行。收口时注记必须全部
//! 在册且全部应用——在册未用即悬空，悬空不许封场。
//!
//! 行数总账：十六批各 300 行起底（4800），四条注记净差 +40，封场合计
//! 4840，逐批落点亦须逐一对上。
//!
//! 拒止面：槽位越界、未知批号、重复落批、前置未执行、注记号越界、注记
//! 重复登记/重复应用、注记目标不符、注记早于目标执行、额度跌破地板、
//! 行数合计不平、封场后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f290;
/// 槽位越界
pub const GT_SLOT: u32 = SEG_BASE + 0x01;
/// 批号越界或未知
pub const GT_BATCH: u32 = SEG_BASE + 0x02;
/// 重复落批
pub const GT_DUP: u32 = SEG_BASE + 0x03;
/// 前置未执行、注记早于目标执行
pub const GT_ORDER: u32 = SEG_BASE + 0x04;
/// 注记号越界或未登记
pub const GT_NOTE: u32 = SEG_BASE + 0x05;
/// 注记重复登记或重复应用
pub const GT_NOTE_DUP: u32 = SEG_BASE + 0x06;
/// 注记目标不符
pub const GT_NOTE_TARGET: u32 = SEG_BASE + 0x07;
/// 额度跌破地板
pub const GT_FLOOR: u32 = SEG_BASE + 0x08;
/// 行数合计不平
pub const GT_SUM: u32 = SEG_BASE + 0x09;
/// 场账已封盘
pub const GT_FROZEN: u32 = SEG_BASE + 0x0a;

/// 场二批数（B21~B36）。
pub const BATCHES: usize = 16;
/// 勘误注记条数。
pub const NOTES: usize = 4;
/// 注记类数。
pub const KINDS: usize = 4;
/// 批名（B21~B36）。
pub const BATCH_NAMES: [&str; BATCHES] = [
    "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28", "B29", "B30", "B31", "B32", "B33",
    "B34", "B35", "B36",
];
/// 注记类名。
pub const KIND_NAMES: [&str; KINDS] = ["范围扩", "范围缩", "序号勘误", "口径勘误"];
/// 每批起底行数。
pub const BASE_LINES: u32 = 300;
/// 起底合计。
pub const BASE_TOTAL: u32 = BATCHES as u32 * BASE_LINES;
/// 额度地板。
pub const LINE_MIN: u32 = 200;
/// 前置批 A 表（0 表示无前置）。
pub const PREREQ_A: [usize; BATCHES] = [0, 1, 2, 3, 4, 5, 6, 1, 8, 9, 10, 11, 12, 13, 14, 15];
/// 前置批 B 表（B35 另需 B27）。
pub const PREREQ_B: [usize; BATCHES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0];
/// 注记目标批（1 起）。
pub const NOTE_TARGET: [usize; NOTES] = [3, 8, 12, 16];
/// 注记类。
pub const NOTE_KIND: [u8; NOTES] = [0, 1, 2, 3];
/// 注记净差（带符号）。
pub const NOTE_DELTA: [i32; NOTES] = [120, 80, 40, 40];
/// 净差合计。
pub const DELTA_TOTAL: i32 = 40;
/// 封场行数合计。
pub const EXPECT_TOTAL: u32 = BASE_TOTAL + DELTA_TOTAL as u32;
/// 逐批落点（1 起批号 -> 行数）：B3 420、B8 220、B12 340、B16 260。
pub const NOTE_LINES: [[usize; 2]; NOTES] = [[3, 420], [8, 220], [12, 340], [16, 260]];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 第 no 批的第一前置（0 = 无）。
pub fn prereq_a(no: usize) -> usize {
    if no == 0 || no > BATCHES {
        return 0;
    }
    match PREREQ_A.get(no - 1) {
        Some(v) => *v,
        None => 0,
    }
}

/// 第 no 批的第二前置（0 = 无）。
pub fn prereq_b(no: usize) -> usize {
    if no == 0 || no > BATCHES {
        return 0;
    }
    match PREREQ_B.get(no - 1) {
        Some(v) => *v,
        None => 0,
    }
}

/// 注记类名。
pub fn kind_name(k: u8) -> &'static str {
    match KIND_NAMES.get(k as usize) {
        Some(n) => n,
        None => "越界",
    }
}

/// 带符号落账：扩容类加、缩减类减、口径勘误按绝对值减。
pub fn signed_delta(kind: u8, delta: i32) -> i32 {
    if delta < 0 {
        return delta;
    }
    match kind {
        1 => -delta,
        3 => -delta,
        _ => delta,
    }
}

/// 一条范围勘误注记的账：登记一次、应用一次、按目标批落账。
#[derive(Clone, Copy)]
pub struct Errata {
    registered: bool,
    applied: bool,
    target: usize,
    kind: u8,
    delta: i32,
    tag: u32,
    digest: u64,
}

impl Errata {
    /// 空注记。
    pub const fn new() -> Self {
        Errata {
            registered: false,
            applied: false,
            target: 0,
            kind: 0,
            delta: 0,
            tag: 0,
            digest: 0,
        }
    }

    /// 是否在册。
    pub fn registered(&self) -> bool {
        self.registered
    }

    /// 是否已应用。
    pub fn applied(&self) -> bool {
        self.applied
    }

    /// 目标批（1 起）。
    pub fn target(&self) -> usize {
        self.target
    }

    /// 注记类。
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 表列净差（恒正）。
    pub fn delta(&self) -> i32 {
        self.delta
    }

    /// 带符号落账差。
    pub fn signed(&self) -> i32 {
        signed_delta(self.kind, self.delta)
    }

    /// 注记登记戳。
    pub fn tag(&self) -> u32 {
        self.tag
    }

    /// 注记指纹。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 挂账时写入表列值并生成指纹。
    fn register(&mut self, idx: usize, tag: u32) {
        self.registered = true;
        self.applied = false;
        self.target = match NOTE_TARGET.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        self.kind = match NOTE_KIND.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        self.delta = match NOTE_DELTA.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        self.tag = tag;
        self.digest = note_digest(idx, self.target, self.kind, self.delta);
    }

    fn mark_applied(&mut self) {
        self.applied = true;
    }
}

/// 注记指纹：序号、目标、类、净差、登记戳全部入账。
pub fn note_digest(idx: usize, target: usize, kind: u8, delta: i32) -> u64 {
    let mut h = FNV_OFF;
    let parts: [u64; 5] = [
        idx as u64,
        target as u64,
        kind as u64,
        delta as u64,
        (SEG_BASE as u64).wrapping_add(idx as u64),
    ];
    let mut i = 0usize;
    while i < parts.len() {
        let b = parts[i].to_le_bytes();
        let mut k = 0usize;
        while k < b.len() {
            h ^= b[k] as u64;
            h = h.wrapping_mul(FNV_PRIME);
            k += 1;
        }
        i += 1;
    }
    h
}

/// 清账场二：执行序 + 范围勘误注记两本账合在一处。
#[derive(Clone)]
pub struct Ground {
    base_tag: u32,
    order: [usize; BATCHES],
    lines: [u32; BATCHES],
    executed: [bool; BATCHES],
    notes: [Errata; NOTES],
    slot: usize,
    registered: usize,
    applied: usize,
    rejects: u32,
    last: u32,
    frozen: bool,
    frozen_at: u64,
}

impl Ground {
    /// 空场。
    pub fn new(base_tag: u32) -> Self {
        Ground {
            base_tag,
            order: [0usize; BATCHES],
            lines: [0u32; BATCHES],
            executed: [false; BATCHES],
            notes: [Errata::new(); NOTES],
            slot: 0,
            registered: 0,
            applied: 0,
            rejects: 0,
            last: 0,
            frozen: false,
            frozen_at: 0,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        self.last = code;
        code
    }

    /// 已填槽位数。
    pub fn filled(&self) -> usize {
        self.slot
    }

    /// 累计拒止。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 最后一次拒止码（0 = 无）。
    pub fn last_error(&self) -> u32 {
        self.last
    }

    /// 是否封场。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 槽位上的批号（0 = 空槽）。
    pub fn order_at(&self, slot: usize) -> usize {
        match self.order.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 批是否已执行。
    pub fn is_executed(&self, no: usize) -> bool {
        if no == 0 || no > BATCHES {
            return false;
        }
        match self.executed.get(no - 1) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 批当前行数（未执行为 0）。
    pub fn line(&self, no: usize) -> u32 {
        if no == 0 || no > BATCHES {
            return 0;
        }
        match self.lines.get(no - 1) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 在册注记数。
    pub fn registered(&self) -> usize {
        self.registered
    }

    /// 已应用注记数。
    pub fn applied(&self) -> usize {
        self.applied
    }

    /// 注记账（越界返回 None）。
    pub fn note(&self, idx: usize) -> Option<Errata> {
        self.notes.get(idx).copied()
    }

    /// 已执行批行数合计。
    pub fn total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BATCHES {
            if self.executed[i] {
                n = n.wrapping_add(self.lines[i]);
            }
            i += 1;
        }
        n
    }
}

impl Ground {
    /// 落批：槽位合法、批号合法、未重复、前置全已执行，才进序并起底 300 行。
    pub fn execute(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(GT_FROZEN));
        }
        if self.slot >= BATCHES {
            return Err(self.fail(GT_SLOT));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(GT_BATCH));
        }
        if self.is_executed(no) {
            return Err(self.fail(GT_DUP));
        }
        let pa = prereq_a(no);
        if pa != 0 && !self.is_executed(pa) {
            return Err(self.fail(GT_ORDER));
        }
        let pb = prereq_b(no);
        if pb != 0 && !self.is_executed(pb) {
            return Err(self.fail(GT_ORDER));
        }
        self.order[self.slot] = no;
        self.slot = self.slot.wrapping_add(1);
        self.executed[no - 1] = true;
        self.lines[no - 1] = BASE_LINES;
        Ok(())
    }

    /// 登记一条注记：只按序号登记，目标/类/净差一律取表列值。
    pub fn register_note(&mut self, idx: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(GT_FROZEN));
        }
        if idx >= NOTES {
            return Err(self.fail(GT_NOTE));
        }
        let already = match self.notes.get(idx) {
            Some(e) => e.registered(),
            None => false,
        };
        if already {
            return Err(self.fail(GT_NOTE_DUP));
        }
        let tag = (self.base_tag as u64)
            .wrapping_add(NOTE_ID_BASE as u64)
            .wrapping_add(idx as u64) as u32;
        if let Some(e) = self.notes.get_mut(idx) {
            e.register(idx, tag);
        }
        self.registered = self.registered.wrapping_add(1);
        Ok(())
    }

    /// 应用一条注记：须已在册、未应用、目标相符且目标批已执行，落账后不得跌破地板。
    pub fn apply_note(&mut self, idx: usize, target: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(GT_FROZEN));
        }
        if idx >= NOTES {
            return Err(self.fail(GT_NOTE));
        }
        let e = match self.notes.get(idx) {
            Some(v) => *v,
            None => return Err(self.fail(GT_NOTE)),
        };
        if !e.registered() {
            return Err(self.fail(GT_NOTE));
        }
        if e.applied() {
            return Err(self.fail(GT_NOTE_DUP));
        }
        if target != e.target() {
            return Err(self.fail(GT_NOTE_TARGET));
        }
        if !self.is_executed(target) {
            return Err(self.fail(GT_ORDER));
        }
        let cur = self.line(target) as i64;
        let signed = (e.signed() as i64).wrapping_add(0);
        let next = cur.wrapping_add(signed);
        if next < LINE_MIN as i64 {
            return Err(self.fail(GT_FLOOR));
        }
        self.lines[target - 1] = next as u32;
        if let Some(m) = self.notes.get_mut(idx) {
            m.mark_applied();
        }
        self.applied = self.applied.wrapping_add(1);
        Ok(())
    }

    /// 执行序拓扑自检：每个批的全部前置都必须落在更早的槽位。
    pub fn topo_ok(&self) -> bool {
        let mut slot = 0usize;
        while slot < self.slot {
            let no = match self.order.get(slot) {
                Some(v) => *v,
                None => return false,
            };
            if no == 0 || no > BATCHES {
                return false;
            }
            let pa = prereq_a(no);
            if pa != 0 && !self.before(slot, pa) {
                return false;
            }
            let pb = prereq_b(no);
            if pb != 0 && !self.before(slot, pb) {
                return false;
            }
            slot += 1;
        }
        true
    }

    fn before(&self, slot: usize, no: usize) -> bool {
        let mut i = 0usize;
        while i < slot {
            if self.order.get(i) == Some(&no) {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 注记登记戳基址。
pub const NOTE_ID_BASE: u32 = 0x2a00;
/// 槽位账上限（十六槽）。
pub const SLOT_CAP: usize = BATCHES;

fn mix_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let b = v.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        x ^= b[i] as u64;
        x = x.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

fn mix_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let b = v.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        x ^= b[i] as u64;
        x = x.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

impl Ground {
    /// 该批行数是否落在表列落点上。
    pub fn line_ok(&self, no: usize) -> bool {
        if !self.is_executed(no) {
            return false;
        }
        let mut want = BASE_LINES;
        let mut i = 0usize;
        while i < NOTES {
            if let Some(p) = NOTE_LINES.get(i) {
                let hit = match p.get(0) {
                    Some(v) => *v == no,
                    None => false,
                };
                if hit {
                    if let Some(v) = p.get(1) {
                        want = *v as u32;
                    }
                }
            }
            i += 1;
        }
        self.line(no) == want
    }

    /// 全账指纹：段基、基址、槽位、计数、每槽批号/行数/执行位、每条注记。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix_u32(h, SEG_BASE);
        h = mix_u32(h, self.base_tag);
        h = mix_u32(h, self.slot as u32);
        h = mix_u32(h, self.registered as u32);
        h = mix_u32(h, self.applied as u32);
        h = mix_u32(h, self.rejects);
        h = mix_u32(h, self.last);
        h = mix_u32(h, self.frozen as u32);
        let mut i = 0usize;
        while i < BATCHES {
            h = mix_u32(h, self.order[i] as u32);
            h = mix_u32(h, self.lines[i]);
            h = mix_u32(h, self.executed[i] as u32);
            i += 1;
        }
        let mut k = 0usize;
        while k < NOTES {
            let e = self.notes[k];
            h = mix_u32(h, e.registered() as u32);
            h = mix_u32(h, e.applied() as u32);
            h = mix_u32(h, e.target() as u32);
            h = mix_u32(h, e.kind() as u32);
            h = mix_u32(h, e.delta() as u32);
            h = mix_u32(h, e.tag());
            h = mix_u64(h, e.digest());
            k += 1;
        }
        h
    }

    /// 封场：序满、拓扑成立、注记全登记且全应用、行数合计 4840、逐批落点相符。
    /// 收口三关的拒止优先序：GT_ORDER > GT_NOTE > GT_SUM。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.frozen_at);
        }
        if self.slot != BATCHES || !self.topo_ok() {
            return Err(self.fail(GT_ORDER));
        }
        if self.registered != NOTES || self.applied != NOTES {
            return Err(self.fail(GT_NOTE));
        }
        if self.total() != EXPECT_TOTAL {
            return Err(self.fail(GT_SUM));
        }
        let mut i = 1usize;
        while i <= BATCHES {
            if !self.line_ok(i) {
                return Err(self.fail(GT_SUM));
            }
            i += 1;
        }
        self.frozen = true;
        let f = self.fold();
        self.frozen_at = f;
        Ok(f)
    }
}

impl Ground {
    /// 重放：按记录下来的执行序重铺一遍场，再按在册注记重演落账，
    /// 逐批比对行数与执行位。封场后重放还须与封场指纹一致。
    pub fn replay(&mut self) -> Result<u64, u32> {
        let mut fresh = Ground::new(self.base_tag);
        let mut i = 0usize;
        while i < self.slot {
            let no = match self.order.get(i) {
                Some(v) => *v,
                None => return Err(self.fail(GT_SUM)),
            };
            if fresh.execute(no).is_err() {
                return Err(self.fail(GT_ORDER));
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < NOTES {
            let e = match self.notes.get(k) {
                Some(v) => *v,
                None => return Err(self.fail(GT_NOTE)),
            };
            if e.applied() && !e.registered() {
                return Err(self.fail(GT_NOTE));
            }
            if e.registered() {
                if fresh.register_note(k).is_err() {
                    return Err(self.fail(GT_NOTE));
                }
                if e.applied() && fresh.apply_note(k, e.target()).is_err() {
                    return Err(self.fail(GT_FLOOR));
                }
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < BATCHES {
            if fresh.lines[j] != self.lines[j] || fresh.executed[j] != self.executed[j] {
                return Err(self.fail(GT_SUM));
            }
            j += 1;
        }
        if !fresh.topo_ok() {
            return Err(self.fail(GT_ORDER));
        }
        fresh.rejects = self.rejects;
        fresh.last = self.last;
        if self.frozen {
            fresh.frozen = true;
            let f = fresh.fold();
            if f != self.frozen_at {
                return Err(self.fail(GT_SUM));
            }
            Ok(f)
        } else {
            Ok(fresh.fold())
        }
    }
}

impl Ground {
    /// 目标批执行后，把全部指向该批的在册未用注记就地应用，返回应用条数。
    pub fn apply_due(&mut self, no: usize) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < NOTES {
            let e = match self.notes.get(k) {
                Some(v) => *v,
                None => return Err(self.fail(GT_NOTE)),
            };
            if e.registered() && !e.applied() && e.target() == no {
                self.apply_note(k, no)?;
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        Ok(n)
    }
}

/// 铺场：四条注记先行在册，B21~B36 顺次执行，目标批一落即应用其注记，
/// 收口封场后终重放。
pub fn build_ground(base_tag: u32) -> (Ground, Result<u64, u32>) {
    let mut g = Ground::new(base_tag);
    let mut k = 0usize;
    while k < NOTES {
        let _ = g.register_note(k);
        k += 1;
    }
    let mut no = 1usize;
    while no <= BATCHES {
        let _ = g.execute(no);
        let _ = g.apply_due(no);
        no += 1;
    }
    let r = match g.freeze() {
        Ok(_) => g.replay(),
        Err(e) => Err(e),
    };
    (g, r)
}

/// 四路注入：前置未到、重复落批、注记早于目标、注记重复应用。
/// 每路都在新场实例上只发一次火。
pub fn inject_four() -> [bool; 4] {
    // 1) B22 前置 B21 未执行。
    let mut a = Ground::new(0x1141);
    let r1 = a.execute(2);
    let ok1 = r1 == Err(GT_ORDER) && a.rejects() == 1 && a.last_error() == GT_ORDER;

    // 2) B21 落两次。
    let mut b = Ground::new(0x1142);
    let _ = b.execute(1);
    let r2 = b.execute(1);
    let ok2 = r2 == Err(GT_DUP) && b.rejects() == 1 && b.last_error() == GT_DUP;

    // 3) 注记 0 的目标 B23 未执行就应用。
    let mut c = Ground::new(0x1143);
    let _ = c.register_note(0);
    let r3 = c.apply_note(0, 3);
    let ok3 = r3 == Err(GT_ORDER) && c.rejects() == 1 && c.last_error() == GT_ORDER;

    // 4) 注记 0 应用两次。
    let mut d = Ground::new(0x1144);
    let _ = d.execute(1);
    let _ = d.execute(2);
    let _ = d.execute(3);
    let _ = d.register_note(0);
    let first = d.apply_note(0, 3);
    let r4 = d.apply_note(0, 3);
    let ok4 = first.is_ok()
        && r4 == Err(GT_NOTE_DUP)
        && d.rejects() == 1
        && d.last_error() == GT_NOTE_DUP;

    [ok1, ok2, ok3, ok4]
}

impl Ground {
    /// 审计复核差额：表列注记之外的单笔带符号调整，同样受地板与封场约束。
    pub fn adjust(&mut self, no: usize, delta: i32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(GT_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(GT_BATCH));
        }
        if !self.is_executed(no) {
            return Err(self.fail(GT_ORDER));
        }
        let next = (self.line(no) as i64).wrapping_add(delta as i64);
        if next < LINE_MIN as i64 {
            return Err(self.fail(GT_FLOOR));
        }
        if next > u32::MAX as i64 {
            return Err(self.fail(GT_SUM));
        }
        self.lines[no - 1] = next as u32;
        Ok(())
    }
}

fn slot_of(g: &Ground, no: usize) -> Option<usize> {
    let mut i = 0usize;
    while i < g.filled() {
        if g.order_at(i) == no {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 场二十三检：常量、执行序、前置、注记在册/应用、总账、地板、封场、
/// 幂等、拒止优先序、四路注入、指纹灵敏度、逐例归零。
pub fn run_unx11141_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11141.ground2_setup");
    let (g, r) = build_ground(0x1141);

    // H1 常量面。
    let c1 = SEG_BASE == 0x1f290
        && BATCHES == 16
        && NOTES == 4
        && KINDS == 4
        && BASE_TOTAL == 4800
        && DELTA_TOTAL == 40
        && EXPECT_TOTAL == 4840
        && LINE_MIN == 200
        && SLOT_CAP == BATCHES
        && BATCH_NAMES.len() == BATCHES
        && KIND_NAMES.len() == KINDS
        && BATCH_NAMES.first() == Some(&"B21")
        && BATCH_NAMES.last() == Some(&"B36");
    s.add("H1-常量面", c1, "十六批 B21~B36 / 四注记 / 4800+40=4840");

    // H2 执行序是十六批的一个置换。
    let mut seen = [false; BATCHES];
    let mut dup = false;
    let mut i2 = 0usize;
    while i2 < g.filled() {
        let no = g.order_at(i2);
        if no == 0 || no > BATCHES {
            dup = true;
        } else if seen[no - 1] {
            dup = true;
        } else {
            seen[no - 1] = true;
        }
        i2 += 1;
    }
    let mut full = 0usize;
    let mut k2 = 0usize;
    while k2 < BATCHES {
        if seen[k2] {
            full += 1;
        }
        k2 += 1;
    }
    s.add_dyn(
        "H2-执行序置换",
        g.filled() == BATCHES && full == BATCHES && !dup,
        &format!("filled={}/{} distinct={}", g.filled(), BATCHES, full),
    );

    // H3 前置落在更早槽位：B22>B21、B29>B21、B35>B34 且 B35>B27、B36>B35。
    let p21 = slot_of(&g, 1);
    let p22 = slot_of(&g, 2);
    let p27 = slot_of(&g, 7);
    let p28 = slot_of(&g, 8);
    let p29 = slot_of(&g, 9);
    let p34 = slot_of(&g, 14);
    let p35 = slot_of(&g, 15);
    let p36 = slot_of(&g, 16);
    let c3 = p21.is_some()
        && p22 > p21
        && p28.is_some()
        && p29 > p28
        && p34.is_some()
        && p27.is_some()
        && p35 > p34
        && p35 > p27
        && p36 > p35
        && g.topo_ok();
    s.add_dyn(
        "H3-前置先后",
        c3,
        &format!(
            "n1={:?} n2={:?} n9={:?} n14={:?} n7={:?} n15={:?} n16={:?}",
            p21, p22, p29, p34, p27, p35, p36
        ),
    );

    // H4 四条注记全在册，目标/类取表列值，指纹两两相异。
    let mut reg = 0usize;
    let mut tgt_ok = true;
    let mut dg = [0u64; NOTES];
    let mut k4 = 0usize;
    while k4 < NOTES {
        if let Some(e) = g.note(k4) {
            if e.registered() {
                reg += 1;
                if e.target() != NOTE_TARGET[k4] || e.kind() != NOTE_KIND[k4] {
                    tgt_ok = false;
                }
                dg[k4] = e.digest();
            }
        }
        k4 += 1;
    }
    let mut distinct = true;
    let mut a4 = 0usize;
    while a4 < NOTES {
        let mut b4 = a4 + 1;
        while b4 < NOTES {
            if dg[a4] == 0 || dg[a4] == dg[b4] {
                distinct = false;
            }
            b4 += 1;
        }
        a4 += 1;
    }
    s.add_dyn(
        "H4-注记在册",
        g.registered() == NOTES && reg == NOTES && tgt_ok && distinct,
        &format!("registered={}/{} target_ok={} distinct={}", reg, NOTES, tgt_ok, distinct),
    );

    // H5 四条注记全应用，四个目标批落点对上表列值。
    let mut app = 0usize;
    let mut land_ok = true;
    let mut k5 = 0usize;
    while k5 < NOTES {
        if let Some(e) = g.note(k5) {
            if e.applied() {
                app += 1;
            }
        }
        if let Some(p) = NOTE_LINES.get(k5) {
            match (p.get(0), p.get(1)) {
                (Some(no), Some(want)) => {
                    if g.line(*no) != *want as u32 {
                        land_ok = false;
                    }
                }
                _ => land_ok = false,
            }
        }
        k5 += 1;
    }
    s.add_dyn(
        "H5-注记应用",
        g.applied() == NOTES && app == NOTES && land_ok,
        &format!(
            "applied={}/{} B3={} B8={} B12={} B16={}",
            app,
            NOTES,
            g.line(3),
            g.line(8),
            g.line(12),
            g.line(16)
        ),
    );

    // H6 总账：4800 起底 + 净差 40 = 4840，表列净差自洽。
    let mut net = 0i32;
    let mut k6 = 0usize;
    while k6 < NOTES {
        net = net.wrapping_add(signed_delta(NOTE_KIND[k6], NOTE_DELTA[k6]));
        k6 += 1;
    }
    s.add_dyn(
        "H6-总账 4840",
        net == DELTA_TOTAL && g.total() == EXPECT_TOTAL && g.total() == BASE_TOTAL + 40,
        &format!("net={} total={}/{}", net, g.total(), EXPECT_TOTAL),
    );

    // H7 地板：十六批行数全部 >= 200，且最低落点是 B8=220。
    let mut min_line = u32::MAX;
    let mut i7 = 1usize;
    while i7 <= BATCHES {
        let v = g.line(i7);
        if v < min_line {
            min_line = v;
        }
        i7 += 1;
    }
    s.add(
        "H7-额度地板",
        min_line >= LINE_MIN && min_line == 220 && g.line(8) == 220,
        "最低 B8=220，全批不低于 200",
    );

    // H8 终重放通过并封场，封场指纹等于当场指纹。
    s.add(
        "H8-封场",
        r.is_ok() && g.frozen() && g.frozen_at() == g.fold(),
        "终重放通过，封场指纹对平",
    );

    // H9 重放与封盘幂等。
    let mut a9 = g.clone();
    let before = a9.fold();
    let again = a9.replay();
    let mut b9 = g.clone();
    let f1 = b9.freeze();
    let f2 = b9.freeze();
    s.add(
        "H9-幂等",
        again.is_ok()
            && a9.fold() == before
            && f1.is_ok()
            && f1 == f2
            && f1 == Ok(g.frozen_at()),
        "二次重放同指纹；二次封盘同指纹",
    );

    // H10 拒止优先序：批号先于重复，槽位先于批号，注记号先于重复，
    //     目标先于应用，地板先于落账，封场压过一切。
    let mut e1 = Ground::new(0x1150);
    let z1 = e1.execute(17);
    let z2 = e1.execute(0);
    let c10a = z1 == Err(GT_BATCH) && z2 == Err(GT_BATCH) && e1.rejects() == 2;

    let mut e2 = Ground::new(0x1151);
    let z3 = e2.execute(2);
    let _ = e2.execute(1);
    let _ = e2.execute(2);
    let z4 = e2.execute(2);
    let c10b = z3 == Err(GT_ORDER) && z4 == Err(GT_DUP) && e2.rejects() == 2;

    let mut e3 = Ground::new(0x1152);
    let z5 = e3.apply_note(4, 3);
    let z6 = e3.register_note(4);
    let _ = e3.register_note(0);
    let z7 = e3.register_note(0);
    let c10c = z5 == Err(GT_NOTE)
        && z6 == Err(GT_NOTE)
        && z7 == Err(GT_NOTE_DUP)
        && e3.rejects() == 3;

    let mut e4 = Ground::new(0x1153);
    let _ = e4.execute(1);
    let _ = e4.execute(2);
    let _ = e4.execute(3);
    let _ = e4.register_note(0);
    let z8 = e4.apply_note(0, 8);
    let ok8 = e4.apply_note(0, 3);
    let z9 = e4.apply_note(0, 3);
    let c10d = z8 == Err(GT_NOTE_TARGET)
        && ok8.is_ok()
        && z9 == Err(GT_NOTE_DUP)
        && e4.rejects() == 2;

    let mut e5 = Ground::new(0x1154);
    let _ = e5.execute(1);
    let z10 = e5.adjust(1, -120);
    let ok10 = e5.adjust(1, -100);
    let z11 = e5.adjust(1, -1);
    let c10e = z10 == Err(GT_FLOOR)
        && ok10.is_ok()
        && e5.line(1) == LINE_MIN
        && z11 == Err(GT_FLOOR)
        && e5.rejects() == 2;

    let mut e6 = g.clone();
    let z12 = e6.execute(1);
    let z13 = e6.adjust(1, 1);
    let c10f = z12 == Err(GT_FROZEN) && z13 == Err(GT_FROZEN) && e6.rejects() == 2;

    s.add(
        "H10-拒止优先序",
        c10a && c10b && c10c && c10d && c10e && c10f,
        "批号<重复；槽位<批号；注记号<重复；目标<应用；地板<落账；封场最前",
    );

    // H11 四路注入。
    let inj = inject_four();
    s.add(
        "H11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "前置/重复落批/注记早于目标/注记重复应用四路各自发火",
    );

    // H12 指纹灵敏度：异基址相异、同基址相同。
    let (g2, r2) = build_ground(0x1142);
    let (g3, r3) = build_ground(0x1141);
    s.add_dyn(
        "H12-指纹灵敏度",
        r2.is_ok()
            && r3.is_ok()
            && g2.fold() != g.fold()
            && g3.fold() == g.fold()
            && g2.note(0).map(|e| e.tag()) != g.note(0).map(|e| e.tag()),
        &format!("0x1141={:016x} 0x1142={:016x}", g.fold(), g2.fold()),
    );

    // H13 逐例归零。
    s.add(
        "H13-逐例归零",
        g.rejects() == 0
            && g.filled() == BATCHES
            && g.applied() == NOTES
            && g.total() == EXPECT_TOTAL,
        "rejects=0，十六批满槽、四注记已用、总账 4840",
    );

    s
}

impl Ground {
    /// 封场指纹（未封场为 0）。
    pub fn frozen_at(&self) -> u64 {
        self.frozen_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11141_flow() {
        let (g, r) = build_ground(0x9000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.filled(), BATCHES);
        assert_eq!(g.registered(), NOTES);
        assert_eq!(g.applied(), NOTES);
        assert_eq!(g.total(), EXPECT_TOTAL);
        assert_eq!(g.rejects(), 0);
        assert!(g.topo_ok());
        assert_eq!(g.order_at(0), 1);
        assert_eq!(g.order_at(BATCHES - 1), 16);
        assert_eq!(g.line(3), 420);
        assert_eq!(g.line(8), 220);
        assert_eq!(g.line(12), 340);
        assert_eq!(g.line(16), 260);
        assert_eq!(g.line(1), 300);
        assert_eq!(g.frozen_at(), g.fold());
        let n0 = match g.note(0) {
            Some(e) => e,
            None => return,
        };
        assert!(n0.registered() && n0.applied());
        assert_eq!(n0.target(), 3);
        assert_eq!(n0.kind(), 0);
        assert_eq!(n0.signed(), 120);
    }

    #[test]
    fn f11141_order_gates() {
        let mut a = Ground::new(0x9100);
        assert_eq!(a.execute(2).err(), Some(GT_ORDER));
        assert_eq!(a.rejects(), 1);
        assert!(a.execute(1).is_ok());
        assert!(a.execute(2).is_ok());
        assert_eq!(a.execute(2).err(), Some(GT_DUP));
        assert_eq!(a.rejects(), 2);
        assert_eq!(a.execute(17).err(), Some(GT_BATCH));
        assert_eq!(a.rejects(), 3);
        assert_eq!(a.last_error(), GT_BATCH);

        // B35 第二前置 B27 未执行时拒止：先铺 1..6，跳过 7，再铺 8..14。
        let mut b = Ground::new(0x9101);
        let mut no = 1usize;
        while no <= 6 {
            assert!(b.execute(no).is_ok());
            no += 1;
        }
        no = 8;
        while no <= 14 {
            assert!(b.execute(no).is_ok());
            no += 1;
        }
        assert!(!b.is_executed(7));
        assert_eq!(b.execute(15).err(), Some(GT_ORDER));
        assert_eq!(b.rejects(), 1);
        assert!(b.execute(7).is_ok());
        assert!(b.execute(15).is_ok());
        assert!(b.execute(16).is_ok());
        // 槽位满后槽位拒止优先于批号与重复。
        let mut c = Ground::new(0x9102);
        let mut k = 1usize;
        while k <= BATCHES {
            assert!(c.execute(k).is_ok());
            k += 1;
        }
        assert_eq!(c.filled(), BATCHES);
        assert_eq!(c.execute(1).err(), Some(GT_SLOT));
        assert_eq!(c.execute(17).err(), Some(GT_SLOT));
        assert_eq!(c.rejects(), 2);
    }

    #[test]
    fn f11141_note_gates() {
        let mut c = Ground::new(0x9200);
        assert_eq!(c.register_note(4).err(), Some(GT_NOTE));
        assert!(c.register_note(0).is_ok());
        assert_eq!(c.register_note(0).err(), Some(GT_NOTE_DUP));
        assert_eq!(c.apply_note(1, 8).err(), Some(GT_NOTE));
        assert_eq!(c.apply_note(0, 3).err(), Some(GT_ORDER));
        assert!(c.execute(1).is_ok());
        assert!(c.execute(2).is_ok());
        assert!(c.execute(3).is_ok());
        assert_eq!(c.apply_note(0, 8).err(), Some(GT_NOTE_TARGET));
        assert!(c.apply_note(0, 3).is_ok());
        assert_eq!(c.line(3), 420);
        assert_eq!(c.apply_note(0, 3).err(), Some(GT_NOTE_DUP));
        assert_eq!(c.rejects(), 6);
        assert_eq!(c.applied(), 1);
    }

    #[test]
    fn f11141_floor_and_freeze() {
        let mut d = Ground::new(0x9300);
        assert!(d.execute(1).is_ok());
        assert_eq!(d.adjust(1, -120).err(), Some(GT_FLOOR));
        assert!(d.adjust(1, -100).is_ok());
        assert_eq!(d.line(1), LINE_MIN);
        assert_eq!(d.adjust(1, -1).err(), Some(GT_FLOOR));
        assert_eq!(d.rejects(), 2);

        // 序未满：GT_ORDER 先于注记关。
        let mut e = Ground::new(0x9301);
        assert_eq!(e.freeze().err(), Some(GT_ORDER));

        // 序已满、注记未登记：GT_NOTE。
        let mut f = Ground::new(0x9302);
        let mut no = 1usize;
        while no <= BATCHES {
            assert!(f.execute(no).is_ok());
            no += 1;
        }
        assert_eq!(f.freeze().err(), Some(GT_NOTE));
        assert_eq!(f.rejects(), 1);

        // 序满、注记全应用但总账被审计差额扰动：GT_SUM。
        let mut h = Ground::new(0x9303);
        let mut k = 0usize;
        while k < NOTES {
            assert!(h.register_note(k).is_ok());
            k += 1;
        }
        let mut n = 1usize;
        while n <= BATCHES {
            assert!(h.execute(n).is_ok());
            n += 1;
        }
        let mut j = 0usize;
        while j < NOTES {
            let t = NOTE_TARGET[j];
            assert!(h.apply_note(j, t).is_ok());
            j += 1;
        }
        assert_eq!(h.total(), EXPECT_TOTAL);
        assert!(h.adjust(1, 10).is_ok());
        assert_eq!(h.total(), EXPECT_TOTAL + 10);
        assert_eq!(h.freeze().err(), Some(GT_SUM));
        assert_eq!(h.rejects(), 1);
        assert!(!h.frozen());
    }

    #[test]
    fn f11141_injection_and_checks() {
        let inj = inject_four();
        assert!(inj[0] && inj[1] && inj[2] && inj[3]);

        let (g, r) = build_ground(0x9400);
        assert!(r.is_ok());
        let mut h = g.clone();
        assert_eq!(h.execute(1).err(), Some(GT_FROZEN));
        assert_eq!(h.register_note(0).err(), Some(GT_FROZEN));
        assert_eq!(h.apply_note(0, 3).err(), Some(GT_FROZEN));
        assert_eq!(h.adjust(1, 1).err(), Some(GT_FROZEN));

        let mut dg = [0u64; NOTES];
        let mut k = 0usize;
        while k < NOTES {
            if let Some(e) = g.note(k) {
                dg[k] = e.digest();
            }
            k += 1;
        }
        assert_ne!(dg[0], dg[1]);
        assert_ne!(dg[1], dg[2]);
        assert_ne!(dg[2], dg[3]);
        assert_eq!(kind_name(0), "范围扩");
        assert_eq!(signed_delta(1, 80), -80);
        assert_eq!(signed_delta(0, 120), 120);

        let cs = run_unx11141_checks();
        assert!(cs.all_passed());
        let again = run_unx11141_checks();
        assert!(again.all_passed());
        let _ = cs.red_items_text();
    }
}
