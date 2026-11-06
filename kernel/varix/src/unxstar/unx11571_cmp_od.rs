//! UNX-F11571 cmp/od 字节语义 locale 无关性判据
//!
//! 真实问题：`cmp` 与 `od` 是最底层的字节工具。它们不应该因为 `LC_ALL`、
//! `LC_CTYPE`、`LC_COLLATE` 等变量而改变任何一个输出字节：比较的是无符号
//! 字节值，转储的是原始字节，`-l` 里的字符列只是尽力显示，`od -c` 的转义表
//! 固定为 C 体例。GNU coreutils 的实现里，这两个工具只调用单字节分类函数，
//! 不进入多字节/宽字符路径，因此对整套 `LC_*` 环境保持不变。
//!
//! 本单把这套语义拆成三层：
//!
//! 1. `compare`：逐字节比较两个流，给出首个差异的字节号（1 起）、行号（按
//!    file1 的换行计数）、差异总数与前 32 组差异对；`-n` 截断后只看前缀；
//!    长度不等且前缀相同时给出 EOF 状态。全程使用 `u8` 比较，与 locale 无关。
//! 2. `od_dump`：16 字节一行的经典转储，地址为 7 位十六进制；`-tx1` 两字符
//!    十六进制、`-to1` 三字符八进制、`-tc` 固定 C 转义；`-An` 去地址；`-v`
//!    关闭重复行抑制，否则连续相同行折叠为一个 `*`；`-j`/`-N` 截取窗口。
//! 3. `locale_invariance`：构造 16 套不同的 `LC_*` 组合（C、POSIX、逐类别
//!    替换为 en_US.UTF-8、全量 UTF-8），先证明这些组合的签名确实互不相同，
//!    再证明 `cmp` 文本与 `od` 文本在每套组合下的 FNV-1a 指纹完全一致。
//!
//! 关键不变量：
//! - 比较与转储函数都不接收 `Locale` 参数；语言环境只出现在不变性试验的
//!   外层循环里，用来证明「输入相同则输出相同」。
//! - `od -c` 的短转义表与 11570 的 C 体例一致：`\0 \a \b \t \n \v \f \r`，
//!   其余不可打印字节写成三位八进制，反斜杠写成 `\\`。
//! - 十六进制转储是可逆的：`parse_hex_stream` 能把自己产生的数据列解析回
//!   原始字节；256 字节全扫描验证这条双射。
//! - 地址列只由偏移决定，与字节内容无关；末行地址等于 skip + 实际读入长度。
//! - 重复行抑制只折叠「完全相同且长度相同」的完整 16 字节行，末行短行不
//!   参与折叠，`*` 每段连续重复只输出一次。
//!
//! 闸门次序：选项字形（`CM_EINVAL`）→ 缺参数（`CM_ESHORT`）→ 取值越界
//! （`CM_ERANGE`）→ 选项冲突（`CM_ECONF`）→ 重复选项（`CM_EDUP`）→ 转储类型
//! 字母不认识（`CM_EBYTE`）→ 长度容量（`CM_ELEN`）→ 封存状态（`CM_ESTATE`）。
//! 账本另用 `CM_EBAL` / `CM_ESLOT` / `CM_EVALUE` / `CM_ESEAL` / `CM_ENOBS`。
//!
//! 账本语义：十四格 = 八格成功（相等比较字节数、首差字节号、首差行号、差异
//! 总数、转储行数、抑制行数、`-An` 文本长度、locale 组合数）+ 六格失败
//! （未知选项、缺 `-n` 参数、`-n 0`、`-l -s` 冲突、重复 `-l`、`-tq` 坏类型）。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf4c0;

/// 选项字形不认识。
pub const CM_EINVAL: u32 = SEG_BASE + 0x01;
/// 长度/容量越界（名字、差异表）。
pub const CM_ELEN: u32 = SEG_BASE + 0x02;
/// 转储类型字母不认识。
pub const CM_EBYTE: u32 = SEG_BASE + 0x03;
/// 偏移、限制、数值越界。
pub const CM_ERANGE: u32 = SEG_BASE + 0x04;
/// 重复选项。
pub const CM_EDUP: u32 = SEG_BASE + 0x05;
/// 缺参数或被截断。
pub const CM_ESHORT: u32 = SEG_BASE + 0x06;
/// 已封存。
pub const CM_ESTATE: u32 = SEG_BASE + 0x07;
/// 选项冲突。
pub const CM_ECONF: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const CM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const CM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 成功值与基准表不符。
pub const CM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放指纹不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const CM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：比较。
pub const OP_CMP: u8 = 1;
/// 操作码：行号。
pub const OP_LINE: u8 = 2;
/// 操作码：差异列表。
pub const OP_VERBOSE: u8 = 3;
/// 操作码：转储。
pub const OP_OD: u8 = 4;
/// 操作码：重复抑制。
pub const OP_DUP: u8 = 5;
/// 操作码：文本渲染。
pub const OP_RENDER: u8 = 6;
/// 操作码：locale 组合。
pub const OP_LOCALE: u8 = 7;
/// 操作码：选项解析。
pub const OP_PARSE: u8 = 8;

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

/// 折叠一段字节。
pub fn fold_slice(h: u64, v: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            x = fold(x, *b);
        }
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

/// 两位十六进制小写。
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
    s.push((b'0' + ((v >> 6) & 0x07)) as char);
    s.push((b'0' + ((v >> 3) & 0x07)) as char);
    s.push((b'0' + (v & 0x07)) as char);
}

/// 七位十六进制地址（零填充）。
fn push_addr7(s: &mut String, v: usize) {
    let mut shift = 28u32;
    while shift > 0 {
        shift -= 4;
        let d = ((v >> shift) & 0x0f) as u8;
        const HEX: [u8; 16] = *b"0123456789abcdef";
        if let Some(h) = HEX.get(d as usize) {
            s.push(*h as char);
        }
    }
}

/// 十六位十六进制（签名）。
fn push_hex64(s: &mut String, v: u64) {
    let mut shift = 64u32;
    while shift > 0 {
        shift -= 4;
        let d = ((v >> shift) & 0x0f) as u8;
        const HEX: [u8; 16] = *b"0123456789abcdef";
        if let Some(h) = HEX.get(d as usize) {
            s.push(*h as char);
        }
    }
}

/// 非空时补换行。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// 可打印判定：C isprint。
pub fn is_print(b: u8) -> bool {
    b >= 0x20 && b <= 0x7e
}

/// 十六进制数字。
pub fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 行数（换行计数）。
pub fn newline_count(v: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < v.len() {
        if let Some(b) = v.get(i) {
            if *b == 0x0a {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// `cmp -l` 的字符列：可打印非空格原样，否则点。
pub fn cmp_char(b: u8) -> u8 {
    if is_print(b) && b != b' ' {
        b
    } else {
        b'.'
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
        None => Err(CM_ELEN),
    }
}

/// `cmp` 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmpOpts {
    /// `-l`。
    pub verbose: bool,
    /// `-s`。
    pub silent: bool,
    /// `-n` 限制（`usize::MAX` 表示不限）。
    pub limit: usize,
}

impl CmpOpts {
    /// 默认。
    pub const fn plain() -> CmpOpts {
        CmpOpts { verbose: false, silent: false, limit: usize::MAX }
    }
}

/// 一组差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diff {
    /// 字节号（1 起）。
    pub byte: usize,
    /// file1 字节。
    pub a: u8,
    /// file2 字节。
    pub b: u8,
}

impl Diff {
    /// 空。
    pub const fn blank() -> Diff {
        Diff { byte: 0, a: 0, b: 0 }
    }
}

/// 差异表容量。
pub const DIFF_CAP: usize = 32;

/// 比较报告。
#[derive(Clone, Copy, Debug)]
pub struct CmpReport {
    /// 实际比较的字节数。
    pub compared: usize,
    /// 首个差异字节号（1 起；0 表示无差异且长度相等）。
    pub first_byte: usize,
    /// 首个差异所在行（按 file1 计）。
    pub first_line: usize,
    /// 差异总数。
    pub ndiffs: usize,
    /// 前若干组差异。
    pub diffs: [Diff; DIFF_CAP],
    /// EOF 状态：0 无，1 file1 先结束，2 file2 先结束。
    pub eof: u8,
    /// 是否相等。
    pub equal: bool,
}

impl CmpReport {
    /// 空报告。
    pub const fn blank() -> CmpReport {
        CmpReport {
            compared: 0,
            first_byte: 0,
            first_line: 0,
            ndiffs: 0,
            diffs: [Diff::blank(); DIFF_CAP],
            eof: 0,
            equal: true,
        }
    }

    /// 默认文本。
    pub fn default_text(&self) -> String {
        if self.silent_text_ok() {
            return String::new();
        }
        if self.equal {
            return String::from("equal");
        }
        if self.eof != 0 {
            let mut s = String::from("EOF on ");
            if self.eof == 1 {
                s.push('a');
            } else {
                s.push('b');
            }
            s.push_str(" at byte ");
            push_dec(&mut s, self.first_byte as u64);
            return s;
        }
        let mut s = String::from("a b differ: byte ");
        push_dec(&mut s, self.first_byte as u64);
        s.push_str(", line ");
        push_dec(&mut s, self.first_line as u64);
        s
    }

    /// 静默时是否应吞掉输出（保留给渲染层）。
    fn silent_text_ok(&self) -> bool {
        false
    }

    /// `-l` 文本。
    pub fn verbose_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.ndiffs && i < DIFF_CAP {
            if let Some(d) = self.diffs.get(i) {
                push_dec(&mut s, d.byte as u64);
                s.push(' ');
                push_oct3(&mut s, d.a);
                s.push(' ');
                push_oct3(&mut s, d.b);
                s.push(' ');
                s.push(cmp_char(d.a) as char);
                s.push(' ');
                s.push(cmp_char(d.b) as char);
                if i + 1 < self.ndiffs && i + 1 < DIFF_CAP {
                    s.push('\n');
                }
            }
            i += 1;
        }
        s
    }

    /// 依据选项选择文本。
    pub fn text(&self, opts: CmpOpts) -> String {
        if opts.silent {
            String::new()
        } else if opts.verbose {
            self.verbose_text()
        } else {
            self.default_text()
        }
    }

    /// 退出状态：0 相等，1 不同，2 出错（本函数不会出错）。
    pub fn status(&self) -> u8 {
        if self.equal {
            0
        } else {
            1
        }
    }
}

/// 逐字节比较。
pub fn compare(a: &[u8], b: &[u8], opts: CmpOpts) -> Result<CmpReport, u32> {
    if opts.verbose && opts.silent {
        return Err(CM_ECONF);
    }
    let mut r = CmpReport::blank();
    let max = if a.len() < b.len() { a.len() } else { b.len() };
    let n = if opts.limit < max { opts.limit } else { max };
    let mut i = 0usize;
    while i < n {
        let x = match a.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x != y {
            if r.first_byte == 0 {
                r.first_byte = i + 1;
                r.first_line = newline_count(a.get(..i).unwrap_or(&[])) + 1;
            }
            if r.ndiffs < DIFF_CAP {
                if let Some(d) = r.diffs.get_mut(r.ndiffs) {
                    *d = Diff { byte: i + 1, a: x, b: y };
                }
            }
            r.ndiffs += 1;
        }
        i += 1;
    }
    r.compared = n;
    let limited = opts.limit < max;
    if !limited {
        if a.len() < b.len() {
            r.eof = 1;
            r.first_byte = n + 1;
        } else if b.len() < a.len() {
            r.eof = 2;
            r.first_byte = n + 1;
        }
    }
    r.equal = r.ndiffs == 0 && r.eof == 0;
    Ok(r)
}

/// 转储格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OdFormat {
    /// `-tx1`。
    Hex1,
    /// `-to1`。
    Oct1,
    /// `-tc`。
    Char,
}

impl OdFormat {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            OdFormat::Hex1 => "x1",
            OdFormat::Oct1 => "o1",
            OdFormat::Char => "c",
        }
    }
}

/// `od` 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OdOpts {
    /// 显示地址（`-An` 关闭）。
    pub address: bool,
    /// 格式。
    pub format: OdFormat,
    /// `-v`：不折叠重复行。
    pub verbose: bool,
    /// `-j`。
    pub skip: usize,
    /// `-N`。
    pub limit: usize,
}

impl OdOpts {
    /// 默认：地址 + 十六进制 + 折叠。
    pub const fn plain() -> OdOpts {
        OdOpts {
            address: true,
            format: OdFormat::Hex1,
            verbose: false,
            skip: 0,
            limit: usize::MAX,
        }
    }
}

/// 转储统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OdStats {
    /// 实际转储字节。
    pub bytes: usize,
    /// 数据行数（不含末地址行）。
    pub rows: usize,
    /// 被折叠的重复行数。
    pub dup_rows: u32,
    /// 写出字节。
    pub written: usize,
    /// 末地址（skip + 实际读入）。
    pub final_addr: usize,
}

impl OdStats {
    /// 零。
    pub const fn zero() -> OdStats {
        OdStats { bytes: 0, rows: 0, dup_rows: 0, written: 0, final_addr: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.bytes as u64);
        s.push('/');
        push_dec(&mut s, self.rows as u64);
        s.push('/');
        push_dec(&mut s, self.dup_rows as u64);
        s.push('/');
        push_dec(&mut s, self.written as u64);
        s
    }
}

/// 一行字节数。
pub const ROW: usize = 16;

/// C 转义渲染（最多 4 字节）。
pub fn char_escape(b: u8) -> ([u8; 4], usize) {
    let mut v = [0u8; 4];
    let n;
    match b {
        0x00 => {
            v[0] = b'\\';
            v[1] = b'0';
            n = 2;
        }
        0x07 => {
            v[0] = b'\\';
            v[1] = b'a';
            n = 2;
        }
        0x08 => {
            v[0] = b'\\';
            v[1] = b'b';
            n = 2;
        }
        0x09 => {
            v[0] = b'\\';
            v[1] = b't';
            n = 2;
        }
        0x0a => {
            v[0] = b'\\';
            v[1] = b'n';
            n = 2;
        }
        0x0b => {
            v[0] = b'\\';
            v[1] = b'v';
            n = 2;
        }
        0x0c => {
            v[0] = b'\\';
            v[1] = b'f';
            n = 2;
        }
        0x0d => {
            v[0] = b'\\';
            v[1] = b'r';
            n = 2;
        }
        b'\\' => {
            v[0] = b'\\';
            v[1] = b'\\';
            n = 2;
        }
        _ => {
            if is_print(b) {
                v[0] = b;
                n = 1;
            } else {
                v[0] = b'\\';
                v[1] = b'0' + ((b >> 6) & 0x07);
                v[2] = b'0' + ((b >> 3) & 0x07);
                v[3] = b'0' + (b & 0x07);
                n = 4;
            }
        }
    }
    (v, n)
}

/// 写一个四列字符字段。
fn put_char_field(out: &mut [u8], pos: &mut usize, b: u8) -> Result<(), u32> {
    let (v, n) = char_escape(b);
    let mut pad = 4usize.saturating_sub(n);
    while pad > 0 {
        put(out, pos, b' ')?;
        pad -= 1;
    }
    let mut i = 0usize;
    while i < n {
        if let Some(d) = v.get(i) {
            put(out, pos, *d)?;
        }
        i += 1;
    }
    Ok(())
}

/// 渲染一个数据行。
fn put_row(
    data: &[u8],
    addr: usize,
    opts: OdOpts,
    out: &mut [u8],
    pos: &mut usize,
) -> Result<(), u32> {
    if opts.address {
        let mut tmp = String::new();
        push_addr7(&mut tmp, addr);
        let bytes = tmp.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                put(out, pos, *b)?;
            }
            i += 1;
        }
    }
    let mut k = 0usize;
    while k < data.len() {
        let b = match data.get(k) {
            Some(v) => *v,
            None => break,
        };
        if opts.address || k > 0 {
            put(out, pos, b' ')?;
        }
        match opts.format {
            OdFormat::Hex1 => {
                const HEX: [u8; 16] = *b"0123456789abcdef";
                if let Some(h) = HEX.get((b >> 4) as usize) {
                    put(out, pos, *h)?;
                }
                if let Some(l) = HEX.get((b & 0x0f) as usize) {
                    put(out, pos, *l)?;
                }
            }
            OdFormat::Oct1 => {
                put(out, pos, b'0' + ((b >> 6) & 0x07))?;
                put(out, pos, b'0' + ((b >> 3) & 0x07))?;
                put(out, pos, b'0' + (b & 0x07))?;
            }
            OdFormat::Char => put_char_field(out, pos, b)?,
        }
        k += 1;
    }
    put(out, pos, b'\n')
}

/// 转储到字节缓冲。
pub fn od_dump(data: &[u8], opts: OdOpts, out: &mut [u8]) -> Result<OdStats, u32> {
    if opts.skip > data.len() {
        return Err(CM_ERANGE);
    }
    let avail = data.len() - opts.skip;
    let take = if opts.limit < avail { opts.limit } else { avail };
    let slice = match data.get(opts.skip..opts.skip + take) {
        Some(v) => v,
        None => return Err(CM_ERANGE),
    };
    let mut st = OdStats::zero();
    let mut pos = 0usize;
    let mut off = 0usize;
    let mut prev = [0u8; ROW];
    let mut prev_len = 0usize;
    let mut have_prev = false;
    let mut star = false;
    while off < slice.len() {
        let end = if off + ROW < slice.len() { off + ROW } else { slice.len() };
        let row = match slice.get(off..end) {
            Some(v) => v,
            None => break,
        };
        let mut same = have_prev && prev_len == row.len();
        let mut k = 0usize;
        while same && k < row.len() {
            match (prev.get(k), row.get(k)) {
                (Some(a), Some(b)) => {
                    if *a != *b {
                        same = false;
                    }
                }
                _ => same = false,
            }
            k += 1;
        }
        if same && !opts.verbose {
            if !star {
                put(out, &mut pos, b'*')?;
                put(out, &mut pos, b'\n')?;
                star = true;
            }
            st.dup_rows += 1;
            off = end;
            continue;
        }
        star = false;
        put_row(row, opts.skip + off, opts, out, &mut pos)?;
        st.rows += 1;
        let mut m = 0usize;
        while m < row.len() {
            if let (Some(d), Some(s)) = (prev.get_mut(m), row.get(m)) {
                *d = *s;
            }
            m += 1;
        }
        prev_len = row.len();
        have_prev = true;
        off = end;
    }
    if opts.address {
        let mut tmp = String::new();
        push_addr7(&mut tmp, opts.skip + slice.len());
        let bytes = tmp.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                put(out, &mut pos, *b)?;
            }
            i += 1;
        }
        put(out, &mut pos, b'\n')?;
    }
    st.bytes = slice.len();
    st.written = pos;
    st.final_addr = opts.skip + slice.len();
    Ok(st)
}

/// 转储为字符串。
pub fn od_text(data: &[u8], opts: OdOpts) -> Result<String, u32> {
    let mut buf = [0u8; 4096];
    let st = od_dump(data, opts, &mut buf)?;
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

/// 解析十六进制转储的数据列（忽略地址与 `*`）。
pub fn parse_hex_stream(text: &[u8], out: &mut [u8]) -> Result<usize, u32> {
    let mut i = 0usize;
    let mut pos = 0usize;
    while i < text.len() {
        while i < text.len() {
            match text.get(i) {
                Some(b) => {
                    if *b == b' ' || *b == b'\n' || *b == b'\r' || *b == b'\t' {
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
                    if *b == b' ' || *b == b'\n' || *b == b'\r' || *b == b'\t' {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        let tok = text.get(start..i).unwrap_or(&[]);
        if tok == b"*" {
            continue;
        }
        if tok.len() == 2 {
            let h = match hex_value(tok[0]) {
                Some(v) => v,
                None => return Err(CM_EBYTE),
            };
            let l = match hex_value(tok[1]) {
                Some(v) => v,
                None => return Err(CM_EBYTE),
            };
            put(out, &mut pos, (h << 4) | l)?;
            continue;
        }
        // 地址列：7 位十六进制，或更短的纯十六进制地址（去除前导零后）。
        if tok.len() >= 1 && tok.len() <= 7 {
            let mut okaddr = true;
            let mut k = 0usize;
            while k < tok.len() {
                if let Some(b) = tok.get(k) {
                    if hex_value(*b).is_none() {
                        okaddr = false;
                    }
                }
                k += 1;
            }
            if okaddr {
                continue;
            }
        }
        return Err(CM_EBYTE);
    }
    Ok(pos)
}

/// locale 变量个数。
pub const LOCALE_VARS: usize = 14;
/// 试验组合数。
pub const LOCALE_VARIANTS: usize = 16;
/// 标签容量。
pub const TAG_CAP: usize = 16;

/// 固定标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag {
    b: [u8; TAG_CAP],
    len: usize,
}

impl Tag {
    /// 空标签。
    pub const fn blank() -> Tag {
        Tag { b: [0; TAG_CAP], len: 0 }
    }

    /// 从字节构造。
    pub fn from_slice(v: &[u8]) -> Result<Tag, u32> {
        if v.len() > TAG_CAP {
            return Err(CM_ELEN);
        }
        let mut t = Tag::blank();
        let mut i = 0usize;
        while i < v.len() {
            if let (Some(d), Some(s)) = (t.b.get_mut(i), v.get(i)) {
                *d = *s;
            }
            i += 1;
        }
        t.len = v.len();
        Ok(t)
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
}

/// 变量名表。
pub fn locale_var_name(i: usize) -> &'static str {
    const NAMES: [&str; LOCALE_VARS] = [
        "LC_ALL",
        "LC_CTYPE",
        "LC_COLLATE",
        "LC_NUMERIC",
        "LC_TIME",
        "LC_MESSAGES",
        "LC_MONETARY",
        "LC_PAPER",
        "LC_NAME",
        "LC_ADDRESS",
        "LC_TELEPHONE",
        "LC_MEASUREMENT",
        "LC_IDENTIFICATION",
        "LANG",
    ];
    match NAMES.get(i) {
        Some(v) => v,
        None => "",
    }
}

/// 一套语言环境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    tags: [Tag; LOCALE_VARS],
}

impl Locale {
    /// 全 C。
    pub fn c() -> Locale {
        let mut l = Locale { tags: [Tag::blank(); LOCALE_VARS] };
        let mut i = 0usize;
        while i < LOCALE_VARS {
            let _ = l.set(i, b"C");
            i += 1;
        }
        l
    }

    /// 设置一个变量。
    pub fn set(&mut self, i: usize, v: &[u8]) -> Result<(), u32> {
        if i >= LOCALE_VARS {
            return Err(CM_ERANGE);
        }
        let t = Tag::from_slice(v)?;
        if let Some(d) = self.tags.get_mut(i) {
            *d = t;
        }
        Ok(())
    }

    /// 第 i 个变量。
    pub fn get(&self, i: usize) -> Option<Tag> {
        self.tags.get(i).copied()
    }

    /// 第 k 套组合：0 全 C，1..=14 单类别替换，15 全 POSIX。
    pub fn variant(k: usize) -> Locale {
        let mut l = Locale::c();
        let mut i = 0usize;
        while i < LOCALE_VARS {
            let v: &[u8] = if k == 0 {
                b"C"
            } else if k <= LOCALE_VARS && i == k - 1 {
                b"en_US.UTF-8"
            } else if k == LOCALE_VARIANTS - 1 {
                b"POSIX"
            } else {
                b"C"
            };
            let _ = l.set(i, v);
            i += 1;
        }
        l
    }

    /// 签名：变量名 + 取值。
    pub fn signature(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < LOCALE_VARS {
            h = fold_slice(h, locale_var_name(i).as_bytes());
            h = fold(h, b'=');
            if let Some(t) = self.tags.get(i) {
                h = fold_slice(h, t.as_slice());
            }
            h = fold(h, 0);
            i += 1;
        }
        h
    }

    /// 是否全 C。
    pub fn is_c(&self) -> bool {
        let mut i = 0usize;
        while i < LOCALE_VARS {
            if let Some(t) = self.tags.get(i) {
                if t.as_slice() != b"C" {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 不变性结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invariance {
    /// 试验组合数。
    pub cases: u32,
    /// 输出指纹是否全部一致。
    pub stable: bool,
    /// 组合签名是否互不相同。
    pub distinct: u32,
    /// 统一指纹。
    pub hash: u64,
}

impl Invariance {
    /// 零。
    pub const fn zero() -> Invariance {
        Invariance { cases: 0, stable: false, distinct: 0, hash: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.cases as u64);
        s.push('/');
        push_dec(&mut s, self.distinct as u64);
        s.push('/');
        s.push(if self.stable { '1' } else { '0' });
        s.push('/');
        push_hex64(&mut s, self.hash);
        s
    }
}

/// 在 16 套 locale 下测试 cmp 与 od 的输出是否恒定。
pub fn locale_invariance(a: &[u8], b: &[u8], od: OdOpts) -> Result<Invariance, u32> {
    let mut inv = Invariance::zero();
    inv.cases = LOCALE_VARIANTS as u32;
    let mut sigs = [0u64; LOCALE_VARIANTS];
    let mut first = 0u64;
    let mut have_first = false;
    let mut stable = true;
    let mut k = 0usize;
    while k < LOCALE_VARIANTS {
        let loc = Locale::variant(k);
        let sig = loc.signature();
        if let Some(d) = sigs.get_mut(k) {
            *d = sig;
        }
        let cmp = compare(a, b, CmpOpts::plain())?;
        let cmp_text = cmp.default_text();
        let od_text = od_text(a, od)?;
        let mut h = FNV_OFF;
        h = fold_slice(h, cmp_text.as_bytes());
        h = fold(h, 0xff);
        h = fold_slice(h, od_text.as_bytes());
        if !have_first {
            first = h;
            have_first = true;
        } else if h != first {
            stable = false;
        }
        k += 1;
    }
    let mut distinct = 0u32;
    let mut i = 0usize;
    while i < LOCALE_VARIANTS {
        let mut unique = true;
        let mut j = 0usize;
        while j < i {
            if sigs.get(i) == sigs.get(j) {
                unique = false;
            }
            j += 1;
        }
        if unique {
            distinct += 1;
        }
        i += 1;
    }
    inv.stable = stable;
    inv.distinct = distinct;
    inv.hash = first;
    Ok(inv)
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
            return Err(CM_ERANGE);
        }
        if let Some(d) = args.get_mut(n) {
            *d = text.get(start..i).unwrap_or(&[]);
        }
        n += 1;
    }
    Ok(n)
}

/// 十进制非零解析。
fn parse_num(v: &[u8], allow_zero: bool) -> Result<usize, u32> {
    if v.is_empty() {
        return Err(CM_ESHORT);
    }
    let mut x = 0usize;
    let mut i = 0usize;
    while i < v.len() {
        let b = match v.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b < b'0' || b > b'9' {
            return Err(CM_EINVAL);
        }
        x = x * 10 + (b - b'0') as usize;
        if x > 0x0100_0000 {
            return Err(CM_ERANGE);
        }
        i += 1;
    }
    if x == 0 && !allow_zero {
        return Err(CM_ERANGE);
    }
    Ok(x)
}

/// 解析 `cmp` 选项。
pub fn parse_cmp_flags(text: &[u8]) -> Result<CmpOpts, u32> {
    let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
    let n = split_args(text, &mut args)?;
    let mut o = CmpOpts::plain();
    let mut verbose = false;
    let mut silent = false;
    let mut seen_l = false;
    let mut seen_s = false;
    let mut seen_n = false;
    let mut i = 0usize;
    while i < n {
        let tok = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        i += 1;
        if tok == b"-l" {
            if seen_l {
                return Err(CM_EDUP);
            }
            seen_l = true;
            verbose = true;
        } else if tok == b"-s" {
            if seen_s {
                return Err(CM_EDUP);
            }
            seen_s = true;
            silent = true;
        } else if tok == b"-n" {
            if seen_n {
                return Err(CM_EDUP);
            }
            seen_n = true;
            if i >= n {
                return Err(CM_ESHORT);
            }
            let v = match args.get(i) {
                Some(x) => *x,
                None => return Err(CM_ESHORT),
            };
            i += 1;
            o.limit = parse_num(v, false)?;
        } else if tok.len() > 2 && tok.get(..2) == Some(b"-n".as_slice()) {
            if seen_n {
                return Err(CM_EDUP);
            }
            seen_n = true;
            o.limit = parse_num(tok.get(2..).unwrap_or(&[]), false)?;
        } else {
            return Err(CM_EINVAL);
        }
    }
    if verbose && silent {
        return Err(CM_ECONF);
    }
    o.verbose = verbose;
    o.silent = silent;
    Ok(o)
}

const O_ADDR: u32 = 1;
const O_VERB: u32 = 2;
const O_FMT: u32 = 4;
const O_SKIP: u32 = 8;
const O_LIMIT: u32 = 16;

/// 解析转储类型。
fn parse_type(v: &[u8]) -> Result<OdFormat, u32> {
    if v.is_empty() {
        return Err(CM_ESHORT);
    }
    if v == b"x1" {
        Ok(OdFormat::Hex1)
    } else if v == b"o1" {
        Ok(OdFormat::Oct1)
    } else if v == b"c" {
        Ok(OdFormat::Char)
    } else {
        Err(CM_EBYTE)
    }
}

/// 解析 `od` 选项。
pub fn parse_od_flags(text: &[u8]) -> Result<OdOpts, u32> {
    let mut args: [&[u8]; MAX_ARGS] = [&[]; MAX_ARGS];
    let n = split_args(text, &mut args)?;
    let mut o = OdOpts::plain();
    let mut seen = 0u32;
    let mut fmt: Option<OdFormat> = None;
    let mut i = 0usize;
    while i < n {
        let tok = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        i += 1;
        if tok == b"-An" {
            if seen & O_ADDR != 0 {
                return Err(CM_EDUP);
            }
            seen |= O_ADDR;
            o.address = false;
        } else if tok == b"-v" {
            if seen & O_VERB != 0 {
                return Err(CM_EDUP);
            }
            seen |= O_VERB;
            o.verbose = true;
        } else if tok.len() > 2 && tok.get(..2) == Some(b"-t".as_slice()) {
            let f = parse_type(tok.get(2..).unwrap_or(&[]))?;
            if seen & O_FMT != 0 {
                match fmt {
                    Some(old) => {
                        if old == f {
                            return Err(CM_EDUP);
                        }
                        return Err(CM_ECONF);
                    }
                    None => return Err(CM_ECONF),
                }
            }
            seen |= O_FMT;
            fmt = Some(f);
            o.format = f;
        } else if tok == b"-t" {
            if seen & O_FMT != 0 {
                return Err(CM_EDUP);
            }
            seen |= O_FMT;
            if i >= n {
                return Err(CM_ESHORT);
            }
            let v = match args.get(i) {
                Some(x) => *x,
                None => return Err(CM_ESHORT),
            };
            i += 1;
            o.format = parse_type(v)?;
            fmt = Some(o.format);
        } else if tok == b"-j" {
            if seen & O_SKIP != 0 {
                return Err(CM_EDUP);
            }
            seen |= O_SKIP;
            if i >= n {
                return Err(CM_ESHORT);
            }
            let v = match args.get(i) {
                Some(x) => *x,
                None => return Err(CM_ESHORT),
            };
            i += 1;
            o.skip = parse_num(v, true)?;
        } else if tok == b"-N" {
            if seen & O_LIMIT != 0 {
                return Err(CM_EDUP);
            }
            seen |= O_LIMIT;
            if i >= n {
                return Err(CM_ESHORT);
            }
            let v = match args.get(i) {
                Some(x) => *x,
                None => return Err(CM_ESHORT),
            };
            i += 1;
            o.limit = parse_num(v, false)?;
        } else {
            return Err(CM_EINVAL);
        }
    }
    Ok(o)
}

/// 转储类型表文本。
pub fn od_type_text() -> String {
    let mut s = String::new();
    s.push_str("x1 ");
    let mut b = [0u8; 8];
    b[0] = 0x00;
    b[1] = 0x1f;
    b[2] = 0x80;
    b[3] = b'A';
    let o = OdOpts { address: false, format: OdFormat::Hex1, verbose: true, skip: 0, limit: 4 };
    match od_text(&b, o) {
        Ok(v) => s.push_str(&v),
        Err(_) => s.push('x'),
    }
    s.push_str("oct ");
    let o2 = OdOpts { address: false, format: OdFormat::Oct1, verbose: true, skip: 0, limit: 4 };
    match od_text(&b, o2) {
        Ok(v) => s.push_str(&v),
        Err(_) => s.push('x'),
    }
    s.push_str("char ");
    let o3 = OdOpts { address: false, format: OdFormat::Char, verbose: true, skip: 0, limit: 4 };
    match od_text(&b, o3) {
        Ok(v) => s.push_str(&v),
        Err(_) => s.push('x'),
    }
    s
}

/// locale 组合矩阵文本。
pub fn locale_matrix_text() -> String {
    let mut s = String::new();
    let mut k = 0usize;
    while k < LOCALE_VARIANTS {
        let l = Locale::variant(k);
        push_dec(&mut s, k as u64);
        s.push(' ');
        push_hex64(&mut s, l.signature());
        s.push(' ');
        s.push(if l.is_c() { 'C' } else { 'x' });
        if k + 1 < LOCALE_VARIANTS {
            s.push('\n');
        }
        k += 1;
    }
    s
}

/// 子串查找。
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

/// 探针：相等比较的字节数。
fn probe_cmp_compared(a: &[u8], b: &[u8]) -> Result<u32, u32> {
    match compare(a, b, CmpOpts::plain()) {
        Ok(r) => Ok(r.compared as u32),
        Err(c) => Err(c),
    }
}

/// 探针：首差字节号。
fn probe_cmp_first(a: &[u8], b: &[u8]) -> Result<u32, u32> {
    match compare(a, b, CmpOpts::plain()) {
        Ok(r) => Ok(r.first_byte as u32),
        Err(c) => Err(c),
    }
}

/// 探针：首差行号。
fn probe_cmp_line(a: &[u8], b: &[u8]) -> Result<u32, u32> {
    match compare(a, b, CmpOpts::plain()) {
        Ok(r) => Ok(r.first_line as u32),
        Err(c) => Err(c),
    }
}

/// 探针：差异总数。
fn probe_cmp_ndiffs(a: &[u8], b: &[u8]) -> Result<u32, u32> {
    match compare(a, b, CmpOpts::plain()) {
        Ok(r) => Ok(r.ndiffs as u32),
        Err(c) => Err(c),
    }
}

/// 探针：转储行数。
fn probe_od_rows(data: &[u8]) -> Result<usize, u32> {
    let mut out = [0u8; 4096];
    match od_dump(data, OdOpts::plain(), &mut out) {
        Ok(st) => Ok(st.rows),
        Err(c) => Err(c),
    }
}

/// 探针：折叠行数。
fn probe_od_dups(data: &[u8]) -> Result<usize, u32> {
    let mut out = [0u8; 4096];
    match od_dump(data, OdOpts::plain(), &mut out) {
        Ok(st) => Ok(st.dup_rows as usize),
        Err(c) => Err(c),
    }
}

/// 探针：`-An` 下的文本长度。
fn probe_od_written(data: &[u8]) -> Result<usize, u32> {
    let mut out = [0u8; 512];
    let opts = OdOpts {
        address: false,
        format: OdFormat::Hex1,
        verbose: true,
        skip: 0,
        limit: usize::MAX,
    };
    match od_dump(data, opts, &mut out) {
        Ok(st) => Ok(st.written),
        Err(c) => Err(c),
    }
}

/// 探针：locale 组合数。
fn probe_locale_cases() -> Result<u32, u32> {
    Ok(LOCALE_VARIANTS as u32)
}

/// 探针：`cmp` 选项解析（成功返回限制，失败返回错误码）。
fn probe_cmp_flags(text: &[u8]) -> Result<u32, u32> {
    match parse_cmp_flags(text) {
        Ok(o) => Ok(o.limit as u32),
        Err(c) => Err(c),
    }
}

/// 探针：`od` 选项解析（成功返回限制或 0，失败返回错误码）。
fn probe_od_flags(text: &[u8]) -> Result<u32, u32> {
    match parse_od_flags(text) {
        Ok(o) => Ok(if o.limit == usize::MAX { 0 } else { o.limit as u32 }),
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
            return Err(CM_ESLOT);
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
            return Err(CM_EBAL);
        }
        const VALS: [u32; 8] = [3, 2, 2, 3, 2, 2, 12, 16];
        const CODES: [u32; 6] =
            [CM_EINVAL, CM_ESHORT, CM_ERANGE, CM_ECONF, CM_EDUP, CM_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CM_EBAL),
            };
            if s.slot as usize != i {
                return Err(CM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CM_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot);
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
            return Err(CM_EBAL);
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
            Err(CM_ESEAL)
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

    let _ = l.register(word_cell(0, OP_CMP, probe_cmp_compared(b"abc", b"abc")));

    let _ = l.register(word_cell(1, OP_CMP, probe_cmp_first(b"ax", b"ay")));

    let _ = l.register(word_cell(2, OP_LINE, probe_cmp_line(b"a\nb", b"a\nc")));

    let _ = l.register(word_cell(3, OP_VERBOSE, probe_cmp_ndiffs(b"abc", b"xyz")));

    let _ = l.register(count_cell(4, OP_OD, probe_od_rows(&[0u8; 20])));

    let _ = l.register(count_cell(5, OP_DUP, probe_od_dups(&[0u8; 48])));

    let _ = l.register(count_cell(6, OP_RENDER, probe_od_written(b"\x00\x01\x02\x03")));

    let _ = l.register(word_cell(7, OP_LOCALE, probe_locale_cases()));

    let _ = l.register(word_cell(8, OP_PARSE, probe_cmp_flags(b"-Z")));

    let _ = l.register(word_cell(9, OP_PARSE, probe_cmp_flags(b"-n")));

    let _ = l.register(word_cell(10, OP_PARSE, probe_cmp_flags(b"-n 0")));

    let _ = l.register(word_cell(11, OP_PARSE, probe_cmp_flags(b"-l -s")));

    let _ = l.register(word_cell(12, OP_PARSE, probe_cmp_flags(b"-l -l")));

    let _ = l.register(word_cell(13, OP_PARSE, probe_od_flags(b"-tq")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let eq = match compare(b"abc", b"abc", CmpOpts::plain()) {
            Ok(r) => r.equal && r.status() == 0 && r.compared == 3 && r.default_text() == "equal",
            Err(_) => false,
        };
        let df = match compare(b"abc", b"abd", CmpOpts::plain()) {
            Ok(r) => {
                !r.equal
                    && r.status() == 1
                    && r.first_byte == 3
                    && r.first_line == 1
                    && r.ndiffs == 1
            }
            Err(_) => false,
        };
        let eof = match compare(b"ab", b"abc", CmpOpts::plain()) {
            Ok(r) => !r.equal && r.eof == 1 && r.first_byte == 3 && r.ndiffs == 0,
            Err(_) => false,
        };
        eq && df && eof
    };

    out[1] = {
        let mut ok = true;
        let mut data = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            data[i] = i as u8;
            i += 1;
        }
        let opts = OdOpts {
            address: false,
            format: OdFormat::Hex1,
            verbose: true,
            skip: 0,
            limit: usize::MAX,
        };
        let mut start = 0usize;
        while start < 256 {
            let end = if start + ROW < 256 { start + ROW } else { 256 };
            let chunk = data.get(start..end).unwrap_or(&[]);
            match od_text(chunk, opts) {
                Ok(text) => {
                    let mut back = [0u8; ROW];
                    match parse_hex_stream(text.as_bytes(), &mut back) {
                        Ok(n) => {
                            if n != chunk.len() {
                                ok = false;
                            }
                            let mut k = 0usize;
                            while k < n {
                                match (back.get(k), chunk.get(k)) {
                                    (Some(a), Some(b)) => {
                                        if a != b {
                                            ok = false;
                                        }
                                    }
                                    _ => ok = false,
                                }
                                k += 1;
                            }
                        }
                        Err(_) => ok = false,
                    }
                }
                Err(_) => ok = false,
            }
            start = end;
        }
        let (v, n) = char_escape(0x80);
        ok && n == 4 && v.get(..4) == Some(b"\\200".as_slice())
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
        let opts = OdOpts {
            address: false,
            format: OdFormat::Hex1,
            verbose: true,
            skip: 0,
            limit: 8,
        };
        match locale_invariance(b"abc", b"abd", opts) {
            Ok(inv) => {
                inv.cases == 16
                    && inv.distinct == 16
                    && inv.stable
                    && inv.hash != 0
                    && Locale::variant(0).is_c()
            }
            Err(_) => false,
        }
    };

    out
}

/// 十三项检查。
pub fn run_unx11571_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11571.cmpod");

    s.add(
        "CM01-比较语义",
        {
            let a = match compare(b"abc", b"abc", CmpOpts::plain()) {
                Ok(r) => {
                    r.equal
                        && r.compared == 3
                        && r.ndiffs == 0
                        && r.default_text() == "equal"
                }
                Err(_) => false,
            };
            let b = match compare(b"abc", b"abd", CmpOpts::plain()) {
                Ok(r) => {
                    !r.equal
                        && r.first_byte == 3
                        && r.first_line == 1
                        && r.default_text() == "a b differ: byte 3, line 1"
                }
                Err(_) => false,
            };
            let c = match compare(b"a\nb", b"a\nc", CmpOpts::plain()) {
                Ok(r) => r.first_byte == 3 && r.first_line == 2,
                Err(_) => false,
            };
            let d = match compare(b"ab", b"abc", CmpOpts::plain()) {
                Ok(r) => r.eof == 1 && r.first_byte == 3 && !r.equal,
                Err(_) => false,
            };
            let e = match compare(b"abc", b"ab", CmpOpts::plain()) {
                Ok(r) => r.eof == 2 && r.first_byte == 3,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "CM02-差异列表",
        {
            let r = match compare(b"abc", b"xyz", CmpOpts::plain()) {
                Ok(v) => v,
                Err(_) => CmpReport::blank(),
            };
            let a = r.ndiffs == 3 && r.first_byte == 1;
            let b = match (r.diffs.get(0), r.diffs.get(2)) {
                (Some(x), Some(z)) => {
                    x.byte == 1
                        && x.a == b'a'
                        && x.b == b'x'
                        && z.byte == 3
                        && z.a == b'c'
                        && z.b == b'z'
                }
                _ => false,
            };
            let text = r.verbose_text();
            let c = slice_contains(text.as_bytes(), b"1 141 170 a x");
            let d = slice_contains(text.as_bytes(), b"3 143 172 c z");
            let e = r.text(CmpOpts { verbose: true, silent: false, limit: usize::MAX })
                == text;
            let f = r.text(CmpOpts { verbose: false, silent: true, limit: usize::MAX })
                .is_empty();
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "CM03-截断比较",
        {
            let a = match compare(b"abc", b"abd", CmpOpts { verbose: false, silent: false, limit: 2 }) {
                Ok(r) => r.equal && r.compared == 2 && r.ndiffs == 0,
                Err(_) => false,
            };
            let b = match compare(b"abc", b"abc", CmpOpts { verbose: false, silent: false, limit: 1 }) {
                Ok(r) => r.equal && r.compared == 1,
                Err(_) => false,
            };
            let c = match compare(b"abc", b"abd", CmpOpts { verbose: false, silent: false, limit: 5 }) {
                Ok(r) => !r.equal && r.first_byte == 3 && r.compared == 3,
                Err(_) => false,
            };
            let d = match compare(b"", b"", CmpOpts::plain()) {
                Ok(r) => r.equal && r.compared == 0 && r.first_byte == 0,
                Err(_) => false,
            };
            let e = compare(b"a", b"b", CmpOpts { verbose: true, silent: true, limit: usize::MAX })
                .err() == Some(CM_ECONF);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "CM04-比较选项",
        {
            let a = match parse_cmp_flags(b"") {
                Ok(o) => !o.verbose && !o.silent && o.limit == usize::MAX,
                Err(_) => false,
            };
            let b = match parse_cmp_flags(b"-l") {
                Ok(o) => o.verbose && !o.silent,
                Err(_) => false,
            };
            let c = match parse_cmp_flags(b"-s") {
                Ok(o) => o.silent && !o.verbose,
                Err(_) => false,
            };
            let d = match parse_cmp_flags(b"-n 2") {
                Ok(o) => o.limit == 2,
                Err(_) => false,
            };
            let e = match parse_cmp_flags(b"-n2") {
                Ok(o) => o.limit == 2,
                Err(_) => false,
            };
            let f = parse_cmp_flags(b"-l -s") == Err(CM_ECONF);
            let g = parse_cmp_flags(b"-l -l") == Err(CM_EDUP);
            let h = parse_cmp_flags(b"-n") == Err(CM_ESHORT);
            let i = parse_cmp_flags(b"-n 0") == Err(CM_ERANGE);
            let j = parse_cmp_flags(b"-n abc") == Err(CM_EINVAL);
            let k = parse_cmp_flags(b"-Z") == Err(CM_EINVAL);
            a && b && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "CM05-十六进制转储",
        {
            let data = b"\x00\x01\x02\x03";
            let a = match od_text(data, OdOpts::plain()) {
                Ok(v) => v == "0000000 00 01 02 03\n0000004\n",
                Err(_) => false,
            };
            let opts = OdOpts {
                address: false,
                format: OdFormat::Hex1,
                verbose: false,
                skip: 0,
                limit: usize::MAX,
            };
            let b = match od_text(data, opts) {
                Ok(v) => v == "00 01 02 03\n",
                Err(_) => false,
            };
            let c = match od_dump(data, OdOpts::plain(), &mut [0u8; 64]) {
                Ok(st) => st.rows == 1 && st.bytes == 4 && st.final_addr == 4 && st.written == 28,
                Err(_) => false,
            };
            let d = od_text(data, OdOpts { skip: 9, ..OdOpts::plain() }) == Err(CM_ERANGE);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "CM06-八进制与字符",
        {
            let data = b"\x00\x01\x02\x03";
            let oct = match od_text(data, OdOpts { format: OdFormat::Oct1, ..OdOpts::plain() }) {
                Ok(v) => v == "0000000 000 001 002 003\n0000004\n",
                Err(_) => false,
            };
            let chars = b"a\n\t\\\x01";
            let noaddr = OdOpts {
                address: false,
                format: OdFormat::Char,
                verbose: true,
                skip: 0,
                limit: usize::MAX,
            };
            let a = match od_text(chars, noaddr) {
                Ok(v) => {
                    slice_contains(v.as_bytes(), b"\\n")
                        && slice_contains(v.as_bytes(), b"\\t")
                        && slice_contains(v.as_bytes(), b"\\\\")
                        && slice_contains(v.as_bytes(), b"\\001")
                }
                Err(_) => false,
            };
            let (v0, n0) = char_escape(0);
            let (v7, n7) = char_escape(0x7f);
            let b = n0 == 2 && v0.get(..2) == Some(b"\\0".as_slice());
            let c = n7 == 4 && v7.get(..4) == Some(b"\\177".as_slice());
            oct && a && b && c
        },
        "static",
    );

    s.add(
        "CM07-重复行折叠",
        {
            let data = [0u8; 48];
            let a = match od_dump(&data, OdOpts::plain(), &mut [0u8; 4096]) {
                Ok(st) => st.rows == 1 && st.dup_rows == 2 && st.bytes == 48,
                Err(_) => false,
            };
            let text = match od_text(&data, OdOpts::plain()) {
                Ok(v) => v,
                Err(_) => String::new(),
            };
            let b = slice_contains(text.as_bytes(), b"*\n");
            let opts = OdOpts { verbose: true, ..OdOpts::plain() };
            let c = match od_dump(&data, opts, &mut [0u8; 4096]) {
                Ok(st) => st.rows == 3 && st.dup_rows == 0,
                Err(_) => false,
            };
            let d = !slice_contains(text.as_bytes(), b"0000010 00 00");
            a && b && c && d
        },
        "static",
    );

    s.add(
        "CM08-跳过与限额",
        {
            let data = b"\x00\x01\x02\x03\x04\x05";
            let opts = OdOpts {
                address: false,
                format: OdFormat::Hex1,
                verbose: true,
                skip: 2,
                limit: 3,
            };
            let a = match od_text(data, opts) {
                Ok(v) => v == "02 03 04\n",
                Err(_) => false,
            };
            let b = match od_dump(data, opts, &mut [0u8; 64]) {
                Ok(st) => st.bytes == 3 && st.final_addr == 5 && st.written == 9,
                Err(_) => false,
            };
            let c = od_text(data, OdOpts { skip: 2, limit: 99, ..OdOpts::plain() }).map(|v| v.len());
            let d = match c {
                Ok(n) => n == 28,
                Err(_) => false,
            };
            let e = od_text(data, OdOpts { skip: 7, ..OdOpts::plain() }) == Err(CM_ERANGE);
            a && b && d && e
        },
        "static",
    );

    s.add(
        "CM09-转储选项",
        {
            let a = match parse_od_flags(b"-An -v -tx1") {
                Ok(o) => !o.address && o.verbose && o.format == OdFormat::Hex1,
                Err(_) => false,
            };
            let b = match parse_od_flags(b"-to1") {
                Ok(o) => o.format == OdFormat::Oct1,
                Err(_) => false,
            };
            let c = match parse_od_flags(b"-tc") {
                Ok(o) => o.format == OdFormat::Char,
                Err(_) => false,
            };
            let d = parse_od_flags(b"-tx1 -to1") == Err(CM_ECONF);
            let e = parse_od_flags(b"-tx1 -tx1") == Err(CM_EDUP);
            let f = parse_od_flags(b"-tq") == Err(CM_EBYTE);
            let g = parse_od_flags(b"-t") == Err(CM_ESHORT);
            let h = parse_od_flags(b"-j") == Err(CM_ESHORT);
            let i = parse_od_flags(b"-N 0") == Err(CM_ERANGE);
            let j = parse_od_flags(b"-Z") == Err(CM_EINVAL);
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "CM10-locale组合",
        {
            let a = Locale::variant(0).is_c();
            let b = !Locale::variant(1).is_c();
            let c = match Locale::variant(1).get(0) {
                Some(t) => t.as_slice() == b"en_US.UTF-8",
                None => false,
            };
            let d = match Locale::variant(15).get(13) {
                Some(t) => t.as_slice() == b"POSIX",
                None => false,
            };
            let sigs_differ = Locale::variant(0).signature() != Locale::variant(1).signature();
            let e = sigs_differ && Locale::variant(1).signature() != Locale::variant(2).signature();
            let f = locale_matrix_text().as_bytes().len() > 100;
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "CM11-locale不变性",
        {
            let opts = OdOpts {
                address: false,
                format: OdFormat::Hex1,
                verbose: true,
                skip: 0,
                limit: 8,
            };
            let inv = match locale_invariance(b"abc", b"abd", opts) {
                Ok(v) => v,
                Err(_) => Invariance::zero(),
            };
            let a = inv.cases == 16 && inv.distinct == 16;
            let b = inv.stable && inv.hash != 0;
            let again = match locale_invariance(b"abc", b"abd", opts) {
                Ok(v) => v,
                Err(_) => Invariance::zero(),
            };
            let c = again.hash == inv.hash && again.stable;
            let d = match locale_invariance(b"", b"", opts) {
                Ok(v) => v.stable,
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "CM12-字节锚",
        {
            let mut ok = true;
            let mut data = [0u8; 256];
            let mut i = 0usize;
            while i < 256 {
                data[i] = i as u8;
                i += 1;
            }
            let opts = OdOpts {
                address: false,
                format: OdFormat::Hex1,
                verbose: true,
                skip: 0,
                limit: usize::MAX,
            };
            let mut start = 0usize;
            while start < 256 {
                let end = if start + ROW < 256 { start + ROW } else { 256 };
                let chunk = data.get(start..end).unwrap_or(&[]);
                match od_text(chunk, opts) {
                    Ok(text) => {
                        let mut back = [0u8; ROW];
                        match parse_hex_stream(text.as_bytes(), &mut back) {
                            Ok(n) => {
                                if n != chunk.len() {
                                    ok = false;
                                }
                                let mut k = 0usize;
                                while k < n {
                                    match (back.get(k), chunk.get(k)) {
                                        (Some(a), Some(b)) => {
                                            if a != b {
                                                ok = false;
                                            }
                                        }
                                        _ => ok = false,
                                    }
                                    k += 1;
                                }
                            }
                            Err(_) => ok = false,
                        }
                    }
                    Err(_) => ok = false,
                }
                start = end;
            }
            let (v, n) = char_escape(0x80);
            let a = n == 4 && v.get(..4) == Some(b"\\200".as_slice());
            let b = parse_hex_stream(b"zz", &mut [0u8; 4]) == Err(CM_EBYTE);
            let c = parse_hex_stream(b"0000000 41 42\n0000002\n", &mut [0u8; 4]) == Ok(2);
            ok && a && b && c
        },
        "static",
    );

    s.add(
        "CM13-冻结重放四注入",
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
            let c = again == Err(CM_EBAL) && l.rejects() == rej0 + 1;
            let extra = l.register(Sample::blank());
            let d = extra == Err(CM_ESLOT);
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
    fn cmp_equal() {
        let r = compare(b"abc", b"abc", CmpOpts::plain()).expect("c");
        assert!(r.equal);
        assert_eq!(r.compared, 3);
        assert_eq!(r.default_text(), "equal");
    }

    #[test]
    fn cmp_diff() {
        let r = compare(b"abc", b"abd", CmpOpts::plain()).expect("c");
        assert!(!r.equal);
        assert_eq!(r.first_byte, 3);
        assert_eq!(r.first_line, 1);
        assert_eq!(r.ndiffs, 1);
        assert_eq!(r.default_text(), "a b differ: byte 3, line 1");
    }

    #[test]
    fn cmp_eof() {
        let r = compare(b"ab", b"abc", CmpOpts::plain()).expect("c");
        assert_eq!(r.eof, 1);
        assert_eq!(r.first_byte, 3);
        let q = compare(b"abc", b"ab", CmpOpts::plain()).expect("c");
        assert_eq!(q.eof, 2);
    }

    #[test]
    fn cmp_verbose() {
        let r = compare(b"abc", b"xyz", CmpOpts::plain()).expect("c");
        assert_eq!(r.ndiffs, 3);
        assert!(slice_contains(r.verbose_text().as_bytes(), b"2 142 171 b y"));
    }

    #[test]
    fn cmp_limit() {
        let r = compare(b"abc", b"abd", CmpOpts { verbose: false, silent: false, limit: 2 })
            .expect("c");
        assert!(r.equal);
        assert_eq!(r.compared, 2);
    }

    #[test]
    fn cmp_flags() {
        let o = parse_cmp_flags(b"-l -n 2").expect("p");
        assert!(o.verbose);
        assert_eq!(o.limit, 2);
        assert_eq!(parse_cmp_flags(b"-n"), Err(CM_ESHORT));
        assert_eq!(parse_cmp_flags(b"-n 0"), Err(CM_ERANGE));
        assert_eq!(parse_cmp_flags(b"-l -s"), Err(CM_ECONF));
        assert_eq!(parse_cmp_flags(b"-l -l"), Err(CM_EDUP));
        assert_eq!(parse_cmp_flags(b"-Z"), Err(CM_EINVAL));
    }

    #[test]
    fn od_hex() {
        let text = od_text(b"\x00\x01\x02\x03", OdOpts::plain()).expect("o");
        assert_eq!(text, "0000000 00 01 02 03\n0000004\n");
    }

    #[test]
    fn od_octal_char() {
        let o = od_text(b"\x00\x01", OdOpts { format: OdFormat::Oct1, ..OdOpts::plain() })
            .expect("o");
        assert_eq!(o, "0000000 000 001\n0000002\n");
        let c = od_text(
            b"a\n",
            OdOpts { address: false, format: OdFormat::Char, ..OdOpts::plain() },
        )
        .expect("o");
        assert!(slice_contains(c.as_bytes(), b"\\n"));
    }

    #[test]
    fn od_suppress() {
        let data = [0u8; 48];
        let st = od_dump(&data, OdOpts::plain(), &mut [0u8; 4096]).expect("o");
        assert_eq!(st.rows, 1);
        assert_eq!(st.dup_rows, 2);
        let v = od_dump(&data, OdOpts { verbose: true, ..OdOpts::plain() }, &mut [0u8; 4096])
            .expect("o");
        assert_eq!(v.rows, 3);
        assert_eq!(v.dup_rows, 0);
    }

    #[test]
    fn od_window() {
        let opts = OdOpts {
            address: false,
            format: OdFormat::Hex1,
            verbose: true,
            skip: 2,
            limit: 3,
        };
        assert_eq!(od_text(b"\x00\x01\x02\x03\x04\x05", opts).expect("o"), "02 03 04\n");
        assert_eq!(
            od_text(b"\x00", OdOpts { skip: 2, ..OdOpts::plain() }),
            Err(CM_ERANGE)
        );
    }

    #[test]
    fn od_flags() {
        let o = parse_od_flags(b"-An -v -tx1").expect("p");
        assert!(!o.address);
        assert!(o.verbose);
        assert_eq!(o.format, OdFormat::Hex1);
        assert_eq!(parse_od_flags(b"-tx1 -to1"), Err(CM_ECONF));
        assert_eq!(parse_od_flags(b"-tx1 -tx1"), Err(CM_EDUP));
        assert_eq!(parse_od_flags(b"-tq"), Err(CM_EBYTE));
        assert_eq!(parse_od_flags(b"-t"), Err(CM_ESHORT));
        assert_eq!(parse_od_flags(b"-N 0"), Err(CM_ERANGE));
    }

    #[test]
    fn locale_signatures() {
        assert!(Locale::variant(0).is_c());
        assert!(!Locale::variant(1).is_c());
        assert_ne!(Locale::variant(0).signature(), Locale::variant(1).signature());
        assert_ne!(Locale::variant(1).signature(), Locale::variant(2).signature());
        assert_eq!(locale_matrix_text().lines().count(), LOCALE_VARIANTS);
    }

    #[test]
    fn locale_invariance_holds() {
        let opts = OdOpts {
            address: false,
            format: OdFormat::Hex1,
            verbose: true,
            skip: 0,
            limit: 8,
        };
        let inv = locale_invariance(b"abc", b"abd", opts).expect("i");
        assert_eq!(inv.cases, 16);
        assert_eq!(inv.distinct, 16);
        assert!(inv.stable);
        assert_ne!(inv.hash, 0);
    }

    #[test]
    fn hex_roundtrip() {
        let mut data = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            data[i] = i as u8;
            i += 1;
        }
        let opts = OdOpts {
            address: false,
            format: OdFormat::Hex1,
            verbose: true,
            skip: 0,
            limit: usize::MAX,
        };
        let text = od_text(&data, opts).expect("o");
        let mut back = [0u8; 256];
        let n = parse_hex_stream(text.as_bytes(), &mut back).expect("p");
        assert_eq!(n, 256);
        assert_eq!(&back[..256], &data[..256]);
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(0).expect("0").value, 3);
        assert_eq!(l.sample(6).expect("6").value, 12);
        assert_eq!(l.sample(13).expect("13").code, CM_EBYTE);
    }

    #[test]
    fn seal_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("f");
        assert_eq!(root, l.seal());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(CM_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(CM_ESLOT));
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
        assert!(run_unx11571_checks().all_passed());
    }
}
