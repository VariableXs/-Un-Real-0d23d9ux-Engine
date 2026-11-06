//! UNX-F11600 B20 批 ktest 断言集聚合
//!
//! B20 批的 20 条判据按域切成四组（busybox / elf / locale / ktest），每组
//! 五张卡，每卡三条断言，合计 60 条。与平坦聚合不同，这一层先把断言折到
//! 组，再把组折到批，因此任何一组红都会在总表上留下可定位的组位。
//!
//! 断言有三种结局：通过、失败、跳过。跳过不是通过——没跑的断言不能拿来
//! 证明「通过率 100%」，所以只要出现一条跳过，整批就不许进绿档，闸门落
//! `KA_ESKIP`；出现失败落 `KA_EVER`；覆盖不足 20 卡 × 3 断言落 `KA_ENOBS`。
//!
//! 通过率按权重计、向上取整为千分比：每条断言带 1..3 的权重（越靠后的
//! 断言覆盖越多路径），117/120 = 975‰，119/120 向上取整 = 992‰。权重和
//! 之所以不进分母之外的任何位置，是为了让「少跑一条断言」无法靠权重
//! 取巧变绿。
//!
//! 闸门次序：字节 → 名称 → 断言 → 卡 → 组 → 批 → 加权率 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfad0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 域组数。
pub const GROUPS: usize = 4;

/// 每域卡数。
pub const PER_GROUP: usize = 5;

/// B20 批卡数。
pub const CARDS20: usize = GROUPS * PER_GROUP;

/// 每卡断言数。
pub const CASE_LIMIT: usize = 3;

/// B20 批断言总数。
pub const ASSERTIONS: usize = CARDS20 * CASE_LIMIT;

/// 名称容量。
pub const NAME_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const KA_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const KA_ENAME: u32 = SEG_BASE + 0x02;
/// 编号重复。
pub const KA_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const KA_ELEN: u32 = SEG_BASE + 0x04;
/// 断言编号重复。
pub const KA_ECASE: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const KA_ESTATE: u32 = SEG_BASE + 0x06;
/// 存在跳过。
pub const KA_ESKIP: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const KA_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const KA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const KA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const KA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const KA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const KA_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 卡装载。
pub const OP_CARD: u8 = 0x01;
/// 断言登记。
pub const OP_CASE: u8 = 0x02;
/// 折批。
pub const OP_FOLD: u8 = 0x03;
/// 加权率。
pub const OP_WEIGHT: u8 = 0x04;
/// 报告渲染。
pub const OP_REPORT: u8 = 0x05;
/// 绿档闸门。
pub const OP_GATE: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 十进制渲染
// ---------------------------------------------------------------------------

/// 追加一个无符号十进制数。
fn push_dec(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 10];
    let mut len = 0usize;
    while n > 0 {
        let d = (n % 10) as u8;
        if let Some(dst) = buf.get_mut(len) {
            *dst = b'0' + d;
        }
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        if let Some(b) = buf.get(len) {
            out.push(*b as char);
        }
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长卡名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 装载：可打印 ASCII，16 字节内。
    pub fn new(bytes: &[u8]) -> Result<Name, u32> {
        if bytes.is_empty() {
            return Err(KA_ENAME);
        }
        if bytes.len() > NAME_CAP {
            return Err(KA_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(KA_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(KA_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: bytes.len() })
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 逐字节比对。
    pub fn same(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.buf.get(i), other.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 文本视图。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 域组
// ---------------------------------------------------------------------------

/// 四个判据域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// busybox 分发面。
    Busybox,
    /// ELF 装载面。
    Elf,
    /// locale 与字符面。
    Locale,
    /// ktest 自检面。
    Ktest,
}

impl Group {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Group::Busybox => "busybox",
            Group::Elf => "elf",
            Group::Locale => "locale",
            Group::Ktest => "ktest",
        }
    }

    /// 编码。
    pub fn code(&self) -> u8 {
        match self {
            Group::Busybox => 1,
            Group::Elf => 2,
            Group::Locale => 3,
            Group::Ktest => 4,
        }
    }

    /// 组序（0..3）。
    pub fn index(&self) -> usize {
        match self {
            Group::Busybox => 0,
            Group::Elf => 1,
            Group::Locale => 2,
            Group::Ktest => 3,
        }
    }

    /// 按组序反查。
    pub fn at(i: usize) -> Option<Group> {
        match i {
            0 => Some(Group::Busybox),
            1 => Some(Group::Elf),
            2 => Some(Group::Locale),
            3 => Some(Group::Ktest),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 断言
// ---------------------------------------------------------------------------

/// 断言结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 跳过（未实际执行）。
    Skip,
}

impl Outcome {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::Fail => "FAIL",
            Outcome::Skip => "SKIP",
        }
    }

    /// 编码。
    pub fn code(&self) -> u8 {
        match self {
            Outcome::Pass => 1,
            Outcome::Fail => 2,
            Outcome::Skip => 3,
        }
    }

    /// 是否算作已通过。
    pub fn passed(&self) -> bool {
        match self {
            Outcome::Pass => true,
            _ => false,
        }
    }
}

/// 一条断言：编号 + 结局 + 权重。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    id: u8,
    outcome: Outcome,
    weight: u16,
}

impl Case {
    /// 装载：编号不得为 0，权重不得为 0。
    pub fn new(id: u8, outcome: Outcome, weight: u16) -> Result<Case, u32> {
        if id == 0 || weight == 0 {
            return Err(KA_EVALUE);
        }
        Ok(Case { id, outcome, weight })
    }

    /// 编号。
    pub fn id(&self) -> u8 {
        self.id
    }

    /// 结局。
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// 权重。
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// 得分：通过得权重，其余为 0。
    pub fn points(&self) -> u16 {
        if self.outcome.passed() {
            self.weight
        } else {
            0
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.id as u64);
        h = fold(h, self.outcome.code() as u64);
        h = fold(h, self.weight as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 卡
// ---------------------------------------------------------------------------

/// 单卡结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardVerdict {
    /// 三条全过。
    Pass,
    /// 有失败。
    Fail,
    /// 有跳过。
    Skip,
    /// 条数不足。
    Incomplete,
}

impl CardVerdict {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            CardVerdict::Pass => "PASS",
            CardVerdict::Fail => "FAIL",
            CardVerdict::Skip => "SKIP",
            CardVerdict::Incomplete => "INCOMPLETE",
        }
    }

    /// 是否不可放行。
    pub fn bad(&self) -> bool {
        match self {
            CardVerdict::Pass => false,
            _ => true,
        }
    }
}

/// 一张判据卡：编号 + 域 + 名称 + 三条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    no: u16,
    group: Group,
    name: Name,
    cases: [Option<Case>; CASE_LIMIT],
    len: usize,
}

impl Card {
    /// 装载：编号不得为 0。
    pub fn new(no: u16, group: Group, name: &[u8]) -> Result<Card, u32> {
        if no == 0 {
            return Err(KA_EVALUE);
        }
        let n = Name::new(name)?;
        Ok(Card { no, group, name: n, cases: [None; CASE_LIMIT], len: 0 })
    }

    /// 编号。
    pub fn no(&self) -> u16 {
        self.no
    }

    /// 域。
    pub fn group(&self) -> Group {
        self.group
    }

    /// 名称字节。
    pub fn name(&self) -> &[u8] {
        self.name.bytes()
    }

    /// 名称文本。
    pub fn name_text(&self) -> String {
        self.name.text()
    }

    /// 断言条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空卡。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取第 i 条。
    pub fn case(&self, i: usize) -> Option<Case> {
        if i >= self.len {
            return None;
        }
        self.cases.get(i).copied().unwrap_or(None)
    }

    /// 按编号找。
    pub fn find(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                if c.id() == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 挂一条：同号拒绝，超额拒绝。
    pub fn push(&mut self, c: Case) -> Result<(), u32> {
        if self.find(c.id()).is_some() {
            return Err(KA_ECASE);
        }
        if self.len >= CASE_LIMIT {
            return Err(KA_ELEN);
        }
        if let Some(dst) = self.cases.get_mut(self.len) {
            *dst = Some(c);
        }
        self.len += 1;
        Ok(())
    }

    /// 通过条数。
    pub fn passed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                if c.outcome() == Outcome::Pass {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败条数。
    pub fn failed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                if c.outcome() == Outcome::Fail {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 跳过条数。
    pub fn skipped(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                if c.outcome() == Outcome::Skip {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 权重和。
    pub fn weight_total(&self) -> u32 {
        let mut w = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                w = w.saturating_add(c.weight() as u32);
            }
            i += 1;
        }
        w
    }

    /// 得分和。
    pub fn points(&self) -> u32 {
        let mut w = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cases.get(i) {
                w = w.saturating_add(c.points() as u32);
            }
            i += 1;
        }
        w
    }

    /// 条数是否齐备。
    pub fn complete(&self) -> bool {
        self.len == CASE_LIMIT
    }

    /// 卡结论。
    pub fn verdict(&self) -> CardVerdict {
        if !self.complete() {
            return CardVerdict::Incomplete;
        }
        if self.failed() > 0 {
            return CardVerdict::Fail;
        }
        if self.skipped() > 0 {
            return CardVerdict::Skip;
        }
        CardVerdict::Pass
    }

    /// 卡是否绿。
    pub fn green(&self) -> bool {
        self.verdict() == CardVerdict::Pass
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.no as u64);
        h = fold(h, self.group.code() as u64);
        let nb = self.name.bytes();
        let mut i = 0usize;
        while i < nb.len() {
            if let Some(b) = nb.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(Some(c)) = self.cases.get(k) {
                h = fold(h, c.seal());
            }
            k += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 批
// ---------------------------------------------------------------------------

/// 20 卡批，按编号升序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    cards: [Option<Card>; CARDS20],
    len: usize,
}

impl Batch {
    /// 空批。
    pub fn new() -> Batch {
        Batch { cards: [None; CARDS20], len: 0 }
    }

    /// 卡数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空批。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取卡。
    pub fn card(&self, i: usize) -> Option<Card> {
        if i >= self.len {
            return None;
        }
        self.cards.get(i).copied().unwrap_or(None)
    }

    /// 按编号找位。
    pub fn find(&self, no: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cards.get(i) {
                if c.no() == no {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否升序。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            let prev = match self.cards.get(i - 1) {
                Some(Some(v)) => *v,
                _ => return false,
            };
            let cur = match self.cards.get(i) {
                Some(Some(v)) => *v,
                _ => return false,
            };
            if prev.no() >= cur.no() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 升序插位入批。
    pub fn add(&mut self, card: Card) -> Result<(), u32> {
        if self.find(card.no()).is_some() {
            return Err(KA_EDUP);
        }
        if self.len >= CARDS20 {
            return Err(KA_ELEN);
        }
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            match self.cards.get(i) {
                Some(Some(c)) => {
                    if c.no() > card.no() {
                        pos = i;
                        break;
                    }
                }
                _ => return Err(KA_ESLOT),
            }
            i += 1;
        }
        let mut j = self.len;
        while j > pos {
            let src = match self.cards.get(j - 1).copied() {
                Some(v) => v,
                None => return Err(KA_ESLOT),
            };
            if let Some(dst) = self.cards.get_mut(j) {
                *dst = src;
            }
            j -= 1;
        }
        if let Some(dst) = self.cards.get_mut(pos) {
            *dst = Some(card);
        }
        self.len += 1;
        Ok(())
    }

    /// 某域卡数。
    pub fn group_cards(&self, g: Group) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cards.get(i) {
                if c.group() == g {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖档：20 卡齐备且每卡三条。
    pub fn coverage(&self) -> Result<(), u32> {
        if self.len != CARDS20 {
            return Err(KA_ENOBS);
        }
        let mut i = 0usize;
        while i < self.len {
            let c = match self.cards.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(KA_ESLOT),
            };
            if !c.complete() {
                return Err(KA_ENOBS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 断言总数。
    pub fn assertions(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cards.get(i) {
                n += c.len();
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 组聚合
// ---------------------------------------------------------------------------

/// 单组聚合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupAgg {
    group: Group,
    cards: usize,
    cases: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    weight: u32,
    points: u32,
}

impl GroupAgg {
    /// 空组。
    pub const fn zero(group: Group) -> GroupAgg {
        GroupAgg {
            group,
            cards: 0,
            cases: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            weight: 0,
            points: 0,
        }
    }

    /// 组。
    pub fn group(&self) -> Group {
        self.group
    }

    /// 卡数。
    pub fn cards(&self) -> usize {
        self.cards
    }

    /// 断言数。
    pub fn cases(&self) -> usize {
        self.cases
    }

    /// 通过数。
    pub fn passed(&self) -> usize {
        self.passed
    }

    /// 失败数。
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// 跳过数。
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// 权重和。
    pub fn weight(&self) -> u32 {
        self.weight
    }

    /// 得分和。
    pub fn points(&self) -> u32 {
        self.points
    }

    /// 加权通过率千分比。
    pub fn rate(&self) -> u32 {
        if self.weight == 0 {
            return 0;
        }
        let p = (self.points as u64).saturating_mul(1000);
        let d = self.weight as u64;
        (p.saturating_add(d - 1) / d) as u32
    }

    /// 组是否绿。
    pub fn green(&self) -> bool {
        self.failed == 0 && self.skipped == 0 && self.cards == PER_GROUP
    }

    /// 并入一卡。
    pub fn with_card(self, c: Card) -> GroupAgg {
        let mut g = self;
        g.cards += 1;
        g.cases += c.len();
        g.passed += c.passed();
        g.failed += c.failed();
        g.skipped += c.skipped();
        g.weight = g.weight.saturating_add(c.weight_total());
        g.points = g.points.saturating_add(c.points());
        g
    }
}

// ---------------------------------------------------------------------------
// 批结论与汇总
// ---------------------------------------------------------------------------

/// 批结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 全绿。
    Green,
    /// 未完成或有跳过。
    Amber,
    /// 有失败。
    Red,
}

impl Verdict {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Verdict::Green => "GREEN",
            Verdict::Amber => "AMBER",
            Verdict::Red => "RED",
        }
    }

    /// 编码。
    pub fn code(&self) -> u32 {
        match self {
            Verdict::Green => 1,
            Verdict::Amber => 2,
            Verdict::Red => 3,
        }
    }

    /// 是否不可放行。
    pub fn bad(&self) -> bool {
        match self {
            Verdict::Green => false,
            _ => true,
        }
    }
}

/// 汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    groups: [GroupAgg; GROUPS],
    cards: usize,
    cases: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    weight: u32,
    points: u32,
    rate: u32,
    reds: [u16; CARDS20],
    red_len: usize,
}

impl Summary {
    /// 空汇总。
    pub const fn new() -> Summary {
        Summary {
            groups: [
                GroupAgg::zero(Group::Busybox),
                GroupAgg::zero(Group::Elf),
                GroupAgg::zero(Group::Locale),
                GroupAgg::zero(Group::Ktest),
            ],
            cards: 0,
            cases: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            weight: 0,
            points: 0,
            rate: 0,
            reds: [0u16; CARDS20],
            red_len: 0,
        }
    }

    /// 卡数。
    pub fn cards(&self) -> usize {
        self.cards
    }

    /// 断言数。
    pub fn cases(&self) -> usize {
        self.cases
    }

    /// 通过数。
    pub fn passed(&self) -> usize {
        self.passed
    }

    /// 失败数。
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// 跳过数。
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// 权重和。
    pub fn weight(&self) -> u32 {
        self.weight
    }

    /// 得分和。
    pub fn points(&self) -> u32 {
        self.points
    }

    /// 加权通过率千分比。
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// 非空组数。
    pub fn group_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < GROUPS {
            if let Some(g) = self.groups.get(i) {
                if g.cards() > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 取一组。
    pub fn group(&self, g: Group) -> GroupAgg {
        match self.groups.get(g.index()) {
            Some(v) => *v,
            None => GroupAgg::zero(g),
        }
    }

    /// 红卡数。
    pub fn red_len(&self) -> usize {
        self.red_len
    }

    /// 取红卡编号。
    pub fn red(&self, i: usize) -> Option<u16> {
        if i >= self.red_len {
            return None;
        }
        self.reds.get(i).copied()
    }

    /// 红卡编号串。
    pub fn red_text(&self) -> String {
        let mut s = String::new();
        if self.red_len == 0 {
            s.push('-');
            return s;
        }
        let mut i = 0usize;
        while i < self.red_len {
            if i > 0 {
                s.push(',');
            }
            if let Some(no) = self.reds.get(i) {
                push_dec(&mut s, *no as u32);
            }
            i += 1;
        }
        s
    }

    /// 绿档：零失败、零跳过、20 卡 60 断言。
    pub fn green(&self) -> bool {
        self.failed == 0 && self.skipped == 0 && self.cards == CARDS20 && self.cases == ASSERTIONS
    }

    /// 绿档闸门：失败优先，其次跳过，再次覆盖。
    pub fn gate(&self) -> Result<(), u32> {
        if self.failed > 0 {
            return Err(KA_EVER);
        }
        if self.skipped > 0 {
            return Err(KA_ESKIP);
        }
        if self.cards != CARDS20 || self.cases != ASSERTIONS {
            return Err(KA_ENOBS);
        }
        Ok(())
    }

    /// 结论。
    pub fn verdict(&self) -> Verdict {
        if self.failed > 0 {
            Verdict::Red
        } else if !self.green() {
            Verdict::Amber
        } else {
            Verdict::Green
        }
    }

    /// 各组卡数串 `5/5/5/5`。
    pub fn by_group_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < GROUPS {
            if i > 0 {
                s.push('/');
            }
            let n = match self.groups.get(i) {
                Some(g) => g.cards() as u32,
                None => 0,
            };
            push_dec(&mut s, n);
            i += 1;
        }
        s
    }

    /// 单行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("b20 groups=");
        push_dec(&mut s, self.group_count() as u32);
        s.push_str(" cards=");
        push_dec(&mut s, self.cards as u32);
        s.push_str(" cases=");
        push_dec(&mut s, self.cases as u32);
        s.push_str(" pass=");
        push_dec(&mut s, self.passed as u32);
        s.push_str(" fail=");
        push_dec(&mut s, self.failed as u32);
        s.push_str(" skip=");
        push_dec(&mut s, self.skipped as u32);
        s.push_str(" weight=");
        push_dec(&mut s, self.weight);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate);
        s.push_str("ppm by_group=");
        let bv = self.by_group_text();
        s.push_str(bv.as_str());
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.cards as u64);
        h = fold(h, self.cases as u64);
        h = fold(h, self.passed as u64);
        h = fold(h, self.failed as u64);
        h = fold(h, self.skipped as u64);
        h = fold(h, self.weight as u64);
        h = fold(h, self.points as u64);
        h = fold(h, self.rate as u64);
        let mut i = 0usize;
        while i < self.red_len {
            if let Some(no) = self.reds.get(i) {
                h = fold(h, *no as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 折批
// ---------------------------------------------------------------------------

/// 把批折成汇总。
pub fn fold_batch(batch: &Batch) -> Result<Summary, u32> {
    if batch.is_empty() {
        return Err(KA_ESTATE);
    }
    let mut sum = Summary::new();
    let mut reds = [0u16; CARDS20];
    let mut red_len = 0usize;
    let mut i = 0usize;
    while i < batch.len() {
        let c = match batch.card(i) {
            Some(v) => v,
            None => return Err(KA_ESLOT),
        };
        let idx = c.group().index();
        let cur = match sum.groups.get(idx) {
            Some(v) => *v,
            None => return Err(KA_ESLOT),
        };
        let next = cur.with_card(c);
        if let Some(dst) = sum.groups.get_mut(idx) {
            *dst = next;
        }
        sum.cards += 1;
        sum.cases += c.len();
        sum.passed += c.passed();
        sum.failed += c.failed();
        sum.skipped += c.skipped();
        sum.weight = sum.weight.saturating_add(c.weight_total());
        sum.points = sum.points.saturating_add(c.points());
        if !c.green() && red_len < CARDS20 {
            if let Some(dst) = reds.get_mut(red_len) {
                *dst = c.no();
            }
            red_len += 1;
        }
        i += 1;
    }
    sum.reds = reds;
    sum.red_len = red_len;
    sum.rate = if sum.weight == 0 {
        0
    } else {
        ((sum.points as u64).saturating_mul(1000).saturating_add(sum.weight as u64 - 1) / sum.weight as u64) as u32
    };
    Ok(sum)
}

/// 一次折批到底的口子。
pub fn fold_once() -> Result<Summary, u32> {
    let b = sample_batch()?;
    fold_batch(&b)
}

// ---------------------------------------------------------------------------
// 样本
// ---------------------------------------------------------------------------

/// 20 张卡名称。
const NAMES20: [&[u8]; CARDS20] = [
    b"bb_ls_matrix",
    b"bb_cp_sem",
    b"bb_ash",
    b"bb_date_spec",
    b"bb_sed_sub",
    b"elf_magic",
    b"elf_hdr",
    b"elf_phoff",
    b"elf_ptload",
    b"elf_etype",
    b"lc_collate",
    b"lc_ctype",
    b"lc_num",
    b"lc_time",
    b"lc_msg",
    b"kt_batch",
    b"kt_gate",
    b"kt_report",
    b"kt_budget",
    b"kt_replay",
];

/// 编号所在的域。
pub fn group_of(no: u16) -> Option<Group> {
    if no == 0 || (no as usize) > CARDS20 {
        return None;
    }
    Group::at(((no as usize) - 1) / PER_GROUP)
}

/// 造一张标准卡（三条全过，权重 1/2/3）。
pub fn sample_card(no: u16) -> Result<Card, u32> {
    let group = match group_of(no) {
        Some(v) => v,
        None => return Err(KA_EVALUE),
    };
    let name = match NAMES20.get((no as usize) - 1) {
        Some(v) => *v,
        None => return Err(KA_ENOBS),
    };
    let mut c = Card::new(no, group, name)?;
    let mut i = 0u8;
    while i < CASE_LIMIT as u8 {
        let case = Case::new(i + 1, Outcome::Pass, (i as u16) + 1)?;
        c.push(case)?;
        i += 1;
    }
    Ok(c)
}

/// 20 卡全绿样本。
pub fn sample_batch() -> Result<Batch, u32> {
    let mut b = Batch::new();
    let mut no = 1u16;
    while no <= CARDS20 as u16 {
        b.add(sample_card(no)?)?;
        no += 1;
    }
    Ok(b)
}

/// 换一张卡里某条断言的结局。
fn with_outcome(no: u16, at: usize, outcome: Outcome) -> Result<Card, u32> {
    let card = sample_card(no)?;
    let mut fixed = Card::new(card.no(), card.group(), card.name())?;
    let mut i = 0usize;
    while i < card.len() {
        let cs = match card.case(i) {
            Some(v) => v,
            None => return Err(KA_ESLOT),
        };
        let turned = if i == at {
            Case::new(cs.id(), outcome, cs.weight())?
        } else {
            cs
        };
        fixed.push(turned)?;
        i += 1;
    }
    Ok(fixed)
}

/// 失败样本：第 12 卡（locale 组）第三条断言失败。
pub fn sample_fail() -> Result<Batch, u32> {
    let mut b = Batch::new();
    let mut no = 1u16;
    while no <= CARDS20 as u16 {
        let card = if no == 12 {
            with_outcome(no, 2, Outcome::Fail)?
        } else {
            sample_card(no)?
        };
        b.add(card)?;
        no += 1;
    }
    Ok(b)
}

/// 跳过样本：第 5 卡（busybox 组）第一条断言跳过。
pub fn sample_skip() -> Result<Batch, u32> {
    let mut b = Batch::new();
    let mut no = 1u16;
    while no <= CARDS20 as u16 {
        let card = if no == 5 {
            with_outcome(no, 0, Outcome::Skip)?
        } else {
            sample_card(no)?
        };
        b.add(card)?;
        no += 1;
    }
    Ok(b)
}

/// 缺卡样本：只有前 19 卡。
pub fn sample_short() -> Result<Batch, u32> {
    let mut b = Batch::new();
    let mut no = 1u16;
    while no < CARDS20 as u16 {
        b.add(sample_card(no)?)?;
        no += 1;
    }
    Ok(b)
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本一格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KA_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KA_EBAL);
        }
        const VALS: [u32; 8] = [20, 60, 60, 0, 1000, 4, 120, 1];
        const CODES: [u32; 6] = [KA_EVALUE, KA_ECASE, KA_EDUP, KA_EBYTE, KA_ESTATE, KA_ESKIP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KA_EBAL),
            };
            if s.slot as usize != i {
                return Err(KA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KA_EVALUE);
                        }
                    }
                    None => return Err(KA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KA_EVALUE);
                        }
                    }
                    None => return Err(KA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KA_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(KA_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<u64, u32>` → 样本。
fn long_cell(slot: u8, op: u8, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_CARD, sample_batch().map(|b| b.len())));

    let _ = l.register(count_cell(1, OP_FOLD, sample_batch().map(|b| b.assertions())));

    let _ = l.register(count_cell(
        2,
        OP_FOLD,
        sample_batch().and_then(|b| fold_batch(&b)).map(|s| s.passed),
    ));

    let _ = l.register(count_cell(
        3,
        OP_FOLD,
        sample_batch().and_then(|b| fold_batch(&b)).map(|s| s.failed),
    ));

    let _ = l.register(count_cell(
        4,
        OP_WEIGHT,
        sample_batch().and_then(|b| fold_batch(&b)).map(|s| s.rate as usize),
    ));

    let _ = l.register(count_cell(
        5,
        OP_FOLD,
        sample_batch().and_then(|b| fold_batch(&b)).map(|s| s.group_count()),
    ));

    let _ = l.register(count_cell(
        6,
        OP_WEIGHT,
        sample_batch().and_then(|b| fold_batch(&b)).map(|s| s.weight as usize),
    ));

    let _ = l.register(count_cell(
        7,
        OP_GATE,
        sample_batch()
            .and_then(|b| fold_batch(&b))
            .map(|s| s.verdict().code() as usize),
    ));

    let _ = l.register(long_cell(
        8,
        OP_CASE,
        Case::new(1, Outcome::Pass, 0).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        9,
        OP_CASE,
        sample_card(1)
            .and_then(|mut c| {
                let dup = match c.case(0) {
                    Some(v) => v,
                    None => return Err(KA_ESLOT),
                };
                c.push(dup)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        10,
        OP_CARD,
        sample_batch()
            .and_then(|mut b| {
                let again = match sample_card(1) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                b.add(again)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        11,
        OP_CARD,
        Card::new(1, Group::Busybox, &[0x80]).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        12,
        OP_FOLD,
        fold_batch(&Batch::new()).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        13,
        OP_GATE,
        sample_skip()
            .and_then(|b| fold_batch(&b))
            .and_then(|s| s.gate())
            .map(|_| 0u64),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 批装载面。
fn batch_ok() -> bool {
    let b = match sample_batch() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = b.len() == CARDS20 && b.is_sorted() && !b.is_empty();
    let b0 = match (b.card(0), b.card(19)) {
        (Some(x), Some(y)) => {
            x.no() == 1
                && y.no() == 20
                && x.group() == Group::Busybox
                && y.group() == Group::Ktest
                && x.name_text() == String::from("bb_ls_matrix")
                && y.name_text() == String::from("kt_replay")
        }
        _ => false,
    };
    let c = b.find(12) == Some(11) && b.find(99).is_none() && b.card(20).is_none();
    let d = b.assertions() == ASSERTIONS && b.coverage() == Ok(());
    a && b0 && c && d
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let a = Name::new(b"").err() == Some(KA_ENAME);
    let b = Name::new(&[0x80]).err() == Some(KA_EBYTE);
    let c = Name::new(&[b'q'; NAME_CAP + 1]).err() == Some(KA_ELEN);
    let d = Name::new(&[0x1f]).err() == Some(KA_EBYTE) && Name::new(&[0x7f]).err() == Some(KA_EBYTE);
    let e = match Name::new(b"elf_phoff") {
        Ok(n) => n.len() == 9 && n.same(b"elf_phoff") && !n.same(b"elf_phof") && n.text() == String::from("elf_phoff"),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 断言闸门。
fn case_ok() -> bool {
    let a = Case::new(0, Outcome::Pass, 1).err() == Some(KA_EVALUE);
    let b = Case::new(1, Outcome::Pass, 0).err() == Some(KA_EVALUE);
    let c = match Case::new(2, Outcome::Pass, 3) {
        Ok(x) => x.points() == 3 && x.outcome().passed() && x.weight() == 3,
        Err(_) => false,
    };
    let d = match Case::new(3, Outcome::Fail, 3) {
        Ok(x) => x.points() == 0 && !x.outcome().passed(),
        Err(_) => false,
    };
    let e = match Case::new(1, Outcome::Skip, 2) {
        Ok(x) => x.points() == 0 && x.outcome() == Outcome::Skip,
        Err(_) => false,
    };
    let f = Outcome::Pass.text() == "PASS"
        && Outcome::Fail.text() == "FAIL"
        && Outcome::Skip.text() == "SKIP"
        && Outcome::Pass.code() == 1
        && Outcome::Skip.code() == 3;
    let g = Case::new(1, Outcome::Pass, 1).map(|x| x.seal()) != Case::new(1, Outcome::Fail, 1).map(|x| x.seal());
    a && b && c && d && e && f && g
}

/// 卡内断言面。
fn card_ok() -> bool {
    let mut c = match sample_card(1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.len() == CASE_LIMIT && c.passed() == CASE_LIMIT && c.failed() == 0 && c.skipped() == 0;
    let b = c.weight_total() == 6 && c.points() == 6 && c.complete() && c.green();
    let c1 = c.verdict() == CardVerdict::Pass && !CardVerdict::Pass.bad();
    let dup = match c.case(0) {
        Some(v) => v,
        None => return false,
    };
    let d = c.push(dup) == Err(KA_ECASE);
    let e = match Card::new(0, Group::Elf, b"x") {
        Err(KA_EVALUE) => true,
        _ => false,
    } && match Card::new(1, Group::Elf, b"") {
        Err(KA_ENAME) => true,
        _ => false,
    };
    let f = CardVerdict::Fail.text() == "FAIL" && CardVerdict::Skip.bad() && CardVerdict::Incomplete.bad();
    a && b && c1 && d && e && f
}

/// 批排序面。
fn order_ok() -> bool {
    let mut b = Batch::new();
    let a = b.is_empty() && b.len() == 0 && b.card(0).is_none();
    let c3 = match sample_card(3) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c1 = match sample_card(1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c2 = match sample_card(2) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let bb = b.add(c3).is_ok() && b.add(c1).is_ok() && b.add(c2).is_ok();
    let c = b.is_sorted() && b.len() == 3;
    let d = match (b.card(0), b.card(1), b.card(2)) {
        (Some(x), Some(y), Some(z)) => x.no() == 1 && y.no() == 2 && z.no() == 3,
        _ => false,
    };
    let e = match sample_card(1) {
        Ok(v) => b.add(v) == Err(KA_EDUP),
        Err(_) => false,
    };
    a && bb && c && d && e
}

/// 分组面。
fn groups_ok() -> bool {
    let b = match sample_batch() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = b.group_cards(Group::Busybox) == PER_GROUP
        && b.group_cards(Group::Elf) == PER_GROUP
        && b.group_cards(Group::Locale) == PER_GROUP
        && b.group_cards(Group::Ktest) == PER_GROUP;
    let s = match fold_batch(&b) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = s.group_count() == GROUPS && s.group(Group::Elf).cards() == PER_GROUP;
    let d = s.group(Group::Locale).rate() == 1000 && s.group(Group::Ktest).green();
    let e = Group::Busybox.text() == "busybox" && Group::Ktest.code() == 4 && Group::at(2) == Some(Group::Locale) && Group::at(4).is_none();
    let f = group_of(1) == Some(Group::Busybox) && group_of(10) == Some(Group::Elf) && group_of(20) == Some(Group::Ktest) && group_of(21).is_none();
    a && c && d && e && f
}

/// 压一条断言，构造失败即假。
fn push_case(card: &mut Card, id: u8, outcome: Outcome, weight: u16) -> bool {
    match Case::new(id, outcome, weight) {
        Ok(c) => card.push(c).is_ok(),
        Err(_) => false,
    }
}

/// 跳过闸门面。
fn skip_ok() -> bool {
    let b = match sample_skip() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let s = match fold_batch(&b) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s.skipped() == 1 && s.failed() == 0 && s.passed() == ASSERTIONS - 1;
    let c = s.rate() == 992 && !s.green();
    let d = s.gate() == Err(KA_ESKIP) && s.verdict() == Verdict::Amber;
    let e = s.red_len() == 1 && s.red(0) == Some(5) && s.red_text() == String::from("5");
    let card = match b.card(4) {
        Some(v) => v,
        None => return false,
    };
    let f = card.verdict() == CardVerdict::Skip && !card.green();
    let g = match Card::new(9, Group::Elf, b"skipcard") {
        Ok(mut x) => {
            let mut ok = push_case(&mut x, 1, Outcome::Pass, 1);
            ok = ok && push_case(&mut x, 2, Outcome::Skip, 2);
            ok = ok && push_case(&mut x, 3, Outcome::Pass, 3);
            ok && x.verdict() == CardVerdict::Skip
        }
        Err(_) => false,
    };
    a && c && d && e && f && g
}

/// 加权通过率面。
fn weight_ok() -> bool {
    let a = match fold_once() {
        Ok(s) => s.rate() == 1000 && s.weight() == 120 && s.points() == 120,
        Err(_) => false,
    };
    let b = match sample_fail().and_then(|x| fold_batch(&x)) {
        Ok(s) => s.rate() == 975 && s.points() == 117 && s.weight() == 120,
        Err(_) => false,
    };
    let c = match sample_skip().and_then(|x| fold_batch(&x)) {
        Ok(s) => s.rate() == 992 && s.points() == 119,
        Err(_) => false,
    };
    let d = Summary::new().rate() == 0 && GroupAgg::zero(Group::Elf).rate() == 0;
    let e = match sample_card(6) {
        Ok(card) => match GroupAgg::zero(Group::Elf).with_card(card) {
            g => g.rate() == 1000 && g.weight() == 6 && g.points() == 6,
        },
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 汇总面。
fn summarize_ok() -> bool {
    let s = match fold_once() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s.cards() == CARDS20 && s.cases() == ASSERTIONS && s.passed() == ASSERTIONS;
    let b = s.failed() == 0 && s.skipped() == 0 && s.weight() == 120 && s.points() == 120;
    let c = s.rate() == 1000 && s.green() && s.gate() == Ok(()) && s.verdict() == Verdict::Green;
    let d = s.by_group_text() == String::from("5/5/5/5") && s.red_len() == 0 && s.red_text() == String::from("-");
    a && b && c && d
}

/// 报告渲染面。
fn report_ok() -> bool {
    let s = match fold_once() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s.render()
        == String::from("b20 groups=4 cards=20 cases=60 pass=60 fail=0 skip=0 weight=120 rate=1000ppm by_group=5/5/5/5\n");
    let b = match sample_skip().and_then(|x| fold_batch(&x)) {
        Ok(v) => v.render()
            == String::from("b20 groups=4 cards=20 cases=60 pass=59 fail=0 skip=1 weight=120 rate=992ppm by_group=5/5/5/5\n"),
        Err(_) => false,
    };
    let c = match sample_short().and_then(|x| fold_batch(&x)) {
        Ok(v) => v.render()
            == String::from("b20 groups=4 cards=19 cases=57 pass=57 fail=0 skip=0 weight=114 rate=1000ppm by_group=5/5/5/4\n"),
        Err(_) => false,
    };
    let d = Summary::new().render()
        == String::from("b20 groups=0 cards=0 cases=0 pass=0 fail=0 skip=0 weight=0 rate=0ppm by_group=0/0/0/0\n");
    a && b && c && d
}

/// 失败显式面。
fn reds_ok() -> bool {
    let s = match sample_fail().and_then(|x| fold_batch(&x)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s.failed() == 1 && s.passed() == ASSERTIONS - 1 && s.skipped() == 0;
    let b = s.rate() == 975 && s.gate() == Err(KA_EVER) && s.verdict() == Verdict::Red;
    let c = s.red_len() == 1 && s.red(0) == Some(12) && s.red(1).is_none() && s.red_text() == String::from("12");
    let d = s.group(Group::Locale).failed() == 1 && s.group(Group::Elf).failed() == 0;
    let e = s.group(Group::Locale).rate() == 900;
    a && b && c && d && e
}

/// 覆盖档面。
fn cover_ok() -> bool {
    let short = match sample_short() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = short.len() == CARDS20 - 1 && short.coverage() == Err(KA_ENOBS);
    let s = match fold_batch(&short) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = s.cards() == CARDS20 - 1 && s.cases() == ASSERTIONS - CASE_LIMIT && s.passed() == ASSERTIONS - CASE_LIMIT;
    let c = s.rate() == 1000 && s.gate() == Err(KA_ENOBS) && s.verdict() == Verdict::Amber;
    let d = s.by_group_text() == String::from("5/5/5/4") && s.group(Group::Ktest).cards() == PER_GROUP - 1;
    let e = fold_batch(&Batch::new()) == Err(KA_ESTATE);
    let f = !Verdict::Green.bad() && Verdict::Amber.bad() && Verdict::Red.bad();
    a && b && c && d && e && f
}

/// 容量与空表面。
fn bounds_ok() -> bool {
    let s = Summary::new();
    let a = s.cards() == 0 && s.cases() == 0 && s.rate() == 0 && !s.green() && s.red(0).is_none();
    let b = s.by_group_text() == String::from("0/0/0/0") && s.group_count() == 0 && s.red_text() == String::from("-");
    let c = match sample_card(0) {
        Err(KA_EVALUE) => true,
        _ => false,
    } && match sample_card(21) {
        Err(KA_EVALUE) => true,
        _ => false,
    };
    let d = match sample_card(20) {
        Ok(x) => x.group() == Group::Ktest && x.name_text() == String::from("kt_replay"),
        Err(_) => false,
    };
    let e = Batch::new().coverage() == Err(KA_ENOBS) && Batch::new().assertions() == 0;
    let f = {
        let mut full = match sample_batch() {
            Ok(v) => v,
            Err(_) => return false,
        };
        match Card::new(21, Group::Ktest, b"extra") {
            Ok(c) => full.add(c) == Err(KA_ELEN),
            Err(_) => false,
        }
    };
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = batch_ok() && name_ok();
    out[1] = case_ok() && card_ok() && order_ok();
    out[2] = groups_ok() && skip_ok() && weight_ok();
    out[3] = summarize_ok() && report_ok() && reds_ok() && cover_ok() && bounds_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11600_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11600.ktest_b20");

    s.add(
        "KB01-批装载",
        {
            batch_ok()
        },
        "static",
    );

    s.add(
        "KB02-名称与字节",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "KB03-断言闸门",
        {
            case_ok()
        },
        "static",
    );

    s.add(
        "KB04-卡内断言",
        {
            card_ok()
        },
        "static",
    );

    s.add(
        "KB05-批内排序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "KB06-分组聚合",
        {
            groups_ok()
        },
        "static",
    );

    s.add(
        "KB07-跳过闸门",
        {
            skip_ok()
        },
        "static",
    );

    s.add(
        "KB08-加权通过率",
        {
            weight_ok()
        },
        "static",
    );

    s.add(
        "KB09-批汇总",
        {
            summarize_ok()
        },
        "static",
    );

    s.add(
        "KB10-报告渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "KB11-失败显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "KB12-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "KB13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_shape() {
        let b = sample_batch().expect("b");
        assert_eq!(b.len(), CARDS20);
        assert_eq!(b.assertions(), ASSERTIONS);
        assert!(b.is_sorted());
        assert_eq!(b.find(1), Some(0));
        assert_eq!(b.find(20), Some(19));
        assert_eq!(b.find(21), None);
        assert_eq!(b.find(11), Some(10));
        assert_eq!(b.card(10).map(|c| c.name_text()), Some(String::from("lc_collate")));
        assert_eq!(b.card(11).map(|c| c.name_text()), Some(String::from("lc_ctype")));
        assert_eq!(b.card(11).map(|c| c.group()), Some(Group::Locale));
    }

    #[test]
    fn card_gates() {
        assert_eq!(Card::new(0, Group::Elf, b"x").err(), Some(KA_EVALUE));
        assert_eq!(Card::new(1, Group::Elf, b"").err(), Some(KA_ENAME));
        let c = sample_card(1).expect("c");
        assert_eq!(c.len(), CASE_LIMIT);
        assert_eq!(c.weight_total(), 6);
        assert_eq!(c.points(), 6);
        assert_eq!(c.verdict(), CardVerdict::Pass);
    }

    #[test]
    fn case_gates() {
        assert_eq!(Case::new(1, Outcome::Pass, 0).err(), Some(KA_EVALUE));
        assert_eq!(Case::new(0, Outcome::Pass, 1).err(), Some(KA_EVALUE));
        assert_eq!(Case::new(2, Outcome::Fail, 3).expect("c").points(), 0);
        assert_eq!(Case::new(3, Outcome::Skip, 2).expect("s").outcome(), Outcome::Skip);
    }

    #[test]
    fn duplicate_case() {
        let mut c = sample_card(1).expect("c");
        let dup = c.case(0).expect("d");
        assert_eq!(c.push(dup), Err(KA_ECASE));
        let mut hollow = Card::new(2, Group::Elf, b"hollow").expect("h");
        assert!(hollow.push(Case::new(1, Outcome::Pass, 1).expect("1")).is_ok());
        assert!(hollow.push(Case::new(2, Outcome::Pass, 2).expect("2")).is_ok());
        assert!(!hollow.complete());
        assert_eq!(hollow.verdict(), CardVerdict::Incomplete);
        assert!(hollow.push(Case::new(3, Outcome::Pass, 3).expect("3")).is_ok());
        assert!(hollow.complete());
    }

    #[test]
    fn batch_insert() {
        let mut b = Batch::new();
        b.add(sample_card(3).expect("3")).expect("a3");
        b.add(sample_card(1).expect("1")).expect("a1");
        assert!(b.is_sorted());
        assert_eq!(b.card(0).map(|c| c.no()), Some(1));
        assert_eq!(b.card(1).map(|c| c.no()), Some(3));
        assert_eq!(b.add(sample_card(1).expect("1b")), Err(KA_EDUP));
    }

    #[test]
    fn batch_capacity() {
        let mut full = sample_batch().expect("b");
        assert_eq!(full.add(Card::new(21, Group::Ktest, b"extra").expect("x")), Err(KA_ELEN));
    }

    #[test]
    fn fold_full() {
        let s = fold_once().expect("s");
        assert_eq!(s.cards(), CARDS20);
        assert_eq!(s.cases(), ASSERTIONS);
        assert_eq!(s.passed(), ASSERTIONS);
        assert_eq!(s.failed(), 0);
        assert_eq!(s.skipped(), 0);
        assert_eq!(s.weight(), 120);
        assert_eq!(s.rate(), 1000);
        assert_eq!(s.verdict(), Verdict::Green);
        assert_eq!(s.gate(), Ok(()));
    }

    #[test]
    fn fold_fail() {
        let s = fold_batch(&sample_fail().expect("f")).expect("s");
        assert_eq!(s.failed(), 1);
        assert_eq!(s.passed(), ASSERTIONS - 1);
        assert_eq!(s.rate(), 975);
        assert_eq!(s.red(0), Some(12));
        assert_eq!(s.gate(), Err(KA_EVER));
        assert_eq!(s.group(Group::Locale).failed(), 1);
        assert_eq!(s.group(Group::Locale).rate(), 900);
    }

    #[test]
    fn fold_skip() {
        let s = fold_batch(&sample_skip().expect("k")).expect("s");
        assert_eq!(s.skipped(), 1);
        assert_eq!(s.failed(), 0);
        assert_eq!(s.rate(), 992);
        assert_eq!(s.gate(), Err(KA_ESKIP));
        assert_eq!(s.verdict(), Verdict::Amber);
        assert_eq!(s.red_text(), String::from("5"));
    }

    #[test]
    fn fold_short() {
        let b = sample_short().expect("b");
        assert_eq!(b.len(), CARDS20 - 1);
        assert_eq!(b.coverage(), Err(KA_ENOBS));
        let s = fold_batch(&b).expect("s");
        assert_eq!(s.cards(), CARDS20 - 1);
        assert_eq!(s.rate(), 1000);
        assert_eq!(s.gate(), Err(KA_ENOBS));
        assert_eq!(s.by_group_text(), String::from("5/5/5/4"));
    }

    #[test]
    fn group_views() {
        let s = fold_once().expect("s");
        assert_eq!(s.group_count(), GROUPS);
        assert_eq!(s.group(Group::Busybox).cards(), PER_GROUP);
        assert_eq!(s.group(Group::Ktest).rate(), 1000);
        assert!(s.group(Group::Elf).green());
        assert_eq!(s.by_group_text(), String::from("5/5/5/5"));
    }

    #[test]
    fn render_lines() {
        assert_eq!(
            fold_once().expect("s").render().as_str(),
            "b20 groups=4 cards=20 cases=60 pass=60 fail=0 skip=0 weight=120 rate=1000ppm by_group=5/5/5/5\n"
        );
        assert_eq!(
            Summary::new().render().as_str(),
            "b20 groups=0 cards=0 cases=0 pass=0 fail=0 skip=0 weight=0 rate=0ppm by_group=0/0/0/0\n"
        );
    }

    #[test]
    fn empty_summary() {
        let s = Summary::new();
        assert_eq!(s.cards(), 0);
        assert_eq!(s.rate(), 0);
        assert_eq!(s.group_count(), 0);
        assert_eq!(s.red_text(), String::from("-"));
        assert_eq!(fold_batch(&Batch::new()), Err(KA_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(20));
        assert_eq!(l.sample(6).map(|s| s.value), Some(120));
        assert_eq!(l.sample(9).map(|s| s.code), Some(KA_ECASE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(KA_ESKIP));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(KA_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11600_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
