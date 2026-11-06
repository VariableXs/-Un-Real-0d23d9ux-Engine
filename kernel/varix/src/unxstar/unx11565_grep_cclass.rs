//! UNX-F11565 grep 字符类与区间在 C locale 的字节语义
//!
//! `grep '[a-z]'` 在 C locale 下不是一个「区间表达式」，而是一张 256 位的字节
//! 真值表：区间 `a-z` 就是把 0x61、0x62、……、0x7a 这二十六个位置置一，别的位
//! 一律为零。高位字节 0x80–0xff 不会因为「看起来像字母」而被纳入，字符类
//! `[:alpha:]` 也只认 ASCII 的五十二个码位。把这件事当作文本处理，测试就只能
//! 断言「a 命中、z 命中」，永远测不到 0x7b 为什么**不**命中；本单把它当位图，
//! 于是每一个字节都有明确答案，穷举 256 次即可判定。
//!
//! 三层结构：
//!
//! 1. **字节集 `ByteSet`**。四个 `u64` 共 256 位，区间置位有方向约束（lo>hi
//!    即 `GR_ERANGE`），补集、交、并都是纯位运算，计数就是 `count_ones`。
//!    C ctype 的十二个类由 `in_class` 逐字节定义，类是集合的**来源**而不是
//!    另一套匹配逻辑——`[:alpha:]` 与「手写 52 个字节」必须给出同一张表。
//! 2. **括号表达式 `Bracket`**。解析 `[...]`、前导 `^`、首字符 `]`、转义 `\`、
//!    区间 `a-z`、类 `[:name:]`，拒绝空集、未闭合、倒序区间、未知类名和 NUL。
//!    解析结果仍然是 `ByteSet`，`matches` 只查一位，不存在「解析对了、匹配
//!    另算」的漂移。
//! 3. **行匹配 `Pattern`**。把括号单元、字面字节和 `.` 编成 token，`*` 只修饰
//!    前一个 token（出现在开头即 `GR_EPAT`），`^`/`$` 只在首尾成为锚点，中部
//!    按字面处理。`find` 给出最早起点与在该起点下贪心回溯得到的终点，`^`
//!    把起点锁死为 0，`$` 把终点锁死为行长。
//!
//! 闸门次序：模式字节（`GR_EBYTE`）→ 长度（`GR_ELEN`）→ 空串（`GR_EEMPTY`）
//! → 括号闭合与 token 语法（`GR_EPAT`）→ 类名（`GR_ECLASS`）→ 区间方向
//! （`GR_ERANGE`）→ 选择/标志状态（`GR_ESTATE`/`GR_EFLAG`）→ 账本槽位
//! （`GR_ESLOT`）→ 账本值（`GR_EVALUE`）→ 指纹（`GR_ESEAL`）。前六道都在
//! 解析路径上，后四道只在账本路径上，互不掩盖。
//!
//! 账本语义：十四格 = 八格成功（区间字节数、类字节数、补集字节数、基准语料
//! 命中行数、锚定命中行数、星号跨度、反选行数、模式形状）+ 六格失败（语法、
//! 类名、区间方向、NUL、空串、超长）。`audit` 认「八值六码」，`seal` 做
//! FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf460;

/// 括号或模式语法错误。
pub const GR_EPAT: u32 = SEG_BASE + 0x01;
/// 未知字符类名。
pub const GR_ECLASS: u32 = SEG_BASE + 0x02;
/// 区间方向反转或端点越界。
pub const GR_ERANGE: u32 = SEG_BASE + 0x03;
/// 模式含 NUL 字节（C 字符串提前终止）。
pub const GR_EBYTE: u32 = SEG_BASE + 0x04;
/// 空模式或空括号集。
pub const GR_EEMPTY: u32 = SEG_BASE + 0x05;
/// 模式或行超长。
pub const GR_ELEN: u32 = SEG_BASE + 0x06;
/// grep 状态不成立（未运行即取结果）。
pub const GR_ESTATE: u32 = SEG_BASE + 0x07;
/// 选项标志非法或组合非法。
pub const GR_EFLAG: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const GR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const GR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const GR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const GR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const GR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式原文容量。
pub const PAT_CAP: usize = 48;
/// token 容量。
pub const TOK_CAP: usize = 24;
/// 行容量。
pub const LINE_CAP: usize = 96;
/// 语料行数容量。
pub const LINES_CAP: usize = 16;

/// 操作码：字节区间。
pub const OP_RANGE: u8 = 0x01;
/// 操作码：字符类。
pub const OP_CLASS: u8 = 0x02;
/// 操作码：补集。
pub const OP_NEG: u8 = 0x03;
/// 操作码：普通匹配。
pub const OP_MATCH: u8 = 0x04;
/// 操作码：锚定。
pub const OP_ANCHOR: u8 = 0x05;
/// 操作码：星号。
pub const OP_STAR: u8 = 0x06;
/// 操作码：反选。
pub const OP_INVERT: u8 = 0x07;
/// 操作码：token 形状。
pub const OP_TOKEN: u8 = 0x08;
/// 操作码：坏模式。
pub const OP_BADPAT: u8 = 0x09;
/// 操作码：坏类名。
pub const OP_BADCLASS: u8 = 0x0a;
/// 操作码：坏区间。
pub const OP_BADRANGE: u8 = 0x0b;
/// 操作码：NUL。
pub const OP_NUL: u8 = 0x0c;
/// 操作码：空模式。
pub const OP_EMPTY: u8 = 0x0d;
/// 操作码：超长。
pub const OP_LONG: u8 = 0x0e;

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

/// 取一个字节（越界给 None，绝不索引）。
fn at(raw: &[u8], i: usize) -> Option<u8> {
    raw.get(i).copied()
}

/// 可打印字节转义。
fn push_byte(s: &mut String, b: u8) {
    if b >= 0x20 && b < 0x7f && b != b'\\' {
        s.push(b as char);
        return;
    }
    const HEX: [u8; 16] = *b"0123456789abcdef";
    s.push('\\');
    s.push('x');
    let hi = (b >> 4) & 0x0f;
    let lo = b & 0x0f;
    if let Some(c) = HEX.get(hi as usize) {
        s.push(*c as char);
    }
    if let Some(c) = HEX.get(lo as usize) {
        s.push(*c as char);
    }
}

/// 256 位字节集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteSet {
    w: [u64; 4],
}

impl ByteSet {
    /// 空集。
    pub const fn new() -> ByteSet {
        ByteSet { w: [0u64; 4] }
    }

    /// 全集。
    pub const fn full() -> ByteSet {
        ByteSet { w: [u64::MAX; 4] }
    }

    /// 置一位。
    pub fn set(&mut self, b: u8) {
        let idx = b as usize;
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(w) = self.w.get_mut(word) {
            *w |= 1u64 << bit;
        }
    }

    /// 清一位。
    pub fn clear(&mut self, b: u8) {
        let idx = b as usize;
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(w) = self.w.get_mut(word) {
            *w &= !(1u64 << bit);
        }
    }

    /// 查询一位。
    pub fn has(&self, b: u8) -> bool {
        let idx = b as usize;
        let word = idx / 64;
        let bit = idx % 64;
        match self.w.get(word) {
            Some(w) => (*w & (1u64 << bit)) != 0,
            None => false,
        }
    }

    /// 计数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if let Some(w) = self.w.get(i) {
                n = n.saturating_add(w.count_ones());
            }
            i += 1;
        }
        n
    }

    /// 是否空集。
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// 并集。
    pub fn union(&mut self, other: &ByteSet) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(a), Some(b)) = (self.w.get_mut(i), other.w.get(i)) {
                *a |= *b;
            }
            i += 1;
        }
    }

    /// 交集。
    pub fn intersect(&mut self, other: &ByteSet) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(a), Some(b)) = (self.w.get_mut(i), other.w.get(i)) {
                *a &= *b;
            }
            i += 1;
        }
    }

    /// 差集。
    pub fn difference(&mut self, other: &ByteSet) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(a), Some(b)) = (self.w.get_mut(i), other.w.get(i)) {
                *a &= !*b;
            }
            i += 1;
        }
    }

    /// 补集（全集为 256 字节）。
    pub fn complement(&mut self) {
        let mut i = 0usize;
        while i < 4 {
            if let Some(a) = self.w.get_mut(i) {
                *a = !*a;
            }
            i += 1;
        }
    }

    /// 区间置位；lo>hi 即方向错误。
    pub fn set_range(&mut self, lo: u8, hi: u8) -> Result<(), u32> {
        if lo > hi {
            return Err(GR_ERANGE);
        }
        let mut b = lo as usize;
        let end = hi as usize;
        while b <= end {
            self.set(b as u8);
            if b == end {
                break;
            }
            b += 1;
        }
        Ok(())
    }

    /// 单一字节集合。
    pub fn single(b: u8) -> ByteSet {
        let mut s = ByteSet::new();
        s.set(b);
        s
    }

    /// 从字节串构造（逐字节置位，重复无副作用）。
    pub fn from_bytes(raw: &[u8]) -> ByteSet {
        let mut s = ByteSet::new();
        let mut i = 0usize;
        while i < raw.len() {
            if let Some(b) = raw.get(i).copied() {
                s.set(b);
            }
            i += 1;
        }
        s
    }
}

/// C ctype 的十二个类名（字典序）。
pub const CLASS_NAMES: [&str; 12] = [
    "alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
    "space", "upper", "xdigit",
];

/// 十二个类在 C locale 下的字节数，与 `CLASS_NAMES` 对齐。
pub const CLASS_COUNTS: [u32; 12] = [62, 52, 2, 33, 10, 94, 26, 95, 32, 6, 26, 22];

/// 类名下标。
pub fn class_index(name: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < CLASS_NAMES.len() {
        if let Some(n) = CLASS_NAMES.get(i) {
            if n.as_bytes() == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 某类是否含某字节（C locale，仅 ASCII 有定义）。
pub fn in_class(idx: usize, b: u8) -> bool {
    match idx {
        0 => is_alpha(b) || is_digit(b),
        1 => is_alpha(b),
        2 => b == b' ' || b == b'\t',
        3 => b < 0x20 || b == 0x7f,
        4 => is_digit(b),
        5 => b > 0x20 && b < 0x7f,
        6 => b >= b'a' && b <= b'z',
        7 => b >= 0x20 && b < 0x7f,
        8 => (b >= 0x21 && b <= 0x2f)
            || (b >= 0x3a && b <= 0x40)
            || (b >= 0x5b && b <= 0x60)
            || (b >= 0x7b && b <= 0x7e),
        9 => b == b' ' || (b >= 0x09 && b <= 0x0d),
        10 => b >= b'A' && b <= b'Z',
        11 => is_digit(b) || (b >= b'A' && b <= b'F') || (b >= b'a' && b <= b'f'),
        _ => false,
    }
}

/// ASCII 字母。
fn is_alpha(b: u8) -> bool {
    (b >= b'A' && b <= b'Z') || (b >= b'a' && b <= b'z')
}

/// ASCII 数字。
fn is_digit(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// 按名取类集合。
pub fn class_named(name: &[u8]) -> Result<ByteSet, u32> {
    let idx = match class_index(name) {
        Some(v) => v,
        None => return Err(GR_ECLASS),
    };
    Ok(class_set(idx))
}

/// 按类下标构造集合（逐字节 0..=255 判定）。
pub fn class_set(idx: usize) -> ByteSet {
    let mut s = ByteSet::new();
    let mut i = 0usize;
    while i < 256 {
        let b = i as u8;
        if in_class(idx, b) {
            s.set(b);
        }
        i += 1;
    }
    s
}

/// 括号表达式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bracket {
    /// 字节集（未取反）。
    pub set: ByteSet,
    /// 是否前导 `^`。
    pub negated: bool,
    /// 元素个数。
    pub items: u8,
    /// 区间个数。
    pub ranges: u8,
    /// 字符类个数。
    pub classes: u8,
}

impl Bracket {
    /// 空括号（不可匹配）。
    pub const fn empty() -> Bracket {
        Bracket { set: ByteSet::new(), negated: false, items: 0, ranges: 0, classes: 0 }
    }

    /// 是否匹配某字节（取反在这里生效）。
    pub fn matches(&self, b: u8) -> bool {
        let inside = self.set.has(b);
        if self.negated {
            !inside
        } else {
            inside
        }
    }

    /// 匹配的字节数（取反后）。
    pub fn count(&self) -> u32 {
        if self.negated {
            256u32.saturating_sub(self.set.count())
        } else {
            self.set.count()
        }
    }

    /// 诊断串：`[neg;items;ranges;classes;count]`。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push('[');
        push_dec(&mut s, self.negated as u64);
        s.push(';');
        push_dec(&mut s, self.items as u64);
        s.push(';');
        push_dec(&mut s, self.ranges as u64);
        s.push(';');
        push_dec(&mut s, self.classes as u64);
        s.push(';');
        push_dec(&mut s, self.count() as u64);
        s.push(']');
        s
    }

    /// 解析一个完整的 `[...]`。
    pub fn parse(raw: &[u8]) -> Result<Bracket, u32> {
        if raw.len() < 2 {
            return Err(GR_EPAT);
        }
        if at(raw, 0) != Some(b'[') || at(raw, raw.len() - 1) != Some(b']') {
            return Err(GR_EPAT);
        }
        let mut out = Bracket::empty();
        let mut i = 1usize;
        if at(raw, i) == Some(b'^') {
            out.negated = true;
            i += 1;
        }
        // 首个 `]` 按字面处理（`[]]` 匹配 `]`）。
        if at(raw, i) == Some(b']') && i < raw.len() - 1 {
            out.set.set(b']');
            out.items = out.items.saturating_add(1);
            i += 1;
        }
        let end = raw.len() - 1;
        while i < end {
            let c = match at(raw, i) {
                Some(v) => v,
                None => return Err(GR_EPAT),
            };
            if c == 0 {
                return Err(GR_EBYTE);
            }
            // 字符类 `[:name:]`。
            if c == b'[' && at(raw, i + 1) == Some(b':') {
                let mut k = i + 2;
                let mut close = None;
                while k < end {
                    if at(raw, k) == Some(b':') && at(raw, k + 1) == Some(b']') {
                        close = Some(k);
                        break;
                    }
                    k += 1;
                }
                let ck = match close {
                    Some(v) => v,
                    None => return Err(GR_EPAT),
                };
                let name = raw.get(i + 2..ck).unwrap_or(&[]);
                let cls = class_named(name)?;
                out.set.union(&cls);
                out.classes = out.classes.saturating_add(1);
                out.items = out.items.saturating_add(1);
                i = ck + 2;
                continue;
            }
            // 转义。
            if c == b'\\' {
                let lit = match at(raw, i + 1) {
                    Some(v) => v,
                    None => return Err(GR_EPAT),
                };
                if lit == 0 {
                    return Err(GR_EBYTE);
                }
                out.set.set(lit);
                out.items = out.items.saturating_add(1);
                i += 2;
                continue;
            }
            // 区间 `lo-hi`（`-` 后若直接是闭合 `]` 则为字面）。
            if at(raw, i + 1) == Some(b'-') && i + 2 < end {
                let hi = match at(raw, i + 2) {
                    Some(v) => v,
                    None => return Err(GR_EPAT),
                };
                if hi == 0 {
                    return Err(GR_EBYTE);
                }
                if hi == b']' && i + 2 == end {
                    out.set.set(c);
                    out.items = out.items.saturating_add(1);
                    i += 1;
                    continue;
                }
                out.set.set_range(c, hi)?;
                out.ranges = out.ranges.saturating_add(1);
                out.items = out.items.saturating_add(1);
                i += 3;
                continue;
            }
            out.set.set(c);
            out.items = out.items.saturating_add(1);
            i += 1;
        }
        if out.items == 0 {
            return Err(GR_EEMPTY);
        }
        Ok(out)
    }
}

/// 模式原子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Atom {
    /// 字面字节。
    Lit(u8),
    /// 括号字节集。
    Set(Bracket),
    /// `.`：除换行外任意字节。
    Any,
}

impl Atom {
    /// 单字节是否命中。
    pub fn matches(&self, b: u8) -> bool {
        match self {
            Atom::Lit(c) => *c == b,
            Atom::Set(s) => s.matches(b),
            Atom::Any => b != b'\n',
        }
    }

    /// 诊断串。
    pub fn text(&self) -> String {
        match self {
            Atom::Lit(b) => {
                let mut s = String::new();
                push_byte(&mut s, *b);
                s
            }
            Atom::Set(br) => br.summary(),
            Atom::Any => String::from("."),
        }
    }
}

/// 一个 token：原子 + 是否带 `*`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// 原子。
    pub atom: Atom,
    /// 是否 `*`。
    pub star: bool,
}

impl Token {
    /// 空白 token。
    pub const fn blank() -> Token {
        Token { atom: Atom::Lit(0), star: false }
    }
}

/// token 是否命中。
fn atom_matches(a: &Atom, b: u8) -> bool {
    a.matches(b)
}

/// 解析后的模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pattern {
    toks: [Token; TOK_CAP],
    len: usize,
    anchored_start: bool,
    anchored_end: bool,
}

impl Pattern {
    /// 空模式（无 token、无锚点）。
    pub const fn empty() -> Pattern {
        Pattern {
            toks: [Token::blank(); TOK_CAP],
            len: 0,
            anchored_start: false,
            anchored_end: false,
        }
    }

    /// token 数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否无 token。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否有行首锚。
    pub fn anchored_start(&self) -> bool {
        self.anchored_start
    }

    /// 是否有行尾锚。
    pub fn anchored_end(&self) -> bool {
        self.anchored_end
    }

    /// 第 i 个 token。
    pub fn token(&self, i: usize) -> Option<Token> {
        if i >= self.len {
            return None;
        }
        self.toks.get(i).copied()
    }

    /// 解析模式。
    pub fn parse(raw: &[u8]) -> Result<Pattern, u32> {
        if raw.is_empty() {
            return Err(GR_EEMPTY);
        }
        if raw.len() > PAT_CAP {
            return Err(GR_ELEN);
        }
        let mut z = 0usize;
        while z < raw.len() {
            if at(raw, z) == Some(0) {
                return Err(GR_EBYTE);
            }
            z += 1;
        }
        let mut p = Pattern::empty();
        let mut start = 0usize;
        if at(raw, 0) == Some(b'^') {
            p.anchored_start = true;
            start = 1;
        }
        let mut end = raw.len();
        // 末尾 `$` 仅在未被转义时成为锚点。
        if raw.len() > 0 && at(raw, raw.len() - 1) == Some(b'$') {
            let mut bs = 0usize;
            let mut k = raw.len() - 1;
            while k > 0 {
                if at(raw, k - 1) == Some(b'\\') {
                    bs += 1;
                    k -= 1;
                } else {
                    break;
                }
            }
            if bs % 2 == 0 {
                p.anchored_end = true;
                end = raw.len() - 1;
            }
        }
        if start > end {
            start = end;
        }
        let mut i = start;
        while i < end {
            let c = match at(raw, i) {
                Some(v) => v,
                None => return Err(GR_EPAT),
            };
            if c == b'*' {
                return Err(GR_EPAT);
            }
            let atom: Atom;
            if c == b'\\' {
                let lit = match at(raw, i + 1) {
                    Some(v) => v,
                    None => return Err(GR_EPAT),
                };
                if i + 1 >= end {
                    return Err(GR_EPAT);
                }
                atom = Atom::Lit(lit);
                i += 2;
            } else if c == b'[' {
                let close = match bracket_end(raw, i) {
                    Some(v) => v,
                    None => return Err(GR_EPAT),
                };
                if close >= end {
                    return Err(GR_EPAT);
                }
                let sub = raw.get(i..=close).unwrap_or(&[]);
                atom = Atom::Set(Bracket::parse(sub)?);
                i = close + 1;
            } else if c == b'.' {
                atom = Atom::Any;
                i += 1;
            } else {
                atom = Atom::Lit(c);
                i += 1;
            }
            let mut star = false;
            if i < end && at(raw, i) == Some(b'*') {
                star = true;
                i += 1;
            }
            if p.len >= TOK_CAP {
                return Err(GR_ELEN);
            }
            if let Some(dst) = p.toks.get_mut(p.len) {
                *dst = Token { atom, star };
            }
            p.len += 1;
        }
        Ok(p)
    }

    /// 在 `start` 处尝试匹配，返回终点。
    fn match_from(&self, ti: usize, li: usize, line: &[u8]) -> Option<usize> {
        if ti >= self.len {
            if self.anchored_end {
                return if li == line.len() { Some(li) } else { None };
            }
            return Some(li);
        }
        let t = match self.toks.get(ti) {
            Some(v) => *v,
            None => return None,
        };
        if t.star {
            let mut max = li;
            while max < line.len() {
                let b = match line.get(max).copied() {
                    Some(v) => v,
                    None => break,
                };
                if atom_matches(&t.atom, b) {
                    max += 1;
                } else {
                    break;
                }
            }
            let mut k = max;
            loop {
                if let Some(e) = self.match_from(ti + 1, k, line) {
                    return Some(e);
                }
                if k == li {
                    break;
                }
                k -= 1;
            }
            None
        } else if li < line.len() {
            let b = match line.get(li).copied() {
                Some(v) => v,
                None => return None,
            };
            if atom_matches(&t.atom, b) {
                self.match_from(ti + 1, li + 1, line)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 找最早命中区间 `(start, end)`；允许零长度命中。
    pub fn find(&self, line: &[u8]) -> Option<(usize, usize)> {
        let limit = if self.anchored_start { 0 } else { line.len() };
        let mut start = 0usize;
        while start <= limit {
            if let Some(end) = self.match_from(0, start, line) {
                return Some((start, end));
            }
            start += 1;
        }
        None
    }

    /// 行是否命中。
    pub fn matches_line(&self, line: &[u8]) -> bool {
        self.find(line).is_some()
    }

    /// 诊断串：`^` + token 串 + `$`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.anchored_start {
            s.push('^');
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.toks.get(i) {
                s.push_str(&t.atom.text());
                if t.star {
                    s.push('*');
                }
            }
            i += 1;
        }
        if self.anchored_end {
            s.push('$');
        }
        s
    }
}

/// 找出从 `open` 开始的括号表达式的闭合 `]`。
fn bracket_end(raw: &[u8], open: usize) -> Option<usize> {
    if at(raw, open) != Some(b'[') {
        return None;
    }
    let mut j = open + 1;
    if at(raw, j) == Some(b'^') {
        j += 1;
    }
    if at(raw, j) == Some(b']') {
        // 只有后面还存在闭合 `]` 时才把首个 `]` 当字面。
        let mut k = j + 1;
        let mut later = false;
        while k < raw.len() {
            if at(raw, k) == Some(b']') {
                later = true;
                break;
            }
            k += 1;
        }
        if later {
            j += 1;
        }
    }
    while j < raw.len() {
        let c = match at(raw, j) {
            Some(v) => v,
            None => return None,
        };
        if c == b'\\' {
            j += 2;
            continue;
        }
        if c == b'[' && at(raw, j + 1) == Some(b':') {
            let mut k = j + 2;
            let mut closed = false;
            while k < raw.len() {
                if at(raw, k) == Some(b':') && at(raw, k + 1) == Some(b']') {
                    j = k + 2;
                    closed = true;
                    break;
                }
                k += 1;
            }
            if closed {
                continue;
            }
            return None;
        }
        if c == b']' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// 一行文本。
#[derive(Clone, Copy, Debug)]
pub struct Line {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl Line {
    /// 空行。
    pub const fn empty() -> Line {
        Line { buf: [0u8; LINE_CAP], len: 0 }
    }

    /// 从字节串构造；超长即 `GR_ELEN`。
    pub fn parse(raw: &[u8]) -> Result<Line, u32> {
        if raw.len() > LINE_CAP {
            return Err(GR_ELEN);
        }
        let mut l = Line::empty();
        let mut i = 0usize;
        while i < raw.len() {
            if let (Some(dst), Some(b)) = (l.buf.get_mut(i), raw.get(i)) {
                *dst = *b;
            }
            i += 1;
        }
        l.len = raw.len();
        Ok(l)
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 行长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空行。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i).copied() {
                s.push(b as char);
            }
            i += 1;
        }
        s
    }
}

/// 固定容量语料。
#[derive(Clone, Copy, Debug)]
pub struct Corpus {
    lines: [Line; LINES_CAP],
    len: usize,
}

impl Corpus {
    /// 空语料。
    pub const fn new() -> Corpus {
        Corpus { lines: [Line::empty(); LINES_CAP], len: 0 }
    }

    /// 追加一行；满即 `GR_ERANGE`。
    pub fn push(&mut self, l: Line) -> Result<(), u32> {
        if self.len >= LINES_CAP {
            return Err(GR_ERANGE);
        }
        if let Some(dst) = self.lines.get_mut(self.len) {
            *dst = l;
        }
        self.len += 1;
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 行。
    pub fn get(&self, i: usize) -> Option<&Line> {
        if i >= self.len {
            return None;
        }
        self.lines.get(i)
    }
}

/// grep 标志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags {
    /// `-v` 反选。
    pub invert: bool,
    /// `-c` 只输出计数。
    pub count: bool,
    /// `-n` 行号前缀。
    pub number: bool,
    /// `-q` 静默。
    pub quiet: bool,
}

impl Flags {
    /// 全关。
    pub const fn new() -> Flags {
        Flags { invert: false, count: false, number: false, quiet: false }
    }

    /// 诊断串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.invert {
            s.push_str("-v ");
        }
        if self.count {
            s.push_str("-c ");
        }
        if self.number {
            s.push_str("-n ");
        }
        if self.quiet {
            s.push_str("-q ");
        }
        while s.ends_with(' ') {
            s.pop();
        }
        s
    }

    /// 解析组合标志（空格分隔、可连写）。
    pub fn parse(raw: &[u8]) -> Result<Flags, u32> {
        let mut f = Flags::new();
        let mut i = 0usize;
        while i < raw.len() {
            match at(raw, i) {
                Some(b' ') | Some(b'\t') => {
                    i += 1;
                }
                Some(b'-') => {
                    i += 1;
                    let mut any = false;
                    while i < raw.len() {
                        let c = match at(raw, i) {
                            Some(v) => v,
                            None => break,
                        };
                        if c == b' ' || c == b'\t' {
                            break;
                        }
                        match c {
                            b'v' => f.invert = true,
                            b'c' => f.count = true,
                            b'n' => f.number = true,
                            b'q' => f.quiet = true,
                            _ => return Err(GR_EFLAG),
                        }
                        any = true;
                        i += 1;
                    }
                    if !any {
                        return Err(GR_EFLAG);
                    }
                }
                _ => return Err(GR_EFLAG),
            }
        }
        Ok(f)
    }
}

/// grep 结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepOut {
    /// 命中行数。
    pub selected: usize,
    /// 实际输出行数。
    pub shown: usize,
    /// 渲染文本。
    pub out: String,
    /// 是否静默。
    pub quiet: bool,
}

/// 运行 grep。
pub fn run(p: &Pattern, c: &Corpus, f: Flags) -> GrepOut {
    let mut out = String::new();
    let mut selected = 0usize;
    let mut shown = 0usize;
    let mut i = 0usize;
    while i < c.len() {
        let line = match c.get(i) {
            Some(v) => v,
            None => break,
        };
        let hit = p.matches_line(line.as_bytes());
        let take = if f.invert { !hit } else { hit };
        if take {
            selected = selected.saturating_add(1);
            if !f.quiet && !f.count {
                newline(&mut out);
                if f.number {
                    push_dec(&mut out, (i + 1) as u64);
                    out.push(':');
                }
                out.push_str(&line.text());
                shown = shown.saturating_add(1);
            }
        }
        i += 1;
    }
    if f.count && !f.quiet {
        push_dec(&mut out, selected as u64);
        shown = selected;
    }
    GrepOut { selected, shown, out, quiet: f.quiet }
}

/// 基准语料：大写、小写、数字混合。
pub fn base_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"alpha")?)?;
    c.push(Line::parse(b"Beta")?)?;
    c.push(Line::parse(b"gamma")?)?;
    c.push(Line::parse(b"delta")?)?;
    c.push(Line::parse(b"42")?)?;
    c.push(Line::parse(b"a1b2")?)?;
    Ok(c)
}

/// 区间字节数探针。
fn probe_range_bytes() -> Result<u32, u32> {
    Bracket::parse(b"[a-z]").map(|b| b.count())
}

/// 类字节数探针。
fn probe_class_bytes() -> Result<u32, u32> {
    Bracket::parse(b"[[:digit:]]").map(|b| b.count())
}

/// 补集字节数探针。
fn probe_neg_bytes() -> Result<u32, u32> {
    Bracket::parse(b"[^a-z]").map(|b| b.count())
}

/// 普通命中行数探针。
fn probe_match_count() -> Result<u32, u32> {
    let c = base_corpus()?;
    let p = Pattern::parse(b"[a-z]")?;
    let o = run(&p, &c, Flags::new());
    Ok(o.selected as u32)
}

/// 锚定命中行数探针。
fn probe_anchor_count() -> Result<u32, u32> {
    let c = base_corpus()?;
    let p = Pattern::parse(b"^[a-z]")?;
    let o = run(&p, &c, Flags::new());
    Ok(o.selected as u32)
}

/// 星号跨度探针。
fn probe_star_span() -> Result<u32, u32> {
    let p = Pattern::parse(b"ba*")?;
    match p.find(b"baaa") {
        Some((_, e)) => Ok(e as u32),
        None => Err(GR_ESTATE),
    }
}

/// 反选命中行数探针。
fn probe_invert_count() -> Result<u32, u32> {
    let c = base_corpus()?;
    let p = Pattern::parse(b"[a-z]")?;
    let f = Flags::parse(b"-v")?;
    let o = run(&p, &c, f);
    Ok(o.selected as u32)
}

/// 模式形状探针：token 数 + 两个锚点。
fn probe_shape() -> Result<u32, u32> {
    let p = Pattern::parse(b"^[a-z][0-9]*$")?;
    let mut n = p.len() as u32;
    if p.anchored_start() {
        n = n.saturating_add(1);
    }
    if p.anchored_end() {
        n = n.saturating_add(1);
    }
    Ok(n)
}

/// 坏语法探针。
fn probe_badpat() -> Result<u32, u32> {
    Pattern::parse(b"[abc").map(|p| p.len() as u32)
}

/// 坏类名探针。
fn probe_badclass() -> Result<u32, u32> {
    Pattern::parse(b"[[:nope:]]").map(|p| p.len() as u32)
}

/// 坏区间探针。
fn probe_badrange() -> Result<u32, u32> {
    Bracket::parse(b"[z-a]").map(|b| b.count())
}

/// NUL 探针。
fn probe_nul() -> Result<u32, u32> {
    Pattern::parse(b"a\x00b").map(|p| p.len() as u32)
}

/// 空模式探针。
fn probe_empty() -> Result<u32, u32> {
    Pattern::parse(b"").map(|p| p.len() as u32)
}

/// 超长探针。
fn probe_long() -> Result<u32, u32> {
    let raw = [b'a'; PAT_CAP + 1];
    Pattern::parse(&raw).map(|p| p.len() as u32)
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
            return Err(GR_ESLOT);
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
            return Err(GR_EBAL);
        }
        const VALS: [u32; 8] = [26, 10, 230, 5, 4, 4, 1, 4];
        const CODES: [u32; 6] = [GR_EPAT, GR_ECLASS, GR_ERANGE, GR_EBYTE, GR_EEMPTY, GR_ELEN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(GR_EBAL),
            };
            if s.slot as usize != i {
                return Err(GR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(GR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(GR_EVALUE);
                        }
                    }
                    None => return Err(GR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(GR_EVALUE);
                        }
                    }
                    None => return Err(GR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(GR_EBAL);
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
            return Err(GR_EBAL);
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
            Err(GR_ESEAL)
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
    let _ = l.register(word_cell(0, OP_RANGE, probe_range_bytes()));
    let _ = l.register(word_cell(1, OP_CLASS, probe_class_bytes()));
    let _ = l.register(word_cell(2, OP_NEG, probe_neg_bytes()));
    let _ = l.register(word_cell(3, OP_MATCH, probe_match_count()));
    let _ = l.register(word_cell(4, OP_ANCHOR, probe_anchor_count()));
    let _ = l.register(word_cell(5, OP_STAR, probe_star_span()));
    let _ = l.register(word_cell(6, OP_INVERT, probe_invert_count()));
    let _ = l.register(word_cell(7, OP_TOKEN, probe_shape()));
    let _ = l.register(word_cell(8, OP_BADPAT, probe_badpat()));
    let _ = l.register(word_cell(9, OP_BADCLASS, probe_badclass()));
    let _ = l.register(word_cell(10, OP_BADRANGE, probe_badrange()));
    let _ = l.register(word_cell(11, OP_NUL, probe_nul()));
    let _ = l.register(word_cell(12, OP_EMPTY, probe_empty()));
    let _ = l.register(word_cell(13, OP_LONG, probe_long()));
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let br = match Bracket::parse(b"[a-z]") {
            Ok(v) => v,
            Err(_) => Bracket::empty(),
        };
        let neg = match Bracket::parse(b"[^a-z]") {
            Ok(v) => v,
            Err(_) => Bracket::empty(),
        };
        let mut ok = true;
        let mut i = 0usize;
        while i < 256 {
            let b = i as u8;
            let want = b >= b'a' && b <= b'z';
            if br.matches(b) != want {
                ok = false;
            }
            if neg.matches(b) == want {
                ok = false;
            }
            i += 1;
        }
        ok && br.count() == 26 && neg.count() == 230
    };

    out[1] = {
        let mut ok = true;
        let mut i = 0usize;
        while i < CLASS_NAMES.len() {
            let set = class_set(i);
            if set.count() != CLASS_COUNTS[i] {
                ok = false;
            }
            i += 1;
        }
        // 分区恒等式：alpha+digit=alnum，alnum+punct=graph，graph+blank(空格)=print。
        let alpha = class_set(1);
        let digit = class_set(4);
        let alnum = class_set(0);
        let punct = class_set(8);
        let graph = class_set(5);
        let blank = class_set(2);
        let print = class_set(7);
        let mut u = alpha;
        u.union(&digit);
        if u != alnum {
            ok = false;
        }
        let mut u2 = alnum;
        u2.union(&punct);
        if u2 != graph {
            ok = false;
        }
        let mut sp = ByteSet::single(b' ');
        let space_only = sp;
        sp.set(b'\t');
        if sp != blank {
            ok = false;
        }
        let mut u3 = graph;
        u3.union(&space_only);
        if u3 != print {
            ok = false;
        }
        let mut both = alpha;
        both.intersect(&digit);
        if !both.is_empty() {
            ok = false;
        }
        ok
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
        let mut ok = true;
        let p = match Pattern::parse(b"a*ab") {
            Ok(v) => v,
            Err(_) => Pattern::empty(),
        };
        if p.find(b"aaab") != Some((0, 4)) {
            ok = false;
        }
        let p2 = match Pattern::parse(b"a*b") {
            Ok(v) => v,
            Err(_) => Pattern::empty(),
        };
        if p2.find(b"b") != Some((0, 1)) {
            ok = false;
        }
        let p3 = match Pattern::parse(b"^a*$") {
            Ok(v) => v,
            Err(_) => Pattern::empty(),
        };
        if !p3.matches_line(b"") || !p3.matches_line(b"aaa") || p3.matches_line(b"aab") {
            ok = false;
        }
        let p4 = match Pattern::parse(b"a.c") {
            Ok(v) => v,
            Err(_) => Pattern::empty(),
        };
        if p4.find(b"xabc") != Some((1, 4)) {
            ok = false;
        }
        if p4.find(b"a\nc").is_some() {
            ok = false;
        }
        ok
    };

    out
}

/// 判据集合。
pub fn run_unx11565_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11565.grepcclass");

    s.add(
        "GR01-字符区间逐值",
        {
            let br = match Bracket::parse(b"[a-z]") {
                Ok(v) => v,
                Err(_) => Bracket::empty(),
            };
            let neg = match Bracket::parse(b"[^a-z]") {
                Ok(v) => v,
                Err(_) => Bracket::empty(),
            };
            let up = match Bracket::parse(b"[A-Z]") {
                Ok(v) => v,
                Err(_) => Bracket::empty(),
            };
            let dig = match Bracket::parse(b"[0-9]") {
                Ok(v) => v,
                Err(_) => Bracket::empty(),
            };
            let mut ok = true;
            let mut i = 0usize;
            while i < 256 {
                let b = i as u8;
                let low = b >= 0x61 && b <= 0x7a;
                let high = b >= 0x41 && b <= 0x5a;
                let num = b >= 0x30 && b <= 0x39;
                if br.matches(b) != low {
                    ok = false;
                }
                if neg.matches(b) == low {
                    ok = false;
                }
                if up.matches(b) != high {
                    ok = false;
                }
                if dig.matches(b) != num {
                    ok = false;
                }
                i += 1;
            }
            ok && br.count() == 26 && neg.count() == 230 && up.count() == 26 && dig.count() == 10
        },
        "static",
    );

    s.add(
        "GR02-字符类实况",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CLASS_NAMES.len() {
                let set = class_set(i);
                if set.count() != CLASS_COUNTS[i] {
                    ok = false;
                }
                let mut b = 0usize;
                while b < 256 {
                    if set.has(b as u8) != in_class(i, b as u8) {
                        ok = false;
                    }
                    b += 1;
                }
                i += 1;
            }
            let alpha = class_set(1);
            let digit = class_set(4);
            let alnum = class_set(0);
            let punct = class_set(8);
            let graph = class_set(5);
            let print = class_set(7);
            let lower = class_set(6);
            let upper = class_set(10);
            let xdigit = class_set(11);
            let mut u = alpha;
            u.union(&digit);
            let mut g = alnum;
            g.union(&punct);
            let mut p = graph;
            let space_only = ByteSet::single(b' ');
            p.union(&space_only);
            let mut lu = lower;
            lu.union(&upper);
            let mut xd = digit;
            let mut af = ByteSet::new();
            let mut k = 0usize;
            while k < 6 {
                af.set(b'A' + k as u8);
                af.set(b'a' + k as u8);
                k += 1;
            }
            xd.union(&af);
            let mut cross = alpha;
            cross.intersect(&digit);
            ok && u == alnum
                && g == graph
                && p == print
                && lu == alpha
                && xd == xdigit
                && cross.is_empty()
        },
        "static",
    );

    s.add(
        "GR03-括号语法",
        {
            let a = match Bracket::parse(b"[abc]") {
                Ok(v) => v.matches(b'a') && v.matches(b'b') && v.matches(b'c') && !v.matches(b'd'),
                Err(_) => false,
            };
            let n = match Bracket::parse(b"[^abc]") {
                Ok(v) => v.matches(b'd') && !v.matches(b'a') && v.count() == 253,
                Err(_) => false,
            };
            let rb = match Bracket::parse(b"[]]") {
                Ok(v) => v.matches(b']') && v.count() == 1,
                Err(_) => false,
            };
            let tail = match Bracket::parse(b"[a-]") {
                Ok(v) => v.matches(b'a') && v.matches(b'-') && v.count() == 2,
                Err(_) => false,
            };
            let esc = match Bracket::parse(b"[a\\-z]") {
                Ok(v) => v.matches(b'a') && v.matches(b'-') && v.matches(b'z') && v.count() == 3,
                Err(_) => false,
            };
            let cls = match Bracket::parse(b"[[:alpha:][:digit:]]") {
                Ok(v) => v.count() == 62 && v.classes == 2,
                Err(_) => false,
            };
            let e1 = Bracket::parse(b"[]").err() == Some(GR_EEMPTY);
            let e2 = Bracket::parse(b"[abc").err() == Some(GR_EPAT);
            let e3 = Bracket::parse(b"[z-a]").err() == Some(GR_ERANGE);
            let e4 = Bracket::parse(b"[[:bogus:]]").err() == Some(GR_ECLASS);
            a && n && rb && tail && esc && cls && e1 && e2 && e3 && e4
        },
        "static",
    );

    s.add(
        "GR04-模式解析",
        {
            let p = match Pattern::parse(b"^[a-z][0-9]*$") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let a = p.len() == 2
                && p.anchored_start()
                && p.anchored_end()
                && match p.token(1) {
                    Some(t) => t.star,
                    None => false,
                };
            let b = match Pattern::parse(b"^") {
                Ok(v) => v.len() == 0 && v.anchored_start() && !v.anchored_end(),
                Err(_) => false,
            };
            let c = match Pattern::parse(b"$") {
                Ok(v) => v.len() == 0 && v.anchored_end() && !v.anchored_start(),
                Err(_) => false,
            };
            let d = match Pattern::parse(b"a^b") {
                Ok(v) => {
                    v.len() == 3
                        && v.token(1) == Some(Token { atom: Atom::Lit(b'^'), star: false })
                }
                Err(_) => false,
            };
            let e = match Pattern::parse(b"a$b") {
                Ok(v) => v.len() == 3 && !v.anchored_end(),
                Err(_) => false,
            };
            let f = Pattern::parse(b"*a").err() == Some(GR_EPAT);
            let g = Pattern::parse(b"a**").err() == Some(GR_EPAT);
            let h = Pattern::parse(b"").err() == Some(GR_EEMPTY);
            let i = Pattern::parse(b"\\").err() == Some(GR_EPAT);
            let j = Pattern::parse(b"[a-z").err() == Some(GR_EPAT);
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "GR05-匹配语义",
        {
            let p = match Pattern::parse(b"a*b") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let a = p.find(b"aaab") == Some((0, 4))
                && p.find(b"ab") == Some((0, 2))
                && p.find(b"b") == Some((0, 1))
                && p.find(b"c").is_none();
            let q = match Pattern::parse(b"ba*") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let b = q.find(b"xbaaay") == Some((1, 5));
            let r = match Pattern::parse(b"^a*$") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let c = r.matches_line(b"") && r.matches_line(b"aaa") && !r.matches_line(b"aab");
            let d = match Pattern::parse(b"^$") {
                Ok(v) => v.matches_line(b"") && !v.matches_line(b"x"),
                Err(_) => false,
            };
            let e = match Pattern::parse(b"a.c") {
                Ok(v) => v.find(b"xabc") == Some((1, 4)) && v.find(b"a\nc").is_none(),
                Err(_) => false,
            };
            let f = match Pattern::parse(b"$") {
                Ok(v) => v.find(b"abc") == Some((3, 3)),
                Err(_) => false,
            };
            let g = match Pattern::parse(b"^") {
                Ok(v) => v.find(b"abc") == Some((0, 0)),
                Err(_) => false,
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "GR06-区间穷举",
        {
            let mut ok = true;
            let mut lo = 0usize;
            while lo < 256 {
                let mut hi = 0usize;
                while hi < 256 {
                    let mut set = ByteSet::new();
                    let r = set.set_range(lo as u8, hi as u8);
                    if lo <= hi {
                        if r.is_err() {
                            ok = false;
                        }
                        let want = (hi - lo + 1) as u32;
                        if set.count() != want {
                            ok = false;
                        }
                        if !set.has(lo as u8) || !set.has(hi as u8) {
                            ok = false;
                        }
                        if lo > 0 && set.has((lo - 1) as u8) {
                            ok = false;
                        }
                        if hi < 255 && set.has((hi + 1) as u8) {
                            ok = false;
                        }
                    } else if r.err() != Some(GR_ERANGE) {
                        ok = false;
                    }
                    hi += 1;
                }
                lo += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "GR07-grep选择",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let p = match Pattern::parse(b"[a-z]") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let base = run(&p, &c, Flags::new());
            let a = base.selected == 5 && base.shown == 5 && base.out.starts_with("alpha");
            let cf = match Flags::parse(b"-c") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let b = run(&p, &c, cf).out == String::from("5");
            let nf = match Flags::parse(b"-n") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let p2 = match Pattern::parse(b"^[a-z]") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let c2 = run(&p2, &c, nf).out == String::from("1:alpha\n3:gamma\n4:delta\n6:a1b2");
            let vf = match Flags::parse(b"-v") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let p3 = match Pattern::parse(b"[a-z]") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let d = run(&p3, &c, vf).out == String::from("42");
            let qf = match Flags::parse(b"-q") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let q = run(&p, &c, qf);
            let e = q.quiet && q.out.is_empty() && q.selected == 5;
            let f = Flags::parse(b"-z").err() == Some(GR_EFLAG);
            let g = Flags::parse(b"v").err() == Some(GR_EFLAG);
            a && b && c2 && d && e && f && g
        },
        "static",
    );

    s.add(
        "GR08-组合与静默",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let p = match Pattern::parse(b"[a-z]") {
                Ok(v) => v,
                Err(_) => Pattern::empty(),
            };
            let f1 = match Flags::parse(b"-v -c") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let a = run(&p, &c, f1).out == String::from("1");
            let f2 = match Flags::parse(b"-cn") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let b = run(&p, &c, f2).out == String::from("5");
            let f3 = match Flags::parse(b"-v -v") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let c2 = f3.invert && !f3.count && f3.text() == String::from("-v");
            let f4 = match Flags::parse(b"-q -c") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let d = run(&p, &c, f4).out.is_empty();
            let f5 = match Flags::parse(b"-vcnq") {
                Ok(v) => v,
                Err(_) => Flags::new(),
            };
            let e = f5.invert
                && f5.count
                && f5.number
                && f5.quiet
                && f5.text() == String::from("-v -c -n -q");
            a && b && c2 && d && e
        },
        "static",
    );

    s.add(
        "GR09-状态与容量",
        {
            let a = Line::parse(&[b'x'; LINE_CAP]).is_ok();
            let b = Line::parse(&[b'x'; LINE_CAP + 1]).err() == Some(GR_ELEN);
            let mut c = Corpus::new();
            let mut i = 0usize;
            let mut full = true;
            while i < LINES_CAP {
                let l = match Line::parse(b"x") {
                    Ok(v) => v,
                    Err(_) => Line::empty(),
                };
                if c.push(l).is_err() {
                    full = false;
                }
                i += 1;
            }
            let extra = match Line::parse(b"x") {
                Ok(v) => v,
                Err(_) => Line::empty(),
            };
            let d = full && c.len() == LINES_CAP && c.push(extra).err() == Some(GR_ERANGE);
            let ok24 = [b'a'; TOK_CAP];
            let e = Pattern::parse(&ok24).is_ok();
            let over = [b'a'; TOK_CAP + 1];
            let f = Pattern::parse(&over).err() == Some(GR_ELEN);
            let g = Pattern::parse(&[b'a'; PAT_CAP + 1]).err() == Some(GR_ELEN);
            let h = Line::parse(b"").is_ok();
            a && b && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "GR10-诊断渲染",
        {
            let a = match Bracket::parse(b"[^a-z]") {
                Ok(v) => v.summary() == String::from("[1;1;1;0;230]"),
                Err(_) => false,
            };
            let b = match Pattern::parse(b"^[a-z][0-9]*$") {
                Ok(v) => v.text() == String::from("^[0;1;1;0;26][0;1;1;0;10]*$"),
                Err(_) => false,
            };
            let c = match Flags::parse(b"-vn") {
                Ok(v) => v.text() == String::from("-v -n"),
                Err(_) => false,
            };
            let d = Atom::Any.text() == String::from(".");
            let e = Atom::Lit(0x0a).text() == String::from("\\x0a");
            let f = match Line::parse(b"ab") {
                Ok(v) => v.text() == String::from("ab") && v.len() == 2,
                Err(_) => false,
            };
            let g = match Pattern::parse(b"\\^$") {
                Ok(v) => v.len() == 1 && v.anchored_end() && !v.anchored_start(),
                Err(_) => false,
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "GR11-边界对照",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let cases: [(&[u8], u32); 5] = [
                (b"[a-m]", 5),
                (b"[^0-9]", 5),
                (b"a.*2", 1),
                (b"[[:space:]]", 0),
                (b"^[A-Z]", 1),
            ];
            let mut ok = true;
            let mut i = 0usize;
            while i < cases.len() {
                let (raw, want) = cases[i];
                let p = match Pattern::parse(raw) {
                    Ok(v) => v,
                    Err(_) => Pattern::empty(),
                };
                let o = run(&p, &c, Flags::new());
                if o.selected as u32 != want {
                    ok = false;
                }
                i += 1;
            }
            // 手算对照：[a-m] 只看首字节是否落在区间内。
            let br = match Bracket::parse(b"[a-m]") {
                Ok(v) => v,
                Err(_) => Bracket::empty(),
            };
            let mut manual = 0u32;
            let mut k = 0usize;
            while k < c.len() {
                if let Some(l) = c.get(k) {
                    if let Some(b) = l.as_bytes().first().copied() {
                        if br.matches(b) {
                            manual += 1;
                        }
                    }
                }
                k += 1;
            }
            ok && manual == 4
        },
        "static",
    );

    s.add(
        "GR12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES && !l.is_empty() && l.ok_count() == 8 && l.err_count() == 6;
            let b = l.audit().is_ok();
            let c = l.sample(0).map(|s| s.ok) == Some(true);
            let d = l.sample(13).map(|s| s.ok) == Some(false);
            let e = l.rejects() == 0;
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "GR13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut l = build_ledger();
            let sealed = l.seal();
            let fresh = build_ledger();
            if fresh.seal() != sealed {
                ok = false;
            }
            let root = match l.freeze() {
                Ok(v) => v,
                Err(_) => 0,
            };
            if root != sealed || !l.is_frozen() || l.root() != root {
                ok = false;
            }
            if l.replay() != Ok(sealed) {
                ok = false;
            }
            let again = match Line::parse(b"x") {
                Ok(v) => v,
                Err(_) => Line::empty(),
            };
            let _ = again;
            let mut l2 = build_ledger();
            let _ = l2.freeze();
            let before = l2.rejects();
            let second = l2.freeze();
            if second.err() != Some(GR_EBAL) || l2.rejects() != before.saturating_add(1) {
                ok = false;
            }
            ok
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bracket(raw: &[u8]) -> Bracket {
        match Bracket::parse(raw) {
            Ok(v) => v,
            Err(_) => Bracket::empty(),
        }
    }

    fn pattern(raw: &[u8]) -> Pattern {
        match Pattern::parse(raw) {
            Ok(v) => v,
            Err(_) => Pattern::empty(),
        }
    }

    #[test]
    fn fold_is_deterministic() {
        let a = fold(FNV_OFF, 0x41);
        let b = fold(FNV_OFF, 0x41);
        assert_eq!(a, b);
        assert_ne!(fold(FNV_OFF, 0x41), fold(FNV_OFF, 0x42));
    }

    #[test]
    fn set_bits_ops() {
        let mut s = ByteSet::new();
        s.set(b'a');
        s.set(b'a');
        assert!(s.has(b'a'));
        assert_eq!(s.count(), 1);
        s.clear(b'a');
        assert!(!s.has(b'a'));
        let f = ByteSet::full();
        assert_eq!(f.count(), 256);
        let mut g = f;
        g.complement();
        assert!(g.is_empty());
    }

    #[test]
    fn set_range_direction() {
        let mut s = ByteSet::new();
        assert_eq!(s.set_range(b'a', b'z'), Ok(()));
        assert_eq!(s.count(), 26);
        assert_eq!(s.set_range(b'z', b'a'), Err(GR_ERANGE));
    }

    #[test]
    fn bracket_basic() {
        let b = bracket(b"[abc]");
        assert!(b.matches(b'a'));
        assert!(b.matches(b'c'));
        assert!(!b.matches(b'd'));
        assert_eq!(b.count(), 3);
    }

    #[test]
    fn bracket_negate() {
        let b = bracket(b"[^abc]");
        assert!(b.matches(b'z'));
        assert!(!b.matches(b'b'));
        assert_eq!(b.count(), 253);
    }

    #[test]
    fn bracket_first_close() {
        let b = bracket(b"[]]");
        assert!(b.matches(b']'));
        assert_eq!(b.count(), 1);
    }

    #[test]
    fn bracket_trailing_dash() {
        let b = bracket(b"[a-]");
        assert!(b.matches(b'-'));
        assert_eq!(b.count(), 2);
    }

    #[test]
    fn bracket_escape() {
        let b = bracket(b"[a\\-z]");
        assert!(b.matches(b'-'));
        assert_eq!(b.count(), 3);
    }

    #[test]
    fn bracket_classes() {
        let b = bracket(b"[[:alpha:][:digit:]]");
        assert_eq!(b.count(), 62);
        assert_eq!(b.classes, 2);
    }

    #[test]
    fn bracket_errors() {
        assert_eq!(Bracket::parse(b"[]").err(), Some(GR_EEMPTY));
        assert_eq!(Bracket::parse(b"[abc").err(), Some(GR_EPAT));
        assert_eq!(Bracket::parse(b"[z-a]").err(), Some(GR_ERANGE));
        assert_eq!(Bracket::parse(b"[[:bogus:]]").err(), Some(GR_ECLASS));
    }

    #[test]
    fn class_counts_match() {
        let mut i = 0usize;
        while i < CLASS_NAMES.len() {
            assert_eq!(class_set(i).count(), CLASS_COUNTS[i]);
            i += 1;
        }
    }

    #[test]
    fn class_partition() {
        let mut u = class_set(1);
        u.union(&class_set(4));
        assert_eq!(u, class_set(0));
        let mut g = class_set(0);
        g.union(&class_set(8));
        assert_eq!(g, class_set(5));
    }

    #[test]
    fn unknown_class() {
        assert_eq!(class_named(b"nope").err(), Some(GR_ECLASS));
        assert!(class_named(b"alpha").is_ok());
    }

    #[test]
    fn pattern_tokens() {
        let p = pattern(b"^[a-z][0-9]*$");
        assert_eq!(p.len(), 2);
        assert!(p.anchored_start());
        assert!(p.anchored_end());
        assert!(match p.token(1) {
            Some(t) => t.star,
            None => false,
        });
    }

    #[test]
    fn pattern_errors() {
        assert_eq!(Pattern::parse(b"").err(), Some(GR_EEMPTY));
        assert_eq!(Pattern::parse(b"*a").err(), Some(GR_EPAT));
        assert_eq!(Pattern::parse(b"a**").err(), Some(GR_EPAT));
        assert_eq!(Pattern::parse(b"\\").err(), Some(GR_EPAT));
        assert_eq!(Pattern::parse(b"[a-z").err(), Some(GR_EPAT));
    }

    #[test]
    fn pattern_nul_and_long() {
        assert_eq!(Pattern::parse(b"a\x00b").err(), Some(GR_EBYTE));
        assert_eq!(Pattern::parse(&[b'a'; PAT_CAP + 1]).err(), Some(GR_ELEN));
    }

    #[test]
    fn match_literal_and_star() {
        let p = pattern(b"a*b");
        assert_eq!(p.find(b"aaab"), Some((0, 4)));
        assert_eq!(p.find(b"b"), Some((0, 1)));
        assert_eq!(p.find(b"c"), None);
    }

    #[test]
    fn match_backtrack() {
        let p = pattern(b"a*ab");
        assert_eq!(p.find(b"aaab"), Some((0, 4)));
        let q = pattern(b"a*bc");
        assert_eq!(q.find(b"aaabc"), Some((0, 5)));
    }

    #[test]
    fn match_anchors() {
        let p = pattern(b"^a*$");
        assert!(p.matches_line(b""));
        assert!(p.matches_line(b"aaa"));
        assert!(!p.matches_line(b"aab"));
        let q = pattern(b"^$");
        assert!(q.matches_line(b""));
        assert!(!q.matches_line(b"x"));
    }

    #[test]
    fn match_dot() {
        let p = pattern(b"a.c");
        assert_eq!(p.find(b"xabc"), Some((1, 4)));
        assert!(p.find(b"a\nc").is_none());
    }

    #[test]
    fn grep_basic() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let p = pattern(b"[a-z]");
        let o = run(&p, &c, Flags::new());
        assert_eq!(o.selected, 5);
        assert!(o.out.starts_with("alpha\nBeta"));
    }

    #[test]
    fn grep_number_and_count() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let p = pattern(b"^[a-z]");
        let f = match Flags::parse(b"-n") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(run(&p, &c, f).out, String::from("1:alpha\n3:gamma\n4:delta\n6:a1b2"));
        let cf = match Flags::parse(b"-c") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(run(&p, &c, cf).out, String::from("4"));
    }

    #[test]
    fn grep_flags_errors() {
        assert_eq!(Flags::parse(b"-z").err(), Some(GR_EFLAG));
        assert_eq!(Flags::parse(b"v").err(), Some(GR_EFLAG));
        assert_eq!(Flags::parse(b"-").err(), Some(GR_EFLAG));
    }

    #[test]
    fn corpus_capacity() {
        let mut c = Corpus::new();
        let mut i = 0usize;
        while i < LINES_CAP {
            let l = match Line::parse(b"x") {
                Ok(v) => v,
                Err(_) => return,
            };
            assert!(c.push(l).is_ok());
            i += 1;
        }
        let l = match Line::parse(b"x") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.push(l).err(), Some(GR_ERANGE));
    }

    #[test]
    fn line_capacity() {
        assert!(Line::parse(&[b'x'; LINE_CAP]).is_ok());
        assert_eq!(Line::parse(&[b'x'; LINE_CAP + 1]).err(), Some(GR_ELEN));
    }

    #[test]
    fn ledger_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let sealed = l.seal();
        assert_eq!(l.replay(), Ok(sealed));
        assert_eq!(l.freeze(), Ok(sealed));
        assert!(l.is_frozen());
        assert_eq!(l.root(), sealed);
    }

    #[test]
    fn ledger_register_slot() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 3, op: OP_RANGE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad).err(), Some(GR_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn inject_all_true() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11565_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }
}
