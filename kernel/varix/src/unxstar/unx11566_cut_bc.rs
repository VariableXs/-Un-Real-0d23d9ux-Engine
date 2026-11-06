//! UNX-F11566 cut -b/-c 在 C locale 等价判据
//!
//! `cut -b` 按字节选，`cut -c` 按字符选。两者只有在「一个字符恰好一个字节」时
//! 才等价；C locale 正是这样的单字节字符集（`MB_CUR_MAX == 1`），于是
//! `-b 2-4` 与 `-c 2-4` 对同一行必须给出同一串字节，连 0x80–0xff 也一样：
//! `-c1` 在 `e4 b8 ad` 上只取 `e4`，不会把三字节当成一个 UTF-8 字符。
//! 这条等价不是「实现成同一个函数」就能证明的，所以本单把两条选择路径分开写，
//! 再在全语料上逐行对拍；等价判据是**观测结果**，不是代码结构。
//!
//! 三层结构：
//!
//! 1. **位置集 `PosSet`**。128 位，位置一基。`set` 对 0 与 129 给
//!    `CT_ERANGE`，其余越界一律拒绝；集合运算只有并、交、计数。
//! 2. **列表 `List`**。解析 `1,3-5,7-`、`-3`、`2-`、`4`，逗号分隔、区间闭
//!    端点、开放端 `N-`，重复与乱序都接受（选择语义与顺序无关），但空段、
//!    非数字、零、超界、倒序区间全部拒绝。列表还负责 `count_upto`，用它与
//!    位集互相校验。
//! 3. **选项与执行 `Opts`**。`-b/-c/-f` 三选一（重复或缺失都是
//!    `CT_EMODE`），`-d` 只允许单字节且只与 `-f` 同用，`-s` 只抑制无分隔符
//!    行，`--complement` 取反。字段模式按分隔符切分，保留尾随空字段；无分隔
//!    符行按 POSIX 原样输出（除非 `-s`）。
//!
//! 闸门次序：列表语法（`CT_ELIST`）→ 位置范围（`CT_ERANGE`）→ 列表容量
//! （`CT_ECAP`）→ 模式选择（`CT_EMODE`）→ 选项标志（`CT_EFLAG`）→ 分隔符
//! （`CT_EDELIM`）→ 操作数缺失（`CT_ESHORT`）→ 执行状态（`CT_ESTATE`）→
//! 账本槽位（`CT_ESLOT`）→ 账本值（`CT_EVALUE`）→ 指纹（`CT_ESEAL`）。
//!
//! 账本语义：十四格 = 八格成功（字节选择首行长、字符选择首行长、补集首行长、
//! 第二字段长、开放字段长、列表计数、去重计数、对拍位置数）+ 六格失败
//! （坏列表、零位置、缺模式、坏标志、坏分隔符、缺操作数）。`audit` 认
//! 「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf470;

/// 列表语法错误。
pub const CT_ELIST: u32 = SEG_BASE + 0x01;
/// 位置为零、越界或区间倒序。
pub const CT_ERANGE: u32 = SEG_BASE + 0x02;
/// 模式缺失或重复。
pub const CT_EMODE: u32 = SEG_BASE + 0x03;
/// 未知选项或非法组合。
pub const CT_EFLAG: u32 = SEG_BASE + 0x04;
/// 分隔符不是单字节或与模式不配。
pub const CT_EDELIM: u32 = SEG_BASE + 0x05;
/// 操作数缺失或列表为空。
pub const CT_ESHORT: u32 = SEG_BASE + 0x06;
/// 列表段数或字段数超容量。
pub const CT_ECAP: u32 = SEG_BASE + 0x07;
/// 执行状态不成立。
pub const CT_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const CT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const CT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const CT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const CT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const CT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 位置上限。
pub const POS_MAX: u16 = 128;
/// 列表段容量。
pub const LIST_CAP: usize = 12;
/// 字段容量。
pub const FIELDS_CAP: usize = 32;
/// 行容量。
pub const LINE_CAP: usize = 96;
/// 语料行数容量。
pub const LINES_CAP: usize = 16;
/// C locale 的单字节字符宽度。
pub const MB_CUR_MAX: usize = 1;

/// 操作码：列表。
pub const OP_LIST: u8 = 0x01;
/// 操作码：区间。
pub const OP_RANGE: u8 = 0x02;
/// 操作码：字节选择。
pub const OP_BYTES: u8 = 0x03;
/// 操作码：字符选择。
pub const OP_CHARS: u8 = 0x04;
/// 操作码：补集。
pub const OP_COMP: u8 = 0x05;
/// 操作码：字段。
pub const OP_FIELD: u8 = 0x06;
/// 操作码：开放端。
pub const OP_OPEN: u8 = 0x07;
/// 操作码：对拍。
pub const OP_SWEEP: u8 = 0x08;
/// 操作码：坏列表。
pub const OP_BADLIST: u8 = 0x09;
/// 操作码：零位置。
pub const OP_ZERO: u8 = 0x0a;
/// 操作码：缺模式。
pub const OP_NOMODE: u8 = 0x0b;
/// 操作码：坏标志。
pub const OP_BADFLAG: u8 = 0x0c;
/// 操作码：坏分隔符。
pub const OP_BADDELIM: u8 = 0x0d;
/// 操作码：缺操作数。
pub const OP_SHORT: u8 = 0x0e;

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

/// 取一个字节。
fn at(raw: &[u8], i: usize) -> Option<u8> {
    raw.get(i).copied()
}

/// 128 位位置集合（位置一基）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PosSet {
    w: [u64; 2],
}

impl PosSet {
    /// 空集。
    pub const fn new() -> PosSet {
        PosSet { w: [0u64; 2] }
    }

    /// 置一位；0 或超过 128 即越界。
    pub fn set(&mut self, pos: u16) -> Result<(), u32> {
        if pos == 0 || pos > POS_MAX {
            return Err(CT_ERANGE);
        }
        let idx = (pos - 1) as usize;
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(w) = self.w.get_mut(word) {
            *w |= 1u64 << bit;
        }
        Ok(())
    }

    /// 清一位。
    pub fn clear(&mut self, pos: u16) {
        if pos == 0 || pos > POS_MAX {
            return;
        }
        let idx = (pos - 1) as usize;
        let word = idx / 64;
        let bit = idx % 64;
        if let Some(w) = self.w.get_mut(word) {
            *w &= !(1u64 << bit);
        }
    }

    /// 查询。
    pub fn has(&self, pos: u16) -> bool {
        if pos == 0 || pos > POS_MAX {
            return false;
        }
        let idx = (pos - 1) as usize;
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
        while i < 2 {
            if let Some(w) = self.w.get(i) {
                n = n.saturating_add(w.count_ones());
            }
            i += 1;
        }
        n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// 并集。
    pub fn union(&mut self, other: &PosSet) {
        let mut i = 0usize;
        while i < 2 {
            if let (Some(a), Some(b)) = (self.w.get_mut(i), other.w.get(i)) {
                *a |= *b;
            }
            i += 1;
        }
    }
}

/// 一个闭区间；`open` 表示 `lo-`（到行尾）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// 下界（一基）。
    pub lo: u16,
    /// 上界（一基；开放端时为 0）。
    pub hi: u16,
    /// 是否开放端。
    pub open: bool,
}

impl Range {
    /// 空白区间。
    pub const fn blank() -> Range {
        Range { lo: 0, hi: 0, open: false }
    }

    /// 是否含某位置。
    pub fn contains(&self, pos: u16) -> bool {
        if self.open {
            return pos >= self.lo;
        }
        pos >= self.lo && pos <= self.hi
    }

    /// 诊断串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.lo as u64);
        if self.open {
            s.push('-');
        } else if self.hi != self.lo {
            s.push('-');
            push_dec(&mut s, self.hi as u64);
        }
        s
    }
}

/// 位置列表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct List {
    ranges: [Range; LIST_CAP],
    len: usize,
    open_any: bool,
}

impl List {
    /// 空列表。
    pub const fn empty() -> List {
        List { ranges: [Range::blank(); LIST_CAP], len: 0, open_any: false }
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 段。
    pub fn range(&self, i: usize) -> Option<Range> {
        if i >= self.len {
            return None;
        }
        self.ranges.get(i).copied()
    }

    /// 是否含某位置。
    pub fn contains(&self, pos: u16) -> bool {
        if pos == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.ranges.get(i) {
                if r.contains(pos) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 在 1..=limit 内的位置数。
    pub fn count_upto(&self, limit: u16) -> u32 {
        let mut n = 0u32;
        let mut p = 1u16;
        while p <= limit {
            if self.contains(p) {
                n = n.saturating_add(1);
            }
            if p == u16::MAX {
                break;
            }
            p += 1;
        }
        n
    }

    /// 是否有开放端。
    pub fn has_open(&self) -> bool {
        self.open_any
    }

    /// 诊断串（逗号分隔、端点规范化）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if i > 0 {
                s.push(',');
            }
            if let Some(r) = self.ranges.get(i) {
                s.push_str(&r.text());
            }
            i += 1;
        }
        s
    }

    /// 解析列表。
    pub fn parse(raw: &[u8]) -> Result<List, u32> {
        if raw.is_empty() {
            return Err(CT_ESHORT);
        }
        let mut out = List::empty();
        let mut i = 0usize;
        loop {
            let start = i;
            while i < raw.len() && at(raw, i) != Some(b',') {
                i += 1;
            }
            let seg = raw.get(start..i).unwrap_or(&[]);
            if seg.is_empty() {
                return Err(CT_ELIST);
            }
            if out.len >= LIST_CAP {
                return Err(CT_ECAP);
            }
            let r = parse_segment(seg)?;
            if r.open {
                out.open_any = true;
            }
            if let Some(dst) = out.ranges.get_mut(out.len) {
                *dst = r;
            }
            out.len += 1;
            if i >= raw.len() {
                break;
            }
            i += 1;
        }
        if out.len == 0 {
            return Err(CT_ESHORT);
        }
        Ok(out)
    }
}

/// 解析十进制位置（拒绝空串、非数字、零、超界）。
fn parse_pos(raw: &[u8]) -> Result<u16, u32> {
    if raw.is_empty() {
        return Err(CT_ESHORT);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < raw.len() {
        let b = match at(raw, i) {
            Some(v) => v,
            None => return Err(CT_ELIST),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(CT_ELIST);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
        if v > POS_MAX as u32 {
            return Err(CT_ERANGE);
        }
        i += 1;
    }
    if v == 0 {
        return Err(CT_ERANGE);
    }
    Ok(v as u16)
}

/// 解析一段：`N`、`N-M`、`N-`、`-M`、`-`。
fn parse_segment(seg: &[u8]) -> Result<Range, u32> {
    let mut dash: Option<usize> = None;
    let mut i = 0usize;
    while i < seg.len() {
        if at(seg, i) == Some(b'-') {
            if dash.is_some() {
                return Err(CT_ELIST);
            }
            dash = Some(i);
        }
        i += 1;
    }
    match dash {
        None => {
            let v = parse_pos(seg)?;
            Ok(Range { lo: v, hi: v, open: false })
        }
        Some(k) => {
            let lpart = seg.get(..k).unwrap_or(&[]);
            let rpart = seg.get(k + 1..).unwrap_or(&[]);
            let lo = if lpart.is_empty() { 1 } else { parse_pos(lpart)? };
            if rpart.is_empty() {
                return Ok(Range { lo, hi: 0, open: true });
            }
            let hi = parse_pos(rpart)?;
            if hi < lo {
                return Err(CT_ERANGE);
            }
            Ok(Range { lo, hi, open: false })
        }
    }
}

/// 模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 未选。
    None,
    /// 字节。
    Bytes,
    /// 字符。
    Chars,
    /// 字段。
    Fields,
}

impl Mode {
    /// 选项字母。
    pub fn letter(&self) -> u8 {
        match self {
            Mode::None => 0,
            Mode::Bytes => b'b',
            Mode::Chars => b'c',
            Mode::Fields => b'f',
        }
    }
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 模式。
    pub mode: Mode,
    /// 位置列表。
    pub list: List,
    /// 字段分隔符（缺省 TAB）。
    pub delim: u8,
    /// 是否显式给了 `-d`。
    pub delim_set: bool,
    /// 是否 `--complement`。
    pub complement: bool,
    /// 是否 `-s`。
    pub suppress: bool,
}

impl Opts {
    /// 缺省。
    pub const fn new() -> Opts {
        Opts {
            mode: Mode::None,
            list: List::empty(),
            delim: b'\t',
            delim_set: false,
            complement: false,
            suppress: false,
        }
    }

    /// 给模式与列表构造（补集/抑制关）。
    pub fn with(mode: Mode, list: List) -> Opts {
        Opts {
            mode,
            list,
            delim: b'\t',
            delim_set: false,
            complement: false,
            suppress: false,
        }
    }

    /// 诊断串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let letter = self.mode.letter();
        if letter != 0 {
            s.push('-');
            s.push(letter as char);
            s.push(' ');
            s.push_str(&self.list.text());
        }
        if self.delim_set {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("-d ");
            s.push(self.delim as char);
        }
        if self.complement {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("--complement");
        }
        if self.suppress {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str("-s");
        }
        s
    }
}

/// 取下一个空格/制表符分隔的 token。
fn next_token<'a>(raw: &'a [u8], i: &mut usize) -> Option<&'a [u8]> {
    while *i < raw.len() {
        match at(raw, *i) {
            Some(b' ') | Some(b'\t') => *i += 1,
            _ => break,
        }
    }
    if *i >= raw.len() {
        return None;
    }
    let start = *i;
    while *i < raw.len() {
        match at(raw, *i) {
            Some(b' ') | Some(b'\t') => break,
            _ => *i += 1,
        }
    }
    raw.get(start..*i)
}

/// 解析命令行。
pub fn parse_args(raw: &[u8]) -> Result<Opts, u32> {
    let mut o = Opts::new();
    let mut i = 0usize;
    while let Some(tok) = next_token(raw, &mut i) {
        if tok == b"--complement" {
            o.complement = true;
            continue;
        }
        if tok.len() < 2 || at(tok, 0) != Some(b'-') {
            return Err(CT_EFLAG);
        }
        let letter = match at(tok, 1) {
            Some(v) => v,
            None => return Err(CT_EFLAG),
        };
        match letter {
            b'b' | b'c' | b'f' => {
                if o.mode != Mode::None {
                    return Err(CT_EMODE);
                }
                let list_raw = if tok.len() > 2 {
                    tok.get(2..).unwrap_or(&[])
                } else {
                    match next_token(raw, &mut i) {
                        Some(v) => v,
                        None => return Err(CT_ESHORT),
                    }
                };
                let list = List::parse(list_raw)?;
                o.list = list;
                o.mode = if letter == b'b' {
                    Mode::Bytes
                } else if letter == b'c' {
                    Mode::Chars
                } else {
                    Mode::Fields
                };
            }
            b'd' => {
                let d = if tok.len() > 2 {
                    if tok.len() > 3 {
                        return Err(CT_EDELIM);
                    }
                    match at(tok, 2) {
                        Some(v) => v,
                        None => return Err(CT_EDELIM),
                    }
                } else {
                    let v = match next_token(raw, &mut i) {
                        Some(v) => v,
                        None => return Err(CT_ESHORT),
                    };
                    if v.len() != 1 {
                        return Err(CT_EDELIM);
                    }
                    match at(v, 0) {
                        Some(c) => c,
                        None => return Err(CT_EDELIM),
                    }
                };
                if d == 0 {
                    return Err(CT_EDELIM);
                }
                o.delim = d;
                o.delim_set = true;
            }
            b's' => {
                if tok.len() != 2 {
                    return Err(CT_EFLAG);
                }
                o.suppress = true;
            }
            _ => return Err(CT_EFLAG),
        }
    }
    if o.mode == Mode::None {
        return Err(CT_EMODE);
    }
    if o.delim_set && o.mode != Mode::Fields {
        return Err(CT_EDELIM);
    }
    if o.suppress && o.mode != Mode::Fields {
        return Err(CT_EFLAG);
    }
    if o.list.is_empty() {
        return Err(CT_ESHORT);
    }
    Ok(o)
}

/// 选中字节缓冲。
#[derive(Clone, Copy, Debug)]
pub struct Picked {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl Picked {
    /// 空。
    pub const fn empty() -> Picked {
        Picked { buf: [0u8; LINE_CAP], len: 0 }
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
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
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

    /// 压入一个字节（超容量被忽略，容量与行容量一致）。
    fn push(&mut self, b: u8) {
        if let Some(dst) = self.buf.get_mut(self.len) {
            *dst = b;
            self.len += 1;
        }
    }
}

/// 字符数：C locale 下一个字节就是一个字符。
pub fn char_count(line: &[u8]) -> usize {
    if MB_CUR_MAX == 1 {
        line.len()
    } else {
        0
    }
}

/// 第 i 个字符的字节（单字节字符集）。
pub fn char_at(line: &[u8], i: usize) -> Option<u8> {
    if MB_CUR_MAX == 1 {
        line.get(i).copied()
    } else {
        None
    }
}

/// 单字节宽度闸门。
pub fn mb_cur_max() -> usize {
    MB_CUR_MAX
}

/// 按字节位置选择。
pub fn pick_bytes(line: &[u8], list: &List, complement: bool) -> Picked {
    let mut out = Picked::empty();
    let mut i = 0usize;
    while i < line.len() {
        let pos = (i + 1) as u16;
        let inside = list.contains(pos);
        let want = if complement { !inside } else { inside };
        if want {
            if let Some(b) = line.get(i).copied() {
                out.push(b);
            }
        }
        i += 1;
    }
    out
}

/// 按字符位置选择（独立路径，单字节字符集下应与 `pick_bytes` 相同）。
pub fn pick_chars(line: &[u8], list: &List, complement: bool) -> Picked {
    let mut out = Picked::empty();
    let n = char_count(line);
    let mut i = 0usize;
    while i < n {
        let pos = (i + 1) as u16;
        let inside = list.contains(pos);
        let want = if complement { !inside } else { inside };
        if want {
            if let Some(b) = char_at(line, i) {
                out.push(b);
            }
        }
        i += 1;
    }
    out
}

/// 字段跨度（字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起始。
    pub start: usize,
    /// 结束（不包含）。
    pub end: usize,
}

impl Span {
    /// 空跨度。
    pub const fn empty() -> Span {
        Span { start: 0, end: 0 }
    }
}

/// 字段视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldView {
    spans: [Span; FIELDS_CAP],
    len: usize,
    has_delim: bool,
}

impl FieldView {
    /// 空。
    pub const fn empty() -> FieldView {
        FieldView { spans: [Span::empty(); FIELDS_CAP], len: 0, has_delim: false }
    }

    /// 字段数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否含分隔符。
    pub fn has_delim(&self) -> bool {
        self.has_delim
    }

    /// 第 i 个字段（零基）。
    pub fn get(&self, i: usize) -> Option<Span> {
        if i >= self.len {
            return None;
        }
        self.spans.get(i).copied()
    }

    /// 切片文本。
    pub fn text_at(&self, line: &[u8], i: usize) -> String {
        let mut s = String::new();
        if let Some(sp) = self.get(i) {
            if let Some(part) = line.get(sp.start..sp.end) {
                let mut k = 0usize;
                while k < part.len() {
                    if let Some(b) = part.get(k).copied() {
                        s.push(b as char);
                    }
                    k += 1;
                }
            }
        }
        s
    }
}

/// 按分隔符切分字段（保留尾随空字段）。
pub fn split_fields(line: &[u8], delim: u8) -> Result<FieldView, u32> {
    let mut v = FieldView::empty();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < line.len() {
        let b = match at(line, i) {
            Some(v) => v,
            None => break,
        };
        if b == delim {
            if v.len >= FIELDS_CAP {
                return Err(CT_ECAP);
            }
            if let Some(dst) = v.spans.get_mut(v.len) {
                *dst = Span { start, end: i };
            }
            v.len += 1;
            v.has_delim = true;
            start = i + 1;
        }
        i += 1;
    }
    if v.len >= FIELDS_CAP {
        return Err(CT_ECAP);
    }
    if let Some(dst) = v.spans.get_mut(v.len) {
        *dst = Span { start, end: line.len() };
    }
    v.len += 1;
    Ok(v)
}

/// 选择字段并重新拼装。
pub fn select_fields(
    line: &[u8],
    view: &FieldView,
    list: &List,
    complement: bool,
    delim: u8,
) -> String {
    let mut out = String::new();
    let mut first = true;
    let mut i = 0usize;
    while i < view.len() {
        let pos = (i + 1) as u16;
        let inside = list.contains(pos);
        let want = if complement { !inside } else { inside };
        if want {
            if let Some(sp) = view.get(i) {
                if !first {
                    out.push(delim as char);
                }
                first = false;
                if let Some(part) = line.get(sp.start..sp.end) {
                    let mut k = 0usize;
                    while k < part.len() {
                        if let Some(b) = part.get(k).copied() {
                            out.push(b as char);
                        }
                        k += 1;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// 一行。
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

    /// 从字节串构造。
    pub fn parse(raw: &[u8]) -> Result<Line, u32> {
        if raw.len() > LINE_CAP {
            return Err(CT_ERANGE);
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

    /// 长度。
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
    /// 空。
    pub const fn new() -> Corpus {
        Corpus { lines: [Line::empty(); LINES_CAP], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, l: Line) -> Result<(), u32> {
        if self.len >= LINES_CAP {
            return Err(CT_ERANGE);
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

/// 输出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutOut {
    /// 输出行数（含空行）。
    pub shown: usize,
    /// 渲染文本。
    pub out: String,
}

/// 执行 cut。
pub fn run(o: &Opts, c: &Corpus) -> Result<CutOut, u32> {
    if o.mode == Mode::None {
        return Err(CT_EMODE);
    }
    if o.mode != Mode::Fields && mb_cur_max() != 1 {
        return Err(CT_ESTATE);
    }
    let mut out = String::new();
    let mut shown = 0usize;
    let mut i = 0usize;
    while i < c.len() {
        let line = match c.get(i) {
            Some(v) => v,
            None => break,
        };
        let mut emit = true;
        let mut body = String::new();
        match o.mode {
            Mode::Bytes => {
                let p = pick_bytes(line.as_bytes(), &o.list, o.complement);
                body.push_str(&p.text());
            }
            Mode::Chars => {
                let p = pick_chars(line.as_bytes(), &o.list, o.complement);
                body.push_str(&p.text());
            }
            Mode::Fields => {
                let view = split_fields(line.as_bytes(), o.delim)?;
                if !view.has_delim() {
                    if o.suppress {
                        emit = false;
                    } else {
                        body.push_str(&line.text());
                    }
                } else {
                    body.push_str(&select_fields(
                        line.as_bytes(),
                        &view,
                        &o.list,
                        o.complement,
                        o.delim,
                    ));
                }
            }
            Mode::None => return Err(CT_EMODE),
        }
        if emit {
            if shown > 0 {
                out.push('\n');
            }
            out.push_str(&body);
            shown += 1;
        }
        i += 1;
    }
    Ok(CutOut { shown, out })
}

/// 基准语料。
pub fn base_corpus() -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(b"abcdef")?)?;
    c.push(Line::parse(b"12345")?)?;
    c.push(Line::parse(b"a,b:c")?)?;
    c.push(Line::parse(b"")?)?;
    c.push(Line::parse(b"hello world")?)?;
    Ok(c)
}

/// 单行语料。
pub fn one_line(raw: &[u8]) -> Result<Corpus, u32> {
    let mut c = Corpus::new();
    c.push(Line::parse(raw)?)?;
    Ok(c)
}

/// 结果首行长度。
fn head_len(o: &CutOut) -> u32 {
    let b = o.out.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if at(b, i) == Some(b'\n') {
            break;
        }
        i += 1;
    }
    i as u32
}

/// 字节选择探针。
fn probe_bytes_len() -> Result<u32, u32> {
    let c = base_corpus()?;
    let o = parse_args(b"-b2-4")?;
    run(&o, &c).map(|v| head_len(&v))
}

/// 字符选择探针。
fn probe_chars_len() -> Result<u32, u32> {
    let c = base_corpus()?;
    let o = parse_args(b"-c2-4")?;
    run(&o, &c).map(|v| head_len(&v))
}

/// 补集探针。
fn probe_comp_len() -> Result<u32, u32> {
    let c = base_corpus()?;
    let o = parse_args(b"-b2-4 --complement")?;
    run(&o, &c).map(|v| head_len(&v))
}

/// 第二字段探针。
fn probe_field_len() -> Result<u32, u32> {
    let c = one_line(b"a:b:c")?;
    let o = parse_args(b"-f2 -d:")?;
    run(&o, &c).map(|v| head_len(&v))
}

/// 开放字段探针。
fn probe_open_len() -> Result<u32, u32> {
    let c = one_line(b"x:y:z")?;
    let o = parse_args(b"-f2- -d:")?;
    run(&o, &c).map(|v| head_len(&v))
}

/// 列表计数探针。
fn probe_list_count() -> Result<u32, u32> {
    let l = List::parse(b"1,3-5,7-")?;
    Ok(l.count_upto(8))
}

/// 去重计数探针。
fn probe_dup_count() -> Result<u32, u32> {
    let l = List::parse(b"1,1,2-3")?;
    Ok(l.count_upto(4))
}

/// 字节/字符对拍探针。
fn probe_sweep() -> Result<u32, u32> {
    let c = base_corpus()?;
    let raws: [&[u8]; 4] = [b"1", b"2-4", b"1,3-5", b"5-"];
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < raws.len() {
        let raw = match raws.get(i) {
            Some(v) => *v,
            None => break,
        };
        let list = List::parse(raw)?;
        let ob = Opts::with(Mode::Bytes, list);
        let oc = Opts::with(Mode::Chars, list);
        let rb = run(&ob, &c)?;
        let rc = run(&oc, &c)?;
        if rb.out != rc.out || rb.shown != rc.shown {
            return Err(CT_ESTATE);
        }
        let mut k = 0usize;
        while k < c.len() {
            if let Some(l) = c.get(k) {
                checked = checked.saturating_add(l.len() as u32);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(checked)
}

/// 坏列表探针。
fn probe_bad_list() -> Result<u32, u32> {
    List::parse(b"1,x").map(|l| l.len() as u32)
}

/// 零位置探针。
fn probe_zero() -> Result<u32, u32> {
    List::parse(b"0").map(|l| l.len() as u32)
}

/// 缺模式探针。
fn probe_nomode() -> Result<u32, u32> {
    parse_args(b"").map(|o| o.list.len() as u32)
}

/// 坏标志探针。
fn probe_badflag() -> Result<u32, u32> {
    parse_args(b"-Z").map(|o| o.list.len() as u32)
}

/// 坏分隔符探针。
fn probe_baddelim() -> Result<u32, u32> {
    parse_args(b"-f1 -d ab").map(|o| o.list.len() as u32)
}

/// 缺操作数探针。
fn probe_short() -> Result<u32, u32> {
    parse_args(b"-b").map(|o| o.list.len() as u32)
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
            return Err(CT_ESLOT);
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
            return Err(CT_EBAL);
        }
        const VALS: [u32; 8] = [3, 3, 3, 1, 3, 6, 3, 108];
        const CODES: [u32; 6] = [CT_ELIST, CT_ERANGE, CT_EMODE, CT_EFLAG, CT_EDELIM, CT_ESHORT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CT_EBAL),
            };
            if s.slot as usize != i {
                return Err(CT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CT_EBAL);
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
            return Err(CT_EBAL);
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
            Err(CT_ESEAL)
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
    let _ = l.register(word_cell(0, OP_BYTES, probe_bytes_len()));
    let _ = l.register(word_cell(1, OP_CHARS, probe_chars_len()));
    let _ = l.register(word_cell(2, OP_COMP, probe_comp_len()));
    let _ = l.register(word_cell(3, OP_FIELD, probe_field_len()));
    let _ = l.register(word_cell(4, OP_OPEN, probe_open_len()));
    let _ = l.register(word_cell(5, OP_LIST, probe_list_count()));
    let _ = l.register(word_cell(6, OP_RANGE, probe_dup_count()));
    let _ = l.register(word_cell(7, OP_SWEEP, probe_sweep()));
    let _ = l.register(word_cell(8, OP_BADLIST, probe_bad_list()));
    let _ = l.register(word_cell(9, OP_ZERO, probe_zero()));
    let _ = l.register(word_cell(10, OP_NOMODE, probe_nomode()));
    let _ = l.register(word_cell(11, OP_BADFLAG, probe_badflag()));
    let _ = l.register(word_cell(12, OP_BADDELIM, probe_baddelim()));
    let _ = l.register(word_cell(13, OP_SHORT, probe_short()));
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => Corpus::new(),
        };
        let raws: [&[u8]; 4] = [b"1", b"2-4", b"1,3-5", b"5-"];
        let mut ok = true;
        let mut i = 0usize;
        while i < raws.len() {
            let raw = match raws.get(i) {
                Some(v) => *v,
                None => break,
            };
            let list = match List::parse(raw) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    List::empty()
                }
            };
            let rb = run(&Opts::with(Mode::Bytes, list), &c);
            let rc = run(&Opts::with(Mode::Chars, list), &c);
            match (rb, rc) {
                (Ok(a), Ok(b)) => {
                    if a != b {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            i += 1;
        }
        let high: [u8; 3] = [0xe4, 0xb8, 0xad];
        let one = match List::parse(b"1") {
            Ok(v) => v,
            Err(_) => List::empty(),
        };
        let pb = pick_bytes(&high, &one, false);
        let pc = pick_chars(&high, &one, false);
        if pb.as_bytes() != [0xe4] || pc.as_bytes() != [0xe4] {
            ok = false;
        }
        ok
    };

    out[1] = {
        let mut ok = true;
        let mut p = 1u16;
        while p <= POS_MAX {
            let mut txt = String::new();
            push_dec(&mut txt, p as u64);
            let list = match List::parse(txt.as_bytes()) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    List::empty()
                }
            };
            if !list.contains(p) || list.count_upto(POS_MAX) != 1 {
                ok = false;
            }
            let mut set = PosSet::new();
            if set.set(p).is_err() || !set.has(p) || set.count() != 1 {
                ok = false;
            }
            if set.set(0) != Err(CT_ERANGE) {
                ok = false;
            }
            p += 1;
        }
        // 全部单段区间的计数恒等式。
        let mut lo = 1u16;
        while lo <= 16 {
            let mut hi = lo;
            while hi <= 16 {
                let mut txt = String::new();
                push_dec(&mut txt, lo as u64);
                txt.push('-');
                push_dec(&mut txt, hi as u64);
                let list = match List::parse(txt.as_bytes()) {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        List::empty()
                    }
                };
                let want = (hi - lo + 1) as u32;
                if list.count_upto(16) != want {
                    ok = false;
                }
                hi += 1;
            }
            lo += 1;
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
        let line = b"a:b:c:d";
        let view = match split_fields(line, b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        if view.len() != 4 || !view.has_delim() {
            ok = false;
        }
        let list = match List::parse(b"2,4") {
            Ok(v) => v,
            Err(_) => List::empty(),
        };
        let picked = select_fields(line, &view, &list, false, b':');
        if picked != String::from("b:d") {
            ok = false;
        }
        let comp = select_fields(line, &view, &list, true, b':');
        if comp != String::from("a:c") {
            ok = false;
        }
        let trail = match split_fields(b"a:b:", b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        if trail.len() != 3 || trail.text_at(b"a:b:", 2) != String::from("") {
            ok = false;
        }
        let none = match split_fields(b"abc", b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        if none.has_delim() || none.len() != 1 {
            ok = false;
        }
        ok
    };

    out
}

/// 判据集合。
pub fn run_unx11566_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11566.cutbc");

    s.add(
        "CT01-位置集语义",
        {
            let mut set = PosSet::new();
            let a = set.set(1).is_ok() && set.set(64).is_ok() && set.set(128).is_ok();
            let b = set.count() == 3 && set.has(1) && set.has(64) && set.has(128);
            let c = !set.has(2) && !set.has(0) && !set.has(129);
            let d = set.set(0) == Err(CT_ERANGE) && set.set(129) == Err(CT_ERANGE);
            let mut other = PosSet::new();
            let e = other.set(64).is_ok() && other.set(65).is_ok();
            set.union(&other);
            let f = set.count() == 4 && set.has(65);
            set.clear(64);
            let g = !set.has(64) && set.count() == 3;
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "CT02-列表语法",
        {
            let l = match List::parse(b"1,3-5,7-") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let a = l.len() == 3
                && l.has_open()
                && l.contains(1)
                && !l.contains(2)
                && l.contains(3)
                && l.contains(5)
                && !l.contains(6)
                && l.contains(7)
                && l.contains(100);
            let b = l.count_upto(8) == 6;
            let c = match List::parse(b"-3") {
                Ok(v) => v.contains(1) && v.contains(3) && !v.contains(4) && v.count_upto(4) == 3,
                Err(_) => false,
            };
            let d = match List::parse(b"2-") {
                Ok(v) => v.has_open() && !v.contains(1) && v.contains(2) && v.contains(128),
                Err(_) => false,
            };
            let e = List::parse(b"").err() == Some(CT_ESHORT);
            let f = List::parse(b",").err() == Some(CT_ELIST);
            let g = List::parse(b"1,").err() == Some(CT_ELIST);
            let h = List::parse(b",1").err() == Some(CT_ELIST);
            let i = List::parse(b"1,,2").err() == Some(CT_ELIST);
            let j = List::parse(b"0").err() == Some(CT_ERANGE);
            let k = List::parse(b"129").err() == Some(CT_ERANGE);
            let m = List::parse(b"5-2").err() == Some(CT_ERANGE);
            a && b && c && d && e && f && g && h && i && j && k && m
        },
        "static",
    );

    s.add(
        "CT03-字节选择",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let o = match parse_args(b"-b2-4") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let a = match run(&o, &c) {
                Ok(v) => {
                    v.shown == 5
                        && v.out == String::from("bcd\n234\n,b:\n\nell")
                }
                Err(_) => false,
            };
            let l1 = match List::parse(b"1,3,5") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let pb = pick_bytes(b"abcdef", &l1, false);
            let b = pb.as_bytes() == b"ace";
            let high: [u8; 3] = [0xe4, 0xb8, 0xad];
            let l2 = match List::parse(b"2") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let p2 = pick_bytes(&high, &l2, false);
            let d = p2.len() == 1 && p2.as_bytes() == [0xb8];
            let empty = pick_bytes(b"", &l1, false);
            a && b && d && empty.is_empty()
        },
        "static",
    );

    s.add(
        "CT04-字符选择",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let o = match parse_args(b"-c2-4") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let a = match run(&o, &c) {
                Ok(v) => v.out == String::from("bcd\n234\n,b:\n\nell"),
                Err(_) => false,
            };
            let b = mb_cur_max() == 1;
            let high: [u8; 3] = [0xe4, 0xb8, 0xad];
            let l = match List::parse(b"1") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let pc = pick_chars(&high, &l, false);
            let d = char_count(&high) == 3
                && pc.len() == 1
                && pc.as_bytes() == [0xe4]
                && char_at(&high, 2) == Some(0xad);
            let e = char_at(&high, 3).is_none();
            a && b && d && e
        },
        "static",
    );

    s.add(
        "CT05-等价穷举",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let raws: [&[u8]; 5] = [b"1", b"2-4", b"1,3-5", b"5-", b"1,2,3,4,5,6"];
            let mut ok = true;
            let mut i = 0usize;
            while i < raws.len() {
                let raw = match raws.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                let list = match List::parse(raw) {
                    Ok(v) => v,
                    Err(_) => {
                        ok = false;
                        List::empty()
                    }
                };
                let rb = run(&Opts::with(Mode::Bytes, list), &c);
                let rc = run(&Opts::with(Mode::Chars, list), &c);
                let rb2 = run(&Opts::with(Mode::Bytes, list), &c);
                match (rb, rc, rb2) {
                    (Ok(a), Ok(b), Ok(d)) => {
                        if a != b || a != d || a.shown != 5 {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "CT06-字段选择",
        {
            let line = b"a:b:c";
            let view = match split_fields(line, b':') {
                Ok(v) => v,
                Err(_) => FieldView::empty(),
            };
            let l2 = match List::parse(b"2") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let l23 = match List::parse(b"2,3") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let a = view.len() == 3 && view.has_delim();
            let b = select_fields(line, &view, &l2, false, b':') == String::from("b");
            let c = select_fields(line, &view, &l23, false, b':') == String::from("b:c");
            let l9 = match List::parse(b"9") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let d = select_fields(line, &view, &l9, false, b':') == String::from("");
            let trail = match split_fields(b"a:b:", b':') {
                Ok(v) => v,
                Err(_) => FieldView::empty(),
            };
            let e = trail.len() == 3 && trail.text_at(b"a:b:", 2) == String::from("");
            let none = match split_fields(b"abc", b':') {
                Ok(v) => v,
                Err(_) => FieldView::empty(),
            };
            let f = none.len() == 1 && !none.has_delim();
            let g = match parse_args(b"-f1 -d ab") {
                Ok(_) => false,
                Err(c) => c == CT_EDELIM,
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "CT07-补集与区间",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let comp = match parse_args(b"-b2-4 --complement") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let a = match run(&comp, &c) {
                Ok(v) => v.out == String::from("aef\n15\nac\n\nho world"),
                Err(_) => false,
            };
            let line = b"a:b:c";
            let view = match split_fields(line, b':') {
                Ok(v) => v,
                Err(_) => FieldView::empty(),
            };
            let l2 = match List::parse(b"2") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let b = select_fields(line, &view, &l2, true, b':') == String::from("a:c");
            let l_open = match List::parse(b"3-") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let d = select_fields(line, &view, &l_open, false, b':') == String::from("c");
            let dup = match List::parse(b"1,1,2,2,1") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let e = dup.count_upto(2) == 2 && dup.contains(1) && dup.contains(2);
            a && b && d && e
        },
        "static",
    );

    s.add(
        "CT08-选项解析",
        {
            let a = match parse_args(b"-b1-3") {
                Ok(v) => v.mode == Mode::Bytes && v.list.count_upto(4) == 3,
                Err(_) => false,
            };
            let b = match parse_args(b"-f2 -d:") {
                Ok(v) => v.mode == Mode::Fields && v.delim == b':' && v.delim_set,
                Err(_) => false,
            };
            let c = match parse_args(b"-b1 -c1") {
                Ok(_) => false,
                Err(c) => c == CT_EMODE,
            };
            let d = parse_args(b"").err() == Some(CT_EMODE);
            let e = parse_args(b"-Z").err() == Some(CT_EFLAG);
            let f = parse_args(b"-b").err() == Some(CT_ESHORT);
            let g = parse_args(b"-b1 -d:").err() == Some(CT_EDELIM);
            let h = parse_args(b"-b1 -s").err() == Some(CT_EFLAG);
            let i = match parse_args(b"-b1 --complement") {
                Ok(v) => v.complement,
                Err(_) => false,
            };
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "CT09-边界容量",
        {
            let a = Line::parse(&[b'x'; LINE_CAP]).is_ok();
            let b = Line::parse(&[b'x'; LINE_CAP + 1]).err() == Some(CT_ERANGE);
            let mut c = Corpus::new();
            let mut i = 0usize;
            let mut full = true;
            while i < LINES_CAP {
                let l = match Line::parse(b"x") {
                    Ok(v) => v,
                    Err(_) => {
                        full = false;
                        Line::empty()
                    }
                };
                if c.push(l).is_err() {
                    full = false;
                }
                i += 1;
            }
            let extra = Line::empty();
            let d = full && c.push(extra).err() == Some(CT_ERANGE);
            let line = [b':'; 40];
            let e = split_fields(&line, b':').err() == Some(CT_ECAP);
            let many = b"1,2,3,4,5,6,7,8,9,10,11,12,13";
            let f = List::parse(many).err() == Some(CT_ECAP);
            let empty = Corpus::new();
            let o = match parse_args(b"-b1") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let g = match run(&o, &empty) {
                Ok(v) => v.shown == 0 && v.out.is_empty(),
                Err(_) => false,
            };
            let l = match List::parse(b"1-") {
                Ok(v) => v,
                Err(_) => List::empty(),
            };
            let mut co = Opts::with(Mode::Bytes, l);
            co.complement = true;
            let h = match run(&co, &c) {
                Ok(v) => v.shown == LINES_CAP && v.out == String::from("\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"),
                Err(_) => false,
            };
            a && b && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "CT10-渲染与诊断",
        {
            let a = match List::parse(b"1,3-5,7-") {
                Ok(v) => v.text() == String::from("1,3-5,7-"),
                Err(_) => false,
            };
            let b = match List::parse(b"-3") {
                Ok(v) => v.text() == String::from("1-3"),
                Err(_) => false,
            };
            let c = match List::parse(b"2-") {
                Ok(v) => v.text() == String::from("2-"),
                Err(_) => false,
            };
            let d = match List::parse(b"4") {
                Ok(v) => v.text() == String::from("4"),
                Err(_) => false,
            };
            let e = match parse_args(b"-b1-3 --complement") {
                Ok(v) => v.text() == String::from("-b 1-3 --complement"),
                Err(_) => false,
            };
            let f = match parse_args(b"-f2 -d:") {
                Ok(v) => v.text() == String::from("-f 2 -d :"),
                Err(_) => false,
            };
            let g = match parse_args(b"-f1 -d: -s") {
                Ok(v) => v.text() == String::from("-f 1 -d : -s"),
                Err(_) => false,
            };
            let h = match List::parse(b"1-1") {
                Ok(v) => v.text() == String::from("1"),
                Err(_) => false,
            };
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "CT11-基准对照",
        {
            let c = match base_corpus() {
                Ok(v) => v,
                Err(_) => Corpus::new(),
            };
            let ob = match parse_args(b"-b2-4") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let oc = match parse_args(b"-c2-4") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let of = match parse_args(b"-f2 -d:") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let ofc = match parse_args(b"-f2 -d: --complement") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let os = match parse_args(b"-f1 -d: -s") {
                Ok(v) => v,
                Err(_) => Opts::new(),
            };
            let want_b = String::from("bcd\n234\n,b:\n\nell");
            let want_f = String::from("abcdef\n12345\nc\n\nhello world");
            let want_fc = String::from("abcdef\n12345\na,b\n\nhello world");
            let a = match run(&ob, &c) {
                Ok(v) => v.out == want_b && v.shown == 5,
                Err(_) => false,
            };
            let b = match run(&oc, &c) {
                Ok(v) => v.out == want_b && v.shown == 5,
                Err(_) => false,
            };
            let d = match run(&of, &c) {
                Ok(v) => v.out == want_f,
                Err(_) => false,
            };
            let e = match run(&ofc, &c) {
                Ok(v) => v.out == want_fc,
                Err(_) => false,
            };
            let f = match run(&os, &c) {
                Ok(v) => v.out == String::from("a,b") && v.shown == 1,
                Err(_) => false,
            };
            let mut total = 0u32;
            let mut i = 0usize;
            while i < c.len() {
                if let Some(l) = c.get(i) {
                    total = total.saturating_add(l.len() as u32);
                }
                i += 1;
            }
            a && b && d && e && f && total == 27
        },
        "static",
    );

    s.add(
        "CT12-账本对账",
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
        "CT13-冻结重放四注入",
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
            let mut l2 = build_ledger();
            let _ = l2.freeze();
            let before = l2.rejects();
            let second = l2.freeze();
            if second.err() != Some(CT_EBAL) || l2.rejects() != before.saturating_add(1) {
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

    fn list(raw: &[u8]) -> List {
        match List::parse(raw) {
            Ok(v) => v,
            Err(_) => List::empty(),
        }
    }

    fn opts(raw: &[u8]) -> Opts {
        match parse_args(raw) {
            Ok(v) => v,
            Err(_) => Opts::new(),
        }
    }

    #[test]
    fn fold_is_deterministic() {
        assert_eq!(fold(FNV_OFF, 7), fold(FNV_OFF, 7));
        assert_ne!(fold(FNV_OFF, 7), fold(FNV_OFF, 8));
    }

    #[test]
    fn pos_set_bits() {
        let mut s = PosSet::new();
        assert_eq!(s.set(0), Err(CT_ERANGE));
        assert_eq!(s.set(129), Err(CT_ERANGE));
        assert_eq!(s.set(3), Ok(()));
        assert!(s.has(3));
        assert!(!s.has(4));
        assert_eq!(s.count(), 1);
    }

    #[test]
    fn list_basic() {
        let l = list(b"1,3-5,7-");
        assert_eq!(l.len(), 3);
        assert!(l.has_open());
        assert!(l.contains(5));
        assert!(!l.contains(6));
        assert_eq!(l.count_upto(8), 6);
    }

    #[test]
    fn list_forms() {
        assert_eq!(list(b"-3").count_upto(4), 3);
        assert!(list(b"2-").contains(128));
        assert_eq!(list(b"1-1").count_upto(2), 1);
        assert_eq!(list(b"4").text(), String::from("4"));
    }

    #[test]
    fn list_errors() {
        assert_eq!(List::parse(b"").err(), Some(CT_ESHORT));
        assert_eq!(List::parse(b"1,").err(), Some(CT_ELIST));
        assert_eq!(List::parse(b"0").err(), Some(CT_ERANGE));
        assert_eq!(List::parse(b"129").err(), Some(CT_ERANGE));
        assert_eq!(List::parse(b"9-2").err(), Some(CT_ERANGE));
    }

    #[test]
    fn parse_bytes() {
        let o = opts(b"-b2-4");
        assert_eq!(o.mode, Mode::Bytes);
        assert_eq!(o.list.count_upto(5), 3);
    }

    #[test]
    fn parse_fields_delim() {
        let o = opts(b"-f2 -d:");
        assert_eq!(o.mode, Mode::Fields);
        assert_eq!(o.delim, b':');
        assert!(o.delim_set);
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse_args(b"").err(), Some(CT_EMODE));
        assert_eq!(parse_args(b"-Z").err(), Some(CT_EFLAG));
        assert_eq!(parse_args(b"-b").err(), Some(CT_ESHORT));
        assert_eq!(parse_args(b"-b1 -c1").err(), Some(CT_EMODE));
        assert_eq!(parse_args(b"-f1 -d ab").err(), Some(CT_EDELIM));
        assert_eq!(parse_args(b"-b1 -s").err(), Some(CT_EFLAG));
    }

    #[test]
    fn pick_byte_equivalence() {
        let l = list(b"2-4");
        let a = pick_bytes(b"abcdef", &l, false);
        let b = pick_chars(b"abcdef", &l, false);
        assert_eq!(a.as_bytes(), b"bcd");
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn pick_high_byte_single() {
        let high: [u8; 3] = [0xe4, 0xb8, 0xad];
        let l = list(b"1");
        assert_eq!(pick_bytes(&high, &l, false).as_bytes(), [0xe4]);
        assert_eq!(pick_chars(&high, &l, false).as_bytes(), [0xe4]);
        assert_eq!(char_count(&high), 3);
    }

    #[test]
    fn complement_pick() {
        let l = list(b"2-4");
        let p = pick_bytes(b"abcdef", &l, true);
        assert_eq!(p.as_bytes(), b"aef");
    }

    #[test]
    fn fields_split() {
        let v = match split_fields(b"a:b:c", b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        assert_eq!(v.len(), 3);
        assert!(v.has_delim());
        assert_eq!(v.text_at(b"a:b:c", 1), String::from("b"));
    }

    #[test]
    fn fields_trailing_empty() {
        let v = match split_fields(b"a:b:", b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        assert_eq!(v.len(), 3);
        assert_eq!(v.text_at(b"a:b:", 2), String::from(""));
    }

    #[test]
    fn fields_no_delim() {
        let v = match split_fields(b"abc", b':') {
            Ok(v) => v,
            Err(_) => FieldView::empty(),
        };
        assert_eq!(v.len(), 1);
        assert!(!v.has_delim());
    }

    #[test]
    fn run_bytes_output() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let o = opts(b"-b2-4");
        let out = match run(&o, &c) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(out.shown, 5);
        assert_eq!(out.out, String::from("bcd\n234\n,b:\n\nell"));
    }

    #[test]
    fn run_chars_output() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let o = opts(b"-c2-4");
        let out = match run(&o, &c) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(out.out, String::from("bcd\n234\n,b:\n\nell"));
    }

    #[test]
    fn run_field_output() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let o = opts(b"-f2 -d:");
        let out = match run(&o, &c) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(out.out, String::from("abcdef\n12345\nc\n\nhello world"));
    }

    #[test]
    fn run_suppress() {
        let c = match base_corpus() {
            Ok(v) => v,
            Err(_) => return,
        };
        let o = opts(b"-f1 -d: -s");
        let out = match run(&o, &c) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(out.shown, 1);
        assert_eq!(out.out, String::from("a,b"));
    }

    #[test]
    fn run_empty_corpus() {
        let c = Corpus::new();
        let o = opts(b"-b1");
        let out = match run(&o, &c) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(out.shown, 0);
        assert!(out.out.is_empty());
    }

    #[test]
    fn line_and_corpus_caps() {
        assert!(Line::parse(&[b'x'; LINE_CAP]).is_ok());
        assert_eq!(Line::parse(&[b'x'; LINE_CAP + 1]).err(), Some(CT_ERANGE));
        let mut c = Corpus::new();
        let mut i = 0usize;
        while i < LINES_CAP {
            if c.push(Line::empty()).is_err() {
                return;
            }
            i += 1;
        }
        assert_eq!(c.push(Line::empty()).err(), Some(CT_ERANGE));
    }

    #[test]
    fn field_cap() {
        let line = [b':'; 40];
        assert_eq!(split_fields(&line, b':').err(), Some(CT_ECAP));
    }

    #[test]
    fn list_cap() {
        assert_eq!(
            List::parse(b"1,2,3,4,5,6,7,8,9,10,11,12,13").err(),
            Some(CT_ECAP)
        );
    }

    #[test]
    fn opts_rendering() {
        assert_eq!(opts(b"-b1-3 --complement").text(), String::from("-b 1-3 --complement"));
        assert_eq!(opts(b"-f2 -d:").text(), String::from("-f 2 -d :"));
        assert_eq!(opts(b"-f1 -d: -s").text(), String::from("-f 1 -d : -s"));
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
    fn ledger_slot_reject() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 2, op: OP_LIST, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad).err(), Some(CT_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
    }

    #[test]
    fn inject_all_true() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11566_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }
}
