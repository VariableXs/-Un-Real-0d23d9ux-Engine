//! UNX-F11572 uniq/comm/join 排序前提一致性（C 序合同）
//!
//! 三件套共用同一份「C 序合同」：`uniq` 只折叠相邻重复，`comm` 只对两列
//! 已排序列做三路归并，`join` 只按首字段做内连接——三者的前提都是
//! `LC_COLLATE=C` 的逐字节升序。本模块把这份合同写成可判定对象：
//! `c_compare` 给出 -1/0/1，`Lines::require_sorted` 是唯一入口闸门，
//! 未排序输入一律 `UJ_EORDER`，不让任何工具在错序数据上悄悄出结果。
//!
//! 闸门次序：字节 → 行长 → 容量 → 排序 → 选项 → 字段 → 渲染 → 账本。
//!
//! 无 std、无 unsafe、无 panic：失败落 13 个错误码，账本 14 格（8 成
//! 6 败）逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf4e0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 行表容量。
pub const ROWS_CAP: usize = 8;

/// 单行容量。
pub const LINE_CAP: usize = 64;

/// 账号列数。
pub const COLUMNS: usize = 3;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 输入未按 C 序排序。
pub const UJ_EORDER: u32 = SEG_BASE + 0x01;
/// 选项非法。
pub const UJ_EOPT: u32 = SEG_BASE + 0x02;
/// 行数超容量。
pub const UJ_ELEN: u32 = SEG_BASE + 0x03;
/// 字节非法。
pub const UJ_EBYTE: u32 = SEG_BASE + 0x04;
/// 字段缺失。
pub const UJ_EFIELD: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const UJ_ESTATE: u32 = SEG_BASE + 0x06;
/// 空输入。
pub const UJ_ENOENT: u32 = SEG_BASE + 0x07;
/// 重复定义。
pub const UJ_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const UJ_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const UJ_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const UJ_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const UJ_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const UJ_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 字节比较。
pub const OP_CMP: u8 = 0x01;
/// 排序判定。
pub const OP_ORDER: u8 = 0x02;
/// 折叠。
pub const OP_UNIQ: u8 = 0x03;
/// 三路归并。
pub const OP_COMM: u8 = 0x04;
/// 首字段连接。
pub const OP_JOIN: u8 = 0x05;
/// 选项。
pub const OP_OPT: u8 = 0x06;

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

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 行内可打印：空格到 `~`。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b < 0x20 || *b > 0x7e {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 逐字节相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// C 序比较：逐字节升序，短者在前。
pub fn c_compare(a: &[u8], b: &[u8]) -> i8 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x < y {
                    return -1;
                }
                if x > y {
                    return 1;
                }
            }
            _ => break,
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

/// 推入无符号十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at -= 1;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 行
// ---------------------------------------------------------------------------

/// 一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    bytes: [u8; LINE_CAP],
    len: usize,
}

impl Line {
    /// 空白。
    pub const fn blank() -> Line {
        Line { bytes: [0u8; LINE_CAP], len: 0 }
    }

    /// 构造。
    pub fn new(s: &[u8]) -> Result<Line, u32> {
        if s.is_empty() {
            return Err(UJ_EBYTE);
        }
        if s.len() > LINE_CAP {
            return Err(UJ_ELEN);
        }
        if !printable(s) {
            return Err(UJ_EBYTE);
        }
        let mut n = Line::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), n.bytes.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(UJ_ELEN),
            }
            i += 1;
        }
        n.len = s.len();
        Ok(n)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 首字段：到第一个空格前。
    pub fn first_field(&self) -> &[u8] {
        let mut end = 0usize;
        while end < self.len {
            match self.bytes.get(end) {
                Some(b) => {
                    if *b == b' ' {
                        break;
                    }
                }
                None => break,
            }
            end += 1;
        }
        self.bytes.get(0..end).unwrap_or(&[])
    }

    /// 首字段之后的部分（去掉一个空格）。
    pub fn rest(&self) -> &[u8] {
        let f = self.first_field().len();
        if f >= self.len {
            return &[];
        }
        self.bytes.get(f + 1..self.len).unwrap_or(&[])
    }

    /// 是否同名。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.bytes(), other)
    }
}

// ---------------------------------------------------------------------------
// 行表
// ---------------------------------------------------------------------------

/// 有序行表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lines {
    rows: [Line; ROWS_CAP],
    len: usize,
}

impl Lines {
    /// 空表。
    pub const fn new() -> Lines {
        Lines { rows: [Line::blank(); ROWS_CAP], len: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取行。
    pub fn at(&self, i: usize) -> Option<Line> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 追加。
    pub fn push(&mut self, l: Line) -> Result<(), u32> {
        if self.len >= ROWS_CAP {
            return Err(UJ_ELEN);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = l;
        }
        self.len += 1;
        Ok(())
    }

    /// 从字节数组建表。
    pub fn build(items: &[&[u8]]) -> Result<Lines, u32> {
        if items.is_empty() {
            return Err(UJ_ENOENT);
        }
        if items.len() > ROWS_CAP {
            return Err(UJ_ELEN);
        }
        let mut t = Lines::new();
        let mut i = 0usize;
        while i < items.len() {
            let raw = match items.get(i) {
                Some(v) => *v,
                None => return Err(UJ_ELEN),
            };
            t.push(Line::new(raw)?)?;
            i += 1;
        }
        Ok(t)
    }

    /// 是否严格按 C 序（相邻不降，允许相等）。
    pub fn is_sorted(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            match (self.rows.get(i - 1), self.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if c_compare(a.bytes(), b.bytes()) > 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 排序闸门：唯一入口。
    pub fn require_sorted(&self) -> Result<(), u32> {
        if self.is_empty() {
            return Err(UJ_ENOENT);
        }
        if self.is_sorted() {
            Ok(())
        } else {
            Err(UJ_EORDER)
        }
    }

    /// 升序副本（插入排序，容量小）。
    pub fn sorted_copy(&self) -> Lines {
        let mut out = Lines::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.rows.get(i) {
                let mut at = out.len;
                while at > 0 {
                    let prev = match out.rows.get(at - 1) {
                        Some(v) => *v,
                        None => break,
                    };
                    if c_compare(prev.bytes(), l.bytes()) > 0 {
                        if let Some(dst) = out.rows.get_mut(at) {
                            *dst = prev;
                        }
                        at -= 1;
                    } else {
                        break;
                    }
                }
                if let Some(dst) = out.rows.get_mut(at) {
                    *dst = *l;
                }
                out.len += 1;
            }
            i += 1;
        }
        out
    }

    /// 渲染整表，逐行加 `\n`。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.rows.get(i) {
                s.push_str(l.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }

    /// 行字节的 FNV。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.rows.get(i) {
                let mut j = 0usize;
                while j < l.len() {
                    if let Some(b) = l.bytes().get(j) {
                        h = fold_byte(h, *b);
                    }
                    j += 1;
                }
                h = fold_byte(h, b'\n');
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 折叠
// ---------------------------------------------------------------------------

/// 折叠模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniqMode {
    /// 只出唯一行（缺省）。
    Plain,
    /// 出计数 + 行。
    Count,
    /// 只出重复行。
    DupOnly,
    /// 只出唯一行（全局唯一，`-u`）。
    UniqueOnly,
}

impl UniqMode {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            UniqMode::Plain => "plain",
            UniqMode::Count => "count",
            UniqMode::DupOnly => "dup",
            UniqMode::UniqueOnly => "unique",
        }
    }

    /// 是否输出。
    pub fn wants(&self, count: u32) -> bool {
        match self {
            UniqMode::Plain | UniqMode::Count => true,
            UniqMode::DupOnly => count >= 2,
            UniqMode::UniqueOnly => count == 1,
        }
    }

    /// 是否显示计数。
    pub fn shows_count(&self) -> bool {
        match self {
            UniqMode::Count => true,
            _ => false,
        }
    }
}

/// 一条折叠结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Folded {
    /// 行。
    pub line: Line,
    /// 相邻重复次数。
    pub count: u32,
}

/// 折叠结果表。
#[derive(Clone, Copy, Debug)]
pub struct UniqResult {
    rows: [Folded; ROWS_CAP],
    len: usize,
}

impl UniqResult {
    /// 空白。
    pub const fn new() -> UniqResult {
        UniqResult { rows: [Folded { line: Line::blank(), count: 0 }; ROWS_CAP], len: 0 }
    }

    /// 组数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一组。
    pub fn at(&self, i: usize) -> Option<Folded> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 某组计数。
    pub fn count_of(&self, line: &[u8]) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(f) = self.rows.get(i) {
                if eq_bytes(f.line.bytes(), line) {
                    return Some(f.count);
                }
            }
            i += 1;
        }
        None
    }

    /// 渲染。
    pub fn render(&self, mode: &UniqMode) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(f) = self.rows.get(i) {
                if mode.wants(f.count) {
                    if mode.shows_count() {
                        push_u64(&mut s, f.count as u64);
                        s.push(' ');
                    }
                    s.push_str(f.line.text().as_str());
                    s.push('\n');
                }
            }
            i += 1;
        }
        s
    }
}

/// `uniq`：折叠相邻重复，要求 C 序。
pub fn uniq(list: &Lines) -> Result<UniqResult, u32> {
    list.require_sorted()?;
    let mut out = UniqResult::new();
    let mut i = 0usize;
    while i < list.len() {
        let cur = match list.at(i) {
            Some(v) => v,
            None => return Err(UJ_ENOENT),
        };
        let mut count: u32 = 1;
        let mut j = i + 1;
        while j < list.len() {
            match list.at(j) {
                Some(next) => {
                    if eq_bytes(cur.bytes(), next.bytes()) {
                        count = count.saturating_add(1);
                        j += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        if out.len >= ROWS_CAP {
            return Err(UJ_ELEN);
        }
        if let Some(dst) = out.rows.get_mut(out.len) {
            *dst = Folded { line: cur, count };
        }
        out.len += 1;
        i = j;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// comm
// ---------------------------------------------------------------------------

/// 归并行列号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    /// 仅 A 有。
    OnlyA,
    /// 仅 B 有。
    OnlyB,
    /// 共有。
    Both,
}

impl Column {
    /// 列下标：0/1/2。
    pub fn index(&self) -> usize {
        match self {
            Column::OnlyA => 0,
            Column::OnlyB => 1,
            Column::Both => 2,
        }
    }
}

/// 三路归并结果：按 C 序一次归并出的行序列，每行带所属列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommResult {
    rows: [Line; ROWS_CAP],
    cols: [Column; ROWS_CAP],
    len: usize,
}

impl CommResult {
    /// 空结果。
    pub const fn new() -> CommResult {
        CommResult { rows: [Line::blank(); ROWS_CAP], cols: [Column::OnlyA; ROWS_CAP], len: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取行与所属列。
    pub fn row(&self, i: usize) -> Option<(Line, Column)> {
        if i >= self.len {
            return None;
        }
        match (self.rows.get(i), self.cols.get(i)) {
            (Some(l), Some(c)) => Some((*l, *c)),
            _ => None,
        }
    }

    /// 某列计数。
    pub fn count(&self, c: Column) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(k) = self.cols.get(i) {
                if *k == c {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某列抽成行表。
    pub fn collect(&self, c: Column) -> Lines {
        let mut t = Lines::new();
        let mut i = 0usize;
        while i < self.len {
            if let (Some(l), Some(k)) = (self.rows.get(i), self.cols.get(i)) {
                if *k == c {
                    let _ = t.push(*l);
                }
            }
            i += 1;
        }
        t
    }

    /// 仅 A 有。
    pub fn only_a(&self) -> Lines {
        self.collect(Column::OnlyA)
    }

    /// 仅 B 有。
    pub fn only_b(&self) -> Lines {
        self.collect(Column::OnlyB)
    }

    /// 共有。
    pub fn both(&self) -> Lines {
        self.collect(Column::Both)
    }

    /// 输出行数（未抑制）。
    pub fn lines(&self) -> usize {
        self.len
    }

    /// 按抑制位渲染：列前加 `列号` 个制表符，被抑制的列整行略去。
    pub fn render(&self, suppress: &[bool; COLUMNS]) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            match self.row(i) {
                Some((l, c)) => {
                    let idx = c.index();
                    if !suppress.get(idx).copied().unwrap_or(false) {
                        let mut t = 0usize;
                        while t < idx {
                            s.push('\t');
                            t += 1;
                        }
                        s.push_str(l.text().as_str());
                        s.push('\n');
                    }
                }
                None => break,
            }
            i += 1;
        }
        s
    }
}

/// 归并追加一行。
fn push_row(out: &mut CommResult, l: Line, c: Column) -> Result<(), u32> {
    if out.len >= ROWS_CAP {
        return Err(UJ_ELEN);
    }
    if let Some(dst) = out.rows.get_mut(out.len) {
        *dst = l;
    }
    if let Some(dst) = out.cols.get_mut(out.len) {
        *dst = c;
    }
    out.len += 1;
    Ok(())
}

/// 两路归并：A、B 均须 C 序。
pub fn comm(a: &Lines, b: &Lines) -> Result<CommResult, u32> {
    a.require_sorted()?;
    b.require_sorted()?;
    let mut out = CommResult::new();
    let mut i = 0usize;
    let mut j = 0usize;
    loop {
        let left = a.at(i);
        let right = b.at(j);
        match (left, right) {
            (None, None) => break,
            (Some(l), None) => {
                push_row(&mut out, l, Column::OnlyA)?;
                i += 1;
            }
            (None, Some(r)) => {
                push_row(&mut out, r, Column::OnlyB)?;
                j += 1;
            }
            (Some(l), Some(r)) => {
                let c = c_compare(l.bytes(), r.bytes());
                if c < 0 {
                    push_row(&mut out, l, Column::OnlyA)?;
                    i += 1;
                } else if c > 0 {
                    push_row(&mut out, r, Column::OnlyB)?;
                    j += 1;
                } else {
                    push_row(&mut out, l, Column::Both)?;
                    i += 1;
                    j += 1;
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// join
// ---------------------------------------------------------------------------

/// 内连接结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinResult {
    lines: Lines,
    sep: u8,
}

impl JoinResult {
    /// 结果行。
    pub fn lines(&self) -> Lines {
        self.lines
    }

    /// 分隔符。
    pub fn separator(&self) -> u8 {
        self.sep
    }

    /// 渲染：`a<sep>b`。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.at(i) {
                s.push_str(l.text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 拼接两行的首字段连接结果：`键[<sep>A 余部][<sep>B 余部]`。
fn join_pair(a: &Line, b: &Line, sep: u8) -> Result<Line, u32> {
    let mut buf = [0u8; LINE_CAP];
    let mut at = 0usize;
    let key = a.first_field();
    let mut i = 0usize;
    while i < key.len() {
        match (key.get(i), buf.get_mut(at)) {
            (Some(src), Some(dst)) => {
                *dst = *src;
                at += 1;
            }
            _ => return Err(UJ_ELEN),
        }
        i += 1;
    }
    let rests: [&[u8]; 2] = [a.rest(), b.rest()];
    let mut r = 0usize;
    while r < rests.len() {
        let part: &[u8] = match rests.get(r) {
            Some(v) => *v,
            None => &[],
        };
        if !part.is_empty() {
            match buf.get_mut(at) {
                Some(dst) => {
                    *dst = sep;
                    at += 1;
                }
                None => return Err(UJ_ELEN),
            }
            let mut k = 0usize;
            while k < part.len() {
                match (part.get(k), buf.get_mut(at)) {
                    (Some(src), Some(dst)) => {
                        *dst = *src;
                        at += 1;
                    }
                    _ => return Err(UJ_ELEN),
                }
                k += 1;
            }
        }
        r += 1;
    }
    Line::new(buf.get(0..at).unwrap_or(&[]))
}

/// 首字段内连接：两侧须 C 序，输出 `A<sep>B 余部`。
pub fn join(a: &Lines, b: &Lines, sep: u8) -> Result<JoinResult, u32> {
    if sep < 0x20 || sep > 0x7e {
        return Err(UJ_EBYTE);
    }
    a.require_sorted()?;
    b.require_sorted()?;
    let mut out = Lines::new();
    let mut i = 0usize;
    while i < a.len() {
        let l = match a.at(i) {
            Some(v) => v,
            None => return Err(UJ_ENOENT),
        };
        if l.first_field().is_empty() {
            return Err(UJ_EFIELD);
        }
        let mut j = 0usize;
        while j < b.len() {
            let r = match b.at(j) {
                Some(v) => v,
                None => return Err(UJ_ENOENT),
            };
            if r.first_field().is_empty() {
                return Err(UJ_EFIELD);
            }
            if eq_bytes(l.first_field(), r.first_field()) {
                out.push(join_pair(&l, &r, sep)?)?;
            }
            j += 1;
        }
        i += 1;
    }
    Ok(JoinResult { lines: out, sep })
}

// ---------------------------------------------------------------------------
// 选项
// ---------------------------------------------------------------------------

/// 三工具选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// 折叠模式。
    pub mode: UniqMode,
    /// comm 抑制位。
    pub suppress: [bool; COLUMNS],
    /// join 分隔符。
    pub sep: u8,
    /// 是否强制排序（缺省要求已排序）。
    pub assume_sorted: bool,
}

impl Options {
    /// 缺省。
    pub const fn new() -> Options {
        Options {
            mode: UniqMode::Plain,
            suppress: [false; COLUMNS],
            sep: b' ',
            assume_sorted: true,
        }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("mode ");
        s.push_str(self.mode.text());
        s.push_str(" sep ");
        s.push(self.sep as char);
        if self.assume_sorted {
            s.push_str(" sorted");
        }
        s
    }
}

/// 解析 `uniq/comm/join` 共有选项。
pub fn parse_argv(args: &[&[u8]]) -> Result<Options, u32> {
    let mut o = Options::new();
    let mut mode_seen = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(UJ_EOPT),
        };
        if a.len() > 1 && a.get(0) == Some(&b'-') {
            let mut j = 1usize;
            while j < a.len() {
                let b = match a.get(j) {
                    Some(v) => *v,
                    None => return Err(UJ_EOPT),
                };
                match b {
                    b'c' => {
                        if mode_seen {
                            return Err(UJ_ESTATE);
                        }
                        o.mode = UniqMode::Count;
                        mode_seen = true;
                    }
                    b'd' => {
                        if mode_seen {
                            return Err(UJ_ESTATE);
                        }
                        o.mode = UniqMode::DupOnly;
                        mode_seen = true;
                    }
                    b'u' => {
                        if mode_seen {
                            return Err(UJ_ESTATE);
                        }
                        o.mode = UniqMode::UniqueOnly;
                        mode_seen = true;
                    }
                    b'1' | b'2' | b'3' => {
                        let idx = (b - b'1') as usize;
                        if let Some(dst) = o.suppress.get_mut(idx) {
                            *dst = true;
                        }
                    }
                    b't' => {
                        j += 1;
                        let v = match a.get(j) {
                            Some(v) => *v,
                            None => return Err(UJ_EFIELD),
                        };
                        if v < 0x20 || v > 0x7e {
                            return Err(UJ_EBYTE);
                        }
                        o.sep = v;
                    }
                    _ => return Err(UJ_EOPT),
                }
                j += 1;
            }
            i += 1;
            continue;
        }
        return Err(UJ_EOPT);
    }
    Ok(o)
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            return Err(UJ_ESLOT);
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
            return Err(UJ_EBAL);
        }
        const VALS: [u32; 8] = [1, 3, 2, 2, 1, 2, 3, 1];
        const CODES: [u32; 6] = [UJ_EORDER, UJ_EOPT, UJ_ELEN, UJ_EBYTE, UJ_EFIELD, UJ_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UJ_EBAL),
            };
            if s.slot as usize != i {
                return Err(UJ_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(UJ_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(UJ_EVALUE);
                        }
                    }
                    None => return Err(UJ_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(UJ_EVALUE);
                        }
                    }
                    None => return Err(UJ_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(UJ_EBAL);
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
            return Err(UJ_EBAL);
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
            Err(UJ_ESEAL)
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

/// 样本 A：`a b b d`。
pub fn list_a() -> Result<Lines, u32> {
    Lines::build(&[b"a", b"b", b"b", b"d"])
}

/// 样本 B：`b c d`。
pub fn list_b() -> Result<Lines, u32> {
    Lines::build(&[b"b", b"c", b"d"])
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(
        0,
        OP_ORDER,
        list_a().map(|t| if t.is_sorted() { 1usize } else { 0usize }),
    ));

    let _ = l.register(count_cell(1, OP_UNIQ, list_a().and_then(|t| uniq(&t)).map(|u| u.len())));

    let _ = l.register(count_cell(
        2,
        OP_UNIQ,
        list_a()
            .and_then(|t| uniq(&t))
            .and_then(|u| u.count_of(b"b").ok_or(UJ_ENOENT))
            .map(|c| c as usize),
    ));

    let _ = l.register(count_cell(
        3,
        OP_COMM,
        match (list_a(), list_b()) {
            (Ok(a), Ok(b)) => comm(&a, &b).map(|r| r.only_a().len()),
            (Err(c), _) => Err(c),
            (_, Err(c)) => Err(c),
        },
    ));

    let _ = l.register(count_cell(
        4,
        OP_COMM,
        match (list_a(), list_b()) {
            (Ok(a), Ok(b)) => comm(&a, &b).map(|r| r.only_b().len()),
            (Err(c), _) => Err(c),
            (_, Err(c)) => Err(c),
        },
    ));

    let _ = l.register(count_cell(
        5,
        OP_COMM,
        match (list_a(), list_b()) {
            (Ok(a), Ok(b)) => comm(&a, &b).map(|r| r.both().len()),
            (Err(c), _) => Err(c),
            (_, Err(c)) => Err(c),
        },
    ));

    let _ = l.register(count_cell(
        6,
        OP_JOIN,
        match (list_a(), list_b()) {
            (Ok(a), Ok(b)) => join(&a, &b, b' ').map(|r| r.lines().len()),
            (Err(c), _) => Err(c),
            (_, Err(c)) => Err(c),
        },
    ));

    let _ = l.register(count_cell(
        7,
        OP_ORDER,
        Lines::build(&[b"d", b"a", b"c"])
            .map(|t| t.sorted_copy())
            .and_then(|t| t.at(0).map(|l| l.len()).ok_or(UJ_ENOENT)),
    ));

    let _ = l.register(word_cell(
        8,
        OP_ORDER,
        Lines::build(&[b"b", b"a"]).and_then(|t| match uniq(&t) {
            Err(c) => Err(c),
            Ok(_) => Ok(0),
        }),
    ));

    let _ = l.register(word_cell(9, OP_OPT, parse_argv(&[b"-z"]).map(|_| 0)));

    let _ = l.register(word_cell(
        10,
        OP_UNIQ,
        Lines::build(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i"]).map(|_| 0),
    ));

    let _ = l.register(word_cell(11, OP_OPT, parse_argv(&[b"-t\x01"]).map(|_| 0)));

    let _ = l.register(word_cell(
        12,
        OP_JOIN,
        Lines::build(&[b"a b"]).and_then(|a| {
            Lines::build(&[b" lead"]).and_then(|b| join(&a, &b, b' ').map(|_| 0))
        }),
    ));

    let _ = l.register(word_cell(
        13,
        OP_OPT,
        parse_argv(&[b"-c", b"-d"]).map(|_| 0),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 比较器。
fn compare_ok() -> bool {
    c_compare(b"a", b"b") == -1
        && c_compare(b"b", b"a") == 1
        && c_compare(b"b", b"b") == 0
        && c_compare(b"ab", b"b") == -1
        && c_compare(b"a", b"ab") == -1
        && c_compare(b"", b"a") == -1
        && c_compare(b"Z", b"a") == -1
        && c_compare(b"~", b"a") == 1
}

/// 排序判定。
fn order_ok() -> bool {
    let sorted = match Lines::build(&[b"a", b"b", b"b", b"d"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let unsorted = match Lines::build(&[b"b", b"a"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let dup_ok = match Lines::build(&[b"a", b"a", b"b"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    sorted.is_sorted()
        && sorted.require_sorted() == Ok(())
        && !unsorted.is_sorted()
        && unsorted.require_sorted() == Err(UJ_EORDER)
        && dup_ok.require_sorted() == Ok(())
        && Lines::new().require_sorted() == Err(UJ_ENOENT)
}

/// 折叠。
fn uniq_ok() -> bool {
    let a = match list_a() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let u = match uniq(&a) {
        Ok(v) => v,
        Err(_) => UniqResult::new(),
    };
    let plain = u.render(&UniqMode::Plain);
    let counted = u.render(&UniqMode::Count);
    let dup = u.render(&UniqMode::DupOnly);
    let unique = u.render(&UniqMode::UniqueOnly);
    u.len() == 3
        && u.count_of(b"b") == Some(2)
        && u.count_of(b"a") == Some(1)
        && plain.as_str() == "a\nb\nd\n"
        && counted.as_str() == "1 a\n2 b\n1 d\n"
        && dup.as_str() == "b\n"
        && unique.as_str() == "a\nd\n"
}

/// 三路归并。
fn comm_ok() -> bool {
    let a = match list_a() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let b = match list_b() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let r = match comm(&a, &b) {
        Ok(v) => v,
        Err(_) => CommResult::new(),
    };
    let none = [false; COLUMNS];
    let no_a = [true, false, false];
    let no_b = [false, true, false];
    let no_both = [false, false, true];
    r.only_a().len() == 2
        && r.only_b().len() == 1
        && r.both().len() == 2
        && r.lines() == 5
        && r.count(Column::OnlyA) == 2
        && r.count(Column::Both) == 2
        && r.render(&none).as_str() == "a\n\t\tb\nb\n\tc\n\t\td\n"
        && r.render(&no_a).as_str() == "\t\tb\n\tc\n\t\td\n"
        && r.render(&no_b).as_str() == "a\n\t\tb\nb\n\t\td\n"
        && r.render(&no_both).as_str() == "a\nb\n\tc\n"
}

/// 首字段连接。
fn join_ok() -> bool {
    let a = match list_a() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let b = match list_b() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let r = match join(&a, &b, b' ') {
        Ok(v) => v,
        Err(_) => JoinResult { lines: Lines::new(), sep: b' ' },
    };
    let x = Lines::build(&[b"x 1"]).unwrap_or_else(|_| Lines::new());
    let y = Lines::build(&[b"x 2"]).unwrap_or_else(|_| Lines::new());
    let two = match join(&x, &y, b':') {
        Ok(v) => v,
        Err(_) => JoinResult { lines: Lines::new(), sep: b':' },
    };
    r.lines().len() == 3
        && r.render().as_str() == "b\nb\nd\n"
        && two.render().as_str() == "x:1:2\n"
        && two.separator() == b':'
}

/// 字段与分隔符闸门。
fn field_ok() -> bool {
    let a = match list_a() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let lead = match Lines::build(&[b" y"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let name = match Lines::build(&[b"x 1"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let other = match Lines::build(&[b"x 2"]) {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let bad_sep = join(&a, &lead, b'\t') == Err(UJ_EBYTE);
    let lead_err = join(&a, &lead, b' ') == Err(UJ_EFIELD);
    let pair = match join(&name, &other, b' ') {
        Ok(v) => v,
        Err(_) => JoinResult { lines: Lines::new(), sep: b' ' },
    };
    bad_sep && lead_err && pair.render().as_str() == "x 1 2\n"
}

/// 选项面。
fn option_ok() -> bool {
    let a = parse_argv(&[b"-c"]).map(|o| o.mode == UniqMode::Count) == Ok(true);
    let b = parse_argv(&[b"-d"]).map(|o| o.mode == UniqMode::DupOnly) == Ok(true);
    let c = parse_argv(&[b"-u"]).map(|o| o.mode == UniqMode::UniqueOnly) == Ok(true);
    let d = parse_argv(&[b"-12"]).map(|o| o.suppress == [true, true, false]) == Ok(true);
    let e = parse_argv(&[b"-3"]).map(|o| o.suppress == [false, false, true]) == Ok(true);
    let f = parse_argv(&[b"-t:"]).map(|o| o.sep == b':') == Ok(true);
    let g = parse_argv(&[b"-ct:"]).map(|o| o.mode == UniqMode::Count && o.sep == b':') == Ok(true);
    a && b && c && d && e && f && g
}

/// 选项错误面。
fn option_err_ok() -> bool {
    parse_argv(&[b"-z"]).err() == Some(UJ_EOPT)
        && parse_argv(&[b"-c", b"-d"]).err() == Some(UJ_ESTATE)
        && parse_argv(&[b"-u", b"-u"]).err() == Some(UJ_ESTATE)
        && parse_argv(&[b"-t"]).err() == Some(UJ_EFIELD)
        && parse_argv(&[b"-t\x01"]).err() == Some(UJ_EBYTE)
        && parse_argv(&[b"file"]).err() == Some(UJ_EOPT)
}

/// 渲染与摘要。
fn render_ok() -> bool {
    let a = match list_a() {
        Ok(v) => v,
        Err(_) => Lines::new(),
    };
    let u = match uniq(&a) {
        Ok(v) => v,
        Err(_) => UniqResult::new(),
    };
    let text = u.render(&UniqMode::Count);
    let digest = a.digest();
    let same = match list_a() {
        Ok(v) => v.digest(),
        Err(_) => 0,
    };
    text.as_str().starts_with("1 a\n")
        && text.as_str().matches('\n').count() == 3
        && digest == same
        && a.render().as_str() == "a\nb\nb\nd\n"
        && a.render().as_str().matches('\n').count() == 4
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l2.register(probe) != Err(UJ_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = compare_ok() && order_ok();
    out[1] = uniq_ok() && comm_ok();
    out[2] = ledger_ok();
    out[3] = join_ok() && field_ok() && option_ok() && option_err_ok() && render_ok();
    out
}

/// 判据集合。
pub fn run_unx11572_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11572.ujc");

    s.add(
        "UJ01-字节比较器",
        {
            compare_ok()
        },
        "static",
    );

    s.add(
        "UJ02-排序闸门",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "UJ03-相邻折叠",
        {
            uniq_ok()
        },
        "static",
    );

    s.add(
        "UJ04-三路归并",
        {
            comm_ok()
        },
        "static",
    );

    s.add(
        "UJ05-首字段连接",
        {
            join_ok()
        },
        "static",
    );

    s.add(
        "UJ06-字段闸门",
        {
            field_ok()
        },
        "static",
    );

    s.add(
        "UJ07-选项组合",
        {
            option_ok()
        },
        "static",
    );

    s.add(
        "UJ08-选项错误码",
        {
            option_err_ok()
        },
        "static",
    );

    s.add(
        "UJ09-渲染与摘要",
        {
            render_ok()
        },
        "static",
    );

    s.add(
        "UJ10-容量边界",
        {
            let a = Lines::build(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i"]).err()
                == Some(UJ_ELEN);
            let b = Line::new(&[b'x'; 65]).err() == Some(UJ_ELEN);
            let c = Line::new(b"").err() == Some(UJ_EBYTE);
            let d = Lines::build(&[]).err() == Some(UJ_ENOENT);
            let e = match Lines::build(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h"]) {
                Ok(t) => t.len() == 8,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "UJ11-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "UJ12-排序副本",
        {
            let t = match Lines::build(&[b"d", b"a", b"c", b"b"]) {
                Ok(v) => v,
                Err(_) => Lines::new(),
            };
            let s = t.sorted_copy();
            s.is_sorted()
                && s.render().as_str() == "a\nb\nc\nd\n"
                && t.render().as_str() == "d\na\nc\nb\n"
                && s.require_sorted() == Ok(())
        },
        "static",
    );

    s.add(
        "UJ13-冻结重放四注入",
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
    fn byte_compare() {
        assert_eq!(c_compare(b"a", b"b"), -1);
        assert_eq!(c_compare(b"b", b"a"), 1);
        assert_eq!(c_compare(b"ab", b"ab"), 0);
        assert_eq!(c_compare(b"ab", b"abc"), -1);
        assert_eq!(c_compare(b"Z", b"a"), -1);
    }

    #[test]
    fn sorted_gate() {
        let ok = Lines::build(&[b"a", b"b", b"b"]).expect("ok");
        assert!(ok.is_sorted());
        assert_eq!(ok.require_sorted(), Ok(()));
        let bad = Lines::build(&[b"b", b"a"]).expect("bad");
        assert!(!bad.is_sorted());
        assert_eq!(bad.require_sorted(), Err(UJ_EORDER));
    }

    #[test]
    fn fold_modes() {
        let a = list_a().expect("a");
        let u = uniq(&a).expect("u");
        assert_eq!(u.len(), 3);
        assert_eq!(u.count_of(b"b"), Some(2));
        assert_eq!(u.render(&UniqMode::Plain).as_str(), "a\nb\nd\n");
        assert_eq!(u.render(&UniqMode::Count).as_str(), "1 a\n2 b\n1 d\n");
        assert_eq!(u.render(&UniqMode::DupOnly).as_str(), "b\n");
        assert_eq!(u.render(&UniqMode::UniqueOnly).as_str(), "a\nd\n");
    }

    #[test]
    fn unsorted_rejected() {
        let bad = Lines::build(&[b"c", b"a"]).expect("bad");
        assert_eq!(uniq(&bad).err(), Some(UJ_EORDER));
        let a = list_a().expect("a");
        assert_eq!(comm(&bad, &a).err(), Some(UJ_EORDER));
        assert_eq!(join(&bad, &a, b' ').err(), Some(UJ_EORDER));
    }

    #[test]
    fn comm_columns() {
        let a = list_a().expect("a");
        let b = list_b().expect("b");
        let r = comm(&a, &b).expect("r");
        assert_eq!(r.only_a().len(), 2);
        assert_eq!(r.only_b().len(), 1);
        assert_eq!(r.both().len(), 2);
        assert_eq!(r.lines(), 5);
        assert_eq!(r.render(&[false, false, false]).as_str(), "a\n\t\tb\nb\n\tc\n\t\td\n");
        assert_eq!(r.row(0).map(|(_, c)| c), Some(Column::OnlyA));
        assert_eq!(r.row(1).map(|(_, c)| c), Some(Column::Both));
    }

    #[test]
    fn comm_suppress() {
        let a = list_a().expect("a");
        let b = list_b().expect("b");
        let r = comm(&a, &b).expect("r");
        assert_eq!(r.render(&[true, false, false]).as_str(), "\t\tb\n\tc\n\t\td\n");
        assert_eq!(r.render(&[false, true, false]).as_str(), "a\n\t\tb\nb\n\t\td\n");
        assert_eq!(r.render(&[true, true, true]).as_str(), "");
    }

    #[test]
    fn join_inner() {
        let a = list_a().expect("a");
        let b = list_b().expect("b");
        let r = join(&a, &b, b' ').expect("r");
        assert_eq!(r.lines().len(), 3);
        assert_eq!(r.render().as_str(), "b\nb\nd\n");
        let x = Lines::build(&[b"x 1"]).expect("x");
        let y = Lines::build(&[b"x 2"]).expect("y");
        let q = join(&x, &y, b':').expect("q");
        assert_eq!(q.render().as_str(), "x:1:2\n");
    }

    #[test]
    fn join_fields() {
        let name = Lines::build(&[b"x 1"]).expect("n");
        let other = Lines::build(&[b"x 2"]).expect("o");
        let r = join(&name, &other, b' ').expect("r");
        assert_eq!(r.render().as_str(), "x 1 2\n");
        let lead = Lines::build(&[b" y"]).expect("l");
        assert_eq!(join(&name, &lead, b' ').err(), Some(UJ_EFIELD));
        assert_eq!(join(&name, &other, b'\t').err(), Some(UJ_EBYTE));
    }

    #[test]
    fn options() {
        assert_eq!(parse_argv(&[b"-c"]).map(|o| o.mode), Ok(UniqMode::Count));
        assert_eq!(parse_argv(&[b"-d"]).map(|o| o.mode), Ok(UniqMode::DupOnly));
        assert_eq!(parse_argv(&[b"-12"]).map(|o| o.suppress), Ok([true, true, false]));
        assert_eq!(parse_argv(&[b"-t:"]).map(|o| o.sep), Ok(b':'));
        assert_eq!(parse_argv(&[b"-z"]).err(), Some(UJ_EOPT));
        assert_eq!(parse_argv(&[b"-c", b"-d"]).err(), Some(UJ_ESTATE));
        assert_eq!(parse_argv(&[b"-t"]).err(), Some(UJ_EFIELD));
    }

    #[test]
    fn sort_copy() {
        let t = Lines::build(&[b"d", b"a", b"c", b"b"]).expect("t");
        let s = t.sorted_copy();
        assert!(s.is_sorted());
        assert_eq!(s.render().as_str(), "a\nb\nc\nd\n");
    }

    #[test]
    fn fields() {
        let l = Line::new(b"b rest of line").expect("l");
        assert_eq!(l.first_field(), b"b");
        assert_eq!(l.rest(), b"rest of line");
        let bare = Line::new(b"x").expect("x");
        assert_eq!(bare.first_field(), b"x");
        assert_eq!(bare.rest(), b"");
    }

    #[test]
    fn render_all() {
        let a = list_a().expect("a");
        assert_eq!(a.render().as_str(), "a\nb\nb\nd\n");
        assert_eq!(a.digest(), list_a().expect("a").digest());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(1).map(|s| s.value), Some(3));
        assert_eq!(l.sample(2).map(|s| s.value), Some(2));
        assert_eq!(l.sample(8).map(|s| s.code), Some(UJ_EORDER));
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(UJ_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(UJ_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11572_checks();
        assert!(s.all_passed());
    }
}
