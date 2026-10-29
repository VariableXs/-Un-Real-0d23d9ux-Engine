//! UNX-F11136 清账段一总账（B01–B20 段账闭合）（0x1f240 段）
//!
//! 纯功能模型：清账段一把 B01~B20 二十批的账合成一本总账，每批记三个数
//! ——卡数、判据数、行数。三个数各有三本账：声明账（批启动时立的数）、
//! 重放账（终重放时逐批点出的数）、写回账（写回 API/软件的数）。三数
//! 对平的含义是：对每个数位，声明合计 = 重放合计 = 写回合计，且逐批
//! 亦相等；任何一处不等，段账不成账。
//!
//! 批内自洽规则：
//!   - 一张卡恰悬十道判据（判据数 = 卡数 × 10）；
//!   - 每批行数不得低于 260 行（清账卡的行数地板）；
//!   - 重放只能逐位单调推进，不得越位、不得超声明；
//!   - 写回必须等三数齐备，且逐位等于重放数；
//!   - 批闭合须判据全闭、三数齐、写回齐。
//!
//! 拒止面：批号越界、批序倒挂、数位越界、声明不自洽、重复登记、
//! 未齐即写回、重放超声明、写回与重放不符、闭合断言不成立、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f240;
/// 批号越界
pub const SG_NO: u32 = SEG_BASE + 0x01;
/// 批序倒挂或数位倒挂
pub const SG_ORDER: u32 = SEG_BASE + 0x02;
/// 数位越界（只有卡/判据/行三位）
pub const SG_THREE: u32 = SEG_BASE + 0x03;
/// 声明不自洽（判据 ≠ 卡数×10，或行数低于地板）
pub const SG_DECL: u32 = SEG_BASE + 0x04;
/// 重复声明或重复重放
pub const SG_DUP: u32 = SEG_BASE + 0x05;
/// 未齐即写回
pub const SG_SHORT: u32 = SEG_BASE + 0x06;
/// 重放超声明
pub const SG_OVER: u32 = SEG_BASE + 0x07;
/// 写回与重放不符
pub const SG_MISMATCH: u32 = SEG_BASE + 0x08;
/// 闭合断言不成立
pub const SG_CLOSE: u32 = SEG_BASE + 0x09;
/// 总账已封盘
pub const SG_FROZEN: u32 = SEG_BASE + 0x0a;

/// 段内批数。
pub const BATCHES: usize = 20;
/// 数位数：卡 / 判据 / 行。
pub const THREE: usize = 3;
/// 数位：卡。
pub const IDX_CARDS: usize = 0;
/// 数位：判据。
pub const IDX_CRITS: usize = 1;
/// 数位：行。
pub const IDX_LINES: usize = 2;
/// 数位名。
pub const IDX_NAMES: [&str; THREE] = ["卡", "判据", "行"];
/// 每张卡悬的判据数。
pub const CRITS_PER_CARD: u32 = 10;
/// 每批行数地板。
pub const LINE_MIN: u32 = 260;
/// 二十批声明基线（卡）。
pub const BASE_CARDS: u32 = 1;
/// 二十批声明基线（判据）。
pub const BASE_CRITS: u32 = BASE_CARDS * CRITS_PER_CARD;
/// 二十批声明基线（行）。
pub const BASE_LINES: u32 = 320;
/// 段一判据合计（20 批 × 10）。
pub const CRIT_TOTAL: u32 = BATCHES as u32 * BASE_CRITS;
/// 段一行数合计（20 批 × 320）。
pub const LINE_TOTAL: u32 = BATCHES as u32 * BASE_LINES;
/// 段一卡数合计。
pub const CARD_TOTAL: u32 = BATCHES as u32 * BASE_CARDS;
/// 段一三数合计（按数位序）。
pub const TOTALS: [u32; THREE] = [CARD_TOTAL, CRIT_TOTAL, LINE_TOTAL];
/// 段一判据名。
pub const SEG_NAME: &str = "清账段一(B01–B20)";

/// 批名（B01~B20，两位补零由调用方排版）。
pub const BATCH_NAMES: [&str; BATCHES] = [
    "B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B09", "B10", "B11", "B12", "B13",
    "B14", "B15", "B16", "B17", "B18", "B19", "B20",
];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 声明自洽校验：卡非零、判据恰为卡数×10、行数不低于地板。
pub fn decl_ok(cards: u32, crits: u32, lines: u32) -> bool {
    cards != 0
        && crits == cards.wrapping_mul(CRITS_PER_CARD)
        && lines >= LINE_MIN
}

/// 三账逐位对平：声明 = 重放 = 写回。
pub fn three_balanced(declared: &[u32; THREE], replayed: &[u32; THREE], written: &[u32; THREE]) -> bool {
    let mut i = 0usize;
    while i < THREE {
        let (d, r, w) = match (declared.get(i), replayed.get(i), written.get(i)) {
            (Some(d), Some(r), Some(w)) => (*d, *r, *w),
            _ => return false,
        };
        if d != r || r != w {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 一批的段账（三数三账）。
#[derive(Clone, Copy, Debug)]
pub struct BatchRec {
    /// 批号（1..=20）。
    pub no: usize,
    declared: [u32; THREE],
    replayed: [u32; THREE],
    written: [u32; THREE],
    replay_cursor: usize,
    write_cursor: usize,
    declared_ok: bool,
    crits_total: u32,
    crits_closed: u32,
    closed: bool,
}

impl BatchRec {
    /// 空白批账。
    pub const fn new(no: usize) -> BatchRec {
        BatchRec {
            no,
            declared: [0u32; THREE],
            replayed: [0u32; THREE],
            written: [0u32; THREE],
            replay_cursor: 0,
            write_cursor: 0,
            declared_ok: false,
            crits_total: 0,
            crits_closed: 0,
            closed: false,
        }
    }

    /// 立声明账。
    pub fn declare(&mut self, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.closed {
            return Err(SG_CLOSE);
        }
        if self.declared_ok {
            return Err(SG_DUP);
        }
        if !decl_ok(cards, crits, lines) {
            return Err(SG_DECL);
        }
        self.declared = [cards, crits, lines];
        self.crits_total = crits;
        self.declared_ok = true;
        Ok(())
    }

    /// 重放账：逐位单调推进，不得超声明。
    pub fn replay(&mut self, idx: usize, v: u32) -> Result<(), u32> {
        if self.closed {
            return Err(SG_CLOSE);
        }
        if idx >= THREE {
            return Err(SG_THREE);
        }
        if !self.declared_ok {
            return Err(SG_SHORT);
        }
        if idx != self.replay_cursor {
            return Err(SG_ORDER);
        }
        let cap = match self.declared.get(idx) {
            Some(x) => *x,
            None => return Err(SG_THREE),
        };
        if v > cap {
            return Err(SG_OVER);
        }
        if let Some(slot) = self.replayed.get_mut(idx) {
            *slot = v;
        }
        self.replay_cursor = self.replay_cursor.wrapping_add(1);
        Ok(())
    }

    /// 写回账：三数齐后方可写，逐位须等于重放数。
    pub fn writeback(&mut self, idx: usize, v: u32) -> Result<(), u32> {
        if self.closed {
            return Err(SG_CLOSE);
        }
        if idx >= THREE {
            return Err(SG_THREE);
        }
        if self.replay_cursor != THREE {
            return Err(SG_SHORT);
        }
        if idx != self.write_cursor {
            return Err(SG_ORDER);
        }
        let want = match self.replayed.get(idx) {
            Some(x) => *x,
            None => return Err(SG_THREE),
        };
        if v != want {
            return Err(SG_MISMATCH);
        }
        if let Some(slot) = self.written.get_mut(idx) {
            *slot = v;
        }
        self.write_cursor = self.write_cursor.wrapping_add(1);
        Ok(())
    }

    /// 判据收口登记。
    pub fn close_crits(&mut self, n: u32) -> Result<(), u32> {
        if self.closed {
            return Err(SG_CLOSE);
        }
        if !self.declared_ok {
            return Err(SG_SHORT);
        }
        let next = self.crits_closed.wrapping_add(n);
        if next > self.crits_total {
            return Err(SG_CLOSE);
        }
        self.crits_closed = next;
        Ok(())
    }

    /// 批闭合：三数齐、写回齐、判据全闭。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if !self.declared_ok || self.replay_cursor != THREE || self.write_cursor != THREE {
            return Err(SG_SHORT);
        }
        if self.crits_closed != self.crits_total {
            return Err(SG_CLOSE);
        }
        if !three_balanced(&self.declared, &self.replayed, &self.written) {
            return Err(SG_MISMATCH);
        }
        self.closed = true;
        Ok(())
    }

    /// 是否已声明。
    pub fn declared(&self) -> bool {
        self.declared_ok
    }

    /// 声明数（数位）。
    pub fn declared_at(&self, idx: usize) -> u32 {
        match self.declared.get(idx) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 重放数（数位）。
    pub fn replayed_at(&self, idx: usize) -> u32 {
        match self.replayed.get(idx) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 写回数（数位）。
    pub fn written_at(&self, idx: usize) -> u32 {
        match self.written.get(idx) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 重放游标。
    pub fn replay_cursor(&self) -> usize {
        self.replay_cursor
    }

    /// 写回游标。
    pub fn write_cursor(&self) -> usize {
        self.write_cursor
    }

    /// 判据收口数。
    pub fn crits_closed(&self) -> u32 {
        self.crits_closed
    }

    /// 判据总数。
    pub fn crits_total(&self) -> u32 {
        self.crits_total
    }

    /// 是否闭合。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 本批三数逐位对平。
    pub fn balanced(&self) -> bool {
        self.replay_cursor == THREE
            && self.write_cursor == THREE
            && three_balanced(&self.declared, &self.replayed, &self.written)
    }

    /// 批账指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < THREE {
            if let (Some(d), Some(r), Some(w)) =
                (self.declared.get(i), self.replayed.get(i), self.written.get(i))
            {
                h ^= *d as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= *r as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= *w as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h ^= self.replay_cursor as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.write_cursor as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.crits_closed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.closed { 1 } else { 0 };
        h
    }
}

/// 段一总账（B01–B20）。
#[derive(Clone, Debug)]
pub struct SegmentLedger {
    /// 段内基址。
    pub base: u32,
    /// 段名。
    pub name: &'static str,
    batches: [BatchRec; BATCHES],
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl SegmentLedger {
    /// 新建二十个空白批账。
    pub fn new(base: u32) -> SegmentLedger {
        let mut batches = [BatchRec::new(1); BATCHES];
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = batches.get_mut(i) {
                *b = BatchRec::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        SegmentLedger {
            base,
            name: SEG_NAME,
            batches,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 批账只读。
    pub fn batch(&self, no: usize) -> Option<BatchRec> {
        if no == 0 || no > BATCHES {
            return None;
        }
        self.batches.get(no - 1).copied()
    }

    /// 立声明账（批号须紧接游标）。
    pub fn declare(&mut self, no: usize, cards: u32, crits: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(SG_NO));
        }
        // 重复声明先于批序判定：同一批二次立账是「重登」而非「倒挂」。
        if let Some(b) = self.batches.get(no - 1) {
            if b.declared() {
                return Err(self.fail(SG_DUP));
            }
        }
        if no != self.cursor.wrapping_add(1) {
            return Err(self.fail(SG_ORDER));
        }
        let r = match self.batches.get_mut(no - 1) {
            Some(b) => b.declare(cards, crits, lines),
            None => Err(SG_NO),
        };
        match r {
            Ok(()) => {
                self.cursor = no;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 重放一位。
    pub fn replay_one(&mut self, no: usize, idx: usize, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(SG_NO));
        }
        let r = match self.batches.get_mut(no - 1) {
            Some(b) => b.replay(idx, v),
            None => Err(SG_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 写回一位。
    pub fn writeback(&mut self, no: usize, idx: usize, v: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(SG_NO));
        }
        let r = match self.batches.get_mut(no - 1) {
            Some(b) => b.writeback(idx, v),
            None => Err(SG_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 判据收口登记。
    pub fn close_crits(&mut self, no: usize, n: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(SG_NO));
        }
        let r = match self.batches.get_mut(no - 1) {
            Some(b) => b.close_crits(n),
            None => Err(SG_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 批闭合。
    pub fn close_batch(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_FROZEN));
        }
        if no == 0 || no > BATCHES {
            return Err(self.fail(SG_NO));
        }
        let r = match self.batches.get_mut(no - 1) {
            Some(b) => b.close(),
            None => Err(SG_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 某数位三账合计：[声明, 重放, 写回]。
    pub fn sums(&self, idx: usize) -> [u32; 3] {
        let mut out = [0u32; 3];
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                out[0] = out[0].wrapping_add(b.declared_at(idx));
                out[1] = out[1].wrapping_add(b.replayed_at(idx));
                out[2] = out[2].wrapping_add(b.written_at(idx));
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 段账三数对平（逐数位、逐账）。
    pub fn balanced(&self) -> bool {
        let mut i = 0usize;
        while i < THREE {
            let s = self.sums(i);
            if s[0] != s[1] || s[1] != s[2] || s[0] != TOTALS[i] {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 已闭合批数。
    pub fn closed_batches(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                if b.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 逐批对平批数。
    pub fn balanced_batches(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                if b.balanced() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 判据合计（声明侧）。
    pub fn crits_declared(&self) -> u32 {
        self.sums(IDX_CRITS)[0]
    }

    /// 行数合计（声明侧）。
    pub fn lines_declared(&self) -> u32 {
        self.sums(IDX_LINES)[0]
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
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                h ^= b.digest();
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

    /// 段账终重放：二十批闭合、三数对平、判据与行数合计对平，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.closed_batches() != BATCHES {
            return Err(self.fail(SG_CLOSE));
        }
        if !self.balanced() {
            return Err(self.fail(SG_MISMATCH));
        }
        if self.crits_declared() != CRIT_TOTAL || self.lines_declared() != LINE_TOTAL {
            return Err(self.fail(SG_DECL));
        }
        self.freeze()
    }
}

impl SegmentLedger {
    /// 已立声明账批数。
    pub fn declared_batches(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                if b.declared() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已写回齐批数。
    pub fn written_batches(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                if b.write_cursor() == THREE {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }
}

/// 搭建段一总账：二十批逐批声明、三数重放、三数写回、判据收口、批闭合。
pub fn build_segment(base: u32) -> (SegmentLedger, Result<u64, u32>) {
    let mut g = SegmentLedger::new(base);
    let mut no = 1usize;
    while no <= BATCHES {
        if g.declare(no, BASE_CARDS, BASE_CRITS, BASE_LINES).is_err() {
            return (g, Err(SG_DECL));
        }
        // 重放：三数逐位点账。
        let want = match g.batch(no) {
            Some(b) => [b.declared_at(0), b.declared_at(1), b.declared_at(2)],
            None => return (g, Err(SG_NO)),
        };
        let mut idx = 0usize;
        while idx < THREE {
            let v = match want.get(idx) {
                Some(x) => *x,
                None => 0,
            };
            if g.replay_one(no, idx, v).is_err() {
                return (g, Err(SG_ORDER));
            }
            idx = idx.wrapping_add(1);
        }
        // 写回：三数逐位回填。
        let mut widx = 0usize;
        while widx < THREE {
            let v = match g.batch(no) {
                Some(b) => b.replayed_at(widx),
                None => 0,
            };
            if g.writeback(no, widx, v).is_err() {
                return (g, Err(SG_MISMATCH));
            }
            widx = widx.wrapping_add(1);
        }
        if g.close_crits(no, BASE_CRITS).is_err() {
            return (g, Err(SG_CLOSE));
        }
        if g.close_batch(no).is_err() {
            return (g, Err(SG_CLOSE));
        }
        no = no.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：声明不自洽、重放超声明、未齐即写回、写回与重放不符。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 行数低于地板落 SG_DECL。
    let mut a = SegmentLedger::new(0x3600);
    out[0] = a.declare(1, 1, 10, LINE_MIN - 1).err() == Some(SG_DECL) && a.rejects() == 1;

    // ② 重放超声明落 SG_OVER。
    let mut b = SegmentLedger::new(0x3601);
    let ok2 = b.declare(1, 1, 10, 300).is_ok();
    out[1] = ok2 && b.replay_one(1, 0, 2).err() == Some(SG_OVER) && b.rejects() == 1;

    // ③ 三数未齐即写回落 SG_SHORT。
    let mut c = SegmentLedger::new(0x3602);
    let ok3 = c.declare(1, 1, 10, 300).is_ok();
    out[2] = ok3 && c.writeback(1, 0, 1).err() == Some(SG_SHORT) && c.rejects() == 1;

    // ④ 写回与重放不符落 SG_MISMATCH。
    let mut d = SegmentLedger::new(0x3603);
    let mut ok4 = d.declare(1, 1, 10, 300).is_ok();
    let mut i = 0usize;
    while i < THREE {
        let v = if i == 0 { 1 } else { 10 };
        if d.replay_one(1, i, v).is_err() {
            ok4 = false;
        }
        i = i.wrapping_add(1);
    }
    out[3] = ok4 && d.writeback(1, 0, 2).err() == Some(SG_MISMATCH) && d.rejects() == 1;

    out
}

/// 判据集：C1~C13。
pub fn run_unx11136_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11136.seg1_grand_ledger");
    let (g, r) = build_segment(0x1136);

    // C1 常量面。
    let c1 = SEG_BASE == 0x1f240
        && BATCHES == 20
        && THREE == 3
        && LINE_MIN == 260
        && CRITS_PER_CARD == 10
        && CRIT_TOTAL == 200
        && LINE_TOTAL == 6400
        && CARD_TOTAL == 20
        && TOTALS == [20, 200, 6400]
        && BATCH_NAMES.len() == BATCHES
        && IDX_NAMES.len() == THREE;
    s.add("C1-常量面", c1, "20 批，三数合计 20/200/6400，行数地板 260");

    // C2 二十批声明齐。
    s.add_dyn(
        "C2-声明齐账",
        g.declared_batches() == BATCHES,
        &format!("declared={}/{}", g.declared_batches(), BATCHES),
    );

    // C3 三数对平：三个数位、三本账合计全等。
    let mut c3 = g.balanced();
    let mut i3 = 0usize;
    while i3 < THREE {
        let sm = g.sums(i3);
        c3 = c3 && sm[0] == TOTALS[i3] && sm[1] == TOTALS[i3] && sm[2] == TOTALS[i3];
        i3 = i3.wrapping_add(1);
    }
    s.add_dyn(
        "C3-三数对平",
        c3,
        &format!(
            "cards={:?} crits={:?} lines={:?}",
            g.sums(0),
            g.sums(1),
            g.sums(2)
        ),
    );

    // C4 逐批闭合 20/20。
    s.add_dyn(
        "C4-批闭合 20/20",
        g.closed_batches() == BATCHES && g.balanced_batches() == BATCHES,
        &format!(
            "closed={}/{} balanced={}/{}",
            g.closed_batches(),
            BATCHES,
            g.balanced_batches(),
            BATCHES
        ),
    );

    // C5 判据合计 200，逐批十道全收。
    let mut c5 = g.crits_declared() == CRIT_TOTAL;
    let mut i5 = 1usize;
    while i5 <= BATCHES {
        match g.batch(i5) {
            Some(b) => c5 = c5 && b.crits_closed() == b.crits_total() && b.crits_total() == BASE_CRITS,
            None => c5 = false,
        }
        i5 = i5.wrapping_add(1);
    }
    s.add_dyn(
        "C5-判据合计",
        c5,
        &format!("crits={}/{}", g.crits_declared(), CRIT_TOTAL),
    );

    // C6 行数合计不下地板。
    s.add_dyn(
        "C6-行数守恒",
        g.lines_declared() == LINE_TOTAL
            && g.lines_declared() >= (LINE_MIN.wrapping_mul(BATCHES as u32)),
        &format!("lines={}/{}", g.lines_declared(), LINE_TOTAL),
    );

    // C7 段账终重放成立并封盘。
    s.add("C7-段账封盘", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // C8 重放幂等。
    let mut c8 = g.clone();
    let before = c8.fold();
    let again = c8.replay();
    s.add(
        "C8-重放幂等",
        again.is_ok() && c8.fold() == before && c8.rejects() == g.rejects(),
        "二次重放同指纹、零新增拒止",
    );

    // C9 封盘幂等。
    let mut c9 = SegmentLedger::new(0x1136);
    let f1 = c9.freeze();
    let f2 = c9.freeze();
    s.add("C9-封盘幂等", f1.is_ok() && f1 == f2, "两次封盘同指纹");

    // C10 拒止优先序：封盘先于批号校验。
    let mut c10 = g.clone();
    let z1 = c10.declare(BATCHES + 1, 1, 10, 300).err();
    let mut c10b = SegmentLedger::new(0x1137);
    let z2 = c10b.declare(BATCHES + 1, 1, 10, 300).err();
    s.add(
        "C10-拒止优先序",
        z1 == Some(SG_FROZEN) && z2 == Some(SG_NO),
        "封盘优先于批号；未封盘则 SG_NO",
    );

    // C11 四路注入。
    let inj = inject_four();
    s.add(
        "C11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "声明/超声明/未齐/不符四路各自发火",
    );

    // C12 指纹灵敏度。
    let (g2, _) = build_segment(0x1138);
    let (g3, _) = build_segment(0x1136);
    s.add(
        "C12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址相异、同基址相同",
    );

    // C13 逐例归零。
    s.add(
        "C13-逐例归零",
        g.rejects() == 0
            && g.closed_batches() == BATCHES
            && g.written_batches() == BATCHES
            && g.balanced(),
        "rejects=0，20 批写回齐、三数对平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11136_flow() {
        let (g, r) = build_segment(0x5000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.declared_batches(), BATCHES);
        assert_eq!(g.written_batches(), BATCHES);
        assert_eq!(g.closed_batches(), BATCHES);
        assert_eq!(g.balanced_batches(), BATCHES);
        assert_eq!(g.sums(IDX_CARDS), [20, 20, 20]);
        assert_eq!(g.sums(IDX_CRITS), [200, 200, 200]);
        assert_eq!(g.sums(IDX_LINES), [6400, 6400, 6400]);
        assert!(g.balanced());
        assert_eq!(g.rejects(), 0);
        // 首批三数三账逐位相等。
        let b1 = match g.batch(1) {
            Some(b) => b,
            None => return,
        };
        assert_eq!(b1.declared_at(IDX_CARDS), 1);
        assert_eq!(b1.replayed_at(IDX_CRITS), 10);
        assert_eq!(b1.written_at(IDX_LINES), 320);
        assert_eq!(b1.replay_cursor(), 3);
    }

    #[test]
    fn f11136_gates() {
        let mut g = SegmentLedger::new(0x5001);
        assert_eq!(g.declare(2, 1, 10, 300).err(), Some(SG_ORDER));
        assert_eq!(g.declare(1, 1, 9, 300).err(), Some(SG_DECL));
        assert!(g.declare(1, 1, 10, 300).is_ok());
        assert_eq!(g.declare(1, 1, 10, 300).err(), Some(SG_DUP));
        assert_eq!(g.replay_one(1, 1, 10).err(), Some(SG_ORDER));
        assert_eq!(g.replay_one(1, 0, 2).err(), Some(SG_OVER));
        assert!(g.replay_one(1, 0, 1).is_ok());
        assert!(g.replay_one(1, 1, 10).is_ok());
        assert!(g.replay_one(1, 2, 300).is_ok());
        assert_eq!(g.writeback(1, 0, 2).err(), Some(SG_MISMATCH));
        assert_eq!(g.close_batch(1).err(), Some(SG_SHORT));
        assert_eq!(g.close_crits(1, 11).err(), Some(SG_CLOSE));
        assert_eq!(g.rejects(), 8);
        assert!(g.freeze().is_ok());
        assert_eq!(g.declare(2, 1, 10, 300).err(), Some(SG_FROZEN));
        assert_eq!(g.rejects(), 9);
    }

    #[test]
    fn f11136_rules() {
        assert!(decl_ok(1, 10, 260));
        assert!(!decl_ok(0, 10, 260));
        assert!(!decl_ok(1, 9, 260));
        assert!(!decl_ok(1, 10, 259));
        let d = [1u32, 10, 320];
        let mut same = [1u32, 10, 320];
        assert!(three_balanced(&d, &same, &same));
        same[1] = 11;
        assert!(!three_balanced(&d, &same, &d));
        // 段名与批名表。
        assert_eq!(SEG_NAME, "清账段一(B01–B20)");
        assert_eq!(BATCH_NAMES[0], "B01");
        assert_eq!(BATCH_NAMES[BATCHES - 1], "B20");
        // 批账单调性：未声明不得重放。
        let mut b = BatchRec::new(3);
        assert_eq!(b.replay(0, 1).err(), Some(SG_SHORT));
        assert_err_decl(&mut b);
    }

    fn assert_err_decl(b: &mut BatchRec) {
        assert_eq!(b.declare(2, 10, 260).err(), Some(SG_DECL));
    }

    #[test]
    fn f11136_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11136_checks_green() {
        let cs = run_unx11136_checks();
        assert!(cs.all_passed(), "checks red in unx11136");
    }
}
