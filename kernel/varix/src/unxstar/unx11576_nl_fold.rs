//! UNX-F11576 nl/fold 字节宽度判据
//!
//! `fold` 与 `nl` 的列宽在 C locale 下**等于字节位**：一个字节占一位，
//! 没有宽字符（东亚双宽、组合附加、零宽连接）的概念，`0x80..=0xFF` 的
//! 每一个字节都计 1 列。于是「160 个 0x80 在 `-w 80` 下折成 2 行」成为一条
//! 可复算的硬判据；任何偷偷按 UTF-8 字符数或显示宽度折算的实现都会在此
//! 露馅。本模块同时把 `nl` 的行编号模型做进来，两条工具共享同一套列宽账：
//!
//! * 列宽层：[`column_span`] 逐字节累加列号；`\t` 在默认模式下跳到下一个
//!   8 的倍数（`-b` 时只算 1），`\x08` 回退一列（饱和到 0），`\r` 归零，
//!   `\n` 归零并断行。0x80..=0xFF 一律 1 列。
//! * 折叠层：[`fold`] 用定容行缓冲（256 字节）做硬断行；`-s` 模式下溢出时
//!   回退到本行最后一个空格处断开，剩余部分重算列号后继续；单个超宽词
//!   仍然硬断，绝不留下一行超过宽度（制表符跳列是唯一例外，与 GNU 同）。
//! * 编号层：[`nl`] 支持 `-b a/t/n` 三种编号面、`ln/rn/rz` 三种格式、
//!   `-v` 起始、`-i` 步长、`-w` 宽度、`-s` 分隔符，以及 `\:\:\:` /
//!   `\:\:` 两级分隔符重置。
//! * 账本层：14 格样本（8 成 6 败）把列宽、折叠行数、编号前缀长度、
//!   选项错误码全部固定下来，冻结后重放必须同指纹。
//!
//! 闸门次序：字节 → 宽度 → 列号 → 断行 → 空格回退 → 编号 → 分隔 → 渲染。
//!
//! 全模块无 std、无 unsafe、无 panic：缓冲定容、索引一律 `get()`、
//! 减法一律 `saturating_sub`；失败全部落 13 个错误码。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf930;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 行缓冲容量。
pub const LINE_CAP: usize = 256;

/// 列宽上界。
pub const WIDTH_MAX: usize = 200;

/// 制表位。
pub const TAB_STOP: usize = 8;

/// `nl` 编号宽度上界。
pub const NUM_WIDTH_MAX: u8 = 20;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 文本含 0x00。
pub const NF_EBYTE: u32 = SEG_BASE + 0x01;
/// 列宽越界（0 或超上界）。
pub const NF_EWIDTH: u32 = SEG_BASE + 0x02;
/// 编号样式未知。
pub const NF_ESTYLE: u32 = SEG_BASE + 0x03;
/// 编号格式未知。
pub const NF_EFORMAT: u32 = SEG_BASE + 0x04;
/// 起始值/步长为 0。
pub const NF_ESTART: u32 = SEG_BASE + 0x05;
/// 分隔符非法。
pub const NF_EDELIM: u32 = SEG_BASE + 0x06;
/// 选项语法错误。
pub const NF_EOPT: u32 = SEG_BASE + 0x07;
/// 状态冲突（重复选项、行缓冲溢出）。
pub const NF_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const NF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const NF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const NF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const NF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const NF_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 列号推算。
pub const OP_COL: u8 = 0x01;
/// 折叠。
pub const OP_FOLD: u8 = 0x02;
/// 编号。
pub const OP_NL: u8 = 0x03;
/// 编号前缀格式化。
pub const OP_FMT: u8 = 0x04;
/// 选项解析。
pub const OP_PARSE: u8 = 0x05;
/// 计数。
pub const OP_COUNT: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（逐位八次乘法）。
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
fn fold_word(h: u64, w: u64) -> u64 {
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

/// 逐字节相等。
pub fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
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

/// 推入无符号十进制。
fn push_u32(out: &mut String, mut v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 10];
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
            out.push(*b as char);
        }
        i += 1;
    }
}

/// 十进制位数。
pub fn digit_count(mut v: u32) -> usize {
    if v == 0 {
        return 1;
    }
    let mut n = 0usize;
    while v > 0 {
        v /= 10;
        n += 1;
    }
    n
}

/// 制表推进量（不改变列号）。
pub fn tab_advance(col: usize) -> usize {
    let rem = col % TAB_STOP;
    TAB_STOP - rem
}

/// 制表后的列号。
pub fn tab_column(col: usize) -> usize {
    col.saturating_add(tab_advance(col))
}

// ---------------------------------------------------------------------------
// 列号
// ---------------------------------------------------------------------------

/// 逐字节推算末列号（不折行）。
pub fn column_span(text: &[u8], bytes_only: bool) -> Result<usize, u32> {
    let mut col = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        let b = match text.get(i) {
            Some(v) => *v,
            None => return Err(NF_EBYTE),
        };
        if b == 0 {
            return Err(NF_EBYTE);
        }
        if b == b'\n' || b == b'\r' {
            col = 0;
        } else if b == 0x08 {
            col = col.saturating_sub(1);
        } else if b == b'\t' {
            col = if bytes_only { col.saturating_add(1) } else { tab_column(col) };
        } else {
            col = col.saturating_add(1);
        }
        i += 1;
    }
    Ok(col)
}

/// 单字节的列宽（0x80..=0xFF 与 ASCII 同为 1）。
pub fn byte_width(b: u8) -> usize {
    if b == b'\t' {
        TAB_STOP
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// 行缓冲
// ---------------------------------------------------------------------------

/// 定容行缓冲。
#[derive(Clone, Copy)]
struct Line {
    bytes: [u8; LINE_CAP],
    len: usize,
    col: usize,
    /// 最后一个空格的下标 + 1；0 表示没有空格。
    last_space: usize,
}

impl Line {
    const fn new() -> Line {
        Line { bytes: [0u8; LINE_CAP], len: 0, col: 0, last_space: 0 }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.col = 0;
        self.last_space = 0;
    }

    fn push(&mut self, b: u8) -> Result<(), u32> {
        if self.len >= LINE_CAP {
            return Err(NF_ESTATE);
        }
        match self.bytes.get_mut(self.len) {
            Some(dst) => {
                *dst = b;
                self.len += 1;
                Ok(())
            }
            None => Err(NF_ESTATE),
        }
    }

    fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.bytes.get(i).copied()
    }

    /// 丢弃 `0..p`，把 `p..len` 前移。
    fn shift_from(&mut self, p: usize) {
        let mut w = 0usize;
        let mut i = p;
        while i < self.len {
            if let Some(b) = self.bytes.get(i).copied() {
                if let Some(dst) = self.bytes.get_mut(w) {
                    *dst = b;
                }
                w += 1;
            }
            i += 1;
        }
        self.len = w;
        self.last_space = 0;
        let mut k = 0usize;
        while k < self.len {
            if self.bytes.get(k) == Some(&b' ') {
                self.last_space = k + 1;
            }
            k += 1;
        }
    }

    /// 从前缀重算列号（不折行）。
    fn recompute(&mut self, bytes_only: bool) {
        let mut col = 0usize;
        let mut last = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.bytes.get(i).copied() {
                Some(b'\t') => {
                    col = if bytes_only { col.saturating_add(1) } else { tab_column(col) };
                }
                Some(0x08) => col = col.saturating_sub(1),
                Some(b'\r') => col = 0,
                Some(b' ') => {
                    col = col.saturating_add(1);
                    last = i + 1;
                }
                Some(_) => col = col.saturating_add(1),
                None => {}
            }
            i += 1;
        }
        self.col = col;
        self.last_space = last;
    }
}

/// 把整行推入输出并补换行，返回原始字节数（含换行）。
fn emit_line(out: &mut String, line: &Line) -> usize {
    let mut i = 0usize;
    while i < line.len {
        if let Some(b) = line.at(i) {
            out.push(b as char);
        }
        i += 1;
    }
    out.push('\n');
    line.len + 1
}

/// 把行首 `0..p` 推入输出并补换行，返回原始字节数（含换行）。
fn emit_prefix(out: &mut String, line: &Line, p: usize) -> usize {
    let mut i = 0usize;
    while i < p {
        if let Some(b) = line.at(i) {
            out.push(b as char);
        }
        i += 1;
    }
    out.push('\n');
    p + 1
}

// ---------------------------------------------------------------------------
// 折叠
// ---------------------------------------------------------------------------

/// 折叠选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldOpts {
    /// 列宽。
    pub width: usize,
    /// `-b` 只数字节。
    pub bytes_only: bool,
    /// `-s` 在空格处断。
    pub break_spaces: bool,
}

impl FoldOpts {
    /// 缺省 80 列、按列、硬断。
    pub const fn new() -> FoldOpts {
        FoldOpts { width: 80, bytes_only: false, break_spaces: false }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("w=");
        push_u32(&mut s, self.width as u32);
        if self.bytes_only {
            s.push_str(" bytes");
        }
        if self.break_spaces {
            s.push_str(" spaces");
        }
        s
    }
}

/// 折叠结果。
#[derive(Clone, Debug)]
pub struct FoldResult {
    text: String,
    lines: usize,
    max_col: usize,
    bytes: usize,
}

impl FoldResult {
    /// 文本。
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 行数。
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// 最大列号。
    pub fn max_col(&self) -> usize {
        self.max_col
    }

    /// 输出字节数。
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// 折叠：按字节列宽断行。
pub fn fold(text: &[u8], opts: &FoldOpts) -> Result<FoldResult, u32> {
    if opts.width == 0 || opts.width > WIDTH_MAX {
        return Err(NF_EWIDTH);
    }
    let width = opts.width;
    let mut out = String::new();
    let mut lines = 0usize;
    let mut raw = 0usize;
    let mut max = 0usize;
    let mut line = Line::new();
    let mut i = 0usize;
    while i < text.len() {
        let b = match text.get(i) {
            Some(v) => *v,
            None => return Err(NF_EBYTE),
        };
        if b == 0 {
            return Err(NF_EBYTE);
        }
        if b == b'\n' {
            raw += emit_line(&mut out, &line);
            lines += 1;
            line.clear();
            i += 1;
            continue;
        }
        if b == b'\r' {
            line.push(b)?;
            line.col = 0;
            line.last_space = 0;
            i += 1;
            continue;
        }
        if b == 0x08 {
            line.push(b)?;
            line.col = line.col.saturating_sub(1);
            i += 1;
            continue;
        }
        let advance = if opts.bytes_only {
            1usize
        } else if b == b'\t' {
            tab_advance(line.col)
        } else {
            1usize
        };
        if line.len > 0 && line.col.saturating_add(advance) > width {
            if opts.break_spaces && line.last_space > 0 {
                let p = line.last_space - 1;
                raw += emit_prefix(&mut out, &line, p);
                lines += 1;
                line.shift_from(p + 1);
                line.recompute(opts.bytes_only);
                if line.len > 0 && line.col.saturating_add(advance) > width {
                    raw += emit_line(&mut out, &line);
                    lines += 1;
                    line.clear();
                }
            } else {
                raw += emit_line(&mut out, &line);
                lines += 1;
                line.clear();
            }
        }
        line.push(b)?;
        if b == b'\t' && !opts.bytes_only {
            line.col = tab_column(line.col);
        } else {
            line.col = line.col.saturating_add(1);
        }
        if b == b' ' {
            line.last_space = line.len;
        }
        if line.col > max {
            max = line.col;
        }
        i += 1;
    }
    if line.len > 0 {
        raw += emit_line(&mut out, &line);
        lines += 1;
    }
    let bytes = raw;
    Ok(FoldResult { text: out, lines, max_col: max, bytes })
}

// ---------------------------------------------------------------------------
// nl
// ---------------------------------------------------------------------------

/// 编号样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// `-b n` 不编号。
    None,
    /// `-b t` 只编非空行。
    NonEmpty,
    /// `-b a` 全编。
    All,
}

impl Style {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Style::None => "none",
            Style::NonEmpty => "nonempty",
            Style::All => "all",
        }
    }

    /// 由 `a`/`t`/`n` 解析。
    pub fn from_byte(b: u8) -> Result<Style, u32> {
        match b {
            b'a' => Ok(Style::All),
            b't' => Ok(Style::NonEmpty),
            b'n' => Ok(Style::None),
            _ => Err(NF_ESTYLE),
        }
    }

    /// 该行是否编号。
    pub fn numbers(&self, empty: bool) -> bool {
        match self {
            Style::None => false,
            Style::NonEmpty => !empty,
            Style::All => true,
        }
    }
}

/// 编号格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// `ln` 左对齐。
    Left,
    /// `rn` 右对齐。
    Right,
    /// `rz` 前导零。
    Zero,
}

impl Format {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Format::Left => "ln",
            Format::Right => "rn",
            Format::Zero => "rz",
        }
    }

    /// 由两个字节解析。
    pub fn from_bytes(b: &[u8]) -> Result<Format, u32> {
        if eq_bytes(b, b"ln") {
            return Ok(Format::Left);
        }
        if eq_bytes(b, b"rn") {
            return Ok(Format::Right);
        }
        if eq_bytes(b, b"rz") {
            return Ok(Format::Zero);
        }
        Err(NF_EFORMAT)
    }
}

/// 编号选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NlOpts {
    /// 样式。
    pub style: Style,
    /// 格式。
    pub format: Format,
    /// 起始号。
    pub start: u32,
    /// 步长。
    pub increment: u32,
    /// 编号宽度。
    pub width: u8,
    /// 分隔符。
    pub sep: u8,
}

impl NlOpts {
    /// 缺省：仅非空行、右对齐、1 起 1 步、宽 6、TAB 分隔。
    pub const fn new() -> NlOpts {
        NlOpts {
            style: Style::NonEmpty,
            format: Format::Right,
            start: 1,
            increment: 1,
            width: 6,
            sep: b'\t',
        }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(self.style.text());
        s.push('/');
        s.push_str(self.format.text());
        s.push_str(" v=");
        push_u32(&mut s, self.start);
        s.push_str(" i=");
        push_u32(&mut s, self.increment);
        s.push_str(" w=");
        push_u32(&mut s, self.width as u32);
        s
    }
}

/// 编号结果。
#[derive(Clone, Debug)]
pub struct NlResult {
    text: String,
    lines: usize,
    numbered: usize,
    last: u32,
}

impl NlResult {
    /// 文本。
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 行数。
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// 被编号的行数。
    pub fn numbered(&self) -> usize {
        self.numbered
    }

    /// 最后一个编号（无则 0）。
    pub fn last(&self) -> u32 {
        self.last
    }
}

/// 编号前缀：数字按格式填到宽度，再接分隔符。
pub fn number_prefix(opts: &NlOpts, n: u32) -> String {
    let mut digits = String::new();
    push_u32(&mut digits, n);
    let dn = digits.len();
    let w = opts.width as usize;
    let padn = w.saturating_sub(dn);
    let mut out = String::new();
    match opts.format {
        Format::Left => {
            out.push_str(digits.as_str());
            let mut k = 0usize;
            while k < padn {
                out.push(' ');
                k += 1;
            }
        }
        Format::Right => {
            let mut k = 0usize;
            while k < padn {
                out.push(' ');
                k += 1;
            }
            out.push_str(digits.as_str());
        }
        Format::Zero => {
            let mut k = 0usize;
            while k < padn {
                out.push('0');
                k += 1;
            }
            out.push_str(digits.as_str());
        }
    }
    out.push(opts.sep as char);
    out
}

/// 是否一级分隔符 `\:\:\:`。
fn is_major(line: &[u8]) -> bool {
    eq_bytes(line, b"\\:\\:\\:")
}

/// 是否二级分隔符 `\:\:`。
fn is_secondary(line: &[u8]) -> bool {
    eq_bytes(line, b"\\:\\:")
}

/// 编号整段文本。
pub fn nl(text: &[u8], opts: &NlOpts) -> Result<NlResult, u32> {
    if opts.start == 0 || opts.increment == 0 {
        return Err(NF_ESTART);
    }
    if opts.width == 0 || opts.width > NUM_WIDTH_MAX {
        return Err(NF_EWIDTH);
    }
    let mut out = String::new();
    let mut lines = 0usize;
    let mut numbered = 0usize;
    let mut num = opts.start;
    let mut last = 0u32;
    let mut start = 0usize;
    let mut i = 0usize;
    loop {
        let at_end = i >= text.len();
        let is_break = match text.get(i) {
            Some(v) => *v == b'\n',
            None => false,
        };
        if at_end || is_break {
            if !(at_end && start >= text.len()) {
                let seg = text.get(start..i).unwrap_or(&[]);
                let mut k = 0usize;
                while k < seg.len() {
                    if seg.get(k) == Some(&0) {
                        return Err(NF_EBYTE);
                    }
                    k += 1;
                }
                if is_major(seg) {
                    num = opts.start;
                    push_segment(&mut out, seg);
                } else if is_secondary(seg) {
                    num = 1;
                    push_segment(&mut out, seg);
                } else {
                    let empty = seg.is_empty();
                    if opts.style.numbers(empty) {
                        out.push_str(number_prefix(opts, num).as_str());
                        last = num;
                        numbered += 1;
                        num = num.saturating_add(opts.increment);
                    }
                    push_segment(&mut out, seg);
                }
                lines += 1;
            }
            if at_end {
                break;
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(NlResult { text: out, lines, numbered, last })
}

/// 追加一段字节并补换行。
fn push_segment(out: &mut String, seg: &[u8]) {
    let mut i = 0usize;
    while i < seg.len() {
        if let Some(b) = seg.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// 选项解析
// ---------------------------------------------------------------------------

/// 解析十进制（无符号）。
fn parse_u32(b: &[u8]) -> Result<u32, u32> {
    if b.is_empty() {
        return Err(NF_EOPT);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(NF_EOPT),
        };
        if c < b'0' || c > b'9' {
            return Err(NF_EOPT);
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
        if v > 1_000_000 {
            return Err(NF_EWIDTH);
        }
        i += 1;
    }
    Ok(v)
}

/// 解析 `fold` 选项：`-wN` `-b` `-s`。
pub fn parse_fold_args(args: &[&[u8]]) -> Result<FoldOpts, u32> {
    let mut o = FoldOpts::new();
    let mut seen_w = false;
    let mut seen_b = false;
    let mut seen_s = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(NF_EOPT),
        };
        if eq_bytes(a, b"-b") {
            if seen_b {
                return Err(NF_ESTATE);
            }
            o.bytes_only = true;
            seen_b = true;
        } else if eq_bytes(a, b"-s") {
            if seen_s {
                return Err(NF_ESTATE);
            }
            o.break_spaces = true;
            seen_s = true;
        } else if a.get(0) == Some(&b'-') && a.get(1) == Some(&b'w') {
            if seen_w {
                return Err(NF_ESTATE);
            }
            let w = parse_u32(a.get(2..).unwrap_or(&[]))?;
            if w == 0 || w > WIDTH_MAX as u32 {
                return Err(NF_EWIDTH);
            }
            o.width = w as usize;
            seen_w = true;
        } else {
            return Err(NF_EOPT);
        }
        i += 1;
    }
    Ok(o)
}

/// 解析 `nl` 选项。
pub fn parse_nl_args(args: &[&[u8]]) -> Result<NlOpts, u32> {
    let mut o = NlOpts::new();
    let mut seen_style = false;
    let mut seen_format = false;
    let mut seen_start = false;
    let mut seen_inc = false;
    let mut seen_width = false;
    let mut seen_sep = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(NF_EOPT),
        };
        if a.len() == 3 && a.get(0) == Some(&b'-') && a.get(1) == Some(&b'b') {
            if seen_style {
                return Err(NF_ESTATE);
            }
            let c = match a.get(2) {
                Some(v) => *v,
                None => return Err(NF_ESTYLE),
            };
            o.style = Style::from_byte(c)?;
            seen_style = true;
        } else if a.len() == 4 && a.get(0) == Some(&b'-') && a.get(1) == Some(&b'n') {
            if seen_format {
                return Err(NF_ESTATE);
            }
            o.format = Format::from_bytes(a.get(2..4).unwrap_or(&[]))?;
            seen_format = true;
        } else if a.get(0) == Some(&b'-') && a.get(1) == Some(&b'w') {
            if seen_width {
                return Err(NF_ESTATE);
            }
            let w = parse_u32(a.get(2..).unwrap_or(&[]))?;
            if w == 0 || w > NUM_WIDTH_MAX as u32 {
                return Err(NF_EWIDTH);
            }
            o.width = w as u8;
            seen_width = true;
        } else if a.get(0) == Some(&b'-') && a.get(1) == Some(&b'v') {
            if seen_start {
                return Err(NF_ESTATE);
            }
            let v = parse_u32(a.get(2..).unwrap_or(&[]))?;
            if v == 0 {
                return Err(NF_ESTART);
            }
            o.start = v;
            seen_start = true;
        } else if a.get(0) == Some(&b'-') && a.get(1) == Some(&b'i') {
            if seen_inc {
                return Err(NF_ESTATE);
            }
            let v = parse_u32(a.get(2..).unwrap_or(&[]))?;
            if v == 0 {
                return Err(NF_ESTART);
            }
            o.increment = v;
            seen_inc = true;
        } else if a.len() == 3 && a.get(0) == Some(&b'-') && a.get(1) == Some(&b's') {
            if seen_sep {
                return Err(NF_ESTATE);
            }
            let c = match a.get(2) {
                Some(v) => *v,
                None => return Err(NF_EDELIM),
            };
            if c < 0x20 || c == 0x7f {
                return Err(NF_EDELIM);
            }
            o.sep = c;
            seen_sep = true;
        } else {
            return Err(NF_EOPT);
        }
        i += 1;
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NF_ESLOT);
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
            return Err(NF_EBAL);
        }
        const VALS: [u32; 8] = [9, 80, 2, 12, 3, 7, 3, 2];
        const CODES: [u32; 6] = [NF_EWIDTH, NF_EWIDTH, NF_ESTYLE, NF_EFORMAT, NF_ESTART, NF_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NF_EBAL),
            };
            if s.slot as usize != i {
                return Err(NF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NF_EVALUE);
                        }
                    }
                    None => return Err(NF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NF_EVALUE);
                        }
                    }
                    None => return Err(NF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NF_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NF_EBAL);
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
            Err(NF_ESEAL)
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

// ---------------------------------------------------------------------------
// 账本组装
// ---------------------------------------------------------------------------

/// `Result<usize, u32>` → 样本。
fn size_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn u32_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 高位填充样本：160 个 0x80。
fn high160() -> [u8; 160] {
    [0x80u8; 160]
}

/// 缺省折叠选项。
fn opt(width: usize, bytes_only: bool, break_spaces: bool) -> FoldOpts {
    FoldOpts { width, bytes_only, break_spaces }
}

/// 账本组装：槽 0..7 成、8..13 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(size_cell(0, OP_COL, column_span(b"\tx", false)));

    let hi = high160();
    let _ = l.register(size_cell(
        1,
        OP_FOLD,
        fold(&hi, &opt(80, true, false)).map(|r| r.max_col()),
    ));

    let _ = l.register(size_cell(
        2,
        OP_FOLD,
        fold(&hi, &opt(80, true, false)).map(|r| r.lines()),
    ));

    let _ = l.register(size_cell(
        3,
        OP_FOLD,
        fold(b"hello world", &opt(80, false, false)).map(|r| r.bytes()),
    ));

    let nl_all = NlOpts { style: Style::All, ..NlOpts::new() };
    let _ = l.register(size_cell(
        4,
        OP_NL,
        nl(b"a\nb\nc\n", &nl_all).map(|r| r.lines()),
    ));

    let _ = l.register(size_cell(
        5,
        OP_FMT,
        Ok(number_prefix(&NlOpts::new(), 1).len()),
    ));

    let _ = l.register(size_cell(
        6,
        OP_FOLD,
        fold(b"a\nb\nc\n", &opt(80, false, false)).map(|r| r.lines()),
    ));

    let _ = l.register(size_cell(
        7,
        OP_FOLD,
        fold(b"aaa bbb ccc", &opt(7, true, true)).map(|r| r.lines()),
    ));

    let _ = l.register(size_cell(
        8,
        OP_FOLD,
        fold(b"abc", &opt(0, false, false)).map(|r| r.lines()),
    ));
    let _ = l.register(size_cell(
        9,
        OP_FOLD,
        fold(b"abc", &opt(WIDTH_MAX + 1, false, false)).map(|r| r.lines()),
    ));
    let _ = l.register(u32_cell(
        10,
        OP_PARSE,
        parse_nl_args(&[b"-bx"]).map(|o| o.width as u32),
    ));
    let _ = l.register(u32_cell(
        11,
        OP_PARSE,
        parse_nl_args(&[b"-nnx"]).map(|o| o.width as u32),
    ));
    let _ = l.register(u32_cell(
        12,
        OP_PARSE,
        parse_nl_args(&[b"-v0"]).map(|o| o.start),
    ));
    let _ = l.register(size_cell(
        13,
        OP_FOLD,
        fold(&[b'a', 0, b'b'], &opt(80, false, false)).map(|r| r.lines()),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 列号面。
fn column_ok() -> bool {
    let a = column_span(b"\tx", false) == Ok(9);
    let b = column_span(b"\t\tx", false) == Ok(17);
    let c = column_span(b"abc", false) == Ok(3);
    let d = column_span(b"abc\x08", false) == Ok(2);
    let e = column_span(b"abc\rx", false) == Ok(1);
    let f = column_span(b"a\nb", false) == Ok(1);
    let g = column_span(b"\tx", true) == Ok(2);
    let h = column_span(&[0x80; 5], false) == Ok(5);
    let i = tab_advance(0) == 8 && tab_advance(7) == 1 && tab_column(7) == 8;
    a && b && c && d && e && f && g && h && i
}

/// 逐字节宽度面。
fn byte_width_ok() -> bool {
    let a = byte_width(0x80) == 1 && byte_width(0xff) == 1 && byte_width(b'A') == 1;
    let b = byte_width(b'\t') == TAB_STOP;
    let mut v = 0x80u32;
    let mut all_one = true;
    while v <= 0xff {
        if byte_width(v as u8) != 1 {
            all_one = false;
        }
        v += 1;
    }
    let c = column_span(&[0x80; 160], false) == Ok(160);
    let d = column_span(&[0xff; 10], true) == Ok(10);
    a && b && all_one && c && d
}

/// 高位折叠面。
fn high_fold_ok() -> bool {
    let hi = high160();
    let r = match fold(&hi, &opt(80, true, false)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.lines() == 2 && r.max_col() == 80;
    let b = r.bytes() == 162;
    let mut small = [0x80u8; 81];
    let c = match fold(&small, &opt(80, true, false)) {
        Ok(v) => v.lines() == 2,
        Err(_) => false,
    };
    small[80] = 0x81;
    let d = match fold(&small, &opt(80, true, false)) {
        Ok(v) => v.lines() == 2 && v.max_col() == 80,
        Err(_) => false,
    };
    a && b && c && d
}

/// 硬断行面。
fn hard_break_ok() -> bool {
    let a = match fold(b"abcdefghij", &opt(4, true, false)) {
        Ok(v) => v.text() == "abcd\nefgh\nij\n" && v.lines() == 3,
        Err(_) => false,
    };
    let b = match fold(b"abcd", &opt(4, true, false)) {
        Ok(v) => v.text() == "abcd\n" && v.lines() == 1,
        Err(_) => false,
    };
    let c = match fold(b"abc", &opt(80, false, false)) {
        Ok(v) => v.text() == "abc\n" && v.lines() == 1,
        Err(_) => false,
    };
    let d = match fold(b"", &opt(10, false, false)) {
        Ok(v) => v.lines() == 0 && v.bytes() == 0,
        Err(_) => false,
    };
    let e = match fold(b"ab\ncd", &opt(80, false, false)) {
        Ok(v) => v.lines() == 2 && v.text() == "ab\ncd\n",
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 空格断行面。
fn spaces_break_ok() -> bool {
    let a = match fold(b"aaa bbb ccc", &opt(7, true, true)) {
        Ok(v) => v.text() == "aaa\nbbb ccc\n" && v.lines() == 2,
        Err(_) => false,
    };
    let b = match fold(b"aaa bbb ccc", &opt(7, true, false)) {
        Ok(v) => v.text() == "aaa bbb\n ccc\n" && v.lines() == 2,
        Err(_) => false,
    };
    let c = match fold(b"aaaaaaaaaa", &opt(4, true, true)) {
        Ok(v) => v.text() == "aaaa\naaaa\naa\n",
        Err(_) => false,
    };
    let d = match fold(b"ab cd ef", &opt(5, true, true)) {
        Ok(v) => v.lines() == 2,
        Err(_) => false,
    };
    a && b && c && d
}

/// 制表与退格面。
fn tab_ok() -> bool {
    let a = match fold(b"\tab", &opt(80, false, false)) {
        Ok(v) => v.max_col() == 10,
        Err(_) => false,
    };
    let b = match fold(b"\tab", &opt(80, true, false)) {
        Ok(v) => v.max_col() == 3,
        Err(_) => false,
    };
    let c = match fold(b"ab\x08c", &opt(80, false, false)) {
        Ok(v) => v.max_col() == 2,
        Err(_) => false,
    };
    let d = match fold(b"ab\tc", &opt(4, false, false)) {
        Ok(v) => v.lines() == 3,
        Err(_) => false,
    };
    let e = match fold(b"a\rb", &opt(80, false, false)) {
        Ok(v) => v.max_col() == 1,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 编号样式面。
fn nl_style_ok() -> bool {
    let text = b"a\n\nb\n";
    let all = NlOpts { style: Style::All, ..NlOpts::new() };
    let nonempty = NlOpts::new();
    let none = NlOpts { style: Style::None, ..NlOpts::new() };
    let a = nl(text, &all).map(|r| (r.lines(), r.numbered(), r.last())) == Ok((3, 3, 3));
    let b = nl(text, &nonempty).map(|r| (r.lines(), r.numbered(), r.last())) == Ok((3, 2, 2));
    let c = nl(text, &none).map(|r| (r.lines(), r.numbered())) == Ok((3, 0));
    let d = nl(b"", &all).map(|r| r.lines()) == Ok(0);
    let e = Style::from_byte(b'x').err() == Some(NF_ESTYLE);
    let f = Style::All.numbers(true) && !Style::NonEmpty.numbers(true);
    a && b && c && d && e && f
}

/// 编号格式面。
fn nl_format_ok() -> bool {
    let rn = NlOpts::new();
    let ln = NlOpts { format: Format::Left, ..NlOpts::new() };
    let rz = NlOpts { format: Format::Zero, ..NlOpts::new() };
    let a = number_prefix(&rn, 1) == "     1\t";
    let b = number_prefix(&ln, 1) == "1     \t";
    let c = number_prefix(&rz, 1) == "000001\t";
    let d = number_prefix(&rn, 1234567) == "1234567\t";
    let e = Format::from_bytes(b"rz") == Ok(Format::Zero);
    let f = Format::from_bytes(b"zz").err() == Some(NF_EFORMAT);
    let g = rn.text().contains("rn");
    a && b && c && d && e && f && g
}

/// 分隔符与步长面。
fn nl_delim_ok() -> bool {
    let all = NlOpts { style: Style::All, start: 5, increment: 2, ..NlOpts::new() };
    let a = nl(b"x\ny\n", &all).map(|r| r.last()) == Ok(7);
    let text = b"x\n\\:\\:\\:\ny\n";
    let b = nl(text, &all).map(|r| r.last()) == Ok(5);
    let c = nl(text, &all).map(|r| r.lines()) == Ok(3);
    let text2 = b"x\n\\:\\:\ny\n";
    let d = nl(text2, &all).map(|r| r.last()) == Ok(1);
    let e = nl(b"x", &NlOpts { sep: b':', ..NlOpts::new() })
        .map(|r| r.text().starts_with("     1:x")) == Ok(true);
    a && b && c && d && e
}

/// 选项解析面。
fn parse_ok() -> bool {
    let a = parse_fold_args(&[b"-w20", b"-b", b"-s"]).map(|o| (o.width, o.bytes_only, o.break_spaces))
        == Ok((20, true, true));
    let b = parse_fold_args(&[b"-w0"]).err() == Some(NF_EWIDTH);
    let c = parse_fold_args(&[b"-w201"]).err() == Some(NF_EWIDTH);
    let d = parse_fold_args(&[b"-x"]).err() == Some(NF_EOPT);
    let e = parse_fold_args(&[b"-b", b"-b"]).err() == Some(NF_ESTATE);
    let f = parse_nl_args(&[b"-ba", b"-nrz", b"-w4", b"-v10", b"-i2", b"-s:"])
        .map(|o| (o.style, o.format, o.width, o.start, o.increment, o.sep))
        == Ok((Style::All, Format::Zero, 4, 10, 2, b':'));
    let g = parse_nl_args(&[b"-bx"]).err() == Some(NF_ESTYLE);
    let h = parse_nl_args(&[b"-nnx"]).err() == Some(NF_EFORMAT);
    let i = parse_nl_args(&[b"-v0"]).err() == Some(NF_ESTART);
    let j = parse_nl_args(&[b"-w0"]).err() == Some(NF_EWIDTH);
    a && b && c && d && e && f && g && h && i && j
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
    if l2.register(Sample::blank()) != Err(NF_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = column_ok() && byte_width_ok();
    out[1] = high_fold_ok() && hard_break_ok();
    out[2] = spaces_break_ok() && tab_ok();
    out[3] = nl_style_ok() && nl_format_ok() && nl_delim_ok() && parse_ok() && ledger_ok();
    out
}

/// 判据集合。
pub fn run_unx11576_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11576.nl_fold");

    s.add("NF01-逐字节列宽", column_ok(), "static");
    s.add("NF02-字节宽度表", byte_width_ok(), "static");
    s.add("NF03-高位折叠", high_fold_ok(), "static");
    s.add("NF04-硬断行", hard_break_ok(), "static");
    s.add("NF05-空格断行", spaces_break_ok(), "static");
    s.add("NF06-制表与退格", tab_ok(), "static");
    s.add("NF07-编号样式", nl_style_ok(), "static");
    s.add("NF08-编号格式", nl_format_ok(), "static");
    s.add("NF09-分隔与步长", nl_delim_ok(), "static");
    s.add("NF10-选项解析", parse_ok(), "static");
    s.add("NF11-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "NF12-冻结指纹",
        {
            let mut l = build_ledger();
            match l.freeze() {
                Ok(r) => r == l.seal() && l.is_frozen() && l.freeze() == Err(NF_EBAL),
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "NF13-冻结重放四注入",
        {
            let four = inject_four();
            four.get(0) == Some(&true)
                && four.get(1) == Some(&true)
                && four.get(2) == Some(&true)
                && four.get(3) == Some(&true)
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns() {
        assert_eq!(column_span(b"\tx", false), Ok(9));
        assert_eq!(column_span(b"\t\tx", false), Ok(17));
        assert_eq!(column_span(b"abc", false), Ok(3));
        assert_eq!(column_span(b"abc\x08", false), Ok(2));
        assert_eq!(column_span(b"abc\rx", false), Ok(1));
        assert_eq!(column_span(b"\tx", true), Ok(2));
        assert_eq!(column_span(b"", false), Ok(0));
    }

    #[test]
    fn tabs() {
        assert_eq!(tab_advance(0), 8);
        assert_eq!(tab_advance(1), 7);
        assert_eq!(tab_advance(7), 1);
        assert_eq!(tab_advance(8), 8);
        assert_eq!(tab_column(3), 8);
        assert_eq!(tab_column(8), 16);
    }

    #[test]
    fn digits() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(1234567), 7);
    }

    #[test]
    fn fold_plain() {
        let r = fold(b"hello world", &FoldOpts::new()).expect("f");
        assert_eq!(r.text(), "hello world\n");
        assert_eq!(r.lines(), 1);
        assert_eq!(r.bytes(), 12);
        assert_eq!(r.max_col(), 11);
    }

    #[test]
    fn fold_hard() {
        let r = fold(b"abcdefghij", &FoldOpts { width: 4, bytes_only: true, break_spaces: false })
            .expect("f");
        assert_eq!(r.text(), "abcd\nefgh\nij\n");
        assert_eq!(r.lines(), 3);
    }

    #[test]
    fn fold_high_bytes() {
        let hi = [0x80u8; 160];
        let r = fold(&hi, &FoldOpts { width: 80, bytes_only: true, break_spaces: false })
            .expect("f");
        assert_eq!(r.lines(), 2);
        assert_eq!(r.max_col(), 80);
        assert_eq!(r.bytes(), 162);
    }

    #[test]
    fn fold_spaces() {
        let r = fold(b"aaa bbb ccc", &FoldOpts { width: 7, bytes_only: true, break_spaces: true })
            .expect("f");
        assert_eq!(r.text(), "aaa\nbbb ccc\n");
        let r2 = fold(b"aaa bbb ccc", &FoldOpts { width: 7, bytes_only: true, break_spaces: false })
            .expect("f");
        assert_eq!(r2.text(), "aaa bbb\n ccc\n");
    }

    #[test]
    fn fold_empty_and_newlines() {
        assert_eq!(fold(b"", &FoldOpts::new()).map(|r| r.lines()), Ok(0));
        assert_eq!(fold(b"a\nb\nc\n", &FoldOpts::new()).map(|r| r.lines()), Ok(3));
        assert_eq!(
            fold(b"ab\ncd", &FoldOpts::new()).ok().map(|r| r.text() == "ab\ncd\n"),
            Some(true)
        );
    }

    #[test]
    fn fold_width_errors() {
        let zero = FoldOpts { width: 0, bytes_only: false, break_spaces: false };
        assert_eq!(fold(b"abc", &zero).err(), Some(NF_EWIDTH));
        let big = FoldOpts { width: WIDTH_MAX + 1, bytes_only: false, break_spaces: false };
        assert_eq!(fold(b"abc", &big).err(), Some(NF_EWIDTH));
        assert_eq!(fold(&[b'a', 0], &FoldOpts::new()).err(), Some(NF_EBYTE));
    }

    #[test]
    fn numbering_prefixes() {
        assert_eq!(number_prefix(&NlOpts::new(), 1), "     1\t");
        assert_eq!(
            number_prefix(&NlOpts { format: Format::Left, ..NlOpts::new() }, 1),
            "1     \t"
        );
        assert_eq!(
            number_prefix(&NlOpts { format: Format::Zero, ..NlOpts::new() }, 42),
            "000042\t"
        );
        assert_eq!(number_prefix(&NlOpts::new(), 1234567), "1234567\t");
    }

    #[test]
    fn numbering_styles() {
        let text = b"a\n\nb\n";
        let all = NlOpts { style: Style::All, ..NlOpts::new() };
        assert_eq!(nl(text, &all).map(|r| (r.lines(), r.numbered(), r.last())), Ok((3, 3, 3)));
        assert_eq!(nl(text, &NlOpts::new()).map(|r| (r.numbered(), r.last())), Ok((2, 2)));
        let none = NlOpts { style: Style::None, ..NlOpts::new() };
        assert_eq!(nl(text, &none).map(|r| r.numbered()), Ok(0));
    }

    #[test]
    fn numbering_delimiters() {
        let all = NlOpts { style: Style::All, start: 5, increment: 2, ..NlOpts::new() };
        assert_eq!(nl(b"x\ny\n", &all).map(|r| r.last()), Ok(7));
        assert_eq!(nl(b"x\n\\:\\:\\:\ny\n", &all).map(|r| r.last()), Ok(5));
        assert_eq!(nl(b"x\n\\:\\:\ny\n", &all).map(|r| r.last()), Ok(1));
        assert_eq!(nl(b"", &all).map(|r| r.lines()), Ok(0));
    }

    #[test]
    fn numbering_text() {
        let all = NlOpts { style: Style::All, ..NlOpts::new() };
        let r = nl(b"x\ny\n", &all).expect("n");
        assert_eq!(r.text(), "     1\tx\n     2\ty\n");
    }

    #[test]
    fn option_parsing() {
        let f = parse_fold_args(&[b"-w20", b"-b", b"-s"]).expect("f");
        assert_eq!(f.width, 20);
        assert!(f.bytes_only && f.break_spaces);
        let n = parse_nl_args(&[b"-ba", b"-nrz", b"-w4", b"-v10", b"-i2", b"-s:"]).expect("n");
        assert_eq!(n.style, Style::All);
        assert_eq!(n.format, Format::Zero);
        assert_eq!(n.width, 4);
        assert_eq!(n.start, 10);
        assert_eq!(n.increment, 2);
        assert_eq!(n.sep, b':');
    }

    #[test]
    fn option_errors() {
        assert_eq!(parse_fold_args(&[b"-w0"]).err(), Some(NF_EWIDTH));
        assert_eq!(parse_fold_args(&[b"-x"]).err(), Some(NF_EOPT));
        assert_eq!(parse_fold_args(&[b"-b", b"-b"]).err(), Some(NF_ESTATE));
        assert_eq!(parse_nl_args(&[b"-bx"]).err(), Some(NF_ESTYLE));
        assert_eq!(parse_nl_args(&[b"-nnx"]).err(), Some(NF_EFORMAT));
        assert_eq!(parse_nl_args(&[b"-v0"]).err(), Some(NF_ESTART));
        assert_eq!(parse_nl_args(&[b"-i0"]).err(), Some(NF_ESTART));
        assert_eq!(parse_nl_args(&[b"-q"]).err(), Some(NF_EOPT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(9));
        assert_eq!(l.sample(2).map(|s| s.value), Some(2));
        assert_eq!(l.sample(8).map(|s| s.code), Some(NF_EWIDTH));
        assert_eq!(l.sample(13).map(|s| s.code), Some(NF_EBYTE));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(NF_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(NF_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11576_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 13);
    }
}
