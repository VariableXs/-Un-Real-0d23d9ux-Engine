//! UNX-F50140 B27 批收官印（20 条冻结 / 累计 162,000 行）（0x43da0 段）
//!
//! 锚点：UNX-F50140-J1 批印入册：20/20 冻结、域账 162,000/240,000；
//! 判据锚定 §AI-63 §7。
//!
//! 收官印要能独立证明三件事：这一批恰好 20 张卡、每张卡都已冻结、行数合计
//! 落在 162,000（承诺）与 240,000（域容量）之间。为了不让「印」变成一张
//! 谁都能填的表，这里用**链式凭据**：第 i 张卡的摘要必然吃进第 i-1 张的摘要，
//! 最终根 = 第 20 张的摘要。
//!
//! 链式而不是「一次性汇总」的好处有三条，全部落在判据里：
//! * 前缀稳定：前 7 张的链根与后来补了多少张无关，可单独交给审计方复核；
//! * 顺序绑定：把两张卡的行数对调，根必变——「第几张」是印面的一部分；
//! * 定位篡改：审计从第 0 张重算，第一次失配就停在那张卡上，而不是只报
//!   「整本不对」。
//!
//! 162,000 / 20 = 8,100 行/卡是整除的，这是「按卡摊平」能作为基准的前提；
//! 单卡上界取 2 × 8,100 = 16,200（容纳分布不均），但 20 张全顶格会冲到
//! 324,000，必被域容量 240,000 挡住——两条边界一上一下，把「串域」写死。
//!
//! 数据结构（状态机 + 账本 + 印册）：
//! * `SealChain` / `ChainState` —— 20 槽定长链（Empty→Filling→Closed→Sealed）；
//! * `Link` —— 单槽（卡号/行数/冻结位/链摘要）；
//! * `BatchSeal` / `SealArchive` —— 印凭据与印册（册序稠密、批号单调）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43da0 段），十二个码值全部被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43da0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43da0;

pub const E_STATE: u16 = 1;
pub const E_COUNT: u16 = 2;
pub const E_NOT_FROZEN: u16 = 3;
pub const E_SHORT: u16 = 4;
pub const E_OVER: u16 = 5;
pub const E_CARD: u16 = 6;
pub const E_FULL: u16 = 7;
pub const E_MISSING: u16 = 8;
pub const E_RANGE: u16 = 9;
pub const E_CHAIN: u16 = 10;
pub const E_SEQ: u16 = 11;
pub const E_CAP: u16 = 12;

/// 收官印错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    State,
    Count,
    NotFrozen,
    Short,
    Over,
    Card,
    Full,
    Missing,
    Range,
    Chain,
    Seq,
    Cap,
}

impl SealError {
    /// 诊断码 = 段基址 + 序号。
    pub fn code(self) -> u32 {
        let n = match self {
            SealError::State => E_STATE,
            SealError::Count => E_COUNT,
            SealError::NotFrozen => E_NOT_FROZEN,
            SealError::Short => E_SHORT,
            SealError::Over => E_OVER,
            SealError::Card => E_CARD,
            SealError::Full => E_FULL,
            SealError::Missing => E_MISSING,
            SealError::Range => E_RANGE,
            SealError::Chain => E_CHAIN,
            SealError::Seq => E_SEQ,
            SealError::Cap => E_CAP,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            SealError::State => "chain state does not allow this transition",
            SealError::Count => "chain does not hold exactly 20 cards",
            SealError::NotFrozen => "one or more cards are still mutable",
            SealError::Short => "accumulated lines below 162,000",
            SealError::Over => "accumulated lines above domain cap 240,000",
            SealError::Card => "card id is not the next slot id",
            SealError::Full => "chain has no free slot",
            SealError::Missing => "card id not present in this chain",
            SealError::Range => "line count outside 1..=16,200",
            SealError::Chain => "chain digest does not reproduce",
            SealError::Seq => "seal sequence or batch order is not monotone",
            SealError::Cap => "archive capacity invalid or exhausted",
        }
    }
}

/// B27 批常量（卡面承诺值）。
pub const BATCH_NO: u16 = 27;
pub const BATCH_CARDS: u16 = 20;
pub const BATCH_LINES: u32 = 162_000;
pub const DOMAIN_CAP: u32 = 240_000;
/// 20 槽定长——与 `BATCH_CARDS` 同源，判据里交叉核对。
pub const SLOTS: usize = 20;
/// 均摊配额：162,000 / 20 = 8,100（整除是「按卡摊平」的前提）。
pub const QUOTA_PER_CARD: u32 = BATCH_LINES / BATCH_CARDS as u32;
/// 单卡上界 = 2 × 均摊配额。容纳分布不均，但不允许一张卡吃掉整批。
pub const MAX_CARD_LINES: u32 = QUOTA_PER_CARD * 2;
/// B27 首卡编号（50121..50140）。
pub const FIRST_CARD: u16 = 50121;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

fn fnv_u16(mut h: u64, v: u16) -> u64 {
    h = fnv_u8(h, (v & 0xff) as u8);
    fnv_u8(h, (v >> 8) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 单槽链摘要：吃进前一环，因此任何前缀都自带完整历史。
pub fn link_digest(prev: u64, card: u16, lines: u32) -> u64 {
    let mut h = prev;
    h = fnv_u16(h, card);
    h = fnv_u32(h, lines);
    h.wrapping_mul(FNV_PRIME)
}

/// 链状态。单向推进，`Sealed` 之后不可写。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainState {
    Empty,
    Filling,
    Closed,
    Sealed,
}

impl ChainState {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            ChainState::Empty => "empty",
            ChainState::Filling => "filling",
            ChainState::Closed => "closed",
            ChainState::Sealed => "sealed",
        }
    }

    /// 是否允许追加。
    pub fn accepts_cards(self) -> bool {
        matches!(self, ChainState::Empty | ChainState::Filling)
    }
}

/// 单槽明细。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub card: u16,
    pub lines: u32,
    pub frozen: bool,
    pub digest: u64,
}

impl Link {
    /// 空槽哨兵（卡号 0 在真实编号里不可能出现）。
    pub const EMPTY: Link = Link {
        card: 0,
        lines: 0,
        frozen: false,
        digest: 0,
    };
}

/// 20 槽定长链。槽号即卡序，不接受乱序插入。
#[derive(Clone, Debug)]
pub struct SealChain {
    slots: [Link; SLOTS],
    len: usize,
    state: ChainState,
    root: u64,
    freeze_replays: u16,
}

impl SealChain {
    /// 空链（root 为 FNV 初值，空链的审计值就是这个初值）。
    pub fn new() -> SealChain {
        SealChain {
            slots: [Link::EMPTY; SLOTS],
            len: 0,
            state: ChainState::Empty,
            root: FNV_OFFSET,
            freeze_replays: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ChainState {
        self.state
    }

    /// 已入槽张数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空链。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 链根（当前最后一环的摘要）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重复冻结次数（幂等痕迹）。
    pub fn freeze_replays(&self) -> u16 {
        self.freeze_replays
    }

    /// 取第 `idx` 槽。
    pub fn slot(&self, idx: usize) -> Option<&Link> {
        if idx < self.len {
            self.slots.get(idx)
        } else {
            None
        }
    }

    /// 追加一张卡。卡号必须严格等于 `FIRST_CARD + len`。
    ///
    /// 为什么不允许跳号：跳号意味着「中间那张卡没入账」，而 20/20 的
    /// 结论要求编号连续；一旦允许跳号，`len == 20` 就不再等价于「20 张卡」。
    pub fn append(&mut self, card: u16, lines: u32) -> Result<u64, SealError> {
        if !self.state.accepts_cards() {
            return Err(SealError::State);
        }
        if self.len >= SLOTS {
            return Err(SealError::Full);
        }
        let expect = FIRST_CARD + self.len as u16;
        if card != expect {
            return Err(SealError::Card);
        }
        if lines == 0 || lines > MAX_CARD_LINES {
            return Err(SealError::Range);
        }
        let d = link_digest(self.root, card, lines);
        match self.slots.get_mut(self.len) {
            Some(s) => {
                *s = Link {
                    card,
                    lines,
                    frozen: false,
                    digest: d,
                };
            }
            None => return Err(SealError::Full),
        }
        self.len += 1;
        self.root = d;
        if self.state == ChainState::Empty {
            self.state = ChainState::Filling;
        }
        Ok(d)
    }

    /// 冻结一张卡。重复冻结幂等（只记一次痕迹，不涨计数）。
    pub fn freeze(&mut self, card: u16) -> Result<(), SealError> {
        if self.state == ChainState::Sealed {
            return Err(SealError::State);
        }
        let mut idx: Option<usize> = None;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.slots.get(i) {
                if s.card == card {
                    idx = Some(i);
                }
            }
            i += 1;
        }
        let at = match idx {
            Some(a) => a,
            None => return Err(SealError::Missing),
        };
        let already = match self.slots.get(at) {
            Some(s) => s.frozen,
            None => return Err(SealError::Missing),
        };
        if already {
            self.freeze_replays = self.freeze_replays.saturating_add(1);
            return Ok(());
        }
        if let Some(s) = self.slots.get_mut(at) {
            s.frozen = true;
        }
        Ok(())
    }

    /// 冻结张数。
    pub fn frozen_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.slots.get(i) {
                if s.frozen {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 合计（张数, 行数）。现算不缓存。
    pub fn totals(&self) -> (u16, u32) {
        let mut n = 0u16;
        let mut lines = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.slots.get(i) {
                n += 1;
                lines = lines.saturating_add(s.lines);
            }
            i += 1;
        }
        (n, lines)
    }

    /// 收口：Filling → Closed。要求 20 槽齐且全部冻结。
    pub fn close(&mut self) -> Result<(), SealError> {
        if self.state == ChainState::Closed || self.state == ChainState::Sealed {
            return Err(SealError::State);
        }
        if self.len != SLOTS {
            return Err(SealError::Count);
        }
        if self.frozen_count() != BATCH_CARDS {
            return Err(SealError::NotFrozen);
        }
        self.state = ChainState::Closed;
        Ok(())
    }

    /// 盖印：Closed → Sealed，产出链根凭据。
    ///
    /// 检查次序：状态 → 计数 → 冻结 → 下界 → 上界。下界先于上界，是因为
    /// 「不够」比「超了」更常见，也更容易修（补卡），先报它更省一轮。
    pub fn seal(&mut self) -> Result<BatchSeal, SealError> {
        if self.state != ChainState::Closed {
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
        self.state = ChainState::Sealed;
        Ok(BatchSeal {
            batch: BATCH_NO,
            count,
            lines,
            cap: DOMAIN_CAP,
            root: self.root,
            ratio_bp: ratio_bp(lines, DOMAIN_CAP),
            frozen: true,
            seq: 0,
        })
    }

    /// 从第 0 槽重算链，返回根；任何一环摘要或最终根对不上即 `E_CHAIN`。
    pub fn audit(&self) -> Result<u64, SealError> {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.slots.get(i) {
                Some(v) => *v,
                None => return Err(SealError::Chain),
            };
            h = link_digest(h, s.card, s.lines);
            if h != s.digest {
                return Err(SealError::Chain);
            }
            i += 1;
        }
        if h != self.root {
            return Err(SealError::Chain);
        }
        Ok(h)
    }

    /// 前 `n` 环的链根（n ≤ len）。前缀审计不依赖后补的卡。
    pub fn prefix_root(&self, n: usize) -> Result<u64, SealError> {
        if n > self.len {
            return Err(SealError::Range);
        }
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < n {
            let s = match self.slots.get(i) {
                Some(v) => *v,
                None => return Err(SealError::Chain),
            };
            h = link_digest(h, s.card, s.lines);
            i += 1;
        }
        Ok(h)
    }

    /// 审计注入口：只改行数、不重算链 digest（模拟「手改明细」）。
    pub fn forge_lines(&mut self, idx: usize, lines: u32) -> Result<(), SealError> {
        if idx >= self.len {
            return Err(SealError::Range);
        }
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.lines = lines;
                Ok(())
            }
            None => Err(SealError::Range),
        }
    }

    /// 一行渲染（诊断用）。
    pub fn render(&self) -> String {
        let (n, lines) = self.totals();
        format!(
            "chain {} {}/{} lines={} frozen={} root={:016x}",
            self.state.label(),
            n,
            BATCH_CARDS,
            lines,
            self.frozen_count(),
            self.root
        )
    }
}

/// 万分比（u64 中间量防 240,000 × 10,000 在 u32 上溢出）。
pub fn ratio_bp(lines: u32, cap: u32) -> u32 {
    if cap == 0 {
        return 0;
    }
    ((lines as u64 * 10_000) / cap as u64) as u32
}

/// 印凭据。`root` 由链复算得出，改任何明细而不重算链都会在 `audit` 露馅。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchSeal {
    pub batch: u16,
    pub count: u16,
    pub lines: u32,
    pub cap: u32,
    pub root: u64,
    pub ratio_bp: u32,
    pub frozen: bool,
    pub seq: u32,
}

impl BatchSeal {
    /// 20/20 冻结且行数达标。
    pub fn complete(&self) -> bool {
        self.count == BATCH_CARDS && self.frozen && self.lines >= BATCH_LINES
    }

    /// 换册序（印面内容不变，链根不变）。
    pub fn renumber(self, seq: u32) -> BatchSeal {
        BatchSeal { seq, ..self }
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        format!(
            "seal b{} {}/{} lines={} bp={} root={:016x} seq={}",
            self.batch, self.count, BATCH_CARDS, self.lines, self.ratio_bp, self.root, self.seq
        )
    }
}

/// 印册：册序稠密、批号单调、容量定死。
pub struct SealArchive {
    seals: Vec<BatchSeal>,
    cap: usize,
    last_batch: u16,
}

impl SealArchive {
    /// 建册。容量 1..=8。
    pub fn new(cap: usize) -> Result<SealArchive, SealError> {
        if cap == 0 || cap > 8 {
            return Err(SealError::Cap);
        }
        Ok(SealArchive {
            seals: Vec::with_capacity(cap),
            cap,
            last_batch: 0,
        })
    }

    /// 已入册份数。
    pub fn len(&self) -> usize {
        self.seals.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.seals.is_empty()
    }

    /// 累计行数（多批相加；跨批只做口径展示，不参与域容量判定）。
    pub fn cumulative_lines(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(s) = self.seals.get(i) {
                sum = sum.saturating_add(s.lines);
            }
            i += 1;
        }
        sum
    }

    /// 印册摘要。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET ^ (self.seals.len() as u64);
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(s) = self.seals.get(i) {
                h ^= s.root.rotate_left(9);
                h = h.wrapping_mul(FNV_PRIME);
                h = fnv_u16(h, s.batch);
            }
            i += 1;
        }
        h
    }

    /// 入册。次序：内容门槛 → 链根非零 → 册序 → 批号 → 容量。
    pub fn register(&mut self, s: &BatchSeal) -> Result<u32, SealError> {
        if s.count != BATCH_CARDS {
            return Err(SealError::Count);
        }
        if !s.frozen {
            return Err(SealError::NotFrozen);
        }
        if s.lines == 0 || s.lines > DOMAIN_CAP {
            return Err(SealError::Over);
        }
        if s.root == 0 {
            return Err(SealError::Chain);
        }
        let expected = self.seals.len() as u32;
        if s.seq != expected {
            return Err(SealError::Seq);
        }
        if !self.seals.is_empty() && s.batch <= self.last_batch {
            return Err(SealError::Seq);
        }
        if self.seals.len() >= self.cap {
            return Err(SealError::Cap);
        }
        self.seals.push(*s);
        self.last_batch = s.batch;
        Ok(expected)
    }
}

fn fill_uniform(chain: &mut SealChain, each: u32) -> bool {
    let mut i = 0u16;
    while i < BATCH_CARDS {
        if chain.append(FIRST_CARD + i, each).is_err() {
            return false;
        }
        i += 1;
    }
    true
}

fn freeze_all(chain: &mut SealChain) -> bool {
    let mut i = 0u16;
    let mut ok = true;
    while i < BATCH_CARDS {
        if chain.freeze(FIRST_CARD + i).is_err() {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 构造一本合规链（20 × 8,100 = 162,000），已冻结但未收口。
pub fn planned_chain() -> Result<SealChain, SealError> {
    let mut c = SealChain::new();
    if !fill_uniform(&mut c, QUOTA_PER_CARD) {
        return Err(SealError::Count);
    }
    if !freeze_all(&mut c) {
        return Err(SealError::Missing);
    }
    Ok(c)
}

/// 直接产出一枚合规印（判据与注入核共用）。
pub fn planned_seal() -> Result<BatchSeal, SealError> {
    let mut c = planned_chain()?;
    c.close()?;
    c.seal()
}

fn custom_chain(lines_at: &[(usize, u32)], default_lines: u32) -> Result<SealChain, SealError> {
    let mut c = SealChain::new();
    let mut i = 0usize;
    while i < SLOTS {
        let mut lines = default_lines;
        let mut k = 0usize;
        while k < lines_at.len() {
            if let Some((at, v)) = lines_at.get(k) {
                if *at == i {
                    lines = *v;
                }
            }
            k += 1;
        }
        c.append(FIRST_CARD + i as u16, lines)?;
        i += 1;
    }
    if !freeze_all(&mut c) {
        return Err(SealError::Missing);
    }
    Ok(c)
}

/// 注入核：九类坏印，逐类断言专属码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1 跳号入槽
    let mut c1 = SealChain::new();
    if c1.append(FIRST_CARD + 1, QUOTA_PER_CARD).err() == Some(SealError::Card) {
        caught += 1;
    }

    // 2 单卡 0 行
    let mut c2 = SealChain::new();
    if c2.append(FIRST_CARD, 0).err() == Some(SealError::Range) {
        caught += 1;
    }

    // 3 19 张就收口
    let mut c3 = SealChain::new();
    let mut i = 0u16;
    while i < BATCH_CARDS - 1 {
        let _ = c3.append(FIRST_CARD + i, QUOTA_PER_CARD);
        i += 1;
    }
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = c3.freeze(FIRST_CARD + f);
        f += 1;
    }
    if c3.close().err() == Some(SealError::Count) {
        caught += 1;
    }

    // 4 少冻一张
    let mut c4 = SealChain::new();
    let _ = fill_uniform(&mut c4, QUOTA_PER_CARD);
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = c4.freeze(FIRST_CARD + f);
        f += 1;
    }
    if c4.close().err() == Some(SealError::NotFrozen) {
        caught += 1;
    }

    // 5 行数不足盖印（20 × 8,000 = 160,000）
    let mut c5 = SealChain::new();
    let _ = fill_uniform(&mut c5, 8_000);
    let _ = freeze_all(&mut c5);
    let ok = c5.close().is_ok();
    if ok && c5.seal().err() == Some(SealError::Short) {
        caught += 1;
    }

    // 6 超域盖印（20 × 13,000 = 260,000）
    let mut c6 = SealChain::new();
    let _ = fill_uniform(&mut c6, 13_000);
    let _ = freeze_all(&mut c6);
    let ok = c6.close().is_ok();
    if ok && c6.seal().err() == Some(SealError::Over) {
        caught += 1;
    }

    // 7 未收口盖印
    let mut c7 = match planned_chain() {
        Ok(c) => c,
        Err(_) => return (caught, total),
    };
    if c7.seal().err() == Some(SealError::State) {
        caught += 1;
    }

    // 8 明细被改后审计
    let mut c8 = match planned_chain() {
        Ok(c) => c,
        Err(_) => return (caught, total),
    };
    if c8.close().is_ok() {
        if c8.forge_lines(5, 9_000).is_ok() {
            if c8.audit().err() == Some(SealError::Chain) {
                caught += 1;
            }
        }
    }

    // 9 未知卡号冻结
    let mut c9 = SealChain::new();
    let _ = c9.append(FIRST_CARD, QUOTA_PER_CARD);
    if c9.freeze(50999).err() == Some(SealError::Missing) {
        caught += 1;
    }

    (caught, total)
}

/// 判据集（17 条）。
pub fn run_unx50140_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50140");

    // J1-1 常量整分：162,000 / 20 = 8,100，无余数
    let ok = QUOTA_PER_CARD == 8_100
        && QUOTA_PER_CARD * BATCH_CARDS as u32 == BATCH_LINES
        && BATCH_LINES % BATCH_CARDS as u32 == 0
        && MAX_CARD_LINES == 16_200
        && SLOTS == BATCH_CARDS as usize
        && FIRST_CARD + BATCH_CARDS - 1 == 50140;
    let det = format!(
        "quota={} total={} cap={}",
        QUOTA_PER_CARD,
        QUOTA_PER_CARD * BATCH_CARDS as u32,
        DOMAIN_CAP
    );
    s.add_dyn("50140-J1-配额整分", ok, &det);

    // J1-2 比值：162,000 / 240,000 = 6,750 bp，端点不漂
    let ok = ratio_bp(BATCH_LINES, DOMAIN_CAP) == 6750
        && ratio_bp(0, DOMAIN_CAP) == 0
        && ratio_bp(DOMAIN_CAP, DOMAIN_CAP) == 10_000
        && ratio_bp(123, 0) == 0;
    let det = format!("bp={}", ratio_bp(BATCH_LINES, DOMAIN_CAP));
    s.add_dyn("50140-J1-目标万分比", ok, &det);

    // J1-3 状态机单向：Empty → Filling → Closed → Sealed
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut c) = planned_chain() {
        let before = c.state();
        let closed = c.close().is_ok();
        let after = c.state();
        let sealed = c.seal().is_ok();
        ok = SealChain::new().state() == ChainState::Empty
            && before == ChainState::Filling
            && closed
            && after == ChainState::Closed
            && sealed
            && c.state() == ChainState::Sealed;
        det = format!("{}->{}->{}", before.label(), after.label(), c.state().label());
    }
    s.add_dyn("50140-J1-状态单向", ok, &det);

    // J1-4 链守恒：审计复算 == 根，前缀复算 == 根
    let mut ok = false;
    let mut det = String::new();
    if let Ok(c) = planned_chain() {
        let root = c.root();
        let audit = c.audit();
        let prefix = c.prefix_root(SLOTS);
        ok = audit == Ok(root) && prefix == Ok(root) && root != 0 && c.len() == SLOTS;
        det = format!("root={:016x}", root);
    }
    s.add_dyn("50140-J1-链守恒", ok, &det);

    // J1-5 计划账盖印：20/20 冻结、162,000 行、6,750 bp
    let mut det = String::new();
    let ok = match planned_seal() {
        Ok(seal) => {
            det = seal.render();
            seal.count == BATCH_CARDS
                && seal.lines == BATCH_LINES
                && seal.cap == DOMAIN_CAP
                && seal.frozen
                && seal.complete()
                && seal.ratio_bp == 6750
                && seal.batch == BATCH_NO
        }
        Err(_) => false,
    };
    s.add_dyn("50140-J1-印面守恒", ok, &det);

    // J1-6 摘要确定：两本同构链盖印逐位一致
    let a = planned_seal();
    let b = planned_seal();
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x == y && x.root == y.root,
        _ => false,
    };
    s.add("50140-J1-印面确定", ok, if ok { "" } else { "同构链印面漂移" });

    // J1-7 前缀稳定：前 7 环与另建的同首 7 环一致，且不等于全链根
    let mut ok = false;
    if let Ok(full) = custom_chain(&[], QUOTA_PER_CARD) {
        if let Ok(part) = custom_chain(&[], QUOTA_PER_CARD) {
            let p7 = match full.prefix_root(7) {
                Ok(v) => v,
                Err(_) => 0,
            };
            let q7 = match part.prefix_root(7) {
                Ok(v) => v,
                Err(_) => 0,
            };
            ok = p7 == q7 && p7 != full.root() && p7 != 0;
        }
    }
    s.add("50140-J1-前缀稳定", ok, if ok { "" } else { "前缀依赖后补卡" });

    // J1-8 顺序绑定：对调两卡行数，根必变且合计不变
    let mut ok = false;
    let mut det = String::new();
    let a8 = custom_chain(&[(1, 8_101), (2, 8_099)], QUOTA_PER_CARD);
    let b8 = custom_chain(&[(1, 8_099), (2, 8_101)], QUOTA_PER_CARD);
    if let (Ok(x), Ok(y)) = (a8, b8) {
        ok = x.root() != y.root() && x.totals() == y.totals() && x.totals().1 == BATCH_LINES;
        det = format!("{:016x} vs {:016x}", x.root(), y.root());
    }
    s.add_dyn("50140-J1-顺序绑定", ok, &det);

    // J1-9 计数不足：19 张就收口必拒
    let mut c9 = SealChain::new();
    let mut i = 0u16;
    while i < BATCH_CARDS - 1 {
        let _ = c9.append(FIRST_CARD + i, QUOTA_PER_CARD);
        i += 1;
    }
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = c9.freeze(FIRST_CARD + f);
        f += 1;
    }
    let ok = c9.close().err() == Some(SealError::Count) && c9.len() == 19;
    let det = format!("n={}", c9.len());
    s.add_dyn("50140-J1-计数不足", ok, &det);

    // J1-10 冻结不足：20 张齐、只冻 19
    let mut c10 = SealChain::new();
    let _ = fill_uniform(&mut c10, QUOTA_PER_CARD);
    let mut f = 0u16;
    while f < BATCH_CARDS - 1 {
        let _ = c10.freeze(FIRST_CARD + f);
        f += 1;
    }
    let ok = c10.close().err() == Some(SealError::NotFrozen) && c10.frozen_count() == 19;
    s.add("50140-J1-冻结不足", ok, if ok { "" } else { "少冻一张被放行" });

    // J1-11 行数不足：20 × 8,000 = 160,000
    let mut c11 = SealChain::new();
    let _ = fill_uniform(&mut c11, 8_000);
    let _ = freeze_all(&mut c11);
    let ok = c11.close().is_ok()
        && c11.totals() == (BATCH_CARDS, 160_000)
        && c11.seal().err() == Some(SealError::Short);
    s.add("50140-J1-行数不足", ok, if ok { "" } else { "160,000 被当成达标" });

    // J1-12 串域超容：20 × 13,000 = 260,000（单卡仍在界内）
    let mut c12 = SealChain::new();
    let _ = fill_uniform(&mut c12, 13_000);
    let _ = freeze_all(&mut c12);
    let ok = c12.close().is_ok()
        && c12.totals().1 == 260_000
        && c12.seal().err() == Some(SealError::Over);
    let det = format!("lines={} cap={}", c12.totals().1, DOMAIN_CAP);
    s.add_dyn("50140-J1-串域超容", ok, &det);

    // J1-13 卡号/行数/容量边界
    let mut c13 = SealChain::new();
    let ok = c13.append(50999, QUOTA_PER_CARD).err() == Some(SealError::Card)
        && c13.append(FIRST_CARD, 0).err() == Some(SealError::Range)
        && c13.append(FIRST_CARD, MAX_CARD_LINES + 1).err() == Some(SealError::Range)
        && c13.append(FIRST_CARD, MAX_CARD_LINES).is_ok()
        && c13.append(FIRST_CARD + 1, 1).is_ok()
        && c13.freeze(50999).err() == Some(SealError::Missing);
    s.add("50140-J1-单槽边界", ok, if ok { "" } else { "单槽边界未守" });

    // J1-14 容量满：20 槽之后第 21 张 E_FULL
    let mut c14 = SealChain::new();
    let _ = fill_uniform(&mut c14, QUOTA_PER_CARD);
    let ok = c14.len() == SLOTS
        && c14.append(FIRST_CARD + BATCH_CARDS, 1).err() == Some(SealError::Full);
    s.add("50140-J1-槽满", ok, if ok { "" } else { "第 21 张被收下" });

    // J1-15 冻结幂等
    let mut c15 = SealChain::new();
    let _ = c15.append(FIRST_CARD, QUOTA_PER_CARD);
    let _ = c15.freeze(FIRST_CARD);
    let once = c15.frozen_count();
    let _ = c15.freeze(FIRST_CARD);
    let ok = once == 1 && c15.frozen_count() == 1 && c15.freeze_replays() == 1;
    s.add("50140-J1-冻结幂等", ok, if ok { "" } else { "重复冻结涨计数" });

    // J1-16 审计抓篡改：手改第 5 槽行数 → 链失配
    let mut ok = false;
    if let Ok(mut c) = planned_chain() {
        if c.close().is_ok() && c.seal().is_ok() {
            let before = c.audit();
            let forged = c.forge_lines(5, 9_000);
            let after = c.audit();
            ok = before == Ok(c.root()) && forged.is_ok() && after.err() == Some(SealError::Chain);
        }
    }
    s.add("50140-J1-审计抓篡改", ok, if ok { "" } else { "手改明细未被发现" });

    // J1-17 印册：册序稠密、批号单调、容量/根校验
    let mut ok = false;
    let mut det = String::new();
    if let Ok(seal) = planned_seal() {
        if let Ok(mut arch) = SealArchive::new(4) {
            let first = arch.register(&seal);
            let dup_seq = arch.register(&seal.renumber(0));
            let back_batch = arch.register(&BatchSeal { batch: 26, ..seal.renumber(1) });
            let second = arch.register(&BatchSeal { batch: 28, ..seal.renumber(1) });
            let zero_root = SealArchive::new(1)
                .and_then(|mut a| a.register(&BatchSeal { root: 0, ..seal }));
            let capped = SealArchive::new(1).and_then(|mut a| {
                let _ = a.register(&seal);
                a.register(&BatchSeal { batch: 28, ..seal.renumber(1) })
            });
            ok = first == Ok(0)
                && dup_seq.err() == Some(SealError::Seq)
                && back_batch.err() == Some(SealError::Seq)
                && second == Ok(1)
                && arch.len() == 2
                && arch.cumulative_lines() == 324_000
                && zero_root.err() == Some(SealError::Chain)
                && capped.err() == Some(SealError::Cap)
                && SealArchive::new(0).err() == Some(SealError::Cap)
                && SealArchive::new(9).err() == Some(SealError::Cap);
            det = format!("n={} cum={}", arch.len(), arch.cumulative_lines());
        }
    }
    s.add_dyn("50140-J1-印册纪律", ok, &det);

    // J1-18 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50140-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50140_checks_green() {
        let set = run_unx50140_checks();
        assert!(set.all_passed(), "UNX-F50140 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50140_planned_seal_is_6750bp() {
        let seal = planned_seal();
        assert!(seal.is_ok());
        if let Ok(x) = seal {
            assert_eq!(x.count, 20);
            assert_eq!(x.lines, 162_000);
            assert_eq!(x.ratio_bp, 6750);
            assert!(x.complete());
        }
    }

    #[test]
    fn unx50140_chain_audit_reproduces_root() {
        let c = planned_chain();
        assert!(c.is_ok());
        if let Ok(x) = c {
            let root = x.root();
            assert_eq!(x.audit(), Ok(root));
            let p7 = x.prefix_root(7);
            assert!(p7.is_ok());
            match p7 {
                Ok(v) => assert_ne!(v, root),
                Err(_) => return,
            }
        }
    }
}
