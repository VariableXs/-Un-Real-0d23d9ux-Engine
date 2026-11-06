//! UNX-F11568 wc -m≡-c 与词界 isspace 判据
//!
//! `wc -m` 数「字符」，`wc -c` 数「字节」。在 UTF-8 终端上这两个数字经常不同，
//! 于是有人以为 `-m` 是「数码点」。**在 C locale 里这个理解是错的**：`LC_CTYPE=C`
//! 时每个字节自成一「字符」，`mbtowc` 对 0x80..0xff 一律返回单字节，`-m` 与 `-c`
//! 必须逐输入全等。本单把这条等式当成硬判据（`WC04`），并且故意把「按 UTF-8 解
//! 码得到的码点数」也算出来放在旁边对照——两者在纯 ASCII 上相同、在高位字节上
//! 分叉，正是「C locale 禁止 UTF-8 解码」的实证。
//!
//! 第二根支柱是词界。`wc -w` 的分词规则只有一条：**C `isspace()` 为真的字节是
//! 分隔符，其余都不是**。这条规则与「人类直觉」的差距很大：`don't` 是一个词，
//! `a-b` 是一个词，`x.y` 是一个词，`a,b` 也是一个词；只有 `0x20 0x09 0x0a 0x0b
//! 0x0c 0x0d` 六个字节会切断词。本单的 `WC02` 拿 32 个「常被误当分隔符」的候选
//! 逐个验证，`WC03` 再拿 `don't stop`、`a-b c_d` 这类真实串验证跨度。
//!
//! 计数状态机只有一遍扫描：遇到 `\n` 结行并结算最长行；遇到空白结算当前词；
//! 其余字节延长当前词并累加显示宽度。显示宽度按 C locale 定义：`\t` 跳到下一个
//! 8 的倍数，控制字节宽度 0，其余宽度 1。`-L` 的最长行因此可能与「字节数最多的
//! 行」不同——本单 `WC05` 用含制表符的行把这两种口径分开验证。
//!
//! 词跨度（`Span`）是第二语义轴的载体：`WC07` 不仅数词数，还把每个跨度切回原文
//! 比对，并要求 `sum(len) + 空白数 == 非空字节数`。这样「数对了但位置错了」的
//! 实现无法蒙混。跨度定容 64，超出给 `WC_ERANGE`，不会静默丢词。
//!
//! UTF-8 解码器只服务于「对照」与「错误分类」，不参与计数主路径：
//! `0xff` 是非法首字节（`WC_EBYTE`），`0xe9` 结尾是截断（`WC_ESHORT`），
//! `0xc0 0x80` 是过长编码（`WC_ERANGE`）。这三类错误各自有独立出口，`WC06`
//! 与账本的三个失败格一一对应。
//!
//! 闸门次序：标志（`WC_EINVAL`）→ 重复标志（`WC_EDUP`）→ 冲突
//! （`WC_ECONF`，`-m` 与 `--utf8` 同时给）→ 输入长度（`WC_ELEN`）→ 跨度容量
//! （`WC_ERANGE`）→ 解码截断（`WC_ESHORT`）→ 非法字节（`WC_EBYTE`）→ 封存状态
//! （`WC_ESTATE`）。前四道在 `parse_flags`/`scan` 入口，解码三道在 `utf8_count`，
//! 状态在 `Counter::run` 的封存判定。
//!
//! 账本语义：十四格 = 八格成功（行数、词数、字节数、`-m≡-c` 标志、空白类基数、
//! 最长行、UTF-8 码点数、跨度数）+ 六格失败（空标志、超长输入、非法字节、
//! 过长编码、截断编码、标志冲突）。`audit` 认「八值六码」，`seal` 做 FNV-1a
//! 折叠，`replay` 重建比对，`freeze` 后拒绝再登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf490;

/// 标志或参数非法。
pub const WC_EINVAL: u32 = SEG_BASE + 0x01;
/// 输入超出定容。
pub const WC_ELEN: u32 = SEG_BASE + 0x02;
/// 非法字节。
pub const WC_EBYTE: u32 = SEG_BASE + 0x03;
/// 跨度容量或编码范围越界。
pub const WC_ERANGE: u32 = SEG_BASE + 0x04;
/// 编码被截断。
pub const WC_ESHORT: u32 = SEG_BASE + 0x05;
/// 标志冲突。
pub const WC_ECONF: u32 = SEG_BASE + 0x06;
/// 重复标志。
pub const WC_EDUP: u32 = SEG_BASE + 0x07;
/// 已封存后继续使用。
pub const WC_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const WC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const WC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const WC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const WC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const WC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 输入定容。
pub const IN_CAP: usize = 4096;

/// 词跨度定容。
pub const WORDS_CAP: usize = 64;

/// 制表位宽。
pub const TAB_STOP: u32 = 8;

/// 标志：行。
pub const F_LINES: u8 = 1 << 0;
/// 标志：词。
pub const F_WORDS: u8 = 1 << 1;
/// 标志：字节。
pub const F_BYTES: u8 = 1 << 2;
/// 标志：字符（C locale 下等价字节）。
pub const F_CHARS: u8 = 1 << 3;
/// 标志：最长行。
pub const F_MAXLINE: u8 = 1 << 4;
/// 标志：UTF-8 对照解码。
pub const F_UTF8: u8 = 1 << 5;
/// 全部标志。
pub const F_ALL: u8 = F_LINES | F_WORDS | F_BYTES | F_CHARS | F_MAXLINE | F_UTF8;

/// C isspace 的六个成员。
pub const SPACE_BYTES: [u8; 6] = [0x20, 0x09, 0x0a, 0x0b, 0x0c, 0x0d];

/// 操作码：扫描。
pub const OP_SCAN: u8 = 0x01;
/// 操作码：空格类。
pub const OP_SPACE: u8 = 0x02;
/// 操作码：字符等价。
pub const OP_MC: u8 = 0x03;
/// 操作码：最长行。
pub const OP_MAX: u8 = 0x04;
/// 操作码：解码。
pub const OP_UTF8: u8 = 0x05;
/// 操作码：跨度。
pub const OP_SPAN: u8 = 0x06;
/// 操作码：标志。
pub const OP_FLAG: u8 = 0x07;
/// 操作码：行数。
pub const OP_LINE: u8 = 0x08;

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
    if let Some(c) = DIG.get((b >> 4) as usize) {
        s.push(*c as char);
    }
    if let Some(c) = DIG.get((b & 0x0f) as usize) {
        s.push(*c as char);
    }
}

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// C isspace：只认六个字节。
pub fn is_space_c(b: u8) -> bool {
    let mut i = 0usize;
    while i < SPACE_BYTES.len() {
        if let Some(v) = SPACE_BYTES.get(i) {
            if *v == b {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 空白类基数（应为 6）。
pub fn space_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < 256 {
        if is_space_c(i as u8) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 空白集文本。
pub fn space_text() -> String {
    let mut s = String::from("space");
    let mut i = 0usize;
    while i < SPACE_BYTES.len() {
        s.push(' ');
        if let Some(v) = SPACE_BYTES.get(i) {
            push_hex2(&mut s, *v);
        }
        i += 1;
    }
    s
}

/// 单个字节的显示宽度（C locale 口径）。
pub fn width_of(b: u8, col: u32) -> u32 {
    if b == 0x09 {
        let next = (col / TAB_STOP + 1).saturating_mul(TAB_STOP);
        return next.saturating_sub(col);
    }
    if b < 0x20 || b == 0x7f {
        return 0;
    }
    1
}

/// 一行的显示宽度。
pub fn line_width(line: &[u8]) -> u32 {
    let mut w = 0u32;
    let mut i = 0usize;
    while i < line.len() {
        if let Some(b) = line.get(i) {
            w = w.saturating_add(width_of(*b, w));
        }
        i += 1;
    }
    w
}

/// 词跨度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起始字节。
    pub start: u32,
    /// 长度。
    pub len: u32,
}

impl Span {
    /// 空。
    pub const fn blank() -> Span {
        Span { start: 0, len: 0 }
    }

    /// 末尾（不含）。
    pub fn end(&self) -> u32 {
        self.start.saturating_add(self.len)
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("@");
        push_dec(&mut s, self.start as u64);
        s.push('+');
        push_dec(&mut s, self.len as u64);
        s
    }
}

/// 计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    /// 行数。
    pub lines: u32,
    /// 词数。
    pub words: u32,
    /// 字符数（C locale = 字节数）。
    pub chars: u32,
    /// 字节数。
    pub bytes: u32,
    /// 最长行显示宽度。
    pub max_line: u32,
}

impl Counts {
    /// 零。
    pub const fn zero() -> Counts {
        Counts { lines: 0, words: 0, chars: 0, bytes: 0, max_line: 0 }
    }

    /// `-m ≡ -c`。
    pub fn m_eq_c(&self) -> bool {
        self.chars == self.bytes
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.lines as u64);
        s.push(' ');
        push_dec(&mut s, self.words as u64);
        s.push(' ');
        push_dec(&mut s, self.chars as u64);
        s.push(' ');
        push_dec(&mut s, self.bytes as u64);
        s.push(' ');
        push_dec(&mut s, self.max_line as u64);
        s
    }
}

/// 扫描结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scan {
    /// 计数。
    pub counts: Counts,
    /// 跨度表。
    pub spans: [Span; WORDS_CAP],
    /// 跨度数。
    pub nspans: usize,
    /// 被切断的次数（空白字符数）。
    pub breaks: u32,
}

impl Scan {
    /// 空。
    pub const fn blank() -> Scan {
        Scan { counts: Counts::zero(), spans: [Span::blank(); WORDS_CAP], nspans: 0, breaks: 0 }
    }

    /// 第 i 个跨度。
    pub fn span(&self, i: usize) -> Option<Span> {
        if i >= self.nspans {
            return None;
        }
        self.spans.get(i).copied()
    }

    /// 跨度覆盖的字节数。
    pub fn covered(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.nspans {
            if let Some(s) = self.spans.get(i) {
                n = n.saturating_add(s.len);
            }
            i += 1;
        }
        n
    }

    /// 跨度文本行。
    pub fn span_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.nspans {
            if i > 0 {
                s.push(' ');
            }
            if let Some(sp) = self.spans.get(i) {
                s.push_str(&sp.text());
            }
            i += 1;
        }
        s
    }
}

/// 扫描一遍。
///
/// 状态机：`\n` 结行、空白结词、其余延长词并累加宽度。
pub fn scan(inp: &[u8], span_cap: usize) -> Result<Scan, u32> {
    if inp.len() > IN_CAP {
        return Err(WC_ELEN);
    }
    let cap = if span_cap > WORDS_CAP { WORDS_CAP } else { span_cap };
    let mut s = Scan::blank();
    s.counts.bytes = inp.len() as u32;
    s.counts.chars = s.counts.bytes;
    let mut in_word = false;
    let mut start = 0usize;
    let mut line_w = 0u32;
    let mut i = 0usize;
    while i < inp.len() {
        let b = match inp.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b == 0x0a {
            if in_word {
                let len = i.saturating_sub(start);
                let sp = Span { start: start as u32, len: len as u32 };
                match s.spans.get_mut(s.nspans) {
                    Some(d) => {
                        if s.nspans < cap {
                            *d = sp;
                            s.nspans += 1;
                            s.counts.words = s.counts.words.saturating_add(1);
                        } else {
                            return Err(WC_ERANGE);
                        }
                    }
                    None => return Err(WC_ERANGE),
                }
                in_word = false;
            }
            s.counts.lines = s.counts.lines.saturating_add(1);
            if line_w > s.counts.max_line {
                s.counts.max_line = line_w;
            }
            line_w = 0;
            i += 1;
            continue;
        }
        if is_space_c(b) {
            s.breaks = s.breaks.saturating_add(1);
            line_w = line_w.saturating_add(width_of(b, line_w));
            if in_word {
                let len = i.saturating_sub(start);
                let sp = Span { start: start as u32, len: len as u32 };
                if s.nspans < cap {
                    match s.spans.get_mut(s.nspans) {
                        Some(d) => *d = sp,
                        None => return Err(WC_ERANGE),
                    }
                    s.nspans += 1;
                    s.counts.words = s.counts.words.saturating_add(1);
                } else {
                    return Err(WC_ERANGE);
                }
                in_word = false;
            }
            i += 1;
            continue;
        }
        if !in_word {
            start = i;
            in_word = true;
        }
        line_w = line_w.saturating_add(width_of(b, line_w));
        i += 1;
    }
    if in_word {
        let len = inp.len().saturating_sub(start);
        let sp = Span { start: start as u32, len: len as u32 };
        if s.nspans < cap {
            match s.spans.get_mut(s.nspans) {
                Some(d) => *d = sp,
                None => return Err(WC_ERANGE),
            }
            s.nspans += 1;
            s.counts.words = s.counts.words.saturating_add(1);
        } else {
            return Err(WC_ERANGE);
        }
    }
    if line_w > s.counts.max_line {
        s.counts.max_line = line_w;
    }
    Ok(s)
}

/// 取跨度原文。
pub fn span_slice<'a>(inp: &'a [u8], sp: Span) -> Option<&'a [u8]> {
    let a = sp.start as usize;
    let b = sp.end() as usize;
    if a > b || b > inp.len() {
        return None;
    }
    inp.get(a..b)
}

/// 判断某字节是否为词分隔符。
pub fn splits_word(b: u8) -> bool {
    is_space_c(b)
}

/// 词界探针表：32 个候选字节的判定。
pub fn boundary_text() -> String {
    const CAND: [u8; 32] = [
        b' ', 0x09, 0x0a, 0x0b, 0x0c, 0x0d, b'\'', b'"', b'-', b'_', b'.', b',', b':', b';', b'!',
        b'?', b'/', b'\\', b'|', b'@', b'+', b'=', b'*', b'&', b'%', b'$', b'#', b'~', b'^', b'`',
        b'(', b')',
    ];
    let mut s = String::new();
    newline(&mut s);
    s.push_str("BYTE SPLIT");
    let mut i = 0usize;
    while i < CAND.len() {
        if let Some(b) = CAND.get(i) {
            newline(&mut s);
            push_hex2(&mut s, *b);
            s.push(' ');
            if splits_word(*b) {
                s.push('1');
            } else {
                s.push('0');
            }
        }
        i += 1;
    }
    s
}

/// 词界探针：返回 `(分隔, 不分隔)` 的计数。
pub fn boundary_counts() -> (u32, u32) {
    let mut yes = 0u32;
    let mut no = 0u32;
    let mut i = 0u32;
    while i < 256 {
        if splits_word(i as u8) {
            yes += 1;
        } else {
            no += 1;
        }
        i += 1;
    }
    (yes, no)
}

/// UTF-8 码点计数（对照轴，不参与主计数）。
pub fn utf8_count(inp: &[u8]) -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < inp.len() {
        let b = match inp.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b < 0x80 {
            n += 1;
            i += 1;
            continue;
        }
        let need: usize;
        let min: u32;
        if b >= 0xc2 && b <= 0xdf {
            need = 1;
            min = 0x80;
        } else if b >= 0xe0 && b <= 0xef {
            need = 2;
            min = 0x800;
        } else if b >= 0xf0 && b <= 0xf4 {
            need = 3;
            min = 0x10000;
        } else if b == 0xc0 || b == 0xc1 {
            return Err(WC_ERANGE);
        } else {
            return Err(WC_EBYTE);
        }
        if i.saturating_add(need) >= inp.len() {
            return Err(WC_ESHORT);
        }
        let lead_mask = 0x7fu32 >> need;
        let mut cp = (b as u32) & lead_mask;
        let mut k = 1usize;
        while k <= need {
            let c = match inp.get(i + k) {
                Some(v) => *v,
                None => return Err(WC_ESHORT),
            };
            if c < 0x80 || c > 0xbf {
                return Err(WC_EBYTE);
            }
            cp = (cp << 6) | ((c as u32) & 0x3f);
            k += 1;
        }
        if cp < min {
            return Err(WC_ERANGE);
        }
        if cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff) {
            return Err(WC_ERANGE);
        }
        n += 1;
        i = i.saturating_add(need).saturating_add(1);
    }
    Ok(n)
}

/// 解析标志串。
pub fn parse_flags(args: &[&[u8]]) -> Result<u8, u32> {
    if args.is_empty() {
        return Err(WC_EINVAL);
    }
    let mut bits = 0u8;
    let mut utf8 = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        i += 1;
        if a == b"--utf8" {
            utf8 = true;
            continue;
        }
        let mut set: u8 = 0;
        if a == b"--lines" {
            set = F_LINES;
        } else if a == b"--words" {
            set = F_WORDS;
        } else if a == b"--bytes" {
            set = F_BYTES;
        } else if a == b"--chars" {
            set = F_CHARS;
        } else if a == b"--max-line-length" {
            set = F_MAXLINE;
        } else if a.len() > 1 && a.first() == Some(&b'-') {
            let mut k = 1usize;
            while k < a.len() {
                let c = match a.get(k) {
                    Some(v) => *v,
                    None => break,
                };
                let bit = match c {
                    b'l' => F_LINES,
                    b'w' => F_WORDS,
                    b'c' => F_BYTES,
                    b'm' => F_CHARS,
                    b'L' => F_MAXLINE,
                    _ => return Err(WC_EINVAL),
                };
                if set & bit != 0 {
                    return Err(WC_EDUP);
                }
                set |= bit;
                k += 1;
            }
        } else {
            return Err(WC_EINVAL);
        }
        if set == 0 {
            return Err(WC_EINVAL);
        }
        if bits & set != 0 {
            return Err(WC_EDUP);
        }
        bits |= set;
    }
    if utf8 {
        if bits & F_CHARS != 0 {
            return Err(WC_ECONF);
        }
        bits |= F_UTF8;
    }
    Ok(bits)
}

/// 标志渲染。
pub fn flags_text(bits: u8) -> String {
    let mut s = String::new();
    let pairs: [(u8, &str); 6] = [
        (F_LINES, "l"),
        (F_WORDS, "w"),
        (F_BYTES, "c"),
        (F_CHARS, "m"),
        (F_MAXLINE, "L"),
        (F_UTF8, "utf8"),
    ];
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some((b, t)) = pairs.get(i) {
            if bits & *b != 0 {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(t);
            }
        }
        i += 1;
    }
    s
}

/// 计数器：一次扫描 + 可选 UTF-8 对照。
#[derive(Clone, Copy)]
pub struct Counter {
    scan: Scan,
    flags: u8,
    utf8_cps: u32,
    sealed: bool,
}

impl Counter {
    /// 空。
    pub const fn blank() -> Counter {
        Counter { scan: Scan::blank(), flags: 0, utf8_cps: 0, sealed: false }
    }

    /// 对输入计数。
    pub fn run(inp: &[u8], flags: u8) -> Result<Counter, u32> {
        if flags == 0 || flags & !F_ALL != 0 {
            return Err(WC_EINVAL);
        }
        let sc = scan(inp, WORDS_CAP)?;
        let mut c = Counter { scan: sc, flags, utf8_cps: 0, sealed: false };
        if flags & F_UTF8 != 0 {
            c.utf8_cps = utf8_count(inp)?;
        }
        if flags & F_CHARS != 0 && !c.scan.counts.m_eq_c() {
            return Err(WC_ESTATE);
        }
        Ok(c)
    }

    /// 计数。
    pub fn counts(&self) -> Counts {
        self.scan.counts
    }

    /// 标志。
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// UTF-8 码点数。
    pub fn utf8_cps(&self) -> u32 {
        self.utf8_cps
    }

    /// 扫描。
    pub fn scan(&self) -> &Scan {
        &self.scan
    }

    /// 封存。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 再跑一次（封存后拒绝）。
    pub fn rerun(&mut self, inp: &[u8]) -> Result<Counts, u32> {
        if self.sealed {
            return Err(WC_ESTATE);
        }
        let sc = scan(inp, WORDS_CAP)?;
        self.scan = sc;
        Ok(self.scan.counts)
    }

    /// 按标志渲染一行输出。
    pub fn render(&self) -> String {
        let mut s = String::new();
        if self.flags & F_LINES != 0 {
            push_dec(&mut s, self.scan.counts.lines as u64);
        }
        if self.flags & F_WORDS != 0 {
            if !s.is_empty() {
                s.push(' ');
            }
            push_dec(&mut s, self.scan.counts.words as u64);
        }
        if self.flags & F_CHARS != 0 {
            if !s.is_empty() {
                s.push(' ');
            }
            push_dec(&mut s, self.scan.counts.chars as u64);
        }
        if self.flags & F_BYTES != 0 {
            if !s.is_empty() {
                s.push(' ');
            }
            push_dec(&mut s, self.scan.counts.bytes as u64);
        }
        if self.flags & F_MAXLINE != 0 {
            if !s.is_empty() {
                s.push(' ');
            }
            push_dec(&mut s, self.scan.counts.max_line as u64);
        }
        s
    }

    /// 计划串。
    pub fn plan(&self) -> String {
        let mut s = String::from(if self.flags == 0 { "none" } else { "wc" });
        s.push_str(" flags=");
        s.push_str(&flags_text(self.flags));
        s.push_str(" bytes=");
        push_dec(&mut s, self.scan.counts.bytes as u64);
        s.push_str(" chars=");
        push_dec(&mut s, self.scan.counts.chars as u64);
        if self.flags & F_UTF8 != 0 {
            s.push_str(" utf8=");
            push_dec(&mut s, self.utf8_cps as u64);
        }
        s
    }
}

/// 对账：实测 `-m` 与 `-c` 是否全等。
pub fn m_eq_c_ok(inp: &[u8]) -> bool {
    match scan(inp, WORDS_CAP) {
        Ok(s) => s.counts.m_eq_c(),
        Err(_) => false,
    }
}

/// 一组样本输入。
pub const CASES: [&[u8]; 12] = [
    b"",
    b"a",
    b"a\n",
    b"\n\n\n",
    b"one two\nthree four\n",
    b"don't stop",
    b"a-b c_d",
    b"   leading and trailing   ",
    b"tab\there\n",
    b"\xc3\xa9\xc3\xa9\n",
    b"\xe4\xb8\xad\xe6\x96\x87 text\n",
    b"no newline at end",
];

/// 全样本表：每行一例。
pub fn matrix_text() -> String {
    let mut s = String::new();
    newline(&mut s);
    s.push_str("CASE L W M C MAX EQ UTF8");
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            let sc = scan(c, WORDS_CAP);
            newline(&mut s);
            push_dec(&mut s, i as u64);
            match sc {
                Ok(v) => {
                    s.push(' ');
                    push_dec(&mut s, v.counts.lines as u64);
                    s.push(' ');
                    push_dec(&mut s, v.counts.words as u64);
                    s.push(' ');
                    push_dec(&mut s, v.counts.chars as u64);
                    s.push(' ');
                    push_dec(&mut s, v.counts.bytes as u64);
                    s.push(' ');
                    push_dec(&mut s, v.counts.max_line as u64);
                    s.push(' ');
                    if v.counts.m_eq_c() {
                        s.push('1');
                    } else {
                        s.push('0');
                    }
                    s.push(' ');
                    match utf8_count(c) {
                        Ok(n) => push_dec(&mut s, n as u64),
                        Err(_) => s.push('-'),
                    }
                }
                Err(_) => s.push_str(" ----"),
            }
        }
        i += 1;
    }
    s
}

/// 反推：跨度覆盖数 + 空白数 应等于非换行字节数。
pub fn reconstruct_ok(inp: &[u8]) -> bool {
    match scan(inp, WORDS_CAP) {
        Ok(s) => {
            let mut nl = 0u32;
            let mut i = 0usize;
            while i < inp.len() {
                if let Some(b) = inp.get(i) {
                    if *b == 0x0a {
                        nl += 1;
                    }
                }
                i += 1;
            }
            let covered = s.covered();
            let total = s.counts.bytes;
            covered
                .saturating_add(s.breaks)
                .saturating_add(nl)
                == total
        }
        Err(_) => false,
    }
}

/// 对齐差值（实测减声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delta {
    /// 行差。
    pub lines: i64,
    /// 词差。
    pub words: i64,
}

impl Delta {
    /// 比对。
    pub fn of(a: &Counts, b: &Counts) -> Delta {
        Delta {
            lines: a.lines as i64 - b.lines as i64,
            words: a.words as i64 - b.words as i64,
        }
    }

    /// 是否零。
    pub fn zero(&self) -> bool {
        self.lines == 0 && self.words == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("lines");
        push_i64(&mut s, self.lines);
        s.push_str(" words");
        push_i64(&mut s, self.words);
        s
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
            return Err(WC_ESLOT);
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
            return Err(WC_EBAL);
        }
        const VALS: [u32; 8] = [2, 4, 19, 1, 6, 10, 5, 2];
        const CODES: [u32; 6] =
            [WC_EINVAL, WC_ELEN, WC_EBYTE, WC_ERANGE, WC_ESHORT, WC_ECONF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(WC_EBAL),
            };
            if s.slot as usize != i {
                return Err(WC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(WC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(WC_EVALUE);
                        }
                    }
                    None => return Err(WC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(WC_EVALUE);
                        }
                    }
                    None => return Err(WC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(WC_EBAL);
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
            return Err(WC_EBAL);
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
            Err(WC_ESEAL)
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

/// 样例文本。
pub const SAMPLE: &[u8] = b"one two\nthree four\n";

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_LINE, {
        scan(SAMPLE, WORDS_CAP).map(|s| s.counts.lines)
    }));

    let _ = l.register(word_cell(1, OP_SCAN, {
        scan(SAMPLE, WORDS_CAP).map(|s| s.counts.words)
    }));

    let _ = l.register(word_cell(2, OP_SCAN, {
        scan(SAMPLE, WORDS_CAP).map(|s| s.counts.bytes)
    }));

    let _ = l.register(word_cell(3, OP_MC, {
        scan(SAMPLE, WORDS_CAP)
            .map(|s| if s.counts.m_eq_c() { 1 } else { 0 })
    }));

    let _ = l.register(word_cell(4, OP_SPACE, Ok(space_count())));

    let _ = l.register(word_cell(5, OP_MAX, {
        scan(SAMPLE, WORDS_CAP).map(|s| s.counts.max_line)
    }));

    let _ = l.register(word_cell(6, OP_UTF8, utf8_count(b"h\xc3\xa9llo")));

    let _ = l.register(count_cell(7, OP_SPAN, {
        scan(b"a-b c_d", WORDS_CAP).map(|s| s.nspans)
    }));

    let _ = l.register(count_cell(8, OP_FLAG, {
        parse_flags(&[]).map(|v| v as usize)
    }));

    let _ = l.register(count_cell(9, OP_SCAN, {
        let big = [b'a'; IN_CAP + 1];
        scan(&big, WORDS_CAP).map(|s| s.counts.bytes as usize)
    }));

    let _ = l.register(word_cell(10, OP_UTF8, utf8_count(b"\xff")));

    let _ = l.register(word_cell(11, OP_UTF8, utf8_count(b"\xc0\x80")));

    let _ = l.register(word_cell(12, OP_UTF8, utf8_count(b"\xe9")));

    let _ = l.register(count_cell(13, OP_FLAG, {
        parse_flags(&[b"-m", b"--utf8"]).map(|v| v as usize)
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let sc = scan(SAMPLE, WORDS_CAP);
        match sc {
            Ok(v) => {
                v.counts.lines == 2
                    && v.counts.words == 4
                    && v.counts.bytes == 19
                    && v.counts.chars == 19
                    && v.counts.max_line == 10
                    && v.nspans == 4
                    && v.counts.m_eq_c()
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
            match scan(&one, 1) {
                Ok(v) => {
                    let expect = if is_space_c(b) { 0 } else { 1 };
                    if v.counts.words != expect {
                        ok = false;
                    }
                    if !v.counts.m_eq_c() {
                        ok = false;
                    }
                    if splits_word(b) != (expect == 0) {
                        ok = false;
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
        let a = utf8_count(b"\xf0\x9f\x98\x80\xe4\xb8\xad") == Ok(2);
        let b = utf8_count(b"\xff") == Err(WC_EBYTE);
        let c = match scan(b"a-b c_d", WORDS_CAP) {
            Ok(v) => match (v.span(0), v.span(1), v.span(2)) {
                (Some(x), Some(y), None) => {
                    span_slice(b"a-b c_d", x) == Some(b"a-b".as_slice())
                        && span_slice(b"a-b c_d", y) == Some(b"c_d".as_slice())
                }
                _ => false,
            },
            Err(_) => false,
        };
        let d = reconstruct_ok(SAMPLE) && reconstruct_ok(b"a-b c_d");
        a && b && c && d
    };

    out
}

/// 判据集合。
pub fn run_unx11568_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11568.wc");

    s.add(
        "WC01-空白集",
        {
            let a = space_count() == 6;
            let b = SPACE_BYTES.len() == 6;
            let mut ok = true;
            let mut i = 0usize;
            while i < SPACE_BYTES.len() {
                if let Some(v) = SPACE_BYTES.get(i) {
                    if !is_space_c(*v) {
                        ok = false;
                    }
                }
                i += 1;
            }
            let c = ok;
            let d = !is_space_c(b'_')
                && !is_space_c(0x00)
                && !is_space_c(0x7f)
                && !is_space_c(0x85)
                && !is_space_c(0xa0);
            let e = is_space_c(0x0b) && is_space_c(0x0c) && is_space_c(0x0d);
            let f = space_text() == String::from("space 20 09 0a 0b 0c 0d");
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "WC02-词界语义",
        {
            let (yes, no) = boundary_counts();
            let a = yes == 6 && no == 250;
            let b = !splits_word(b'\'') && !splits_word(b'-') && !splits_word(b'.');
            let c = !splits_word(b'_') && !splits_word(b',') && !splits_word(b':');
            let d = !splits_word(b'/') && !splits_word(b'@') && !splits_word(b'\\');
            let e = splits_word(b' ') && splits_word(b'\t') && splits_word(b'\n');
            let f = splits_word(b'\x0b') && splits_word(b'\x0c') && splits_word(b'\x0d');
            let text = boundary_text();
            let g = text.split('\n').count() == 33;
            let h = text.contains("27 0") && text.contains("2d 0") && text.contains("20 1");
            let i = match scan(b"don't stop", WORDS_CAP) {
                Ok(v) => v.counts.words == 2,
                Err(_) => false,
            };
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "WC03-计数基本",
        {
            let mut ok = true;
            match scan(b"one two\nthree four\n", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.lines != 2 || v.counts.words != 4 || v.counts.bytes != 19 {
                        ok = false;
                    }
                    if v.counts.max_line != 10 || v.nspans != 4 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match scan(b"", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.lines != 0
                        || v.counts.words != 0
                        || v.counts.bytes != 0
                        || v.counts.max_line != 0
                    {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match scan(b"\n\n\n", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.lines != 3 || v.counts.words != 0 || v.counts.bytes != 3 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match scan(b"a", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.lines != 0 || v.counts.words != 1 || v.counts.max_line != 1 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match scan(b"   leading and trailing   ", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.words != 3 || v.nspans != 3 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            match scan(b"a-b c_d", WORDS_CAP) {
                Ok(v) => {
                    if v.counts.words != 2 {
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
        "WC04-m≡c 全等",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CASES.len() {
                if let Some(c) = CASES.get(i) {
                    match scan(c, WORDS_CAP) {
                        Ok(v) => {
                            if v.counts.chars != v.counts.bytes || !v.counts.m_eq_c() {
                                ok = false;
                            }
                            if v.counts.chars as usize != c.len() {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                }
                i += 1;
            }
            let mut b = 0u32;
            while b < 256 {
                let one = [b as u8];
                match scan(&one, 1) {
                    Ok(v) => {
                        if v.counts.chars != 1 || v.counts.bytes != 1 {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                b += 1;
            }
            let diverge = match (utf8_count(b"\xc3\xa9\xc3\xa9"), scan(b"\xc3\xa9\xc3\xa9", WORDS_CAP))
            {
                (Ok(u), Ok(v)) => u == 2 && v.counts.bytes == 4 && v.counts.chars == 4,
                _ => false,
            };
            ok && diverge
        },
        "static",
    );

    s.add(
        "WC05-行与最长行",
        {
            let a = line_width(b"tab\there") == 12;
            let b = line_width(b"") == 0;
            let c = width_of(0x09, 0) == 8 && width_of(0x09, 7) == 1 && width_of(0x09, 8) == 8;
            let d = width_of(0x01, 3) == 0 && width_of(0x7f, 3) == 0 && width_of(b'a', 3) == 1;
            let e = match scan(b"tab\there\n", WORDS_CAP) {
                Ok(v) => v.counts.max_line == 12,
                Err(_) => false,
            };
            let f = match scan(b"ab\nabcdef\nab\n", WORDS_CAP) {
                Ok(v) => v.counts.max_line == 6 && v.counts.lines == 3,
                Err(_) => false,
            };
            let g = match scan(b"no newline at end", WORDS_CAP) {
                Ok(v) => v.counts.lines == 0 && v.counts.max_line == 17,
                Err(_) => false,
            };
            let h = match scan(b"a\nbb\n", WORDS_CAP) {
                Ok(v) => v.counts.max_line == 2,
                Err(_) => false,
            };
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "WC06-UTF8 解码",
        {
            let a = utf8_count(b"") == Ok(0);
            let b = utf8_count(b"abc") == Ok(3);
            let c = utf8_count(b"\xc3\xa9") == Ok(1);
            let d = utf8_count(b"\xe4\xb8\xad") == Ok(1);
            let e = utf8_count(b"\xf0\x9f\x98\x80") == Ok(1);
            let f = utf8_count(b"a\xc3\xa9\xe4\xb8\xad\xf0\x9f\x98\x80") == Ok(4);
            let g = utf8_count(b"\xff") == Err(WC_EBYTE);
            let h = utf8_count(b"\x80") == Err(WC_EBYTE);
            let i = utf8_count(b"\xe0\x28\x41") == Err(WC_EBYTE);
            let j = utf8_count(b"\xc0\x80") == Err(WC_ERANGE);
            let k = utf8_count(b"\xed\xa0\x80") == Err(WC_ERANGE);
            let l = utf8_count(b"\xf4\x90\x80\x80") == Err(WC_ERANGE);
            let m = utf8_count(b"\xe9") == Err(WC_ESHORT);
            let n = utf8_count(b"\xf0\x9f") == Err(WC_ESHORT);
            let o = utf8_count(b"\xc3") == Err(WC_ESHORT);
            a && b && c && d && e && f && g && h && i && j && k && l && m && n && o
        },
        "static",
    );

    s.add(
        "WC07-词跨度",
        {
            let src: &[u8] = b"a-b c_d";
            let a = match scan(src, WORDS_CAP) {
                Ok(v) => {
                    let s0 = v.span(0);
                    let s1 = v.span(1);
                    let s2 = v.span(2);
                    let p0 = match s0 {
                        Some(sp) => span_slice(src, sp) == Some(b"a-b".as_slice()),
                        None => false,
                    };
                    let p1 = match s1 {
                        Some(sp) => span_slice(src, sp) == Some(b"c_d".as_slice()),
                        None => false,
                    };
                    p0 && p1 && s2.is_none() && v.covered() == 6 && v.breaks == 1
                }
                Err(_) => false,
            };
            let b = match scan(b"  x  ", WORDS_CAP) {
                Ok(v) => {
                    v.nspans == 1
                        && match v.span(0) {
                            Some(sp) => sp.start == 2 && sp.len == 1,
                            None => false,
                        }
                }
                Err(_) => false,
            };
            let c = match scan(b"     ", WORDS_CAP) {
                Ok(v) => v.nspans == 0 && v.counts.words == 0 && v.breaks == 5,
                Err(_) => false,
            };
            let d = match scan(b"", WORDS_CAP) {
                Ok(v) => v.nspans == 0 && v.covered() == 0,
                Err(_) => false,
            };
            let e = match scan(b"alpha beta gamma", WORDS_CAP) {
                Ok(v) => {
                    v.nspans == 3
                        && v.span_text() == String::from("@0+5 @6+4 @11+5")
                        && v.covered() == 14
                }
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "WC08-标志解析",
        {
            let a = parse_flags(&[b"-l"]) == Ok(F_LINES);
            let b = parse_flags(&[b"-lw"]) == Ok(F_LINES | F_WORDS);
            let c = parse_flags(&[b"-lwcmL"]) == Ok(F_LINES | F_WORDS | F_BYTES | F_CHARS | F_MAXLINE);
            let d = parse_flags(&[b"--bytes"]) == Ok(F_BYTES);
            let e = parse_flags(&[b"-l", b"-w"]) == Ok(F_LINES | F_WORDS);
            let f = parse_flags(&[b"-ll"]) == Err(WC_EDUP);
            let g = parse_flags(&[b"-l", b"--lines"]) == Err(WC_EDUP);
            let h = parse_flags(&[b"-q"]) == Err(WC_EINVAL);
            let i = parse_flags(&[]) == Err(WC_EINVAL);
            let j = parse_flags(&[b"file"]) == Err(WC_EINVAL);
            let k = parse_flags(&[b"-m", b"--utf8"]) == Err(WC_ECONF);
            let l = parse_flags(&[b"-c", b"--utf8"]) == Ok(F_BYTES | F_UTF8);
            let m = flags_text(F_LINES | F_WORDS | F_BYTES) == String::from("l w c");
            let n = flags_text(F_MAXLINE | F_UTF8) == String::from("L utf8");
            a && b && c && d && e && f && g && h && i && j && k && l && m && n
        },
        "static",
    );

    s.add(
        "WC09-容量闸门",
        {
            let a = {
                let big = [b'a'; IN_CAP + 1];
                scan(&big, WORDS_CAP) == Err(WC_ELEN)
            };
            let b = scan(&[b'a'; IN_CAP], WORDS_CAP).is_ok();
            let c = match scan(b"a ", 1) {
                Ok(v) => v.nspans == 1,
                Err(_) => false,
            };
            let d = scan(b"a b c", 2) == Err(WC_ERANGE);
            let e = scan(b"a", 0) == Err(WC_ERANGE);
            let f = match scan(b"  ", 0) {
                Ok(v) => v.nspans == 0,
                Err(_) => false,
            };
            let g = {
                let mut many = [0u8; 200];
                let mut i = 0usize;
                while i < many.len() {
                    many[i] = if i % 2 == 0 { b'a' } else { b' ' };
                    i += 1;
                }
                scan(&many, 64) == Err(WC_ERANGE)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "WC10-渲染总表",
        {
            let c = Counter::run(SAMPLE, F_ALL);
            let a = match c {
                Ok(v) => {
                    v.render() == String::from("2 4 19 19 10")
                        && v.plan()
                            == String::from("wc flags=l w c m L utf8 bytes=19 chars=19 utf8=19")
                        && v.utf8_cps() == 19
                }
                Err(_) => false,
            };
            let b = match Counter::run(b"a\n", F_LINES | F_WORDS) {
                Ok(v) => v.render() == String::from("1 1"),
                Err(_) => false,
            };
            let d = match Counter::run(b"abc", F_BYTES) {
                Ok(v) => v.render() == String::from("3"),
                Err(_) => false,
            };
            let e = match Counter::run(b"abc", 0) {
                Ok(_) => false,
                Err(c) => c == WC_EINVAL,
            };
            let text = matrix_text();
            let f = text.split('\n').count() == 13;
            let g = text.contains("4 2 4 19 19 10 1 19");
            let h = text.contains("9 1 1 5 5 4 1 3");
            a && b && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "WC11-反推校验",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CASES.len() {
                if let Some(c) = CASES.get(i) {
                    if !reconstruct_ok(c) {
                        ok = false;
                    }
                }
                i += 1;
            }
            let a = reconstruct_ok(SAMPLE) && reconstruct_ok(b"alpha beta gamma");
            let b = match (scan(SAMPLE, WORDS_CAP), scan(b"a\n", WORDS_CAP)) {
                (Ok(u), Ok(v)) => {
                    let d = Delta::of(&u.counts, &v.counts);
                    d.lines == 1 && d.words == 3 && !d.zero()
                        && d.text() == String::from("lines1 words3")
                }
                _ => false,
            };
            let c = match (scan(b"a", WORDS_CAP), scan(b"a", WORDS_CAP)) {
                (Ok(u), Ok(v)) => Delta::of(&u.counts, &v.counts).zero(),
                _ => false,
            };
            ok && a && b && c
        },
        "static",
    );

    s.add(
        "WC12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = match l.sample(0) {
                Some(v) => v.ok && v.value == 2 && v.op == OP_LINE,
                None => false,
            };
            let e = match l.sample(9) {
                Some(v) => !v.ok && v.code == WC_ELEN,
                None => false,
            };
            let f = match l.sample(13) {
                Some(v) => !v.ok && v.code == WC_ECONF,
                None => false,
            };
            let g = l.replay() == Ok(l.seal());
            let h = l.sample(SAMPLES).is_none() && !l.is_empty();
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "WC13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut l = build_ledger();
            let sealed = l.seal();
            let replay_ok = l.replay() == Ok(sealed);
            let frozen = l.freeze() == Ok(sealed);
            let late = l.register(Sample { slot: 13, op: OP_SCAN, ok: true, code: 0, value: 1 });
            let rej1 = l.rejects();
            let again = l.freeze();
            let mut c = match Counter::run(b"a b\n", F_LINES | F_WORDS) {
                Ok(v) => v,
                Err(_) => Counter::blank(),
            };
            c.seal();
            let state = c.rerun(b"c") == Err(WC_ESTATE);
            ok = ok
                && replay_ok
                && frozen
                && late == Err(WC_ESLOT)
                && rej1 == 1
                && again == Err(WC_EBAL)
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
    fn space_members() {
        assert_eq!(space_count(), 6);
        assert!(is_space_c(b' '));
        assert!(is_space_c(b'\t'));
        assert!(!is_space_c(b'a'));
        assert!(!is_space_c(0x85));
        assert_eq!(space_text(), "space 20 09 0a 0b 0c 0d");
    }

    #[test]
    fn width_rules() {
        assert_eq!(width_of(b'a', 0), 1);
        assert_eq!(width_of(b'\t', 0), 8);
        assert_eq!(width_of(b'\t', 7), 1);
        assert_eq!(width_of(b'\t', 8), 8);
        assert_eq!(width_of(0x01, 5), 0);
        assert_eq!(line_width(b"tab\there"), 12);
    }

    #[test]
    fn scan_sample() {
        let s = scan(SAMPLE, WORDS_CAP).expect("scan");
        assert_eq!(s.counts.lines, 2);
        assert_eq!(s.counts.words, 4);
        assert_eq!(s.counts.bytes, 19);
        assert_eq!(s.counts.chars, 19);
        assert_eq!(s.counts.max_line, 10);
        assert_eq!(s.nspans, 4);
        assert!(s.counts.m_eq_c());
    }

    #[test]
    fn scan_empty() {
        let s = scan(b"", WORDS_CAP).expect("scan");
        assert_eq!(s.counts, Counts::zero());
        assert_eq!(s.nspans, 0);
        assert!(s.counts.m_eq_c());
    }

    #[test]
    fn word_boundaries() {
        assert_eq!(boundary_counts(), (6, 250));
        assert!(splits_word(b'\n'));
        assert!(!splits_word(b'-'));
        assert!(!splits_word(b'\''));
        let s = scan(b"don't stop", WORDS_CAP).expect("scan");
        assert_eq!(s.counts.words, 2);
    }

    #[test]
    fn spans_roundtrip() {
        let src = b"alpha beta gamma";
        let s = scan(src, WORDS_CAP).expect("scan");
        assert_eq!(s.nspans, 3);
        let a = s.span(0).expect("0");
        assert_eq!(span_slice(src, a), Some(b"alpha".as_slice()));
        let b = s.span(2).expect("2");
        assert_eq!(span_slice(src, b), Some(b"gamma".as_slice()));
        assert!(reconstruct_ok(src));
    }

    #[test]
    fn span_capacity() {
        assert_eq!(scan(b"a b c", 2), Err(WC_ERANGE));
        assert_eq!(scan(b"a", 0), Err(WC_ERANGE));
        assert!(scan(b"  ", 0).is_ok());
    }

    #[test]
    fn input_capacity() {
        let big = [b'a'; IN_CAP + 1];
        assert_eq!(scan(&big, WORDS_CAP), Err(WC_ELEN));
    }

    #[test]
    fn utf8_valid() {
        assert_eq!(utf8_count(b""), Ok(0));
        assert_eq!(utf8_count(b"abc"), Ok(3));
        assert_eq!(utf8_count(b"\xc3\xa9"), Ok(1));
        assert_eq!(utf8_count(b"\xe4\xb8\xad"), Ok(1));
        assert_eq!(utf8_count(b"\xf0\x9f\x98\x80"), Ok(1));
    }

    #[test]
    fn utf8_errors() {
        assert_eq!(utf8_count(b"\xff"), Err(WC_EBYTE));
        assert_eq!(utf8_count(b"\xc0\x80"), Err(WC_ERANGE));
        assert_eq!(utf8_count(b"\xe9"), Err(WC_ESHORT));
        assert_eq!(utf8_count(b"\xed\xa0\x80"), Err(WC_ERANGE));
    }

    #[test]
    fn flags_parse() {
        assert_eq!(parse_flags(&[b"-lw"]), Ok(F_LINES | F_WORDS));
        assert_eq!(parse_flags(&[b"-ll"]), Err(WC_EDUP));
        assert_eq!(parse_flags(&[]), Err(WC_EINVAL));
        assert_eq!(parse_flags(&[b"-m", b"--utf8"]), Err(WC_ECONF));
    }

    #[test]
    fn counter_render() {
        let c = Counter::run(b"one two\n", F_LINES | F_WORDS | F_BYTES).expect("run");
        assert_eq!(c.render(), "1 2 8");
        assert!(c.plan().starts_with("wc flags="));
    }

    #[test]
    fn counter_seal() {
        let mut c = Counter::run(b"a", F_WORDS).expect("run");
        c.seal();
        assert!(c.is_sealed());
        assert_eq!(c.rerun(b"b"), Err(WC_ESTATE));
    }

    #[test]
    fn m_eq_c_matrix() {
        for c in CASES {
            assert!(m_eq_c_ok(c));
        }
        let mut i = 0u32;
        while i < 256 {
            let one = [i as u8];
            assert!(m_eq_c_ok(&one));
            i += 1;
        }
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(2).map(|v| v.value), Some(19));
        assert_eq!(l.sample(10).map(|v| v.code), Some(WC_EBYTE));
    }

    #[test]
    fn seal_freeze_replay() {
        let mut l = build_ledger();
        let sealed = l.seal();
        assert_eq!(l.replay().expect("replay"), sealed);
        assert_eq!(l.freeze().expect("freeze"), sealed);
        assert!(l.is_frozen());
        let late = Sample { slot: 13, op: OP_SCAN, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(late), Err(WC_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze(), Err(WC_EBAL));
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
        assert!(run_unx11568_checks().all_passed());
    }
}
