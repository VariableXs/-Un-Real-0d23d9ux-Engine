//! UNX-F11570 ls 不可打印字节呈现（? 替换与引号体例）
//!
//! 真实问题：`ls` 面对一个文件名时，首先要决定「哪些字节不能原样落到终端」，
//! 再决定「整个名字要不要加引号、加哪一种引号」。GNU coreutils 把这两件事拆成
//! 两个互相独立的维度：
//!
//! 1. 呈现档（presentation tier）：可打印字节原样显示；控制字节与高位字节要么
//!    被替换成 `?`（`-q` / `--hide-control-chars`，终端默认行为），要么写成
//!    八进制转义 `\ooo`（`-b` / `--escape`），要么按 C 字符串体例写成
//!    `\n`、`\t`、`\ooo`（`--quoting-style=c`）。
//! 2. 引号体例（quoting style）：`literal` 完全不加引号；`shell` 仅在需要时
//!    用单引号包住，名字里的单引号写成 `'\''`；`shell-always` 总是加引号；
//!    `shell-escape` / `shell-escape-always` 在出现不可打印字节时改用
//!    `$'...'`；`c` 用双引号加 C 转义；`escape` 只加反斜杠不加引号。
//!
//! 本单把这两个维度做成独立的纯函数：`quote_name` 负责「字节 → 显示串」，
//! `column_count` / `render_grid` 负责「显示串 → 列布局」，`parse_flags` 负责
//! 「命令行 → 呈现档 + 引号体例 + 宽度」。三者都不读环境变量、不读 locale，
//! 因此与基准机在 C/POSIX locale 下的输出一致。
//!
//! 关键不变量：
//! - 可打印集合严格取 C 的 `isprint`：`0x20..=0x7e`；`0x7f` 与 `0x80..=0xff`
//!   一律视为不可打印，绝不依赖宽字符库。
//! - `escape` 体例是双射：任意单字节经 `quote_name(..., Escape)` 编码后再经
//!   `decode_escapes` 解码必然还原，这条性质由 256 字节全扫描验证。
//! - `shell` 体例的单引号内不允许出现裸单引号，嵌入时必须走 `'\''` 四字节
//!   序列；`$'...'` 内则用 `\'`。
//! - 列布局按「向下填充」的经典 `ls` 语义计算：行数 = ceil(n/cols)，第 c 列
//!   取索引 `c*rows .. c*rows+rows`，列宽取该列最长显示串，列间两空格，行尾
//!   不留空格。
//! - 排序使用 C 序（无符号字节比较），与 `LC_COLLATE` 无关；文件名按原始
//!   字节保存，显示时才转义，因此排序不会因引号体例而变化。
//!
//! 闸门次序：选项字形（`LS_EINVAL`）→ 缺参数（`LS_ESHORT`）→ 宽度/容量越界
//! （`LS_ERANGE`）→ 重复选项（`LS_EDUP`）→ 体例冲突（`LS_ECONF`）→ 名字过长
//! （`LS_ELEN`）→ 封存状态（`LS_ESTATE`）→ 坏转义（`LS_EBYTE`）。账本对账另用
//! `LS_EBAL` / `LS_ESLOT` / `LS_EVALUE` / `LS_ESEAL` / `LS_ENOBS` 五码。
//!
//! 账本语义：十四格 = 八格成功（shell 引号长度、escape 转义长度、C 引号长度、
//! `?` 替换长度、八进制档长度、列数、需要引号的名字个数、最长名宽）+ 六格失败
//! （未知选项、缺宽度、宽度为零、`-b -q` 冲突、重复 `-b`、名字过长）。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze`
//! 后拒绝再登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf4b0;

/// 选项字形不认识。
pub const LS_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字超长 / 空字段。
pub const LS_ELEN: u32 = SEG_BASE + 0x02;
/// 转义字母不认识。
pub const LS_EBYTE: u32 = SEG_BASE + 0x03;
/// 宽度、容量、八进制值越界。
pub const LS_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复选项。
pub const LS_EDUP: u32 = SEG_BASE + 0x05;
/// 缺参数或被截断。
pub const LS_ESHORT: u32 = SEG_BASE + 0x06;
/// 清单已封存。
pub const LS_ESTATE: u32 = SEG_BASE + 0x07;
/// 体例冲突（`-b` 与 `-q`）。
pub const LS_ECONF: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const LS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 成功值与基准表不符。
pub const LS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放指纹不一致。
pub const LS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：引号体例。
pub const OP_QUOTE: u8 = 1;
/// 操作码：`?` 替换。
pub const OP_HIDE: u8 = 2;
/// 操作码：反斜杠转义。
pub const OP_ESC: u8 = 3;
/// 操作码：列布局。
pub const OP_COLS: u8 = 4;
/// 操作码：选项解析。
pub const OP_PARSE: u8 = 5;
/// 操作码：宽度统计。
pub const OP_WIDTH: u8 = 6;
/// 操作码：引号需求计数。
pub const OP_COUNT: u8 = 7;
/// 操作码：名字容量。
pub const OP_NAME: u8 = 8;

/// FNV-1a 偏移基数。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 逐字节折叠（每字节混八次）。
pub fn fold(h: u64, b: u8) -> u64 {
    let mut x = h ^ (b as u64);
    let mut i = 0u32;
    while i < 8 {
        x = x.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

/// 十进制追加。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        if let Some(d) = buf.get_mut(n) {
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        if let Some(d) = buf.get(n) {
            s.push(*d as char);
        }
    }
}

/// 有符号十进制追加。
fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_dec(s, v.unsigned_abs());
    } else {
        push_dec(s, v as u64);
    }
}

/// 两位十六进制。
fn push_hex2(s: &mut String, v: u8) {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    if let Some(h) = HEX.get((v >> 4) as usize) {
        s.push(*h as char);
    }
    if let Some(l) = HEX.get((v & 0x0f) as usize) {
        s.push(*l as char);
    }
}

/// 三位八进制。
fn push_oct3(s: &mut String, v: u8) {
    let hi = (v >> 6) & 0x07;
    let mid = (v >> 3) & 0x07;
    let lo = v & 0x07;
    s.push((b'0' + hi) as char);
    s.push((b'0' + mid) as char);
    s.push((b'0' + lo) as char);
}

/// 非空时补换行。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 类别位：可打印（C isprint）。
pub const K_PRINT: u8 = 1;
/// 类别位：图形（`0x21..=0x7e`）。
pub const K_GRAPH: u8 = 2;
/// 类别位：控制字节。
pub const K_CNTRL: u8 = 4;
/// 类别位：高位字节。
pub const K_HIGH: u8 = 8;
/// 类别位：shell 需要引号的字节。
pub const K_SHELL: u8 = 16;
/// 类别位：有短转义（`\n` 等）。
pub const K_SHORT: u8 = 32;

/// 构造 256 项类别表。
const fn class_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let b = i as u8;
        let mut f = 0u8;
        if b >= 0x20 && b <= 0x7e {
            f |= K_PRINT;
        }
        if b > 0x20 && b <= 0x7e {
            f |= K_GRAPH;
        }
        if b < 0x20 || b == 0x7f {
            f |= K_CNTRL;
        }
        if b >= 0x80 {
            f |= K_HIGH;
        }
        // 空白与 shell 元字符：单引号体例必须包住。
        if b == b' '
            || b == b'\t'
            || b == 0x0a
            || b == 0x0b
            || b == 0x0c
            || b == 0x0d
            || b == b'!'
            || b == b'"'
            || b == b'#'
            || b == b'$'
            || b == b'&'
            || b == b'\''
            || b == b'('
            || b == b')'
            || b == b'*'
            || b == b';'
            || b == b'<'
            || b == b'='
            || b == b'>'
            || b == b'?'
            || b == b'['
            || b == b'\\'
            || b == b']'
            || b == b'^'
            || b == b'`'
            || b == b'{'
            || b == b'|'
            || b == b'}'
            || b == b'~'
            || b >= 0x80
        {
            f |= K_SHELL;
        }
        // 短转义字母。
        if b == 0x07
            || b == 0x08
            || b == 0x09
            || b == 0x0a
            || b == 0x0b
            || b == 0x0c
            || b == 0x0d
            || b == b'\\'
        {
            f |= K_SHORT;
        }
        t[i] = f;
        i += 1;
    }
    t
}

/// 类别表（唯一事实来源）。
pub const CLASS_TABLE: [u8; 256] = class_table();

/// 类别查询。
pub fn class_of(b: u8) -> u8 {
    match CLASS_TABLE.get(b as usize) {
        Some(v) => *v,
        None => 0,
    }
}

/// C isprint。
pub fn is_print(b: u8) -> bool {
    class_of(b) & K_PRINT != 0
}

/// C isgraph。
pub fn is_graph(b: u8) -> bool {
    class_of(b) & K_GRAPH != 0
}

/// 是否控制字节。
pub fn is_cntrl(b: u8) -> bool {
    class_of(b) & K_CNTRL != 0
}

/// 是否高位字节。
pub fn is_high(b: u8) -> bool {
    class_of(b) & K_HIGH != 0
}

/// 短转义字母（不含反斜杠）。
pub fn short_letter(b: u8) -> Option<u8> {
    match b {
        0x07 => Some(b'a'),
        0x08 => Some(b'b'),
        0x09 => Some(b't'),
        0x0a => Some(b'n'),
        0x0b => Some(b'v'),
        0x0c => Some(b'f'),
        0x0d => Some(b'r'),
        _ => None,
    }
}

/// 是否非打印（控制 + 高位）。
pub fn has_nonprint(v: &[u8]) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            if !is_print(*b) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// shell 体例是否需要包引号。
pub fn needs_shell(v: &[u8]) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            if class_of(*b) & K_SHELL != 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 替换档下是否需要引号：只看原始可打印字节里的 shell 元字符。
/// 被替换成 `?` 的控制/高位字节不参与引号判定，否则 `?` 会被误当成通配符。
pub fn needs_shell_display(v: &[u8]) -> bool {
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            if is_print(*b) && class_of(*b) & K_SHELL != 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 需要 shell 引号的名字个数。
pub fn count_needing_shell(names: &[&[u8]]) -> u32 {    let mut n = 0u32;
    let mut i = 0usize;
    while i < names.len() {
        if let Some(v) = names.get(i) {
            if needs_shell(v) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 最长名字的字节长度。
pub fn longest_name(names: &[&[u8]]) -> u32 {
    let mut m = 0u32;
    let mut i = 0usize;
    while i < names.len() {
        if let Some(v) = names.get(i) {
            if v.len() as u32 > m {
                m = v.len() as u32;
            }
        }
        i += 1;
    }
    m
}

/// 引号体例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteStyle {
    /// 原样。
    Literal,
    /// 需要时单引号。
    Shell,
    /// 总是单引号（非打印时 `$'...'`）。
    ShellAlways,
    /// 需要时单引号，非打印时 `$'...'`。
    ShellEscape,
    /// 总是引号，非打印时 `$'...'`。
    ShellEscapeAlways,
    /// C 双引号体例。
    C,
    /// 仅反斜杠转义。
    Escape,
}

impl QuoteStyle {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            QuoteStyle::Literal => "literal",
            QuoteStyle::Shell => "shell",
            QuoteStyle::ShellAlways => "shell-always",
            QuoteStyle::ShellEscape => "shell-escape",
            QuoteStyle::ShellEscapeAlways => "shell-escape-always",
            QuoteStyle::C => "c",
            QuoteStyle::Escape => "escape",
        }
    }

    /// 代号（账本折叠用）。
    pub fn code(&self) -> u8 {
        match self {
            QuoteStyle::Literal => 0,
            QuoteStyle::Shell => 1,
            QuoteStyle::ShellAlways => 2,
            QuoteStyle::ShellEscape => 3,
            QuoteStyle::ShellEscapeAlways => 4,
            QuoteStyle::C => 5,
            QuoteStyle::Escape => 6,
        }
    }

    /// 从 `--quoting-style=` 的取值解析。
    pub fn from_name(v: &[u8]) -> Option<QuoteStyle> {
        if v == b"literal" {
            Some(QuoteStyle::Literal)
        } else if v == b"shell" {
            Some(QuoteStyle::Shell)
        } else if v == b"shell-always" {
            Some(QuoteStyle::ShellAlways)
        } else if v == b"shell-escape" {
            Some(QuoteStyle::ShellEscape)
        } else if v == b"shell-escape-always" {
            Some(QuoteStyle::ShellEscapeAlways)
        } else if v == b"c" {
            Some(QuoteStyle::C)
        } else if v == b"escape" {
            Some(QuoteStyle::Escape)
        } else {
            None
        }
    }
}

/// 引号统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteStats {
    /// 读入字节。
    pub read: usize,
    /// 写出字节。
    pub written: usize,
    /// 被转义的字节数。
    pub escaped: u32,
    /// 是否加了外层引号。
    pub quoted: u8,
}

impl QuoteStats {
    /// 零。
    pub const fn zero() -> QuoteStats {
        QuoteStats { read: 0, written: 0, escaped: 0, quoted: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.read as u64);
        s.push('/');
        push_dec(&mut s, self.written as u64);
        s.push('/');
        push_dec(&mut s, self.escaped as u64);
        s.push('/');
        push_dec(&mut s, self.quoted as u64);
        s
    }
}

/// 写一字节。
fn put(out: &mut [u8], pos: &mut usize, v: u8) -> Result<(), u32> {
    match out.get_mut(*pos) {
        Some(d) => {
            *d = v;
            *pos += 1;
            Ok(())
        }
        None => Err(LS_ESHORT),
    }
}

/// 写 `\ooo`。
fn put_octal(out: &mut [u8], pos: &mut usize, v: u8) -> Result<(), u32> {
    put(out, pos, b'\\')?;
    put(out, pos, b'0' + ((v >> 6) & 0x07))?;
    put(out, pos, b'0' + ((v >> 3) & 0x07))?;
    put(out, pos, b'0' + (v & 0x07))
}

/// 写短转义或八进制（不含外层引号）。
fn put_escaped(out: &mut [u8], pos: &mut usize, b: u8) -> Result<(), u32> {
    if let Some(l) = short_letter(b) {
        put(out, pos, b'\\')?;
        put(out, pos, l)
    } else if b == b'\\' {
        put(out, pos, b'\\')?;
        put(out, pos, b'\\')
    } else if !is_print(b) {
        put_octal(out, pos, b)
    } else {
        put(out, pos, b)
    }
}

/// 写 C 体例的一字节（含 `"` 与 `\` 转义）。
fn put_c_byte(out: &mut [u8], pos: &mut usize, b: u8) -> Result<(), u32> {
    if b == b'"' || b == b'\\' {
        put(out, pos, b'\\')?;
        put(out, pos, b)
    } else {
        put_escaped(out, pos, b)
    }
}

/// 写 `$'...'` 体例的一字节。
fn put_dollar_byte(out: &mut [u8], pos: &mut usize, b: u8) -> Result<(), u32> {
    if b == b'\'' {
        put(out, pos, b'\\')?;
        put(out, pos, b'\'')
    } else {
        put_escaped(out, pos, b)
    }
}

/// 单引号体例：嵌入的 `'` 写成 `'\''`。
fn put_single_quoted(
    v: &[u8],
    out: &mut [u8],
    pos: &mut usize,
    st: &mut QuoteStats,
) -> Result<(), u32> {
    put(out, pos, b'\'')?;
    let mut i = 0usize;
    while i < v.len() {
        let b = match v.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b == b'\'' {
            put(out, pos, b'\'')?;
            put(out, pos, b'\\')?;
            put(out, pos, b'\'')?;
            put(out, pos, b'\'')?;
            st.escaped += 1;
        } else {
            put(out, pos, b)?;
        }
        i += 1;
    }
    put(out, pos, b'\'')
}

/// `$'...'` 体例。
fn put_dollar_quoted(
    v: &[u8],
    out: &mut [u8],
    pos: &mut usize,
    st: &mut QuoteStats,
) -> Result<(), u32> {
    put(out, pos, b'$')?;
    put(out, pos, b'\'')?;
    let mut i = 0usize;
    while i < v.len() {
        let b = match v.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b == b'\'' || b == b'\\' || !is_print(b) {
            st.escaped += 1;
        }
        put_dollar_byte(out, pos, b)?;
        i += 1;
    }
    put(out, pos, b'\'')
}

/// 按体例渲染一个名字。
pub fn quote_name(
    name: &[u8],
    style: QuoteStyle,
    out: &mut [u8],
) -> Result<QuoteStats, u32> {
    let mut st = QuoteStats { read: name.len(), written: 0, escaped: 0, quoted: 0 };
    let mut pos = 0usize;
    match style {
        QuoteStyle::Literal => {
            let mut i = 0usize;
            while i < name.len() {
                if let Some(b) = name.get(i) {
                    put(out, &mut pos, *b)?;
                }
                i += 1;
            }
        }
        QuoteStyle::Shell => {
            if needs_shell(name) {
                st.quoted = 1;
                put_single_quoted(name, out, &mut pos, &mut st)?;
            } else {
                let mut i = 0usize;
                while i < name.len() {
                    if let Some(b) = name.get(i) {
                        put(out, &mut pos, *b)?;
                    }
                    i += 1;
                }
            }
        }
        QuoteStyle::ShellAlways => {
            st.quoted = 1;
            if has_nonprint(name) {
                put_dollar_quoted(name, out, &mut pos, &mut st)?;
            } else {
                put_single_quoted(name, out, &mut pos, &mut st)?;
            }
        }
        QuoteStyle::ShellEscape => {
            if has_nonprint(name) {
                st.quoted = 1;
                put_dollar_quoted(name, out, &mut pos, &mut st)?;
            } else if needs_shell(name) {
                st.quoted = 1;
                put_single_quoted(name, out, &mut pos, &mut st)?;
            } else {
                let mut i = 0usize;
                while i < name.len() {
                    if let Some(b) = name.get(i) {
                        put(out, &mut pos, *b)?;
                    }
                    i += 1;
                }
            }
        }
        QuoteStyle::ShellEscapeAlways => {
            st.quoted = 1;
            if has_nonprint(name) {
                put_dollar_quoted(name, out, &mut pos, &mut st)?;
            } else {
                put_single_quoted(name, out, &mut pos, &mut st)?;
            }
        }
        QuoteStyle::C => {
            st.quoted = 1;
            put(out, &mut pos, b'"')?;
            let mut i = 0usize;
            while i < name.len() {
                let b = match name.get(i) {
                    Some(x) => *x,
                    None => break,
                };
                if b == b'"' || b == b'\\' || !is_print(b) {
                    st.escaped += 1;
                }
                put_c_byte(out, &mut pos, b)?;
                i += 1;
            }
            put(out, &mut pos, b'"')?;
        }
        QuoteStyle::Escape => {
            let mut i = 0usize;
            while i < name.len() {
                let b = match name.get(i) {
                    Some(x) => *x,
                    None => break,
                };
                if b == b' ' {
                    put(out, &mut pos, b'\\')?;
                    put(out, &mut pos, b' ')?;
                    st.escaped += 1;
                } else {
                    if b == b'\\' || !is_print(b) {
                        st.escaped += 1;
                    }
                    put_escaped(out, &mut pos, b)?;
                }
                i += 1;
            }
        }
    }
    st.written = pos;
    Ok(st)
}

/// `?` 替换档：不可打印字节（含高位）一律显示为 `?`。
pub fn hide_control(name: &[u8], out: &mut [u8]) -> Result<QuoteStats, u32> {
    let mut st = QuoteStats { read: name.len(), written: 0, escaped: 0, quoted: 0 };
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < name.len() {
        let b = match name.get(i) {
            Some(x) => *x,
            None => break,
        };
        if is_print(b) {
            put(out, &mut pos, b)?;
        } else {
            put(out, &mut pos, b'?')?;
            st.escaped += 1;
        }
        i += 1;
    }
    st.written = pos;
    Ok(st)
}

/// `-q` 档 + shell 引号：先替换，再按 shell 体例包引号。
pub fn quote_name_hide(name: &[u8], out: &mut [u8]) -> Result<QuoteStats, u32> {
    if name.len() > NAME_CAP {
        return Err(LS_ELEN);
    }
    let mut hid = [0u8; NAME_CAP];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < name.len() {
        let b = match name.get(i) {
            Some(x) => *x,
            None => break,
        };
        let v = if is_print(b) { b } else { b'?' };
        if let Some(d) = hid.get_mut(n) {
            *d = v;
        }
        n += 1;
        i += 1;
    }
    let slice = hid.get(..n).unwrap_or(&[]);
    if needs_shell_display(name) {
        quote_name(slice, QuoteStyle::Shell, out)
    } else {
        let mut st = QuoteStats { read: name.len(), written: 0, escaped: 0, quoted: 0 };
        let mut pos = 0usize;
        let mut k = 0usize;
        while k < n {
            if let Some(b) = slice.get(k) {
                put(out, &mut pos, *b)?;
            }
            if let Some(raw) = name.get(k) {
                if !is_print(*raw) {
                    st.escaped += 1;
                }
            }
            k += 1;
        }
        st.written = pos;
        Ok(st)
    }
}

/// 解码反斜杠转义（`escape` / C 体例的逆）。
pub fn decode_escapes(raw: &[u8], out: &mut [u8]) -> Result<usize, u32> {
    let mut i = 0usize;
    let mut pos = 0usize;
    while i < raw.len() {
        let b = match raw.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b != b'\\' {
            put(out, &mut pos, b)?;
            i += 1;
            continue;
        }
        i += 1;
        let c = match raw.get(i) {
            Some(v) => *v,
            None => return Err(LS_ESHORT),
        };
        i += 1;
        match c {
            b'\\' => put(out, &mut pos, b'\\')?,
            b'\'' => put(out, &mut pos, b'\'')?,
            b' ' => put(out, &mut pos, b' ')?,
            b'a' => put(out, &mut pos, 0x07)?,
            b'b' => put(out, &mut pos, 0x08)?,
            b't' => put(out, &mut pos, 0x09)?,
            b'n' => put(out, &mut pos, 0x0a)?,
            b'v' => put(out, &mut pos, 0x0b)?,
            b'f' => put(out, &mut pos, 0x0c)?,
            b'r' => put(out, &mut pos, 0x0d)?,
            b'x' => {
                let mut v = 0u32;
                let mut n = 0u32;
                while n < 2 {
                    let d = match raw.get(i) {
                        Some(x) => *x,
                        None => break,
                    };
                    let hv = match d {
                        b'0'..=b'9' => (d - b'0') as u32,
                        b'a'..=b'f' => (d - b'a') as u32 + 10,
                        b'A'..=b'F' => (d - b'A') as u32 + 10,
                        _ => break,
                    };
                    v = v * 16 + hv;
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    return Err(LS_EBYTE);
                }
                if v > 0xff {
                    return Err(LS_ERANGE);
                }
                put(out, &mut pos, v as u8)?;
            }
            b'0'..=b'7' => {
                let mut v = (c - b'0') as u32;
                let mut n = 1u32;
                while n < 3 {
                    let d = match raw.get(i) {
                        Some(x) => *x,
                        None => break,
                    };
                    if !(b'0'..=b'7').contains(&d) {
                        break;
                    }
                    v = v * 8 + (d - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                if v > 0xff {
                    return Err(LS_ERANGE);
                }
                put(out, &mut pos, v as u8)?;
            }
            _ => return Err(LS_EBYTE),
        }
    }
    Ok(pos)
}

/// 名字容量。
pub const NAME_CAP: usize = 32;

/// 固定容量名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    b: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 空名。
    pub const fn blank() -> Name {
        Name { b: [0; NAME_CAP], len: 0 }
    }

    /// 从字节切片构造。
    pub fn from_slice(v: &[u8]) -> Result<Name, u32> {
        if v.len() > NAME_CAP {
            return Err(LS_ELEN);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < v.len() {
            if let (Some(d), Some(s)) = (n.b.get_mut(i), v.get(i)) {
                *d = *s;
            }
            i += 1;
        }
        n.len = v.len();
        Ok(n)
    }

    /// 切片。
    pub fn as_slice(&self) -> &[u8] {
        self.b.get(..self.len).unwrap_or(&[])
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 字节。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.b.get(i).copied()
    }

    /// C 序比较（无符号字节，短前缀更小）。
    pub fn compare_c(&self, o: &Name) -> i32 {
        let n = if self.len < o.len { self.len } else { o.len };
        let mut i = 0usize;
        while i < n {
            let a = match self.b.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let b = match o.b.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if a != b {
                if a < b {
                    return -1;
                }
                return 1;
            }
            i += 1;
        }
        if self.len < o.len {
            -1
        } else if self.len > o.len {
            1
        } else {
            0
        }
    }
}

/// 名字表容量。
pub const MAX_NAMES: usize = 18;
/// 最大列数。
pub const MAX_COLS: usize = 8;

/// 固定容量名字表。
#[derive(Clone, Copy, Debug)]
pub struct NameList {
    items: [Name; MAX_NAMES],
    len: usize,
}

impl NameList {
    /// 空表。
    pub const fn new() -> NameList {
        NameList { items: [Name::blank(); MAX_NAMES], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, n: Name) -> Result<(), u32> {
        if self.len >= MAX_NAMES {
            return Err(LS_ERANGE);
        }
        if let Some(d) = self.items.get_mut(self.len) {
            *d = n;
        }
        self.len += 1;
        Ok(())
    }

    /// 追加字节。
    pub fn push_bytes(&mut self, v: &[u8]) -> Result<(), u32> {
        let n = Name::from_slice(v)?;
        self.push(n)
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 项。
    pub fn get(&self, i: usize) -> Option<&Name> {
        if i >= self.len {
            return None;
        }
        self.items.get(i)
    }

    /// 复制第 i 项。
    pub fn copy_at(&self, i: usize) -> Option<Name> {
        self.get(i).copied()
    }

    /// C 序插入排序（稳定）。
    pub fn sort_c(&mut self) {
        let mut i = 1usize;
        while i < self.len {
            let mut j = i;
            while j > 0 {
                let a = match self.items.get(j - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let b = match self.items.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if a.compare_c(&b) > 0 {
                    if let Some(d) = self.items.get_mut(j - 1) {
                        *d = b;
                    }
                    if let Some(d) = self.items.get_mut(j) {
                        *d = a;
                    }
                    j -= 1;
                } else {
                    break;
                }
            }
            i += 1;
        }
    }

    /// 是否已按 C 序。
    pub fn is_sorted_c(&self) -> bool {
        let mut i = 1usize;
        while i < self.len {
            let a = match self.items.get(i - 1) {
                Some(v) => v,
                None => return false,
            };
            let b = match self.items.get(i) {
                Some(v) => v,
                None => return false,
            };
            if a.compare_c(b) > 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 最长名宽。
    pub fn longest(&self) -> usize {
        let mut m = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.items.get(i) {
                if n.len() > m {
                    m = n.len();
                }
            }
            i += 1;
        }
        m
    }
}

/// 显示格容量。
pub const CELL_CAP: usize = 4 * NAME_CAP + 8;

/// 显示格栅（已渲染的列出项）。
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    cells: [[u8; CELL_CAP]; MAX_NAMES],
    lens: [usize; MAX_NAMES],
    len: usize,
}

impl Grid {
    /// 空栅。
    pub const fn new() -> Grid {
        Grid { cells: [[0; CELL_CAP]; MAX_NAMES], lens: [0; MAX_NAMES], len: 0 }
    }

    /// 追加一格。
    pub fn push(&mut self, v: &[u8]) -> Result<(), u32> {
        if self.len >= MAX_NAMES {
            return Err(LS_ERANGE);
        }
        if v.len() > CELL_CAP {
            return Err(LS_ELEN);
        }
        if let Some(cell) = self.cells.get_mut(self.len) {
            let mut i = 0usize;
            while i < v.len() {
                if let (Some(d), Some(s)) = (cell.get_mut(i), v.get(i)) {
                    *d = *s;
                }
                i += 1;
            }
        }
        if let Some(d) = self.lens.get_mut(self.len) {
            *d = v.len();
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

    /// 第 i 格。
    pub fn get(&self, i: usize) -> Option<&[u8]> {
        if i >= self.len {
            return None;
        }
        let n = match self.lens.get(i) {
            Some(v) => *v,
            None => return None,
        };
        match self.cells.get(i) {
            Some(c) => c.get(..n),
            None => None,
        }
    }

    /// 第 i 格长度。
    pub fn width_at(&self, i: usize) -> usize {
        if i >= self.len {
            return 0;
        }
        match self.lens.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 布局统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutStats {
    /// 列数。
    pub cols: usize,
    /// 行数。
    pub rows: usize,
    /// 行数（同 rows，便于文本）。
    pub lines: usize,
    /// 写出字节。
    pub written: usize,
}

impl LayoutStats {
    /// 零。
    pub const fn zero() -> LayoutStats {
        LayoutStats { cols: 0, rows: 0, lines: 0, written: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.cols as u64);
        s.push(' ');
        push_dec(&mut s, self.rows as u64);
        s.push(' ');
        push_dec(&mut s, self.lines as u64);
        s.push(' ');
        push_dec(&mut s, self.written as u64);
        s
    }
}

/// 向下填充的列数。
pub fn column_count(grid: &Grid, width: usize) -> usize {
    let n = grid.len();
    if n == 0 {
        return 0;
    }
    if width == 0 {
        return 1;
    }
    let mut cols = if n < MAX_COLS { n } else { MAX_COLS };
    while cols >= 1 {
        let rows = (n + cols - 1) / cols;
        let mut total = 0usize;
        let mut c = 0usize;
        while c < cols {
            let start = c * rows;
            if start >= n {
                break;
            }
            let end = if start + rows < n { start + rows } else { n };
            let mut m = 0usize;
            let mut i = start;
            while i < end {
                let w = grid.width_at(i);
                if w > m {
                    m = w;
                }
                i += 1;
            }
            if c > 0 {
                total += 2;
            }
            total += m;
            c += 1;
        }
        if total <= width {
            return cols;
        }
        if cols == 1 {
            break;
        }
        cols -= 1;
    }
    1
}

/// 计算每列宽度。
fn column_widths(grid: &Grid, cols: usize, rows: usize) -> [usize; MAX_COLS] {
    let mut w = [0usize; MAX_COLS];
    let mut c = 0usize;
    while c < cols {
        let start = c * rows;
        if start >= grid.len() {
            break;
        }
        let end = if start + rows < grid.len() { start + rows } else { grid.len() };
        let mut m = 0usize;
        let mut i = start;
        while i < end {
            let x = grid.width_at(i);
            if x > m {
                m = x;
            }
            i += 1;
        }
        if let Some(d) = w.get_mut(c) {
            *d = m;
        }
        c += 1;
    }
    w
}

/// 渲染列布局。
pub fn render_grid(grid: &Grid, width: usize, out: &mut [u8]) -> Result<LayoutStats, u32> {
    let n = grid.len();
    if n == 0 {
        return Ok(LayoutStats::zero());
    }
    let cols = column_count(grid, width);
    if cols == 0 {
        return Ok(LayoutStats::zero());
    }
    let rows = (n + cols - 1) / cols;
    let widths = column_widths(grid, cols, rows);
    let mut st = LayoutStats { cols, rows, lines: 0, written: 0 };
    let mut r = 0usize;
    while r < rows {
        let mut last = 0usize;
        let mut c = 0usize;
        while c < cols {
            if c * rows + r < n {
                last = c;
            }
            c += 1;
        }
        c = 0;
        while c < cols {
            let idx = c * rows + r;
            if idx < n {
                let cell = match grid.get(idx) {
                    Some(v) => v,
                    None => &[],
                };
                let mut k = 0usize;
                while k < cell.len() {
                    if let Some(b) = cell.get(k) {
                        put(out, &mut st.written, *b)?;
                    }
                    k += 1;
                }
                if c < last {
                    let pad = match widths.get(c) {
                        Some(w) => *w - cell.len() + 2,
                        None => 2,
                    };
                    let mut p = 0usize;
                    while p < pad {
                        put(out, &mut st.written, b' ')?;
                        p += 1;
                    }
                }
            }
            c += 1;
        }
        put(out, &mut st.written, b'\n')?;
        st.lines += 1;
        r += 1;
    }
    Ok(st)
}

/// 渲染单列。
pub fn render_one_per_line(grid: &Grid, out: &mut [u8]) -> Result<LayoutStats, u32> {
    let n = grid.len();
    if n == 0 {
        return Ok(LayoutStats::zero());
    }
    let mut st = LayoutStats { cols: 1, rows: n, lines: 0, written: 0 };
    let mut i = 0usize;
    while i < n {
        if let Some(cell) = grid.get(i) {
            let mut k = 0usize;
            while k < cell.len() {
                if let Some(b) = cell.get(k) {
                    put(out, &mut st.written, *b)?;
                }
                k += 1;
            }
        }
        put(out, &mut st.written, b'\n')?;
        st.lines += 1;
        i += 1;
    }
    Ok(st)
}

/// 清单渲染统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderStats {
    /// 条目数。
    pub entries: usize,
    /// 列数。
    pub cols: usize,
    /// 行数。
    pub lines: usize,
    /// 写出字节。
    pub written: usize,
}

impl RenderStats {
    /// 零。
    pub const fn zero() -> RenderStats {
        RenderStats { entries: 0, cols: 0, lines: 0, written: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.entries as u64);
        s.push(' ');
        push_dec(&mut s, self.cols as u64);
        s.push(' ');
        push_dec(&mut s, self.lines as u64);
        s.push(' ');
        push_dec(&mut s, self.written as u64);
        s
    }
}

/// 选项集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags {
    /// 引号体例。
    pub style: QuoteStyle,
    /// `?` 替换档。
    pub hide: bool,
    /// 横向填充（保留）。
    pub across: bool,
    /// 单列。
    pub one_per_line: bool,
    /// 含点文件。
    pub almost_all: bool,
    /// 追加类型指示（保留）。
    pub classify: bool,
    /// 宽度。
    pub width: usize,
}

impl Flags {
    /// 默认：shell-escape、宽 80。
    pub const fn plain() -> Flags {
        Flags {
            style: QuoteStyle::ShellEscape,
            hide: false,
            across: false,
            one_per_line: false,
            almost_all: false,
            classify: false,
            width: 80,
        }
    }
}

/// 参数表容量。
pub const MAX_ARGS: usize = 16;

/// 按空格切参数。
pub fn split_args<'a>(text: &'a [u8], args: &mut [&'a [u8]; MAX_ARGS]) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        while i < text.len() {
            match text.get(i) {
                Some(b) => {
                    if *b == b' ' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        if i >= text.len() {
            break;
        }
        let start = i;
        while i < text.len() {
            match text.get(i) {
                Some(b) => {
                    if *b == b' ' {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if n >= MAX_ARGS {
            return Err(LS_ERANGE);
        }
        if let Some(d) = args.get_mut(n) {
            *d = text.get(start..i).unwrap_or(&[]);
        }
        n += 1;
    }
    Ok(n)
}

/// 解析宽度。
fn parse_width(v: &[u8]) -> Result<usize, u32> {
    if v.is_empty() {
        return Err(LS_ESHORT);
    }
    let mut w = 0usize;
    let mut i = 0usize;
    while i < v.len() {
        let b = match v.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b < b'0' || b > b'9' {
            return Err(LS_EINVAL);
        }
        w = w * 10 + (b - b'0') as usize;
        if w > 4096 {
            return Err(LS_ERANGE);
        }
        i += 1;
    }
    if w == 0 {
        return Err(LS_ERANGE);
    }
    Ok(w)
}

const S_B: u32 = 1;
const S_Q: u32 = 2;
const S_CAP: u32 = 4;
const S_ONE: u32 = 8;
const S_AA: u32 = 16;
const S_ALL: u32 = 32;
const S_F: u32 = 64;
const S_W: u32 = 128;
const S_STYLE: u32 = 256;

/// 解析选项串（空格分隔）。
pub fn parse_flags(text: &[u8]) -> Result<Flags, u32> {
    let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
    let n = split_args(text, &mut args)?;
    let mut f = Flags::plain();
    let mut seen = 0u32;
    let mut b_flag = false;
    let mut c_flag = false;
    let mut hide = false;
    let mut i = 0usize;
    while i < n {
        let tok = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        i += 1;
        if tok == b"--" {
            break;
        }
        if tok.len() >= 2 && tok.get(0) == Some(&b'-') && tok.get(1) == Some(&b'-') {
            if tok == b"--escape" {
                if seen & S_B != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_B;
                b_flag = true;
                f.style = QuoteStyle::Escape;
            } else if tok == b"--hide-control-chars" {
                if seen & S_Q != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_Q;
                hide = true;
            } else if tok == b"--show-control-chars" {
                hide = false;
            } else if tok == b"--almost-all" {
                if seen & S_AA != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_AA;
                f.almost_all = true;
            } else if tok == b"--all" {
                if seen & S_ALL != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_ALL;
                f.almost_all = true;
            } else if tok == b"--classify" {
                if seen & S_F != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_F;
                f.classify = true;
            } else if tok == b"--width" {
                if seen & S_W != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_W;
                if i >= n {
                    return Err(LS_ESHORT);
                }
                let v = match args.get(i) {
                    Some(x) => *x,
                    None => return Err(LS_ESHORT),
                };
                i += 1;
                f.width = parse_width(v)?;
            } else if tok.len() > 8 && tok.get(..8) == Some(b"--width=".as_slice()) {
                if seen & S_W != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_W;
                f.width = parse_width(tok.get(8..).unwrap_or(&[]))?;
            } else if tok.len() > 16 && tok.get(..16) == Some(b"--quoting-style=".as_slice()) {
                if seen & S_STYLE != 0 {
                    return Err(LS_EDUP);
                }
                seen |= S_STYLE;
                let name = tok.get(16..).unwrap_or(&[]);
                match QuoteStyle::from_name(name) {
                    Some(s) => {
                        f.style = s;
                        if s == QuoteStyle::Escape {
                            b_flag = true;
                        }
                        if s == QuoteStyle::C {
                            c_flag = true;
                        }
                    }
                    None => return Err(LS_EINVAL),
                }
            } else {
                return Err(LS_EINVAL);
            }
            continue;
        }
        if tok.len() >= 2 && tok.get(0) == Some(&b'-') {
            let mut k = 1usize;
            while k < tok.len() {
                let c = match tok.get(k) {
                    Some(v) => *v,
                    None => break,
                };
                match c {
                    b'b' => {
                        if seen & S_B != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_B;
                        b_flag = true;
                        f.style = QuoteStyle::Escape;
                        k += 1;
                    }
                    b'q' => {
                        if seen & S_Q != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_Q;
                        hide = true;
                        k += 1;
                    }
                    b'Q' => {
                        if seen & S_CAP != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_CAP;
                        c_flag = true;
                        f.style = QuoteStyle::C;
                        k += 1;
                    }
                    b'1' => {
                        if seen & S_ONE != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_ONE;
                        f.one_per_line = true;
                        k += 1;
                    }
                    b'a' => {
                        if seen & S_ALL != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_ALL;
                        f.almost_all = true;
                        k += 1;
                    }
                    b'A' => {
                        if seen & S_AA != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_AA;
                        f.almost_all = true;
                        k += 1;
                    }
                    b'F' => {
                        if seen & S_F != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_F;
                        f.classify = true;
                        k += 1;
                    }
                    b'w' => {
                        if seen & S_W != 0 {
                            return Err(LS_EDUP);
                        }
                        seen |= S_W;
                        let rest = tok.get(k + 1..).unwrap_or(&[]);
                        if !rest.is_empty() {
                            f.width = parse_width(rest)?;
                            k = tok.len();
                        } else {
                            if i >= n {
                                return Err(LS_ESHORT);
                            }
                            let v = match args.get(i) {
                                Some(x) => *x,
                                None => return Err(LS_ESHORT),
                            };
                            i += 1;
                            f.width = parse_width(v)?;
                            k = tok.len();
                        }
                    }
                    _ => return Err(LS_EINVAL),
                }
            }
            continue;
        }
        return Err(LS_EINVAL);
    }
    if b_flag && c_flag {
        return Err(LS_ECONF);
    }
    if hide {
        if f.style == QuoteStyle::Escape {
            return Err(LS_ECONF);
        }
        f.hide = true;
    }
    Ok(f)
}

/// 清单（名字 + 选项）。
#[derive(Clone, Copy, Debug)]
pub struct Listing {
    list: NameList,
    flags: Flags,
    sealed: bool,
    root: u64,
}

impl Listing {
    /// 空清单。
    pub const fn new(flags: Flags) -> Listing {
        Listing { list: NameList::new(), flags, sealed: false, root: 0 }
    }

    /// 追加名字。
    pub fn push(&mut self, v: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_ESTATE);
        }
        self.list.push_bytes(v)
    }

    /// 追加已有名字。
    pub fn push_name(&mut self, n: Name) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_ESTATE);
        }
        self.list.push(n)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 选项。
    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// 名字表。
    pub fn names(&self) -> &NameList {
        &self.list
    }

    /// C 序排序。
    pub fn sort(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_ESTATE);
        }
        self.list.sort_c();
        Ok(())
    }

    /// 封存并返回指纹。
    pub fn seal(&mut self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.list.len() {
            if let Some(n) = self.list.get(i) {
                let mut k = 0usize;
                while k < n.len() {
                    if let Some(b) = n.at(k) {
                        h = fold(h, b);
                    }
                    k += 1;
                }
                h = fold(h, 0xff);
            }
            i += 1;
        }
        h = fold(h, self.flags.style.code());
        h = fold(h, self.flags.hide as u8);
        h = fold(h, self.flags.one_per_line as u8);
        h = fold(h, (self.flags.width & 0xff) as u8);
        self.root = h;
        self.sealed = true;
        h
    }

    /// 指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染到字节缓冲。
    pub fn render(&self, out: &mut [u8]) -> Result<RenderStats, u32> {
        if self.sealed {
            return Err(LS_ESTATE);
        }
        if self.list.is_empty() {
            return Ok(RenderStats::zero());
        }
        let mut grid = Grid::new();
        let mut i = 0usize;
        while i < self.list.len() {
            let n = match self.list.get(i) {
                Some(v) => v,
                None => break,
            };
            let mut cell = [0u8; CELL_CAP];
            let q = if self.flags.hide {
                quote_name_hide(n.as_slice(), &mut cell)?
            } else {
                quote_name(n.as_slice(), self.flags.style, &mut cell)?
            };
            let slice = cell.get(..q.written).unwrap_or(&[]);
            grid.push(slice)?;
            i += 1;
        }
        let st = if self.flags.one_per_line {
            render_one_per_line(&grid, out)?
        } else {
            render_grid(&grid, self.flags.width, out)?
        };
        Ok(RenderStats {
            entries: self.list.len(),
            cols: st.cols,
            lines: st.lines,
            written: st.written,
        })
    }

    /// 渲染为字符串。
    pub fn render_text(&self) -> Result<String, u32> {
        let mut buf = [0u8; 4096];
        let st = self.render(&mut buf)?;
        let mut s = String::new();
        let mut i = 0usize;
        while i < st.written {
            if let Some(b) = buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        Ok(s)
    }
}

/// 转义表文本。
pub fn escape_table_text() -> String {
    let mut s = String::new();
    s.push_str("n ");
    push_hex2(&mut s, 0x0a);
    s.push_str(" t ");
    push_hex2(&mut s, 0x09);
    s.push_str(" r ");
    push_hex2(&mut s, 0x0d);
    s.push_str(" a ");
    push_hex2(&mut s, 0x07);
    s.push_str(" b ");
    push_hex2(&mut s, 0x08);
    s.push_str(" f ");
    push_hex2(&mut s, 0x0c);
    s.push_str(" v ");
    push_hex2(&mut s, 0x0b);
    s.push_str(" oct3 3 hex2 2");
    s
}

/// 引号体例矩阵（每行：体例名 + 样本的渲染长度）。
pub fn quote_matrix_text() -> String {
    let mut s = String::new();
    let samples: [&[u8]; 4] = [b"plain", b"a b", b"a\tb", b"a\"b"];
    let styles: [QuoteStyle; 7] = [
        QuoteStyle::Literal,
        QuoteStyle::Shell,
        QuoteStyle::ShellAlways,
        QuoteStyle::ShellEscape,
        QuoteStyle::ShellEscapeAlways,
        QuoteStyle::C,
        QuoteStyle::Escape,
    ];
    let mut i = 0usize;
    while i < styles.len() {
        if let Some(st) = styles.get(i) {
            s.push_str(st.name());
            let mut j = 0usize;
            while j < samples.len() {
                let mut buf = [0u8; 256];
                let q = match samples.get(j) {
                    Some(v) => quote_name(v, *st, &mut buf),
                    None => Ok(QuoteStats::zero()),
                };
                match q {
                    Ok(v) => {
                        s.push(' ');
                        push_dec(&mut s, v.written as u64);
                    }
                    Err(_) => {
                        s.push(' ');
                        s.push('x');
                    }
                }
                j += 1;
            }
            s.push('\n');
        }
        i += 1;
    }
    s
}

/// 类别总数（可打印字节 95）。
pub fn printable_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < 256 {
        if is_print(i as u8) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 子串查找（纯字节）。
pub fn slice_contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut same = true;
        let mut k = 0usize;
        while k < needle.len() {
            match (hay.get(i + k), needle.get(k)) {
                (Some(a), Some(b)) => {
                    if *a != *b {
                        same = false;
                    }
                }
                _ => same = false,
            }
            k += 1;
        }
        if same {
            return true;
        }
        i += 1;
    }
    false
}

/// 建立显示格栅。
fn grid_of(names: &[&[u8]]) -> Result<Grid, u32> {
    let mut g = Grid::new();
    let mut i = 0usize;
    while i < names.len() {
        let v = match names.get(i) {
            Some(x) => *x,
            None => break,
        };
        g.push(v)?;
        i += 1;
    }
    Ok(g)
}

/// 探针：引号渲染长度。
fn probe_quote(name: &[u8], style: QuoteStyle) -> Result<u32, u32> {
    let mut out = [0u8; 256];
    match quote_name(name, style, &mut out) {
        Ok(st) => Ok(st.written as u32),
        Err(c) => Err(c),
    }
}

/// 探针：`?` 替换长度。
fn probe_hide(name: &[u8]) -> Result<usize, u32> {
    let mut out = [0u8; 256];
    match hide_control(name, &mut out) {
        Ok(st) => Ok(st.written),
        Err(c) => Err(c),
    }
}

/// 探针：列数。
fn probe_cols(names: &[&[u8]], width: usize) -> Result<usize, u32> {
    let g = grid_of(names)?;
    Ok(column_count(&g, width))
}

/// 探针：需要 shell 引号的名字个数。
fn probe_count(names: &[&[u8]]) -> Result<u32, u32> {
    Ok(count_needing_shell(names))
}

/// 探针：最长名宽。
fn probe_width(names: &[&[u8]]) -> Result<u32, u32> {
    Ok(longest_name(names))
}

/// 探针：选项解析（成功时返回宽度，失败返回错误码）。
fn probe_flags(text: &[u8]) -> Result<u32, u32> {
    match parse_flags(text) {
        Ok(f) => Ok(f.width as u32),
        Err(c) => Err(c),
    }
}

/// 探针：名字容量。
fn probe_name(v: &[u8]) -> Result<u32, u32> {
    match Name::from_slice(v) {
        Ok(n) => Ok(n.len() as u32),
        Err(c) => Err(c),
    }
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
            return Err(LS_ESLOT);
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
            return Err(LS_EBAL);
        }
        const VALS: [u32; 8] = [5, 4, 6, 3, 4, 2, 2, 4];
        const CODES: [u32; 6] =
            [LS_EINVAL, LS_ESHORT, LS_ERANGE, LS_ECONF, LS_EDUP, LS_ELEN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LS_EBAL),
            };
            if s.slot as usize != i {
                return Err(LS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LS_EVALUE);
                        }
                    }
                    None => return Err(LS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LS_EVALUE);
                        }
                    }
                    None => return Err(LS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LS_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u8);
                h = fold(h, s.op);
                h = fold(h, s.ok as u8);
                h = fold(h, (s.code & 0xff) as u8);
                h = fold(h, ((s.code >> 8) & 0xff) as u8);
                h = fold(h, (s.value & 0xff) as u8);
                h = fold(h, ((s.value >> 8) & 0xff) as u8);
                h = fold(h, ((s.value >> 16) & 0xff) as u8);
                h = fold(h, ((s.value >> 24) & 0xff) as u8);
            }
            i += 1;
        }
        fold(h, self.rejects as u8)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LS_EBAL);
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
            Err(LS_ESEAL)
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

/// 从领域操作算出十四格。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_QUOTE, probe_quote(b"a b", QuoteStyle::Shell)));

    let _ = l.register(word_cell(1, OP_ESC, probe_quote(b"a\tb", QuoteStyle::Escape)));

    let _ = l.register(word_cell(2, OP_QUOTE, probe_quote(b"a\"b", QuoteStyle::C)));

    let _ = l.register(count_cell(3, OP_HIDE, probe_hide(b"a\x01b")));

    let _ = l.register(word_cell(4, OP_ESC, probe_quote(b"\x80", QuoteStyle::Escape)));

    let cols: [&[u8]; 5] = [b"a", b"bb", b"ccc", b"dddd", b"eeeee"];
    let _ = l.register(count_cell(5, OP_COLS, probe_cols(&cols, 12)));

    let mixed: [&[u8]; 4] = [b"plain", b"a b", b"x\ty", b"ok"];
    let _ = l.register(word_cell(6, OP_COUNT, probe_count(&mixed)));

    let widths: [&[u8]; 3] = [b"ab", b"abcd", b"a"];
    let _ = l.register(word_cell(7, OP_WIDTH, probe_width(&widths)));

    let _ = l.register(word_cell(8, OP_PARSE, probe_flags(b"-z")));

    let _ = l.register(word_cell(9, OP_PARSE, probe_flags(b"-w")));

    let _ = l.register(word_cell(10, OP_PARSE, probe_flags(b"-w 0")));

    let _ = l.register(word_cell(11, OP_PARSE, probe_flags(b"-b -q")));

    let _ = l.register(word_cell(12, OP_PARSE, probe_flags(b"-b -b")));

    let _ = l.register(word_cell(13, OP_NAME, probe_name(&[b'a'; NAME_CAP + 1])));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let mut buf = [0u8; 32];
        match quote_name(b"it's", QuoteStyle::Shell, &mut buf) {
            Ok(st) => {
                const WANT: [u8; 9] =
                    [b'\'', b'i', b't', b'\'', b'\\', b'\'', b'\'', b's', b'\''];
                st.written == 9
                    && st.quoted == 1
                    && st.escaped == 1
                    && buf.get(..9) == Some(WANT.as_slice())
            }
            Err(_) => false,
        }
    };

    out[1] = {
        let mut ok = true;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let one = [b];
            let mut enc = [0u8; 8];
            match quote_name(&one, QuoteStyle::Escape, &mut enc) {
                Ok(st) => {
                    let mut back = [0u8; 8];
                    match decode_escapes(enc.get(..st.written).unwrap_or(&[]), &mut back) {
                        Ok(n) => {
                            if n != 1 || back.first() != Some(&b) {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
            let mut cbuf = [0u8; 8];
            match quote_name(&one, QuoteStyle::C, &mut cbuf) {
                Ok(st) => {
                    let mut k = 0usize;
                    while k < st.written {
                        match cbuf.get(k) {
                            Some(v) => {
                                if !is_print(*v) {
                                    ok = false;
                                }
                            }
                            None => ok = false,
                        }
                        k += 1;
                    }
                }
                Err(_) => ok = false,
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
        let names: [&[u8]; 4] = [b"alpha", b"a b", b"x\x01y", b"delta"];
        let g = grid_of(&names);
        match g {
            Ok(grid) => {
                let mut buf = [0u8; 512];
                let dense = match render_grid(&grid, 80, &mut buf) {
                    Ok(st) => {
                        st.cols == 4
                            && st.lines == 1
                            && slice_contains(buf.get(..st.written).unwrap_or(&[]), b"alpha")
                            && slice_contains(buf.get(..st.written).unwrap_or(&[]), b"delta")
                    }
                    Err(_) => false,
                };
                let narrow = match render_grid(&grid, 6, &mut buf) {
                    Ok(st) => column_count(&grid, 6) == 1 && st.lines == 4,
                    Err(_) => false,
                };
                let mut hbuf = [0u8; 8];
                let hide = match hide_control(b"x\x80y", &mut hbuf) {
                    Ok(st) => {
                        st.written == 3
                            && hbuf.get(0) == Some(&b'x')
                            && hbuf.get(1) == Some(&b'?')
                            && hbuf.get(2) == Some(&b'y')
                    }
                    Err(_) => false,
                };
                let mono = column_count(&grid, 5) <= column_count(&grid, 20);
                dense && narrow && hide && mono
            }
            Err(_) => false,
        }
    };

    out
}

/// 十三项检查。
pub fn run_unx11570_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11570.ls");

    s.add(
        "LS01-引号体例",
        {
            let mut b1 = [0u8; 64];
            let a = match quote_name(b"a b", QuoteStyle::Shell, &mut b1) {
                Ok(st) => {
                    st.written == 5 && st.quoted == 1 && b1.get(..5) == Some(b"'a b'".as_slice())
                }
                Err(_) => false,
            };
            let mut b2 = [0u8; 64];
            let b = match quote_name(b"ok", QuoteStyle::ShellAlways, &mut b2) {
                Ok(st) => st.written == 4 && b2.get(..4) == Some(b"'ok'".as_slice()),
                Err(_) => false,
            };
            let mut b3 = [0u8; 64];
            let c = match quote_name(b"a b", QuoteStyle::Literal, &mut b3) {
                Ok(st) => st.written == 3 && !needs_shell(b"a b") == false && b3.get(..3) == Some(b"a b".as_slice()),
                Err(_) => false,
            };
            let mut b4 = [0u8; 64];
            let d = match quote_name(b"a\"b", QuoteStyle::C, &mut b4) {
                Ok(st) => {
                    st.written == 6 && b4.get(..6) == Some(b"\"a\\\"b\"".as_slice())
                }
                Err(_) => false,
            };
            let mut b5 = [0u8; 64];
            let e = match quote_name(b"it's", QuoteStyle::Shell, &mut b5) {
                Ok(st) => st.written == 9 && st.escaped == 1,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "LS02-问号替换",
        {
            let mut b1 = [0u8; 32];
            let a = match hide_control(b"a\x01b", &mut b1) {
                Ok(st) => {
                    st.written == 3
                        && st.escaped == 1
                        && b1.get(..3) == Some(b"a?b".as_slice())
                }
                Err(_) => false,
            };
            let mut b2 = [0u8; 32];
            let b = match hide_control(b"\x80", &mut b2) {
                Ok(st) => st.written == 1 && b2.get(0) == Some(&b'?'),
                Err(_) => false,
            };
            let mut b3 = [0u8; 32];
            let c = match hide_control(b"abc", &mut b3) {
                Ok(st) => st.written == 3 && st.escaped == 0,
                Err(_) => false,
            };
            let mut b4 = [0u8; 32];
            let d = match hide_control(b"\x7f", &mut b4) {
                Ok(st) => st.written == 1 && b4.get(0) == Some(&b'?'),
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "LS03-八进制档",
        {
            let mut b1 = [0u8; 32];
            let a = match quote_name(b"\x80", QuoteStyle::Escape, &mut b1) {
                Ok(st) => st.written == 4 && b1.get(..4) == Some(b"\\200".as_slice()),
                Err(_) => false,
            };
            let mut b2 = [0u8; 32];
            let b = match quote_name(b"\x1f", QuoteStyle::Escape, &mut b2) {
                Ok(st) => st.written == 4 && b2.get(..4) == Some(b"\\037".as_slice()),
                Err(_) => false,
            };
            let mut b3 = [0u8; 32];
            let c = match quote_name(b"a\tb", QuoteStyle::Escape, &mut b3) {
                Ok(st) => st.written == 4 && b3.get(..4) == Some(b"a\\tb".as_slice()),
                Err(_) => false,
            };
            let mut b4 = [0u8; 32];
            let d = match decode_escapes(b"\\200", &mut b4) {
                Ok(n) => n == 1 && b4.get(0) == Some(&0x80),
                Err(_) => false,
            };
            let mut b5 = [0u8; 32];
            let e = match quote_name(b"a b", QuoteStyle::Escape, &mut b5) {
                Ok(st) => st.written == 4 && b5.get(..4) == Some(b"a\\ b".as_slice()),
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "LS04-引号需求",
        {
            let a = !needs_shell(b"abc-1.txt") && needs_shell(b"a b");
            let b = needs_shell(b"$x") && needs_shell(b"a*b") && needs_shell(b"x\ty");
            let c = needs_shell(b"\x80") && !needs_shell(b"plain_1");
            let set: [&[u8]; 4] = [b"plain", b"a b", b"x\ty", b"ok"];
            let d = count_needing_shell(&set) == 2;
            let e = printable_total() == 95 && is_print(b' ') && !is_print(0x7f);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "LS05-转义往返",
        {
            let mut ok = true;
            let mut i = 0u32;
            while i < 256 {
                let b = i as u8;
                let one = [b];
                let mut enc = [0u8; 8];
                match quote_name(&one, QuoteStyle::Escape, &mut enc) {
                    Ok(st) => {
                        let mut back = [0u8; 8];
                        match decode_escapes(enc.get(..st.written).unwrap_or(&[]), &mut back) {
                            Ok(n) => {
                                if n != 1 || back.first() != Some(&b) {
                                    ok = false;
                                }
                            }
                            Err(_) => ok = false,
                        }
                    }
                    Err(_) => ok = false,
                }
                i += 1;
            }
            let mut b1 = [0u8; 8];
            let a = match decode_escapes(b"\\n", &mut b1) {
                Ok(n) => n == 1 && b1.get(0) == Some(&0x0a),
                Err(_) => false,
            };
            let mut b2 = [0u8; 8];
            let b = match decode_escapes(b"\\101", &mut b2) {
                Ok(n) => n == 1 && b2.get(0) == Some(&b'A'),
                Err(_) => false,
            };
            let mut b3 = [0u8; 8];
            let c = match decode_escapes(b"\\x41", &mut b3) {
                Ok(n) => n == 1 && b3.get(0) == Some(&b'A'),
                Err(_) => false,
            };
            let d = decode_escapes(b"\\q", &mut b1) == Err(LS_EBYTE);
            let e = decode_escapes(b"\\", &mut b1) == Err(LS_ESHORT);
            let f = decode_escapes(b"\\400", &mut b1) == Err(LS_ERANGE);
            let g = decode_escapes(b"\\x", &mut b1) == Err(LS_EBYTE);
            ok && a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "LS06-列布局",
        {
            let names: [&[u8]; 5] = [b"a", b"bb", b"ccc", b"dddd", b"eeeee"];
            let g = match grid_of(&names) {
                Ok(v) => v,
                Err(_) => Grid::new(),
            };
            let a = column_count(&g, 12) == 2 && column_count(&g, 4) == 1;
            let mut buf = [0u8; 128];
            let b = match render_grid(&g, 12, &mut buf) {
                Ok(st) => {
                    st.cols == 2
                        && st.rows == 3
                        && st.lines == 3
                        && buf.get(..st.written)
                            == Some(b"a    dddd\nbb   eeeee\nccc\n".as_slice())
                }
                Err(_) => false,
            };
            let mut buf2 = [0u8; 128];
            let c = match render_one_per_line(&g, &mut buf2) {
                Ok(st) => st.lines == 5 && buf2.get(..6) == Some(b"a\nbb\nc".as_slice()),
                Err(_) => false,
            };
            let d = column_count(&g, 80) == 5;
            let mut buf3 = [0u8; 128];
            let e = match render_grid(&g, 12, &mut buf3) {
                Ok(st) => match buf3.get(st.written - 1) {
                    Some(v) => *v == b'\n',
                    None => false,
                },
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "LS07-C序排序",
        {
            let mut l = NameList::new();
            let seq: [&[u8]; 4] = [b"b", b"a", b"B", b"aa"];
            let mut i = 0usize;
            while i < seq.len() {
                if let Some(v) = seq.get(i) {
                    let _ = l.push_bytes(v);
                }
                i += 1;
            }
            let pre = !l.is_sorted_c();
            l.sort_c();
            let a = l.is_sorted_c() && l.len() == 4;
            let want: [&[u8]; 4] = [b"B", b"a", b"aa", b"b"];
            let mut ok = true;
            let mut k = 0usize;
            while k < want.len() {
                match (l.get(k), want.get(k)) {
                    (Some(n), Some(w)) => {
                        if n.as_slice() != *w {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                k += 1;
            }
            let mut big = NameList::new();
            let _ = big.push_bytes(b"abc");
            let _ = big.push_bytes(b"ab");
            let _ = big.push_bytes(b"abc");
            big.sort_c();
            let stable = match (big.get(0), big.get(1), big.get(2)) {
                (Some(x), Some(y), Some(z)) => {
                    x.as_slice() == b"ab"
                        && y.as_slice() == b"abc"
                        && z.as_slice() == b"abc"
                }
                _ => false,
            };
            pre && a && ok && stable
        },
        "static",
    );

    s.add(
        "LS08-选项解析",
        {
            let a = match parse_flags(b"") {
                Ok(f) => {
                    f.style == QuoteStyle::ShellEscape
                        && !f.hide
                        && !f.one_per_line
                        && f.width == 80
                }
                Err(_) => false,
            };
            let b = match parse_flags(b"-b") {
                Ok(f) => f.style == QuoteStyle::Escape && !f.hide,
                Err(_) => false,
            };
            let c = match parse_flags(b"-q") {
                Ok(f) => f.hide && f.style != QuoteStyle::Escape,
                Err(_) => false,
            };
            let d = match parse_flags(b"-1 -w 40") {
                Ok(f) => f.one_per_line && f.width == 40,
                Err(_) => false,
            };
            let e = match parse_flags(b"--quoting-style=c") {
                Ok(f) => f.style == QuoteStyle::C,
                Err(_) => false,
            };
            let f = parse_flags(b"-bq") == Err(LS_ECONF);
            let g = parse_flags(b"-z") == Err(LS_EINVAL);
            let h = parse_flags(b"-b -b") == Err(LS_EDUP);
            let i = parse_flags(b"-w") == Err(LS_ESHORT);
            let j = parse_flags(b"-w 4097") == Err(LS_ERANGE);
            let k = match parse_flags(b"-bQ") {
                Ok(_) => false,
                Err(c) => c == LS_ECONF,
            };
            a && b && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "LS09-容量闸门",
        {
            let ok32 = [b'a'; NAME_CAP];
            let bad33 = [b'a'; NAME_CAP + 1];
            let a = match Name::from_slice(&ok32) {
                Ok(n) => n.len() == NAME_CAP && n.at(0) == Some(b'a'),
                Err(_) => false,
            };
            let b = Name::from_slice(&bad33) == Err(LS_ELEN);
            let mut l = NameList::new();
            let mut i = 0usize;
            while i < MAX_NAMES {
                let _ = l.push_bytes(b"n");
                i += 1;
            }
            let c = l.len() == MAX_NAMES && l.push_bytes(b"n") == Err(LS_ERANGE);
            let mut g = Grid::new();
            let cell = [b'z'; CELL_CAP];
            let d = g.push(&cell).is_ok();
            let e = g.push(b"ok").is_ok() && g.len() == 2 && g.width_at(0) == CELL_CAP;
            let big = [b'z'; CELL_CAP + 1];
            let f = g.push(&big) == Err(LS_ELEN);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "LS10-清单渲染",
        {
            let mut l = Listing::new(Flags {
                style: QuoteStyle::Shell,
                hide: false,
                across: false,
                one_per_line: false,
                almost_all: false,
                classify: false,
                width: 80,
            });
            let _ = l.push(b"b");
            let _ = l.push(b"a b");
            let _ = l.push(b"c");
            let sorted = l.sort().is_ok();
            let text = l.render_text();
            let a = match text {
                Ok(v) => v == "'a b'  b  c\n",
                Err(_) => false,
            };
            let mut buf = [0u8; 128];
            let b = match l.render(&mut buf) {
                Ok(st) => st.entries == 3 && st.lines == 1 && st.written == 12,
                Err(_) => false,
            };
            let root = l.seal();
            let c = l.is_sealed() && l.root() == root;
            let d = l.render(&mut buf) == Err(LS_ESTATE);
            let e = l.push(b"d") == Err(LS_ESTATE);
            let mut one = Listing::new(Flags {
                style: QuoteStyle::Literal,
                hide: false,
                across: false,
                one_per_line: true,
                almost_all: false,
                classify: false,
                width: 80,
            });
            let _ = one.push(b"x");
            let _ = one.push(b"y");
            let f = match one.render_text() {
                Ok(v) => v == "x\ny\n",
                Err(_) => false,
            };
            sorted && a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "LS11-替换加引号",
        {
            let mut b1 = [0u8; 64];
            let a = match quote_name_hide(b"a b\x01", &mut b1) {
                Ok(st) => st.written == 6 && b1.get(..6) == Some(b"'a b?'".as_slice()),
                Err(_) => false,
            };
            let mut b2 = [0u8; 64];
            let b = match quote_name_hide(b"a\x80b", &mut b2) {
                Ok(st) => st.written == 3 && b2.get(..3) == Some(b"a?b".as_slice()),
                Err(_) => false,
            };
            let mut b3 = [0u8; 64];
            let c = match quote_name_hide(b"plain", &mut b3) {
                Ok(st) => st.written == 5 && st.escaped == 0,
                Err(_) => false,
            };
            let d = quote_name_hide(&[b'a'; NAME_CAP + 1], &mut b3) == Err(LS_ELEN);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "LS12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = match l.sample(0) {
                Some(v) => v.ok && v.value == 5 && v.op == OP_QUOTE,
                None => false,
            };
            let e = match l.sample(3) {
                Some(v) => v.ok && v.value == 3 && v.op == OP_HIDE,
                None => false,
            };
            let f = match l.sample(10) {
                Some(v) => !v.ok && v.code == LS_ERANGE,
                None => false,
            };
            let g = l.replay() == Ok(l.seal());
            let h = l.sample(SAMPLES).is_none() && !l.is_empty();
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "LS13-冻结重放四注入",
        {
            let four = inject_four();
            let a = four[0] && four[1] && four[2] && four[3];
            let mut l = build_ledger();
            let before = l.seal();
            let rej0 = l.rejects();
            let frozen = l.freeze();
            let b = match frozen {
                Ok(v) => {
                    v == before && v == l.root() && l.seal() == v && l.replay() == Ok(v)
                }
                Err(_) => false,
            };
            let again = l.freeze();
            let c = again == Err(LS_EBAL) && l.rejects() == rej0 + 1;
            let extra = l.register(Sample::blank());
            let d = extra == Err(LS_ESLOT);
            let e = l.root() == before;
            a && b && c && d && e
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_shell() {
        let mut buf = [0u8; 64];
        let st = quote_name(b"a b", QuoteStyle::Shell, &mut buf).expect("q");
        assert_eq!(st.written, 5);
        assert_eq!(&buf[..5], b"'a b'");
        let st2 = quote_name(b"plain", QuoteStyle::Shell, &mut buf).expect("q");
        assert_eq!(st2.written, 5);
        assert_eq!(&buf[..5], b"plain");
        let st3 = quote_name(b"it's", QuoteStyle::Shell, &mut buf).expect("q");
        assert_eq!(st3.written, 9);
        assert_eq!(&buf[..9], b"'it'\\''s'");
    }

    #[test]
    fn quote_styles() {
        let mut buf = [0u8; 64];
        quote_name(b"ok", QuoteStyle::ShellAlways, &mut buf).expect("q");
        assert_eq!(&buf[..4], b"'ok'");
        quote_name(b"ok", QuoteStyle::ShellEscapeAlways, &mut buf).expect("q");
        assert_eq!(&buf[..4], b"'ok'");
        quote_name(b"a\x01b", QuoteStyle::ShellEscape, &mut buf).expect("q");
        assert_eq!(&buf[..9], b"$'a\\001b'");
        quote_name(b"a b", QuoteStyle::Literal, &mut buf).expect("q");
        assert_eq!(&buf[..3], b"a b");
    }

    #[test]
    fn c_style() {
        let mut buf = [0u8; 64];
        let st = quote_name(b"a\tb", QuoteStyle::C, &mut buf).expect("q");
        assert_eq!(st.written, 6);
        assert_eq!(&buf[..6], b"\"a\\tb\"");
        let st2 = quote_name(b"a\"b", QuoteStyle::C, &mut buf).expect("q");
        assert_eq!(st2.written, 6);
        assert_eq!(&buf[..6], b"\"a\\\"b\"");
    }

    #[test]
    fn hide_tier() {
        let mut buf = [0u8; 32];
        let st = hide_control(b"a\x80b", &mut buf).expect("h");
        assert_eq!(st.written, 3);
        assert_eq!(&buf[..3], b"a?b");
        let st2 = hide_control(b"a b", &mut buf).expect("h");
        assert_eq!(st2.written, 3);
        assert_eq!(&buf[..3], b"a b");
    }

    #[test]
    fn octal_tier() {
        let mut buf = [0u8; 32];
        quote_name(b"\x80", QuoteStyle::Escape, &mut buf).expect("q");
        assert_eq!(&buf[..4], b"\\200");
        quote_name(b"\x1f", QuoteStyle::Escape, &mut buf).expect("q");
        assert_eq!(&buf[..4], b"\\037");
        let mut dec = [0u8; 8];
        assert_eq!(decode_escapes(b"\\200", &mut dec).expect("d"), 1);
        assert_eq!(dec[0], 0x80);
    }

    #[test]
    fn escapes_roundtrip() {
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let one = [b];
            let mut enc = [0u8; 8];
            let st = quote_name(&one, QuoteStyle::Escape, &mut enc).expect("q");
            let mut dec = [0u8; 8];
            let n = decode_escapes(&enc[..st.written], &mut dec).expect("d");
            assert_eq!(n, 1);
            assert_eq!(dec[0], b);
            i += 1;
        }
    }

    #[test]
    fn escape_errors() {
        let mut buf = [0u8; 8];
        assert_eq!(decode_escapes(b"\\q", &mut buf), Err(LS_EBYTE));
        assert_eq!(decode_escapes(b"\\", &mut buf), Err(LS_ESHORT));
        assert_eq!(decode_escapes(b"\\400", &mut buf), Err(LS_ERANGE));
        assert_eq!(decode_escapes(b"\\x", &mut buf), Err(LS_EBYTE));
        assert_eq!(decode_escapes(b"\\x41", &mut buf), Ok(1));
    }

    #[test]
    fn flags_basic() {
        let f = parse_flags(b"-b").expect("f");
        assert_eq!(f.style, QuoteStyle::Escape);
        let g = parse_flags(b"-1 -w 40").expect("f");
        assert!(g.one_per_line);
        assert_eq!(g.width, 40);
        let h = parse_flags(b"--quoting-style=shell").expect("f");
        assert_eq!(h.style, QuoteStyle::Shell);
        let i = parse_flags(b"-q").expect("f");
        assert!(i.hide);
    }

    #[test]
    fn flags_errors() {
        assert_eq!(parse_flags(b"-z"), Err(LS_EINVAL));
        assert_eq!(parse_flags(b"-w"), Err(LS_ESHORT));
        assert_eq!(parse_flags(b"-w 0"), Err(LS_ERANGE));
        assert_eq!(parse_flags(b"-b -q"), Err(LS_ECONF));
        assert_eq!(parse_flags(b"-b -b"), Err(LS_EDUP));
        assert_eq!(parse_flags(b"-bQ"), Err(LS_ECONF));
    }

    #[test]
    fn name_capacity() {
        let ok = [b'a'; NAME_CAP];
        assert_eq!(Name::from_slice(&ok).expect("n").len(), NAME_CAP);
        let bad = [b'a'; NAME_CAP + 1];
        assert_eq!(Name::from_slice(&bad), Err(LS_ELEN));
        let mut l = NameList::new();
        let mut i = 0usize;
        while i < MAX_NAMES {
            l.push_bytes(b"n").expect("p");
            i += 1;
        }
        assert_eq!(l.push_bytes(b"n"), Err(LS_ERANGE));
    }

    #[test]
    fn sort_c_order() {
        let mut l = NameList::new();
        l.push_bytes(b"b").expect("p");
        l.push_bytes(b"a").expect("p");
        l.push_bytes(b"B").expect("p");
        l.push_bytes(b"aa").expect("p");
        assert!(!l.is_sorted_c());
        l.sort_c();
        assert!(l.is_sorted_c());
        assert_eq!(l.get(0).expect("0").as_slice(), b"B");
        assert_eq!(l.get(1).expect("1").as_slice(), b"a");
        assert_eq!(l.get(2).expect("2").as_slice(), b"aa");
        assert_eq!(l.get(3).expect("3").as_slice(), b"b");
    }

    #[test]
    fn columns() {
        let names: [&[u8]; 5] = [b"a", b"bb", b"ccc", b"dddd", b"eeeee"];
        let g = grid_of(&names).expect("g");
        assert_eq!(column_count(&g, 12), 2);
        assert_eq!(column_count(&g, 4), 1);
        assert_eq!(column_count(&g, 80), 5);
    }

    #[test]
    fn grid_render() {
        let names: [&[u8]; 5] = [b"a", b"bb", b"ccc", b"dddd", b"eeeee"];
        let g = grid_of(&names).expect("g");
        let mut buf = [0u8; 128];
        let st = render_grid(&g, 12, &mut buf).expect("r");
        assert_eq!(st.lines, 3);
        assert_eq!(&buf[..st.written], b"a    dddd\nbb   eeeee\nccc\n");
        let mut buf2 = [0u8; 128];
        let st2 = render_one_per_line(&g, &mut buf2).expect("r");
        assert_eq!(st2.lines, 5);
    }

    #[test]
    fn listing_render() {
        let mut l = Listing::new(Flags {
            style: QuoteStyle::Shell,
            hide: false,
            across: false,
            one_per_line: false,
            almost_all: false,
            classify: false,
            width: 80,
        });
        l.push(b"b").expect("p");
        l.push(b"a b").expect("p");
        l.push(b"c").expect("p");
        l.sort().expect("s");
        assert_eq!(l.render_text().expect("t"), "'a b'  b  c\n");
        l.seal();
        assert_eq!(l.render_text(), Err(LS_ESTATE));
    }

    #[test]
    fn hide_and_quote() {
        let mut buf = [0u8; 64];
        let st = quote_name_hide(b"a b\x01", &mut buf).expect("q");
        assert_eq!(st.written, 6);
        assert_eq!(&buf[..6], b"'a b?'");
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(0).expect("0").value, 5);
        assert_eq!(l.sample(10).expect("a").code, LS_ERANGE);
    }

    #[test]
    fn seal_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("f");
        assert_eq!(root, l.seal());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(LS_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(LS_ESLOT));
    }

    #[test]
    fn injection_four() {
        let f = inject_four();
        assert!(f[0]);
        assert!(f[1]);
        assert!(f[2]);
        assert!(f[3]);
    }

    #[test]
    fn checks_all_pass() {
        assert!(run_unx11570_checks().all_passed());
    }
}
