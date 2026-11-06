//! UNX-F11564 sort LC_COLLATE=C 硬字典序判据
//!
//! 问题的根：`sort` 的输出**不是**「按字母排」，而是「按当前 collation 的比较函数
//! 排」。`LC_COLLATE=C` 时这个函数退化成最朴素的一条：**逐字节比无符号字节值**，
//! 没有重权（一个字符只有一个权重）、没有等价类（`a` 与 `A` 不相等）、没有
//! 多级比较（不区分「先比字母再比重音」）。于是 `sort` 与 `sort -f`、`sort -n`
//! 的行为差异全部来自**键的构造**，而不是来自「语言」。
//!
//! 本单实现一条完整的 sort 管线，每一段都可独立判定：
//!
//! 1. **选项**。`-r/-u/-f/-n/-b/-s`、`-k F[.C][,F[.C]]`、`-t SEP`。`-k` 给两次
//!    是冲突（`SR_ECONF`），`-t` 给两次是重复（`SR_EDUP`），缺值是截断
//!    （`SR_ESHORT`），多字节分隔符是字节错（`SR_EBYTE`）。
//! 2. **键提取**。字段按分隔符切；缺省分隔符是「空白串」（连续空格/制表算一次
//!    切分），`-t` 则按单字节精确切，空字段也算一个字段。字段不存在时键为空，
//!    排在所有非空键之前——这是 `sort -k9` 在短行上「什么都不做」的根因。
//! 3. **比较**。非数字模式做逐字节比较（`-f` 先把 A–Z 折成 a–z，这是 sort 自己
//!    做的折叠，不是 collation 的等价类）；数字模式解析 `[+-]?digits[.digits]`，
//!    先比符号、再比整数部分、最后比小数部分（补齐到 9 位），非法数字按 0 处理。
//! 4. **排序**。自底向上归并，**相等时先取左段**——稳定性是可判定的：排序后
//!    键相等的相邻行，其原始下标必须递增。`-u` 只去掉**比较相等**的相邻行，
//!    不是去掉字节相等的行（`-f -u` 会把 A 与 a 合成一条）。
//! 5. **检查**。`-c` 语义：任何相邻逆序就是 `SR_ESTATE`；`violations` 给出逆序
//!    条数，供账本入账。
//!
//! 闸门次序：`SR_EINVAL`（未知选项）→ `SR_ESHORT`（缺参数/空语料）→
//! `SR_EBYTE`（多字节分隔符/非法字节）→ `SR_ERANGE`（字段号 0 或越界）→
//! `SR_ELEN`（行超长）→ `SR_EDUP`（重复 -t）→ `SR_ECONF`（重复 -k）→
//! `SR_ESTATE`（已排序检查失败）。前七道在 `Opts::parse`/`Key::parse`/
//! `Line::parse`，最后一道在 `Corpus::check`。
//!
//! 账本语义：十四格 = 八格成功（首行原下标、逆序首行、折叠首行、数字首行、
//! 键排序首行、去重条数、逆序计数、数字整数部分）+ 六格失败（未知选项、
//! 字段 0、缺分隔符、多字节分隔符、行超长、检查失败）。`audit` 认「八值六码」，
//! `seal` 折叠，`replay` 重建比对，`freeze` 后拒登。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf450;

/// 未知选项/非法参数。
pub const SR_EINVAL: u32 = SEG_BASE + 0x01;
/// 行/参数超长。
pub const SR_ELEN: u32 = SEG_BASE + 0x02;
/// 字节非法（多字节分隔符）。
pub const SR_EBYTE: u32 = SEG_BASE + 0x03;
/// 字段号越界。
pub const SR_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复 -t。
pub const SR_EDUP: u32 = SEG_BASE + 0x05;
/// 缺参数/空语料。
pub const SR_ESHORT: u32 = SEG_BASE + 0x06;
/// 已排序检查失败。
pub const SR_ESTATE: u32 = SEG_BASE + 0x07;
/// 选项冲突（重复 -k）。
pub const SR_ECONF: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const SR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const SR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const SR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单行容量。
pub const LINE_CAP: usize = 32;

/// 语料容量。
pub const LINES_CAP: usize = 16;

/// 字段号上限。
pub const FIELD_MAX: u16 = 32;

/// 小数位上限。
pub const FRAC_DIGITS: u8 = 9;

/// 操作码：排序。
pub const OP_SORT: u8 = 0x01;
/// 操作码：逆序。
pub const OP_REV: u8 = 0x02;
/// 操作码：折叠。
pub const OP_FOLD: u8 = 0x03;
/// 操作码：数字。
pub const OP_NUM: u8 = 0x04;
/// 操作码：键。
pub const OP_KEY: u8 = 0x05;
/// 操作码：去重。
pub const OP_UNIQ: u8 = 0x06;
/// 操作码：检查。
pub const OP_CHECK: u8 = 0x07;
/// 操作码：解析。
pub const OP_PARSE: u8 = 0x08;

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

/// 三值比较。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ord3 {
    /// 小于。
    Less,
    /// 相等。
    Equal,
    /// 大于。
    Greater,
}

impl Ord3 {
    /// 取反。
    pub fn flip(&self) -> Ord3 {
        match self {
            Ord3::Less => Ord3::Greater,
            Ord3::Equal => Ord3::Equal,
            Ord3::Greater => Ord3::Less,
        }
    }

    /// 码。
    pub fn code(&self) -> i32 {
        match self {
            Ord3::Less => -1,
            Ord3::Equal => 0,
            Ord3::Greater => 1,
        }
    }

    /// 秩。
    pub fn rank(&self) -> u32 {
        match self {
            Ord3::Less => 1,
            Ord3::Equal => 2,
            Ord3::Greater => 3,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Ord3::Less => "lt",
            Ord3::Equal => "eq",
            Ord3::Greater => "gt",
        }
    }
}

/// 字节序比较（C locale 的全部定义）。
pub fn cmp_bytes(a: &[u8], b: &[u8]) -> Ord3 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x < y {
            return Ord3::Less;
        }
        if x > y {
            return Ord3::Greater;
        }
        i += 1;
    }
    if a.len() < b.len() {
        Ord3::Less
    } else if a.len() > b.len() {
        Ord3::Greater
    } else {
        Ord3::Equal
    }
}

/// 折叠比较（A–Z → a–z 后再逐字节）。
pub fn cmp_fold(a: &[u8], b: &[u8]) -> Ord3 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        let x = a.get(i).copied().unwrap_or(0).to_ascii_lowercase();
        let y = b.get(i).copied().unwrap_or(0).to_ascii_lowercase();
        if x < y {
            return Ord3::Less;
        }
        if x > y {
            return Ord3::Greater;
        }
        i += 1;
    }
    if a.len() < b.len() {
        Ord3::Less
    } else if a.len() > b.len() {
        Ord3::Greater
    } else {
        Ord3::Equal
    }
}

/// 键规格：`-k F[.C][,F[.C]]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// 起始字段（一基）。
    pub f1: u16,
    /// 起始字符（一基，0 = 字段首）。
    pub c1: u16,
    /// 终止字段（一基）。
    pub f2: u16,
    /// 终止字符（一基，0 = 字段尾）。
    pub c2: u16,
    /// 是否给了 `,F` 部分。
    pub has_end: bool,
}

impl Key {
    /// 空键（整行）。
    pub const fn whole() -> Key {
        Key { f1: 0, c1: 0, f2: 0, c2: 0, has_end: false }
    }

    /// 解析一个键规格。
    pub fn parse(raw: &[u8]) -> Result<Key, u32> {
        if raw.is_empty() {
            return Err(SR_ESHORT);
        }
        let mut comma = raw.len();
        let mut i = 0usize;
        while i < raw.len() {
            if let Some(b) = raw.get(i).copied() {
                if b == b',' {
                    comma = i;
                    break;
                }
            }
            i += 1;
        }
        let left = raw.get(0..comma).unwrap_or(&[]);
        let right = if comma < raw.len() { raw.get(comma + 1..).unwrap_or(&[]) } else { &[] };
        let (f1, c1) = parse_pos(left)?;
        if comma == raw.len() {
            return Ok(Key { f1, c1, f2: f1, c2: 0, has_end: false });
        }
        if right.is_empty() {
            return Err(SR_ESHORT);
        }
        let (f2, c2) = parse_pos(right)?;
        if f2 < f1 {
            return Err(SR_ERANGE);
        }
        Ok(Key { f1, c1, f2, c2, has_end: true })
    }

    /// 渲染。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.f1 as u64);
        if self.c1 > 0 {
            s.push('.');
            push_dec(&mut s, self.c1 as u64);
        }
        if self.has_end {
            s.push(',');
            push_dec(&mut s, self.f2 as u64);
            if self.c2 > 0 {
                s.push('.');
                push_dec(&mut s, self.c2 as u64);
            }
        }
        s
    }
}

/// 解析 `F[.C]`。
fn parse_pos(raw: &[u8]) -> Result<(u16, u16), u32> {
    if raw.is_empty() {
        return Err(SR_ESHORT);
    }
    let mut dot = raw.len();
    let mut i = 0usize;
    while i < raw.len() {
        if let Some(b) = raw.get(i).copied() {
            if b == b'.' {
                dot = i;
                break;
            }
        }
        i += 1;
    }
    let fpart = raw.get(0..dot).unwrap_or(&[]);
    let cpart = if dot < raw.len() { raw.get(dot + 1..).unwrap_or(&[]) } else { &[] };
    let f = parse_u16(fpart)?;
    if f == 0 {
        return Err(SR_ERANGE);
    }
    if f > FIELD_MAX {
        return Err(SR_ERANGE);
    }
    let c = if cpart.is_empty() {
        if dot < raw.len() {
            return Err(SR_ESHORT);
        }
        0
    } else {
        parse_u16(cpart)?
    };
    Ok((f, c))
}

/// 解析无符号十进制（拒绝空串/非数字/溢出）。
fn parse_u16(raw: &[u8]) -> Result<u16, u32> {
    if raw.is_empty() {
        return Err(SR_ESHORT);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < raw.len() {
        let b = match raw.get(i).copied() {
            Some(v) => v,
            None => return Err(SR_ESHORT),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(SR_EBYTE);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
        if v > 0xffff {
            return Err(SR_ERANGE);
        }
        i += 1;
    }
    Ok(v as u16)
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 逆序。
    pub reverse: bool,
    /// 去重。
    pub unique: bool,
    /// 折叠大小写。
    pub fold: bool,
    /// 数字比较。
    pub numeric: bool,
    /// 跳过键首空白。
    pub blanks: bool,
    /// 末位比较关闭（-s）。
    pub stable_only: bool,
    /// 键。
    pub key: Key,
    /// 分隔符（0 = 缺省空白串）。
    pub sep: u8,
    /// 是否显式给了 -t。
    pub sep_set: bool,
}

impl Opts {
    /// 缺省。
    pub const fn new() -> Opts {
        Opts {
            reverse: false,
            unique: false,
            fold: false,
            numeric: false,
            blanks: false,
            stable_only: false,
            key: Key::whole(),
            sep: 0,
            sep_set: false,
        }
    }

    /// 是否用键。
    pub fn has_key(&self) -> bool {
        self.key.f1 > 0
    }

    /// 渲染。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.reverse {
            s.push_str("-r ");
        }
        if self.unique {
            s.push_str("-u ");
        }
        if self.fold {
            s.push_str("-f ");
        }
        if self.numeric {
            s.push_str("-n ");
        }
        if self.blanks {
            s.push_str("-b ");
        }
        if self.stable_only {
            s.push_str("-s ");
        }
        if self.has_key() {
            s.push_str("-k");
            s.push_str(&self.key.text());
            s.push(' ');
        }
        if self.sep_set {
            s.push_str("-t");
            s.push(self.sep as char);
        }
        while s.ends_with(' ') {
            s.pop();
        }
        s
    }
}

/// 解析命令行（空格分隔）。
pub fn parse_args(raw: &[u8]) -> Result<Opts, u32> {
    let mut o = Opts::new();
    let mut i = 0usize;
    while i < raw.len() {
        while i < raw.len() {
            match raw.get(i).copied() {
                Some(b' ') | Some(b'\t') => i += 1,
                _ => break,
            }
        }
        if i >= raw.len() {
            break;
        }
        let start = i;
        while i < raw.len() {
            match raw.get(i).copied() {
                Some(b' ') | Some(b'\t') => break,
                _ => i += 1,
            }
        }
        let tok = raw.get(start..i).unwrap_or(&[]);
        if tok.len() < 2 || tok.first().copied() != Some(b'-') {
            return Err(SR_EINVAL);
        }
        let flag = match tok.get(1).copied() {
            Some(v) => v,
            None => return Err(SR_ESHORT),
        };
        let rest = tok.get(2..).unwrap_or(&[]);
        match flag {
            b'r' if rest.is_empty() => o.reverse = true,
            b'u' if rest.is_empty() => o.unique = true,
            b'f' if rest.is_empty() => o.fold = true,
            b'n' if rest.is_empty() => o.numeric = true,
            b'b' if rest.is_empty() => o.blanks = true,
            b's' if rest.is_empty() => o.stable_only = true,
            b'k' => {
                if o.has_key() {
                    return Err(SR_ECONF);
                }
                let spec = if rest.is_empty() { next_token(raw, &mut i)? } else { rest };
                o.key = Key::parse(spec)?;
            }
            b't' => {
                if o.sep_set {
                    return Err(SR_EDUP);
                }
                let val = if rest.is_empty() { next_token(raw, &mut i)? } else { rest };
                if val.len() != 1 {
                    return Err(SR_EBYTE);
                }
                let b = val.first().copied().unwrap_or(0);
                if b == b'\n' || b == 0 {
                    return Err(SR_EBYTE);
                }
                o.sep = b;
                o.sep_set = true;
            }
            _ => return Err(SR_EINVAL),
        }
    }
    Ok(o)
}

/// 取下一个空格分隔的记号（借用原文，返回临时切片的辅助）。
fn next_token<'a>(raw: &'a [u8], i: &mut usize) -> Result<&'a [u8], u32> {
    let mut k = *i;
    while k < raw.len() {
        match raw.get(k).copied() {
            Some(b' ') | Some(b'\t') => k += 1,
            _ => break,
        }
    }
    if k >= raw.len() {
        return Err(SR_ESHORT);
    }
    let start = k;
    while k < raw.len() {
        match raw.get(k).copied() {
            Some(b' ') | Some(b'\t') => break,
            _ => k += 1,
        }
    }
    *i = k;
    Ok(raw.get(start..k).unwrap_or(&[]))
}

/// 定容行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl Line {
    /// 空行。
    pub const fn empty() -> Line {
        Line { buf: [0u8; LINE_CAP], len: 0 }
    }

    /// 解析；拒绝 NUL 与换行（它们破坏行边界）。
    pub fn parse(raw: &[u8]) -> Result<Line, u32> {
        if raw.len() > LINE_CAP {
            return Err(SR_ELEN);
        }
        let mut buf = [0u8; LINE_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => return Err(SR_ESHORT),
            };
            if b == 0 || b == b'\n' || b == b'\r' {
                return Err(SR_EBYTE);
            }
            if let Some(d) = buf.get_mut(i) {
                *d = b;
            }
            i += 1;
        }
        Ok(Line { buf, len: raw.len() })
    }

    /// 字节。
    pub fn as_bytes(&self) -> &[u8] {
        let n = if self.len <= LINE_CAP { self.len } else { LINE_CAP };
        match self.buf.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// 是否空行。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let v = self.as_bytes();
        let mut i = 0usize;
        while i < v.len() {
            if let Some(b) = v.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

/// 字段跨度 `[start, end)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起。
    pub start: usize,
    /// 止（不含）。
    pub end: usize,
}

impl Span {
    /// 空跨度。
    pub const fn empty(at: usize) -> Span {
        Span { start: at, end: at }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 取第 n 个字段（一基；0 或越界返回 None）。
///
/// `sep == 0`：缺省「空白串」语义，连续空格/制表算一次切分，前导空白跳过；
/// `sep != 0`：精确按单字节切，空字段也占位。
pub fn field_span(line: &[u8], sep: u8, n: u16) -> Option<Span> {
    if n == 0 || line.is_empty() {
        return None;
    }
    let want = n as usize;
    let mut idx = 0usize;
    let mut i = 0usize;
    if sep == 0 {
        while i < line.len() {
            while i < line.len() {
                match line.get(i).copied() {
                    Some(b' ') | Some(b'\t') => i += 1,
                    _ => break,
                }
            }
            if i >= line.len() {
                return None;
            }
            let start = i;
            while i < line.len() {
                match line.get(i).copied() {
                    Some(b' ') | Some(b'\t') => break,
                    _ => i += 1,
                }
            }
            idx += 1;
            if idx == want {
                return Some(Span { start, end: i });
            }
        }
        None
    } else {
        let mut start = 0usize;
        loop {
            let mut end = start;
            while end < line.len() {
                match line.get(end).copied() {
                    Some(b) => {
                        if b == sep {
                            break;
                        }
                        end += 1;
                    }
                    None => break,
                }
            }
            idx += 1;
            if idx == want {
                return Some(Span { start, end });
            }
            if end >= line.len() {
                return None;
            }
            start = end + 1;
            if start > line.len() {
                return None;
            }
        }
    }
}

/// 某行的键跨度。
pub fn key_span(line: &[u8], o: &Opts) -> Span {
    if !o.has_key() {
        return Span { start: 0, end: line.len() };
    }
    let k = o.key;
    let fs = match field_span(line, o.sep, k.f1) {
        Some(v) => v,
        None => return Span::empty(line.len()),
    };
    let mut start = fs.start;
    if k.c1 > 0 {
        let off = (k.c1 - 1) as usize;
        start = fs.start.saturating_add(off);
        if start > fs.end {
            start = fs.end;
        }
    } else if o.blanks {
        while start < fs.end {
            match line.get(start).copied() {
                Some(b' ') | Some(b'\t') => start += 1,
                _ => break,
            }
        }
    }
    let end = if !k.has_end {
        line.len()
    } else {
        match field_span(line, o.sep, k.f2) {
            Some(fe) => {
                if k.c2 > 0 {
                    let e = fe.start.saturating_add(k.c2 as usize);
                    if e > fe.end {
                        fe.end
                    } else {
                        e
                    }
                } else {
                    fe.end
                }
            }
            None => line.len(),
        }
    };
    let end = if end < start { start } else { end };
    Span { start, end }
}

/// 数字键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Num {
    /// 负数标志（零值恒为非负）。
    pub neg: bool,
    /// 整数部分（饱和）。
    pub int: u64,
    /// 小数部分（补齐到 9 位）。
    pub frac: u64,
    /// 原始小数位数。
    pub fdigits: u8,
    /// 是否解析到数字。
    pub valid: bool,
}

impl Num {
    /// 零。
    pub const fn zero() -> Num {
        Num { neg: false, int: 0, frac: 0, fdigits: 0, valid: false }
    }

    /// 符号（-1/0/1）。
    pub fn sign(&self) -> i8 {
        if self.int == 0 && self.frac == 0 {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    /// 文本化（诊断用）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.neg {
            s.push('-');
        }
        push_dec(&mut s, self.int);
        if self.fdigits > 0 {
            s.push('.');
            push_dec(&mut s, self.frac);
        }
        s
    }
}

/// 解析数字前缀；无数字时返回零值（`valid == false`）。
pub fn parse_num(raw: &[u8]) -> Num {
    let mut i = 0usize;
    while i < raw.len() {
        match raw.get(i).copied() {
            Some(b' ') | Some(b'\t') => i += 1,
            _ => break,
        }
    }
    let mut neg = false;
    if let Some(c) = raw.get(i).copied() {
        if c == b'-' {
            neg = true;
            i += 1;
        } else if c == b'+' {
            i += 1;
        }
    }
    let mut int: u64 = 0;
    let mut any = false;
    while i < raw.len() {
        let b = match raw.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        if !(b'0'..=b'9').contains(&b) {
            break;
        }
        int = int.saturating_mul(10).saturating_add((b - b'0') as u64);
        any = true;
        i += 1;
    }
    let mut frac: u64 = 0;
    let mut fd: u8 = 0;
    if raw.get(i).copied() == Some(b'.') {
        i += 1;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => break,
            };
            if !(b'0'..=b'9').contains(&b) {
                break;
            }
            any = true;
            if fd < FRAC_DIGITS {
                frac = frac.saturating_mul(10).saturating_add((b - b'0') as u64);
                fd += 1;
            }
            i += 1;
        }
    }
    if !any {
        return Num::zero();
    }
    let mut scaled = frac;
    let mut pad = FRAC_DIGITS - fd;
    while pad > 0 {
        scaled = scaled.saturating_mul(10);
        pad -= 1;
    }
    Num { neg, int, frac: scaled, fdigits: fd, valid: true }
}

/// 数字比较。
pub fn cmp_num(a: Num, b: Num) -> Ord3 {
    let sa = a.sign();
    let sb = b.sign();
    if sa != sb {
        return if sa < sb { Ord3::Less } else { Ord3::Greater };
    }
    if sa == 0 {
        return Ord3::Equal;
    }
    let mag = {
        if a.int != b.int {
            if a.int < b.int { Ord3::Less } else { Ord3::Greater }
        } else if a.frac != b.frac {
            if a.frac < b.frac { Ord3::Less } else { Ord3::Greater }
        } else {
            Ord3::Equal
        }
    };
    if sa < 0 {
        mag.flip()
    } else {
        mag
    }
}

/// 语料。
#[derive(Clone, Copy, Debug)]
pub struct Corpus {
    lines: [Line; LINES_CAP],
    len: usize,
}

impl Corpus {
    /// 空。
    pub const fn new() -> Corpus {
        Corpus { lines: [Line::empty(); LINES_CAP], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, l: Line) -> Result<(), u32> {
        if self.len >= LINES_CAP {
            return Err(SR_ERANGE);
        }
        if let Some(d) = self.lines.get_mut(self.len) {
            *d = l;
        }
        self.len += 1;
        Ok(())
    }

    /// 按 '\n' 构造（跳过空行，便于命令行用法）。
    pub fn parse(text: &[u8]) -> Result<Corpus, u32> {
        let mut c = Corpus::new();
        let mut start = 0usize;
        let mut i = 0usize;
        while i <= text.len() {
            let at_end = i == text.len();
            let nl = matches!(text.get(i).copied(), Some(b'\n'));
            if at_end || nl {
                if i > start {
                    let raw = text.get(start..i).unwrap_or(&[]);
                    c.push(Line::parse(raw)?)?;
                }
                start = i + 1;
            }
            i += 1;
        }
        Ok(c)
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取行。
    pub fn get(&self, i: usize) -> Option<Line> {
        if i >= self.len {
            return None;
        }
        self.lines.get(i).copied()
    }

    /// 两行比较（按选项）。
    pub fn compare(&self, a: usize, b: usize, o: &Opts) -> Result<Ord3, u32> {
        let la = match self.get(a) {
            Some(v) => v,
            None => return Err(SR_ESLOT),
        };
        let lb = match self.get(b) {
            Some(v) => v,
            None => return Err(SR_ESLOT),
        };
        let ka = key_span(la.as_bytes(), o);
        let kb = key_span(lb.as_bytes(), o);
        let sa = la.as_bytes().get(ka.start..ka.end).unwrap_or(&[]);
        let sb = lb.as_bytes().get(kb.start..kb.end).unwrap_or(&[]);
        let base = if o.numeric {
            cmp_num(parse_num(sa), parse_num(sb))
        } else if o.fold {
            cmp_fold(sa, sb)
        } else {
            cmp_bytes(sa, sb)
        };
        let base = if o.reverse { base.flip() } else { base };
        if base != Ord3::Equal {
            return Ok(base);
        }
        // 末位比较：-s / -u 关闭，缺省按整行字节序兜底（再按原下标由稳定排序保证）。
        if o.stable_only || o.unique {
            return Ok(Ord3::Equal);
        }
        let full = cmp_bytes(la.as_bytes(), lb.as_bytes());
        Ok(if o.reverse { full.flip() } else { full })
    }

    /// 归并排序。
    pub fn sort(&self, o: &Opts) -> Result<Sorted, u32> {
        if self.len == 0 {
            return Err(SR_ESHORT);
        }
        let mut a = [0u8; LINES_CAP];
        let mut b = [0u8; LINES_CAP];
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = a.get_mut(i) {
                *d = i as u8;
            }
            i += 1;
        }
        let mut width = 1usize;
        let mut cmp = 0usize;
        while width < self.len {
            let mut lo = 0usize;
            while lo < self.len {
                let mid = if lo + width < self.len { lo + width } else { self.len };
                let hi = if lo + 2 * width < self.len { lo + 2 * width } else { self.len };
                let mut x = lo;
                let mut y = mid;
                let mut k = lo;
                while k < hi {
                    let take_left = if x >= mid {
                        false
                    } else if y >= hi {
                        true
                    } else {
                        let li = a.get(x).copied().unwrap_or(0) as usize;
                        let ri = a.get(y).copied().unwrap_or(0) as usize;
                        let c = self.compare(li, ri, o)?;
                        cmp += 1;
                        c != Ord3::Greater
                    };
                    let v = if take_left {
                        let v = a.get(x).copied().unwrap_or(0);
                        x += 1;
                        v
                    } else {
                        let v = a.get(y).copied().unwrap_or(0);
                        y += 1;
                        v
                    };
                    if let Some(d) = b.get_mut(k) {
                        *d = v;
                    }
                    k += 1;
                }
                lo = hi;
            }
            let mut t = 0usize;
            while t < self.len {
                if let (Some(d), Some(s)) = (a.get_mut(t), b.get(t).copied()) {
                    *d = s;
                }
                t += 1;
            }
            width *= 2;
        }
        let mut dups = 0usize;
        let mut stable_ok = true;
        let mut k = 1usize;
        while k < self.len {
            let p = a.get(k - 1).copied().unwrap_or(0) as usize;
            let c = a.get(k).copied().unwrap_or(0) as usize;
            if self.compare(p, c, o)? == Ord3::Equal && !o.stable_only {
                dups += 1;
            }
            if self.compare(p, c, o)? == Ord3::Equal && p > c {
                stable_ok = false;
            }
            k += 1;
        }
        Ok(Sorted { idx: a, len: self.len, dups, comparisons: cmp, stable_ok })
    }

    /// 已排序检查：首对逆序即失败。
    pub fn check(&self, o: &Opts) -> Result<(), u32> {
        if self.len == 0 {
            return Err(SR_ESHORT);
        }
        let mut i = 1usize;
        while i < self.len {
            if self.compare(i - 1, i, o)? == Ord3::Greater {
                return Err(SR_ESTATE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 逆序对数（相邻）。
    pub fn violations(&self, o: &Opts) -> usize {
        let mut n = 0usize;
        let mut i = 1usize;
        while i < self.len {
            if self.compare(i - 1, i, o) == Ok(Ord3::Greater) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 渲染原语料。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            newline(&mut s);
            if let Some(l) = self.lines.get(i) {
                s.push_str(&l.text());
            }
            i += 1;
        }
        s
    }
}

/// 排序结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sorted {
    /// 下标排列。
    pub idx: [u8; LINES_CAP],
    /// 长度。
    pub len: usize,
    /// 比较相等的相邻对数。
    pub dups: usize,
    /// 比较次数。
    pub comparisons: usize,
    /// 稳定性自检。
    pub stable_ok: bool,
}

impl Sorted {
    /// 空。
    pub const fn empty() -> Sorted {
        Sorted { idx: [0u8; LINES_CAP], len: 0, dups: 0, comparisons: 0, stable_ok: true }
    }

    /// 第 k 个原下标。
    pub fn at(&self, k: usize) -> Option<u8> {
        if k >= self.len {
            return None;
        }
        self.idx.get(k).copied()
    }

    /// 输出条数（-u 去掉重复）。
    pub fn out_len(&self, o: &Opts) -> usize {
        if o.unique {
            self.len.saturating_sub(self.dups)
        } else {
            self.len
        }
    }

    /// 渲染排序输出。
    pub fn render(&self, c: &Corpus, o: &Opts) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            let take = if o.unique && i > 0 {
                let p = self.idx.get(i - 1).copied().unwrap_or(0) as usize;
                let q = self.idx.get(i).copied().unwrap_or(0) as usize;
                c.compare(p, q, o) != Ok(Ord3::Equal)
            } else {
                true
            };
            if take {
                newline(&mut s);
                if let Some(l) = c.get(self.idx.get(i).copied().unwrap_or(0) as usize) {
                    s.push_str(&l.text());
                }
            }
            i += 1;
        }
        s
    }
}

/// 基准五行：故意混入大写、小写、数字。
pub fn base_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"B")?)?;
    c.push(Line::parse(b"a")?)?;
    c.push(Line::parse(b"A")?)?;
    c.push(Line::parse(b"10")?)?;
    c.push(Line::parse(b"2")?)?;
    Ok(c)
}

/// 折叠语料。
pub fn fold_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"c")?)?;
    c.push(Line::parse(b"B")?)?;
    c.push(Line::parse(b"a")?)?;
    Ok(c)
}

/// 数字语料。
pub fn num_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"10")?)?;
    c.push(Line::parse(b"2")?)?;
    c.push(Line::parse(b"1")?)?;
    Ok(c)
}

/// 字段语料。
pub fn field_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"x:3")?)?;
    c.push(Line::parse(b"y:1")?)?;
    c.push(Line::parse(b"z:2")?)?;
    Ok(c)
}

/// 缺省排序首行探针。
fn probe_base_sort() -> Result<u32, u32> {
    let c = base_corpus()?;
    let s = c.sort(&Opts::new())?;
    s.at(0).map(|v| v as u32).ok_or(SR_ESLOT)
}

/// 选项探针。
fn probe_opts() -> Result<u32, u32> {
    let o = parse_args(b"-n -r -k2,2 -t:")?;
    let mut n = 0u32;
    if o.numeric {
        n += 1;
    }
    if o.reverse {
        n += 1;
    }
    if o.has_key() && o.key.f1 == 2 && o.key.f2 == 2 {
        n += 1;
    }
    if o.sep == b':' {
        n += 1;
    }
    Ok(n)
}

/// 逆序探针。
fn probe_rev() -> Result<u32, u32> {
    let c = base_corpus()?;
    let mut o = Opts::new();
    o.reverse = true;
    let s = c.sort(&o)?;
    s.at(0).map(|v| v as u32).ok_or(SR_ESLOT)
}

/// 折叠探针。
fn probe_fold() -> Result<u32, u32> {
    let c = fold_corpus()?;
    let mut o = Opts::new();
    o.fold = true;
    let s = c.sort(&o)?;
    s.at(0).map(|v| v as u32).ok_or(SR_ESLOT)
}

/// 数字探针。
fn probe_num_sort() -> Result<u32, u32> {
    let c = num_corpus()?;
    let mut o = Opts::new();
    o.numeric = true;
    let s = c.sort(&o)?;
    s.at(0).map(|v| v as u32).ok_or(SR_ESLOT)
}

/// 键探针。
fn probe_key() -> Result<u32, u32> {
    let c = field_corpus()?;
    let o = parse_args(b"-t: -k2,2 -n")?;
    let s = c.sort(&o)?;
    s.at(0).map(|v| v as u32).ok_or(SR_ESLOT)
}

/// 去重探针。
fn probe_uniq() -> Result<u32, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"A")?)?;
    c.push(Line::parse(b"a")?)?;
    let o = parse_args(b"-f -u")?;
    let s = c.sort(&o)?;
    Ok(s.dups as u32)
}

/// 检查探针。
fn probe_check() -> Result<u32, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"b")?)?;
    c.push(Line::parse(b"a")?)?;
    Ok(c.violations(&Opts::new()) as u32)
}

/// 数字解析探针。
fn probe_parse() -> Result<u32, u32> {
    let n = parse_num(b"-42.50");
    if n.valid && n.neg {
        Ok(n.int as u32)
    } else {
        Err(SR_EINVAL)
    }
}

/// 未知选项探针。
fn probe_badopt() -> Result<u32, u32> {
    parse_args(b"-Q").map(|o| o.text().len() as u32)
}

/// 字段 0 探针。
fn probe_field0() -> Result<u32, u32> {
    Key::parse(b"0").map(|k| k.f1 as u32)
}

/// 缺参数探针。
fn probe_missing() -> Result<u32, u32> {
    parse_args(b"-t").map(|o| o.text().len() as u32)
}

/// 多字节分隔符探针。
fn probe_sep() -> Result<u32, u32> {
    parse_args(b"-t ab").map(|o| o.text().len() as u32)
}

/// 行超长探针。
fn probe_longline() -> Result<u32, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(&[b'x'; LINE_CAP + 1])?).map(|_| 1)
}

/// 检查失败探针。
fn probe_unsorted() -> Result<u32, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"b")?)?;
    c.push(Line::parse(b"a")?)?;
    c.check(&Opts::new()).map(|_| 1)
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
            return Err(SR_ESLOT);
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

    /// 取格。
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
            return Err(SR_EBAL);
        }
        const VALS: [u32; 8] = [3, 1, 2, 2, 1, 1, 1, 42];
        const CODES: [u32; 6] = [SR_EINVAL, SR_ERANGE, SR_ESHORT, SR_EBYTE, SR_ELEN, SR_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SR_EBAL),
            };
            if s.slot as usize != i {
                return Err(SR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SR_EVALUE);
                        }
                    }
                    None => return Err(SR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SR_EVALUE);
                        }
                    }
                    None => return Err(SR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SR_EBAL);
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
            return Err(SR_EBAL);
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
            Err(SR_ESEAL)
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

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(word_cell(0, OP_SORT, probe_base_sort()));
    let _ = l.register(word_cell(1, OP_REV, probe_rev()));
    let _ = l.register(word_cell(2, OP_FOLD, probe_fold()));
    let _ = l.register(word_cell(3, OP_NUM, probe_num_sort()));
    let _ = l.register(word_cell(4, OP_KEY, probe_key()));
    let _ = l.register(word_cell(5, OP_UNIQ, probe_uniq()));
    let _ = l.register(word_cell(6, OP_CHECK, probe_check()));
    let _ = l.register(word_cell(7, OP_PARSE, probe_parse()));
    let _ = l.register(word_cell(8, OP_SORT, probe_badopt()));
    let _ = l.register(word_cell(9, OP_SORT, probe_field0()));
    let _ = l.register(word_cell(10, OP_SORT, probe_missing()));
    let _ = l.register(word_cell(11, OP_SORT, probe_sep()));
    let _ = l.register(word_cell(12, OP_SORT, probe_longline()));
    let _ = l.register(word_cell(13, OP_CHECK, probe_unsorted()));
    l
}

/// 已排序结果的相邻有序性自检（用于注入路径）。
fn sorted_adjacent(c: &Corpus, s: &Sorted, o: &Opts) -> bool {
    let mut i = 1usize;
    while i < s.len {
        let p = match s.at(i - 1) {
            Some(v) => v as usize,
            None => return false,
        };
        let q = match s.at(i) {
            Some(v) => v as usize,
            None => return false,
        };
        match c.compare(p, q, o) {
            Ok(Ord3::Greater) => return false,
            Ok(_) => {}
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => Corpus::new(),
        };
        let o = Opts::new();
        match c.sort(&o) {
            Ok(s) => {
                s.len == 5
                    && s.at(0) == Some(3)
                    && s.at(1) == Some(4)
                    && s.at(2) == Some(2)
                    && s.at(3) == Some(0)
                    && s.at(4) == Some(1)
                    && s.stable_ok
                    && sorted_adjacent(&c, &s, &o)
            }
            Err(_) => false,
        }
    };

    out[1] = {
        let c = match num_corpus() {
            Ok(v) => v,
            Err(_) => Corpus::new(),
        };
        let o = match parse_args(b"-n") {
            Ok(v) => v,
            Err(_) => Opts::new(),
        };
        let s = match c.sort(&o) {
            Ok(v) => v,
            Err(_) => Sorted::empty(),
        };
        let a = parse_num(b"-1.5");
        let b = parse_num(b"-1.25");
        let c1 = cmp_num(a, b) == Ord3::Less;
        let d = cmp_num(parse_num(b"2"), parse_num(b"10")) == Ord3::Less;
        let e = cmp_num(parse_num(b"abc"), parse_num(b"0")) == Ord3::Equal;
        s.at(0) == Some(2) && s.at(2) == Some(0) && c1 && d && e
    };

    out[2] = {
        let mut l = build_ledger();
        let sealed = l.seal();
        let replayed = match l.replay() {
            Ok(v) => v == sealed,
            Err(_) => false,
        };
        let frozen = l.freeze().is_ok();
        replayed && frozen && l.is_frozen() && l.audit().is_ok()
    };

    out[3] = {
        let c = match field_corpus() {
            Ok(v) => v,
            Err(_) => Corpus::new(),
        };
        let o = match parse_args(b"-t: -k2,2 -n") {
            Ok(v) => v,
            Err(_) => Opts::new(),
        };
        let a = key_span(b"x:3", &o) == Span { start: 2, end: 3 };
        let b = key_span(b"x:33", &o) == Span { start: 2, end: 4 };
        let d = key_span(b"x", &o) == Span::empty(1);
        let s = match c.sort(&o) {
            Ok(v) => v,
            Err(_) => Sorted::empty(),
        };
        let e = s.at(0) == Some(1) && s.at(1) == Some(2) && s.at(2) == Some(0);
        let f = o.text() == String::from("-n -k2,2 -t:");
        a && b && d && e && f
    };

    out
}

/// 判据集合。
pub fn run_unx11564_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11564.sortcollate");

    s.add(
        "SR01-字节序比较",
        {
            let a = cmp_bytes(b"", b"") == Ord3::Equal;
            let b = cmp_bytes(b"", b"a") == Ord3::Less;
            let c = cmp_bytes(b"a", b"") == Ord3::Greater;
            let d = cmp_bytes(b"10", b"2") == Ord3::Less;
            let e = cmp_bytes(b"B", b"a") == Ord3::Less;
            let f = cmp_bytes(b"a", b"ab") == Ord3::Less;
            let g = cmp_bytes(&[0x7f], &[0x80]) == Ord3::Less;
            let h = cmp_bytes(&[0xff], &[0x00]) == Ord3::Greater;
            let i = cmp_bytes(b"abc", b"abc") == Ord3::Equal;
            let j = Ord3::Less.flip() == Ord3::Greater && Ord3::Equal.flip() == Ord3::Equal;
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "SR02-选项解析",
        {
            let a = parse_args(b"-r").map(|o| o.reverse) == Ok(true);
            let b = parse_args(b"-u -f").map(|o| o.unique && o.fold) == Ok(true);
            let c = parse_args(b"-n -b").map(|o| o.numeric && o.blanks) == Ok(true);
            let d = parse_args(b"-Q").err() == Some(SR_EINVAL);
            let e = parse_args(b"-k1 -k2").err() == Some(SR_ECONF);
            let f = parse_args(b"-t: -t,").err() == Some(SR_EDUP);
            let g = parse_args(b"-t").err() == Some(SR_ESHORT);
            let h = parse_args(b"-t ab").err() == Some(SR_EBYTE);
            let i = parse_args(b"-k").err() == Some(SR_ESHORT);
            let j = parse_args(b"nokey").err() == Some(SR_EINVAL);
            let k = parse_args(b"-k2,2 -n").map(|o| o.key.f1 == 2 && o.key.has_end) == Ok(true);
            let l = parse_args(b"-t:").map(|o| o.sep == b':' && o.sep_set) == Ok(true);
            a && b && c && d && e && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "SR03-键规格解析",
        {
            let a = Key::parse(b"2").map(|k| (k.f1, k.c1, k.has_end)) == Ok((2, 0, false));
            let b = Key::parse(b"2,2").map(|k| (k.f1, k.f2, k.has_end)) == Ok((2, 2, true));
            let c = Key::parse(b"1.2,1.4").map(|k| (k.f1, k.c1, k.f2, k.c2)) == Ok((1, 2, 1, 4));
            let d = Key::parse(b"0").err() == Some(SR_ERANGE);
            let e = Key::parse(b"0,2").err() == Some(SR_ERANGE);
            let f = Key::parse(b"2,1").err() == Some(SR_ERANGE);
            let g = Key::parse(b"").err() == Some(SR_ESHORT);
            let h = Key::parse(b"2,").err() == Some(SR_ESHORT);
            let i = Key::parse(b"2.").err() == Some(SR_ESHORT);
            let j = Key::parse(b"x").err() == Some(SR_EBYTE);
            let k = Key::parse(b"3,2").err() == Some(SR_ERANGE);
            let l = Key::parse(b"2.3,2.5").map(|k| k.text()) == Ok(String::from("2.3,2.5"));
            a && b && c && d && e && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "SR04-字段切分",
        {
            let a = field_span(b"a b c", 0, 1) == Some(Span { start: 0, end: 1 });
            let b = field_span(b"a b c", 0, 2) == Some(Span { start: 2, end: 3 });
            let c = field_span(b"a b c", 0, 3) == Some(Span { start: 4, end: 5 });
            let d = field_span(b"a b c", 0, 4).is_none();
            let e = field_span(b"  a   b ", 0, 1) == Some(Span { start: 2, end: 3 });
            let f = field_span(b"  a   b ", 0, 2) == Some(Span { start: 6, end: 7 });
            let g = field_span(b"a::b", b':', 1) == Some(Span { start: 0, end: 1 });
            let h = field_span(b"a::b", b':', 2) == Some(Span { start: 2, end: 2 });
            let i = field_span(b"a::b", b':', 3) == Some(Span { start: 3, end: 4 });
            let j = field_span(b"a::b", b':', 4).is_none();
            let k = field_span(b"", 0, 1).is_none();
            let l = field_span(b":x", b':', 1) == Some(Span { start: 0, end: 0 });
            a && b && c && d && e && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "SR05-键提取",
        {
            let o = Opts::new();
            let a = key_span(b"hello", &o) == Span { start: 0, end: 5 };
            let o2 = match parse_args(b"-k2") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let b = key_span(b"aa bb cc", &o2) == Span { start: 3, end: 8 };
            let o3 = match parse_args(b"-k2,2") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let c = key_span(b"aa bb cc", &o3) == Span { start: 3, end: 5 };
            let d = key_span(b"aa", &o3) == Span::empty(2);
            let o4 = match parse_args(b"-t: -k1.2,1.3") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let e = key_span(b"abc:def", &o4) == Span { start: 1, end: 3 };
            let o5 = match parse_args(b"-b -k1") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let f = key_span(b"   abc", &o5) == Span { start: 3, end: 6 };
            let o6 = match parse_args(b"-k9") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let g = key_span(b"short", &o6) == Span::empty(5);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SR06-数字解析",
        {
            let a = parse_num(b"42") == Num {
                neg: false,
                int: 42,
                frac: 0,
                fdigits: 0,
                valid: true,
            };
            let b = parse_num(b"-42.50");
            let c = b.neg && b.int == 42 && b.fdigits == 2;
            let d = parse_num(b"+7").int == 7 && parse_num(b"+7").valid;
            let e = parse_num(b" 12").int == 12 && parse_num(b" 12").valid;
            let f = parse_num(b"abc").valid == false;
            let g = parse_num(b"-").valid == false;
            let h = parse_num(b"").valid == false;
            let i = parse_num(b"9999999999999999999999").int == u64::MAX;
            let j = parse_num(b"0.5").frac == 500_000_000;
            let k = parse_num(b"-0").sign() == 0;
            a && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "SR07-数字比较",
        {
            let a = cmp_num(parse_num(b"2"), parse_num(b"10")) == Ord3::Less;
            let b = cmp_num(parse_num(b"-2"), parse_num(b"1")) == Ord3::Less;
            let c = cmp_num(parse_num(b"-10"), parse_num(b"-2")) == Ord3::Less;
            let d = cmp_num(parse_num(b"1.5"), parse_num(b"1.25")) == Ord3::Greater;
            let e = cmp_num(parse_num(b"1.50"), parse_num(b"1.5")) == Ord3::Equal;
            let f = cmp_num(parse_num(b"abc"), parse_num(b"0")) == Ord3::Equal;
            let g = cmp_num(parse_num(b""), parse_num(b"0")) == Ord3::Equal;
            let h = cmp_num(parse_num(b"0"), parse_num(b"-0")) == Ord3::Equal;
            let i = cmp_num(parse_num(b"3"), parse_num(b"3")) == Ord3::Equal;
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "SR08-排序稳定",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let o = Opts::new();
            let s = match c.sort(&o) {
                Ok(v) => v,
                Err(_) => Sorted::empty(),
            };
            let a = s.len == 5 && s.at(0) == Some(3) && s.at(4) == Some(1);
            let b = s.stable_ok && s.at(5).is_none();
            let mut d = Corpus::new();
            let _ = d.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b"a").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let s2 = d.sort(&o).unwrap_or(Sorted::empty());
            let e = s2.at(0) == Some(1) && s2.at(1) == Some(0) && s2.at(2) == Some(2);
            let f = s2.dups == 1;
            let g = Corpus::new().sort(&o).err() == Some(SR_ESHORT);
            a && b && e && f && g
        },
        "static",
    );

    s.add(
        "SR09-逆序与去重",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let o = match parse_args(b"-r") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let s = match c.sort(&o) {
                Ok(v) => v,
                Err(_) => Sorted::empty(),
            };
            let a = s.at(0) == Some(1) && s.at(4) == Some(3);
            let b = s.stable_ok;
            let mut d = Corpus::new();
            let _ = d.push(Line::parse(b"A").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b"a").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let ou = match parse_args(b"-f -u") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let su = d.sort(&ou).unwrap_or(Sorted::empty());
            let e = su.dups == 1 && su.out_len(&ou) == 2;
            let f = su.render(&d, &ou) == String::from("A\nb");
            let g = {
                let plain = Opts::new();
                let sp = d.sort(&plain).unwrap_or(Sorted::empty());
                sp.out_len(&plain) == 3
            };
            a && b && e && f && g
        },
        "static",
    );

    s.add(
        "SR10-折叠与空白",
        {
            let c = match fold_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let of = match parse_args(b"-f") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let sf = c.sort(&of).unwrap_or(Sorted::empty());
            let a = sf.at(0) == Some(2) && sf.at(1) == Some(1) && sf.at(2) == Some(0);
            let plain = c.sort(&Opts::new()).unwrap_or(Sorted::empty());
            let b = plain.at(0) == Some(1) && plain.at(1) == Some(2);
            let ob = match parse_args(b"-b -k1") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let mut d = Corpus::new();
            let _ = d.push(Line::parse(b"  b").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b" a").unwrap_or(Line::empty()));
            let sb = d.sort(&ob).unwrap_or(Sorted::empty());
            let e = sb.at(0) == Some(1);
            let f = key_span(b"  b", &ob) == Span { start: 2, end: 3 };
            a && b && e && f
        },
        "static",
    );

    s.add(
        "SR11-检查模式",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let o = Opts::new();
            let s = c.sort(&o).unwrap_or(Sorted::empty());
            let sorted_corpus = {
                let mut d = Corpus::new();
                let mut k = 0usize;
                while k < s.len {
                    if let Some(l) = c.get(s.at(k).unwrap_or(0) as usize) {
                        let _ = d.push(l);
                    }
                    k += 1;
                }
                d
            };
            let a = sorted_corpus.check(&o).is_ok();
            let b = c.violations(&o) == 2;
            let mut d = Corpus::new();
            let _ = d.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let _ = d.push(Line::parse(b"a").unwrap_or(Line::empty()));
            let e = d.check(&o).err() == Some(SR_ESTATE);
            let f = d.violations(&o) == 1;
            let g = Corpus::new().check(&o).err() == Some(SR_ESHORT);
            a && b && e && f && g
        },
        "static",
    );

    s.add(
        "SR12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES && !l.is_empty();
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = l.sample(0).map(|v| v.value) == Some(3);
            let e = l.sample(8).map(|v| v.code) == Some(SR_EINVAL);
            let f = l.sample(13).map(|v| v.code) == Some(SR_ESTATE);
            let g = l.sample(7).map(|v| v.value) == Some(42);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SR13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut l = build_ledger();
            let sealed = l.seal();
            let a = l.replay().map(|v| v == sealed).unwrap_or(false);
            let b = l.freeze() == Ok(sealed) && l.is_frozen();
            let c = l.register(Sample::blank()).err() == Some(SR_ESLOT) && l.rejects() == 1;
            let d = l.freeze().err() == Some(SR_EBAL) && l.rejects() == 2;
            let mut bad = build_ledger();
            if let Some(dst) = bad.rows.get_mut(2) {
                *dst = Sample { slot: 2, op: OP_FOLD, ok: true, code: 0, value: 1 };
            }
            let e = bad.audit().err() == Some(SR_EVALUE);
            let f = parse_args(b"-n -r").map(|o| o.text()) == Ok(String::from("-r -n"));
            ok = ok && a && b && c && d && e && f;
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
    fn byte_compare() {
        assert_eq!(cmp_bytes(b"a", b"a"), Ord3::Equal);
        assert_eq!(cmp_bytes(b"a", b"b"), Ord3::Less);
        assert_eq!(cmp_bytes(b"ab", b"abc"), Ord3::Less);
        assert_eq!(cmp_bytes(b"B", b"a"), Ord3::Less);
        assert_eq!(cmp_bytes(b"10", b"2"), Ord3::Less);
        assert_eq!(cmp_bytes(&[0xff], &[0x7f]), Ord3::Greater);
    }

    #[test]
    fn opts_flags() {
        let o = parse_args(b"-r -u -f -n -b -s").expect("opts");
        assert!(o.reverse && o.unique && o.fold && o.numeric && o.blanks && o.stable_only);
        assert_eq!(parse_args(b"-X").err(), Some(SR_EINVAL));
        assert_eq!(parse_args(b"--").err(), Some(SR_EINVAL));
    }

    #[test]
    fn opts_conflicts() {
        assert_eq!(parse_args(b"-k1 -k2").err(), Some(SR_ECONF));
        assert_eq!(parse_args(b"-t: -t:").err(), Some(SR_EDUP));
        assert_eq!(parse_args(b"-k5").map(|o| o.key.f1), Ok(5));
    }

    #[test]
    fn key_spec() {
        assert_eq!(Key::parse(b"2").map(|k| k.f1), Ok(2));
        assert_eq!(Key::parse(b"2,3").map(|k| k.has_end), Ok(true));
        assert_eq!(Key::parse(b"1.2,1.4").map(|k| k.c2), Ok(4));
        assert_eq!(Key::parse(b"0").err(), Some(SR_ERANGE));
        assert_eq!(Key::parse(b"3,2").err(), Some(SR_ERANGE));
        assert_eq!(Key::parse(b"1,").err(), Some(SR_ESHORT));
    }

    #[test]
    fn separator_parse() {
        let o = parse_args(b"-t:").expect("opts");
        assert_eq!(o.sep, b':');
        assert!(o.sep_set);
        assert_eq!(parse_args(b"-t ab").err(), Some(SR_EBYTE));
        assert_eq!(parse_args(b"-t").err(), Some(SR_ESHORT));
    }

    #[test]
    fn fields_default() {
        assert_eq!(field_span(b"a b c", 0, 2), Some(Span { start: 2, end: 3 }));
        assert_eq!(field_span(b"a b c", 0, 4), None);
        assert_eq!(field_span(b"  a  b", 0, 1), Some(Span { start: 2, end: 3 }));
    }

    #[test]
    fn fields_exact() {
        assert_eq!(field_span(b"a::b", b':', 2), Some(Span { start: 2, end: 2 }));
        assert_eq!(field_span(b"a::b", b':', 3), Some(Span { start: 3, end: 4 }));
        assert_eq!(field_span(b":x", b':', 1), Some(Span { start: 0, end: 0 }));
    }

    #[test]
    fn key_spans() {
        let o = parse_args(b"-t: -k2,2").expect("opts");
        assert_eq!(key_span(b"x:3", &o), Span { start: 2, end: 3 });
        let o2 = parse_args(b"-k1.2,1.3").expect("opts");
        assert_eq!(key_span(b"abcd", &o2), Span { start: 1, end: 3 });
        let o3 = parse_args(b"-k9").expect("opts");
        assert_eq!(key_span(b"short", &o3), Span::empty(5));
    }

    #[test]
    fn numeric_parse() {
        let n = parse_num(b"-42.50");
        assert!(n.valid && n.neg && n.int == 42 && n.fdigits == 2);
        assert_eq!(parse_num(b"abc").valid, false);
        assert_eq!(parse_num(b"0.5").frac, 500_000_000);
        assert_eq!(parse_num(b"99999999999999999999999").int, u64::MAX);
    }

    #[test]
    fn numeric_order() {
        assert_eq!(cmp_num(parse_num(b"2"), parse_num(b"10")), Ord3::Less);
        assert_eq!(cmp_num(parse_num(b"-10"), parse_num(b"-2")), Ord3::Less);
        assert_eq!(cmp_num(parse_num(b"1.50"), parse_num(b"1.5")), Ord3::Equal);
        assert_eq!(cmp_num(parse_num(b"x"), parse_num(b"0")), Ord3::Equal);
    }

    #[test]
    fn sort_base() {
        let c = base_corpus().expect("c");
        let s = c.sort(&Opts::new()).expect("sort");
        assert_eq!(s.len, 5);
        let expect = [3u8, 4, 2, 0, 1];
        let mut i = 0usize;
        while i < 5 {
            assert_eq!(s.at(i), Some(expect[i]));
            i += 1;
        }
        assert!(s.stable_ok);
    }

    #[test]
    fn sort_reverse() {
        let c = base_corpus().expect("c");
        let o = parse_args(b"-r").expect("o");
        let s = c.sort(&o).expect("sort");
        assert_eq!(s.at(0), Some(1));
        assert_eq!(s.at(4), Some(3));
    }

    #[test]
    fn sort_numeric() {
        let c = num_corpus().expect("c");
        let o = parse_args(b"-n").expect("o");
        let s = c.sort(&o).expect("sort");
        assert_eq!(s.at(0), Some(2));
        assert_eq!(s.at(1), Some(1));
        assert_eq!(s.at(2), Some(0));
    }

    #[test]
    fn sort_by_key() {
        let c = field_corpus().expect("c");
        let o = parse_args(b"-t: -k2,2 -n").expect("o");
        let s = c.sort(&o).expect("sort");
        assert_eq!(s.at(0), Some(1));
        assert_eq!(s.at(1), Some(2));
        assert_eq!(s.at(2), Some(0));
    }

    #[test]
    fn sort_fold_unique() {
        let mut c = Corpus::new();
        c.push(Line::parse(b"A").expect("l")).expect("p");
        c.push(Line::parse(b"a").expect("l")).expect("p");
        let o = parse_args(b"-f -u").expect("o");
        let s = c.sort(&o).expect("sort");
        assert_eq!(s.dups, 1);
        assert_eq!(s.out_len(&o), 1);
        assert_eq!(s.render(&c, &o), String::from("A"));
    }

    #[test]
    fn sort_stability() {
        let mut c = Corpus::new();
        c.push(Line::parse(b"b").expect("l")).expect("p");
        c.push(Line::parse(b"a").expect("l")).expect("p");
        c.push(Line::parse(b"b").expect("l")).expect("p");
        let s = c.sort(&Opts::new()).expect("sort");
        assert_eq!(s.at(0), Some(1));
        assert_eq!(s.at(1), Some(0));
        assert_eq!(s.at(2), Some(2));
        assert!(s.stable_ok);
    }

    #[test]
    fn check_mode() {
        let mut d = Corpus::new();
        d.push(Line::parse(b"b").expect("l")).expect("p");
        d.push(Line::parse(b"a").expect("l")).expect("p");
        assert_eq!(d.check(&Opts::new()).err(), Some(SR_ESTATE));
        assert_eq!(d.violations(&Opts::new()), 1);
        assert_eq!(Corpus::new().check(&Opts::new()).err(), Some(SR_ESHORT));
    }

    #[test]
    fn line_limits() {
        assert_eq!(Line::parse(&[b'x'; LINE_CAP + 1]).err(), Some(SR_ELEN));
        assert_eq!(Line::parse(b"a\nb").err(), Some(SR_EBYTE));
        assert_eq!(Line::parse(b"ok").expect("l").len(), 2);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(6).map(|v| v.value), Some(1));
    }

    #[test]
    fn ledger_seal_freeze() {
        let mut l = build_ledger();
        let s = l.seal();
        assert_eq!(l.replay().expect("replay"), s);
        assert_eq!(l.freeze().expect("freeze"), s);
        assert!(l.is_frozen());
        assert_eq!(l.register(Sample::blank()).err(), Some(SR_ESLOT));
    }

    #[test]
    fn render_output() {
        let c = base_corpus().expect("c");
        let o = Opts::new();
        let s = c.sort(&o).expect("sort");
        assert_eq!(s.render(&c, &o), String::from("10\n2\nA\nB\na"));
    }

    #[test]
    fn inject_all_true() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11564_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }
}
