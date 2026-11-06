//! UNX-F11567 tr 字符类单源 C ctype 判据
//!
//! `tr '[:lower:]' '[:upper:]'` 看起来只是一个字符串替换，实际却把一个隐含
//! 约定摆到台面上：**字符类必须与 C locale 的 ctype 表逐字节一致**。若实现里
//! 另写一份「类成员表」，那么 `tr` 认为 `0x7f` 可打印、`isprint()` 认为不可打
//! 印时，两个工具就会在同一份输入上给出不同答案，而这种漂移在纯 ASCII 文本上
//! 永远测不出来，只有拿全 256 个字节扫一遍才会现形。
//!
//! 本单的做法是「单源」：`class_table()` 一次算出 256 个字节的 12 位类掩码，
//! `CLASSES` 是它的常量实例；`is_upper()/is_lower()/...` 全部只查这张表，
//! `Class::expand()` 也只查这张表。因此「谓词一致」与「展开一致」不是两条需要
//! 各自维护的规则，而是同一条规则的两个投影——本单的 `TR03` 与 `TR10` 就是拿
//! 全 256 字节把这层投影关系钉死。
//!
//! 类掩码的 12 个位按**包含关系**分层：`alpha = upper | lower`、
//! `alnum = alpha | digit`、`graph = alnum | punct`、`print = graph | blank`
//! （blank 里只有空格落在 print 域，制表符在 cntrl 域，故 print 与 graph 的差
//! 恰为 `{0x20}`）。这些等式在 `TR03` 里逐字节验证；任何一处手写表的笔误都会
//! 让某个等式在某个字节上失败，而不是「看起来差不多」。
//!
//! 区间与转义是第二层语义。`a-e`、`\101`、`[:digit:]`、`[x*3]` 四种写法都归
//! 一到同一条「单元流」上：先读单元（单字节或类展开），再看下一个原始字节是否
//! 为 `-`，是则读右端点并要求两端都是单字节、且右端点不小于左端点。逆序区间
//! `z-a` 在 GNU tr 里是硬错误，本单给 `TR_ERANGE`；类出现在区间端点上是语法
//! 错误，给 `TR_EINVAL`。这样「区间」只有一条实现路径，四种写法不会各错各的。
//!
//! 翻译本身是**位置映射**：SET2 短于 SET1 时用 SET2 的末字节补足，长于 SET1
//! 时截断。补集 `-c` 先把 SET1 的位图取反再按同一位置规则映射，因此补集后的
//! 长度恒为 256 - |SET1|（本单用这一点做 `TR08` 的算术判据）。`-d` 与 `-s`
//! 组合时先删后压，压缩集合取 SET2（无 SET2 时取 SET1），与 GNU 一致。
//!
//! 锚点：`\U \L \u \l` 这类大小写转换不属于 tr，但本单的第二语义轴要证明
//! 「大小写转换只动 ASCII」：`toupper_c()` 只把 `a..=z` 减 0x20，其余字节原样
//! 返回，于是 `0x80..=0xff` 在 `tr '[:lower:]' '[:upper:]'` 下必然不变——这正是
//! C locale 与 UTF-8 locale 的分水岭，也是本单 `TR10` 的判据。
//!
//! 闸门次序：类名（`TR_EINVAL`）→ 集合长度（`TR_ELEN`）→ 转义字节
//! （`TR_EBYTE`）→ 区间与八进制范围（`TR_ERANGE`）→ 规范集重复
//! （`TR_EDUP`）→ 截断（`TR_ESHORT`）→ 封存状态（`TR_ESTATE`）→ 选项冲突
//! （`TR_ECONF`）。前六道在 `parse_set`/`Set256::insert_checked`，状态在
//! `Plan::apply` 的封存判定，冲突在 `parse_cli` 的 `-d` 双操作数检查。
//!
//! 账本语义：十四格 = 八格成功（大写类基数、标点类基数、区间长度、首字节映射、
//! 折叠项数、补集基数、删除后长度、压缩后长度）+ 六格失败（未知类、空集、
//! 坏转义、逆序区间、未闭合类、删除带 SET2）。`audit` 认「八值六码」，`seal`
//! 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝再登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf480;

/// 类名未知或语法不可归约。
pub const TR_EINVAL: u32 = SEG_BASE + 0x01;
/// 集合为空或超出定容。
pub const TR_ELEN: u32 = SEG_BASE + 0x02;
/// 转义字节非法。
pub const TR_EBYTE: u32 = SEG_BASE + 0x03;
/// 区间逆序或八进制越界。
pub const TR_ERANGE: u32 = SEG_BASE + 0x04;
/// 规范集重复插入。
pub const TR_EDUP: u32 = SEG_BASE + 0x05;
/// 输入/输出被截断。
pub const TR_ESHORT: u32 = SEG_BASE + 0x06;
/// 已封存后继续使用。
pub const TR_ESTATE: u32 = SEG_BASE + 0x07;
/// 选项冲突（`-d` 带第二个操作数）。
pub const TR_ECONF: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const TR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const TR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const TR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 类掩码：大写。
pub const CB_UPPER: u16 = 1 << 0;
/// 类掩码：小写。
pub const CB_LOWER: u16 = 1 << 1;
/// 类掩码：数字。
pub const CB_DIGIT: u16 = 1 << 2;
/// 类掩码：十六进制数字。
pub const CB_XDIGIT: u16 = 1 << 3;
/// 类掩码：字母。
pub const CB_ALPHA: u16 = 1 << 4;
/// 类掩码：字母数字。
pub const CB_ALNUM: u16 = 1 << 5;
/// 类掩码：标点。
pub const CB_PUNCT: u16 = 1 << 6;
/// 类掩码：空白。
pub const CB_SPACE: u16 = 1 << 7;
/// 类掩码：横向空白。
pub const CB_BLANK: u16 = 1 << 8;
/// 类掩码：控制字符。
pub const CB_CNTRL: u16 = 1 << 9;
/// 类掩码：可见字符。
pub const CB_GRAPH: u16 = 1 << 10;
/// 类掩码：可打印字符。
pub const CB_PRINT: u16 = 1 << 11;

/// 类的种数。
pub const CLASS_COUNT: usize = 12;

/// 单集定容（类展开最多 95 字节，`[x*n]` 与字面量叠加后留足余量）。
pub const SET_CAP: usize = 512;

/// 单次转换的输出上限（无堆，调用方给缓冲）。
pub const OUT_CAP: usize = 1024;

/// 单单元最大字节数（print 类是 95）。
pub const UNIT_CAP: usize = 96;

/// `[x*n]` 的重复上限，防止 `[a*999999999]` 撑爆定容。
pub const REPEAT_MAX: u32 = 64;

/// 操作码：类基数。
pub const OP_CLASS: u8 = 0x01;
/// 操作码：集合解析。
pub const OP_PARSE: u8 = 0x02;
/// 操作码：翻译。
pub const OP_XLATE: u8 = 0x03;
/// 操作码：删除。
pub const OP_DEL: u8 = 0x04;
/// 操作码：压缩。
pub const OP_SQUEEZE: u8 = 0x05;
/// 操作码：补集。
pub const OP_COMP: u8 = 0x06;
/// 操作码：命令行。
pub const OP_CLI: u8 = 0x07;
/// 操作码：类名。
pub const OP_NAME: u8 = 0x08;

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

/// 两位十六进制。
fn push_hex2(s: &mut String, b: u8) {
    const DIG: [u8; 16] = *b"0123456789abcdef";
    let hi = (b >> 4) as usize;
    let lo = (b & 0x0f) as usize;
    if let Some(c) = DIG.get(hi) {
        s.push(*c as char);
    }
    if let Some(c) = DIG.get(lo) {
        s.push(*c as char);
    }
}

/// 三位八进制（`-b` 档的写法）。
fn push_oct3(s: &mut String, b: u8) {
    let a = (b >> 6) & 0x07;
    let c = (b >> 3) & 0x07;
    let d = b & 0x07;
    s.push((b'0' + a) as char);
    s.push((b'0' + c) as char);
    s.push((b'0' + d) as char);
}

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 单源类表：一次算出 256 个字节的类掩码。
pub const fn class_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        let b = i as u8;
        let up = b >= b'A' && b <= b'Z';
        let lo = b >= b'a' && b <= b'z';
        let di = b >= b'0' && b <= b'9';
        let mut m: u16 = 0;
        if up {
            m |= CB_UPPER;
        }
        if lo {
            m |= CB_LOWER;
        }
        if di {
            m |= CB_DIGIT;
        }
        if di || (up && b <= b'F') || (lo && b <= b'f') {
            m |= CB_XDIGIT;
        }
        if up || lo {
            m |= CB_ALPHA;
        }
        if up || lo || di {
            m |= CB_ALNUM;
        }
        if b == b' ' || b == 0x09 || b == 0x0a || b == 0x0b || b == 0x0c || b == 0x0d {
            m |= CB_SPACE;
        }
        if b == b' ' || b == 0x09 {
            m |= CB_BLANK;
        }
        if b < 0x20 || b == 0x7f {
            m |= CB_CNTRL;
        }
        if b >= 0x21 && b <= 0x7e {
            m |= CB_GRAPH;
        }
        if b >= 0x20 && b <= 0x7e {
            m |= CB_PRINT;
        }
        if b >= 0x21 && b <= 0x7e && !(up || lo || di) {
            m |= CB_PUNCT;
        }
        t[i] = m;
        i += 1;
    }
    t
}

/// 单源表的常量实例。全模块只认这一份。
pub const CLASSES: [u16; 256] = class_table();

/// 查类掩码。
pub fn mask_of(b: u8) -> u16 {
    match CLASSES.get(b as usize) {
        Some(v) => *v,
        None => 0,
    }
}

/// 是否大写。
pub fn is_upper(b: u8) -> bool {
    mask_of(b) & CB_UPPER != 0
}

/// 是否小写。
pub fn is_lower(b: u8) -> bool {
    mask_of(b) & CB_LOWER != 0
}

/// 是否数字。
pub fn is_digit(b: u8) -> bool {
    mask_of(b) & CB_DIGIT != 0
}

/// 是否十六进制数字。
pub fn is_xdigit(b: u8) -> bool {
    mask_of(b) & CB_XDIGIT != 0
}

/// 是否字母。
pub fn is_alpha(b: u8) -> bool {
    mask_of(b) & CB_ALPHA != 0
}

/// 是否字母数字。
pub fn is_alnum(b: u8) -> bool {
    mask_of(b) & CB_ALNUM != 0
}

/// 是否标点。
pub fn is_punct(b: u8) -> bool {
    mask_of(b) & CB_PUNCT != 0
}

/// 是否空白。
pub fn is_space(b: u8) -> bool {
    mask_of(b) & CB_SPACE != 0
}

/// 是否横向空白。
pub fn is_blank(b: u8) -> bool {
    mask_of(b) & CB_BLANK != 0
}

/// 是否控制字符。
pub fn is_cntrl(b: u8) -> bool {
    mask_of(b) & CB_CNTRL != 0
}

/// 是否可见字符。
pub fn is_graph(b: u8) -> bool {
    mask_of(b) & CB_GRAPH != 0
}

/// 是否可打印。
pub fn is_print(b: u8) -> bool {
    mask_of(b) & CB_PRINT != 0
}

/// C 锚大小写：只动 ASCII。
pub fn toupper_c(b: u8) -> u8 {
    if is_lower(b) {
        b.wrapping_sub(0x20)
    } else {
        b
    }
}

/// C 锚小写：只动 ASCII。
pub fn tolower_c(b: u8) -> u8 {
    if is_upper(b) {
        b.wrapping_add(0x20)
    } else {
        b
    }
}

/// 12 个类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 大写。
    Upper,
    /// 小写。
    Lower,
    /// 数字。
    Digit,
    /// 十六进制数字。
    Xdigit,
    /// 字母。
    Alpha,
    /// 字母数字。
    Alnum,
    /// 标点。
    Punct,
    /// 空白。
    Space,
    /// 横向空白。
    Blank,
    /// 控制字符。
    Cntrl,
    /// 可见字符。
    Graph,
    /// 可打印字符。
    Print,
}

impl Class {
    /// 十二种。
    pub fn all() -> [Class; CLASS_COUNT] {
        [
            Class::Upper,
            Class::Lower,
            Class::Digit,
            Class::Xdigit,
            Class::Alpha,
            Class::Alnum,
            Class::Punct,
            Class::Space,
            Class::Blank,
            Class::Cntrl,
            Class::Graph,
            Class::Print,
        ]
    }

    /// 位。
    pub fn bit(&self) -> u16 {
        match self {
            Class::Upper => CB_UPPER,
            Class::Lower => CB_LOWER,
            Class::Digit => CB_DIGIT,
            Class::Xdigit => CB_XDIGIT,
            Class::Alpha => CB_ALPHA,
            Class::Alnum => CB_ALNUM,
            Class::Punct => CB_PUNCT,
            Class::Space => CB_SPACE,
            Class::Blank => CB_BLANK,
            Class::Cntrl => CB_CNTRL,
            Class::Graph => CB_GRAPH,
            Class::Print => CB_PRINT,
        }
    }

    /// 名。
    pub fn name(&self) -> &'static str {
        match self {
            Class::Upper => "upper",
            Class::Lower => "lower",
            Class::Digit => "digit",
            Class::Xdigit => "xdigit",
            Class::Alpha => "alpha",
            Class::Alnum => "alnum",
            Class::Punct => "punct",
            Class::Space => "space",
            Class::Blank => "blank",
            Class::Cntrl => "cntrl",
            Class::Graph => "graph",
            Class::Print => "print",
        }
    }

    /// 下标（零基）。
    pub fn index(&self) -> usize {
        match self {
            Class::Upper => 0,
            Class::Lower => 1,
            Class::Digit => 2,
            Class::Xdigit => 3,
            Class::Alpha => 4,
            Class::Alnum => 5,
            Class::Punct => 6,
            Class::Space => 7,
            Class::Blank => 8,
            Class::Cntrl => 9,
            Class::Graph => 10,
            Class::Print => 11,
        }
    }

    /// 解析类名（不含括号）。
    pub fn parse(raw: &[u8]) -> Result<Class, u32> {
        match raw {
            b"upper" => Ok(Class::Upper),
            b"lower" => Ok(Class::Lower),
            b"digit" => Ok(Class::Digit),
            b"xdigit" => Ok(Class::Xdigit),
            b"alpha" => Ok(Class::Alpha),
            b"alnum" => Ok(Class::Alnum),
            b"punct" => Ok(Class::Punct),
            b"space" => Ok(Class::Space),
            b"blank" => Ok(Class::Blank),
            b"cntrl" => Ok(Class::Cntrl),
            b"graph" => Ok(Class::Graph),
            b"print" => Ok(Class::Print),
            _ => Err(TR_EINVAL),
        }
    }

    /// 是否成员（只查单源表）。
    pub fn has(&self, b: u8) -> bool {
        mask_of(b) & self.bit() != 0
    }

    /// 基数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < 256 {
            if self.has(i as u8) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 首成员。
    pub fn first(&self) -> Option<u8> {
        let mut i = 0u32;
        while i < 256 {
            if self.has(i as u8) {
                return Some(i as u8);
            }
            i += 1;
        }
        None
    }

    /// 末成员。
    pub fn last(&self) -> Option<u8> {
        let mut i = 256u32;
        while i > 0 {
            i -= 1;
            if self.has(i as u8) {
                return Some(i as u8);
            }
        }
        None
    }

    /// 展开进位图。
    pub fn expand(&self) -> Set256 {
        let mut s = Set256::empty();
        let mut i = 0u32;
        while i < 256 {
            if self.has(i as u8) {
                s.insert(i as u8);
            }
            i += 1;
        }
        s
    }

    /// 按序展开成字节缓冲。
    pub fn expand_into(&self, out: &mut [u8]) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut i = 0u32;
        while i < 256 {
            if self.has(i as u8) {
                match out.get_mut(n) {
                    Some(d) => *d = i as u8,
                    None => return Err(TR_ESHORT),
                }
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }
}

/// 256 位集合。低位对齐，第 i 位表示字节 i。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Set256 {
    w: [u64; 4],
}

impl Set256 {
    /// 空集。
    pub const fn empty() -> Set256 {
        Set256 { w: [0; 4] }
    }

    /// 全集。
    pub const fn full() -> Set256 {
        Set256 { w: [u64::MAX; 4] }
    }

    /// 置位；返回此前是否未置位。
    pub fn insert(&mut self, b: u8) -> bool {
        let i = (b >> 6) as usize;
        let m = 1u64 << (b & 63);
        match self.w.get_mut(i) {
            Some(w) => {
                let had = *w & m != 0;
                *w |= m;
                !had
            }
            None => false,
        }
    }

    /// 置位并要求此前未置位（重复给 `TR_EDUP`）。
    pub fn insert_checked(&mut self, b: u8) -> Result<(), u32> {
        if self.insert(b) {
            Ok(())
        } else {
            Err(TR_EDUP)
        }
    }

    /// 清位。
    pub fn remove(&mut self, b: u8) -> bool {
        let i = (b >> 6) as usize;
        let m = 1u64 << (b & 63);
        match self.w.get_mut(i) {
            Some(w) => {
                let had = *w & m != 0;
                *w &= !m;
                had
            }
            None => false,
        }
    }

    /// 测试。
    pub fn contains(&self, b: u8) -> bool {
        let i = (b >> 6) as usize;
        let m = 1u64 << (b & 63);
        match self.w.get(i) {
            Some(w) => *w & m != 0,
            None => false,
        }
    }

    /// 基数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if let Some(w) = self.w.get(i) {
                n += w.count_ones();
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
    pub fn union_with(&mut self, o: &Set256) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(d), Some(s)) = (self.w.get_mut(i), o.w.get(i)) {
                *d |= *s;
            }
            i += 1;
        }
    }

    /// 交。
    pub fn intersect_with(&mut self, o: &Set256) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(d), Some(s)) = (self.w.get_mut(i), o.w.get(i)) {
                *d &= *s;
            }
            i += 1;
        }
    }

    /// 差（self - o）。
    pub fn diff_with(&mut self, o: &Set256) {
        let mut i = 0usize;
        while i < 4 {
            if let (Some(d), Some(s)) = (self.w.get_mut(i), o.w.get(i)) {
                *d &= !*s;
            }
            i += 1;
        }
    }

    /// 补（在 256 域内）。
    pub fn complement(&self) -> Set256 {
        let mut o = Set256::empty();
        let mut i = 0usize;
        while i < 4 {
            if let Some(w) = self.w.get(i) {
                if let Some(d) = o.w.get_mut(i) {
                    *d = !*w;
                }
            }
            i += 1;
        }
        o
    }

    /// 最小成员。
    pub fn min(&self) -> Option<u8> {
        let mut i = 0u32;
        while i < 256 {
            if self.contains(i as u8) {
                return Some(i as u8);
            }
            i += 1;
        }
        None
    }

    /// 最大成员。
    pub fn max(&self) -> Option<u8> {
        let mut i = 256u32;
        while i > 0 {
            i -= 1;
            if self.contains(i as u8) {
                return Some(i as u8);
            }
        }
        None
    }

    /// 第 n 个成员（升序，零基）。
    pub fn nth(&self, n: u32) -> Option<u8> {
        let mut k = 0u32;
        let mut i = 0u32;
        while i < 256 {
            if self.contains(i as u8) {
                if k == n {
                    return Some(i as u8);
                }
                k += 1;
            }
            i += 1;
        }
        None
    }

    /// 按序导出。
    pub fn write_into(&self, out: &mut [u8]) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut i = 0u32;
        while i < 256 {
            if self.contains(i as u8) {
                match out.get_mut(n) {
                    Some(d) => *d = i as u8,
                    None => return Err(TR_ESHORT),
                }
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }
}

/// 一个解析单元（单字节或类展开）。
#[derive(Clone, Copy)]
struct Unit {
    b: [u8; UNIT_CAP],
    len: usize,
}

impl Unit {
    /// 单字节单元。
    fn one(v: u8) -> Unit {
        let mut b = [0u8; UNIT_CAP];
        b[0] = v;
        Unit { b, len: 1 }
    }

    /// 类单元。
    fn of_class(c: Class) -> Result<Unit, u32> {
        let mut b = [0u8; UNIT_CAP];
        let n = c.expand_into(&mut b)?;
        Ok(Unit { b, len: n })
    }
}

/// 读一个 `\` 转义（`i` 指向反斜杠之后）。
fn read_escape(raw: &[u8], i: &mut usize) -> Result<u8, u32> {
    let c = match raw.get(*i) {
        Some(v) => *v,
        None => return Err(TR_ESHORT),
    };
    *i += 1;
    match c {
        b'\\' => Ok(b'\\'),
        b'a' => Ok(0x07),
        b'b' => Ok(0x08),
        b'f' => Ok(0x0c),
        b'n' => Ok(0x0a),
        b'r' => Ok(0x0d),
        b't' => Ok(0x09),
        b'v' => Ok(0x0b),
        b'x' => {
            let mut v = 0u32;
            let mut n = 0u32;
            while n < 2 {
                let d = match raw.get(*i) {
                    Some(d) => *d,
                    None => break,
                };
                let hv = match d {
                    b'0'..=b'9' => (d - b'0') as u32,
                    b'a'..=b'f' => (d - b'a') as u32 + 10,
                    b'A'..=b'F' => (d - b'A') as u32 + 10,
                    _ => break,
                };
                v = v * 16 + hv;
                *i += 1;
                n += 1;
            }
            if n == 0 {
                return Err(TR_EBYTE);
            }
            if v > 0xff {
                return Err(TR_ERANGE);
            }
            Ok(v as u8)
        }
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            let mut n = 1u32;
            while n < 3 {
                let d = match raw.get(*i) {
                    Some(d) => *d,
                    None => break,
                };
                if !(b'0'..=b'7').contains(&d) {
                    break;
                }
                v = v * 8 + (d - b'0') as u32;
                *i += 1;
                n += 1;
            }
            if v > 0xff {
                return Err(TR_ERANGE);
            }
            Ok(v as u8)
        }
        _ => Err(TR_EBYTE),
    }
}

/// 读一个单元：字面字节、`\` 转义、`[:class:]`、`[x*n]`。
fn read_unit(raw: &[u8], i: &mut usize) -> Result<Unit, u32> {
    let c = match raw.get(*i) {
        Some(v) => *v,
        None => return Err(TR_ESHORT),
    };
    if c == b'\\' {
        *i += 1;
        let v = read_escape(raw, i)?;
        return Ok(Unit::one(v));
    }
    if c == b'[' {
        // `[:name:]`
        if raw.get(*i + 1).copied() == Some(b':') {
            let start = *i + 2;
            let mut j = start;
            while j < raw.len() {
                let d = match raw.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if d == b':' {
                    if raw.get(j + 1).copied() == Some(b']') {
                        let name = match raw.get(start..j) {
                            Some(s) => s,
                            None => return Err(TR_EINVAL),
                        };
                        let cls = Class::parse(name)?;
                        *i = j + 2;
                        return Unit::of_class(cls);
                    }
                }
                if d == b']' {
                    // `[:x]` 未闭合
                    return Err(TR_ESHORT);
                }
                j += 1;
            }
            return Err(TR_ESHORT);
        }
        // `[x*n]`
        if raw.get(*i + 2).copied() == Some(b'*') {
            let inner = match raw.get(*i + 1) {
                Some(v) => *v,
                None => return Err(TR_ESHORT),
            };
            let mut j = *i + 3;
            let mut v = 0u32;
            let mut n = 0u32;
            while j < raw.len() {
                let d = match raw.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if !(b'0'..=b'9').contains(&d) {
                    break;
                }
                v = v.saturating_mul(10).saturating_add((d - b'0') as u32);
                j += 1;
                n += 1;
            }
            if n == 0 {
                return Err(TR_EINVAL);
            }
            if raw.get(j).copied() != Some(b']') {
                return Err(TR_ESHORT);
            }
            if v == 0 || v > REPEAT_MAX {
                return Err(TR_ERANGE);
            }
            *i = j + 1;
            let mut u = Unit { b: [0u8; UNIT_CAP], len: 0 };
            let mut k = 0u32;
            while k < v {
                if let Some(d) = u.b.get_mut(k as usize) {
                    *d = inner;
                }
                k += 1;
            }
            u.len = v as usize;
            return Ok(u);
        }
    }
    *i += 1;
    Ok(Unit::one(c))
}

/// 解析 SET 串。
pub fn parse_set(raw: &[u8]) -> Result<SetSpec, u32> {
    let mut out = SetSpec::new();
    let mut i = 0usize;
    if raw.is_empty() {
        return Err(TR_ELEN);
    }
    while i < raw.len() {
        let u = read_unit(raw, &mut i)?;
        let dash = raw.get(i).copied() == Some(b'-') && raw.get(i + 1).is_some();
        if dash {
            let save = i;
            i += 1;
            let v = read_unit(raw, &mut i)?;
            if u.len != 1 || v.len != 1 {
                return Err(TR_EINVAL);
            }
            let lo = match u.b.first() {
                Some(x) => *x,
                None => return Err(TR_ELEN),
            };
            let hi = match v.b.first() {
                Some(x) => *x,
                None => return Err(TR_ELEN),
            };
            if hi < lo {
                return Err(TR_ERANGE);
            }
            let mut x = lo as u16;
            while x <= hi as u16 {
                if out.push(x as u8).is_err() {
                    return Err(TR_ESHORT);
                }
                x += 1;
            }
            let _ = save;
        } else {
            let mut k = 0usize;
            while k < u.len {
                if let Some(b) = u.b.get(k) {
                    if out.push(*b).is_err() {
                        return Err(TR_ESHORT);
                    }
                }
                k += 1;
            }
        }
    }
    if out.is_empty() {
        return Err(TR_ELEN);
    }
    Ok(out)
}

/// 有序字节串（保留重复，翻译按位置取）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetSpec {
    b: [u8; SET_CAP],
    len: usize,
}

impl SetSpec {
    /// 空。
    pub const fn new() -> SetSpec {
        SetSpec { b: [0; SET_CAP], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, v: u8) -> Result<(), u32> {
        if self.len >= SET_CAP {
            return Err(TR_ESHORT);
        }
        if let Some(d) = self.b.get_mut(self.len) {
            *d = v;
        }
        self.len += 1;
        Ok(())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 个。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.b.get(i).copied()
    }

    /// 末字节。
    pub fn last(&self) -> Option<u8> {
        if self.len == 0 {
            None
        } else {
            self.b.get(self.len - 1).copied()
        }
    }

    /// 切片。
    pub fn as_slice(&self) -> &[u8] {
        self.b.get(..self.len).unwrap_or(&[])
    }

    /// 去重位图。
    pub fn to_set(&self) -> Set256 {
        let mut s = Set256::empty();
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.b.get(i) {
                s.insert(*v);
            }
            i += 1;
        }
        s
    }

    /// 是否含某字节。
    pub fn contains(&self, v: u8) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.b.get(i) {
                if *b == v {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 首次出现位置。
    pub fn position(&self, v: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.b.get(i) {
                if *b == v {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 与切片比较。
    pub fn equals(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.b.get(i), other.get(i)) {
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

    /// 十六进制渲染。
    pub fn hex(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if i > 0 {
                s.push(' ');
            }
            if let Some(v) = self.b.get(i) {
                push_hex2(&mut s, *v);
            }
            i += 1;
        }
        s
    }
}

/// 运行模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 翻译。
    Translate,
    /// 删除。
    Delete,
    /// 压缩。
    Squeeze,
    /// 先删后压。
    DeleteSqueeze,
}

impl Mode {
    /// 名。
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Translate => "translate",
            Mode::Delete => "delete",
            Mode::Squeeze => "squeeze",
            Mode::DeleteSqueeze => "delete-squeeze",
        }
    }

    /// 是否用 SET2。
    pub fn uses_set2(&self) -> bool {
        match self {
            Mode::Translate | Mode::Squeeze => true,
            Mode::Delete | Mode::DeleteSqueeze => true,
        }
    }
}

/// 一次转换的统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// 读入字节。
    pub read: usize,
    /// 写出字节。
    pub written: usize,
    /// 被映射的字节数（值确实变了）。
    pub mapped: usize,
    /// 被删除。
    pub deleted: usize,
    /// 被压缩掉。
    pub squeezed: usize,
    /// 补集域内的字节。
    pub complemented: usize,
}

impl Stats {
    /// 零。
    pub const fn zero() -> Stats {
        Stats { read: 0, written: 0, mapped: 0, deleted: 0, squeezed: 0, complemented: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("r");
        push_dec(&mut s, self.read as u64);
        s.push_str(" w");
        push_dec(&mut s, self.written as u64);
        s.push_str(" m");
        push_dec(&mut s, self.mapped as u64);
        s.push_str(" d");
        push_dec(&mut s, self.deleted as u64);
        s.push_str(" s");
        push_dec(&mut s, self.squeezed as u64);
        s.push_str(" c");
        push_dec(&mut s, self.complemented as u64);
        s
    }
}

/// 转换规则：位置映射 + 删除集 + 压缩集。
#[derive(Clone, Copy)]
pub struct Rule {
    map: [u8; 256],
    del: Set256,
    squeeze: Set256,
    mode: Mode,
    complement: bool,
    folded: u16,
}

impl Rule {
    /// 恒等。
    pub fn identity() -> Rule {
        let mut map = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            map[i] = i as u8;
            i += 1;
        }
        Rule {
            map,
            del: Set256::empty(),
            squeeze: Set256::empty(),
            mode: Mode::Translate,
            complement: false,
            folded: 0,
        }
    }

    /// 造规则。
    ///
    /// 翻译：SET2 短则末字节补齐，长则截断；补集按位置取 SET2。
    /// 删除：SET1 入删除集，SET2 必须缺席（否则 `TR_ECONF`）。
    /// 压缩：SET2 存在时压 SET2，否则压 SET1。
    pub fn build(
        s1: &SetSpec,
        s2: Option<&SetSpec>,
        mode: Mode,
        complement: bool,
    ) -> Result<Rule, u32> {
        if s1.is_empty() {
            return Err(TR_ELEN);
        }
        let mut r = Rule::identity();
        r.mode = mode;
        r.complement = complement;
        match mode {
            Mode::Translate | Mode::Squeeze => {
                let mut src = Set256::empty();
                let mut i = 0usize;
                while i < s1.len() {
                    if let Some(v) = s1.at(i) {
                        src.insert(v);
                    }
                    i += 1;
                }
                if complement {
                    src = src.complement();
                }
                let pad = match s2.and_then(|s| s.last()) {
                    Some(v) => v,
                    None => match s1.last() {
                        Some(v) => v,
                        None => return Err(TR_ELEN),
                    },
                };
                let mut k = 0usize;
                let mut i = 0u32;
                while i < 256 {
                    let b = i as u8;
                    if src.contains(b) {
                        let t = match s2 {
                            Some(sp) => match sp.at(k) {
                                Some(v) => v,
                                None => pad,
                            },
                            None => b,
                        };
                        r.map[b as usize] = t;
                        if t != b {
                            r.folded = r.folded.saturating_add(1);
                        }
                        k += 1;
                    }
                    i += 1;
                }
                if mode == Mode::Squeeze {
                    r.squeeze = match s2 {
                        Some(sp) => sp.to_set(),
                        None => s1.to_set(),
                    };
                }
            }
            Mode::Delete | Mode::DeleteSqueeze => {
                if s2.is_some() {
                    return Err(TR_ECONF);
                }
                r.del = if complement { s1.to_set().complement() } else { s1.to_set() };
                if mode == Mode::DeleteSqueeze {
                    r.squeeze = s1.to_set();
                }
            }
        }
        Ok(r)
    }

    /// 映射表。
    pub fn mapping(&self) -> &[u8; 256] {
        &self.map
    }

    /// 规则模式。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 是否补集。
    pub fn is_complement(&self) -> bool {
        self.complement
    }

    /// 折叠项数。
    pub fn folded(&self) -> u16 {
        self.folded
    }

    /// 删除集。
    pub fn del_set(&self) -> Set256 {
        self.del
    }

    /// 压缩集。
    pub fn squeeze_set(&self) -> Set256 {
        self.squeeze
    }

    /// 是否恒等（不动任何字节）。
    pub fn is_identity(&self) -> bool {
        self.folded == 0 && self.del.is_empty() && self.squeeze.is_empty()
    }

    /// 是否双射（对翻译规则：折叠项数相等且值域无新增重复）。
    pub fn is_bijective(&self) -> bool {
        if self.mode != Mode::Translate || self.folded == 0 {
            return false;
        }
        let mut seen = Set256::empty();
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let t = match self.map.get(b as usize) {
                Some(v) => *v,
                None => return false,
            };
            if t != b {
                if !seen.insert(t) {
                    return false;
                }
            }
            i += 1;
        }
        seen.count() == self.folded as u32
    }

    /// 应用。
    pub fn apply(&self, inp: &[u8], out: &mut [u8]) -> Result<Stats, u32> {
        let mut st = Stats::zero();
        st.read = inp.len();
        let mut last: Option<u8> = None;
        let mut i = 0usize;
        while i < inp.len() {
            let b = match inp.get(i) {
                Some(v) => *v,
                None => break,
            };
            i += 1;
            if self.complement && self.mode == Mode::Translate {
                st.complemented = st.complemented.saturating_add(1);
            }
            if self.del.contains(b) {
                st.deleted = st.deleted.saturating_add(1);
                last = None;
                continue;
            }
            let t = match self.map.get(b as usize) {
                Some(v) => *v,
                None => b,
            };
            if t != b {
                st.mapped = st.mapped.saturating_add(1);
            }
            if !self.squeeze.is_empty() && self.squeeze.contains(t) {
                if last == Some(t) {
                    st.squeezed = st.squeezed.saturating_add(1);
                    continue;
                }
            }
            match out.get_mut(st.written) {
                Some(d) => *d = t,
                None => return Err(TR_ESHORT),
            }
            st.written += 1;
            last = Some(t);
        }
        Ok(st)
    }

    /// 复合：先 self 后 other（仅翻译模式）。
    pub fn compose(&self, other: &Rule) -> Result<Rule, u32> {
        if self.mode != Mode::Translate || other.mode != Mode::Translate {
            return Err(TR_ESTATE);
        }
        let mut r = Rule::identity();
        r.mode = Mode::Translate;
        let mut n = 0u16;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let mid = match self.map.get(b as usize) {
                Some(v) => *v,
                None => b,
            };
            let t = match other.map.get(mid as usize) {
                Some(v) => *v,
                None => mid,
            };
            r.map[b as usize] = t;
            if t != b {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        r.folded = n;
        Ok(r)
    }

    /// 非恒等项数统计。
    pub fn changed_table(&self) -> [u8; 256] {
        let mut t = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            t[i] = match self.map.get(i) {
                Some(v) => *v,
                None => i as u8,
            };
            i += 1;
        }
        t
    }

    /// 渲染非恒等项（每行 8 对）。
    pub fn render_table(&self) -> String {
        let mut s = String::new();
        let mut col = 0u32;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let t = match self.map.get(i as usize) {
                Some(v) => *v,
                None => b,
            };
            if t != b {
                if col == 0 {
                    newline(&mut s);
                } else {
                    s.push(' ');
                }
                push_hex2(&mut s, b);
                s.push('>');
                push_hex2(&mut s, t);
                col += 1;
                if col == 8 {
                    col = 0;
                }
            }
            i += 1;
        }
        newline(&mut s);
        s.push_str("folded ");
        push_dec(&mut s, self.folded as u64);
        s.push_str(" del ");
        push_dec(&mut s, self.del.count() as u64);
        s.push_str(" squeeze ");
        push_dec(&mut s, self.squeeze.count() as u64);
        s
    }
}

/// 一次命令行调用。
#[derive(Clone, Copy)]
pub struct Plan {
    rule: Rule,
    source: SetSpec,
    target: Option<SetSpec>,
    sealed: bool,
}

impl Plan {
    /// 造计划。
    pub fn from_sets(
        source: SetSpec,
        target: Option<SetSpec>,
        mode: Mode,
        complement: bool,
    ) -> Result<Plan, u32> {
        let rule = Rule::build(&source, target.as_ref(), mode, complement)?;
        Ok(Plan { rule, source, target, sealed: false })
    }

    /// 规则。
    pub fn rule(&self) -> &Rule {
        &self.rule
    }

    /// 源集长度。
    pub fn source_len(&self) -> usize {
        self.source.len()
    }

    /// 目标集长度。
    pub fn target_len(&self) -> usize {
        match self.target {
            Some(t) => t.len(),
            None => 0,
        }
    }

    /// 执行。
    pub fn run(&mut self, inp: &[u8], out: &mut [u8]) -> Result<Stats, u32> {
        if self.sealed {
            return Err(TR_ESTATE);
        }
        self.rule.apply(inp, out)
    }

    /// 封存（此后再跑给 `TR_ESTATE`）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 计划串。
    pub fn plan(&self) -> String {
        let mut s = String::from(self.rule.mode().name());
        s.push_str(" s1=");
        push_dec(&mut s, self.source.len() as u64);
        s.push_str(" s2=");
        push_dec(&mut s, self.target_len() as u64);
        s.push_str(" folded=");
        push_dec(&mut s, self.rule.folded() as u64);
        if self.rule.is_complement() {
            s.push_str(" complement");
        }
        s
    }
}

/// 命令行解析结果。
#[derive(Clone, Copy)]
pub struct Cli {
    /// 计划。
    pub plan: Plan,
    /// 是否补集。
    pub complement: bool,
    /// 模式。
    pub mode: Mode,
}

/// 解析 `tr` 风格参数。
pub fn parse_cli(args: &[&[u8]]) -> Result<Cli, u32> {
    let mut complement = false;
    let mut delete = false;
    let mut squeeze = false;
    let mut operands: [Option<&[u8]>; 2] = [None, None];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        i += 1;
        if a == b"-c" || a == b"-C" || a == b"--complement" {
            complement = true;
        } else if a == b"-d" || a == b"--delete" {
            delete = true;
        } else if a == b"-s" || a == b"--squeeze-repeats" {
            squeeze = true;
        } else if a.starts_with(b"-") && a.len() > 1 {
            return Err(TR_EINVAL);
        } else {
            if n >= 2 {
                return Err(TR_EINVAL);
            }
            if let Some(d) = operands.get_mut(n) {
                *d = Some(a);
            }
            n += 1;
        }
    }
    let mode = match (delete, squeeze) {
        (true, true) => Mode::DeleteSqueeze,
        (true, false) => Mode::Delete,
        (false, true) => Mode::Squeeze,
        (false, false) => Mode::Translate,
    };
    if n == 0 {
        return Err(TR_EINVAL);
    }
    let s1raw = match operands.first().and_then(|v| *v) {
        Some(v) => v,
        None => return Err(TR_EINVAL),
    };
    let s2raw = operands.get(1).and_then(|v| *v);
    if (mode == Mode::Delete) && s2raw.is_some() {
        return Err(TR_ECONF);
    }
    let source = parse_set(s1raw)?;
    let target = match s2raw {
        Some(raw) => Some(parse_set(raw)?),
        None => None,
    };
    if mode == Mode::Translate && target.is_none() {
        // `tr SET1` 无 SET2 时是删除语义的常见误用，明确拒绝。
        return Err(TR_EINVAL);
    }
    let plan = Plan::from_sets(source, target, mode, complement)?;
    Ok(Cli { plan, complement, mode })
}

/// 全 256 字节大小写锚：高位字节必须原样。
pub fn case_anchor_ok() -> bool {
    let mut i = 0u32;
    while i < 256 {
        let b = i as u8;
        if b >= 0x80 {
            if toupper_c(b) != b || tolower_c(b) != b {
                return false;
            }
        }
        if is_upper(b) && tolower_c(b) != b.wrapping_add(0x20) {
            return false;
        }
        if is_lower(b) && toupper_c(b) != b.wrapping_sub(0x20) {
            return false;
        }
        i += 1;
    }
    true
}

/// 类分层不变式（全 256 扫描）。
pub fn layered_ok() -> bool {
    let mut i = 0u32;
    while i < 256 {
        let b = i as u8;
        let m = mask_of(b);
        let up = m & CB_UPPER != 0;
        let lo = m & CB_LOWER != 0;
        let di = m & CB_DIGIT != 0;
        let alpha = m & CB_ALPHA != 0;
        let alnum = m & CB_ALNUM != 0;
        let punct = m & CB_PUNCT != 0;
        let graph = m & CB_GRAPH != 0;
        let print = m & CB_PRINT != 0;
        let space = m & CB_SPACE != 0;
        let blank = m & CB_BLANK != 0;
        let cntrl = m & CB_CNTRL != 0;
        if alpha != (up || lo) {
            return false;
        }
        if alnum != (alpha || di) {
            return false;
        }
        if graph != (alnum || punct) {
            return false;
        }
        // print = graph | (blank & 0x20)
        let sp = b == b' ';
        if print != (graph || sp) {
            return false;
        }
        if punct && alnum {
            return false;
        }
        if cntrl && print {
            return false;
        }
        if space != (blank || b == 0x0a || b == 0x0b || b == 0x0c || b == 0x0d) {
            return false;
        }
        if graph && cntrl {
            return false;
        }
        i += 1;
    }
    true
}

/// 类名总表（用于渲染）。
pub fn class_table_text() -> String {
    let mut s = String::new();
    newline(&mut s);
    s.push_str("CLASS BIT COUNT FIRST LAST");
    let mut i = 0usize;
    let all = Class::all();
    while i < all.len() {
        if let Some(c) = all.get(i) {
            newline(&mut s);
            s.push_str(c.name());
            s.push(' ');
            push_dec(&mut s, 1u64 << c.index());
            s.push(' ');
            push_dec(&mut s, c.count() as u64);
            s.push(' ');
            match c.first() {
                Some(v) => push_hex2(&mut s, v),
                None => s.push_str("--"),
            }
            s.push(' ');
            match c.last() {
                Some(v) => push_hex2(&mut s, v),
                None => s.push_str("--"),
            }
        }
        i += 1;
    }
    newline(&mut s);
    s.push_str("SUM ");
    let mut tot = 0u64;
    let mut j = 0usize;
    while j < all.len() {
        if let Some(c) = all.get(j) {
            tot += c.count() as u64;
        }
        j += 1;
    }
    push_dec(&mut s, tot);
    s
}

/// 类两两交集矩阵（12×12，只报对称上三角的非零项）。
pub fn intersections_text() -> String {
    let mut s = String::new();
    let all = Class::all();
    let mut i = 0usize;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if let (Some(a), Some(b)) = (all.get(i), all.get(j)) {
                let mut n = 0u32;
                let mut k = 0u32;
                while k < 256 {
                    let x = k as u8;
                    if a.has(x) && b.has(x) {
                        n += 1;
                    }
                    k += 1;
                }
                if n != 0 {
                    newline(&mut s);
                    s.push_str(a.name());
                    s.push('&');
                    s.push_str(b.name());
                    s.push(' ');
                    push_dec(&mut s, n as u64);
                }
            }
            j += 1;
        }
        i += 1;
    }
    s
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
            return Err(TR_ESLOT);
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
            return Err(TR_EBAL);
        }
        const VALS: [u32; 8] = [26, 32, 5, 65, 26, 255, 4, 2];
        const CODES: [u32; 6] =
            [TR_EINVAL, TR_ELEN, TR_EBYTE, TR_ERANGE, TR_ESHORT, TR_ECONF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TR_EBAL),
            };
            if s.slot as usize != i {
                return Err(TR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TR_EBAL);
        }
        Ok(())
    }

    /// 对账诊断：首个不符格的 (下标, 期望值或码)。
    pub fn audit_detail(&self) -> Option<(usize, u32)> {
        const VALS: [u32; 8] = [26, 32, 5, 65, 26, 255, 4, 2];
        const CODES: [u32; 6] =
            [TR_EINVAL, TR_ELEN, TR_EBYTE, TR_ERANGE, TR_ESHORT, TR_ECONF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Some((i, TR_EBAL)),
            };
            if s.ok {
                let want = VALS.get(vi).copied().unwrap_or(TR_EBAL);
                if s.value != want {
                    return Some((i, want));
                }
                vi += 1;
            } else {
                let want = CODES.get(ci).copied().unwrap_or(TR_EBAL);
                if s.code != want {
                    return Some((i, want));
                }
                ci += 1;
            }
            i += 1;
        }
        None
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
            return Err(TR_EBAL);
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
            Err(TR_ESEAL)
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

/// 翻译探针：把 `inp` 用 `s1`→`s2` 转换，返回首字节。
fn probe_translate_first(s1: &[u8], s2: &[u8], inp: &[u8]) -> Result<u32, u32> {
    let a = parse_set(s1)?;
    let b = parse_set(s2)?;
    let rule = Rule::build(&a, Some(&b), Mode::Translate, false)?;
    let mut out = [0u8; OUT_CAP];
    rule.apply(inp, &mut out)?;
    match out.first() {
        Some(v) => Ok(*v as u32),
        None => Err(TR_ELEN),
    }
}

/// 删除探针：返回写出长度。
fn probe_delete_len(s1: &[u8], inp: &[u8]) -> Result<usize, u32> {
    let a = parse_set(s1)?;
    let rule = Rule::build(&a, None, Mode::Delete, false)?;
    let mut out = [0u8; OUT_CAP];
    let st = rule.apply(inp, &mut out)?;
    Ok(st.written)
}

/// 压缩探针：返回写出长度。
fn probe_squeeze_len(s1: &[u8], inp: &[u8]) -> Result<usize, u32> {
    let a = parse_set(s1)?;
    let rule = Rule::build(&a, None, Mode::Squeeze, false)?;
    let mut out = [0u8; OUT_CAP];
    let st = rule.apply(inp, &mut out)?;
    Ok(st.written)
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_CLASS, Ok(Class::Upper.count())));

    let _ = l.register(word_cell(1, OP_CLASS, Ok(Class::Punct.count())));

    let _ = l.register(count_cell(2, OP_PARSE, parse_set(b"a-e").map(|s| s.len())));

    let _ = l.register(
        word_cell(3, OP_XLATE, probe_translate_first(b"abc", b"ABC", b"abc")),
    );

    let _ = l.register(word_cell(4, OP_XLATE, {
        let a = parse_set(b"[:lower:]");
        let b = parse_set(b"[:upper:]");
        match (a, b) {
            (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                Ok(r) => Ok(r.folded() as u32),
                Err(c) => Err(c),
            },
            (Err(c), _) => Err(c),
            (_, Err(c)) => Err(c),
        }
    }));

    let _ = l.register(word_cell(5, OP_COMP, {
        match parse_set(b"a") {
            Ok(s) => Ok(s.to_set().complement().count()),
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(count_cell(6, OP_DEL, probe_delete_len(b"aeiou", b"education")));

    let _ = l.register(count_cell(7, OP_SQUEEZE, probe_squeeze_len(b"ab", b"aaabbb")));

    let _ = l.register(word_cell(8, OP_NAME, Class::parse(b"foo").map(|c| c.index() as u32)));

    let _ = l.register(count_cell(9, OP_PARSE, parse_set(b"").map(|s| s.len())));

    let _ = l.register(count_cell(10, OP_PARSE, parse_set(b"\\q").map(|s| s.len())));

    let _ = l.register(count_cell(11, OP_PARSE, parse_set(b"z-a").map(|s| s.len())));

    let _ = l.register(count_cell(12, OP_PARSE, parse_set(b"[:digit").map(|s| s.len())));

    let _ = l.register(word_cell(13, OP_CLI, {
        match parse_cli(&[b"-d", b"a", b"b"]) {
            Ok(_) => Ok(1),
            Err(c) => Err(c),
        }
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let a = Class::Upper.count() == 26
            && Class::Lower.count() == 26
            && Class::Digit.count() == 10
            && Class::Xdigit.count() == 22
            && Class::Alpha.count() == 52
            && Class::Alnum.count() == 62
            && Class::Punct.count() == 32
            && Class::Graph.count() == 94
            && Class::Print.count() == 95
            && Class::Cntrl.count() == 33
            && Class::Space.count() == 6
            && Class::Blank.count() == 2;
        let b = layered_ok();
        let c = case_anchor_ok();
        a && b && c
    };

    out[1] = {
        let mut ok = true;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            if is_alpha(b) != (is_upper(b) || is_lower(b)) {
                ok = false;
            }
            if is_alnum(b) != (is_alpha(b) || is_digit(b)) {
                ok = false;
            }
            if is_punct(b) != (is_graph(b) && !is_alnum(b)) {
                ok = false;
            }
            if is_upper(b) && (toupper_c(b) != b || toupper_c(tolower_c(b)) != b) {
                ok = false;
            }
            if is_lower(b) && (tolower_c(b) != b || tolower_c(toupper_c(b)) != b) {
                ok = false;
            }
            if !is_alpha(b) && (toupper_c(b) != b || tolower_c(b) != b) {
                ok = false;
            }
            i += 1;
        }
        ok
    };

    out[2] = {
        let mut l = build_ledger();
        let audited = l.audit().is_ok();
        let sealed = l.seal();
        let replayed = match l.replay() {
            Ok(v) => v == sealed,
            Err(_) => false,
        };
        let frozen = l.freeze().is_ok();
        audited && replayed && frozen && l.is_frozen() && l.rejects() == 0
    };

    out[3] = {
        let mut plan = match parse_cli(&[b"-c", b"a", b"z"]) {
            Ok(v) => v.plan,
            Err(_) => Plan {
                rule: Rule::identity(),
                source: SetSpec::new(),
                target: None,
                sealed: true,
            },
        };
        let mut outbuf = [0u8; OUT_CAP];
        let st = match plan.run(b"ab", &mut outbuf) {
            Ok(v) => v,
            Err(_) => Stats::zero(),
        };
        let mapped_ok = outbuf.get(0) == Some(&b'a') && outbuf.get(1) == Some(&b'z');
        let stats_ok = st.written == 2 && st.mapped == 1;
        plan.seal();
        let sealed_ok = plan.run(b"a", &mut outbuf) == Err(TR_ESTATE);
        mapped_ok && stats_ok && sealed_ok
    };

    out
}

/// 判据集合。
pub fn run_unx11567_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11567.tr");

    s.add(
        "TR01-类表基数",
        {
            let a = Class::Upper.count() == 26;
            let b = Class::Lower.count() == 26;
            let c = Class::Digit.count() == 10;
            let d = Class::Xdigit.count() == 22;
            let e = Class::Alpha.count() == 52;
            let f = Class::Alnum.count() == 62;
            let g = Class::Punct.count() == 32;
            let h = Class::Space.count() == 6;
            let i = Class::Blank.count() == 2;
            let j = Class::Cntrl.count() == 33;
            let k = Class::Graph.count() == 94;
            let l = Class::Print.count() == 95;
            let m = Class::all().len() == CLASS_COUNT;
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "TR02-类成员抽样",
        {
            let mut ok = true;
            if !is_upper(b'A') || is_upper(b'a') || is_upper(b'0') {
                ok = false;
            }
            if !is_lower(b'z') || is_lower(b'Z') {
                ok = false;
            }
            if !is_digit(b'7') || is_digit(b'a') {
                ok = false;
            }
            if !is_xdigit(b'f') || !is_xdigit(b'F') || !is_xdigit(b'9') || is_xdigit(b'g') {
                ok = false;
            }
            if !is_punct(b'@') || is_punct(b'A') || is_punct(b' ') {
                ok = false;
            }
            if !is_blank(b'\t') || !is_blank(b' ') || is_blank(b'\n') {
                ok = false;
            }
            if !is_cntrl(0x7f) || is_cntrl(b' ') || is_cntrl(0x80) {
                ok = false;
            }
            if !is_print(b' ') || is_print(0x7f) || is_print(0x80) {
                ok = false;
            }
            if !is_graph(b'!') || is_graph(b' ') {
                ok = false;
            }
            if !is_space(b'\n') || !is_space(0x0b) || !is_space(b' ') || is_space(b'_') {
                ok = false;
            }
            if is_alpha(0x80) || is_alnum(0xff) || is_lower(0xe9) {
                ok = false;
            }
            if mask_of(0) != CB_CNTRL || mask_of(b' ') != (CB_SPACE | CB_BLANK | CB_PRINT) {
                ok = false;
            }
            if mask_of(b'A') != (CB_UPPER | CB_ALPHA | CB_ALNUM | CB_XDIGIT | CB_GRAPH | CB_PRINT)
            {
                ok = false;
            }
            ok
        },
        "static",
    );

    s.add(
        "TR03-分层不变式",
        {
            let mut ok = layered_ok();
            let mut eq_alpha = 0u32;
            let mut eq_alnum = 0u32;
            let mut eq_graph = 0u32;
            let mut eq_print = 0u32;
            let mut i = 0u32;
            while i < 256 {
                let b = i as u8;
                if is_alpha(b) == (is_upper(b) || is_lower(b)) {
                    eq_alpha += 1;
                }
                if is_alnum(b) == (is_alpha(b) || is_digit(b)) {
                    eq_alnum += 1;
                }
                if is_graph(b) == (is_alnum(b) || is_punct(b)) {
                    eq_graph += 1;
                }
                if is_print(b) == (is_graph(b) || b == b' ') {
                    eq_print += 1;
                }
                i += 1;
            }
            ok = ok && eq_alpha == 256 && eq_alnum == 256 && eq_graph == 256 && eq_print == 256;
            let xd = Class::Xdigit.expand();
            let mut xd_expect = Set256::empty();
            let mut k = b'0';
            while k <= b'9' {
                xd_expect.insert(k);
                k += 1;
            }
            let mut k2 = b'A';
            while k2 <= b'F' {
                xd_expect.insert(k2);
                k2 += 1;
            }
            let mut k3 = b'a';
            while k3 <= b'f' {
                xd_expect.insert(k3);
                k3 += 1;
            }
            ok && xd == xd_expect
        },
        "static",
    );

    s.add(
        "TR04-类展开序",
        {
            let mut ok = true;
            let up = Class::Upper.expand();
            if up.count() != 26 || up.min() != Some(b'A') || up.max() != Some(b'Z') {
                ok = false;
            }
            let lo = Class::Lower.expand();
            if lo.min() != Some(b'a') || lo.max() != Some(b'z') {
                ok = false;
            }
            let di = Class::Digit.expand();
            if di.min() != Some(b'0') || di.max() != Some(b'9') {
                ok = false;
            }
            let xd = Class::Xdigit.expand();
            if xd.nth(0) != Some(b'0')
                || xd.nth(9) != Some(b'9')
                || xd.nth(10) != Some(b'A')
                || xd.nth(15) != Some(b'F')
                || xd.nth(16) != Some(b'a')
                || xd.nth(21) != Some(b'f')
            {
                ok = false;
            }
            let mut buf = [0u8; 128];
            let n = match Class::Upper.expand_into(&mut buf) {
                Ok(v) => v,
                Err(_) => 0,
            };
            if n != 26 {
                ok = false;
            }
            let mut i = 0usize;
            while i < n {
                match (buf.get(i), b'A'.checked_add(i as u8)) {
                    (Some(a), Some(b)) => {
                        if *a != b {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            let empty = Class::Punct.expand();
            if empty.nth(0) != Some(b'!') || empty.nth(31) != Some(b'~') {
                ok = false;
            }
            if Class::Punct.expand().nth(32).is_some() {
                ok = false;
            }
            ok
        },
        "static",
    );

    s.add(
        "TR05-集合解析",
        {
            let mut ok = true;
            let a = match parse_set(b"abc") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if a.len() != 3 || !a.equals(b"abc") {
                ok = false;
            }
            let r = match parse_set(b"a-e") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if r.len() != 5 || !r.equals(b"abcde") {
                ok = false;
            }
            let e = match parse_set(b"\\n\\t\\101") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if e.len() != 3 || e.at(0) != Some(0x0a) || e.at(1) != Some(0x09) || e.at(2) != Some(65)
            {
                ok = false;
            }
            let c = match parse_set(b"[:digit:]") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if c.len() != 10 || c.at(0) != Some(b'0') || c.at(9) != Some(b'9') {
                ok = false;
            }
            let m = match parse_set(b"x[:digit:]z") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if m.len() != 12 || m.at(0) != Some(b'x') || m.at(11) != Some(b'z') {
                ok = false;
            }
            let rep = match parse_set(b"[a*3]") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if rep.len() != 3 || !rep.equals(b"aaa") {
                ok = false;
            }
            let trailing = match parse_set(b"a-") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if trailing.len() != 2 || !trailing.equals(b"a-") {
                ok = false;
            }
            let lead = match parse_set(b"-x") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if lead.len() != 2 || !lead.equals(b"-x") {
                ok = false;
            }
            let hex = match parse_set(b"\\x41\\x7e") {
                Ok(v) => v,
                Err(_) => SetSpec::new(),
            };
            if hex.len() != 2 || hex.at(0) != Some(b'A') || hex.at(1) != Some(b'~') {
                ok = false;
            }
            ok
        },
        "static",
    );

    s.add(
        "TR06-解析闸门",
        {
            let a = Class::parse(b"nope") == Err(TR_EINVAL);
            let b = parse_set(b"") == Err(TR_ELEN);
            let c = parse_set(b"\\q") == Err(TR_EBYTE);
            let d = parse_set(b"z-a") == Err(TR_ERANGE);
            let e = parse_set(b"[:digit") == Err(TR_ESHORT);
            let f = parse_set(b"[:upper:]-z") == Err(TR_EINVAL);
            let g = parse_set(b"\\400") == Err(TR_ERANGE);
            let h = parse_set(b"\\") == Err(TR_ESHORT);
            let i = parse_set(b"[a*0]") == Err(TR_ERANGE);
            let j = parse_set(b"[a*99]") == Err(TR_ERANGE);
            let k = parse_set(b"[a*]") == Err(TR_EINVAL);
            let l = parse_set(b"\\x") == Err(TR_EBYTE);
            let m = parse_cli(&[b"-d", b"abc", b"xyz"]).err() == Some(TR_ECONF);
            let n = parse_cli(&[b"-q", b"abc"]).err() == Some(TR_EINVAL);
            let o = parse_cli(&[b"-d"]).err() == Some(TR_EINVAL);
            let p = parse_cli(&[b"abc"]).err() == Some(TR_EINVAL);
            let q = {
                let mut set = Set256::empty();
                let first = set.insert_checked(65) == Ok(());
                let second = set.insert_checked(65) == Err(TR_EDUP);
                first && second && set.count() == 1
            };
            let r = parse_set(b"[a*65]") == Err(TR_ERANGE);
            a && b && c && d && e && f && g && h && i && j && k && l && m && n && o && p && q && r
        },
        "static",
    );

    s.add(
        "TR07-翻译语义",
        {
            let a = {
                let s1 = parse_set(b"abc");
                let s2 = parse_set(b"ABC");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let mut out = [0u8; 8];
                            match r.apply(b"cab", &mut out) {
                                Ok(st) => {
                                    st.written == 3
                                        && out.get(0) == Some(&b'C')
                                        && out.get(1) == Some(&b'A')
                                        && out.get(2) == Some(&b'B')
                                        && st.mapped == 3
                                        && r.folded() == 3
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let b = {
                let s1 = parse_set(b"abcd");
                let s2 = parse_set(b"xy");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let m = r.mapping();
                            m.get(0x61) == Some(&b'x')
                                && m.get(0x62) == Some(&b'y')
                                && m.get(0x63) == Some(&b'y')
                                && m.get(0x64) == Some(&b'y')
                                && m.get(0x65) == Some(&b'e')
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let c = {
                let s1 = parse_set(b"ab");
                let s2 = parse_set(b"XYZ");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let m = r.mapping();
                            m.get(0x61) == Some(&b'X')
                                && m.get(0x62) == Some(&b'Y')
                                && m.get(0x5a) == Some(&b'Z')
                                && r.folded() == 2
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let d = {
                let a1 = parse_set(b"a-c");
                let b1 = parse_set(b"A-C");
                let a2 = parse_set(b"abc");
                let b2 = parse_set(b"ABC");
                match (a1, b1, a2, b2) {
                    (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) => {
                        let r1 = Rule::build(&x1, Some(&y1), Mode::Translate, false);
                        let r2 = Rule::build(&x2, Some(&y2), Mode::Translate, false);
                        match (r1, r2) {
                            (Ok(u), Ok(v)) => u.mapping() == v.mapping() && u.folded() == v.folded(),
                            _ => false,
                        }
                    }
                    _ => false,
                }
            };
            let e = {
                let a1 = parse_set(b"[:lower:]");
                let b1 = parse_set(b"[:upper:]");
                match (a1, b1) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => r.is_bijective() && r.folded() == 26,
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TR08-补集语义",
        {
            let a = match parse_set(b"a") {
                Ok(s) => s.to_set().complement().count() == 255,
                Err(_) => false,
            };
            let b = {
                let s1 = parse_set(b"a");
                let s2 = parse_set(b"z");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, true) {
                        Ok(r) => {
                            let mut out = [0u8; 8];
                            match r.apply(b"az", &mut out) {
                                Ok(st) => {
                                    out.get(0) == Some(&b'a')
                                        && out.get(1) == Some(&b'z')
                                        && st.mapped == 0
                                        && st.complemented == 2
                                        && r.folded() == 254
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let c = {
                let s1 = parse_set(b"[:lower:]");
                let s2 = parse_set(b"[:upper:]");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, true) {
                        Ok(r) => {
                            let m = r.mapping();
                            m.get(0x61) == Some(&0x61) && m.get(0x41) == Some(&0x5a)
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let d = {
                let s1 = parse_set(b"abc");
                match Rule::build(&s1.unwrap_or(SetSpec::new()), None, Mode::Delete, true) {
                    Ok(r) => r.del_set().count() == 253,
                    Err(_) => false,
                }
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "TR09-删除与压缩",
        {
            let a = {
                let s1 = parse_set(b"aeiou");
                match s1 {
                    Ok(x) => match Rule::build(&x, None, Mode::Delete, false) {
                        Ok(r) => {
                            let mut out = [0u8; 32];
                            match r.apply(b"education", &mut out) {
                                Ok(st) => {
                                    st.written == 4
                                        && st.deleted == 5
                                        && out.get(0) == Some(&b'd')
                                        && out.get(1) == Some(&b'c')
                                        && out.get(2) == Some(&b't')
                                        && out.get(3) == Some(&b'n')
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            };
            let b = {
                let s1 = parse_set(b"ab");
                match s1 {
                    Ok(x) => match Rule::build(&x, None, Mode::Squeeze, false) {
                        Ok(r) => {
                            let mut out = [0u8; 32];
                            match r.apply(b"aaabbb", &mut out) {
                                Ok(st) => {
                                    st.written == 2
                                        && st.squeezed == 4
                                        && out.get(0) == Some(&b'a')
                                        && out.get(1) == Some(&b'b')
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            };
            let c = {
                let s1 = parse_set(b"a");
                let s2 = parse_set(b"x");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Squeeze, false) {
                        Ok(r) => {
                            let mut out = [0u8; 32];
                            match r.apply(b"aaab", &mut out) {
                                Ok(st) => {
                                    st.written == 2
                                        && st.squeezed == 2
                                        && out.get(0) == Some(&b'x')
                                        && out.get(1) == Some(&b'b')
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let d = {
                let s1 = parse_set(b"a");
                let s2 = parse_set(b"a");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Squeeze, false) {
                        Ok(r) => {
                            let mut out = [0u8; 32];
                            match r.apply(b"aaa", &mut out) {
                                Ok(st) => st.written == 1 && st.squeezed == 2,
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let e = {
                let s1 = parse_set(b"age");
                match s1 {
                    Ok(x) => match Rule::build(&x, None, Mode::DeleteSqueeze, false) {
                        Ok(r) => {
                            let mut out = [0u8; 32];
                            match r.apply(b"aaggee", &mut out) {
                                Ok(st) => st.written == 0 && st.deleted == 6,
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TR10-大小写锚",
        {
            let mut ok = case_anchor_ok();
            let a = {
                let s1 = parse_set(b"[:lower:]");
                let s2 = parse_set(b"[:upper:]");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let mut out = [0u8; 300];
                            match r.apply(b"Hello, World 123", &mut out) {
                                Ok(st) => {
                                    st.written == 16
                                        && out.get(0) == Some(&b'H')
                                        && out.get(1) == Some(&b'E')
                                        && out.get(7) == Some(&b'W')
                                        && out.get(8) == Some(&b'O')
                                        && r.folded() == 26
                                }
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let b = {
                let s1 = parse_set(b"[:lower:]");
                let s2 = parse_set(b"[:upper:]");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let m = r.mapping();
                            let mut i = 0u32;
                            let mut hit = true;
                            while i < 256 {
                                let c = i as u8;
                                if let Some(v) = m.get(i as usize) {
                                    if *v != toupper_c(c) {
                                        hit = false;
                                    }
                                }
                                i += 1;
                            }
                            hit
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let c = {
                let mut probe = [0u8; 4];
                probe[0] = 0x80;
                probe[1] = 0xe9;
                probe[2] = 0xff;
                probe[3] = b'a';
                toupper_c(probe[0]) == 0x80
                    && toupper_c(probe[1]) == 0xe9
                    && toupper_c(probe[2]) == 0xff
                    && toupper_c(probe[3]) == b'A'
                    && tolower_c(b'Z') == b'z'
                    && tolower_c(b'z') == b'z'
            };
            ok = ok && a && b && c;
            ok
        },
        "static",
    );

    s.add(
        "TR11-渲染总表",
        {
            let text = class_table_text();
            let a = text.starts_with("CLASS BIT COUNT FIRST LAST");
            let b = text.contains("upper 1 26 41 5a");
            let c = text.contains("punct 64 32 21 7e");
            let d = text.contains("print 2048 95 20 7e");
            let e = text.split('\n').count() == 14;
            let f = text.contains("SUM ");
            let g = {
                let s1 = parse_set(b"[:lower:]");
                let s2 = parse_set(b"[:upper:]");
                match (s1, s2) {
                    (Ok(x), Ok(y)) => match Rule::build(&x, Some(&y), Mode::Translate, false) {
                        Ok(r) => {
                            let t = r.render_table();
                            t.matches('>').count() == 26
                                && t.contains("61>41")
                                && t.contains("7a>5a")
                                && t.contains("folded 26")
                        }
                        Err(_) => false,
                    },
                    _ => false,
                }
            };
            let h = {
                match parse_cli(&[b"a-e", b"A-E"]) {
                    Ok(c) => {
                        let p = c.plan.plan();
                        p == String::from("translate s1=5 s2=5 folded=5")
                    }
                    Err(_) => false,
                }
            };
            let i = {
                let t = intersections_text();
                t.contains("upper&xdigit 6") && t.contains("lower&xdigit 6")
            };
            text.len() <= 2048 && a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "TR12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = l.audit_detail().is_none();
            let e = match l.sample(0) {
                Some(v) => v.ok && v.value == 26 && v.op == OP_CLASS,
                None => false,
            };
            let f = match l.sample(13) {
                Some(v) => !v.ok && v.code == TR_ECONF && v.op == OP_CLI,
                None => false,
            };
            let g = l.replay() == Ok(l.seal());
            let h = l.sample(SAMPLES).is_none();
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "TR13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut l = build_ledger();
            let sealed = l.seal();
            let replay_ok = l.replay() == Ok(sealed);
            let frozen = l.freeze() == Ok(sealed);
            let late = l.register(Sample { slot: 13, op: OP_CLASS, ok: true, code: 0, value: 1 });
            let rej1 = l.rejects();
            let again = l.freeze();
            let mut plan = match parse_cli(&[b"[:upper:]", b"[:lower:]"]) {
                Ok(c) => c.plan,
                Err(_) => Plan::from_sets(SetSpec::new(), None, Mode::Translate, false)
                    .unwrap_or_else(|_| Plan { rule: Rule::identity(), source: SetSpec::new(), target: None, sealed: true }),
            };
            plan.seal();
            let mut outbuf = [0u8; 4];
            let state = plan.run(b"A", &mut outbuf) == Err(TR_ESTATE);
            ok = ok
                && replay_ok
                && frozen
                && late == Err(TR_ESLOT)
                && rej1 == 1
                && again == Err(TR_EBAL)
                && l.rejects() == 2
                && state;
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
    fn class_counts() {
        assert_eq!(Class::Upper.count(), 26);
        assert_eq!(Class::Lower.count(), 26);
        assert_eq!(Class::Digit.count(), 10);
        assert_eq!(Class::Xdigit.count(), 22);
        assert_eq!(Class::Alpha.count(), 52);
        assert_eq!(Class::Alnum.count(), 62);
        assert_eq!(Class::Punct.count(), 32);
        assert_eq!(Class::Space.count(), 6);
        assert_eq!(Class::Blank.count(), 2);
        assert_eq!(Class::Cntrl.count(), 33);
        assert_eq!(Class::Graph.count(), 94);
        assert_eq!(Class::Print.count(), 95);
    }

    #[test]
    fn class_parse_names() {
        assert_eq!(Class::parse(b"upper"), Ok(Class::Upper));
        assert_eq!(Class::parse(b"print"), Ok(Class::Print));
        assert_eq!(Class::parse(b"digit"), Ok(Class::Digit));
        assert_eq!(Class::parse(b"xdigit"), Ok(Class::Xdigit));
        assert_eq!(Class::parse(b"UPPER"), Err(TR_EINVAL));
        assert_eq!(Class::parse(b""), Err(TR_EINVAL));
    }

    #[test]
    fn layer_identities() {
        assert!(layered_ok());
        assert!(is_alpha(b'A') && is_alnum(b'A') && !is_punct(b'A'));
        assert!(is_punct(b'!') && is_graph(b'!') && is_print(b'!'));
        assert!(is_space(b'\t') && is_blank(b'\t') && !is_space(b'a'));
    }

    #[test]
    fn mask_exact() {
        assert_eq!(mask_of(b'\0'), CB_CNTRL);
        assert_eq!(mask_of(0x7f), CB_CNTRL);
        assert_eq!(mask_of(0x20), CB_SPACE | CB_BLANK | CB_PRINT);
        assert_eq!(mask_of(0x80), 0);
        assert_eq!(
            mask_of(b'F'),
            CB_UPPER | CB_ALPHA | CB_ALNUM | CB_XDIGIT | CB_GRAPH | CB_PRINT
        );
    }

    #[test]
    fn set_insert_dup() {
        let mut s = Set256::empty();
        assert_eq!(s.insert_checked(1), Ok(()));
        assert_eq!(s.insert_checked(1), Err(TR_EDUP));
        assert_eq!(s.count(), 1);
        assert!(s.contains(1));
        assert!(!s.contains(2));
        assert!(s.remove(1));
        assert!(s.is_empty());
    }

    #[test]
    fn set_complement_roundtrip() {
        let mut s = Set256::empty();
        s.insert(0);
        s.insert(255);
        let c = s.complement();
        assert_eq!(s.count(), 2);
        assert_eq!(c.count(), 254);
        assert_eq!(c.complement(), s);
    }

    #[test]
    fn set_write_order() {
        let mut s = Set256::empty();
        s.insert(200);
        s.insert(3);
        s.insert(9);
        let mut buf = [0u8; 4];
        let n = s.write_into(&mut buf).expect("write");
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], &[3, 9, 200]);
        let mut tiny = [0u8; 2];
        assert_eq!(s.write_into(&mut tiny), Err(TR_ESHORT));
    }

    #[test]
    fn parse_basic_sets() {
        assert_eq!(parse_set(b"abc").expect("set").as_slice(), b"abc");
        let r = parse_set(b"a-c").expect("range");
        assert!(r.equals(b"abc"));
        let e = parse_set(b"\\t\\n\\101").expect("esc");
        assert_eq!(e.as_slice(), &[9, 10, 65]);
        let c = parse_set(b"[:digit:]").expect("class");
        assert_eq!(c.len(), 10);
    }

    #[test]
    fn parse_gates() {
        assert_eq!(parse_set(b""), Err(TR_ELEN));
        assert_eq!(parse_set(b"z-a"), Err(TR_ERANGE));
        assert_eq!(parse_set(b"\\q"), Err(TR_EBYTE));
        assert_eq!(parse_set(b"[:digit"), Err(TR_ESHORT));
        assert_eq!(parse_set(b"[a*0]"), Err(TR_ERANGE));
        assert_eq!(parse_set(b"[:nosuch:]"), Err(TR_EINVAL));
    }

    #[test]
    fn translate_padding() {
        let a = parse_set(b"abcd").expect("a");
        let b = parse_set(b"xy").expect("b");
        let r = Rule::build(&a, Some(&b), Mode::Translate, false).expect("rule");
        assert_eq!(r.mapping()[0x61], b'x');
        assert_eq!(r.mapping()[0x62], b'y');
        assert_eq!(r.mapping()[0x63], b'y');
        assert_eq!(r.mapping()[0x64], b'y');
        assert_eq!(r.folded(), 4);
    }

    #[test]
    fn translate_apply() {
        let a = parse_set(b"abc").expect("a");
        let b = parse_set(b"ABC").expect("b");
        let r = Rule::build(&a, Some(&b), Mode::Translate, false).expect("rule");
        let mut out = [0u8; 8];
        let st = r.apply(b"aabbcc", &mut out).expect("apply");
        assert_eq!(st.written, 6);
        assert_eq!(st.mapped, 6);
        assert_eq!(&out[..6], b"AABBCC");
    }

    #[test]
    fn delete_and_squeeze() {
        let a = parse_set(b"aeiou").expect("a");
        let r = Rule::build(&a, None, Mode::Delete, false).expect("rule");
        let mut out = [0u8; 32];
        let st = r.apply(b"education", &mut out).expect("apply");
        assert_eq!(st.written, 4);
        assert_eq!(&out[..4], b"dctn");
    }

    #[test]
    fn cli_conflicts() {
        assert_eq!(parse_cli(&[b"-d", b"a", b"b"]).err(), Some(TR_ECONF));
        assert_eq!(parse_cli(&[b"-q", b"a"]).err(), Some(TR_EINVAL));
        assert_eq!(parse_cli(&[b"-d"]).err(), Some(TR_EINVAL));
        assert!(parse_cli(&[b"-s", b"[:space:]"]).is_ok());
    }

    #[test]
    fn render_table_has_pairs() {
        let a = parse_set(b"[:lower:]").expect("a");
        let b = parse_set(b"[:upper:]").expect("b");
        let r = Rule::build(&a, Some(&b), Mode::Translate, false).expect("rule");
        let t = r.render_table();
        assert!(t.contains("61>41"));
        assert!(t.contains("7a>5a"));
        assert_eq!(t.matches('>').count(), 26);
    }

    #[test]
    fn seal_freeze_replay() {
        let mut l = build_ledger();
        let sealed = l.seal();
        assert_eq!(l.replay().expect("replay"), sealed);
        assert_eq!(l.freeze().expect("freeze"), sealed);
        assert!(l.is_frozen());
        let late = Sample { slot: 13, op: OP_CLASS, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(late), Err(TR_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze(), Err(TR_EBAL));
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert!(l.audit_detail().is_none());
        assert_eq!(l.sample(0).map(|v| v.value), Some(26));
        assert_eq!(l.sample(13).map(|v| v.code), Some(TR_ECONF));
    }

    #[test]
    fn injection_four() {
        let inj = inject_four();
        assert!(inj[0]);
        assert!(inj[1]);
        assert!(inj[2]);
        assert!(inj[3]);
    }

    #[test]
    fn checks_all_pass() {
        assert!(run_unx11567_checks().all_passed());
    }
}
