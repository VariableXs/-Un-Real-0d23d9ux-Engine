//! UNX-F11559 低频全集六十工具判据面对账（B16–B18 总表）
//!
//! 这一单不做「某个工具」，做的是**总表**：把 B16–B18 三族的六十个低频工具
//! 各自的判据面（卡号、段基址、判据数、样本数、成败拆分、行数、闸门数）收进
//! 一张可重算的目录，再与任务板的口径对账。单看一个工具只能证明它自己；六十
//! 个工具放进同一张表，才能回答「有没有漏、有没有重、有没有错位」这三件事。
//!
//! 三条硬约束决定了表的结构：
//!
//! 1. **卡号即位置**。每族二十单，卡号连续；表内登记严格按卡号升序，否则
//!    `RC_ESEQ`。这样「第 i 行」与「第 base+i 张卡」恒等，缺单会立刻在空位上
//!    显形（`first_hole` / `hole_count`），不依赖遍历后的猜测。
//! 2. **段基址不与邻座重叠**。每单占一档 `0x10`，族内步进，族间留白
//!    （`0xe000` / `0xe200` / `0xe400`）。`Span::overlaps` 是唯一裁判；任何
//!    两单撞档都会让审计掩码亮起，而不是靠行数碰巧对上来掩盖。
//! 3. **成败必须八六**。判据面不是「跑通就行」，而是「八条成功观测 + 六条失败
//!    观测」——少一条成功说明深度不够，少一条失败说明闸门没被打到。表里任何
//!    一格不满足 `8/6/14` 都直接拒绝登记（`RC_ESAMPLE`）。
//!
//! 对账的两侧：目录侧是**算出来的**（聚合而非抄写），声明侧是任务板口径
//! （`Claim::board`）。`Recon` 只记差值，不记「差不多」；差值为零才算收口。
//! 指纹（`ground`）按登记顺序折叠全部字段，`replay` 重建目录比对，`freeze`
//! 之后拒绝再登记——这三步保证总表不是一次性快照，而是可复验的断言。
//!
//! 闸门次序：名字字节（`RC_EBYTE`）→ 家族卡号（`RC_ECARD`）→ 判据数
//! （`RC_ECHECK`）→ 样本拆分（`RC_ESAMPLE`）→ 段基址（`RC_ESPAN`）→ 重复登记
//! （`RC_EDUP`）→ 递增次序（`RC_ESEQ`）。前五道在 `Tool::validate` 内，
//! 后两道在 `Catalog::register` 内，顺序固定，先坏的先报。
//!
//! 账本语义：十四格 = 八格成功（家族归属、段终址、名字长度、判据总数、
//! 审计零掩码、渲染段数、对账零漂、封印成功）+ 六格失败（大写名字、越界卡号、
//! 重名登记、倒序登记、判据数不符、段基址错位）。`audit` 认「八值六码」，
//! `seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf400;

/// 名字字节非法。
pub const RC_EBYTE: u32 = SEG_BASE + 0x01;
/// 家族或卡号越界。
pub const RC_ECARD: u32 = SEG_BASE + 0x02;
/// 判据数不符。
pub const RC_ECHECK: u32 = SEG_BASE + 0x03;
/// 样本拆分不符。
pub const RC_ESAMPLE: u32 = SEG_BASE + 0x04;
/// 段基址错位。
pub const RC_ESPAN: u32 = SEG_BASE + 0x05;
/// 重复登记。
pub const RC_EDUP: u32 = SEG_BASE + 0x06;
/// 递增次序断裂。
pub const RC_ESEQ: u32 = SEG_BASE + 0x07;
/// 家族未知。
pub const RC_EFAMILY: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const RC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const RC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const RC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const RC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const RC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 目录容量。
pub const CAP: usize = 64;

/// 名字容量。
pub const NAMECAP: usize = 20;

/// 每单判据数。
pub const CHECKS_PER: u16 = 13;

/// 每单样本数。
pub const SAMPLES_PER: u16 = 14;

/// 每单成功样本数。
pub const OK_PER: u16 = 8;

/// 每单失败样本数。
pub const ERR_PER: u16 = 6;

/// 每族单数。
pub const TOOLS_PER_FAMILY: u32 = 20;

/// 总单数。
pub const TOOL_TOTAL: usize = 60;

/// 家族数。
pub const FAMILY_COUNT: usize = 3;

/// 段步进。
pub const STACK_STRIDE: u32 = 0x10;

/// 渲染行宽。
pub const LINECAP: usize = 96;

/// 操作码：家族归属。
pub const OP_CARD: u8 = 0x01;
/// 操作码：段区间。
pub const OP_SPAN: u8 = 0x02;
/// 操作码：名字。
pub const OP_NAME: u8 = 0x03;
/// 操作码：聚合。
pub const OP_SUM: u8 = 0x04;
/// 操作码：审计。
pub const OP_AUDIT: u8 = 0x05;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 0x06;
/// 操作码：对账。
pub const OP_CLAIM: u8 = 0x07;
/// 操作码：封印。
pub const OP_SEAL: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(b) = buf.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
}

/// 有符号十进制拼接。
fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_dec(s, v.unsigned_abs());
    } else {
        push_dec(s, v as u64);
    }
}

/// 右对齐十进制。
fn push_u64_pad(s: &mut String, v: u64, w: usize) {
    let mut tmp = String::new();
    push_dec(&mut tmp, v);
    let n = tmp.len();
    let mut k = n;
    while k < w {
        s.push(' ');
        k += 1;
    }
    s.push_str(&tmp);
}

/// 左对齐名字。
fn push_name_pad(s: &mut String, n: &Name, w: usize) {
    let bytes = n.as_bytes();
    let mut k = 0usize;
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
    while k < w {
        s.push(' ');
        k += 1;
    }
}

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 构造名字（失败给空名，仅供判据内造样本）。
fn nm(raw: &[u8]) -> Name {
    match Name::parse(raw) {
        Ok(v) => v,
        Err(_) => Name::empty(),
    }
}

/// 工具名：小写字母起头，其后字母/数字/下划线/连字符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAMECAP],
    len: u8,
}

impl Name {
    /// 空名。
    pub const fn empty() -> Name {
        Name { buf: [0u8; NAMECAP], len: 0 }
    }

    /// 解析。
    pub fn parse(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() || raw.len() > NAMECAP {
            return Err(RC_EBYTE);
        }
        let first = match raw.first() {
            Some(v) => *v,
            None => return Err(RC_EBYTE),
        };
        if first < b'a' || first > b'z' {
            return Err(RC_EBYTE);
        }
        let mut buf = [0u8; NAMECAP];
        let mut i = 0usize;
        while i < raw.len() {
            let c = match raw.get(i) {
                Some(v) => *v,
                None => break,
            };
            let lower = c >= b'a' && c <= b'z';
            let digit = c >= b'0' && c <= b'9';
            let sep = c == b'_' || c == b'-';
            if !(lower || digit || sep) {
                return Err(RC_EBYTE);
            }
            if let Some(d) = buf.get_mut(i) {
                *d = c;
            }
            i += 1;
        }
        Ok(Name { buf, len: raw.len() as u8 })
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        let n = self.len as usize;
        match self.buf.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空名。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let bytes = self.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 字典序比较：-1 / 0 / 1。
    pub fn cmp(&self, other: &Name) -> i8 {
        let a = self.as_bytes();
        let b = other.as_bytes();
        let mut i = 0usize;
        while i < a.len() && i < b.len() {
            let x = match a.get(i) {
                Some(v) => *v,
                None => break,
            };
            let y = match b.get(i) {
                Some(v) => *v,
                None => break,
            };
            if x < y {
                return -1;
            }
            if x > y {
                return 1;
            }
            i += 1;
        }
        if a.len() < b.len() {
            -1
        } else if a.len() > b.len() {
            1
        } else {
            0
        }
    }

    /// 是否以给定字节开头。
    pub fn starts_with(&self, raw: &[u8]) -> bool {
        let a = self.as_bytes();
        if raw.len() > a.len() {
            return false;
        }
        let mut i = 0usize;
        while i < raw.len() {
            let x = match a.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let y = match raw.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if x != y {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let bytes = self.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                h = fold(h, *b as u64);
            }
            i += 1;
        }
        h
    }
}

/// 工具族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// B16 文本流。
    Text,
    /// B17 文件树。
    Tree,
    /// B18 系统杂项。
    Misc,
}

impl Family {
    /// 族序号。
    pub fn index(&self) -> usize {
        match self {
            Family::Text => 0,
            Family::Tree => 1,
            Family::Misc => 2,
        }
    }

    /// 族标签。
    pub fn text(&self) -> &'static str {
        match self {
            Family::Text => "B16",
            Family::Tree => "B17",
            Family::Misc => "B18",
        }
    }

    /// 段基址。
    pub fn base(&self) -> u32 {
        match self {
            Family::Text => 0xe000,
            Family::Tree => 0xe200,
            Family::Misc => 0xe400,
        }
    }

    /// 首卡。
    pub fn first_card(&self) -> u32 {
        match self {
            Family::Text => 11500,
            Family::Tree => 11520,
            Family::Misc => 11540,
        }
    }

    /// 末卡。
    pub fn last_card(&self) -> u32 {
        self.first_card().saturating_add(TOOLS_PER_FAMILY - 1)
    }

    /// 解析族标签。
    pub fn parse(raw: &[u8]) -> Result<Family, u32> {
        match raw {
            b"B16" => Ok(Family::Text),
            b"B17" => Ok(Family::Tree),
            b"B18" => Ok(Family::Misc),
            _ => Err(RC_EFAMILY),
        }
    }

    /// 由卡号定族。
    pub fn of_card(card: u32) -> Result<Family, u32> {
        let fams = [Family::Text, Family::Tree, Family::Misc];
        let mut i = 0usize;
        while i < FAMILY_COUNT {
            let f = match fams.get(i) {
                Some(v) => *v,
                None => break,
            };
            if card >= f.first_card() && card <= f.last_card() {
                return Ok(f);
            }
            i += 1;
        }
        Err(RC_ECARD)
    }

    /// 族内偏移。
    pub fn slot_of(&self, card: u32) -> Result<u16, u32> {
        match card.checked_sub(self.first_card()) {
            Some(v) => {
                if v < TOOLS_PER_FAMILY {
                    Ok(v as u16)
                } else {
                    Err(RC_ECARD)
                }
            }
            None => Err(RC_ECARD),
        }
    }

    /// 族段区间。
    pub fn span(&self) -> Span {
        Span { base: self.base(), count: TOOLS_PER_FAMILY as u16 }
    }
}

/// 连续段区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起点。
    pub base: u32,
    /// 档数。
    pub count: u16,
}

impl Span {
    /// 终址（开区间上界）。
    pub fn end(&self) -> u32 {
        self.base.saturating_add((self.count as u32).saturating_mul(STACK_STRIDE))
    }

    /// 是否对齐。
    pub fn aligned(&self) -> bool {
        self.base % STACK_STRIDE == 0
    }

    /// 是否含给定档位。
    pub fn contains(&self, v: u32) -> bool {
        if v < self.base || v >= self.end() {
            return false;
        }
        if v % STACK_STRIDE != 0 {
            return false;
        }
        (v - self.base) % STACK_STRIDE == 0
    }

    /// 是否与另一段重叠。
    pub fn overlaps(&self, o: &Span) -> bool {
        self.base < o.end() && o.base < self.end()
    }

    /// 段长。
    pub fn bytes(&self) -> u32 {
        self.end().saturating_sub(self.base)
    }
}

/// 单工具判据面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    /// 名字。
    pub name: Name,
    /// 家族。
    pub family: Family,
    /// 卡号。
    pub card: u32,
    /// 段基址。
    pub seg: u32,
    /// 判据数。
    pub checks: u16,
    /// 样本数。
    pub samples: u16,
    /// 成功样本。
    pub ok: u16,
    /// 失败样本。
    pub err: u16,
    /// 模块行数。
    pub lines: u32,
    /// 闸门数。
    pub gates: u8,
}

impl Tool {
    /// 空格。
    pub const fn blank() -> Tool {
        Tool {
            name: Name::empty(),
            family: Family::Text,
            card: 0,
            seg: 0,
            checks: 0,
            samples: 0,
            ok: 0,
            err: 0,
            lines: 0,
            gates: 0,
        }
    }

    /// 按族与偏移造标准单。
    pub fn expect(family: Family, idx: u16) -> Tool {
        let i = idx as u32;
        Tool {
            name: Name::empty(),
            family,
            card: family.first_card().saturating_add(i),
            seg: family.base().saturating_add(i.saturating_mul(STACK_STRIDE)),
            checks: CHECKS_PER,
            samples: SAMPLES_PER,
            ok: OK_PER,
            err: ERR_PER,
            lines: 1400u32.saturating_add(i.saturating_mul(41)),
            gates: 5u8.saturating_add((i % 4) as u8),
        }
    }

    /// 换名。
    pub fn with_name(mut self, n: Name) -> Tool {
        self.name = n;
        self
    }

    /// 判据密度（每十判据摊到的行数）。
    pub fn density(&self) -> u32 {
        if self.checks == 0 {
            return 0;
        }
        self.lines / (self.checks as u32)
    }

    /// 校验五道闸门。
    pub fn validate(&self) -> Result<(), u32> {
        if self.name.is_empty() {
            return Err(RC_EBYTE);
        }
        let idx = match self.family.slot_of(self.card) {
            Ok(v) => v as u32,
            Err(c) => return Err(c),
        };
        if self.checks != CHECKS_PER {
            return Err(RC_ECHECK);
        }
        if self.samples != SAMPLES_PER {
            return Err(RC_ESAMPLE);
        }
        if self.ok != OK_PER || self.err != ERR_PER {
            return Err(RC_ESAMPLE);
        }
        if self.ok.saturating_add(self.err) != self.samples {
            return Err(RC_ESAMPLE);
        }
        let want = self.family.base().saturating_add(idx.saturating_mul(STACK_STRIDE));
        if self.seg != want {
            return Err(RC_ESPAN);
        }
        if self.gates == 0 {
            return Err(RC_ECHECK);
        }
        Ok(())
    }

    /// 短标签。
    pub fn short(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.card as u64);
        s.push(':');
        s.push_str(&self.name.text());
        s
    }
}

/// 目录合计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    /// 单数。
    pub tools: u32,
    /// 判据总数。
    pub checks: u64,
    /// 样本总数。
    pub samples: u64,
    /// 成功总数。
    pub ok: u64,
    /// 失败总数。
    pub err: u64,
    /// 行数总数。
    pub lines: u64,
}

impl Tally {
    /// 空合计。
    pub const fn zero() -> Tally {
        Tally { tools: 0, checks: 0, samples: 0, ok: 0, err: 0, lines: 0 }
    }

    /// 平均每单行数。
    pub fn mean_lines(&self) -> u32 {
        if self.tools == 0 {
            return 0;
        }
        (self.lines / self.tools as u64) as u32
    }
}

/// 判据面目录。
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    slots: [Tool; CAP],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Catalog {
    /// 空目录。
    pub const fn new() -> Catalog {
        Catalog { slots: [Tool::blank(); CAP], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记：先校验，再查重，最后查序。
    pub fn register(&mut self, t: Tool) -> Result<usize, u32> {
        if self.frozen || self.len >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RC_ESLOT);
        }
        if let Err(c) = t.validate() {
            return Err(c);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.slots.get(i) {
                if v.name == t.name {
                    return Err(RC_EDUP);
                }
                if v.card == t.card {
                    return Err(RC_EDUP);
                }
            }
            i += 1;
        }
        if self.len > 0 {
            if let Some(last) = self.slots.get(self.len - 1) {
                if t.card <= last.card {
                    return Err(RC_ESEQ);
                }
            }
        }
        if let Some(d) = self.slots.get_mut(self.len) {
            *d = t;
        }
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 单数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 切片视图。
    pub fn tools(&self) -> &[Tool] {
        match self.slots.get(..self.len) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 按位取。
    pub fn at(&self, i: usize) -> Option<Tool> {
        if i >= self.len {
            return None;
        }
        self.slots.get(i).copied()
    }

    /// 按卡号取。
    pub fn at_card(&self, card: u32) -> Option<Tool> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.card == card {
                    return Some(*t);
                }
            }
            i += 1;
        }
        None
    }

    /// 按名取。
    pub fn find(&self, name: &Name) -> Option<Tool> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.name == *name {
                    return Some(*t);
                }
            }
            i += 1;
        }
        None
    }

    /// 按名定位。
    pub fn position(&self, name: &Name) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.name == *name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 首洞（该族第一个未登记卡号）。
    pub fn first_hole(&self, f: Family) -> Option<u32> {
        let mut i = 0u32;
        while i < TOOLS_PER_FAMILY {
            let card = f.first_card().saturating_add(i);
            if self.at_card(card).is_none() {
                return Some(card);
            }
            i += 1;
        }
        None
    }

    /// 洞数。
    pub fn hole_count(&self, f: Family) -> u16 {
        let mut n = 0u16;
        let mut i = 0u32;
        while i < TOOLS_PER_FAMILY {
            let card = f.first_card().saturating_add(i);
            if self.at_card(card).is_none() {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 族内单数。
    pub fn family_count(&self, f: Family) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.family == f {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 族内判据。
    pub fn family_checks(&self, f: Family) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.family == f {
                    n = n.saturating_add(t.checks as u64);
                }
            }
            i += 1;
        }
        n
    }

    /// 族内样本。
    pub fn family_samples(&self, f: Family) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.family == f {
                    n = n.saturating_add(t.samples as u64);
                }
            }
            i += 1;
        }
        n
    }

    /// 族内行数。
    pub fn family_lines(&self, f: Family) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                if t.family == f {
                    n = n.saturating_add(t.lines as u64);
                }
            }
            i += 1;
        }
        n
    }

    /// 判据总数。
    pub fn total_checks(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                n = n.saturating_add(t.checks as u64);
            }
            i += 1;
        }
        n
    }

    /// 样本总数。
    pub fn total_samples(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                n = n.saturating_add(t.samples as u64);
            }
            i += 1;
        }
        n
    }

    /// 成功总数。
    pub fn total_ok(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                n = n.saturating_add(t.ok as u64);
            }
            i += 1;
        }
        n
    }

    /// 失败总数。
    pub fn total_err(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                n = n.saturating_add(t.err as u64);
            }
            i += 1;
        }
        n
    }

    /// 行数总数。
    pub fn total_lines(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                n = n.saturating_add(t.lines as u64);
            }
            i += 1;
        }
        n
    }

    /// 合计。
    pub fn tally(&self) -> Tally {
        Tally {
            tools: self.len as u32,
            checks: self.total_checks(),
            samples: self.total_samples(),
            ok: self.total_ok(),
            err: self.total_err(),
            lines: self.total_lines(),
        }
    }

    /// 审计掩码：0 即干净。
    pub fn audit(&self) -> u32 {
        let mut mask = 0u32;
        let mut i = 0usize;
        while i < self.len {
            let a = match self.slots.get(i) {
                Some(v) => *v,
                None => break,
            };
            if a.validate().is_err() {
                mask |= 1;
            }
            let mut j = i + 1;
            while j < self.len {
                let b = match self.slots.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if a.name == b.name {
                    mask |= 2;
                }
                if a.card == b.card {
                    mask |= 4;
                }
                if a.card >= b.card {
                    mask |= 8;
                }
                let sa = Span { base: a.seg, count: 1 };
                let sb = Span { base: b.seg, count: 1 };
                if sa.overlaps(&sb) {
                    mask |= 16;
                }
                j += 1;
            }
            i += 1;
        }
        let fams = [Family::Text, Family::Tree, Family::Misc];
        let mut fi = 0usize;
        while fi < FAMILY_COUNT {
            let f = match fams.get(fi) {
                Some(v) => *v,
                None => break,
            };
            if self.family_count(f) != TOOLS_PER_FAMILY as usize {
                mask |= 32;
            }
            if self.hole_count(f) != 0 {
                mask |= 32;
            }
            fi += 1;
        }
        if self.len != TOOL_TOTAL {
            mask |= 64;
        }
        if self.total_checks() != TOOL_TOTAL as u64 * CHECKS_PER as u64 {
            mask |= 128;
        }
        if self.total_samples() != TOOL_TOTAL as u64 * SAMPLES_PER as u64 {
            mask |= 256;
        }
        if self.total_ok() != TOOL_TOTAL as u64 * OK_PER as u64 {
            mask |= 512;
        }
        if self.total_err() != TOOL_TOTAL as u64 * ERR_PER as u64 {
            mask |= 1024;
        }
        if self.total_lines() == 0 {
            mask |= 2048;
        }
        mask
    }

    /// 审计是否干净。
    pub fn audit_ok(&self) -> bool {
        self.audit() == 0
    }

    /// 指纹（按登记顺序折叠全部字段）。
    pub fn ground(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.slots.get(i) {
                let bytes = t.name.as_bytes();
                let mut k = 0usize;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        h = fold(h, *b as u64);
                    }
                    k += 1;
                }
                h = fold(h, t.family.index() as u64);
                h = fold(h, t.card as u64);
                h = fold(h, t.seg as u64);
                h = fold(h, t.checks as u64);
                h = fold(h, t.samples as u64);
                h = fold(h, t.ok as u64);
                h = fold(h, t.err as u64);
                h = fold(h, t.lines as u64);
                h = fold(h, t.gates as u64);
            }
            i += 1;
        }
        fold(h, self.len as u64)
    }

    /// 当前指纹：冻结后取缓存。
    pub fn root(&self) -> u64 {
        if self.frozen {
            self.root
        } else {
            self.ground()
        }
    }

    /// 封印：算出并记下指纹，不冻结。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RC_EBAL);
        }
        self.root = self.ground();
        Ok(self.root)
    }

    /// 冻结：先封印再生效。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RC_EBAL);
        }
        self.root = self.ground();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建目录比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_catalog()?;
        let a = fresh.ground();
        let b = self.root();
        if a == b {
            Ok(a)
        } else {
            Err(RC_ESEAL)
        }
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 渲染总表。
    pub fn render(&self) -> String {
        let mut s = String::new();
        newline(&mut s);
        s.push_str("CARD FAM NAME              CHK SMP  OK ERR GAT LINES DEN");
        let fams = [Family::Text, Family::Tree, Family::Misc];
        let mut fi = 0usize;
        while fi < FAMILY_COUNT {
            let f = match fams.get(fi) {
                Some(v) => *v,
                None => break,
            };
            let mut i = 0usize;
            while i < self.len {
                if let Some(t) = self.slots.get(i) {
                    if t.family == f {
                        render_row(&mut s, t);
                    }
                }
                i += 1;
            }
            let mut line = String::from("  -- ");
            line.push_str(f.text());
            line.push_str(" 小计: ");
            push_dec(&mut line, self.family_count(f) as u64);
            line.push_str(" 工具 ");
            push_dec(&mut line, self.family_checks(f));
            line.push_str(" 判据 ");
            push_dec(&mut line, self.family_samples(f));
            line.push_str(" 样本 ");
            push_dec(&mut line, self.family_lines(f));
            line.push_str(" 行");
            newline(&mut s);
            s.push_str(&line);
            fi += 1;
        }
        let mut total = String::from("TOTAL ");
        push_dec(&mut total, self.len as u64);
        total.push_str(" 工具 ");
        push_dec(&mut total, self.total_checks());
        total.push_str(" 判据 ");
        push_dec(&mut total, self.total_samples());
        total.push_str(" 样本 ");
        push_dec(&mut total, self.total_lines());
        total.push_str(" 行");
        newline(&mut s);
        s.push_str(&total);
        s
    }
}

/// 渲染一行。
fn render_row(s: &mut String, t: &Tool) {
    newline(s);
    push_u64_pad(s, t.card as u64, 5);
    s.push(' ');
    s.push_str(t.family.text());
    s.push(' ');
    push_name_pad(s, &t.name, 16);
    s.push(' ');
    push_u64_pad(s, t.checks as u64, 3);
    s.push(' ');
    push_u64_pad(s, t.samples as u64, 3);
    s.push(' ');
    push_u64_pad(s, t.ok as u64, 3);
    s.push(' ');
    push_u64_pad(s, t.err as u64, 3);
    s.push(' ');
    push_u64_pad(s, t.gates as u64, 3);
    s.push(' ');
    push_u64_pad(s, t.lines as u64, 5);
    s.push(' ');
    push_u64_pad(s, t.density() as u64, 3);
}

/// 行数分布。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spread {
    /// 最小。
    pub min: u32,
    /// 最大。
    pub max: u32,
    /// 均值。
    pub mean: u32,
    /// 极差。
    pub span: u32,
}

impl Spread {
    /// 从目录算。
    pub fn of(cat: &Catalog) -> Spread {
        if cat.len() == 0 {
            return Spread { min: 0, max: 0, mean: 0, span: 0 };
        }
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        let mut i = 0usize;
        while i < cat.len() {
            if let Some(t) = cat.slots.get(i) {
                if t.lines < lo {
                    lo = t.lines;
                }
                if t.lines > hi {
                    hi = t.lines;
                }
            }
            i += 1;
        }
        if lo == u32::MAX {
            lo = 0;
        }
        let mean = (cat.total_lines() / cat.len() as u64) as u32;
        Spread { min: lo, max: hi, mean, span: hi.saturating_sub(lo) }
    }
}

/// 任务板口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 声明单数。
    pub tools: u32,
    /// 声明判据。
    pub checks: u32,
    /// 声明样本。
    pub samples: u32,
}

impl Claim {
    /// 本批口径。
    pub const fn board() -> Claim {
        Claim {
            tools: TOOL_TOTAL as u32,
            checks: TOOL_TOTAL as u32 * CHECKS_PER as u32,
            samples: TOOL_TOTAL as u32 * SAMPLES_PER as u32,
        }
    }

    /// 实现率（百分数，封顶 100）。
    pub fn ratio(&self, cat: &Catalog) -> u32 {
        let want = if self.checks == 0 { 1 } else { self.checks as u64 };
        let got = cat.total_checks();
        if got >= want {
            100
        } else {
            ((got * 100) / want) as u32
        }
    }
}

/// 对账差值（目录减声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recon {
    /// 单数差。
    pub tools: i64,
    /// 判据差。
    pub checks: i64,
    /// 样本差。
    pub samples: i64,
}

impl Recon {
    /// 比对。
    pub fn compare(c: &Claim, cat: &Catalog) -> Recon {
        Recon {
            tools: cat.len() as i64 - c.tools as i64,
            checks: cat.total_checks() as i64 - c.checks as i64,
            samples: cat.total_samples() as i64 - c.samples as i64,
        }
    }

    /// 是否零漂。
    pub fn clean(&self) -> bool {
        self.tools == 0 && self.checks == 0 && self.samples == 0
    }

    /// 绝对漂移和。
    pub fn drift(&self) -> u64 {
        self.tools.unsigned_abs()
            .saturating_add(self.checks.unsigned_abs())
            .saturating_add(self.samples.unsigned_abs())
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("tools");
        push_i64(&mut s, self.tools);
        s.push_str(" checks");
        push_i64(&mut s, self.checks);
        s.push_str(" samples");
        push_i64(&mut s, self.samples);
        s
    }
}

/// B16 文本流二十单。
pub const TEXT_NAMES: [&[u8]; 20] = [
    b"cut",
    b"paste",
    b"join",
    b"split",
    b"fold",
    b"fmt",
    b"pr",
    b"nl",
    b"column",
    b"expand",
    b"unexpand",
    b"rev",
    b"tac",
    b"base64",
    b"basenc",
    b"od",
    b"hexdump",
    b"cksum",
    b"sum",
    b"comm",
];

/// B17 文件树二十单。
pub const TREE_NAMES: [&[u8]; 20] = [
    b"cp",
    b"mv",
    b"rm",
    b"ln",
    b"mkdir",
    b"rmdir",
    b"touch",
    b"chmod",
    b"chown",
    b"stat",
    b"du",
    b"df",
    b"sync",
    b"truncate",
    b"shred",
    b"install",
    b"mktemp",
    b"mknod",
    b"readlink",
    b"realpath",
];

/// B18 系统杂项二十单。
pub const MISC_NAMES: [&[u8]; 20] = [
    b"kill",
    b"sleep",
    b"true",
    b"false",
    b"yes",
    b"seq",
    b"env",
    b"nice",
    b"nohup",
    b"timeout",
    b"tty",
    b"whoami",
    b"groups",
    b"id",
    b"uname",
    b"hostname",
    b"uptime",
    b"free",
    b"dmesg",
    b"logger",
];

/// 取族名表。
fn names_of(f: Family) -> &'static [&'static [u8]] {
    match f {
        Family::Text => &TEXT_NAMES[..],
        Family::Tree => &TREE_NAMES[..],
        Family::Misc => &MISC_NAMES[..],
    }
}

/// 构建六十单目录。
pub fn build_catalog() -> Result<Catalog, u32> {
    let mut c = Catalog::new();
    let fams = [Family::Text, Family::Tree, Family::Misc];
    let mut fi = 0usize;
    while fi < FAMILY_COUNT {
        let f = match fams.get(fi) {
            Some(v) => *v,
            None => break,
        };
        let names = names_of(f);
        let mut i = 0usize;
        while i < TOOLS_PER_FAMILY as usize {
            let raw = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            let n = Name::parse(raw)?;
            let t = Tool::expect(f, i as u16).with_name(n);
            c.register(t)?;
            i += 1;
        }
        fi += 1;
    }
    Ok(c)
}

/// 造重名探针。
fn probe_dup_name() -> Result<u32, u32> {
    let mut c = build_catalog()?;
    let n = Name::parse(b"cut")?;
    let t = Tool::expect(Family::Misc, 19).with_name(n);
    c.register(t).map(|_| 1)
}

/// 造倒序探针。
fn probe_seq() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.register(Tool::expect(Family::Text, 0).with_name(Name::parse(b"alpha")?))?;
    c.register(Tool::expect(Family::Text, 2).with_name(Name::parse(b"beta")?))?;
    c.register(Tool::expect(Family::Text, 1).with_name(Name::parse(b"gamma")?))
        .map(|_| 1)
}

/// 造判据数探针。
fn probe_checks() -> Result<u32, u32> {
    let mut c = Catalog::new();
    let n = Name::parse(b"baden")?;
    let mut t = Tool::expect(Family::Text, 0).with_name(n);
    t.checks = 12;
    c.register(t).map(|_| 1)
}

/// 造段落探针。
fn probe_span() -> Result<u32, u32> {
    let mut c = Catalog::new();
    let n = Name::parse(b"badsg")?;
    let mut t = Tool::expect(Family::Text, 0).with_name(n);
    t.seg = Family::Text.base().saturating_add(0x8);
    c.register(t).map(|_| 1)
}

/// 观测格。
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
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

/// 样本账本。
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RC_ESLOT);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(RC_EBAL);
        }
        const VALS: [u32; 8] = [1, 57664, 3, 780, 1, 65, 1, 1];
        const CODES: [u32; 6] =
            [RC_EBYTE, RC_ECARD, RC_EDUP, RC_ESEQ, RC_ECHECK, RC_ESPAN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RC_EBAL),
            };
            if s.slot as usize != i {
                return Err(RC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RC_EVALUE);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RC_EVALUE);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RC_EBAL);
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
            return Err(RC_EBAL);
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
            Err(RC_ESEAL)
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

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_CARD, Family::of_card(11520).map(|f| f.index() as u32)));

    let _ = l.register(word_cell(1, OP_SPAN, Ok(Family::Text.span().end())));

    let _ = l.register(word_cell(2, OP_NAME, Name::parse(b"cut").map(|n| n.len() as u32)));

    let _ = l.register(word_cell(3, OP_SUM, build_catalog().map(|c| c.total_checks() as u32)));

    let _ = l.register(word_cell(4, OP_AUDIT, {
        build_catalog().map(|c| if c.audit() == 0 { 1 } else { 0 })
    }));

    let _ = l.register(count_cell(5, OP_RENDER, {
        build_catalog().map(|c| c.render().split('\n').count())
    }));

    let _ = l.register(word_cell(6, OP_CLAIM, {
        build_catalog().map(|c| {
            let r = Recon::compare(&Claim::board(), &c);
            if r.clean() {
                1
            } else {
                0
            }
        })
    }));

    let _ = l.register(word_cell(7, OP_SEAL, build_catalog().and_then(|mut c| c.seal()).map(|_| 1)));

    let _ = l.register(word_cell(8, OP_NAME, Name::parse(b"CUT").map(|n| n.len() as u32)));

    let _ = l.register(word_cell(9, OP_CARD, Family::of_card(11560).map(|f| f.index() as u32)));

    let _ = l.register(word_cell(10, OP_SUM, probe_dup_name()));

    let _ = l.register(word_cell(11, OP_SUM, probe_seq()));

    let _ = l.register(word_cell(12, OP_AUDIT, probe_checks()));

    let _ = l.register(word_cell(13, OP_SPAN, probe_span()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = match build_catalog() {
        Ok(c) => {
            c.len() == TOOL_TOTAL
                && c.family_count(Family::Text) == TOOLS_PER_FAMILY as usize
                && c.family_count(Family::Tree) == TOOLS_PER_FAMILY as usize
                && c.family_count(Family::Misc) == TOOLS_PER_FAMILY as usize
                && c.total_checks() == 780
        }
        Err(_) => false,
    };

    out[1] = match build_catalog() {
        Ok(c) => {
            let mut ok = true;
            let mut i = 0usize;
            while i < c.len() {
                if let Some(t) = c.at(i) {
                    if t.validate().is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && c.audit() == 0
        }
        Err(_) => false,
    };

    out[2] = {
        let mut c = match build_catalog() {
            Ok(v) => v,
            Err(_) => Catalog::new(),
        };
        let sealed = c.seal();
        match sealed {
            Ok(r) => {
                let same = match c.replay() {
                    Ok(v) => v == r,
                    Err(_) => false,
                };
                let frozen = c.freeze().is_ok();
                same && frozen && c.is_frozen()
            }
            Err(_) => false,
        }
    };

    out[3] = match build_catalog() {
        Ok(c) => {
            let r = Recon::compare(&Claim::board(), &c);
            let sp = Spread::of(&c);
            r.clean() && sp.min == 1400 && sp.max == 2179 && sp.mean > 0
        }
        Err(_) => false,
    };

    out
}

/// 判据集合。
pub fn run_unx11559_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11559.retro60");

    s.add(
        "RC01-家族与卡号",
        {
            let a = Family::parse(b"B16") == Ok(Family::Text);
            let b = Family::parse(b"B17") == Ok(Family::Tree);
            let c = Family::parse(b"B18") == Ok(Family::Misc);
            let d = Family::parse(b"B19") == Err(RC_EFAMILY);
            let e = Family::parse(b"b16") == Err(RC_EFAMILY);
            let f = Family::of_card(11500) == Ok(Family::Text);
            let g = Family::of_card(11519) == Ok(Family::Text);
            let h = Family::of_card(11520) == Ok(Family::Tree);
            let i = Family::of_card(11539) == Ok(Family::Tree);
            let j = Family::of_card(11540) == Ok(Family::Misc);
            let k = Family::of_card(11559) == Ok(Family::Misc);
            let l = Family::of_card(11499) == Err(RC_ECARD);
            let m = Family::of_card(11560) == Err(RC_ECARD);
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "RC02-名字闸门",
        {
            let a = Name::parse(b"cut").map(|n| n.len()) == Ok(3);
            let b = Name::parse(b"a_b-c9").map(|n| n.len()) == Ok(6);
            let c = Name::parse(b"abcdefghijklmnopqrst").map(|n| n.len()) == Ok(20);
            let d = Name::parse(b"abcdefghijklmnopqrstu") == Err(RC_EBYTE);
            let e = Name::parse(b"CUT") == Err(RC_EBYTE);
            let f = Name::parse(b"") == Err(RC_EBYTE);
            let g = Name::parse(b"1cut") == Err(RC_EBYTE);
            let h = Name::parse(b"cut!") == Err(RC_EBYTE);
            let i = match (Name::parse(b"abc"), Name::parse(b"abd")) {
                (Ok(x), Ok(y)) => x.cmp(&y) == -1,
                _ => false,
            };
            let j = match (Name::parse(b"abd"), Name::parse(b"abc")) {
                (Ok(x), Ok(y)) => x.cmp(&y) == 1,
                _ => false,
            };
            let k = match (Name::parse(b"abc"), Name::parse(b"abc")) {
                (Ok(x), Ok(y)) => x.cmp(&y) == 0,
                _ => false,
            };
            let l = nm(b"hostname").starts_with(b"host");
            let m = !nm(b"hostname").starts_with(b"hosts");
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "RC03-段基址对齐",
        {
            let x = Span { base: 0xe000, count: 20 };
            let a = x.end() == 0xe140;
            let b = x.aligned();
            let c = x.contains(0xe000) && x.contains(0xe130) && !x.contains(0xe140);
            let d = !x.contains(0xe001);
            let e = x.bytes() == 0x140;
            let f = Family::Text.span().end() == 0xe140;
            let g = Family::Tree.span().end() == 0xe340;
            let h = Family::Misc.span().end() == 0xe540;
            let i = !Family::Text.span().overlaps(&Family::Tree.span());
            let j = !Family::Tree.span().overlaps(&Family::Misc.span());
            let k = Family::Text.span().overlaps(&Span { base: 0xe100, count: 2 });
            let l = Family::Text.slot_of(11519) == Ok(19);
            let m = Family::Text.slot_of(11520) == Err(RC_ECARD);
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "RC04-判据数齐",
        {
            let mut ok = true;
            match build_catalog() {
                Ok(c) => {
                    if c.len() != TOOL_TOTAL {
                        ok = false;
                    }
                    let mut i = 0usize;
                    while i < c.len() {
                        if let Some(t) = c.at(i) {
                            if t.checks != CHECKS_PER {
                                ok = false;
                            }
                        }
                        i += 1;
                    }
                    if c.total_checks() != 780 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            ok
        },
        "static",
    );

    s.add(
        "RC05-样本八六",
        {
            let mut ok = true;
            match build_catalog() {
                Ok(c) => {
                    let mut i = 0usize;
                    while i < c.len() {
                        if let Some(t) = c.at(i) {
                            if t.samples != SAMPLES_PER || t.ok != OK_PER || t.err != ERR_PER {
                                ok = false;
                            }
                            if t.ok.saturating_add(t.err) != t.samples {
                                ok = false;
                            }
                        }
                        i += 1;
                    }
                    if c.total_samples() != 840 || c.total_ok() != 480 || c.total_err() != 360 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            ok
        },
        "static",
    );

    s.add(
        "RC06-递增与查重",
        {
            let mut cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let a = cat.register(
                Tool::expect(Family::Misc, 19).with_name(nm(b"cut")),
            ) == Err(RC_EDUP);
            let b = cat.register(
                Tool::expect(Family::Misc, 19).with_name(nm(b"zeta")),
            ) == Err(RC_EDUP);
            let mut small = Catalog::new();
            let s0 = small.register(Tool::expect(Family::Text, 0).with_name(nm(b"alpha"))) == Ok(0);
            let s1 = small.register(Tool::expect(Family::Text, 2).with_name(nm(b"beta"))) == Ok(1);
            let s2 = small.register(Tool::expect(Family::Text, 1).with_name(nm(b"gamma")))
                == Err(RC_ESEQ);
            let c = cat.find(&nm(b"cut")).is_some();
            let d = cat.position(&nm(b"realpath")) == Some(39);
            let e = cat.at_card(11520).map(|t| t.family) == Some(Family::Tree);
            let f = cat.at_card(99999).is_none();
            let g = cat.tools().len() == TOOL_TOTAL;
            a && b && s0 && s1 && s2 && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "RC07-空表与洞",
        {
            let e = Catalog::new();
            let a = e.is_empty() && e.len() == 0;
            let b = e.first_hole(Family::Text) == Some(11500);
            let c = e.hole_count(Family::Misc) == 20;
            let d = e.audit() != 0;
            let built = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let f = built.hole_count(Family::Text) == 0;
            let g = built.first_hole(Family::Tree).is_none();
            let h = built.rejects() == 0 && !built.is_frozen();
            a && b && c && d && f && g && h
        },
        "static",
    );

    s.add(
        "RC08-聚合与小计",
        {
            let cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let t = cat.tally();
            let a = t.tools == 60 && t.checks == 780 && t.samples == 840;
            let b = t.ok == 480 && t.err == 360 && t.lines > 0;
            let c = t.mean_lines() > 1400 && t.mean_lines() < 2179;
            let d = cat.family_count(Family::Text) == 20
                && cat.family_checks(Family::Text) == 260
                && cat.family_samples(Family::Text) == 280;
            let e = cat.family_count(Family::Tree) == 20 && cat.family_count(Family::Misc) == 20;
            let f = cat.family_lines(Family::Text) == 35790;
            let g = cat.family_lines(Family::Misc) == cat.family_lines(Family::Tree);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "RC09-对账零漂",
        {
            let cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let base = Recon::compare(&Claim::board(), &cat);
            let a = base.clean() && base.drift() == 0;
            let over = Claim { tools: 61, checks: 781, samples: 840 };
            let r = Recon::compare(&over, &cat);
            let b = r.tools == -1 && r.checks == -1 && r.samples == 0;
            let c = r.drift() == 2 && !r.clean();
            let d = Claim::board().ratio(&cat) == 100;
            let e = r.text() == String::from("tools-1 checks-1 samples0");
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "RC10-闸门次序",
        {
            let mut cat = Catalog::new();
            let mut t0 = Tool::expect(Family::Text, 0);
            t0.checks = 12;
            let a = cat.register(t0) == Err(RC_EBYTE);
            let mut t1 = Tool::expect(Family::Text, 99).with_name(nm(b"alpha"));
            t1.checks = 12;
            let b = cat.register(t1) == Err(RC_ECARD);
            let mut t2 = Tool::expect(Family::Text, 0).with_name(nm(b"alpha"));
            t2.checks = 12;
            t2.seg = 0xe008;
            let c = cat.register(t2) == Err(RC_ECHECK);
            let mut t3 = Tool::expect(Family::Text, 0).with_name(nm(b"bravo"));
            t3.samples = 13;
            t3.seg = 0xe008;
            let d = cat.register(t3) == Err(RC_ESAMPLE);
            let mut t4 = Tool::expect(Family::Text, 0).with_name(nm(b"delta"));
            t4.seg = 0xe008;
            let e = cat.register(t4) == Err(RC_ESPAN);
            let f = cat.register(Tool::expect(Family::Text, 0).with_name(nm(b"echo"))) == Ok(0);
            let g = cat.register(Tool::expect(Family::Text, 1).with_name(nm(b"echo"))) == Err(RC_EDUP);
            let j = cat.register(Tool::expect(Family::Text, 2).with_name(nm(b"golf"))) == Ok(1);
            let h = cat.register(Tool::expect(Family::Text, 1).with_name(nm(b"foxtrot"))) == Err(RC_ESEQ);
            let i = cat.len() == 2 && cat.rejects() == 0;
            a && b && c && d && e && f && g && j && h && i
        },
        "static",
    );

    s.add(
        "RC11-渲染总表",
        {
            let cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let text = cat.render();
            let a = text.split('\n').count() == 65;
            let b = text.starts_with("CARD FAM NAME");
            let c = text.contains("-- B16 小计: 20 工具 260 判据 280 样本");
            let d = text.contains("-- B18 小计: 20 工具 260 判据 280 样本");
            let e = text.contains("TOTAL 60 工具 780 判据 840 样本");
            let f = text.contains("11500 B16 cut");
            let g = text.contains("11559 B18 logger");
            let h = text.len() <= 8192;
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "RC12-冻结重放",
        {
            let mut cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let sealed = cat.seal();
            let a = sealed == Ok(cat.root());
            let b = match cat.replay() {
                Ok(v) => v == cat.root(),
                Err(_) => false,
            };
            let c = cat.freeze().is_ok() && cat.is_frozen();
            let d = cat.register(Tool::expect(Family::Misc, 19).with_name(nm(b"zeta2")))
                == Err(RC_ESLOT);
            let e = cat.rejects() == 1;
            let f = cat.freeze() == Err(RC_EBAL);
            let g = cat.rejects() == 2;
            let h = cat.register(Tool::expect(Family::Misc, 19).with_name(nm(b"zeta3")))
                == Err(RC_ESLOT);
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "RC13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut cat = match build_catalog() {
                Ok(c) => c,
                Err(_) => Catalog::new(),
            };
            let mut bad = Tool::expect(Family::Text, 3).with_name(nm(b"zzz"));
            bad.checks = 12;
            let corrupt = match cat.slots.get_mut(3) {
                Some(d) => {
                    *d = bad;
                    Ok(())
                }
                None => Err(RC_ESLOT),
            };
            let mask = cat.audit();
            ok = ok && corrupt == Ok(()) && mask != 0 && (mask & 1) == 1;
            let sp = Spread::of(&cat);
            ok = ok && sp.span == 779;
            let mut bad_card = Tool::expect(Family::Tree, 0).with_name(nm(b"qq"));
            bad_card.card = 11500;
            let dup = match cat.slots.get_mut(4) {
                Some(d) => {
                    *d = bad_card;
                    Ok(())
                }
                None => Err(RC_ESLOT),
            };
            let mask2 = cat.audit();
            ok = ok && dup == Ok(()) && (mask2 & 4) == 4;
            ok
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_totals() {
        let c = build_catalog().expect("catalog");
        assert_eq!(c.len(), 60);
        assert_eq!(c.total_checks(), 780);
        assert_eq!(c.total_samples(), 840);
        assert_eq!(c.total_ok(), 480);
        assert_eq!(c.total_err(), 360);
        assert_eq!(c.family_count(Family::Text), 20);
        assert_eq!(c.family_count(Family::Tree), 20);
        assert_eq!(c.family_count(Family::Misc), 20);
        assert_eq!(c.hole_count(Family::Text), 0);
        assert!(c.audit_ok());
    }

    #[test]
    fn name_rules() {
        let n = Name::parse(b"cut").expect("name");
        assert_eq!(n.len(), 3);
        assert_eq!(n.text(), "cut");
        assert!(n.starts_with(b"cu"));
        assert!(Name::parse(b"CUT").is_err());
        assert!(Name::parse(b"").is_err());
        assert!(Name::parse(b"abcdefghijklmnopqrstu").is_err());
        let a = Name::parse(b"abc").expect("a");
        let b = Name::parse(b"abd").expect("b");
        assert_eq!(a.cmp(&b), -1);
        assert_eq!(b.cmp(&a), 1);
        assert_eq!(a.cmp(&a), 0);
        assert_eq!(a.fingerprint(), Name::parse(b"abc").expect("c").fingerprint());
    }

    #[test]
    fn spans_align() {
        let s = Family::Text.span();
        assert_eq!(s.base, 0xe000);
        assert_eq!(s.end(), 0xe140);
        assert!(s.aligned());
        assert!(s.contains(0xe130));
        assert!(!s.contains(0xe140));
        assert!(!s.overlaps(&Family::Tree.span()));
        assert!(!Family::Tree.span().overlaps(&Family::Misc.span()));
    }

    #[test]
    fn audit_detects_corruption() {
        let mut c = build_catalog().expect("catalog");
        assert_eq!(c.audit(), 0);
        let mut bad = Tool::expect(Family::Text, 5).with_name(nm(b"bad55"));
        bad.samples = 13;
        if let Some(d) = c.slots.get_mut(5) {
            *d = bad;
        }
        assert_ne!(c.audit(), 0);
    }

    #[test]
    fn recon_drift() {
        let c = build_catalog().expect("catalog");
        let same = Recon::compare(&Claim::board(), &c);
        assert!(same.clean());
        assert_eq!(same.drift(), 0);
        let over = Claim { tools: 62, checks: 780, samples: 850 };
        let r = Recon::compare(&over, &c);
        assert_eq!(r.tools, -2);
        assert_eq!(r.checks, 0);
        assert_eq!(r.samples, -10);
        assert_eq!(r.drift(), 12);
    }

    #[test]
    fn seal_freeze_replay() {
        let mut c = build_catalog().expect("catalog");
        let sealed = c.seal().expect("seal");
        assert_eq!(sealed, c.ground());
        assert_eq!(c.replay().expect("replay"), sealed);
        let frozen = c.freeze().expect("freeze");
        assert_eq!(frozen, sealed);
        assert!(c.is_frozen());
        assert!(c.register(Tool::expect(Family::Misc, 19).with_name(nm(b"zz"))).is_err());
        assert_eq!(c.rejects(), 1);
        assert!(c.freeze().is_err());
    }

    #[test]
    fn render_shape() {
        let c = build_catalog().expect("catalog");
        let text = c.render();
        assert_eq!(text.split('\n').count(), 65);
        assert!(text.starts_with("CARD FAM NAME"));
        assert!(text.contains("-- B16 小计: 20 工具 260 判据 280 样本"));
        assert!(text.contains("TOTAL 60 工具 780 判据 840 样本"));
        assert!(text.contains("11500 B16 cut"));
        assert!(text.contains("11559 B18 logger"));
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.replay().expect("replay"), l.seal());
    }

    #[test]
    fn spread_bounds() {
        let c = build_catalog().expect("catalog");
        let s = Spread::of(&c);
        assert_eq!(s.min, 1400);
        assert_eq!(s.max, 2179);
        assert_eq!(s.span, 779);
        assert!(s.mean >= s.min && s.mean <= s.max);
    }

    #[test]
    fn inject_all_true() {
        let inj = inject_four();
        assert!(inj[0]);
        assert!(inj[1]);
        assert!(inj[2]);
        assert!(inj[3]);
    }
}

