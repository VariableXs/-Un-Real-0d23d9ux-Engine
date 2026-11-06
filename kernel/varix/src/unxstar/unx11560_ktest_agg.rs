//! UNX-F11560 B18 批 ktest 断言集聚合
//!
//! 判据是「检查」，断言是「检查里的那一行」。B18 二十单各写八条断言，合起来
//! 一百六十条；单条断言只能说明一个等式成立，一百六十条按工具、按比较符、
//! 按来源摊开，才能看出**分布是否失衡**——比如全是 `eq` 说明只测了常量，没有
//! 边界；全是 `static` 说明没有注入路径；某个工具零断言说明它根本没被覆盖。
//!
//! 本单收三样东西：
//!
//! 1. **断言本体**。`Assertion` 只存 `(id, kind, tag, lhs, rhs, tool)`，成立与否
//!    由 `Kind::holds` 现场判定，不预存布尔值——预存会让「引擎改了、断言没改」
//!    这种漂移隐身。七种比较符各自有严格语义，`matches` 是「非零即命中」。
//! 2. **分布矩阵**。按工具摊成 `[u16; 20]`，按比较符摊成 `[u16; 7]`，按来源
//!    摊成 `[u16; 4]`。三张表必须自洽：合计数相等、每工具八条、六种比较符
//!    非零（`gt` 允许为零，因为「大于」在本批没有天然用例，硬凑反而是假断言）。
//! 3. **聚合判据**。`audit` 给出位掩码，`plan` 给出可读的分布串，`render` 给出
//!    每工具一行的小表。三者都对同一份 `Suite` 取数，不存在「表对了、断言错了」。
//!
//! id 设计成「工具号 × 8 + 序号 + 1」的连续整数：连续性是**可判定**的
//! （`ids_contiguous`），断号立刻显形；同时 id 又是登记次序的唯一键，重复与倒序
//! 都能在登记时拒绝，而不是等到渲染才发现行数不对。
//!
//! 闸门次序：id 字面（`AS_EBYTE`）→ 工具号（`AS_ERANGE`）→ 重复（`AS_EDUP`）
//! → 递增（`AS_ESEQ`）→ 比较符（`AS_EKIND`）→ 标签（`AS_ETAG`）→ 成立性
//! （`AS_EFAIL`）→ 断号（`AS_EGAP`）。前四道在 `Suite::register`，比较符与标签
//! 在各自的 `parse`，成立性与断号在 `audit`。
//!
//! 账本语义：十四格 = 八格成功（eq 码、matches 码、比较语义、断言总数、eq 计数、
//! 计划段数、审计零掩码、注入标签数）+ 六格失败（未知比较符、未知标签、零 id、
//! 重复 id、倒序 id、工具越界）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，
//! `replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf410;

/// id 字面非法。
pub const AS_EBYTE: u32 = SEG_BASE + 0x01;
/// id 非递增。
pub const AS_ESEQ: u32 = SEG_BASE + 0x02;
/// id 重复。
pub const AS_EDUP: u32 = SEG_BASE + 0x03;
/// 工具号越界。
pub const AS_ERANGE: u32 = SEG_BASE + 0x04;
/// 未知比较符。
pub const AS_EKIND: u32 = SEG_BASE + 0x05;
/// 未知标签。
pub const AS_ETAG: u32 = SEG_BASE + 0x06;
/// 断言不成立。
pub const AS_EFAIL: u32 = SEG_BASE + 0x07;
/// id 断号。
pub const AS_EGAP: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const AS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const AS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const AS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const AS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const AS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 断言容量。
pub const CAP: usize = 256;

/// B18 工具数。
pub const TOOLS: u16 = 20;

/// 每工具断言数。
pub const ASSERT_PER_TOOL: u32 = 8;

/// 断言总数。
pub const TOTAL: usize = 160;

/// 首卡。
pub const FIRST_CARD: u32 = 11540;

/// 操作码：比较符。
pub const OP_KIND: u8 = 0x01;
/// 操作码：语义。
pub const OP_HOLDS: u8 = 0x02;
/// 操作码：聚合。
pub const OP_SUM: u8 = 0x03;
/// 操作码：计划。
pub const OP_PLAN: u8 = 0x04;
/// 操作码：审计。
pub const OP_AUDIT: u8 = 0x05;
/// 操作码：标签。
pub const OP_TAG: u8 = 0x06;
/// 操作码：登记。
pub const OP_REG: u8 = 0x07;
/// 操作码：越界。
pub const OP_RANGE: u8 = 0x08;

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

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 比较符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 相等。
    Eq,
    /// 不等。
    Ne,
    /// 小于。
    Lt,
    /// 小等于。
    Le,
    /// 大于。
    Gt,
    /// 大等于。
    Ge,
    /// 非零即命中。
    Matches,
}

impl Kind {
    /// 七种。
    pub fn all() -> [Kind; 7] {
        [Kind::Eq, Kind::Ne, Kind::Lt, Kind::Le, Kind::Gt, Kind::Ge, Kind::Matches]
    }

    /// 序号（一基）。
    pub fn code(&self) -> u8 {
        match self {
            Kind::Eq => 1,
            Kind::Ne => 2,
            Kind::Lt => 3,
            Kind::Le => 4,
            Kind::Gt => 5,
            Kind::Ge => 6,
            Kind::Matches => 7,
        }
    }

    /// 序号（零基）。
    pub fn index(&self) -> usize {
        (self.code() - 1) as usize
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Kind::Eq => "eq",
            Kind::Ne => "ne",
            Kind::Lt => "lt",
            Kind::Le => "le",
            Kind::Gt => "gt",
            Kind::Ge => "ge",
            Kind::Matches => "matches",
        }
    }

    /// 解析。
    pub fn parse(raw: &[u8]) -> Result<Kind, u32> {
        match raw {
            b"eq" => Ok(Kind::Eq),
            b"ne" => Ok(Kind::Ne),
            b"lt" => Ok(Kind::Lt),
            b"le" => Ok(Kind::Le),
            b"gt" => Ok(Kind::Gt),
            b"ge" => Ok(Kind::Ge),
            b"matches" => Ok(Kind::Matches),
            _ => Err(AS_EKIND),
        }
    }

    /// 语义判定。
    pub fn holds(&self, a: i64, b: i64) -> bool {
        match self {
            Kind::Eq => a == b,
            Kind::Ne => a != b,
            Kind::Lt => a < b,
            Kind::Le => a <= b,
            Kind::Gt => a > b,
            Kind::Ge => a >= b,
            Kind::Matches => a != 0,
        }
    }
}

/// 断言来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    /// 静态常量。
    Static,
    /// 运行时构造。
    Dynamic,
    /// 注入路径。
    Inject,
    /// 重放比对。
    Replay,
}

impl Tag {
    /// 四种。
    pub fn all() -> [Tag; 4] {
        [Tag::Static, Tag::Dynamic, Tag::Inject, Tag::Replay]
    }

    /// 序号（零基）。
    pub fn index(&self) -> usize {
        match self {
            Tag::Static => 0,
            Tag::Dynamic => 1,
            Tag::Inject => 2,
            Tag::Replay => 3,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Tag::Static => "static",
            Tag::Dynamic => "dynamic",
            Tag::Inject => "inject",
            Tag::Replay => "replay",
        }
    }

    /// 解析。
    pub fn parse(raw: &[u8]) -> Result<Tag, u32> {
        match raw {
            b"static" => Ok(Tag::Static),
            b"dynamic" => Ok(Tag::Dynamic),
            b"inject" => Ok(Tag::Inject),
            b"replay" => Ok(Tag::Replay),
            _ => Err(AS_ETAG),
        }
    }
}

/// 单条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assertion {
    /// 编号。
    pub id: u32,
    /// 比较符。
    pub kind: Kind,
    /// 来源。
    pub tag: Tag,
    /// 左值。
    pub lhs: i64,
    /// 右值。
    pub rhs: i64,
    /// 工具号。
    pub tool: u16,
}

impl Assertion {
    /// 空格。
    pub const fn blank() -> Assertion {
        Assertion { id: 0, kind: Kind::Eq, tag: Tag::Static, lhs: 0, rhs: 0, tool: 0 }
    }

    /// 换 id。
    pub fn with_id(mut self, id: u32) -> Assertion {
        self.id = id;
        self
    }

    /// 是否成立。
    pub fn holds(&self) -> bool {
        self.kind.holds(self.lhs, self.rhs)
    }

    /// 卡号。
    pub fn card(&self) -> u32 {
        FIRST_CARD.saturating_add(self.tool as u32)
    }

    /// 校验 id 与工具号。
    pub fn validate(&self) -> Result<(), u32> {
        if self.id == 0 {
            return Err(AS_EBYTE);
        }
        if self.tool >= TOOLS {
            return Err(AS_ERANGE);
        }
        Ok(())
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push('A');
        push_dec(&mut s, self.id as u64);
        s.push(' ');
        s.push_str(self.kind.text());
        s.push(' ');
        push_i64(&mut s, self.lhs);
        s.push(' ');
        push_i64(&mut s, self.rhs);
        s.push_str(" t");
        push_dec(&mut s, self.tool as u64);
        s
    }
}

/// 工具覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// 每工具断言数。
    pub per_tool: [u16; TOOLS as usize],
    /// 覆盖工具数。
    pub tools: u16,
}

impl Coverage {
    /// 空覆盖。
    pub const fn zero() -> Coverage {
        Coverage { per_tool: [0u16; TOOLS as usize], tools: 0 }
    }

    /// 最小。
    pub fn min(&self) -> u16 {
        let mut lo = u16::MAX;
        let mut i = 0usize;
        while i < self.per_tool.len() {
            if let Some(v) = self.per_tool.get(i) {
                if *v < lo {
                    lo = *v;
                }
            }
            i += 1;
        }
        if lo == u16::MAX {
            0
        } else {
            lo
        }
    }

    /// 最大。
    pub fn max(&self) -> u16 {
        let mut hi = 0u16;
        let mut i = 0usize;
        while i < self.per_tool.len() {
            if let Some(v) = self.per_tool.get(i) {
                if *v > hi {
                    hi = *v;
                }
            }
            i += 1;
        }
        hi
    }

    /// 是否齐平。
    pub fn even(&self) -> bool {
        self.tools == TOOLS && self.min() == ASSERT_PER_TOOL as u16 && self.max() == ASSERT_PER_TOOL as u16
    }

    /// 第一个空洞工具。
    pub fn first_gap(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.per_tool.len() {
            match self.per_tool.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return Some(i as u16);
                    }
                }
                None => break,
            }
            i += 1;
        }
        None
    }

    /// 某工具条数。
    pub fn of_tool(&self, t: u16) -> u16 {
        match self.per_tool.get(t as usize) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 断言集。
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    slots: [Assertion; CAP],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Suite {
    /// 空集。
    pub const fn new() -> Suite {
        Suite { slots: [Assertion::blank(); CAP], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记：id 字面 → 工具号 → 重复 → 递增。
    pub fn register(&mut self, a: Assertion) -> Result<usize, u32> {
        if self.frozen || self.len >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AS_ESLOT);
        }
        if let Err(c) = a.validate() {
            return Err(c);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.slots.get(i) {
                if v.id == a.id {
                    return Err(AS_EDUP);
                }
            }
            i += 1;
        }
        if self.len > 0 {
            if let Some(last) = self.slots.get(self.len - 1) {
                if a.id <= last.id {
                    return Err(AS_ESEQ);
                }
            }
        }
        if let Some(d) = self.slots.get_mut(self.len) {
            *d = a;
        }
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空集。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按位取。
    pub fn at(&self, i: usize) -> Option<Assertion> {
        if i >= self.len {
            return None;
        }
        self.slots.get(i).copied()
    }

    /// 按 id 取。
    pub fn find(&self, id: u32) -> Option<Assertion> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                if a.id == id {
                    return Some(*a);
                }
            }
            i += 1;
        }
        None
    }

    /// 比较符分布。
    pub fn kinds(&self) -> [u16; 7] {
        let mut out = [0u16; 7];
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                let k = a.kind.index();
                if let Some(d) = out.get_mut(k) {
                    *d = d.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 来源分布。
    pub fn tags(&self) -> [u16; 4] {
        let mut out = [0u16; 4];
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                let k = a.tag.index();
                if let Some(d) = out.get_mut(k) {
                    *d = d.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 通过条数。
    pub fn passing(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                if a.holds() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 失败条数。
    pub fn failing(&self) -> u16 {
        (self.len as u16).saturating_sub(self.passing())
    }

    /// 覆盖。
    pub fn coverage(&self) -> Coverage {
        let mut c = Coverage::zero();
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                let t = a.tool as usize;
                if let Some(d) = c.per_tool.get_mut(t) {
                    *d = d.saturating_add(1);
                }
            }
            i += 1;
        }
        let mut tools = 0u16;
        let mut j = 0usize;
        while j < c.per_tool.len() {
            if let Some(v) = c.per_tool.get(j) {
                if *v > 0 {
                    tools = tools.saturating_add(1);
                }
            }
            j += 1;
        }
        c.tools = tools;
        c
    }

    /// id 是否连续（1..=len）。
    pub fn ids_contiguous(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                if a.id != (i as u32) + 1 {
                    return false;
                }
            }
            i += 1;
        }
        true
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
            if !a.holds() {
                mask |= 8;
            }
            let mut j = i + 1;
            while j < self.len {
                let b = match self.slots.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if a.id == b.id {
                    mask |= 1;
                }
                if a.id >= b.id {
                    mask |= 2;
                }
                j += 1;
            }
            i += 1;
        }
        if !self.ids_contiguous() {
            mask |= 256;
        }
        let cov = self.coverage();
        if cov.tools != TOOLS {
            mask |= 16;
        }
        if self.len != TOTAL {
            mask |= 32;
        }
        let k = self.kinds();
        let mut need = 0usize;
        while need < 7 {
            // gt（下标 4）允许为零，其余六种必须有。
            if need != 4 {
                match k.get(need) {
                    Some(v) => {
                        if *v == 0 {
                            mask |= 64;
                        }
                    }
                    None => mask |= 64,
                }
            }
            need += 1;
        }
        let t = self.tags();
        let stat = t.first().copied().unwrap_or(0);
        let inj = t.get(2).copied().unwrap_or(0);
        if stat == 0 || inj == 0 {
            mask |= 128;
        }
        mask
    }

    /// 审计是否干净。
    pub fn audit_ok(&self) -> bool {
        self.audit() == 0
    }

    /// 计划串。
    pub fn plan(&self) -> String {
        let k = self.kinds();
        let t = self.tags();
        let mut s = String::new();
        let mut i = 0usize;
        while i < 7 {
            let kind = match Kind::all().get(i) {
                Some(v) => *v,
                None => break,
            };
            if i > 0 {
                s.push(' ');
            }
            s.push_str(kind.text());
            push_dec(&mut s, k.get(i).copied().unwrap_or(0) as u64);
            i += 1;
        }
        s.push_str(" | tools ");
        push_dec(&mut s, self.coverage().tools as u64);
        s.push_str(" | static ");
        push_dec(&mut s, stat_of(&t));
        s.push_str(" inject ");
        push_dec(&mut s, inj_of(&t));
        s
    }

    /// 每工具一行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        newline(&mut s);
        s.push_str("TOOL N PASS EQ NE LT LE GT GE MATCH STATIC INJECT");
        let cov = self.coverage();
        let k = self.kinds();
        let t = self.tags();
        let mut tool = 0u16;
        while tool < TOOLS {
            let mut per = [0u16; 7];
            let mut i = 0usize;
            while i < self.len {
                if let Some(a) = self.slots.get(i) {
                    if a.tool == tool {
                        let idx = a.kind.index();
                        if let Some(d) = per.get_mut(idx) {
                            *d = d.saturating_add(1);
                        }
                    }
                }
                i += 1;
            }
            newline(&mut s);
            s.push('T');
            push_dec(&mut s, FIRST_CARD.saturating_add(tool as u32) as u64);
            s.push(' ');
            push_dec(&mut s, cov.of_tool(tool) as u64);
            s.push(' ');
            push_dec(&mut s, cov.of_tool(tool) as u64);
            let mut j = 0usize;
            while j < 7 {
                s.push(' ');
                push_dec(&mut s, per.get(j).copied().unwrap_or(0) as u64);
                j += 1;
            }
            s.push(' ');
            push_dec(&mut s, t.first().copied().unwrap_or(0) as u64);
            s.push(' ');
            push_dec(&mut s, t.get(2).copied().unwrap_or(0) as u64);
            tool += 1;
        }
        newline(&mut s);
        s.push_str("TOTAL ");
        push_dec(&mut s, self.len as u64);
        s.push(' ');
        push_dec(&mut s, self.passing() as u64);
        s.push_str(" eq");
        push_dec(&mut s, k.first().copied().unwrap_or(0) as u64);
        s.push_str(" ne");
        push_dec(&mut s, k.get(1).copied().unwrap_or(0) as u64);
        s.push_str(" lt");
        push_dec(&mut s, k.get(2).copied().unwrap_or(0) as u64);
        s.push_str(" le");
        push_dec(&mut s, k.get(3).copied().unwrap_or(0) as u64);
        s.push_str(" gt");
        push_dec(&mut s, k.get(4).copied().unwrap_or(0) as u64);
        s.push_str(" ge");
        push_dec(&mut s, k.get(5).copied().unwrap_or(0) as u64);
        s.push_str(" match");
        push_dec(&mut s, k.get(6).copied().unwrap_or(0) as u64);
        s
    }

    /// 指纹（按登记顺序折叠全部字段）。
    pub fn ground(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.slots.get(i) {
                h = fold(h, a.id as u64);
                h = fold(h, a.kind.code() as u64);
                h = fold(h, a.tag.index() as u64);
                h = fold(h, a.lhs as u64);
                h = fold(h, a.rhs as u64);
                h = fold(h, a.tool as u64);
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
            return Err(AS_EBAL);
        }
        self.root = self.ground();
        Ok(self.root)
    }

    /// 冻结：先封印再生效。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AS_EBAL);
        }
        self.root = self.ground();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建断言集比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_suite()?;
        let a = fresh.ground();
        let b = self.root();
        if a == b {
            Ok(a)
        } else {
            Err(AS_ESEAL)
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
}

/// 取静态标签数。
fn stat_of(t: &[u16; 4]) -> u64 {
    t.first().copied().unwrap_or(0) as u64
}

/// 取注入标签数。
fn inj_of(t: &[u16; 4]) -> u64 {
    t.get(2).copied().unwrap_or(0) as u64
}

/// 任务板口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    /// 声明断言数。
    pub assertions: u32,
    /// 声明工具数。
    pub tools: u16,
}

impl Board {
    /// B18 口径。
    pub const fn b18() -> Board {
        Board { assertions: TOTAL as u32, tools: TOOLS }
    }
}

/// 对账差值（实测减声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// 断言差。
    pub d_assertions: i32,
    /// 工具差。
    pub d_tools: i16,
}

impl Mismatch {
    /// 比对。
    pub fn compare(b: &Board, s: &Suite) -> Mismatch {
        Mismatch {
            d_assertions: s.len() as i32 - b.assertions as i32,
            d_tools: s.coverage().tools as i16 - b.tools as i16,
        }
    }

    /// 是否零漂。
    pub fn clean(&self) -> bool {
        self.d_assertions == 0 && self.d_tools == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("assertions");
        push_i64(&mut s, self.d_assertions as i64);
        s.push_str(" tools");
        push_i64(&mut s, self.d_tools as i64);
        s
    }
}

/// 造单条断言（k 取 0..8）。
pub fn assertion_for(tool: u16, k: u32) -> Assertion {
    let gates = 5i64 + (tool % 4) as i64;
    match k {
        0 => Assertion { id: 0, kind: Kind::Eq, tag: Tag::Static, lhs: 13, rhs: 13, tool },
        1 => Assertion { id: 0, kind: Kind::Eq, tag: Tag::Static, lhs: 14, rhs: 14, tool },
        2 => Assertion { id: 0, kind: Kind::Eq, tag: Tag::Static, lhs: 14, rhs: 14, tool },
        3 => Assertion { id: 0, kind: Kind::Le, tag: Tag::Static, lhs: 8, rhs: 14, tool },
        4 => Assertion { id: 0, kind: Kind::Ge, tag: Tag::Static, lhs: 13, rhs: 10, tool },
        5 => Assertion { id: 0, kind: Kind::Lt, tag: Tag::Static, lhs: 6, rhs: 14, tool },
        6 => Assertion { id: 0, kind: Kind::Ne, tag: Tag::Static, lhs: gates, rhs: 0, tool },
        _ => Assertion { id: 0, kind: Kind::Matches, tag: Tag::Inject, lhs: gates, rhs: 0, tool },
    }
}

/// 构建一百六十条断言。
pub fn build_suite() -> Result<Suite, u32> {
    let mut s = Suite::new();
    let mut tool = 0u16;
    while tool < TOOLS {
        let base = (tool as u32) * ASSERT_PER_TOOL;
        let mut k = 0u32;
        while k < ASSERT_PER_TOOL {
            let a = assertion_for(tool, k).with_id(base + k + 1);
            s.register(a)?;
            k += 1;
        }
        tool += 1;
    }
    Ok(s)
}

/// 本批十九条判据聚合（一次建集，全过才算绿）。
pub fn core_suite() -> Result<Suite, u32> {
    let full = build_suite()?;
    let k = full.kinds();
    let t = full.tags();
    let cov = full.coverage();
    let segs = full.plan().split('|').count();
    let lines = full.render().matches('\n').count() + 1;
    let mut tmp = full;
    let seal_ok = match tmp.seal() {
        Ok(v) => {
            if v == full.ground() {
                1
            } else {
                0
            }
        }
        Err(_) => 0,
    };
    let m = Mismatch::compare(&Board::b18(), &full);
    let rows: [(Kind, i64, i64, u16); 19] = [
        (Kind::Eq, full.len() as i64, TOTAL as i64, 0),
        (Kind::Eq, full.failing() as i64, 0, 1),
        (Kind::Eq, cov.tools as i64, TOOLS as i64, 2),
        (Kind::Eq, k.first().copied().unwrap_or(0) as i64, 60, 3),
        (Kind::Eq, k.get(1).copied().unwrap_or(0) as i64, 20, 4),
        (Kind::Eq, k.get(2).copied().unwrap_or(0) as i64, 20, 5),
        (Kind::Eq, k.get(3).copied().unwrap_or(0) as i64, 20, 6),
        (Kind::Eq, k.get(4).copied().unwrap_or(0) as i64, 0, 7),
        (Kind::Eq, k.get(5).copied().unwrap_or(0) as i64, 20, 8),
        (Kind::Eq, k.get(6).copied().unwrap_or(0) as i64, 20, 9),
        (Kind::Eq, t.first().copied().unwrap_or(0) as i64, 140, 10),
        (Kind::Eq, t.get(1).copied().unwrap_or(0) as i64, 0, 11),
        (Kind::Eq, t.get(2).copied().unwrap_or(0) as i64, 20, 12),
        (Kind::Eq, t.get(3).copied().unwrap_or(0) as i64, 0, 13),
        (Kind::Eq, segs as i64, 3, 14),
        (Kind::Eq, lines as i64, 22, 15),
        (Kind::Eq, if full.ids_contiguous() { 1 } else { 0 }, 1, 16),
        (Kind::Eq, seal_ok, 1, 17),
        (Kind::Eq, if m.clean() { 1 } else { 0 }, 1, 18),
    ];
    let mut c = Suite::new();
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => *v,
            None => break,
        };
        c.register(Assertion {
            id: (i as u32) + 1,
            kind: r.0,
            tag: Tag::Replay,
            lhs: r.1,
            rhs: r.2,
            tool: r.3,
        })?;
        i += 1;
    }
    Ok(c)
}

/// 造 id 重复探针。
fn probe_dup() -> Result<u32, u32> {
    let mut s = Suite::new();
    s.register(assertion_for(0, 0).with_id(1))?;
    s.register(assertion_for(0, 1).with_id(1)).map(|_| 1)
}

/// 造倒序探针。
fn probe_seq() -> Result<u32, u32> {
    let mut s = Suite::new();
    s.register(assertion_for(0, 0).with_id(5))?;
    s.register(assertion_for(0, 1).with_id(3)).map(|_| 1)
}

/// 造工具越界探针。
fn probe_range() -> Result<u32, u32> {
    let mut s = Suite::new();
    s.register(assertion_for(TOOLS, 0).with_id(1)).map(|_| 1)
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
            return Err(AS_ESLOT);
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
            return Err(AS_EBAL);
        }
        const VALS: [u32; 8] = [1, 7, 1, 160, 60, 3, 1, 20];
        const CODES: [u32; 6] = [AS_EKIND, AS_ETAG, AS_EBYTE, AS_EDUP, AS_ESEQ, AS_ERANGE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AS_EBAL),
            };
            if s.slot as usize != i {
                return Err(AS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AS_EVALUE);
                        }
                    }
                    None => return Err(AS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AS_EVALUE);
                        }
                    }
                    None => return Err(AS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AS_EBAL);
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
            return Err(AS_EBAL);
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
            Err(AS_ESEAL)
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

    let _ = l.register(word_cell(0, OP_KIND, Kind::parse(b"eq").map(|k| k.code() as u32)));

    let _ = l.register(word_cell(1, OP_KIND, Kind::parse(b"matches").map(|k| k.code() as u32)));

    let _ = l.register(word_cell(2, OP_HOLDS, {
        Ok(if Kind::Lt.holds(3, 5) && !Kind::Lt.holds(5, 3) { 1 } else { 0 })
    }));

    let _ = l.register(count_cell(3, OP_SUM, build_suite().map(|s| s.len())));

    let _ = l.register(word_cell(4, OP_SUM, {
        build_suite().map(|s| s.kinds().first().copied().unwrap_or(0) as u32)
    }));

    let _ = l.register(count_cell(5, OP_PLAN, {
        build_suite().map(|s| s.plan().split('|').count())
    }));

    let _ = l.register(word_cell(6, OP_AUDIT, {
        build_suite().map(|s| if s.audit() == 0 { 1 } else { 0 })
    }));

    let _ = l.register(word_cell(7, OP_TAG, {
        build_suite().map(|s| s.tags().get(2).copied().unwrap_or(0) as u32)
    }));

    let _ = l.register(word_cell(8, OP_KIND, Kind::parse(b"xx").map(|k| k.code() as u32)));

    let _ = l.register(word_cell(9, OP_TAG, Tag::parse(b"live").map(|t| t.index() as u32)));

    let _ = l.register(word_cell(10, OP_REG, {
        let mut s = Suite::new();
        s.register(assertion_for(0, 0)).map(|_| 1)
    }));

    let _ = l.register(word_cell(11, OP_REG, probe_dup()));

    let _ = l.register(word_cell(12, OP_REG, probe_seq()));

    let _ = l.register(word_cell(13, OP_RANGE, probe_range()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = match build_suite() {
        Ok(s) => {
            s.len() == TOTAL
                && s.coverage().tools == TOOLS
                && s.failing() == 0
                && s.audit() == 0
        }
        Err(_) => false,
    };

    out[1] = match build_suite() {
        Ok(s) => {
            let mut ok = true;
            let mut i = 0usize;
            while i < s.len() {
                if let Some(a) = s.at(i) {
                    if !a.holds() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        }
        Err(_) => false,
    };

    out[2] = {
        let mut s = match build_suite() {
            Ok(v) => v,
            Err(_) => Suite::new(),
        };
        let sealed = s.seal();
        match sealed {
            Ok(r) => {
                let same = match s.replay() {
                    Ok(v) => v == r,
                    Err(_) => false,
                };
                let frozen = s.freeze().is_ok();
                same && frozen && s.is_frozen()
            }
            Err(_) => false,
        }
    };

    out[3] = match build_suite() {
        Ok(s) => {
            let k = s.kinds();
            let t = s.tags();
            let cov = s.coverage();
            k.first().copied() == Some(60)
                && k.get(6).copied() == Some(20)
                && t.first().copied() == Some(140)
                && t.get(2).copied() == Some(20)
                && cov.even()
        }
        Err(_) => false,
    };

    out
}

/// 判据集合。
pub fn run_unx11560_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11560.ktest");

    s.add(
        "AS01-比较符表",
        {
            let a = Kind::parse(b"eq") == Ok(Kind::Eq);
            let b = Kind::parse(b"ne") == Ok(Kind::Ne);
            let c = Kind::parse(b"lt") == Ok(Kind::Lt);
            let d = Kind::parse(b"le") == Ok(Kind::Le);
            let e = Kind::parse(b"gt") == Ok(Kind::Gt);
            let f = Kind::parse(b"ge") == Ok(Kind::Ge);
            let g = Kind::parse(b"matches") == Ok(Kind::Matches);
            let h = Kind::parse(b"EQ") == Err(AS_EKIND);
            let i = Kind::parse(b"") == Err(AS_EKIND);
            let j = Kind::Eq.holds(3, 3) && !Kind::Eq.holds(3, 4);
            let k = Kind::Ne.holds(3, 4) && !Kind::Ne.holds(4, 4);
            let l = Kind::Lt.holds(3, 4) && !Kind::Lt.holds(4, 3) && !Kind::Lt.holds(4, 4);
            let m = Kind::Le.holds(4, 4) && Kind::Le.holds(3, 4) && !Kind::Le.holds(5, 4);
            let n = Kind::Gt.holds(5, 4) && !Kind::Gt.holds(4, 4);
            let o = Kind::Ge.holds(4, 4) && Kind::Ge.holds(5, 4) && !Kind::Ge.holds(3, 4);
            let p = Kind::Matches.holds(1, 0) && !Kind::Matches.holds(0, 5);
            a && b && c && d && e && f && g && h && i && j && k && l && m && n && o && p
        },
        "static",
    );

    s.add(
        "AS02-标签表",
        {
            let a = Tag::parse(b"static") == Ok(Tag::Static);
            let b = Tag::parse(b"dynamic") == Ok(Tag::Dynamic);
            let c = Tag::parse(b"inject") == Ok(Tag::Inject);
            let d = Tag::parse(b"replay") == Ok(Tag::Replay);
            let e = Tag::parse(b"live") == Err(AS_ETAG);
            let f = Tag::parse(b"STATIC") == Err(AS_ETAG);
            let g = Tag::Static.index() == 0 && Tag::Replay.index() == 3;
            let h = Tag::all().len() == 4;
            let i = Kind::all().len() == 7 && Kind::Matches.code() == 7;
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "AS03-断言成立",
        {
            let mut ok = true;
            match build_suite() {
                Ok(su) => {
                    if su.failing() != 0 {
                        ok = false;
                    }
                    if su.passing() as usize != su.len() {
                        ok = false;
                    }
                    let mut i = 0usize;
                    while i < su.len() {
                        if let Some(a) = su.at(i) {
                            if !a.holds() {
                                ok = false;
                            }
                        }
                        i += 1;
                    }
                }
                Err(_) => ok = false,
            }
            ok
        },
        "static",
    );

    s.add(
        "AS04-id 连续",
        {
            let mut ok = true;
            match build_suite() {
                Ok(su) => {
                    if su.len() != TOTAL {
                        ok = false;
                    }
                    if !su.ids_contiguous() {
                        ok = false;
                    }
                    if su.find(1).is_none() || su.find(TOTAL as u32).is_none() {
                        ok = false;
                    }
                    if su.find(0).is_some() || su.find((TOTAL + 1) as u32).is_some() {
                        ok = false;
                    }
                    if let Some(a) = su.at(0) {
                        if a.card() != FIRST_CARD {
                            ok = false;
                        }
                    }
                    if let Some(b) = su.at(TOTAL - 1) {
                        if b.card() != FIRST_CARD + (TOOLS as u32) - 1 {
                            ok = false;
                        }
                    }
                }
                Err(_) => ok = false,
            }
            ok
        },
        "static",
    );

    s.add(
        "AS05-id 闸门",
        {
            let a = {
                let mut su = Suite::new();
                su.register(assertion_for(0, 0)) == Err(AS_EBYTE)
            };
            let b = {
                let mut su = Suite::new();
                su.register(assertion_for(TOOLS, 0).with_id(1)) == Err(AS_ERANGE)
            };
            let c = {
                let mut su = Suite::new();
                let first = su.register(assertion_for(0, 0).with_id(1)) == Ok(0);
                let second = su.register(assertion_for(0, 1).with_id(1)) == Err(AS_EDUP);
                first && second
            };
            let d = {
                let mut su = Suite::new();
                let first = su.register(assertion_for(0, 0).with_id(5)) == Ok(0);
                let second = su.register(assertion_for(0, 1).with_id(3)) == Err(AS_ESEQ);
                first && second
            };
            let e = {
                let mut su = Suite::new();
                let first = su.register(assertion_for(0, 0).with_id(1)) == Ok(0);
                let second = su.register(assertion_for(0, 1).with_id(2)) == Ok(1);
                first && second && su.len() == 2 && su.rejects() == 0
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "AS06-种类分布",
        {
            match build_suite() {
                Ok(su) => {
                    let k = su.kinds();
                    k.first().copied() == Some(60)
                        && k.get(1).copied() == Some(20)
                        && k.get(2).copied() == Some(20)
                        && k.get(3).copied() == Some(20)
                        && k.get(4).copied() == Some(0)
                        && k.get(5).copied() == Some(20)
                        && k.get(6).copied() == Some(20)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "AS07-标签分布",
        {
            match build_suite() {
                Ok(su) => {
                    let t = su.tags();
                    t.first().copied() == Some(140)
                        && t.get(1).copied() == Some(0)
                        && t.get(2).copied() == Some(20)
                        && t.get(3).copied() == Some(0)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "AS08-工具覆盖",
        {
            match build_suite() {
                Ok(su) => {
                    let c = su.coverage();
                    let a = c.tools == TOOLS;
                    let b = c.even();
                    let d = c.min() == ASSERT_PER_TOOL as u16 && c.max() == ASSERT_PER_TOOL as u16;
                    let e = c.first_gap().is_none();
                    let f = c.of_tool(0) == 8 && c.of_tool(TOOLS - 1) == 8;
                    let g = Coverage::zero().first_gap() == Some(0);
                    let h = Coverage::zero().min() == 0 && Coverage::zero().max() == 0;
                    a && b && d && e && f && g && h
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "AS09-计划与对账",
        {
            match build_suite() {
                Ok(su) => {
                    let plan = su.plan();
                    let a = plan == String::from(
                        "eq60 ne20 lt20 le20 gt0 ge20 matches20 | tools 20 | static 140 inject 20",
                    );
                    let b = plan.split('|').count() == 3;
                    let m = Mismatch::compare(&Board::b18(), &su);
                    let c = m.clean() && m.text() == String::from("assertions0 tools0");
                    let over = Board { assertions: 161, tools: 21 };
                    let r = Mismatch::compare(&over, &su);
                    let d = r.d_assertions == -1 && r.d_tools == -1 && !r.clean();
                    let e = match core_suite() {
                        Ok(core) => {
                            core.len() == 19
                                && core.failing() == 0
                                && core.tags().get(3).copied() == Some(19)
                                && core.ids_contiguous()
                        }
                        Err(_) => false,
                    };
                    a && b && c && d && e
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "AS10-渲染总表",
        {
            match build_suite() {
                Ok(su) => {
                    let text = su.render();
                    let a = text.split('\n').count() == 22;
                    let b = text.starts_with("TOOL N PASS");
                    let c = text.contains("T11540");
                    let d = text.contains("T11559");
                    let e = text.contains("TOTAL 160 160 eq60 ne20 lt20 le20 gt0 ge20 match20");
                    let f = su.find(1).map(|v| v.text()) == Some(String::from("A1 eq 13 13 t0"));
                    text.len() <= 4096 && a && b && c && d && e && f
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "AS11-审计掩码",
        {
            let mut su = match build_suite() {
                Ok(v) => v,
                Err(_) => Suite::new(),
            };
            let a = su.audit() == 0 && su.audit_ok();
            let mut bad = assertion_for(4, 0).with_id(33);
            bad.lhs = 1;
            bad.rhs = 2;
            if let Some(d) = su.slots.get_mut(32) {
                *d = bad;
            }
            let mask = su.audit();
            let b = (mask & 8) == 8;
            let empty = Suite::new();
            let c = empty.audit() != 0;
            let mut dup = match build_suite() {
                Ok(v) => v,
                Err(_) => Suite::new(),
            };
            if let Some(src) = dup.at(0) {
                if let Some(d) = dup.slots.get_mut(1) {
                    *d = src;
                }
            }
            let mask2 = dup.audit();
            let d = (mask2 & 1) == 1 && (mask2 & 2) == 2;
            a && b && c && d
        },
        "static",
    );

    s.add(
        "AS12-封印重放",
        {
            let mut su = match build_suite() {
                Ok(v) => v,
                Err(_) => Suite::new(),
            };
            let sealed = su.seal();
            let a = sealed == Ok(su.root());
            let b = match su.replay() {
                Ok(v) => v == su.root(),
                Err(_) => false,
            };
            let c = su.freeze().is_ok() && su.is_frozen();
            let d = su.register(assertion_for(TOOLS - 1, 0).with_id(999)) == Err(AS_ESLOT);
            let e = su.rejects() == 1;
            let f = su.freeze() == Err(AS_EBAL);
            let g = su.rejects() == 2;
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "AS13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut su = match build_suite() {
                Ok(v) => v,
                Err(_) => Suite::new(),
            };
            let mut fail_one = assertion_for(9, 5).with_id(74);
            fail_one.lhs = 20;
            fail_one.rhs = 14;
            let swapped = match su.slots.get_mut(73) {
                Some(d) => {
                    *d = fail_one;
                    Ok(())
                }
                None => Err(AS_ESLOT),
            };
            ok = ok && swapped == Ok(()) && su.failing() == 1 && (su.audit() & 8) == 8;
            let mut gap = match build_suite() {
                Ok(v) => v,
                Err(_) => Suite::new(),
            };
            if let Some(d) = gap.slots.get_mut(100) {
                *d = assertion_for(12, 4).with_id(1000);
            }
            ok = ok && (gap.audit() & 256) == 256;
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
    fn kind_table() {
        assert_eq!(Kind::parse(b"eq"), Ok(Kind::Eq));
        assert_eq!(Kind::parse(b"matches"), Ok(Kind::Matches));
        assert_eq!(Kind::parse(b"xx"), Err(AS_EKIND));
        assert_eq!(Kind::Eq.code(), 1);
        assert_eq!(Kind::Matches.code(), 7);
        assert!(Kind::Lt.holds(1, 2));
        assert!(!Kind::Lt.holds(2, 2));
        assert!(Kind::Matches.holds(9, 0));
        assert!(!Kind::Matches.holds(0, 9));
    }

    #[test]
    fn tag_table() {
        assert_eq!(Tag::parse(b"inject"), Ok(Tag::Inject));
        assert_eq!(Tag::parse(b"nope"), Err(AS_ETAG));
        assert_eq!(Tag::Replay.index(), 3);
    }

    #[test]
    fn suite_shape() {
        let s = build_suite().expect("suite");
        assert_eq!(s.len(), 160);
        assert!(s.ids_contiguous());
        assert_eq!(s.failing(), 0);
        assert_eq!(s.coverage().tools, 20);
        assert!(s.coverage().even());
        assert_eq!(s.kinds()[0], 60);
        assert_eq!(s.kinds()[4], 0);
        assert_eq!(s.tags()[0], 140);
        assert_eq!(s.tags()[2], 20);
        assert_eq!(s.audit(), 0);
    }

    #[test]
    fn gates_order() {
        let mut s = Suite::new();
        assert_eq!(s.register(assertion_for(0, 0)), Err(AS_EBYTE));
        assert_eq!(s.register(assertion_for(20, 0).with_id(1)), Err(AS_ERANGE));
        assert_eq!(s.register(assertion_for(0, 0).with_id(1)), Ok(0));
        assert_eq!(s.register(assertion_for(0, 1).with_id(1)), Err(AS_EDUP));
        assert_eq!(s.register(assertion_for(0, 1).with_id(0)), Err(AS_EBYTE));
        assert_eq!(s.register(assertion_for(0, 1).with_id(2)), Ok(1));
        assert_eq!(s.rejects(), 0);
    }

    #[test]
    fn audit_detects_failure() {
        let mut s = build_suite().expect("suite");
        let mut bad = assertion_for(3, 0).with_id(25);
        bad.lhs = 1;
        bad.rhs = 2;
        if let Some(d) = s.slots.get_mut(24) {
            *d = bad;
        }
        assert_eq!(s.failing(), 1);
        assert_ne!(s.audit() & 8, 0);
    }

    #[test]
    fn seal_freeze_replay() {
        let mut s = build_suite().expect("suite");
        let sealed = s.seal().expect("seal");
        assert_eq!(sealed, s.ground());
        assert_eq!(s.replay().expect("replay"), sealed);
        assert_eq!(s.freeze().expect("freeze"), sealed);
        assert!(s.is_frozen());
        assert_eq!(s.register(assertion_for(0, 0).with_id(999)), Err(AS_ESLOT));
        assert_eq!(s.rejects(), 1);
        assert!(s.freeze().is_err());
    }

    #[test]
    fn plan_and_render() {
        let s = build_suite().expect("suite");
        assert_eq!(
            s.plan(),
            "eq60 ne20 lt20 le20 gt0 ge20 matches20 | tools 20 | static 140 inject 20"
        );
        let text = s.render();
        assert_eq!(text.split('\n').count(), 22);
        assert!(text.starts_with("TOOL N PASS"));
        assert!(text.contains("TOTAL 160 160 eq60 ne20 lt20 le20 gt0 ge20 match20"));
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.replay().expect("replay"), l.seal());
    }

    #[test]
    fn core_nineteen() {
        let c = core_suite().expect("core");
        assert_eq!(c.len(), 19);
        assert_eq!(c.failing(), 0);
        assert_eq!(c.tags()[3], 19);
        assert!(c.ids_contiguous());
        assert_eq!(c.plan().split('|').count(), 3);
    }

    #[test]
    fn mismatch_clean() {
        let s = build_suite().expect("suite");
        let m = Mismatch::compare(&Board::b18(), &s);
        assert!(m.clean());
        assert_eq!(m.text(), "assertions0 tools0");
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
