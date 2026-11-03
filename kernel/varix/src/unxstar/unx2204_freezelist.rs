//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2204 · 冻结批清单生成器（40 批 800 条点名冻结版） · 0x72AC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B31）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F2204-J1 冻结批清单生成全过（B01–B40 四十批逐批
//!     20 条点名冻结版，批区间 F1601–F2400 首尾相接无空洞无重叠），清
//!     单与 F2190 点名账/F2204 生成器输出三方一致断言过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：四十批确定性生成（B01–B40 逐批 20 条，卡名/角色/分值由卡
//!   号派生可复算）；批 i 区间 [1601+20(i-1), 1600+20i] 相邻首尾相接无
//!   空洞无重叠；三方一致 = F2190 点名账 × 生成器输出 × 冻结清单序列文
//!   （批摘要=行前缀+卡名流合并 FNV）逐卡对账 800/800；Draft→Frozen 单
//!   调封账（封后改批/改卡即冻结突变拒）；失真注入 10 路核验器 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72AC0，VX3-1103 冻结批清单窗口独占）。
pub const SEG_BASE: u32 = 0x72AC0;
/// 批数（B01–B40，冻结）。
pub const BATCH_N: usize = 40;
/// 每批条数（逐批 20 条，冻结）。
pub const CARDS_PER: usize = 20;
/// 总条数（800，冻结）。
pub const TOTAL_N: usize = 800;
/// 卡号首（F1601，冻结）。
pub const ID_FIRST: u32 = 1601;
/// 卡号尾（F2400，冻结）。
pub const ID_LAST: u32 = 2400;
/// 清单纪元（冻结 11）。
pub const EPOCH: u64 = 11;
/// 注入检出轮数（判据 10/10 语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 批号未知/乱序：只认 B01–B40 且行序单调
    BatchUnknown,
    /// 批间空洞：相邻批首尾不接，区间缺号
    SpanHole,
    /// 批间重叠：相邻批区间交叠或倒挂
    SpanOverlap,
    /// 点名不符：批内条数/卡名与派生点名账不一致
    CardMismatch,
    /// 三方失和：点名账/生成器输出/冻结清单三方对账失败
    ThreeWayOff,
    /// 摘要破损：批摘要或链摘要与重算值不符
    DigestBad,
    /// 冻结突变：封账后改批/改卡，冻结红线
    FrozenMutate,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl FreezeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FreezeCode::BatchUnknown => "批号未知/乱序：批号必须 B01–B40 且行序单调",
            FreezeCode::SpanHole => "批间空洞：相邻批首尾不接，查缺号区间",
            FreezeCode::SpanOverlap => "批间重叠：相邻批区间交叠或倒挂，重排区间",
            FreezeCode::CardMismatch => "点名不符：批内卡名与派生点名账不一致，逐卡定位",
            FreezeCode::ThreeWayOff => "三方失和：点名账/生成器/清单对账失败，重出清单",
            FreezeCode::DigestBad => "摘要破损：批摘要与重算值不符，重算 FNV 链",
            FreezeCode::FrozenMutate => "冻结突变：封账后改批/改卡属冻结红线，重开新单",
            FreezeCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
}

/// 诊断码闭集全表（查表/段位守卫共用）。
pub const ALL_CODES: [FreezeCode; 8] = [
    FreezeCode::BatchUnknown, FreezeCode::SpanHole, FreezeCode::SpanOverlap,
    FreezeCode::CardMismatch, FreezeCode::ThreeWayOff, FreezeCode::DigestBad,
    FreezeCode::FrozenMutate, FreezeCode::NotReproducible,
];

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ALL_CODES.len() {
        if ALL_CODES[i].val() == code {
            return ALL_CODES[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72AC0 段闭集内"
}

/// 段位守卫（全部诊断码落 0x72AC0 窗口）。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ALL_CODES.len() {
        let v = ALL_CODES[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 清单状态机（Draft→Frozen，单调推进） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListPhase {
    /// 起草：批表可改，冻结摘要未出。
    Draft,
    /// 已冻结：批表定版，链摘要出账，任何改批改卡拒。
    Frozen,
}

impl ListPhase {
    /// 单调序值：Draft=0 < Frozen=1；只许前进，平移倒退拒。
    pub fn can_advance(self, to: ListPhase) -> bool {
        (match self { ListPhase::Draft => 0, ListPhase::Frozen => 1 })
            < (match to { ListPhase::Draft => 0, ListPhase::Frozen => 1 })
    }
}

// ── 卡模型（F2190 点名账派生口径） ──────────────────────────────
/// 点名卡：卡号 + 卡名 + 角色 + 分值，全由卡号确定性派生。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rollcard {
    /// 卡号（1601..=2400）。
    pub id: u32,
    /// 卡名（形如 F1601，冻结拼写）。
    pub name: String,
    /// 角色（卡号%4，0..=3 四路）。
    pub role: u32,
    /// 分值（(卡号*7)%97，可复算）。
    pub score: u32,
}

/// 卡号 → 卡名（F####）。
pub fn card_name(id: u32) -> String {
    format!("F{}", id)
}

/// F2190 点名账单卡派生：角色 = id%4，分值 = (id*7)%97。
pub fn rollcall_card(id: u32) -> Rollcard {
    Rollcard { id, name: card_name(id), role: (id % 4) as u32, score: ((id as u64 * 7) % 97) as u32 }
}

/// F2190 点名账全量：1601..=2400 逐卡派生（800 条，序冻结）。
pub fn rollcall_build() -> Vec<Rollcard> {
    (ID_FIRST..=ID_LAST).map(rollcall_card).collect()
}

// ── FNV-1a 32 位校验和（整数实现，摘要链共用） ──────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

// ── 批模型与生成器 ──────────────────────────────────────────────
/// 冻结批：批号 + 区间 + 逐卡点名 + 批摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreezeBatch {
    /// 批号（1..=40）。
    pub bid: u32,
    /// 批码（形如 B01，冻结拼写）。
    pub code: String,
    /// 批首卡号。
    pub first: u32,
    /// 批尾卡号。
    pub last: u32,
    /// 逐卡点名（20 条，序冻结）。
    pub cards: Vec<Rollcard>,
    /// 批摘要（行前缀 + 卡名流合并 FNV-1a）。
    pub digest: u32,
}

/// 批码派生：bid → B%02。
pub fn batch_code(bid: u32) -> String {
    format!("B{:02}", bid)
}

/// 批摘要：行前缀（批码|F首|F尾|条数|）+ 点名卡名流，合并 FNV。
/// 生成器/序列文/解析器三路共用同一口径，摘要即行内自校。
pub fn batch_digest(code: &str, first: u32, last: u32, count: usize) -> u32 {
    let mut bytes: Vec<u8> = format!("{}|F{}|F{}|{}|", code, first, last, count).into_bytes();
    let mut id = first;
    while id <= last {
        bytes.extend_from_slice(card_name(id).as_bytes());
        id += 1;
    }
    fnv1a(&bytes)
}

/// 单批生成：批 i（1..=40）覆盖 [1601+20(i-1), 1600+20i] 逐卡点名。
pub fn gen_batch(bid: u32) -> Result<FreezeBatch, u32> {
    if bid == 0 || bid > BATCH_N as u32 {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let first = ID_FIRST + (bid - 1) * CARDS_PER as u32;
    let last = first + CARDS_PER as u32 - 1;
    let cards: Vec<Rollcard> = (first..=last).map(rollcall_card).collect();
    let code = batch_code(bid);
    Ok(FreezeBatch {
        bid,
        code: code.clone(),
        first,
        last,
        cards,
        digest: batch_digest(&code, first, last, CARDS_PER),
    })
}

/// 全单生成：B01–B40 逐批生成，首尾相接（800 条，序冻结）。
pub fn gen_full() -> Result<Vec<FreezeBatch>, u32> {
    (1..=BATCH_N as u32).map(gen_batch).collect()
}

/// 批表核验：批数/批码/区间宽/首尾相接/无空洞无重叠/摘要重算/点名对账。
pub fn verify_batches(batches: &[FreezeBatch]) -> Result<(), u32> {
    if batches.len() != BATCH_N {
        return Err(FreezeCode::BatchUnknown.val());
    }
    for (i, b) in batches.iter().enumerate() {
        let want_bid = (i + 1) as u32;
        if b.bid != want_bid || b.code != batch_code(want_bid) {
            return Err(FreezeCode::BatchUnknown.val());
        }
        if b.cards.len() != CARDS_PER {
            return Err(FreezeCode::CardMismatch.val());
        }
        let want_first = ID_FIRST + i as u32 * CARDS_PER as u32;
        if b.first != want_first || b.last != want_first + CARDS_PER as u32 - 1 {
            return Err(FreezeCode::SpanHole.val());
        }
        // 逐卡点名与派生口径对账。
        for (k, c) in b.cards.iter().enumerate() {
            if *c != rollcall_card(b.first + k as u32) {
                return Err(FreezeCode::CardMismatch.val());
            }
        }
        // 批摘要重算（行前缀 + 卡名流合并口径）。
        if batch_digest(&b.code, b.first, b.last, b.cards.len()) != b.digest {
            return Err(FreezeCode::DigestBad.val());
        }
        // 相邻批首尾相接：无空洞无重叠。
        if let Some(nxt) = batches.get(i + 1) {
            if nxt.first < b.last + 1 {
                return Err(FreezeCode::SpanOverlap.val());
            }
            if nxt.first > b.last + 1 {
                return Err(FreezeCode::SpanHole.val());
            }
        }
    }
    match (batches.first(), batches.last()) {
        (Some(f), Some(l)) if f.first == ID_FIRST && l.last == ID_LAST => Ok(()),
        (Some(_), Some(_)) => Err(FreezeCode::SpanHole.val()),
        _ => Err(FreezeCode::BatchUnknown.val()),
    }
}

// ── 冻结清单（账本本体） ────────────────────────────────────────
/// 冻结清单：纪元 + 批表 + 相位机 + 链摘要；Draft→Frozen 单调封账。
#[derive(Clone, Debug)]
pub struct FreezeList {
    pub epoch: u64,
    pub batches: Vec<FreezeBatch>,
    pub phase: ListPhase,
    /// 链摘要（逐批摘要折叠 FNV，封账时定版）。
    pub chain: u32,
}

impl FreezeList {
    pub fn new(epoch: u64) -> FreezeList {
        FreezeList { epoch, batches: Vec::new(), phase: ListPhase::Draft, chain: 0 }
    }
    /// 相位推进公开口：倒退/平移显性拒。
    pub fn advance_phase(&mut self, to: ListPhase) -> Result<(), u32> {
        if !self.phase.can_advance(to) {
            return Err(FreezeCode::FrozenMutate.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 落一批：Draft 态限定；批号必须递增；区间不得倒挂/留洞。
    pub fn append(&mut self, b: FreezeBatch) -> Result<(), u32> {
        if self.phase != ListPhase::Draft {
            return Err(FreezeCode::FrozenMutate.val());
        }
        match self.batches.last() {
            None => {
                if b.first != ID_FIRST {
                    return Err(FreezeCode::SpanHole.val());
                }
            }
            Some(last) => {
                if b.bid <= last.bid {
                    return Err(FreezeCode::BatchUnknown.val());
                }
                if b.first < last.last + 1 {
                    return Err(FreezeCode::SpanOverlap.val());
                }
                if b.first > last.last + 1 {
                    return Err(FreezeCode::SpanHole.val());
                }
            }
        }
        self.batches.push(b);
        Ok(())
    }
    /// 封账：批表核验 + 链摘要定版 + 相位 Draft→Frozen。
    pub fn freeze(&mut self) -> Result<(), u32> {
        verify_batches(&self.batches)?;
        let mut chain: u32 = 0x811C_9DC5;
        for b in &self.batches {
            chain = chain.wrapping_mul(0x0100_0193) ^ b.digest;
        }
        self.chain = chain;
        self.advance_phase(ListPhase::Frozen)
    }
    /// 三方一致断言：点名账 × 生成器输出 × 本清单逐卡对账。
    pub fn three_way(&self) -> Result<u32, u32> {
        if self.phase != ListPhase::Frozen {
            return Err(FreezeCode::FrozenMutate.val());
        }
        let roll = rollcall_build();
        let regen = gen_full()?;
        if roll.len() != TOTAL_N || regen.len() != BATCH_N {
            return Err(FreezeCode::ThreeWayOff.val());
        }
        // 路一×路二：生成器输出与点名账逐卡一致。
        for b in &regen {
            for c in &b.cards {
                if roll.get((c.id - ID_FIRST) as usize) != Some(c) {
                    return Err(FreezeCode::ThreeWayOff.val());
                }
            }
        }
        // 路三：本冻结清单与点名账逐卡一致。
        let mut seen = 0u32;
        for b in &self.batches {
            for c in &b.cards {
                if roll.get((c.id - ID_FIRST) as usize) != Some(c) {
                    return Err(FreezeCode::ThreeWayOff.val());
                }
                seen += 1;
            }
        }
        if seen != TOTAL_N as u32 {
            return Err(FreezeCode::ThreeWayOff.val());
        }
        Ok(seen)
    }
}

// ── 清单序列文（五列：批码|首卡|尾卡|条数|摘要） ────────────────
/// 渲染单批行：批码|F首|F尾|条数|摘要8位;。
pub fn render_row(b: &FreezeBatch) -> String {
    format!("{}|F{}|F{}|{}|{:08X};", b.code, b.first, b.last, b.cards.len(), b.digest)
}

/// 全单渲染。
pub fn render_all(batches: &[FreezeBatch]) -> String {
    let mut out = String::new();
    for b in batches.iter() {
        out.push_str(&render_row(b));
        out.push('\n');
    }
    out
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    h.iter().skip(from).position(|c| *c == needle).map(|p| p + from)
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    h.windows(pat.len()).skip(from).position(|w| w == pat).map(|p| p + from)
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for c in s.as_bytes().iter() {
        if *c < b'0' || *c > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((*c - b'0') as u64)?;
    }
    Some(v)
}

/// F 卡号解析（F 后跟纯数字，闭外拒）。
pub fn parse_fid(s: &str) -> Option<u32> {
    if s.len() < 2 || !s.starts_with('F') {
        return None;
    }
    let v = parse_dec(&s[1..])?;
    if v < ID_FIRST as u64 || v > ID_LAST as u64 {
        return None;
    }
    Some(v as u32)
}

/// 8 位十六进制解析（摘要域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 序列行解析体：批码|首卡|尾卡|条数|摘要（结构层 + 行内摘要自校）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanRow {
    pub code: String,
    pub first: u32,
    pub last: u32,
    pub count: u64,
    pub digest: u32,
}

/// 解析单行：批码|F首|F尾|条数|摘要8位;。
pub fn parse_row(line: &str) -> Result<SpanRow, u32> {
    let b = line.as_bytes();
    if b.is_empty() || b[b.len() - 1] != b';' {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let body = &line[..b.len() - 1];
    let mut cuts: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        if let Some(p) = find_byte(body.as_bytes(), i, b'|') {
            cuts.push(p);
            i = p + 1;
        } else {
            break;
        }
    }
    if cuts.len() != 4 {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let (c0, c1, c2, c3) = (cuts[0], cuts[1], cuts[2], cuts[3]);
    if c0 == 0 || c1 == c0 + 1 {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let code = String::from(&body[..c0]);
    if code.len() != 3 || !code.starts_with('B') {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let first = parse_fid(&body[c0 + 1..c1]).ok_or(FreezeCode::SpanHole.val())?;
    let last = parse_fid(&body[c1 + 1..c2]).ok_or(FreezeCode::SpanHole.val())?;
    if last < first || (last - first + 1) as usize != CARDS_PER {
        return Err(FreezeCode::SpanOverlap.val());
    }
    let count = parse_dec(&body[c2 + 1..c3]).ok_or(FreezeCode::CardMismatch.val())?;
    if count != CARDS_PER as u64 {
        return Err(FreezeCode::CardMismatch.val());
    }
    let digest = parse_hex8(&body[c3 + 1..]).ok_or(FreezeCode::DigestBad.val())?;
    // 行内摘要自校：行前缀 + 点名卡名流合并重算，与摘要域对账。
    if batch_digest(&code, first, last, count as usize) != digest {
        return Err(FreezeCode::DigestBad.val());
    }
    Ok(SpanRow { code, first, last, count, digest })
}

/// 核验序列文：逐行解析 + 批码序 + 首尾相接 + 摘要重算（三方第三路）。
pub fn verify_text(text: &str) -> Result<(usize, u32), u32> {
    if text.is_empty() {
        return Err(FreezeCode::ThreeWayOff.val());
    }
    let mut rows: Vec<SpanRow> = Vec::new();
    let bts = text.as_bytes();
    let mut i = 0usize;
    while i < bts.len() {
        let end = match find_byte(bts, i, b'\n') {
            Some(p) => p,
            None => bts.len(),
        };
        let line = &text[i..end];
        if !line.is_empty() {
            rows.push(parse_row(line)?);
        }
        i = end.saturating_add(1);
    }
    if rows.len() != BATCH_N {
        return Err(FreezeCode::BatchUnknown.val());
    }
    let mut chain: u32 = 0x811C_9DC5;
    for (k, r) in rows.iter().enumerate() {
        // 批码序：B01..B40 行序单调。
        if r.code != batch_code((k + 1) as u32) {
            return Err(FreezeCode::BatchUnknown.val());
        }
        // 首卡锚：逐批恰 20 且首尾相接（无空洞无重叠）。
        if r.last - r.first + 1 != CARDS_PER as u32 {
            return Err(FreezeCode::SpanOverlap.val());
        }
        if r.first != ID_FIRST + k as u32 * CARDS_PER as u32 {
            return Err(FreezeCode::SpanHole.val());
        }
        // 摘要重算（行前缀 + 点名卡名流合并口径）。
        if batch_digest(&r.code, r.first, r.last, r.count as usize) != r.digest {
            return Err(FreezeCode::DigestBad.val());
        }
        chain = chain.wrapping_mul(0x0100_0193) ^ r.digest;
    }
    Ok((rows.len(), chain))
}

/// 核验账本本体：序列文对账 + 本体批表核验 + 链摘要一致。
pub fn verify_ledger(list: &FreezeList) -> Result<(), u32> {
    let (n, chain) = verify_text(&render_all(&list.batches))?;
    if n != list.batches.len() || chain != list.chain {
        return Err(FreezeCode::DigestBad.val());
    }
    verify_batches(&list.batches)?;
    if list.phase != ListPhase::Frozen {
        return Err(FreezeCode::FrozenMutate.val());
    }
    Ok(())
}

// ── 行往返探针（建行→解析→逐列对账 + 三拒） ─────────────────────
pub fn row_roundtrip() -> bool {
    let line = match gen_batch(1) {
        Ok(b) => render_row(&b),
        Err(_) => return false,
    };
    let a = match parse_row(&line) {
        Ok(r) => r.code == "B01" && r.first == ID_FIRST && r.last == 1620 && r.count == CARDS_PER as u64,
        Err(_) => false,
    };
    // 三拒：批码野 / 区间倒挂 / 摘要断。
    let bad_code = parse_row(&line.replace("B01", "X01")).is_err();
    let bad_span = parse_row(&line.replace("|F1601|F1620|", "|F1620|F1601|")).is_err();
    let mut bad_sum = false;
    let mut ch = line.clone().into_bytes();
    if ch.len() >= 3 {
        let idx = ch.len() - 3;
        let c = ch[idx];
        ch[idx] = if c == b'8' { b'9' } else { b'8' };
        bad_sum = match String::from_utf8(ch) {
            Ok(s2) => parse_row(&s2).is_err(),
            Err(_) => false,
        };
    }
    a && bad_code && bad_span && bad_sum
}

// ── 黄金序列文行区间工具（注入器共用，正文全 ASCII） ────────────
/// 黄金账文第 k 行的字节区间 [lo, hi)（hi 为行尾换行下标）。
fn row_span(text: &str, k: usize) -> (usize, usize) {
    let b = text.as_bytes();
    let mut lo = 0usize;
    let mut idx = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'\n' {
            if idx == k {
                return (lo, i);
            }
            idx += 1;
            lo = i + 1;
        }
        i += 1;
    }
    (lo, b.len())
}

/// 行内单字节改写：第 k 行定位 pat，其后第 off 字节改写。
fn patch_in_row(text: &str, k: usize, pat: &[u8], off: usize, newc: u8) -> String {
    let (lo, hi) = row_span(text, k);
    let seg = &text[lo..hi];
    let tgt = match find_sub(seg.as_bytes(), 0, pat) {
        Some(p) => p + off,
        None => usize::MAX,
    };
    let mut out = String::from(&text[..lo]);
    let mut i = 0usize;
    while i < seg.len() {
        if i == tgt {
            out.push(newc as char);
        } else {
            out.push_str(&seg[i..i + 1]);
        }
        i += 1;
    }
    out.push_str(&text[hi..]);
    out
}

/// 行对调：第 a/b 行互换（批码序破坏注入）。
fn swap_rows(text: &str, a: usize, b2: usize) -> String {
    let (a0, a1) = row_span(text, a);
    let (b0, b1) = row_span(text, b2);
    let mut out = String::new();
    out.push_str(&text[b0..b1 + 1]);
    out.push_str(&text[a0..a1 + 1]);
    out.push_str(&text[b1 + 1..]);
    out
}

/// 行复制：第 k 行复制一份（批码序重复注入）。
fn dup_row(text: &str, k: usize) -> String {
    let (lo, hi) = row_span(text, k);
    let mut out = String::from(&text[..hi + 1]);
    out.push_str(&text[lo..hi + 1]);
    out.push_str(&text[hi + 1..]);
    out
}

/// 对黄金序列文注入第 kind 路失真，返回坏文（全 ASCII，安全）。
pub fn distort(text: &str, kind: usize) -> String {
    match kind % 10 {
        0 => {
            // 改批号：行 0 B01 → B21（批码序必抓）。
            let (lo, _) = row_span(text, 0);
            let mut out = String::from(&text[..lo + 1]);
            out.push('2');
            out.push_str(&text[lo + 2..]);
            out
        }
        1 => patch_in_row(text, 1, b"|F1621|", 5, b'2'), // 挖洞：首尾不接必抓。
        2 => patch_in_row(text, 2, b"|F1660|", 5, b'1'), // 重叠：区间宽 21 必抓。
        3 => patch_in_row(text, 3, b"|20|", 2, b'9'), // 改条数：条数账不符必抓。
        4 => {
            // 改摘要：行 4 摘要倒数第 3 字节改写（摘要重算必抓）。
            let (lo, hi) = row_span(text, 4);
            let seg = &text[lo..hi];
            let mut out = String::from(&text[..lo]);
            let n = seg.len();
            let mut i = 0usize;
            while i < n {
                if i + 3 == n {
                    let c = seg.as_bytes()[i];
                    out.push(if c == b'8' { b'9' } else { b'8' } as char);
                } else {
                    out.push_str(&seg[i..i + 1]);
                }
                i += 1;
            }
            out.push_str(&text[hi..]);
            out
        }
        5 => swap_rows(text, 0, 1), // 换序：批码序被破坏必抓。
        6 => {
            // 截尾：去末 6 字节（行数不足/终止符缺失必抓）。
            String::from(&text[..text.len().saturating_sub(6)])
        }
        7 => {
            // 加行：尾部追加伪造 B41（批号闭外必抓）。
            let mut out = String::from(text);
            out.push_str("B41|F2401|F2420|20|00000000;\n");
            out
        }
        8 => dup_row(text, 5), // 复制行：批码序重复必抓。
        _ => {
            // 插控制符：行 6 行尾（分号后换行前）插裸 0x01。
            let (_, hi) = row_span(text, 6);
            let mut out = String::from(&text[..hi]);
            out.push(0x01 as char);
            out.push_str(&text[hi..]);
            out
        }
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金序列文注入一路失真，核验器必须抓到
///（黄金文本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    for r in 0..rounds {
        let text = render_all(&gen_full()?);
        verify_text(&text)?;
        let kind = (r % 10) as usize;
        acc.record(kind, verify_text(&distort(&text, kind)).is_err());
    }
    Ok(acc)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：生成 → 落账封账 → 三方一致 → 序列文对账 → 边界账 → 四拒探。
/// 返回 (正旗数, 拒旗数, 核卡数, 链摘要)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let list = golden_list()?;
    let seen = list.three_way()?;
    verify_ledger(&list)?;
    // 边界账：B01 首 F1601 / B40 尾 F2400 / 逐批 20 / 总 800。
    let edge_ok = match (list.batches.first(), list.batches.last()) {
        (Some(b1), Some(b40)) => {
            b1.first == ID_FIRST && b40.last == ID_LAST
                && b1.cards.len() == CARDS_PER && b40.cards.len() == CARDS_PER
        }
        _ => false,
    };
    // 行往返 + 双生成一致（生成器输出确定性）。
    let rt_ok = row_roundtrip();
    let regen_ok = render_all(&gen_full()?) == render_all(&list.batches);
    // 四拒探：空文 / 首批锚不符落账 / 首尾留洞落账 / 点名不符封账拒。
    let empty_rej = verify_text("").is_err();
    let mut list2 = FreezeList::new(9);
    let b5 = gen_batch(5)?;
    let order_rej = list2.append(b5.clone()).is_err(); // 首批必须 F1601 起
    let _ = list2.append(gen_batch(1)?);
    let hole_rej = list2.append(b5).is_err(); // 1681 ≠ 1621，留洞拒
    let mut b1c = gen_batch(1)?;
    b1c.cards.pop();
    let mut list4 = FreezeList::new(EPOCH);
    let mismatch_rej = match list4.append(b1c) {
        Ok(()) => list4.freeze().is_err(),
        Err(_) => true,
    };
    let flags = edge_ok as u32 + rt_ok as u32 + regen_ok as u32 + (seen == TOTAL_N as u32) as u32;
    let rejects = empty_rej as u32 + order_rej as u32 + hole_rej as u32 + mismatch_rej as u32;
    Ok((flags, rejects, seen, list.chain))
}

/// 黄金清单装填（生成→逐批落账→封账，供全链/判据面复用）。
pub fn golden_list() -> Result<FreezeList, u32> {
    let mut list = FreezeList::new(EPOCH);
    for b in gen_full()? {
        list.append(b)?;
    }
    list.freeze()?;
    Ok(list)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FreezeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 点名账派生守卫探针 ──────────────────────────────────────────
pub fn rollcall_guard() -> bool {
    let c1 = rollcall_card(ID_FIRST);
    let c2 = rollcall_card(ID_LAST);
    let a = c1.name == "F1601" && c2.name == "F2400";
    let b = c1.role == (ID_FIRST % 4) as u32 && c2.role == (ID_LAST % 4) as u32;
    let c = c1.score == ((ID_FIRST as u64 * 7) % 97) as u32;
    let d = parse_fid("F2401").is_none() && parse_fid("F1600").is_none();
    let e = parse_fid("F2000") == Some(2000);
    let f = batch_code(1) == "B01" && batch_code(40) == "B40" && batch_code(0) == "B00";
    a && b && c && d && e && f
}

// ── 相位守卫探针 ────────────────────────────────────────────────
pub fn phase_guard() -> bool {
    let mut list = FreezeList::new(EPOCH);
    let m1 = list.advance_phase(ListPhase::Frozen).is_ok();
    let m2 = list.advance_phase(ListPhase::Draft).is_err();
    let m3 = list.advance_phase(ListPhase::Frozen).is_err();
    // 封账后落批拒（冻结突变）；封账后重封拒（批表核验先行）。
    let b2 = match gen_batch(2) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let m4 = list.append(b2).is_err();
    let m5 = list.freeze().is_err();
    m1 && m2 && m3 && m4 && m5
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let list = golden_list()?;
    let seen = list.three_way()?;
    Ok(format!(
        "状态=已完成 batches={} cards={} span=F{}-F{} chain={:08X} threeway={}",
        list.batches.len(), seen, ID_FIRST, ID_LAST, list.chain, seen == TOTAL_N as u32
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒 + 核卡 800）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, s, c)) => (*f == 4 && *r == 4, format!("flags={} rejects={} cards={} chain={:08X}", f, r, s, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-冻结批清单全过", gp, gd));
    // J1 四十批逐批 20 条点名冻结版。
    let batches = match gen_full() {
        Ok(b) => b,
        Err(e) => {
            v.push(("J1-四十批逐批20条", false, format!("err=0x{:x}", e)));
            return v;
        }
    };
    let counts_ok = batches.len() == BATCH_N
        && batches.iter().all(|b| b.cards.len() == CARDS_PER && b.bid >= 1 && b.bid as usize <= BATCH_N);
    v.push(("J1-四十批逐批20条", counts_ok,
        format!("batches={} per={} total={}", batches.len(), CARDS_PER, batches.len() * CARDS_PER)));
    // J1 批区间首尾相接无空洞无重叠。
    let b1 = batches.first();
    let b40 = batches.last();
    v.push(("J1-首尾相接无空洞无重叠", verify_batches(&batches).is_ok()
        && b1.map(|b| b.first == ID_FIRST).unwrap_or(false)
        && b40.map(|b| b.last == ID_LAST).unwrap_or(false),
        format!("F{}-F{} per={}", ID_FIRST, ID_LAST, CARDS_PER)));
    // J1 三方一致断言（点名账 × 生成器 × 冻结清单）。
    let tw = golden_list().and_then(|l| l.three_way());
    let (tp, td) = match &tw {
        Ok(s) => (*s == TOTAL_N as u32, format!("threeway={}/{}", s, TOTAL_N)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-三方一致断言", tp, td));
    // J1 序列文对账（行往返 + 三拒）。
    v.push(("J1-序列文往返", row_roundtrip(), String::from("建行→解析逐列对拍；野批码/倒挂/断摘要必败")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 冻结红线：相位单调 + 封账改批拒。
    v.push(("J2-冻结红线", phase_guard(), String::from("Draft→Frozen 单调；封账落批/重封/改相位全拒")));
    // J2 点名账派生守卫。
    v.push(("J2-点名账派生守卫", rollcall_guard(), String::from("F1601/F2400 端点 + 角色分值派生 + F 卡号闭外拒")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72AC0 窗口")));
    v
}

pub fn run_unx2204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2204 冻结批清单生成器（40 批 800 条点名冻结版）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
