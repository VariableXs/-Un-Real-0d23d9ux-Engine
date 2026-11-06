//! UNX-F50120 B26 批收官印（20 条冻结 / 累计 156,000 行）（0x43b20 段）
//!
//! 锚点：UNX-F50120-J1 批印入册：20/20 冻结、域账 156,000/240,000；
//! 判据锚定 §AI-63 §7。
//!
//! 收官印不是一句口号，而是一张**可复算的凭据**：它必须由 20 张卡的
//! （编号, 行数, 冻结位）三元组聚合出来，并且聚合过程要满足
//!
//! * 计数恰好 20（少一张就是「批没收完」，多一张是「串批」）；
//! * 每张卡都已冻结（未冻结的卡行数还会变，印上去就是虚报）；
//! * 行数合计 ≥ 156,000（卡面承诺），且 ≤ 240,000（域容量，超了就是串域）；
//! * 印的摘要由 (计数, 行数, 容量) 三元组决定，任何一项被改都会被归档端复算打回。
//!
//! 配额整分：156,000 / 20 = 7,800 行/卡，恰好整除——这是本批行数目标可以
//! 「按卡摊平」的前提；若不能整除，摊平会引入 ±1 的漂移，收官印就没法逐项对账。
//!
//! 状态机：`Draft → Collecting → Frozen → Sealed`，只允许单向推进；
//! `Sealed` 之后既不能补卡也不能重盖，重盖必须新开一本（「印错不改印、只重开」）。
//!
//! 数据结构：
//! * `BookState` —— 四态账本状态机（单向 + 幂等局部动作）；
//! * `Ticket` / `SealBook` —— 20 张卡的定容登记簿（编号严格递增）；
//! * `Seal` —— 收官印凭据（含万分比与内容摘要）；
//! * `SealArchive` —— 印册（摘要复算 + 册序不回填）。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43b20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43b20;

pub const E_STATE: u16 = 1;
pub const E_COUNT: u16 = 2;
pub const E_NOT_FROZEN: u16 = 3;
pub const E_SHORT: u16 = 4;
pub const E_OVER: u16 = 5;
pub const E_RANGE: u16 = 6;
pub const E_DUP: u16 = 7;
pub const E_ORDER: u16 = 8;
pub const E_MISSING: u16 = 9;
pub const E_DIGEST: u16 = 10;
pub const E_SEQ: u16 = 11;
pub const E_CAPACITY: u16 = 12;

/// 收官印错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    State,
    Count,
    NotFrozen,
    Short,
    Over,
    Range,
    Dup,
    Order,
    Missing,
    Digest,
    Seq,
    Capacity,
}

impl SealError {
    pub fn code(self) -> u32 {
        let n = match self {
            SealError::State => E_STATE,
            SealError::Count => E_COUNT,
            SealError::NotFrozen => E_NOT_FROZEN,
            SealError::Short => E_SHORT,
            SealError::Over => E_OVER,
            SealError::Range => E_RANGE,
            SealError::Dup => E_DUP,
            SealError::Order => E_ORDER,
            SealError::Missing => E_MISSING,
            SealError::Digest => E_DIGEST,
            SealError::Seq => E_SEQ,
            SealError::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            SealError::State => "book state does not allow this transition",
            SealError::Count => "ticket count is not 20",
            SealError::NotFrozen => "ticket still mutable (not frozen)",
            SealError::Short => "accumulated lines below 156,000",
            SealError::Over => "accumulated lines above domain cap 240,000",
            SealError::Range => "single ticket line count out of 1..=15,600",
            SealError::Dup => "ticket id repeated",
            SealError::Order => "ticket id went backwards",
            SealError::Missing => "ticket id not in this book",
            SealError::Digest => "seal digest does not match content",
            SealError::Seq => "seal book sequence must be dense and monotone",
            SealError::Capacity => "archive capacity reached",
        }
    }
}

/// B26 批常量（卡面承诺值）。
pub const BATCH_CARDS: u16 = 20;
pub const BATCH_LINES: u32 = 156_000;
pub const DOMAIN_CAP: u32 = 240_000;
/// 单卡上界 = 2 × 均摊配额（7,800 × 2）。留一倍余量是为了容纳行数分布不均，
/// 同时保证「20 张卡不可能同时顶格」之外的串域写法一定被 `Over` 挡住。
pub const MAX_TICKET_LINES: u32 = 15_600;
/// 均摊配额：156,000 / 20 = 7,800（整除判据的推导值）。
pub const QUOTA_PER_CARD: u32 = BATCH_LINES / BATCH_CARDS as u32;

/// 账本四态。只允许沿枚举顺序单向推进；`add` 只在 Draft/Collecting 可用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    Draft,
    Collecting,
    Frozen,
    Sealed,
}

impl BookState {
    pub fn label(self) -> &'static str {
        match self {
            BookState::Draft => "draft",
            BookState::Collecting => "collecting",
            BookState::Frozen => "frozen",
            BookState::Sealed => "sealed",
        }
    }

    /// 该状态下是否还允许登记新卡。
    pub fn accepts_cards(self) -> bool {
        matches!(self, BookState::Draft | BookState::Collecting)
    }
}

/// 一张 B26 卡。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub id: u16,
    pub lines: u32,
    pub frozen: bool,
}

/// 20 张卡的定容登记簿。
pub struct SealBook {
    tickets: Vec<Ticket>,
    state: BookState,
    freeze_replays: u16,
}

impl SealBook {
    pub fn new() -> SealBook {
        SealBook {
            tickets: Vec::with_capacity(BATCH_CARDS as usize),
            state: BookState::Draft,
            freeze_replays: 0,
        }
    }

    pub fn state(&self) -> BookState {
        self.state
    }

    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    pub fn freeze_replays(&self) -> u16 {
        self.freeze_replays
    }

    pub fn get(&self, id: u16) -> Option<&Ticket> {
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if t.id == id {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一张卡。编号必须严格递增——账本顺序即卡面顺序，乱序会让
    /// 「第 N 张」在两次收口里指向不同的卡，摘要就失去可比性。
    pub fn add(&mut self, id: u16, lines: u32) -> Result<(), SealError> {
        if !self.state.accepts_cards() {
            return Err(SealError::State);
        }
        if self.tickets.len() >= BATCH_CARDS as usize {
            return Err(SealError::Count);
        }
        if lines == 0 || lines > MAX_TICKET_LINES {
            return Err(SealError::Range);
        }
        if let Some(last) = self.tickets.last() {
            if last.id == id {
                return Err(SealError::Dup);
            }
            if id < last.id {
                return Err(SealError::Order);
            }
        }
        self.tickets.push(Ticket {
            id,
            lines,
            frozen: false,
        });
        if self.state == BookState::Draft {
            self.state = BookState::Collecting;
        }
        Ok(())
    }

    /// 冻结一张卡。重复冻结幂等（只计一次），冻结后行数不可再改——
    /// 本模块没有 `set_lines`，这正是「冻结」的语义来源。
    pub fn freeze(&mut self, id: u16) -> Result<(), SealError> {
        if self.state == BookState::Sealed {
            return Err(SealError::State);
        }
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if t.id == id {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        let at = match idx {
            Some(a) => a,
            None => return Err(SealError::Missing),
        };
        let already = match self.tickets.get(at) {
            Some(t) => t.frozen,
            None => return Err(SealError::Missing),
        };
        if already {
            self.freeze_replays += 1;
            return Ok(());
        }
        if let Some(t) = self.tickets.get_mut(at) {
            t.frozen = true;
        }
        Ok(())
    }

    pub fn frozen_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if t.frozen {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 合计（计数, 行数）。每次现算，不缓存——缓存的合计一旦与明细脱钩，
    /// 就是最典型的「账实不符」。
    pub fn totals(&self) -> (u16, u32) {
        let mut n = 0u16;
        let mut lines: u32 = 0;
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                n += 1;
                lines = lines.saturating_add(t.lines);
            }
            i += 1;
        }
        (n, lines)
    }

    /// 收口：Collecting → Frozen。要求 20 张齐、全部冻结。
    pub fn close(&mut self) -> Result<(), SealError> {
        if self.state == BookState::Sealed {
            return Err(SealError::State);
        }
        let (n, _) = self.totals();
        if n != BATCH_CARDS {
            return Err(SealError::Count);
        }
        if self.frozen_count() != BATCH_CARDS {
            return Err(SealError::NotFrozen);
        }
        self.state = BookState::Frozen;
        Ok(())
    }

    /// 盖印：Frozen → Sealed，产出可复算的凭据。
    ///
    /// 四个门槛按「越便宜越先查、越致命越先查」的折中排序：状态 → 计数 →
    /// 冻结 → 行数区间。行数区间放在最后是因为它需要遍历累加。
    pub fn seal(&mut self) -> Result<Seal, SealError> {
        if self.state != BookState::Frozen {
            return Err(SealError::State);
        }
        let (count, lines) = self.totals();
        if count != BATCH_CARDS {
            return Err(SealError::Count);
        }
        if self.frozen_count() != BATCH_CARDS {
            return Err(SealError::NotFrozen);
        }
        if lines < BATCH_LINES {
            return Err(SealError::Short);
        }
        if lines > DOMAIN_CAP {
            return Err(SealError::Over);
        }
        self.state = BookState::Sealed;
        let digest = seal_digest(count, lines, DOMAIN_CAP);
        Ok(Seal {
            count,
            lines,
            cap: DOMAIN_CAP,
            ratio_bp: ratio_bp(lines, DOMAIN_CAP),
            digest,
            seq: 0,
            frozen: count == BATCH_CARDS,
        })
    }
}

/// 万分比（整数，u64 中间量防 156,000 × 10,000 在 u32 上溢出）。
pub fn ratio_bp(lines: u32, cap: u32) -> u32 {
    if cap == 0 {
        return 0;
    }
    ((lines as u64 * 10_000) / cap as u64) as u32
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 印面摘要：(计数 ‖ 行数 ‖ 容量)。三项任一被改，归档端复算即报 E_DIGEST。
pub fn seal_digest(count: u16, lines: u32, cap: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv_u8(h, (count & 0xff) as u8);
    h = fnv_u8(h, (count >> 8) as u8);
    h = fnv_u32(h, lines);
    h = fnv_u32(h, cap);
    h
}

/// 收官印凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seal {
    pub count: u16,
    pub lines: u32,
    pub cap: u32,
    pub ratio_bp: u32,
    pub digest: u64,
    pub seq: u32,
    pub frozen: bool,
}

impl Seal {
    /// 20/20 冻结且达标。
    pub fn complete(&self) -> bool {
        self.count == BATCH_CARDS && self.frozen && self.lines >= BATCH_LINES
    }

    /// 换册序（同一凭据入另一本印册时用；印面内容不变，故摘要不变）。
    pub fn renumber(self, seq: u32) -> Seal {
        Seal { seq, ..self }
    }
}

/// 印册：只收摘要可复算、册序稠密递增的凭据。
pub struct SealArchive {
    seals: Vec<Seal>,
    cap: usize,
}

impl SealArchive {
    pub fn new(cap: usize) -> Result<SealArchive, SealError> {
        if cap == 0 || cap > 16 {
            return Err(SealError::Capacity);
        }
        Ok(SealArchive {
            seals: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.seals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seals.is_empty()
    }

    pub fn last(&self) -> Option<&Seal> {
        self.seals.last()
    }

    /// 入册。先复算摘要（防篡改），再校册序（防回填），最后查容量。
    pub fn register(&mut self, s: Seal) -> Result<u32, SealError> {
        if seal_digest(s.count, s.lines, s.cap) != s.digest {
            return Err(SealError::Digest);
        }
        let expected = self.seals.len() as u32;
        if s.seq != expected {
            return Err(SealError::Seq);
        }
        if self.seals.len() >= self.cap {
            return Err(SealError::Capacity);
        }
        self.seals.push(s);
        Ok(expected)
    }

    /// 印册摘要（用于「同一批两次收口印面一致」）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.seals.len() as u64;
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(s) = self.seals.get(i) {
                h ^= s.digest.rotate_left(7);
                h = h.wrapping_mul(0x100_0000_01b3);
                h ^= s.lines as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            i += 1;
        }
        h
    }
}

/// 印面文本（诊断用；不含浮点，便于跨机器逐字节比对）。
pub fn render(s: &Seal) -> String {
    format!(
        "seal {}/{} frozen={} lines={} cap={} bp={} digest={:016x} seq={}",
        s.count, BATCH_CARDS, s.frozen, s.lines, s.cap, s.ratio_bp, s.digest, s.seq
    )
}

fn fill_uniform(book: &mut SealBook, each: u32) -> bool {
    let mut i = 0u16;
    while i < BATCH_CARDS {
        if book.add(50101 + i, each).is_err() {
            return false;
        }
        i += 1;
    }
    true
}

fn freeze_all(book: &mut SealBook) -> bool {
    let mut i = 0u16;
    let mut ok = true;
    while i < BATCH_CARDS {
        if book.freeze(50101 + i).is_err() {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 构造一本合规账（20 × 7,800 = 156,000）。供判据与注入核共用。
pub fn planned_book() -> Result<SealBook, SealError> {
    let mut b = SealBook::new();
    if !fill_uniform(&mut b, QUOTA_PER_CARD) {
        return Err(SealError::Count);
    }
    if !freeze_all(&mut b) {
        return Err(SealError::Missing);
    }
    b.close()?;
    Ok(b)
}

// ---------------------------------------------------------------------------
// 注入核：六类坏账必须全部被拒（各命中专属码）
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1 重复卡号
    let mut dup = SealBook::new();
    if dup.add(50101, 7800).is_ok() {
        if dup.add(50101, 7800).err() == Some(SealError::Dup) {
            caught += 1;
        }
    }

    // 2 卡号倒退
    let mut ord = SealBook::new();
    if ord.add(50102, 7800).is_ok() {
        if ord.add(50101, 7800).err() == Some(SealError::Order) {
            caught += 1;
        }
    }

    // 3 单卡行数 0
    let mut zero = SealBook::new();
    if zero.add(50101, 0).err() == Some(SealError::Range) {
        caught += 1;
    }

    // 4 只有 19 张就收口
    let mut short_count = SealBook::new();
    let mut i = 0u16;
    while i < BATCH_CARDS - 1 {
        let _ = short_count.add(50101 + i, 7800);
        i += 1;
    }
    if short_count.close().err() == Some(SealError::Count) {
        caught += 1;
    }

    // 5 20 张齐但少冻一张
    let mut lazy = SealBook::new();
    let mut k = 0u16;
    while k < BATCH_CARDS {
        let _ = lazy.add(50101 + k, 7800);
        k += 1;
    }
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = lazy.freeze(50101 + f);
        f += 1;
    }
    if lazy.close().err() == Some(SealError::NotFrozen) {
        caught += 1;
    }

    // 6 已全冻结但行数不足就盖印
    let mut light = SealBook::new();
    if fill_uniform(&mut light, 7000) {
        if freeze_all(&mut light) {
            if light.close().is_ok() {
                if light.seal().err() == Some(SealError::Short) {
                    caught += 1;
                }
            }
        }
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50120");

    // J1-1 配额整分：156,000 / 20 = 7,800 且无余数
    let ok = QUOTA_PER_CARD == 7800
        && QUOTA_PER_CARD * BATCH_CARDS as u32 == BATCH_LINES
        && BATCH_LINES % BATCH_CARDS as u32 == 0;
    let det = format!("quota={} total={}", QUOTA_PER_CARD, QUOTA_PER_CARD * BATCH_CARDS as u32);
    s.add_dyn("50120-J1-配额整分", ok, &det);

    // J1-2 目标比：156,000 / 240,000 = 6,500 bp
    let ok = ratio_bp(BATCH_LINES, DOMAIN_CAP) == 6500
        && ratio_bp(0, DOMAIN_CAP) == 0
        && ratio_bp(DOMAIN_CAP, DOMAIN_CAP) == 10_000
        && ratio_bp(5, 0) == 0;
    let det = format!("bp={}", ratio_bp(BATCH_LINES, DOMAIN_CAP));
    s.add_dyn("50120-J1-目标万分比", ok, &det);

    // J1-3 状态机单向：Draft 起手 → Collecting → Frozen → Sealed
    let mut book = match planned_book() {
        Ok(b) => b,
        Err(_) => {
            s.add("50120-J1-账本可建", false, "标准账构造失败");
            return s;
        }
    };
    let st_before = book.state();
    let seal_r = book.seal();
    let st_after = book.state();
    let ok = st_before == BookState::Frozen
        && st_after == BookState::Sealed
        && seal_r.is_ok()
        && SealBook::new().state() == BookState::Draft;
    let det = format!("{}->{}", st_before.label(), st_after.label());
    s.add_dyn("50120-J1-状态单向", ok, &det);

    let seal = match seal_r {
        Ok(x) => x,
        Err(_) => {
            s.add("50120-J1-盖印可用", false, "合规账盖印失败");
            return s;
        }
    };

    // J1-4 印面守恒：20/20 冻结，行数与明细合计逐项相等
    let (detail_count, detail_lines) = book.totals();
    let ok = detail_count == BATCH_CARDS
        && detail_lines == BATCH_LINES
        && seal.count == detail_count
        && seal.lines == detail_lines
        && seal.frozen
        && seal.complete()
        && seal.cap == DOMAIN_CAP;
    let det = format!("{}/{} lines={} bp={}", seal.count, BATCH_CARDS, seal.lines, seal.ratio_bp);
    s.add_dyn("50120-J1-印面守恒", ok, &det);

    // J1-5 摘要确定：两本同构账各盖一印，印面逐位一致
    let twin = match planned_book() {
        Ok(mut t) => match t.seal() {
            Ok(x) => Some(x),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let ok = match twin {
        Some(t) => t == seal && t.digest == seal.digest,
        None => false,
    };
    s.add("50120-J1-摘要确定", ok, if ok { "" } else { "同构账印面漂移" });

    // J1-6 摘要敏感：单卡 +1 行，印面摘要必变（其余三项不变）
    let mut tweak = SealBook::new();
    let mut i = 0u16;
    while i < BATCH_CARDS {
        let lines = if i == 7 { QUOTA_PER_CARD + 1 } else { QUOTA_PER_CARD };
        let _ = tweak.add(50101 + i, lines);
        i += 1;
    }
    let _ = freeze_all(&mut tweak);
    let ok = match tweak.close() {
        Ok(()) => match tweak.seal() {
            Ok(t) => t.lines == seal.lines + 1 && t.digest != seal.digest && t.ratio_bp == seal.ratio_bp,
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50120-J1-摘要敏感", ok, if ok { "" } else { "改行数摘要不变" });

    // J1-7 19 张就收口必拒
    let mut short_count = SealBook::new();
    let mut k = 0u16;
    while k < BATCH_CARDS - 1 {
        let _ = short_count.add(50101 + k, QUOTA_PER_CARD);
        k += 1;
    }
    let ok = short_count.close().err() == Some(SealError::Count);
    s.add("50120-J1-计数不足", ok, if ok { "" } else { "19 张被当成满批" });

    // J1-8 少冻一张就收口必拒
    let mut lazy = SealBook::new();
    let mut k = 0u16;
    while k < BATCH_CARDS {
        let _ = lazy.add(50101 + k, QUOTA_PER_CARD);
        k += 1;
    }
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = lazy.freeze(50101 + f);
        f += 1;
    }
    let ok = lazy.close().err() == Some(SealError::NotFrozen) && lazy.frozen_count() == 19;
    let det = format!("frozen={}", lazy.frozen_count());
    s.add_dyn("50120-J1-冻结不足", ok, &det);

    // J1-9 行数不足盖印必拒（20 × 7,000 = 140,000 < 156,000）
    let mut light = SealBook::new();
    let _ = fill_uniform(&mut light, 7000);
    let _ = freeze_all(&mut light);
    let ok = light.close().is_ok()
        && light.totals() == (BATCH_CARDS, 140_000)
        && light.seal().err() == Some(SealError::Short);
    s.add("50120-J1-行数不足", ok, if ok { "" } else { "140,000 行被当成达标" });

    // J1-10 串域超容盖印必拒（20 × 15,000 = 300,000 > 240,000，单卡仍在界内）
    let mut heavy = SealBook::new();
    let _ = fill_uniform(&mut heavy, 15_000);
    let _ = freeze_all(&mut heavy);
    let ok = heavy.close().is_ok()
        && heavy.totals().1 == 300_000
        && heavy.seal().err() == Some(SealError::Over);
    let det = format!("lines={} cap={}", heavy.totals().1, DOMAIN_CAP);
    s.add_dyn("50120-J1-串域超容", ok, &det);

    // J1-11 单卡界：0 行与超上界都拒，上界本身可收；未知卡号冻结报缺失
    let mut edge = SealBook::new();
    let ok = edge.add(50101, 0).err() == Some(SealError::Range)
        && edge.add(50101, MAX_TICKET_LINES + 1).err() == Some(SealError::Range)
        && edge.add(50101, MAX_TICKET_LINES).is_ok()
        && edge.add(50102, 1).is_ok()
        && edge.freeze(50999).err() == Some(SealError::Missing);
    s.add("50120-J1-单卡界", ok, if ok { "" } else { "单卡行数界未生效" });

    // J1-12 未收口就盖印必拒（Collecting → seal 必须是 State）
    let mut open = SealBook::new();
    let _ = fill_uniform(&mut open, QUOTA_PER_CARD);
    let _ = freeze_all(&mut open);
    let ok = open.state() == BookState::Collecting
        && open.seal().err() == Some(SealError::State);
    s.add("50120-J1-未收口拒印", ok, if ok { "" } else { "跳步盖印被放行" });

    // J1-13 归档册序：首印入册 seq=0，回填同号被拒，顺延重印可入册
    let mut arch = match SealArchive::new(4) {
        Ok(a) => a,
        Err(_) => return s,
    };
    let first = arch.register(seal);
    let backfill = arch.register(seal.renumber(0));
    let again = arch.register(seal.renumber(1));
    let ok = first == Ok(0)
        && backfill.err() == Some(SealError::Seq)
        && again == Ok(1)
        && arch.len() == 2
        && arch.last().map(|x| x.seq == 1).unwrap_or(false)
        && SealArchive::new(0).err() == Some(SealError::Capacity)
        && SealArchive::new(17).err() == Some(SealError::Capacity);
    s.add("50120-J1-册序不回填", ok, if ok { "" } else { "册序可回填" });

    // J1-14 归档复算：篡改行数的印必被摘要打回
    let mut arch2 = match SealArchive::new(2) {
        Ok(a) => a,
        Err(_) => return s,
    };
    let tampered = Seal {
        lines: seal.lines + 1,
        ..seal
    };
    let ok = arch2.register(tampered).err() == Some(SealError::Digest) && arch2.is_empty();
    s.add("50120-J1-篡改打回", ok, if ok { "" } else { "篡改印被收下" });

    // J1-15 冻结幂等：重复冻结只留一次痕，计数不涨
    let mut idem = SealBook::new();
    let _ = idem.add(50101, QUOTA_PER_CARD);
    let _ = idem.freeze(50101);
    let once = idem.frozen_count();
    let _ = idem.freeze(50101);
    let ok = once == 1 && idem.frozen_count() == 1 && idem.freeze_replays() == 1;
    let det = format!("frozen={} replays={}", idem.frozen_count(), idem.freeze_replays());
    s.add_dyn("50120-J1-冻结幂等", ok, &det);

    // J1-16 封版后不可补卡/不可重盖
    let ok = book.add(50121, 100).err() == Some(SealError::State)
        && book.freeze(50101).err() == Some(SealError::State)
        && book.seal().err() == Some(SealError::State)
        && book.len() == BATCH_CARDS as usize;
    s.add("50120-J1-封版不可改", ok, if ok { "" } else { "封版后仍可改写" });

    // J1-17 注入核：六类坏账全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50120-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50120_checks_green() {
        let set = run_unx50120_checks();
        assert!(set.all_passed(), "UNX-F50120 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50120_planned_book_seals_at_6500bp() {
        let mut b = SealBook::new();
        assert!(fill_uniform(&mut b, QUOTA_PER_CARD));
        assert!(freeze_all(&mut b));
        assert!(b.close().is_ok());
        let seal = b.seal();
        assert!(seal.is_ok());
        if let Ok(s) = seal {
            assert_eq!(s.count, 20);
            assert_eq!(s.lines, 156_000);
            assert_eq!(s.ratio_bp, 6500);
            assert!(s.complete());
            assert_eq!(s.digest, seal_digest(20, 156_000, DOMAIN_CAP));
        }
    }

    #[test]
    fn unx50120_digest_binds_content() {
        assert_ne!(
            seal_digest(20, 156_000, DOMAIN_CAP),
            seal_digest(20, 156_001, DOMAIN_CAP)
        );
        assert_eq!(
            seal_digest(20, 156_000, DOMAIN_CAP),
            seal_digest(20, 156_000, DOMAIN_CAP)
        );
    }
}
