//! UNX-F11561 LC_ALL=C 总闸与 C locale 字节序法定义
//!
//! 问题的根：POSIX 的 C locale 并不定义「语言习惯」，它定义的是一张**恒等表**。
//! 排序权重就是字节值本身（0x00 最小、0xff 最大），没有重权（reweight）、没有
//! 等价类（equivalence class）、没有多级比较（level 1 only）；字符类只覆盖
//! ASCII 的 0x00–0x7f；多字节字符集退化为单字节（`MB_CUR_MAX == 1`），于是
//! 「一个字节 == 一个字符」。凡是拿着 `en_US.UTF-8` 的直觉去读 C locale 的
//! 输出，都会在重音、大小写、数字与标点的相对次序上算错。
//!
//! 本单把这张表写成**可判定**的三件东西：
//!
//! 1. **序表**。`Gate` 持有 256 项的 `order` 数组。C locale 下必须是恒等置换
//!    （`order[b] == b`）。`verify_identity` 逐项核对，任何一处被改动都返回
//!    `LC_EBYTE`——这不是「结果不一致」，而是「表本身不再是 C locale」。
//! 2. **类表**。`Ctype::c_locale()` 在编译期算出 12 个类的位图（upper/lower/
//!    alpha/digit/xdigit/space/blank/print/graph/cntrl/punct/alnum），计数是
//!    26/26/52/10/22/6/2/95/94/33/32/62。位图按字节取值，`0x80` 以上全部
//!    不属于任何类（`is_ascii` 为假），这正是 C locale 与 UTF-8 locale 的分界。
//! 3. **总闸**。`LC_ALL` 一旦置为 `C`/`POSIX`，整个进程的十二个域同时落到
//!    C；`LC_ALL=C.UTF-8` 虽然也「是 C 开头」，但它的序值是码点序而不是字节序，
//!    因此被 `LC_ECONF` 拒绝。`LC_ALL` 缺席时退到 `LANG`；两者都无 → `LC_ESTATE`
//!    （没有总闸，任何序值查询都无意义）。
//!
//! 不变式（全部有检查项兜底）：
//!
//! * 恒等：`∀b ∈ 0..=255, key(b) == b`；
//! * 严格全序：`∀b < 255, key(b) < key(b+1)`，且比较满足反对称与传递；
//! * 前缀短者在前：`compare(b"ab", b"abc") == Less`，与字典序一致；
//! * 高位字节：`0x80..=0xff` 在字节序里**合法**（可排序），但在 `mbtowc`
//!    的严格解释下非法（`LC_EBYTE`），两个口径不可混用；
//! * 稳定：键相等时保持登记次序（`sort` 只在严格大于时前移）。
//!
//! 闸门次序：`LC_EBYTE`（序表非恒等）→ `LC_ERANGE`（下标/区间越界）→
//! `LC_EINVAL`（locale 名/类名非法）→ `LC_ELEN`（名或行超长）→ `LC_ESHORT`
//! （空输入/截断）→ `LC_ESTATE`（总闸未合）→ `LC_ECONF`（locale 与字节序冲突）
//! → `LC_EDUP`（重复登记）。前四道在 `Gate`/`Locale`/`Line` 的构造里，状态与
//! 冲突在 `Gate::from_env`，重复在 `Corpus::push_unique`。
//!
//! 账本语义：十四格 = 八格成功（键值、比较符号、小写计数、空格类计数、单字节
//! 消费长度、总闸合闸、排序单调对数、类区间宽度）+ 六格失败（未合闸取键、
//! 冲突 locale、类名非法、区间倒置、严格多字节非法、空输入截断）。`audit`
//! 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒登。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf420;

/// locale 名非法。
pub const LC_EINVAL: u32 = SEG_BASE + 0x01;
/// 名称/行超长。
pub const LC_ELEN: u32 = SEG_BASE + 0x02;
/// 序表字节非法（非恒等或不是单字节）。
pub const LC_EBYTE: u32 = SEG_BASE + 0x03;
/// 下标/区间越界。
pub const LC_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复登记。
pub const LC_EDUP: u32 = SEG_BASE + 0x05;
/// 空输入/截断。
pub const LC_ESHORT: u32 = SEG_BASE + 0x06;
/// 总闸未合（无 LC_ALL/LANG=C）。
pub const LC_ESTATE: u32 = SEG_BASE + 0x07;
/// locale 与字节序定义冲突。
pub const LC_ECONF: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const LC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const LC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const LC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const LC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 字节全集。
pub const BYTES: usize = 256;

/// locale 名容量。
pub const NAME_CAP: usize = 32;

/// 单行容量。
pub const LINE_CAP: usize = 24;

/// 语料容量。
pub const CORPUS_CAP: usize = 8;

/// 字符类个数。
pub const CLASSES: usize = 12;

/// C locale 的字符类别数（含 LC_ALL 一档，用于名表）。
pub const CATEGORIES: usize = 12;

/// 操作码：取序值。
pub const OP_ORDER: u8 = 0x01;
/// 操作码：比较。
pub const OP_COMPARE: u8 = 0x02;
/// 操作码：类表。
pub const OP_CTYPE: u8 = 0x03;
/// 操作码：类名。
pub const OP_CLASS: u8 = 0x04;
/// 操作码：单字节解码。
pub const OP_MB: u8 = 0x05;
/// 操作码：总闸。
pub const OP_GATE: u8 = 0x06;
/// 操作码：排序。
pub const OP_SORT: u8 = 0x07;
/// 操作码：区间。
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

/// 十六进制两位拼接。
fn push_hex2(s: &mut String, v: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if let Some(h) = HEX.get((v >> 4) as usize) {
        s.push(*h as char);
    }
    if let Some(l) = HEX.get((v & 0x0f) as usize) {
        s.push(*l as char);
    }
}

// ---------------------------------------------------------------------------
// 字符类位图
// ---------------------------------------------------------------------------

const CL_UPPER: u16 = 1 << 0;
const CL_LOWER: u16 = 1 << 1;
const CL_ALPHA: u16 = 1 << 2;
const CL_DIGIT: u16 = 1 << 3;
const CL_XDIGIT: u16 = 1 << 4;
const CL_SPACE: u16 = 1 << 5;
const CL_BLANK: u16 = 1 << 6;
const CL_PRINT: u16 = 1 << 7;
const CL_GRAPH: u16 = 1 << 8;
const CL_CNTRL: u16 = 1 << 9;
const CL_PUNCT: u16 = 1 << 10;
const CL_ALNUM: u16 = 1 << 11;

/// C locale 的十二个字符类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 大写字母。
    Upper,
    /// 小写字母。
    Lower,
    /// 字母。
    Alpha,
    /// 数字。
    Digit,
    /// 十六进制数字。
    Xdigit,
    /// 空白（含换行等控制符）。
    Space,
    /// 仅空格与制表。
    Blank,
    /// 可打印。
    Print,
    /// 可打印且非空格。
    Graph,
    /// 控制符。
    Cntrl,
    /// 标点。
    Punct,
    /// 字母或数字。
    Alnum,
}

impl Class {
    /// 十二个，次序即位序。
    pub fn all() -> [Class; CLASSES] {
        [
            Class::Upper,
            Class::Lower,
            Class::Alpha,
            Class::Digit,
            Class::Xdigit,
            Class::Space,
            Class::Blank,
            Class::Print,
            Class::Graph,
            Class::Cntrl,
            Class::Punct,
            Class::Alnum,
        ]
    }

    /// 位掩码。
    pub fn bit(&self) -> u16 {
        match self {
            Class::Upper => CL_UPPER,
            Class::Lower => CL_LOWER,
            Class::Alpha => CL_ALPHA,
            Class::Digit => CL_DIGIT,
            Class::Xdigit => CL_XDIGIT,
            Class::Space => CL_SPACE,
            Class::Blank => CL_BLANK,
            Class::Print => CL_PRINT,
            Class::Graph => CL_GRAPH,
            Class::Cntrl => CL_CNTRL,
            Class::Punct => CL_PUNCT,
            Class::Alnum => CL_ALNUM,
        }
    }

    /// 下标（零基）。
    pub fn index(&self) -> usize {
        let mut i = 0usize;
        while i < CLASSES {
            if let Some(c) = Class::all().get(i) {
                if c == self {
                    return i;
                }
            }
            i += 1;
        }
        CLASSES
    }

    /// 名。
    pub fn name(&self) -> &'static str {
        match self {
            Class::Upper => "upper",
            Class::Lower => "lower",
            Class::Alpha => "alpha",
            Class::Digit => "digit",
            Class::Xdigit => "xdigit",
            Class::Space => "space",
            Class::Blank => "blank",
            Class::Print => "print",
            Class::Graph => "graph",
            Class::Cntrl => "cntrl",
            Class::Punct => "punct",
            Class::Alnum => "alnum",
        }
    }

    /// 解析类名（严格小写、无前后空白）。
    pub fn parse(raw: &[u8]) -> Result<Class, u32> {
        let mut i = 0usize;
        while i < CLASSES {
            if let Some(c) = Class::all().get(i) {
                if c.name().as_bytes() == raw {
                    return Ok(*c);
                }
            }
            i += 1;
        }
        Err(LC_EINVAL)
    }
}

/// 256 位集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bits {
    w: [u64; 4],
}

impl Bits {
    /// 空集。
    pub const fn empty() -> Bits {
        Bits { w: [0; 4] }
    }

    /// 全集。
    pub const fn full() -> Bits {
        Bits { w: [!0u64; 4] }
    }

    /// 置位，返回是否发生变化。
    pub fn set(&mut self, b: u8) -> bool {
        let wi = (b as usize) >> 6;
        let bit = 1u64 << ((b as usize) & 63);
        match self.w.get_mut(wi) {
            Some(w) => {
                let old = *w;
                *w = old | bit;
                old & bit == 0
            }
            None => false,
        }
    }

    /// 清位，返回是否发生变化。
    pub fn clear(&mut self, b: u8) -> bool {
        let wi = (b as usize) >> 6;
        let bit = 1u64 << ((b as usize) & 63);
        match self.w.get_mut(wi) {
            Some(w) => {
                let old = *w;
                *w = old & !bit;
                old & bit != 0
            }
            None => false,
        }
    }

    /// 成员判定。
    pub fn has(&self, b: u8) -> bool {
        let wi = (b as usize) >> 6;
        let bit = 1u64 << ((b as usize) & 63);
        match self.w.get(wi) {
            Some(w) => w & bit != 0,
            None => false,
        }
    }

    /// 基数。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if let Some(w) = self.w.get(i) {
                n += w.count_ones() as usize;
            }
            i += 1;
        }
        n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// 并。
    pub fn union(self, o: Bits) -> Bits {
        let mut w = [0u64; 4];
        let mut i = 0usize;
        while i < 4 {
            let a = self.w.get(i).copied().unwrap_or(0);
            let b = o.w.get(i).copied().unwrap_or(0);
            if let Some(d) = w.get_mut(i) {
                *d = a | b;
            }
            i += 1;
        }
        Bits { w }
    }

    /// 交。
    pub fn inter(self, o: Bits) -> Bits {
        let mut w = [0u64; 4];
        let mut i = 0usize;
        while i < 4 {
            let a = self.w.get(i).copied().unwrap_or(0);
            let b = o.w.get(i).copied().unwrap_or(0);
            if let Some(d) = w.get_mut(i) {
                *d = a & b;
            }
            i += 1;
        }
        Bits { w }
    }

    /// 差（self - o）。
    pub fn diff(self, o: Bits) -> Bits {
        let mut w = [0u64; 4];
        let mut i = 0usize;
        while i < 4 {
            let a = self.w.get(i).copied().unwrap_or(0);
            let b = o.w.get(i).copied().unwrap_or(0);
            if let Some(d) = w.get_mut(i) {
                *d = a & !b;
            }
            i += 1;
        }
        Bits { w }
    }

    /// 首个成员。
    pub fn first(&self) -> Option<u8> {
        let mut b = 0usize;
        while b < BYTES {
            if self.has(b as u8) {
                return Some(b as u8);
            }
            b += 1;
        }
        None
    }

    /// 区间置位（含两端），返回置位个数。
    pub fn set_range(&mut self, lo: u8, hi: u8) -> Result<usize, u32> {
        if lo > hi {
            return Err(LC_ERANGE);
        }
        let mut n = 0usize;
        let mut b = lo as usize;
        while b <= hi as usize {
            if self.set(b as u8) {
                n += 1;
            }
            b += 1;
        }
        Ok(n)
    }

    /// 区间基数。
    pub fn range_count(&self, lo: u8, hi: u8) -> Result<usize, u32> {
        if lo > hi {
            return Err(LC_ERANGE);
        }
        let mut n = 0usize;
        let mut b = lo as usize;
        while b <= hi as usize {
            if self.has(b as u8) {
                n += 1;
            }
            b += 1;
        }
        Ok(n)
    }
}

/// C locale 的类表：每个字节一个 12 位掩码。
#[derive(Clone, Copy)]
pub struct Ctype {
    bits: [u16; BYTES],
}

impl Ctype {
    /// 在编译期算出 C locale 的完整类表。
    pub const fn c_locale() -> Ctype {
        let mut bits = [0u16; BYTES];
        let mut i = 0usize;
        while i < BYTES {
            let b = i as u8;
            let mut m = 0u16;
            if b >= b'A' && b <= b'Z' {
                m |= CL_UPPER;
            }
            if b >= b'a' && b <= b'z' {
                m |= CL_LOWER;
            }
            if (b >= b'A' && b <= b'Z') || (b >= b'a' && b <= b'z') {
                m |= CL_ALPHA;
            }
            if b >= b'0' && b <= b'9' {
                m |= CL_DIGIT;
            }
            if (b >= b'0' && b <= b'9')
                || (b >= b'A' && b <= b'F')
                || (b >= b'a' && b <= b'f')
            {
                m |= CL_XDIGIT;
            }
            if b == 0x20 || b == 0x09 {
                m |= CL_BLANK;
            }
            if b == 0x20 || (b >= 0x09 && b <= 0x0d) {
                m |= CL_SPACE;
            }
            if b >= 0x20 && b <= 0x7e {
                m |= CL_PRINT;
            }
            if b >= 0x21 && b <= 0x7e {
                m |= CL_GRAPH;
            }
            if b < 0x20 || b == 0x7f {
                m |= CL_CNTRL;
            }
            if m & (CL_ALPHA | CL_DIGIT) != 0 {
                m |= CL_ALNUM;
            }
            if m & CL_PRINT != 0 && m & CL_ALNUM == 0 && b != 0x20 {
                m |= CL_PUNCT;
            }
            bits[i] = m;
            i += 1;
        }
        Ctype { bits }
    }

    /// 单字节掩码。
    pub fn mask(&self, b: u8) -> u16 {
        match self.bits.get(b as usize) {
            Some(m) => *m,
            None => 0,
        }
    }

    /// 是否属于某类。
    pub fn has(&self, b: u8, c: Class) -> bool {
        self.mask(b) & c.bit() != 0
    }

    /// 某类的基数。
    pub fn count(&self, c: Class) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BYTES {
            if let Some(m) = self.bits.get(i) {
                if m & c.bit() != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某类的位集。
    pub fn bitset(&self, c: Class) -> Bits {
        let mut out = Bits::empty();
        let mut i = 0usize;
        while i < BYTES {
            if let Some(m) = self.bits.get(i) {
                if m & c.bit() != 0 {
                    out.set(i as u8);
                }
            }
            i += 1;
        }
        out
    }

    /// 是否有任何类命中。
    pub fn any(&self, b: u8) -> bool {
        self.mask(b) != 0
    }

    /// ASCII 判定（C locale 下「有类」等价于 ASCII）。
    pub fn is_ascii(&self, b: u8) -> bool {
        b < 0x80
    }

    /// 数字值。
    pub fn digit_value(&self, b: u8) -> Option<u8> {
        if self.has(b, Class::Digit) {
            Some(b - b'0')
        } else {
            None
        }
    }

    /// 十六进制值。
    pub fn xdigit_value(&self, b: u8) -> Option<u8> {
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'A'..=b'F' => b - b'A' + 10,
            b'a'..=b'f' => b - b'a' + 10,
            _ => return None,
        };
        Some(v)
    }

    /// 大写转换（C locale 只动 a–z）。
    pub fn to_upper(&self, b: u8) -> u8 {
        if self.has(b, Class::Lower) {
            b - 0x20
        } else {
            b
        }
    }

    /// 小写转换（C locale 只动 A–Z）。
    pub fn to_lower(&self, b: u8) -> u8 {
        if self.has(b, Class::Upper) {
            b + 0x20
        } else {
            b
        }
    }

    /// 十二类计数表。
    pub fn counts(&self) -> [usize; CLASSES] {
        let mut out = [0usize; CLASSES];
        let mut i = 0usize;
        while i < CLASSES {
            if let Some(c) = Class::all().get(i) {
                if let Some(d) = out.get_mut(i) {
                    *d = self.count(*c);
                }
            }
            i += 1;
        }
        out
    }

    /// 类交集计数（用于断言「alpha ∩ print == alpha」这类恒等式）。
    pub fn inter_count(&self, a: Class, b: Class) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < BYTES {
            if let Some(m) = self.bits.get(i) {
                if m & a.bit() != 0 && m & b.bit() != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
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
    /// 符号码（-1/0/1）。
    pub fn code(&self) -> i32 {
        match self {
            Ord3::Less => -1,
            Ord3::Equal => 0,
            Ord3::Greater => 1,
        }
    }

    /// 秩（1/2/3），便于入账本。
    pub fn rank(&self) -> u32 {
        match self {
            Ord3::Less => 1,
            Ord3::Equal => 2,
            Ord3::Greater => 3,
        }
    }

    /// 取反。
    pub fn flip(&self) -> Ord3 {
        match self {
            Ord3::Less => Ord3::Greater,
            Ord3::Equal => Ord3::Equal,
            Ord3::Greater => Ord3::Less,
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

// ---------------------------------------------------------------------------
// locale 名与总闸
// ---------------------------------------------------------------------------

/// 字符集部分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codeset {
    /// 未写（C/POSIX 的常态）。
    None,
    /// UTF-8。
    Utf8,
    /// 显式 ASCII。
    Ascii,
    /// 其它。
    Other,
}

/// 解析后的 locale 名（定容，不分配）。
#[derive(Clone, Copy, Debug)]
pub struct Locale {
    buf: [u8; NAME_CAP],
    base_len: usize,
    code: Codeset,
    has_mod: bool,
}

impl Locale {
    /// 空名（非法态，仅供 `blank` 用）。
    pub const fn blank() -> Locale {
        Locale { buf: [0u8; NAME_CAP], base_len: 0, code: Codeset::None, has_mod: false }
    }

    /// 解析 `language[_territory][.codeset][@modifier]`。
    pub fn parse(raw: &[u8]) -> Result<Locale, u32> {
        if raw.is_empty() {
            return Err(LC_ESHORT);
        }
        if raw.len() > NAME_CAP {
            return Err(LC_ELEN);
        }
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => return Err(LC_ESHORT),
            };
            let ok = matches!(b,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'@' | b'-');
            if !ok {
                return Err(LC_EINVAL);
            }
            i += 1;
        }
        let mut dot = raw.len();
        let mut at = raw.len();
        let mut k = 0usize;
        while k < raw.len() {
            if let Some(b) = raw.get(k).copied() {
                if b == b'.' && dot == raw.len() {
                    dot = k;
                }
                if b == b'@' && at == raw.len() {
                    at = k;
                }
            }
            k += 1;
        }
        if at < dot {
            return Err(LC_EINVAL);
        }
        let base_end = if dot < raw.len() { dot } else { at };
        if base_end == 0 {
            return Err(LC_EINVAL);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut m = 0usize;
        while m < base_end {
            if let (Some(d), Some(s)) = (buf.get_mut(m), raw.get(m).copied()) {
                *d = s;
            }
            m += 1;
        }
        let code = if dot < raw.len() {
            let cs = raw.get(dot + 1..if at < raw.len() { at } else { raw.len() });
            match cs {
                Some(v) => {
                    if ascii_ci_eq(v, b"utf-8") || ascii_ci_eq(v, b"utf8") {
                        Codeset::Utf8
                    } else if ascii_ci_eq(v, b"ascii") || ascii_ci_eq(v, b"us-ascii") {
                        Codeset::Ascii
                    } else if v.is_empty() {
                        return Err(LC_EINVAL);
                    } else {
                        Codeset::Other
                    }
                }
                None => return Err(LC_ESHORT),
            }
        } else {
            Codeset::None
        };
        Ok(Locale { buf, base_len: base_end, code, has_mod: at < raw.len() })
    }

    /// 基名。
    pub fn base(&self) -> &[u8] {
        let n = if self.base_len <= NAME_CAP { self.base_len } else { NAME_CAP };
        match self.buf.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 是否 C/POSIX（大小写不敏感，但 charset 必须缺席）。
    pub fn is_c(&self) -> bool {
        let b = self.base();
        (ascii_ci_eq(b, b"C") || ascii_ci_eq(b, b"POSIX")) && self.code == Codeset::None
    }

    /// 是否 C.UTF-8（码点序，不等于字节序）。
    pub fn is_c_utf8(&self) -> bool {
        let b = self.base();
        (ascii_ci_eq(b, b"C") || ascii_ci_eq(b, b"POSIX")) && self.code == Codeset::Utf8
    }

    /// 是否携带修饰（如 `sr_RS.UTF-8@latin`）。
    pub fn has_modifier(&self) -> bool {
        self.has_mod
    }

    /// 字符集。
    pub fn codeset(&self) -> Codeset {
        self.code
    }

    /// 文本化。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let b = self.base();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        match self.code {
            Codeset::None => {}
            Codeset::Utf8 => s.push_str(".UTF-8"),
            Codeset::Ascii => s.push_str(".ASCII"),
            Codeset::Other => s.push_str(".?"),
        }
        if self.has_mod {
            s.push('@');
        }
        s
    }
}

/// ASCII 大小写不敏感比较。
pub fn ascii_ci_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i).copied() {
            Some(v) => v.to_ascii_lowercase(),
            None => return false,
        };
        let y = match b.get(i).copied() {
            Some(v) => v.to_ascii_lowercase(),
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 环境变量槽（值定容）。
#[derive(Clone, Copy, Debug)]
pub struct Var {
    buf: [u8; NAME_CAP],
    len: usize,
    set: bool,
}

impl Var {
    /// 空槽。
    pub const fn empty() -> Var {
        Var { buf: [0u8; NAME_CAP], len: 0, set: false }
    }

    /// 赋值；空值等价于「取消设置」（POSIX 语义）。
    pub fn assign(&mut self, raw: &[u8]) -> Result<(), u32> {
        if raw.len() > NAME_CAP {
            return Err(LC_ELEN);
        }
        if raw.is_empty() {
            self.len = 0;
            self.set = false;
            return Ok(());
        }
        let mut i = 0usize;
        while i < raw.len() {
            if let Some(b) = raw.get(i).copied() {
                if let Some(d) = self.buf.get_mut(i) {
                    *d = b;
                }
            }
            i += 1;
        }
        self.len = raw.len();
        self.set = true;
        Ok(())
    }

    /// 取消设置。
    pub fn unset(&mut self) {
        self.len = 0;
        self.set = false;
    }

    /// 是否设置。
    pub fn is_set(&self) -> bool {
        self.set
    }

    /// 取值。
    pub fn value(&self) -> &[u8] {
        if !self.set {
            return &[];
        }
        let n = if self.len <= NAME_CAP { self.len } else { NAME_CAP };
        match self.buf.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }
}

/// 与总闸相关的最小环境：LC_ALL 与 LANG。
#[derive(Clone, Copy, Debug)]
pub struct Env {
    all: Var,
    lang: Var,
}

impl Env {
    /// 全空。
    pub const fn new() -> Env {
        Env { all: Var::empty(), lang: Var::empty() }
    }

    /// 设定 LC_ALL。
    pub fn set_all(&mut self, raw: &[u8]) -> Result<(), u32> {
        self.all.assign(raw)
    }

    /// 设定 LANG。
    pub fn set_lang(&mut self, raw: &[u8]) -> Result<(), u32> {
        self.lang.assign(raw)
    }

    /// LC_ALL。
    pub fn all(&self) -> Var {
        self.all
    }

    /// LANG。
    pub fn lang(&self) -> Var {
        self.lang
    }

    /// 逐行解析 `NAME=VALUE` 形式的环境文本。
    pub fn parse(text: &[u8]) -> Result<Env, u32> {
        let mut env = Env::new();
        let mut pos = 0usize;
        while pos < text.len() {
            let mut end = pos;
            while end < text.len() {
                match text.get(end).copied() {
                    Some(b'\n') => break,
                    Some(_) => end += 1,
                    None => break,
                }
            }
            let mut line_start = pos;
            while line_start < end {
                match text.get(line_start).copied() {
                    Some(b' ') | Some(b'\t') => line_start += 1,
                    _ => break,
                }
            }
            let mut line_end = end;
            while line_end > line_start {
                match text.get(line_end - 1).copied() {
                    Some(b' ') | Some(b'\t') | Some(b'\r') => line_end -= 1,
                    _ => break,
                }
            }
            if line_end > line_start {
                let line = text.get(line_start..line_end).unwrap_or(&[]);
                let mut eq = line.len();
                let mut i = 0usize;
                while i < line.len() {
                    if let Some(b) = line.get(i).copied() {
                        if b == b'=' {
                            eq = i;
                            break;
                        }
                    }
                    i += 1;
                }
                if eq == line.len() {
                    return Err(LC_EINVAL);
                }
                let key = line.get(0..eq).unwrap_or(&[]);
                let val = line.get(eq + 1..).unwrap_or(&[]);
                if key == b"LC_ALL" {
                    env.set_all(val)?;
                } else if key == b"LANG" {
                    env.set_lang(val)?;
                } else if key.len() > 3 && key.get(0..3) == Some(b"LC_") {
                    // 其它域在 11562 展开；本单只认 LC_ALL/LANG，但语法仍需合法。
                    let mut k = 0usize;
                    while k < key.len() {
                        if let Some(b) = key.get(k).copied() {
                            let ok = matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_');
                            if !ok {
                                return Err(LC_EINVAL);
                            }
                        }
                        k += 1;
                    }
                } else {
                    return Err(LC_EINVAL);
                }
            }
            pos = if end < text.len() { end + 1 } else { end };
        }
        Ok(env)
    }

    /// 渲染回文本（用于检查项逐字比对）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("LC_ALL=");
        if self.all.is_set() {
            let v = self.all.value();
            let mut i = 0usize;
            while i < v.len() {
                if let Some(b) = v.get(i) {
                    s.push(*b as char);
                }
                i += 1;
            }
        }
        s.push('\n');
        s.push_str("LANG=");
        if self.lang.is_set() {
            let v = self.lang.value();
            let mut i = 0usize;
            while i < v.len() {
                if let Some(b) = v.get(i) {
                    s.push(*b as char);
                }
                i += 1;
            }
        }
        s
    }
}

/// C locale 的序表（256 项恒等置换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    engaged: bool,
    order: [u8; BYTES],
}

impl Gate {
    /// 未合闸（序值查询一律 `LC_ESTATE`）。
    pub const fn disengaged() -> Gate {
        Gate { engaged: false, order: [0u8; BYTES] }
    }

    /// 合闸后的恒等表。
    pub const fn identity() -> Gate {
        let mut order = [0u8; BYTES];
        let mut i = 0usize;
        while i < BYTES {
            order[i] = i as u8;
            i += 1;
        }
        Gate { engaged: true, order }
    }

    /// 由环境合闸：LC_ALL 优先，其次 LANG；必须是纯 C/POSIX。
    pub fn from_env(env: &Env) -> Result<Gate, u32> {
        if env.all().is_set() {
            let loc = Locale::parse(env.all().value())?;
            if loc.is_c() {
                return Ok(Gate::identity());
            }
            return Err(LC_ECONF);
        }
        if env.lang().is_set() {
            let loc = Locale::parse(env.lang().value())?;
            if loc.is_c() {
                return Ok(Gate::identity());
            }
            return Err(LC_ECONF);
        }
        Err(LC_ESTATE)
    }

    /// 是否合闸。
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// 序值（0..=255 → 0..=255）。
    pub fn key(&self, b: u8) -> Result<u16, u32> {
        if !self.engaged {
            return Err(LC_ESTATE);
        }
        match self.order.get(b as usize) {
            Some(v) => Ok(*v as u16),
            None => Err(LC_ERANGE),
        }
    }

    /// 权重（一级权重即序值，二级/三级在 C locale 不存在）。
    pub fn weight(&self, b: u8) -> Result<u32, u32> {
        self.key(b).map(|k| k as u32)
    }

    /// 比较级数：C locale 恒为 1。
    pub fn levels(&self) -> u32 {
        1
    }

    /// 等价判定：C locale 无等价类，只有同一字节等价。
    pub fn equiv(&self, a: u8, b: u8) -> Result<bool, u32> {
        Ok(self.key(a)? == self.key(b)?)
    }

    /// 逐字节字典序比较。
    pub fn compare(&self, a: &[u8], b: &[u8]) -> Result<Ord3, u32> {
        if !self.engaged {
            return Err(LC_ESTATE);
        }
        if a.len() > 64 || b.len() > 64 {
            return Err(LC_ELEN);
        }
        let n = if a.len() < b.len() { a.len() } else { b.len() };
        let mut i = 0usize;
        while i < n {
            let xa = match a.get(i).copied() {
                Some(v) => v,
                None => return Err(LC_ESHORT),
            };
            let xb = match b.get(i).copied() {
                Some(v) => v,
                None => return Err(LC_ESHORT),
            };
            let ka = self.key(xa)?;
            let kb = self.key(xb)?;
            if ka < kb {
                return Ok(Ord3::Less);
            }
            if ka > kb {
                return Ok(Ord3::Greater);
            }
            i += 1;
        }
        if a.len() < b.len() {
            Ok(Ord3::Less)
        } else if a.len() > b.len() {
            Ok(Ord3::Greater)
        } else {
            Ok(Ord3::Equal)
        }
    }

    /// 恒等核对：任何漂移都是「表坏了」。
    pub fn verify_identity(&self) -> Result<(), u32> {
        if !self.engaged {
            return Err(LC_ESTATE);
        }
        let mut i = 0usize;
        while i < BYTES {
            match self.order.get(i) {
                Some(v) => {
                    if *v as usize != i {
                        return Err(LC_EBYTE);
                    }
                }
                None => return Err(LC_EBYTE),
            }
            i += 1;
        }
        Ok(())
    }

    /// 严格递增核对（全序性的直接证据）。
    pub fn strictly_increasing(&self) -> bool {
        if !self.engaged {
            return false;
        }
        let mut i = 0usize;
        while i + 1 < BYTES {
            let a = self.order.get(i).copied().unwrap_or(0);
            let b = self.order.get(i + 1).copied().unwrap_or(0);
            if a >= b {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 篡改一项（用于证明检出路径真的在工作）。
    pub fn swap(&mut self, i: u8, j: u8) -> Result<(), u32> {
        if !self.engaged {
            return Err(LC_ESTATE);
        }
        let a = match self.order.get(i as usize).copied() {
            Some(v) => v,
            None => return Err(LC_ERANGE),
        };
        let b = match self.order.get(j as usize).copied() {
            Some(v) => v,
            None => return Err(LC_ERANGE),
        };
        if let Some(d) = self.order.get_mut(i as usize) {
            *d = b;
        }
        if let Some(d) = self.order.get_mut(j as usize) {
            *d = a;
        }
        Ok(())
    }

    /// 序表渲染（每行 16 个十六进制字节）。
    pub fn render_table(&self, lo: u8, hi: u8) -> Result<String, u32> {
        if lo > hi {
            return Err(LC_ERANGE);
        }
        let mut s = String::new();
        let mut b = lo as usize;
        while b <= hi as usize {
            if b % 16 == 0 {
                newline(&mut s);
                push_hex2(&mut s, b as u8);
                s.push(':');
                s.push(' ');
            }
            let v = self.order.get(b).copied().unwrap_or(0);
            push_hex2(&mut s, v);
            if (b + 1) % 16 != 0 && b != hi as usize {
                s.push(' ');
            }
            b += 1;
        }
        Ok(s)
    }
}

/// 单字节解码结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mb {
    /// 码值（C locale 下等于字节值）。
    pub code: u32,
    /// 消费字节数（恒为 1）。
    pub used: usize,
}

/// 宽松解码：字节即字符，任意字节都合法。
pub fn decode_one(raw: &[u8]) -> Result<Mb, u32> {
    match raw.first() {
        Some(b) => Ok(Mb { code: *b as u32, used: 1 }),
        None => Err(LC_ESHORT),
    }
}

/// 严格解码：C locale 的 `MB_CUR_MAX == 1`，0x80 以上不是合法多字节序列。
pub fn decode_strict(raw: &[u8]) -> Result<Mb, u32> {
    match raw.first() {
        Some(b) => {
            if *b >= 0x80 {
                Err(LC_EBYTE)
            } else {
                Ok(Mb { code: *b as u32, used: 1 })
            }
        }
        None => Err(LC_ESHORT),
    }
}

/// 编码回字节（C locale 一一对应）。
pub fn encode_one(code: u32) -> Result<u8, u32> {
    if code > 0xff {
        return Err(LC_ERANGE);
    }
    Ok(code as u8)
}

/// 整个缓冲区解码为码点数（C locale 下等于字节数）。
pub fn decode_all(raw: &[u8]) -> Result<usize, u32> {
    if raw.is_empty() {
        return Err(LC_ESHORT);
    }
    if raw.len() > 64 {
        return Err(LC_ELEN);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < raw.len() {
        let m = decode_one(raw.get(i..).unwrap_or(&[]))?;
        i += m.used;
        n += 1;
    }
    Ok(n)
}

/// 一行（定容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl Line {
    /// 空行。
    pub const fn empty() -> Line {
        Line { buf: [0u8; LINE_CAP], len: 0 }
    }

    /// 解析一行：拒绝超长、拒绝 NUL 与换行（它们会破坏行边界）。
    pub fn parse(raw: &[u8]) -> Result<Line, u32> {
        if raw.len() > LINE_CAP {
            return Err(LC_ELEN);
        }
        let mut buf = [0u8; LINE_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => return Err(LC_ESHORT),
            };
            if b == 0 || b == b'\n' || b == b'\r' {
                return Err(LC_EBYTE);
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
        if self.len <= LINE_CAP { self.len } else { LINE_CAP }
    }

    /// 是否空行。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 文本化。
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

/// 排序结果：下标排列 + 单调性证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sorted {
    /// 下标排列。
    pub idx: [u8; CORPUS_CAP],
    /// 有效长度。
    pub len: usize,
    /// 相邻逆序对数（排序后必须为 0）。
    pub inversions: usize,
}

impl Sorted {
    /// 空。
    pub const fn empty() -> Sorted {
        Sorted { idx: [0u8; CORPUS_CAP], len: 0, inversions: 0 }
    }

    /// 第 k 个原下标。
    pub fn at(&self, k: usize) -> Option<u8> {
        if k >= self.len {
            return None;
        }
        self.idx.get(k).copied()
    }
}

/// 定容语料。
#[derive(Clone, Copy, Debug)]
pub struct Corpus {
    lines: [Line; CORPUS_CAP],
    len: usize,
}

impl Corpus {
    /// 空。
    pub const fn new() -> Corpus {
        Corpus { lines: [Line::empty(); CORPUS_CAP], len: 0 }
    }

    /// 追加（允许重复行，重复是排序稳定性的测试素材）。
    pub fn push(&mut self, l: Line) -> Result<(), u32> {
        if self.len >= CORPUS_CAP {
            return Err(LC_ERANGE);
        }
        if let Some(d) = self.lines.get_mut(self.len) {
            *d = l;
        }
        self.len += 1;
        Ok(())
    }

    /// 追加且拒绝重复（`LC_EDUP`）。
    pub fn push_unique(&mut self, l: Line) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.lines.get(i) {
                if v.as_bytes() == l.as_bytes() {
                    return Err(LC_EDUP);
                }
            }
            i += 1;
        }
        self.push(l)
    }

    /// 按 '\n' 切分构造。
    pub fn parse(text: &[u8]) -> Result<Corpus, u32> {
        let mut c = Corpus::new();
        let mut start = 0usize;
        let mut i = 0usize;
        while i <= text.len() {
            let at_end = i == text.len();
            let is_nl = matches!(text.get(i).copied(), Some(b'\n'));
            if at_end || is_nl {
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

    /// 稳定插入排序。
    pub fn sort(&self, g: &Gate) -> Result<Sorted, u32> {
        if self.len == 0 {
            return Err(LC_ESHORT);
        }
        let mut idx = [0u8; CORPUS_CAP];
        let mut n = 0usize;
        while n < self.len {
            if let Some(d) = idx.get_mut(n) {
                *d = n as u8;
            }
            n += 1;
        }
        let mut i = 1usize;
        while i < self.len {
            let mut j = i;
            while j > 0 {
                let pi = idx.get(j - 1).copied().unwrap_or(0) as usize;
                let ci = idx.get(j).copied().unwrap_or(0) as usize;
                let pl = match self.lines.get(pi) {
                    Some(v) => v.as_bytes(),
                    None => return Err(LC_ESLOT),
                };
                let cl = match self.lines.get(ci) {
                    Some(v) => v.as_bytes(),
                    None => return Err(LC_ESLOT),
                };
                let ord = g.compare(pl, cl)?;
                if ord == Ord3::Greater {
                    if let Some(d) = idx.get_mut(j - 1) {
                        *d = ci as u8;
                    }
                    if let Some(d) = idx.get_mut(j) {
                        *d = pi as u8;
                    }
                    j -= 1;
                } else {
                    break;
                }
            }
            i += 1;
        }
        let mut inv = 0usize;
        let mut k = 1usize;
        while k < self.len {
            let a = idx.get(k - 1).copied().unwrap_or(0) as usize;
            let b = idx.get(k).copied().unwrap_or(0) as usize;
            let al = match self.lines.get(a) {
                Some(v) => v.as_bytes(),
                None => return Err(LC_ESLOT),
            };
            let bl = match self.lines.get(b) {
                Some(v) => v.as_bytes(),
                None => return Err(LC_ESLOT),
            };
            if g.compare(al, bl)? == Ord3::Greater {
                inv += 1;
            }
            k += 1;
        }
        Ok(Sorted { idx, len: self.len, inversions: inv })
    }

    /// 渲染语料（每行一条）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            newline(&mut s);
            if let Some(v) = self.lines.get(i) {
                s.push_str(&v.text());
            }
            i += 1;
        }
        s
    }
}

/// 基准语料：故意混入大写、小写、数字，用来暴露「字典序」与字节序的差异。
pub fn base_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"B")?)?;
    c.push(Line::parse(b"a")?)?;
    c.push(Line::parse(b"A")?)?;
    c.push(Line::parse(b"10")?)?;
    c.push(Line::parse(b"2")?)?;
    Ok(c)
}

/// 合闸探针：LC_ALL=C。
fn probe_engage() -> Result<u32, u32> {
    let mut env = Env::new();
    env.set_all(b"C")?;
    let g = Gate::from_env(&env)?;
    if g.engaged() && g.verify_identity().is_ok() {
        Ok(1)
    } else {
        Err(LC_ESTATE)
    }
}

/// 未合闸探针。
fn probe_no_gate() -> Result<u32, u32> {
    let env = Env::new();
    Gate::from_env(&env).map(|_| 1)
}

/// 冲突 locale 探针。
fn probe_conflict() -> Result<u32, u32> {
    let mut env = Env::new();
    env.set_all(b"en_US.UTF-8")?;
    Gate::from_env(&env).map(|_| 1)
}

/// 类名非法探针。
fn probe_class() -> Result<u32, u32> {
    Class::parse(b"upperr").map(|c| c.index() as u32)
}

/// 倒置区间探针。
fn probe_range() -> Result<u32, u32> {
    let mut b = Bits::empty();
    b.set_range(b'z', b'a').map(|n| n as u32)
}

/// 严格多字节探针。
fn probe_mb() -> Result<u32, u32> {
    decode_strict(&[0x80u8]).map(|m| m.code)
}

/// 空输入探针。
fn probe_short() -> Result<u32, u32> {
    decode_one(&[]).map(|m| m.code)
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
            return Err(LC_ESLOT);
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
            return Err(LC_EBAL);
        }
        const VALS: [u32; 8] = [65, 1, 26, 6, 1, 1, 4, 26];
        const CODES: [u32; 6] = [LC_ESTATE, LC_ECONF, LC_EINVAL, LC_ERANGE, LC_EBYTE, LC_ESHORT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LC_EBAL),
            };
            if s.slot as usize != i {
                return Err(LC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LC_EBAL);
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
            return Err(LC_EBAL);
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
            Err(LC_ESEAL)
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

/// 构建账本：八成功值来自真实域运算，六失败码按固定次序。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let gate = Gate::identity();

    let _ = l.register(word_cell(
        0,
        OP_ORDER,
        gate.key(b'A').map(|k| k as u32),
    ));

    let _ = l.register(word_cell(1, OP_COMPARE, {
        gate.compare(b"abc", b"abd").map(|o| o.rank())
    }));

    let _ = l.register(count_cell(2, OP_CTYPE, {
        let ct = Ctype::c_locale();
        Ok(ct.count(Class::Lower))
    }));

    let _ = l.register(count_cell(3, OP_CLASS, {
        let ct = Ctype::c_locale();
        Ok(ct.count(Class::Space))
    }));

    let _ = l.register(count_cell(4, OP_MB, {
        decode_one(b"Z").map(|m| m.used)
    }));

    let _ = l.register(word_cell(5, OP_GATE, probe_engage()));

    let _ = l.register(count_cell(6, OP_SORT, {
        base_corpus().and_then(|c| c.sort(&gate).map(|s| s.len - 1 - s.inversions))
    }));

    let _ = l.register(count_cell(7, OP_RANGE, {
        let mut b = Bits::empty();
        b.set_range(b'A', b'Z').map(|n| n)
    }));

    let _ = l.register(word_cell(8, OP_GATE, probe_no_gate()));

    let _ = l.register(word_cell(9, OP_GATE, probe_conflict()));

    let _ = l.register(word_cell(10, OP_CLASS, probe_class()));

    let _ = l.register(word_cell(11, OP_RANGE, probe_range()));

    let _ = l.register(word_cell(12, OP_MB, probe_mb()));

    let _ = l.register(word_cell(13, OP_MB, probe_short()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut gate_env = Env::new();
    let _ = gate_env.set_all(b"C");
    out[0] = match Gate::from_env(&gate_env) {
        Ok(g) => {
            g.engaged()
                && g.verify_identity().is_ok()
                && g.strictly_increasing()
                && g.levels() == 1
        }
        Err(_) => false,
    };

    out[1] = {
        let ct = Ctype::c_locale();
        let c = ct.counts();
        let ids = ct.bitset(Class::Alpha)
            .inter(ct.bitset(Class::Print))
            .count() == ct.count(Class::Alpha);
        let disj = ct.bitset(Class::Upper)
            .inter(ct.bitset(Class::Lower))
            .count() == 0;
        let ascii = ct.bitset(Class::Print)
            .diff(ct.bitset(Class::Graph))
            .count() == 1;
        let hp = ct.mask(0x80) == 0 && ct.mask(0xff) == 0;
        ids && disj && ascii && hp
            && c.first().copied() == Some(26)
            && c.get(2).copied() == Some(52)
            && c.get(3).copied() == Some(10)
            && c.get(6).copied() == Some(2)
            && c.get(9).copied() == Some(33)
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
        let g = Gate::identity();
        let prefix = g.compare(b"ab", b"abc") == Ok(Ord3::Less);
        let tail = g.compare(b"abc", b"ab") == Ok(Ord3::Greater);
        let eq = g.compare(b"a", b"a") == Ok(Ord3::Equal);
        let high = g.compare(&[0x7f], &[0x80]) == Ok(Ord3::Less);
        let sorted = match base_corpus() {
            Ok(c) => match c.sort(&g) {
                Ok(s) => {
                    s.inversions == 0
                        && s.at(0) == Some(3)
                        && s.at(1) == Some(4)
                        && s.at(2) == Some(2)
                        && s.at(3) == Some(0)
                        && s.at(4) == Some(1)
                }
                Err(_) => false,
            },
            Err(_) => false,
        };
        prefix && tail && eq && high && sorted
    };

    out
}

/// 判据集合。
pub fn run_unx11561_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11561.cbyteorder");

    s.add(
        "LC01-字节序恒等",
        {
            let g = Gate::identity();
            let mut ok = g.engaged() && g.verify_identity().is_ok() && g.strictly_increasing();
            let mut i = 0usize;
            while i < BYTES {
                let k = g.key(i as u8);
                if k != Ok(i as u16) {
                    ok = false;
                }
                i += 1;
            }
            let d = Gate::disengaged();
            ok = ok && d.key(0) == Err(LC_ESTATE) && d.verify_identity() == Err(LC_ESTATE);
            ok
        },
        "static",
    );

    s.add(
        "LC02-比较语义",
        {
            let g = Gate::identity();
            let a = g.compare(b"", b"") == Ok(Ord3::Equal);
            let b = g.compare(b"", b"a") == Ok(Ord3::Less);
            let c = g.compare(b"a", b"") == Ok(Ord3::Greater);
            let d = g.compare(b"ab", b"abc") == Ok(Ord3::Less);
            let e = g.compare(b"abc", b"ab") == Ok(Ord3::Greater);
            let f = g.compare(b"A", b"a") == Ok(Ord3::Less);
            let h = g.compare(b"Z", b"a") == Ok(Ord3::Less);
            let i = g.compare(&[0xff], &[0x00]) == Ok(Ord3::Greater);
            let j = g.compare(&[0x80], &[0x7f]) == Ok(Ord3::Greater);
            let k = g.compare(b"abc", b"abc") == Ok(Ord3::Equal);
            let l = g.compare(&[b'x'; 65], b"x") == Err(LC_ELEN);
            let m = g.compare(b"B", b"a") == Ok(Ord3::Less);
            a && b && c && d && e && f && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "LC03-ctype 类表",
        {
            let ct = Ctype::c_locale();
            let c = ct.counts();
            let mut ok = c.len() == CLASSES;
            ok = ok
                && c.first().copied() == Some(26)
                && c.get(1).copied() == Some(26)
                && c.get(2).copied() == Some(52)
                && c.get(3).copied() == Some(10)
                && c.get(4).copied() == Some(22)
                && c.get(5).copied() == Some(6)
                && c.get(6).copied() == Some(2)
                && c.get(7).copied() == Some(95)
                && c.get(8).copied() == Some(94)
                && c.get(9).copied() == Some(33)
                && c.get(10).copied() == Some(32)
                && c.get(11).copied() == Some(62);
            ok = ok
                && ct.has(b'a', Class::Lower)
                && !ct.has(b'A', Class::Lower)
                && ct.has(b'F', Class::Xdigit)
                && !ct.has(b'G', Class::Xdigit)
                && ct.has(0x09, Class::Blank)
                && !ct.has(0x0a, Class::Blank)
                && ct.has(0x0a, Class::Space)
                && ct.has(b' ', Class::Punct) == false
                && ct.has(b'!', Class::Punct)
                && ct.has(0x7f, Class::Cntrl)
                && !ct.has(0x80, Class::Print)
                && ct.digit_value(b'7') == Some(7)
                && ct.xdigit_value(b'f') == Some(15)
                && ct.to_upper(b'q') == b'Q'
                && ct.to_lower(b'Q') == b'q'
                && ct.to_upper(b'3') == b'3';
            let total = ct.inter_count(Class::Alpha, Class::Print);
            ok && total == 52 && ct.inter_count(Class::Upper, Class::Lower) == 0
        },
        "static",
    );

    s.add(
        "LC04-类名与区间闸门",
        {
            let a = Class::parse(b"upper") == Ok(Class::Upper);
            let b = Class::parse(b"alnum") == Ok(Class::Alnum);
            let c = Class::parse(b"UPPER") == Err(LC_EINVAL);
            let d = Class::parse(b"") == Err(LC_EINVAL);
            let e = Class::parse(b"alpha ") == Err(LC_EINVAL);
            let f = Class::Upper.index() == 0 && Class::Alnum.index() == 11;
            let mut bits = Bits::empty();
            let g = bits.set_range(b'a', b'z') == Ok(26);
            let h = bits.count() == 26;
            let i = bits.set(b'a') == false;
            let j = bits.range_count(b'a', b'z') == Ok(26);
            let k = bits.set_range(b'z', b'a') == Err(LC_ERANGE);
            let l = Bits::full().count() == 256;
            let m = Bits::full().diff(bits).count() == 230;
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "LC05-总闸三态",
        {
            let a = {
                let mut e = Env::new();
                let _ = e.set_all(b"C");
                match Gate::from_env(&e) {
                    Ok(g) => g.engaged() && g.verify_identity().is_ok(),
                    Err(_) => false,
                }
            };
            let b = {
                let mut e = Env::new();
                let _ = e.set_lang(b"POSIX");
                Gate::from_env(&e).map(|g| g.engaged()).unwrap_or(false)
            };
            let c = Gate::from_env(&Env::new()).err() == Some(LC_ESTATE);
            let d = {
                let mut e = Env::new();
                let _ = e.set_all(b"en_US.UTF-8");
                Gate::from_env(&e).err() == Some(LC_ECONF)
            };
            let f = {
                let mut e = Env::new();
                let _ = e.set_all(b"C.UTF-8");
                Gate::from_env(&e).err() == Some(LC_ECONF)
            };
            let g = {
                let mut e = Env::new();
                let _ = e.set_all(b"C");
                let _ = e.set_lang(b"en_US.UTF-8");
                Gate::from_env(&e).map(|x| x.engaged()).unwrap_or(false)
            };
            let h = {
                let mut e = Env::new();
                let _ = e.set_all(b"");
                let _ = e.set_lang(b"C");
                Gate::from_env(&e).map(|x| x.engaged()).unwrap_or(false)
            };
            let i = Locale::parse(b"C").map(|l| l.is_c()).unwrap_or(false);
            let j = Locale::parse(b"POSIX").map(|l| l.is_c()).unwrap_or(false);
            let k = Locale::parse(b"C.UTF-8").map(|l| l.is_c_utf8()).unwrap_or(false);
            let l = Locale::parse(b"xx/yy").err() == Some(LC_EINVAL);
            a && b && c && d && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "LC06-单字节多字节",
        {
            let a = decode_one(b"A") == Ok(Mb { code: 65, used: 1 });
            let b = decode_one(&[0xff]).map(|m| m.code) == Ok(255);
            let c = decode_one(&[]).err() == Some(LC_ESHORT);
            let d = decode_strict(&[0x80]).err() == Some(LC_EBYTE);
            let e = decode_strict(b"A").map(|m| m.used) == Ok(1);
            let f = encode_one(65) == Ok(b'A');
            let g = encode_one(256).err() == Some(LC_ERANGE);
            let h = decode_all(b"abcd").map(|n| n) == Ok(4);
            let i = decode_all(&[]).err() == Some(LC_ESHORT);
            let j = decode_all(&[b'a'; 65]).err() == Some(LC_ELEN);
            let k = Gate::identity().levels() == 1;
            a && b && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "LC07-排序稳定",
        {
            let g = Gate::identity();
            let mut ok = true;
            match base_corpus() {
                Ok(c) => match c.sort(&g) {
                    Ok(s) => {
                        ok = ok && s.len == 5 && s.inversions == 0;
                        ok = ok && s.at(0) == Some(3);
                        ok = ok && s.at(1) == Some(4);
                        ok = ok && s.at(2) == Some(2);
                        ok = ok && s.at(3) == Some(0);
                        ok = ok && s.at(4) == Some(1);
                        ok = ok && s.at(5).is_none();
                    }
                    Err(_) => ok = false,
                },
                Err(_) => ok = false,
            }
            let mut c = Corpus::new();
            let _ = c.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let _ = c.push(Line::parse(b"a").unwrap_or(Line::empty()));
            let _ = c.push(Line::parse(b"b").unwrap_or(Line::empty()));
            let s = c.sort(&g).unwrap_or(Sorted::empty());
            ok = ok && s.at(0) == Some(1) && s.at(1) == Some(0) && s.at(2) == Some(2);
            let e = Corpus::new().sort(&g).err() == Some(LC_ESHORT);
            let mut u = Corpus::new();
            let p = u.push_unique(Line::parse(b"x").unwrap_or(Line::empty())).is_ok();
            let q = u.push_unique(Line::parse(b"x").unwrap_or(Line::empty())).err() == Some(LC_EDUP);
            ok && e && p && q
        },
        "static",
    );

    s.add(
        "LC08-locale 解析",
        {
            let a = Locale::parse(b"en_US.UTF-8").map(|l| l.codeset()).unwrap_or(Codeset::Other) == Codeset::Utf8;
            let b = Locale::parse(b"sr_RS.UTF-8@latin")
                .map(|l| l.has_modifier())
                .unwrap_or(false);
            let c = Locale::parse(b"").err() == Some(LC_ESHORT);
            let d = Locale::parse(b"bad name").err() == Some(LC_EINVAL);
            let e = Locale::parse(&[b'x'; 33]).err() == Some(LC_ELEN);
            let f = Locale::parse(b"@latin").err() == Some(LC_EINVAL);
            let g = Locale::parse(b"C.utf8").map(|l| l.is_c_utf8()).unwrap_or(false);
            let h = Locale::parse(b"C").map(|l| l.text()) == Ok(String::from("C"));
            let i = Locale::parse(b"en_US.UTF-8").map(|l| l.text())
                == Ok(String::from("en_US.UTF-8"));
            let j = ascii_ci_eq(b"Utf-8", b"utf-8") && !ascii_ci_eq(b"utf-8", b"utf8");
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "LC09-表篡改检出",
        {
            let mut g = Gate::identity();
            let a = g.verify_identity().is_ok();
            let b = g.swap(0x41, 0x61).is_ok();
            let c = g.verify_identity().err() == Some(LC_EBYTE);
            let d = !g.strictly_increasing();
            let e = g.key(b'A') == Ok(b'a' as u16);
            let f = g.key(b'a') == Ok(b'A' as u16);
            let mut h = Gate::identity();
            let i = h.swap(255, 0).is_ok() && h.key(0) == Ok(255);
            let j = h.verify_identity().err() == Some(LC_EBYTE);
            let k = Gate::disengaged().swap(0, 1).err() == Some(LC_ESTATE);
            let l = h.verify_identity() != Ok(());
            a && b && c && d && e && f && i && j && k && l
        },
        "static",
    );

    s.add(
        "LC10-序表渲染",
        {
            let g = Gate::identity();
            let t = g.render_table(0x60, 0x7f).unwrap_or_default();
            let a = t.split('\n').count() == 2;
            let b = t.starts_with("60: 60 61 62 63");
            let c = t.contains("6c 6d 6e 6f\n70: 70 71 72 73 74 75 76 77 78 79 7a 7b 7c 7d 7e 7f");
            let d = t.len() <= 128;
            let e = g.render_table(0x7f, 0x00).err() == Some(LC_ERANGE);
            let f = {
                let mut q = Gate::identity();
                let _ = q.swap(0x61, 0x62);
                let text = q.render_table(0x60, 0x62).unwrap_or_default();
                text == String::from("60: 60 62 61")
            };
            let h = {
                let ct = Ctype::c_locale();
                let mut ok = true;
                let mut i = 0usize;
                while i < BYTES {
                    if ct.has(i as u8, Class::Digit) != ((i as u8) >= b'0' && (i as u8) <= b'9') {
                        ok = false;
                    }
                    i += 1;
                }
                ok
            };
            a && b && c && d && e && f && h
        },
        "static",
    );

    s.add(
        "LC11-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES && !l.is_empty();
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let mut d = true;
            let mut i = 0usize;
            while i < SAMPLES {
                match l.sample(i) {
                    Some(v) => {
                        if v.slot as usize != i {
                            d = false;
                        }
                        if v.ok != (i < 8) {
                            d = false;
                        }
                    }
                    None => d = false,
                }
                i += 1;
            }
            let e = l.sample(SAMPLES).is_none();
            let f = l.sample(0).map(|v| v.value) == Some(65);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "LC12-封印重放",
        {
            let mut l = build_ledger();
            let sealed = l.seal();
            let a = sealed != 0 && l.root() == 0;
            let b = match l.replay() {
                Ok(v) => v == sealed,
                Err(_) => false,
            };
            let c = l.freeze() == Ok(sealed) && l.is_frozen() && l.root() == sealed;
            let d = l.register(Sample::blank()).err() == Some(LC_ESLOT);
            let e = l.rejects() == 1;
            let f = l.freeze().err() == Some(LC_EBAL) && l.rejects() == 2;
            let mut g = build_ledger();
            let mut bad = match g.sample(0) {
                Some(v) => v,
                None => Sample::blank(),
            };
            bad.value = 64;
            if let Some(dst) = g.rows.get_mut(0) {
                *dst = bad;
            }
            let h = g.audit().err() == Some(LC_EVALUE);
            a && b && c && d && e && f && h
        },
        "static",
    );

    s.add(
        "LC13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let a = Gate::identity().compare(b"10", b"2") == Ok(Ord3::Less);
            let b = Gate::identity().compare(b"B", b"a") == Ok(Ord3::Less);
            let ct = Ctype::c_locale();
            let c = ct.count(Class::Punct) == 32 && ct.count(Class::Graph) == 94;
            let mut bits = Bits::empty();
            let d = bits.set_range(0x20, 0x7e) == Ok(95) && bits.count() == 95;
            let e = bits.diff(ct.bitset(Class::Print)).count() == 0;
            let mut l = build_ledger();
            let f = l.audit().is_ok();
            let g = l.freeze().is_ok() && l.replay().is_ok();
            ok = ok && a && b && c && d && e && f && g;
            let mut bad = build_ledger();
            if let Some(dst) = bad.rows.get_mut(3) {
                *dst = Sample { slot: 2, op: OP_CLASS, ok: true, code: 0, value: 6 };
            }
            ok = ok && bad.audit().err() == Some(LC_ESLOT);
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
    fn identity_table() {
        let g = Gate::identity();
        assert!(g.engaged());
        assert!(g.verify_identity().is_ok());
        assert!(g.strictly_increasing());
        assert_eq!(g.key(0), Ok(0));
        assert_eq!(g.key(255), Ok(255));
        assert_eq!(g.levels(), 1);
    }

    #[test]
    fn compare_cases() {
        let g = Gate::identity();
        assert_eq!(g.compare(b"a", b"a"), Ok(Ord3::Equal));
        assert_eq!(g.compare(b"a", b"b"), Ok(Ord3::Less));
        assert_eq!(g.compare(b"b", b"a"), Ok(Ord3::Greater));
        assert_eq!(g.compare(b"ab", b"abc"), Ok(Ord3::Less));
        assert_eq!(g.compare(&[0xff], &[0x80]), Ok(Ord3::Greater));
    }

    #[test]
    fn ctype_counts() {
        let ct = Ctype::c_locale();
        assert_eq!(ct.count(Class::Upper), 26);
        assert_eq!(ct.count(Class::Lower), 26);
        assert_eq!(ct.count(Class::Alpha), 52);
        assert_eq!(ct.count(Class::Digit), 10);
        assert_eq!(ct.count(Class::Xdigit), 22);
        assert_eq!(ct.count(Class::Space), 6);
        assert_eq!(ct.count(Class::Blank), 2);
        assert_eq!(ct.count(Class::Print), 95);
        assert_eq!(ct.count(Class::Graph), 94);
        assert_eq!(ct.count(Class::Cntrl), 33);
        assert_eq!(ct.count(Class::Punct), 32);
        assert_eq!(ct.count(Class::Alnum), 62);
    }

    #[test]
    fn ctype_membership() {
        let ct = Ctype::c_locale();
        assert!(ct.has(b'0', Class::Digit));
        assert!(!ct.has(b'/', Class::Digit));
        assert!(ct.has(b' ', Class::Blank));
        assert!(!ct.has(b'\n', Class::Blank));
        assert!(ct.has(b'\n', Class::Space));
        assert!(ct.has(b'~', Class::Graph));
        assert!(!ct.has(b' ', Class::Graph));
        assert!(ct.has(0x7f, Class::Cntrl));
        assert!(!ct.has(0x80, Class::Print));
        assert!(!ct.any(0x80));
    }

    #[test]
    fn class_names() {
        assert_eq!(Class::parse(b"alpha"), Ok(Class::Alpha));
        assert_eq!(Class::parse(b"ALPHA"), Err(LC_EINVAL));
        assert_eq!(Class::parse(b""), Err(LC_EINVAL));
        assert_eq!(Class::Digit.index(), 3);
        assert_eq!(Class::Punct.bit(), CL_PUNCT);
    }

    #[test]
    fn gate_states() {
        assert_eq!(Gate::from_env(&Env::new()).err(), Some(LC_ESTATE));
        let mut e = Env::new();
        e.set_all(b"C").expect("set");
        assert!(Gate::from_env(&e).expect("gate").engaged());
        let mut f = Env::new();
        f.set_all(b"en_US.UTF-8").expect("set");
        assert_eq!(Gate::from_env(&f).err(), Some(LC_ECONF));
        let mut h = Env::new();
        h.set_all(b"C.UTF-8").expect("set");
        assert_eq!(Gate::from_env(&h).err(), Some(LC_ECONF));
    }

    #[test]
    fn locale_parse() {
        assert!(Locale::parse(b"C").expect("c").is_c());
        assert!(Locale::parse(b"POSIX").expect("p").is_c());
        assert!(Locale::parse(b"C.UTF-8").expect("u").is_c_utf8());
        assert_eq!(Locale::parse(b"").err(), Some(LC_ESHORT));
        assert_eq!(Locale::parse(b"a/b").err(), Some(LC_EINVAL));
        assert_eq!(Locale::parse(b"@x").err(), Some(LC_EINVAL));
        assert_eq!(Locale::parse(b"en_US.UTF-8").expect("e").codeset(), Codeset::Utf8);
    }

    #[test]
    fn bits_ops() {
        let mut b = Bits::empty();
        assert_eq!(b.set_range(b'a', b'z'), Ok(26));
        assert_eq!(b.count(), 26);
        assert!(!b.set(b'a'));
        assert!(b.has(b'z'));
        assert!(!b.has(b'A'));
        let full = Bits::full();
        assert_eq!(full.count(), 256);
        assert_eq!(full.diff(b).count(), 230);
        assert_eq!(b.inter(full).count(), 26);
        assert_eq!(b.set_range(b'z', b'a'), Err(LC_ERANGE));
    }

    #[test]
    fn decode_paths() {
        assert_eq!(decode_one(b"a").map(|m| m.code), Ok(97));
        assert_eq!(decode_one(&[]).err(), Some(LC_ESHORT));
        assert_eq!(decode_strict(&[0x80]).err(), Some(LC_EBYTE));
        assert_eq!(encode_one(0x41), Ok(0x41));
        assert_eq!(encode_one(0x100).err(), Some(LC_ERANGE));
    }

    #[test]
    fn corpus_sort_order() {
        let c = base_corpus().expect("corpus");
        assert_eq!(c.len(), 5);
        let s = c.sort(&Gate::identity()).expect("sort");
        assert_eq!(s.inversions, 0);
        let expect = [3u8, 4, 2, 0, 1];
        let mut i = 0usize;
        while i < 5 {
            assert_eq!(s.at(i), Some(expect[i]));
            i += 1;
        }
    }

    #[test]
    fn corpus_stable() {
        let mut c = Corpus::new();
        c.push(Line::parse(b"b").expect("l")).expect("p");
        c.push(Line::parse(b"a").expect("l")).expect("p");
        c.push(Line::parse(b"b").expect("l")).expect("p");
        let s = c.sort(&Gate::identity()).expect("sort");
        assert_eq!(s.at(0), Some(1));
        assert_eq!(s.at(1), Some(0));
        assert_eq!(s.at(2), Some(2));
    }

    #[test]
    fn corpus_dup_gate() {
        let mut c = Corpus::new();
        let l = Line::parse(b"x").expect("l");
        assert!(c.push_unique(l).is_ok());
        assert_eq!(c.push_unique(l).err(), Some(LC_EDUP));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn line_limits() {
        assert_eq!(Line::parse(&[b'x'; 25]).err(), Some(LC_ELEN));
        assert_eq!(Line::parse(b"a\nb").err(), Some(LC_EBYTE));
        assert!(Line::parse(b"hello").expect("l").len() == 5);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(8).map(|v| v.code), Some(LC_ESTATE));
        assert_eq!(l.sample(13).map(|v| v.code), Some(LC_ESHORT));
    }

    #[test]
    fn ledger_seal_replay() {
        let mut l = build_ledger();
        let s = l.seal();
        assert_eq!(l.replay().expect("replay"), s);
        assert_eq!(l.freeze().expect("freeze"), s);
        assert!(l.is_frozen());
        assert_eq!(l.register(Sample::blank()).err(), Some(LC_ESLOT));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn ledger_tamper() {
        let mut l = build_ledger();
        let mut s = l.sample(2).expect("row");
        s.value = 27;
        l.rows[2] = s;
        assert_eq!(l.audit().err(), Some(LC_EVALUE));
    }

    #[test]
    fn render_table_identity() {
        let g = Gate::identity();
        let t = g.render_table(0x40, 0x4f).expect("t");
        assert!(t.starts_with("40: 40 41 42"));
        assert_eq!(t.split('\n').count(), 1);
        assert_eq!(g.render_table(1, 0).err(), Some(LC_ERANGE));
    }

    #[test]
    fn inject_four_all() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11561_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }

    #[test]
    fn env_parse_roundtrip() {
        let e = Env::parse(b"LC_ALL=C\nLANG=en_US.UTF-8\n").expect("env");
        assert_eq!(e.render(), String::from("LC_ALL=C\nLANG=en_US.UTF-8"));
        assert_eq!(Env::parse(b"badline").err(), Some(LC_EINVAL));
    }
}
