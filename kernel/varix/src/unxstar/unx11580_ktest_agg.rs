//! UNX-F11580 B19 批 ktest 断言集聚合
//!
//! 把 B19 批 19 条判据各自的断言集收进同一张 ktest 注册表：每条判据是一张
//! 「卡」（`Card`），卡上按编号挂一组「用例」（`Case`），用例记着比对型
//! （`Kind`）、权重与本次运行结果。注册表按卡编号升序插位，同一编号只许
//! 出现一次；一次 `run_all` 把 19 张卡全部跑完，聚合面只吐一份 `Report`。
//!
//! 绿的判据是三重的：失败数必须为 0、覆盖档里 19 张卡必须一张不缺、用例
//! 总数必须不小于 `EXPECT_CASES`。只满足「本机没红」而卡数不足的批次落
//! `Amber`（未完成），任何失败落 `Red`（红单），三者之外才是 `Green`。
//!
//! 通过率按用例条数计，向上取整合为千分比：37/38 = 974‰，38/38 = 1000‰。
//! 权重只参与「断言密度」统计，不参与通过率——权重是给后续排序用的，把
//! 权重混进分母会让少一条重用例的批次假装变绿。
//!
//! 闸门次序：字节 → 名称 → 编号 → 用例 → 注册 → 执行 → 聚合 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf500;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// B19 批判据条数。
pub const EXPECT_CARDS: usize = 19;

/// B19 批用例总数。
pub const EXPECT_CASES: usize = 38;

/// 注册表卡容量。
pub const CARD_CAP: usize = 22;

/// 单卡用例容量。
pub const CASE_CAP: usize = 8;

/// 名称容量。
pub const NAME_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const KT_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const KT_ENAME: u32 = SEG_BASE + 0x02;
/// 编号重复。
pub const KT_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const KT_ELEN: u32 = SEG_BASE + 0x04;
/// 用例编号重复。
pub const KT_ECASE: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const KT_ESTATE: u32 = SEG_BASE + 0x06;
/// 执行失败。
pub const KT_EEXEC: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const KT_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const KT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const KT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const KT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const KT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const KT_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 注册入表。
pub const OP_REG: u8 = 0x01;
/// 全跑一次。
pub const OP_RUN: u8 = 0x02;
/// 用例装载。
pub const OP_CASE: u8 = 0x03;
/// 聚合。
pub const OP_AGG: u8 = 0x04;
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

/// 无符号十进制文本。
fn dec_text(v: u32) -> String {
    let mut s = String::new();
    push_dec(&mut s, v);
    s
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长名称：ASCII 可打印，24 字节内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 装载。
    pub fn new(bytes: &[u8]) -> Result<Name, u32> {
        if bytes.is_empty() {
            return Err(KT_ENAME);
        }
        if bytes.len() > NAME_CAP {
            return Err(KT_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(KT_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(KT_EBYTE);
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

    /// 是否空（装载面恒非空，保留给调用方做防御）。
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
// 比对型
// ---------------------------------------------------------------------------

/// 断言比对型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 相等断言。
    Equal,
    /// 不等断言。
    NotEqual,
    /// 有序断言。
    Ordered,
    /// 拒收断言（必须落到某个错误码）。
    Rejects,
    /// 匹配断言。
    Matches,
}

impl Kind {
    /// 短名。
    pub fn text(&self) -> &'static str {
        match self {
            Kind::Equal => "eq",
            Kind::NotEqual => "ne",
            Kind::Ordered => "ord",
            Kind::Rejects => "err",
            Kind::Matches => "match",
        }
    }

    /// 编码。
    pub fn code(&self) -> u8 {
        match self {
            Kind::Equal => 1,
            Kind::NotEqual => 2,
            Kind::Ordered => 3,
            Kind::Rejects => 4,
            Kind::Matches => 5,
        }
    }
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// 单条断言：编号 + 型 + 权重 + 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    id: u8,
    kind: Kind,
    weight: u16,
    passed: bool,
}

impl Case {
    /// 装载：编号不得为 0，权重不得为 0。
    pub fn new(id: u8, kind: Kind, weight: u16, passed: bool) -> Result<Case, u32> {
        if id == 0 {
            return Err(KT_EVALUE);
        }
        if weight == 0 {
            return Err(KT_EVALUE);
        }
        Ok(Case { id, kind, weight, passed })
    }

    /// 编号。
    pub fn id(&self) -> u8 {
        self.id
    }

    /// 型。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 权重。
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// 是否通过。
    pub fn ok(&self) -> bool {
        self.passed
    }

    /// 得分：通过得权重，未过得 0。
    pub fn points(&self) -> u16 {
        if self.passed {
            self.weight
        } else {
            0
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.id as u64);
        h = fold(h, self.kind.code() as u64);
        h = fold(h, self.weight as u64);
        h = fold(h, self.passed as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 运行汇总：通过 / 失败 / 权重。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    passed: usize,
    failed: usize,
    weight: u32,
}

impl Tally {
    /// 空汇总。
    pub const fn new() -> Tally {
        Tally { passed: 0, failed: 0, weight: 0 }
    }

    /// 直接构造（模型面用）。
    pub const fn of(passed: usize, failed: usize, weight: u32) -> Tally {
        Tally { passed, failed, weight }
    }

    /// 并上一份。
    pub fn add(self, o: Tally) -> Tally {
        Tally {
            passed: self.passed + o.passed,
            failed: self.failed + o.failed,
            weight: self.weight.saturating_add(o.weight),
        }
    }

    /// 通过条数。
    pub fn passed(&self) -> usize {
        self.passed
    }

    /// 失败条数。
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// 总条数。
    pub fn total(&self) -> usize {
        self.passed + self.failed
    }

    /// 权重和。
    pub fn weight(&self) -> u32 {
        self.weight
    }

    /// 通过率，千分比向上取整。
    pub fn rate(&self) -> u32 {
        let total = self.total();
        if total == 0 {
            return 0;
        }
        let p = (self.passed as u64).saturating_mul(1000);
        let d = total as u64;
        (p.saturating_add(d - 1) / d) as u32
    }

    /// 绿档：零失败且总数达标。
    pub fn green(&self, expect: usize) -> bool {
        self.failed == 0 && self.total() >= expect
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.passed as u64);
        h = fold(h, self.failed as u64);
        h = fold(h, self.weight as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 卡
// ---------------------------------------------------------------------------

/// 一张判据卡：编号 + 名称 + 一组用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    no: u16,
    name: Name,
    cases: [Option<Case>; CASE_CAP],
    len: usize,
}

impl Card {
    /// 装载：编号不得为 0。
    pub fn new(no: u16, name: &[u8]) -> Result<Card, u32> {
        if no == 0 {
            return Err(KT_EVALUE);
        }
        let n = Name::new(name)?;
        Ok(Card { no, name: n, cases: [None; CASE_CAP], len: 0 })
    }

    /// 编号。
    pub fn no(&self) -> u16 {
        self.no
    }

    /// 名称字节。
    pub fn name(&self) -> &[u8] {
        self.name.bytes()
    }

    /// 名称文本。
    pub fn name_text(&self) -> String {
        self.name.text()
    }

    /// 用例数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空卡。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
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

    /// 挂一条：同号拒绝，满格拒绝。
    pub fn push(&mut self, c: Case) -> Result<(), u32> {
        if self.find(c.id()).is_some() {
            return Err(KT_ECASE);
        }
        if self.len >= CASE_CAP {
            return Err(KT_ELEN);
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
                if c.ok() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败条数。
    pub fn failed(&self) -> usize {
        self.len - self.passed()
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

    /// 跑这一卡：空卡直接落 `KT_EEXEC`。
    pub fn run(&self) -> Result<Tally, u32> {
        if self.len == 0 {
            return Err(KT_EEXEC);
        }
        let mut t = Tally::new();
        let mut i = 0usize;
        while i < self.len {
            let c = match self.cases.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(KT_ESLOT),
            };
            let one = if c.ok() { Tally::of(1, 0, c.weight() as u32) } else { Tally::of(0, 1, c.weight() as u32) };
            t = t.add(one);
            i += 1;
        }
        Ok(t)
    }

    /// 本卡是否绿：非空且零失败。
    pub fn green(&self) -> bool {
        self.len > 0 && self.failed() == 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.no as u64);
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
// 注册表
// ---------------------------------------------------------------------------

/// ktest 注册表：按卡编号升序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registry {
    cards: [Option<Card>; CARD_CAP],
    len: usize,
}

impl Registry {
    /// 空表。
    pub fn new() -> Registry {
        Registry { cards: [None; CARD_CAP], len: 0 }
    }

    /// 卡数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一卡。
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

    /// 升序插位入表。
    pub fn register(&mut self, card: Card) -> Result<(), u32> {
        if self.find(card.no()).is_some() {
            return Err(KT_EDUP);
        }
        if self.len >= CARD_CAP {
            return Err(KT_ELEN);
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
                _ => return Err(KT_ESLOT),
            }
            i += 1;
        }
        let mut j = self.len;
        while j > pos {
            let src = match self.cards.get(j - 1).copied() {
                Some(v) => v,
                None => return Err(KT_ESLOT),
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

    /// 全跑一次：空表落 `KT_ESTATE`。
    pub fn run_all(&self) -> Result<Tally, u32> {
        if self.len == 0 {
            return Err(KT_ESTATE);
        }
        let mut t = Tally::new();
        let mut i = 0usize;
        while i < self.len {
            let c = match self.card(i) {
                Some(v) => v,
                None => return Err(KT_ESLOT),
            };
            t = t.add(c.run()?);
            i += 1;
        }
        Ok(t)
    }

    /// 覆盖档：19 张计划卡一张不缺。
    pub fn coverage(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < EXPECT_CARDS {
            let no = match PLAN.get(i) {
                Some(v) => *v,
                None => return Err(KT_ENOBS),
            };
            if self.find(no).is_none() {
                return Err(KT_ENOBS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 红卡编号串（逗号分隔，无红为 `-`）。
    pub fn red_text(&self) -> String {
        let mut s = String::new();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cards.get(i) {
                if !c.green() {
                    if n > 0 {
                        s.push(',');
                    }
                    push_dec(&mut s, c.no() as u32);
                    n += 1;
                }
            }
            i += 1;
        }
        if n == 0 {
            s.push('-');
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(c)) = self.cards.get(i) {
                h = fold(h, c.seal());
            }
            i += 1;
        }
        fold(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 结论
// ---------------------------------------------------------------------------

/// 批次结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 全绿。
    Green,
    /// 未完成（零失败但卡数不足）。
    Amber,
    /// 红单。
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
            Verdict::Amber => true,
            Verdict::Red => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 聚合报告
// ---------------------------------------------------------------------------

/// 聚合报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    cards: usize,
    cases: usize,
    passed: usize,
    failed: usize,
    rate: u32,
    green_cards: usize,
    reds: [u16; CARD_CAP],
    red_len: usize,
}

impl Report {
    /// 空报告。
    pub fn new() -> Report {
        Report {
            cards: 0,
            cases: 0,
            passed: 0,
            failed: 0,
            rate: 0,
            green_cards: 0,
            reds: [0u16; CARD_CAP],
            red_len: 0,
        }
    }

    /// 卡数。
    pub fn cards(&self) -> usize {
        self.cards
    }

    /// 用例总数。
    pub fn cases(&self) -> usize {
        self.cases
    }

    /// 通过条数。
    pub fn passed(&self) -> usize {
        self.passed
    }

    /// 失败条数。
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// 通过率千分比。
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// 绿卡数。
    pub fn green_cards(&self) -> usize {
        self.green_cards
    }

    /// 红卡数。
    pub fn red_len(&self) -> usize {
        self.red_len
    }

    /// 取一张红卡编号。
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

    /// 绿档：零失败且卡数达标。
    pub fn green(&self) -> bool {
        self.failed == 0 && self.cards >= EXPECT_CARDS
    }

    /// 绿档闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(KT_EVER)
        }
    }

    /// 结论。
    pub fn verdict(&self) -> Verdict {
        if self.failed > 0 {
            Verdict::Red
        } else if self.cards < EXPECT_CARDS {
            Verdict::Amber
        } else {
            Verdict::Green
        }
    }

    /// 结论文本。
    pub fn verdict_text(&self) -> &'static str {
        self.verdict().text()
    }

    /// 结论编码：1 绿 / 2 琥珀 / 3 红。
    pub fn verdict_code(&self) -> u32 {
        self.verdict().code()
    }

    /// 单行渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ktest cards=");
        push_dec(&mut s, self.cards as u32);
        s.push_str(" cases=");
        push_dec(&mut s, self.cases as u32);
        s.push_str(" pass=");
        push_dec(&mut s, self.passed as u32);
        s.push_str(" fail=");
        push_dec(&mut s, self.failed as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate);
        s.push_str("ppm green=");
        push_dec(&mut s, self.green_cards as u32);
        s.push_str(" reds=");
        let rt = self.red_text();
        s.push_str(rt.as_str());
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
        h = fold(h, self.rate as u64);
        h = fold(h, self.green_cards as u64);
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
// 聚合
// ---------------------------------------------------------------------------

/// 一次把整表跑完并聚合。
pub fn aggregate(reg: &Registry) -> Result<Report, u32> {
    let t = reg.run_all()?;
    let mut reds = [0u16; CARD_CAP];
    let mut red_len = 0usize;
    let mut green_cards = 0usize;
    let mut i = 0usize;
    while i < reg.len() {
        let c = match reg.card(i) {
            Some(v) => v,
            None => return Err(KT_ESLOT),
        };
        if c.green() {
            green_cards += 1;
        } else if red_len < CARD_CAP {
            if let Some(dst) = reds.get_mut(red_len) {
                *dst = c.no();
            }
            red_len += 1;
        }
        i += 1;
    }
    Ok(Report {
        cards: reg.len(),
        cases: t.total(),
        passed: t.passed(),
        failed: t.failed(),
        rate: t.rate(),
        green_cards,
        reds,
        red_len,
    })
}

/// 一次命令全跑的口子。
pub fn run_all_once() -> Result<Report, u32> {
    let reg = sample_registry()?;
    aggregate(&reg)
}

// ---------------------------------------------------------------------------
// 样本
// ---------------------------------------------------------------------------

/// 19 张计划卡编号。
const PLAN: [u16; EXPECT_CARDS] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
];

/// 19 张卡的名称（对应 B19 批模块）。
const NAMES: [&[u8]; EXPECT_CARDS] = [
    b"tr_ctype",
    b"wc_mchars",
    b"sed_ycase",
    b"ls_byteshow",
    b"cmp_od",
    b"ujc_order",
    b"date_fmt",
    b"printf_s",
    b"test_expr",
    b"sort_collate",
    b"grep_cclass",
    b"cut_bc",
    b"df_table",
    b"du_rec",
    b"lc_snap",
    b"ktest_agg",
    b"argv0_dispatch",
    b"busybox_list",
    b"applet_form",
];

/// 每卡用例条数：2/3/1 循环，合计 38。
const CASE_COUNTS: [u8; EXPECT_CARDS] = [
    2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2,
];

/// 第 i 条用例的型：五型循环。
fn kind_at(i: usize) -> Kind {
    match i % 5 {
        0 => Kind::Equal,
        1 => Kind::NotEqual,
        2 => Kind::Ordered,
        3 => Kind::Rejects,
        _ => Kind::Matches,
    }
}

/// 造一张标准卡（全过）。
pub fn sample_card(no: u16) -> Result<Card, u32> {
    if no == 0 || (no as usize) > EXPECT_CARDS {
        return Err(KT_EVALUE);
    }
    let idx = (no as usize) - 1;
    let name = match NAMES.get(idx) {
        Some(v) => *v,
        None => return Err(KT_ENOBS),
    };
    let count = match CASE_COUNTS.get(idx) {
        Some(v) => *v,
        None => return Err(KT_ENOBS),
    };
    let mut c = Card::new(no, name)?;
    let mut i = 0u8;
    while i < count {
        let kind = kind_at(i as usize);
        let weight = 1u16 + (i as u16 % 3);
        let case = Case::new(i + 1, kind, weight, true)?;
        c.push(case)?;
        i += 1;
    }
    Ok(c)
}

/// 19 卡全绿样本。
pub fn sample_registry() -> Result<Registry, u32> {
    let mut r = Registry::new();
    let mut no = 1u16;
    while no <= EXPECT_CARDS as u16 {
        r.register(sample_card(no)?)?;
        no += 1;
    }
    Ok(r)
}

/// 红样本：第 7 卡第二条用例判红。
pub fn sample_red() -> Result<Registry, u32> {
    let mut r = Registry::new();
    let mut no = 1u16;
    while no <= EXPECT_CARDS as u16 {
        let mut card = sample_card(no)?;
        if no == 7 {
            let mut fixed = Card::new(card.no(), card.name())?;
            let mut i = 0usize;
            while i < card.len() {
                let cs = match card.case(i) {
                    Some(v) => v,
                    None => return Err(KT_ESLOT),
                };
                let turned = if i == 1 {
                    Case::new(cs.id(), cs.kind(), cs.weight(), false)?
                } else {
                    cs
                };
                fixed.push(turned)?;
                i += 1;
            }
            card = fixed;
        }
        r.register(card)?;
        no += 1;
    }
    Ok(r)
}

/// 琥珀样本卡数：少两张不足以覆盖计划表。
pub const AMBER_CARDS: usize = EXPECT_CARDS - 2;

/// 琥珀样本：只有前 17 卡。
pub fn sample_amber() -> Result<Registry, u32> {
    let mut r = Registry::new();
    let mut no = 1u16;
    while no <= AMBER_CARDS as u16 {
        r.register(sample_card(no)?)?;
        no += 1;
    }
    Ok(r)
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
            return Err(KT_ESLOT);
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
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [19, 38, 38, 0, 1000, 1, 17, 1];
        const CODES: [u32; 6] = [KT_EVALUE, KT_ECASE, KT_EDUP, KT_EBYTE, KT_ESTATE, KT_EVER];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KT_EBAL),
            };
            if s.slot as usize != i {
                return Err(KT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KT_EBAL);
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
            return Err(KT_EBAL);
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
            Err(KT_ESEAL)
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

    let _ = l.register(count_cell(0, OP_REG, sample_registry().map(|r| r.len())));

    let _ = l.register(count_cell(
        1,
        OP_RUN,
        sample_registry().and_then(|r| r.run_all()).map(|t| t.total()),
    ));

    let _ = l.register(count_cell(
        2,
        OP_AGG,
        sample_registry().and_then(|r| aggregate(&r)).map(|x| x.passed),
    ));

    let _ = l.register(count_cell(
        3,
        OP_AGG,
        sample_registry().and_then(|r| aggregate(&r)).map(|x| x.failed),
    ));

    let _ = l.register(count_cell(
        4,
        OP_GATE,
        sample_registry().and_then(|r| aggregate(&r)).map(|x| x.rate as usize),
    ));

    let _ = l.register(count_cell(
        5,
        OP_AGG,
        sample_red().and_then(|r| aggregate(&r)).map(|x| x.red_len),
    ));

    let _ = l.register(count_cell(
        6,
        OP_GATE,
        sample_amber().and_then(|r| aggregate(&r)).map(|x| x.cards),
    ));

    let _ = l.register(count_cell(
        7,
        OP_GATE,
        sample_registry().and_then(|r| aggregate(&r)).map(|x| x.verdict().code() as usize),
    ));

    let _ = l.register(long_cell(
        8,
        OP_CASE,
        Case::new(1, Kind::Equal, 0, true).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        9,
        OP_CASE,
        sample_card(1)
            .and_then(|mut c| {
                let dup = match c.case(0) {
                    Some(v) => v,
                    None => return Err(KT_ESLOT),
                };
                c.push(dup)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        10,
        OP_REG,
        sample_registry()
            .and_then(|mut r| {
                let again = match sample_card(1) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                r.register(again)
            })
            .map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        11,
        OP_REG,
        Card::new(1, &[0x80]).map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        12,
        OP_RUN,
        Registry::new().run_all().map(|_| 0u64),
    ));

    let _ = l.register(long_cell(
        13,
        OP_GATE,
        sample_amber()
            .and_then(|r| aggregate(&r))
            .and_then(|x| x.gate())
            .map(|_| 0u64),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 注册表装载面。
fn registry_ok() -> bool {
    let r = match sample_registry() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.len() == EXPECT_CARDS && r.is_sorted() && !r.is_empty();
    let b = match (r.card(0), r.card(18)) {
        (Some(x), Some(y)) => {
            x.no() == 1
                && y.no() == 19
                && x.name_text() == String::from("tr_ctype")
                && y.name_text() == String::from("applet_form")
        }
        _ => false,
    };
    let c = r.find(7) == Some(6) && r.find(99).is_none() && r.card(19).is_none();
    let d = r.seal() != Registry::new().seal();
    a && b && c && d
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let a = Name::new(b"").err() == Some(KT_ENAME);
    let b = Name::new(&[0x80]).err() == Some(KT_EBYTE);
    let c = Name::new(&[b'q'; NAME_CAP + 1]).err() == Some(KT_ELEN);
    let d = Name::new(&[0x1f]).err() == Some(KT_EBYTE) && Name::new(&[0x7f]).err() == Some(KT_EBYTE);
    let e = match Name::new(b"ktest") {
        Ok(n) => n.len() == 5 && n.same(b"ktest") && !n.same(b"ktes") && n.text() == String::from("ktest"),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 用例闸门。
fn case_ok() -> bool {
    let a = Case::new(0, Kind::Equal, 1, true).err() == Some(KT_EVALUE);
    let b = Case::new(1, Kind::Equal, 0, true).err() == Some(KT_EVALUE);
    let c = match Case::new(2, Kind::Ordered, 3, true) {
        Ok(x) => x.id() == 2 && x.kind() == Kind::Ordered && x.points() == 3 && x.ok(),
        Err(_) => false,
    };
    let d = match Case::new(3, Kind::Rejects, 2, false) {
        Ok(x) => x.points() == 0 && !x.ok() && x.weight() == 2,
        Err(_) => false,
    };
    let e = Kind::Equal.text() == "eq"
        && Kind::NotEqual.text() == "ne"
        && Kind::Ordered.text() == "ord"
        && Kind::Rejects.text() == "err"
        && Kind::Matches.text() == "match";
    let f = Kind::Equal.code() == 1 && Kind::Matches.code() == 5;
    let g = Case::new(1, Kind::Equal, 1, true).map(|x| x.seal()) != Case::new(2, Kind::Equal, 1, true).map(|x| x.seal());
    a && b && c && d && e && f && g
}

/// 卡内用例面。
fn card_push_ok() -> bool {
    let mut c = match sample_card(1) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.len() == 2 && c.passed() == 2 && c.failed() == 0 && c.green();
    let b = c.weight_total() == 3 && c.points() == 3;
    let dup = match c.case(0) {
        Some(v) => v,
        None => return false,
    };
    let d = c.push(dup) == Err(KT_ECASE);
    let mut i = 0usize;
    let mut ok = true;
    while i < 6 {
        let id = (c.len() + 1) as u8;
        match Case::new(id, Kind::Matches, 1, true) {
            Ok(x) => {
                if c.push(x).is_err() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    let e = c.len() == CASE_CAP && c.find(9).is_none();
    let f = match Case::new(9, Kind::Matches, 1, true) {
        Ok(x) => c.push(x) == Err(KT_ELEN),
        Err(_) => false,
    };
    let g = match Card::new(0, b"x") {
        Err(KT_EVALUE) => true,
        _ => false,
    } && match Card::new(1, b"") {
        Err(KT_ENAME) => true,
        _ => false,
    };
    a && b && d && ok && e && f && g
}

/// 注册排序面。
fn reg_ok() -> bool {
    let mut r = Registry::new();
    let a = r.is_empty() && r.len() == 0 && r.run_all() == Err(KT_ESTATE) && r.card(0).is_none();
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
    let b = r.register(c3).is_ok() && r.register(c1).is_ok() && r.register(c2).is_ok();
    let c = r.is_sorted() && r.len() == 3;
    let d = match (r.card(0), r.card(1), r.card(2)) {
        (Some(x), Some(y), Some(z)) => x.no() == 1 && y.no() == 2 && z.no() == 3,
        _ => false,
    };
    let e = match sample_card(1) {
        Ok(v) => r.register(v) == Err(KT_EDUP),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 执行面。
fn run_gate_ok() -> bool {
    let hollow = match Card::new(20, b"hollow_card") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = hollow.run() == Err(KT_EEXEC) && !hollow.green();
    let mut r = Registry::new();
    let b = r.register(hollow).is_ok() && r.run_all() == Err(KT_EEXEC);
    let full = match sample_registry() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = match full.run_all() {
        Ok(t) => t.total() == EXPECT_CASES && t.passed() == EXPECT_CASES && t.failed() == 0 && t.weight() == 63,
        Err(_) => false,
    };
    let e = match sample_card(9) {
        Ok(c) => c.run() == Ok(Tally::of(1, 0, 1)),
        Err(_) => false,
    };
    a && b && d && e
}

/// 汇总累计面。
fn tally_ok() -> bool {
    let t = Tally::new();
    let a = t.total() == 0 && t.rate() == 0 && !t.green(EXPECT_CASES);
    let x = Tally::of(2, 0, 3).add(Tally::of(3, 1, 6));
    let b = x.total() == 6 && x.passed() == 5 && x.failed() == 1 && x.weight() == 9;
    let c = x.rate() == 834;
    let d = !x.green(6) && Tally::of(6, 0, 1).green(6) && !Tally::of(5, 0, 1).green(6);
    let e = Tally::of(0, 0, 0).rate() == 0 && Tally::of(0, 2, 0).rate() == 0;
    let f = Tally::of(1, 0, 1).seal() != Tally::of(0, 1, 1).seal();
    a && b && c && d && e && f
}

/// 通过率面。
fn rate_ok() -> bool {
    let a = Tally::of(38, 0, 0).rate() == 1000;
    let b = Tally::of(37, 1, 0).rate() == 974;
    let c = Tally::of(1, 2, 0).rate() == 334;
    let d = Tally::of(0, 3, 0).rate() == 0 && Tally::of(0, 0, 0).rate() == 0;
    let e = match sample_red().and_then(|r| aggregate(&r)) {
        Ok(x) => x.rate() == 974,
        Err(_) => false,
    };
    let f = match run_all_once() {
        Ok(x) => x.rate() == 1000 && x.cases() == EXPECT_CASES,
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// 聚合报告面。
fn report_ok() -> bool {
    let r = match run_all_once() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.cards() == EXPECT_CARDS && r.cases() == EXPECT_CASES && r.passed() == EXPECT_CASES && r.failed() == 0;
    let b = r.green_cards() == EXPECT_CARDS && r.red_len() == 0 && r.red_text() == String::from("-");
    let c = r.render() == String::from("ktest cards=19 cases=38 pass=38 fail=0 rate=1000ppm green=19 reds=-\n");
    let d = r.green() && r.gate() == Ok(()) && r.verdict() == Verdict::Green && r.verdict_code() == 1;
    let e = r.verdict_text() == "GREEN" && r.seal() != Report::new().seal();
    a && b && c && d && e
}

/// 红单面。
fn reds_ok() -> bool {
    let r = match sample_red().and_then(|x| aggregate(&x)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.cards() == EXPECT_CARDS && r.cases() == EXPECT_CASES && r.passed() == 37 && r.failed() == 1;
    let b = r.rate() == 974 && r.green_cards() == 18;
    let c = r.red_len() == 1 && r.red(0) == Some(7) && r.red(1).is_none() && r.red_text() == String::from("7");
    let d = !r.green() && r.gate() == Err(KT_EVER) && r.verdict() == Verdict::Red && r.verdict_code() == 3;
    let e = r.render() == String::from("ktest cards=19 cases=38 pass=37 fail=1 rate=974ppm green=18 reds=7\n");
    a && b && c && d && e
}

/// 覆盖档面。
fn cover_ok() -> bool {
    let full = match sample_registry() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = full.coverage() == Ok(());
    let amb = match sample_amber() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = amb.coverage() == Err(KT_ENOBS) && amb.len() == 17;
    let r = match aggregate(&amb) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = r.cards() == 17 && r.cases() == 35 && r.passed() == 35 && r.failed() == 0 && r.rate() == 1000;
    let d = r.verdict() == Verdict::Amber && r.verdict_code() == 2 && r.gate() == Err(KT_EVER) && !r.green();
    let e = !Verdict::Green.bad() && Verdict::Amber.bad() && Verdict::Red.bad();
    let f = Verdict::Green.text() == "GREEN" && Verdict::Amber.text() == "AMBER" && Verdict::Red.text() == "RED";
    a && b && c && d && e && f
}

/// 容量与空表面。
fn bounds_ok() -> bool {
    let e = Report::new();
    let a = e.cards() == 0 && e.cases() == 0 && e.passed() == 0 && e.failed() == 0 && e.rate() == 0 && !e.green();
    let b = e.render() == String::from("ktest cards=0 cases=0 pass=0 fail=0 rate=0ppm green=0 reds=-\n");
    let c = e.red_text() == String::from("-") && e.red(0).is_none() && e.verdict() == Verdict::Amber;
    let d = aggregate(&Registry::new()) == Err(KT_ESTATE);
    let f = match sample_card(0) {
        Err(KT_EVALUE) => true,
        _ => false,
    } && match sample_card(20) {
        Err(KT_EVALUE) => true,
        _ => false,
    };
    let g = match sample_card(19) {
        Ok(c) => c.len() == 2 && c.name_text() == String::from("applet_form"),
        Err(_) => false,
    };
    let h = Registry::new().coverage() == Err(KT_ENOBS);
    a && b && c && d && f && g && h
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = registry_ok() && name_ok();
    out[1] = case_ok() && card_push_ok() && reg_ok();
    out[2] = run_gate_ok() && tally_ok() && rate_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && bounds_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11580_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11580.ktest_agg");

    s.add(
        "KG01-注册表装载",
        {
            registry_ok()
        },
        "static",
    );

    s.add(
        "KG02-名称与字节",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "KG03-用例闸门",
        {
            case_ok()
        },
        "static",
    );

    s.add(
        "KG04-卡内用例",
        {
            card_push_ok()
        },
        "static",
    );

    s.add(
        "KG05-注册排序",
        {
            reg_ok()
        },
        "static",
    );

    s.add(
        "KG06-执行闸门",
        {
            run_gate_ok()
        },
        "static",
    );

    s.add(
        "KG07-汇总累计",
        {
            tally_ok()
        },
        "static",
    );

    s.add(
        "KG08-通过率",
        {
            rate_ok()
        },
        "static",
    );

    s.add(
        "KG09-聚合报告",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "KG10-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "KG11-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "KG12-容量与空表",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "KG13-冻结重放四注入",
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
    fn registry_shape() {
        let r = sample_registry().expect("r");
        assert_eq!(r.len(), EXPECT_CARDS);
        assert!(r.is_sorted());
        assert_eq!(r.find(1), Some(0));
        assert_eq!(r.find(19), Some(18));
        assert_eq!(r.find(20), None);
        assert_eq!(r.card(0).map(|c| c.name_text()), Some(String::from("tr_ctype")));
        assert_eq!(r.card(18).map(|c| c.name_text()), Some(String::from("applet_form")));
    }

    #[test]
    fn card_gates() {
        assert_eq!(Card::new(0, b"x").err(), Some(KT_EVALUE));
        assert_eq!(Card::new(1, b"").err(), Some(KT_ENAME));
        assert_eq!(Card::new(1, &[0x80]).err(), Some(KT_EBYTE));
        let c = sample_card(1).expect("c");
        assert_eq!(c.len(), 2);
        assert_eq!(c.weight_total(), 3);
        assert_eq!(c.points(), 3);
        assert!(c.green());
    }

    #[test]
    fn case_gates() {
        assert_eq!(Case::new(1, Kind::Equal, 0, true).err(), Some(KT_EVALUE));
        assert_eq!(Case::new(0, Kind::Equal, 1, true).err(), Some(KT_EVALUE));
        let c = Case::new(2, Kind::NotEqual, 3, false).expect("c");
        assert_eq!(c.points(), 0);
        assert_eq!(c.kind().text(), "ne");
    }

    #[test]
    fn card_push() {
        let mut c = sample_card(1).expect("c");
        let dup = c.case(0).expect("dup");
        assert_eq!(c.push(dup), Err(KT_ECASE));
        while c.len() < CASE_CAP {
            let id = (c.len() + 1) as u8;
            c.push(Case::new(id, Kind::Matches, 1, true).expect("x")).expect("p");
        }
        assert_eq!(c.len(), CASE_CAP);
        assert_eq!(c.push(Case::new(9, Kind::Matches, 1, true).expect("y")), Err(KT_ELEN));
    }

    #[test]
    fn registry_insert() {
        let mut r = Registry::new();
        r.register(sample_card(3).expect("3")).expect("r3");
        r.register(sample_card(1).expect("1")).expect("r1");
        assert!(r.is_sorted());
        assert_eq!(r.card(0).map(|c| c.no()), Some(1));
        assert_eq!(r.card(1).map(|c| c.no()), Some(3));
        assert_eq!(r.register(sample_card(1).expect("1b")), Err(KT_EDUP));
    }

    #[test]
    fn run_gates() {
        let hollow = Card::new(20, b"hollow_card").expect("h");
        assert_eq!(hollow.run(), Err(KT_EEXEC));
        assert!(!hollow.green());
        let mut r = Registry::new();
        assert_eq!(r.run_all(), Err(KT_ESTATE));
        r.register(hollow).expect("reg");
        assert_eq!(r.run_all(), Err(KT_EEXEC));
        let t = sample_registry().expect("full").run_all().expect("run");
        assert_eq!(t.total(), EXPECT_CASES);
        assert_eq!(t.weight(), 63);
        assert!(t.green(EXPECT_CASES));
    }

    #[test]
    fn tally_sum() {
        let x = Tally::of(2, 0, 3).add(Tally::of(3, 1, 6));
        assert_eq!(x.total(), 6);
        assert_eq!(x.passed(), 5);
        assert_eq!(x.failed(), 1);
        assert_eq!(x.weight(), 9);
        assert_eq!(x.rate(), 834);
        assert!(!x.green(6));
        assert_eq!(Tally::new().rate(), 0);
    }

    #[test]
    fn rate_ceil() {
        assert_eq!(Tally::of(38, 0, 0).rate(), 1000);
        assert_eq!(Tally::of(37, 1, 0).rate(), 974);
        assert_eq!(Tally::of(1, 2, 0).rate(), 334);
        assert_eq!(Tally::of(0, 3, 0).rate(), 0);
    }

    #[test]
    fn report_render() {
        let r = run_all_once().expect("r");
        assert_eq!(r.cards(), 19);
        assert_eq!(r.cases(), 38);
        assert_eq!(r.passed(), 38);
        assert_eq!(r.failed(), 0);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.green_cards(), 19);
        assert_eq!(
            r.render().as_str(),
            "ktest cards=19 cases=38 pass=38 fail=0 rate=1000ppm green=19 reds=-\n"
        );
        assert_eq!(r.verdict(), Verdict::Green);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn red_report() {
        let r = aggregate(&sample_red().expect("red")).expect("agg");
        assert_eq!(r.failed(), 1);
        assert_eq!(r.passed(), 37);
        assert_eq!(r.rate(), 974);
        assert_eq!(r.green_cards(), 18);
        assert_eq!(r.red_len(), 1);
        assert_eq!(r.red(0), Some(7));
        assert_eq!(r.red_text(), String::from("7"));
        assert_eq!(r.verdict(), Verdict::Red);
        assert_eq!(r.gate(), Err(KT_EVER));
        assert_eq!(
            r.render().as_str(),
            "ktest cards=19 cases=38 pass=37 fail=1 rate=974ppm green=18 reds=7\n"
        );
    }

    #[test]
    fn amber_cover() {
        let amb = sample_amber().expect("amb");
        assert_eq!(amb.len(), 17);
        assert_eq!(amb.coverage(), Err(KT_ENOBS));
        let r = aggregate(&amb).expect("agg");
        assert_eq!(r.cards(), 17);
        assert_eq!(r.cases(), 35);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.verdict(), Verdict::Amber);
        assert_eq!(r.gate(), Err(KT_EVER));
        assert_eq!(sample_registry().expect("full").coverage(), Ok(()));
    }

    #[test]
    fn empty_report() {
        let e = Report::new();
        assert_eq!(e.cards(), 0);
        assert_eq!(e.rate(), 0);
        assert_eq!(
            e.render().as_str(),
            "ktest cards=0 cases=0 pass=0 fail=0 rate=0ppm green=0 reds=-\n"
        );
        assert_eq!(e.red_text(), String::from("-"));
        assert_eq!(e.red(0), None);
        assert_eq!(aggregate(&Registry::new()), Err(KT_ESTATE));
    }

    #[test]
    fn verdict_text() {
        assert_eq!(Verdict::Green.text(), "GREEN");
        assert_eq!(Verdict::Amber.text(), "AMBER");
        assert_eq!(Verdict::Red.text(), "RED");
        assert!(!Verdict::Green.bad());
        assert!(Verdict::Amber.bad());
        assert!(Verdict::Red.bad());
        assert_eq!(Verdict::Green.code(), 1);
        assert_eq!(Verdict::Red.code(), 3);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(19));
        assert_eq!(l.sample(4).map(|s| s.value), Some(1000));
        assert_eq!(l.sample(8).map(|s| s.code), Some(KT_EVALUE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(KT_EVER));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(KT_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11580_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
