//! UNX-F11586 help/--help 双轨判据（help 总表与 <applet> --help usage）
//!
//! BusyBox 的帮助面天然分成两条轨道：
//!
//! * 全量轨：裸 `help`、`--help`、`-h`（单参数）打印
//!   `Currently defined functions:` 总表，按列排布全部 applet；
//! * 用法轨：`help <applet>` 或 `<applet> --help` 打印该 applet 的
//!   `Usage:` 摘要与选项，退出码 0。
//!
//! 两条轨道共用同一份用法书册（`Book`），但渲染器不同；把轨道判据与
//! 文本渲染拆开，是为了在 argv 解析阶段就能拒绝 `help --help`
//! 这类「双轨同时出现」的歧义请求（`HP_ETRACK`），而不是等到渲染
//! 时才发现两个入口互相覆盖。
//!
//! 闸门次序：空参（全量）→ 帮助词 → 未知轨道 → 空名 → 名长 →
//! 选项形名 → applet 存在性 → 用法记录存在性。分类与渲染都按这个
//! 固定次序短路，保证同一输入永远得到同一诊断。
//!
//! 全量表的列排布与 BusyBox 一致：按列优先填充，列宽取最长名加
//! 两格间隔，行数为 `ceil(n / cols)`；每行最后一格不补尾随空格，
//! 因此文本长度只由名字集合与宽度决定，可复现、可对账。
//!
//! 本模块自带 14 格账本：8 格成功观测（解析、全量、用法、排布、
//! 分发、书册规模）与 6 格错误观测（双轨冲突、过长、空名、重复、
//! 列序、缺用法），逐格与 `VALS` / `CODES` 对账，冻结后拒绝写入，
//! 重放必须给出同一指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与容量
// ---------------------------------------------------------------------------

/// 本判据占用的段基址。
pub const SEG_BASE: u32 = 0xf9b0;

/// applet 名上限。
pub const NAME_CAP: usize = 32;
/// 摘要行上限。
pub const SYN_CAP: usize = 64;
/// 选项文本上限。
pub const OPT_CAP: usize = 128;
/// 渲染文本上限。
pub const TEXT_CAP: usize = 256;
/// 用法书册容量。
pub const BOOK_CAP: usize = 12;
/// 全量表目标宽度。
pub const WIDTH: usize = 72;
/// 列间隔。
pub const COL_GAP: usize = 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 全量表表头。
pub const HEADER: &str = "Currently defined functions:\n";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// applet 不存在。
pub const HP_EAPPLET: u32 = SEG_BASE + 0x01;
/// 选项形名称。
pub const HP_EOPT: u32 = SEG_BASE + 0x02;
/// 空名称/空摘要。
pub const HP_EEMPTY: u32 = SEG_BASE + 0x03;
/// 超长/容量越界。
pub const HP_ELEN: u32 = SEG_BASE + 0x04;
/// 用法记录重复。
pub const HP_EDUP: u32 = SEG_BASE + 0x05;
/// 书册缺该 applet 的用法。
pub const HP_ENOUSAGE: u32 = SEG_BASE + 0x06;
/// 列序/行结构违规。
pub const HP_ECOL: u32 = SEG_BASE + 0x07;
/// 双轨冲突或无轨道。
pub const HP_ETRACK: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const HP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const HP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const HP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const HP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const HP_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 请求解析。
pub const OP_PARSE: u8 = 0x01;
/// 全量表渲染。
pub const OP_FULL: u8 = 0x02;
/// 用法渲染。
pub const OP_USAGE: u8 = 0x03;
/// 列排布。
pub const OP_PACK: u8 = 0x04;
/// 帮助分发。
pub const OP_RENDER: u8 = 0x05;
/// 书册指纹。
pub const OP_SEAL: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每字节 8 次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个小整数。
fn fold(h: u64, x: u64) -> u64 {
    let mut v = h;
    let mut n = x;
    let mut k = 0usize;
    while k < 8 {
        v = fold_byte(v, (n & 0xff) as u8);
        n >>= 8;
        k += 1;
    }
    v
}

/// 折入一段字节。
fn fold_bytes(mut h: u64, src: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < src.len() {
        if let Some(b) = src.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 安全的字节视图
// ---------------------------------------------------------------------------

/// 把定容缓冲的前 `len` 字节当作字符串（全模块只写 ASCII）。
fn slice_str(buf: &[u8], len: usize) -> &str {
    match buf.get(..len) {
        Some(b) => core::str::from_utf8(b).unwrap_or(""),
        None => "",
    }
}

/// 首字节是否为 `-`。
fn starts_dash(src: &[u8]) -> bool {
    match src.get(0) {
        Some(b) => *b == b'-',
        None => false,
    }
}

/// 是否为帮助词（`help` / `--help` / `-h`）。
fn is_help_word(src: &[u8]) -> bool {
    src == b"help" || src == b"--help" || src == b"-h"
}

/// 是否含换行。
fn has_newline(src: &[u8]) -> bool {
    let mut i = 0usize;
    while i < src.len() {
        if let Some(b) = src.get(i) {
            if *b == b'\n' {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 定容文本，所有追加都有长度闸门。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    buf: [u8; TEXT_CAP],
    len: usize,
}

impl Text {
    /// 空文本。
    pub const fn new() -> Text {
        Text { buf: [0u8; TEXT_CAP], len: 0 }
    }

    /// 从字节装载。
    pub fn from_bytes(src: &[u8]) -> Result<Text, u32> {
        let mut t = Text::new();
        t.push_bytes(src)?;
        Ok(t)
    }

    /// 追加一个字节。
    pub fn push_byte(&mut self, b: u8) -> Result<(), u32> {
        if self.len >= TEXT_CAP {
            return Err(HP_ELEN);
        }
        match self.buf.get_mut(self.len) {
            Some(dst) => *dst = b,
            None => return Err(HP_ELEN),
        }
        self.len += 1;
        Ok(())
    }

    /// 追加一段字节。
    pub fn push_bytes(&mut self, src: &[u8]) -> Result<(), u32> {
        if self.len.saturating_add(src.len()) > TEXT_CAP {
            return Err(HP_ELEN);
        }
        let mut i = 0usize;
        while i < src.len() {
            match src.get(i) {
                Some(b) => self.push_byte(*b)?,
                None => break,
            }
            i += 1;
        }
        Ok(())
    }

    /// 追加字符串。
    pub fn push_str(&mut self, src: &str) -> Result<(), u32> {
        self.push_bytes(src.as_bytes())
    }

    /// 字节数。
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

    /// 字符串视图。
    pub fn as_str(&self) -> &str {
        slice_str(&self.buf, self.len)
    }

    /// 复制为拥有所有权的字符串（仅诊断/测试路径使用）。
    pub fn owned(&self) -> String {
        String::from(self.as_str())
    }

    /// 以某段字节开头。
    pub fn starts_with(&self, prefix: &[u8]) -> bool {
        let n = prefix.len();
        match self.buf.get(..n) {
            Some(head) => head == prefix && n <= self.len,
            None => false,
        }
    }

    /// 是否包含某段字节。
    pub fn contains(&self, needle: &[u8]) -> bool {
        let n = needle.len();
        if n == 0 {
            return true;
        }
        if n > self.len {
            return false;
        }
        let mut i = 0usize;
        while i.saturating_add(n) <= self.len {
            match self.buf.get(i..i.saturating_add(n)) {
                Some(win) => {
                    if win == needle {
                        return true;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 用法记录
// ---------------------------------------------------------------------------

/// 一条 applet 用法：名、摘要行、选项块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    name: [u8; NAME_CAP],
    name_len: usize,
    synopsis: [u8; SYN_CAP],
    syn_len: usize,
    options: [u8; OPT_CAP],
    opt_len: usize,
}

impl Usage {
    /// 空白记录。
    pub const fn blank() -> Usage {
        Usage {
            name: [0u8; NAME_CAP],
            name_len: 0,
            synopsis: [0u8; SYN_CAP],
            syn_len: 0,
            options: [0u8; OPT_CAP],
            opt_len: 0,
        }
    }

    /// 构造。闸门：名空 → 名长 → 摘要长 → 选项长 → applet 存在 →
    /// 摘要换行 → 选项首字节。
    pub fn new(name: &[u8], synopsis: &[u8], options: &[u8]) -> Result<Usage, u32> {
        if name.is_empty() || synopsis.is_empty() {
            return Err(HP_EEMPTY);
        }
        if name.len() > NAME_CAP || synopsis.len() > SYN_CAP || options.len() > OPT_CAP {
            return Err(HP_ELEN);
        }
        if find_applet(name).is_none() {
            return Err(HP_EAPPLET);
        }
        if has_newline(synopsis) {
            return Err(HP_ECOL);
        }
        if !options.is_empty() && !starts_dash(options) {
            return Err(HP_EOPT);
        }
        let mut u = Usage::blank();
        let mut i = 0usize;
        while i < name.len() {
            match (name.get(i), u.name.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(HP_ELEN),
            }
            i += 1;
        }
        u.name_len = name.len();
        let mut k = 0usize;
        while k < synopsis.len() {
            match (synopsis.get(k), u.synopsis.get_mut(k)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(HP_ELEN),
            }
            k += 1;
        }
        u.syn_len = synopsis.len();
        let mut m = 0usize;
        while m < options.len() {
            match (options.get(m), u.options.get_mut(m)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(HP_ELEN),
            }
            m += 1;
        }
        u.opt_len = options.len();
        Ok(u)
    }

    /// 名称。
    pub fn name(&self) -> &str {
        slice_str(&self.name, self.name_len)
    }

    /// 名称字节。
    pub fn name_bytes(&self) -> &[u8] {
        self.name.get(..self.name_len).unwrap_or(&[])
    }

    /// 摘要行。
    pub fn synopsis(&self) -> &str {
        slice_str(&self.synopsis, self.syn_len)
    }

    /// 选项块。
    pub fn options(&self) -> &str {
        slice_str(&self.options, self.opt_len)
    }

    /// 用法文本：`Usage: <摘要>\n<选项块>`。
    pub fn render(&self) -> Result<Text, u32> {
        let mut t = Text::new();
        t.push_str("Usage: ")?;
        t.push_str(self.synopsis())?;
        t.push_byte(b'\n')?;
        t.push_str(self.options())?;
        Ok(t)
    }
}

// ---------------------------------------------------------------------------
// 标准 applet 清单
// ---------------------------------------------------------------------------

/// applet 名（升序，含未收录用法者）。
pub const APPLETS: [&str; 16] = [
    "ash", "cat", "cp", "dd", "echo", "grep", "ls", "mkdir", "mv", "ps", "rm",
    "sed", "sh", "tar", "wc", "yes",
];

/// 查 applet，返回规范静态名。
pub fn find_applet(name: &[u8]) -> Option<&'static str> {
    let mut i = 0usize;
    while i < APPLETS.len() {
        if let Some(n) = APPLETS.get(i) {
            if n.as_bytes() == name {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 用法书册
// ---------------------------------------------------------------------------

/// 有序用法书册：名严格升序，重复与乱序当场拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Book {
    entries: [Usage; BOOK_CAP],
    len: usize,
}

impl Book {
    /// 空书册。
    pub const fn new() -> Book {
        Book { entries: [Usage::blank(); BOOK_CAP], len: 0 }
    }

    /// 追加。闸门：重复 → 容量 → 升序。
    pub fn add(&mut self, u: Usage) -> Result<usize, u32> {
        if self.find(u.name_bytes()).is_some() {
            return Err(HP_EDUP);
        }
        if self.len >= BOOK_CAP {
            return Err(HP_ELEN);
        }
        if self.len > 0 {
            let prev = match self.entries.get(self.len.saturating_sub(1)) {
                Some(v) => v,
                None => return Err(HP_ECOL),
            };
            if prev.name().as_bytes() >= u.name_bytes() {
                return Err(HP_ECOL);
            }
        }
        match self.entries.get_mut(self.len) {
            Some(dst) => *dst = u,
            None => return Err(HP_ELEN),
        }
        self.len += 1;
        Ok(self.len)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按下标取。
    pub fn at(&self, i: usize) -> Option<&Usage> {
        if i >= self.len {
            return None;
        }
        self.entries.get(i)
    }

    /// 按名查。
    pub fn find(&self, name: &[u8]) -> Option<&Usage> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(u) = self.entries.get(i) {
                if u.name_bytes() == name {
                    return Some(u);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否含某名。
    pub fn has(&self, name: &[u8]) -> bool {
        self.find(name).is_some()
    }

    /// 最长名长度。
    pub fn max_name_len(&self) -> usize {
        let mut max = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(u) = self.entries.get(i) {
                let n = u.name_bytes().len();
                if n > max {
                    max = n;
                }
            }
            i += 1;
        }
        max
    }

    /// 书册指纹：名、摘要、选项依次折入。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(u) = self.entries.get(i) {
                h = fold_bytes(h, u.name_bytes());
                h = fold_bytes(h, u.synopsis().as_bytes());
                h = fold_bytes(h, u.options().as_bytes());
            }
            i += 1;
        }
        h
    }
}

/// 标准书册（12 条，覆盖六个常用族与三个文本工具）。
pub fn standard_book() -> Result<Book, u32> {
    let mut b = Book::new();
    b.add(Usage::new(b"ash", b"ash [-c CMD] [FILE]", b"-c\tcommand\n")?)?;
    b.add(Usage::new(b"cat", b"cat [FILE]...", b"-u\tignored\n")?)?;
    b.add(Usage::new(b"cp", b"cp [-ar] SRC DST", b"-a\tarchive\n-r\trecurse\n")?)?;
    b.add(Usage::new(b"echo", b"echo [STRING]...", b"-n\tno newline\n-e\tescapes\n")?)?;
    b.add(Usage::new(b"grep", b"grep [-i] PATTERN [FILE]...", b"-i\tignore case\n")?)?;
    b.add(Usage::new(b"ls", b"ls [-l] [FILE]...", b"-l\tlong\n-a\tall\n")?)?;
    b.add(Usage::new(b"mkdir", b"mkdir [-p] DIR...", b"-m\tmode\n-p\tparents\n")?)?;
    b.add(Usage::new(b"mv", b"mv SRC DST", b"-f\tforce\n")?)?;
    b.add(Usage::new(b"rm", b"rm [-r] FILE...", b"-r\trecurse\n")?)?;
    b.add(Usage::new(b"sed", b"sed [-n] SCRIPT [FILE]", b"-n\tquiet\n")?)?;
    b.add(Usage::new(b"sh", b"sh [-c CMD]", b"-c\tcommand\n")?)?;
    b.add(Usage::new(b"wc", b"wc [-lwc] [FILE]", b"-l\tlines\n-w\twords\n-c\tbytes\n")?)?;
    Ok(b)
}

// ---------------------------------------------------------------------------
// 轨道与请求
// ---------------------------------------------------------------------------

/// 帮助轨道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// 全量总表。
    Full,
    /// 单 applet 用法。
    Usage,
}

impl Track {
    /// 轨道名。
    pub fn tag(&self) -> &'static str {
        match self {
            Track::Full => "full",
            Track::Usage => "usage",
        }
    }

    /// 轨道码（全量 0，用法 1）。
    pub fn code(&self) -> u8 {
        match self {
            Track::Full => 0,
            Track::Usage => 1,
        }
    }

    /// 帮助请求的退出码恒为 0。
    pub fn exit(&self) -> u8 {
        0
    }
}

/// 一次帮助请求：轨道 + 目标名 + argc。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    track: Track,
    name: [u8; NAME_CAP],
    name_len: usize,
    argc: usize,
}

impl Request {
    /// 全量请求。
    pub const fn full(argc: usize) -> Request {
        Request { track: Track::Full, name: [0u8; NAME_CAP], name_len: 0, argc }
    }

    /// 用法请求。
    pub fn usage(name: &[u8], argc: usize) -> Result<Request, u32> {
        if name.is_empty() {
            return Err(HP_EEMPTY);
        }
        if name.len() > NAME_CAP {
            return Err(HP_ELEN);
        }
        let mut r = Request::full(argc);
        r.track = Track::Usage;
        let mut i = 0usize;
        while i < name.len() {
            match (name.get(i), r.name.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(HP_ELEN),
            }
            i += 1;
        }
        r.name_len = name.len();
        Ok(r)
    }

    /// 轨道。
    pub fn track(&self) -> Track {
        self.track
    }

    /// 目标名。
    pub fn name(&self) -> &str {
        slice_str(&self.name, self.name_len)
    }

    /// 目标名字节。
    pub fn name_bytes(&self) -> &[u8] {
        self.name.get(..self.name_len).unwrap_or(&[])
    }

    /// 普通参数个数。
    pub fn argc(&self) -> usize {
        self.argc
    }

    /// 是否全量轨。
    pub fn is_full(&self) -> bool {
        self.track == Track::Full
    }
}

/// 解析帮助入口。
///
/// 接受的形式：
/// * `[]` / `help` / `--help` / `-h` → 全量；
/// * `help <applet>` / `<applet> --help` / `<applet> -h` → 用法。
///
/// 拒绝：双轨同时出现（`HP_ETRACK`）、过长（`HP_ELEN`）、空名
/// （`HP_EEMPTY`）、`help -x` 形式（`HP_EOPT`）。
pub fn parse(args: &[&[u8]]) -> Result<Request, u32> {
    let argc = args.len();
    if argc == 0 {
        return Ok(Request::full(0));
    }
    let first = match args.get(0) {
        Some(v) => *v,
        None => return Ok(Request::full(0)),
    };
    if first.is_empty() {
        return Err(HP_EEMPTY);
    }
    if argc == 1 {
        if is_help_word(first) {
            return Ok(Request::full(1));
        }
        return Err(HP_ETRACK);
    }
    if argc > 2 {
        return Err(HP_ELEN);
    }
    let second = match args.get(1) {
        Some(v) => *v,
        None => return Err(HP_ETRACK),
    };
    if second.is_empty() {
        return Err(HP_EEMPTY);
    }
    if first == b"help" {
        if is_help_word(second) {
            return Err(HP_ETRACK);
        }
        if starts_dash(second) {
            return Err(HP_EOPT);
        }
        return Request::usage(second, 2);
    }
    if is_help_word(second) {
        if starts_dash(first) {
            return Err(HP_ETRACK);
        }
        return Request::usage(first, 2);
    }
    Err(HP_ETRACK)
}

// ---------------------------------------------------------------------------
// 列排布与渲染
// ---------------------------------------------------------------------------

/// 计算 `(有效列数, 行数, 列宽)`；列优先填充。
pub fn pack_shape(n: usize, width: usize, maxlen: usize) -> (usize, usize, usize) {
    if n == 0 {
        return (0, 0, 0);
    }
    let colw = maxlen.saturating_add(COL_GAP);
    let colw = if colw == 0 { 1 } else { colw };
    let mut cols = width / colw;
    if cols == 0 {
        cols = 1;
    }
    if cols > n {
        cols = n;
    }
    let rows = n.saturating_add(cols).saturating_sub(1) / cols;
    let rows = if rows == 0 { 1 } else { rows };
    let mut eff = n.saturating_add(rows).saturating_sub(1) / rows;
    if eff == 0 {
        eff = 1;
    }
    (eff, rows, colw)
}

/// 全量表：表头 + 按列排布的 applet 名。
pub fn full_help(book: &Book) -> Result<Text, u32> {
    let mut t = Text::new();
    t.push_str(HEADER)?;
    let n = book.len();
    if n == 0 {
        return Ok(t);
    }
    let (cols, rows, colw) = pack_shape(n, WIDTH, book.max_name_len());
    let mut r = 0usize;
    while r < rows {
        let mut c = 0usize;
        while c < cols {
            let idx = c.saturating_mul(rows).saturating_add(r);
            if idx < n {
                if let Some(u) = book.at(idx) {
                    t.push_str(u.name())?;
                    let next = c.saturating_add(1).saturating_mul(rows).saturating_add(r);
                    if next < n {
                        let mut pad = u.name_bytes().len();
                        while pad < colw {
                            t.push_byte(b' ')?;
                            pad += 1;
                        }
                    }
                }
            }
            c += 1;
        }
        t.push_byte(b'\n')?;
        r += 1;
    }
    Ok(t)
}

/// 单 applet 用法。闸门：空名 → applet 存在 → 书册有记录。
pub fn usage_help(book: &Book, name: &[u8]) -> Result<Text, u32> {
    if name.is_empty() {
        return Err(HP_EEMPTY);
    }
    if find_applet(name).is_none() {
        return Err(HP_EAPPLET);
    }
    match book.find(name) {
        Some(u) => u.render(),
        None => Err(HP_ENOUSAGE),
    }
}

/// 分发结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Help {
    track: Track,
    body: Text,
}

impl Help {
    /// 轨道。
    pub fn track(&self) -> Track {
        self.track
    }

    /// 正文。
    pub fn body(&self) -> &Text {
        &self.body
    }

    /// 正文长度。
    pub fn len(&self) -> usize {
        self.body.len()
    }

    /// 是否空正文。
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// 退出码。
    pub fn exit(&self) -> u8 {
        self.track.exit()
    }
}

/// 按轨道分发到对应渲染器。
pub fn plan(req: &Request, book: &Book) -> Result<Help, u32> {
    match req.track() {
        Track::Full => {
            let body = full_help(book)?;
            Ok(Help { track: Track::Full, body })
        }
        Track::Usage => {
            let body = usage_help(book, req.name_bytes())?;
            Ok(Help { track: Track::Usage, body })
        }
    }
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格观测。
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
            return Err(HP_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(HP_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 105, 40, 38, 40, 12];
        const CODES: [u32; 6] = [
            HP_ETRACK,
            HP_ELEN,
            HP_EEMPTY,
            HP_EDUP,
            HP_ECOL,
            HP_ENOUSAGE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(HP_EBAL),
            };
            if s.slot as usize != i {
                return Err(HP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(HP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(HP_EVALUE);
                        }
                    }
                    None => return Err(HP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(HP_EVALUE);
                        }
                    }
                    None => return Err(HP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(HP_EBAL);
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
            return Err(HP_EBAL);
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
            Err(HP_ESEAL)
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

/// 格构造。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 全量表长度。
fn full_len() -> Result<usize, u32> {
    let b = standard_book()?;
    full_help(&b).map(|t| t.len())
}

/// 用法文本长度。
fn usage_len() -> Result<usize, u32> {
    let b = standard_book()?;
    usage_help(&b, b"ls").map(|t| t.len())
}

/// 列排布编码：行数 × 16 + 列数。
fn pack_code() -> Result<usize, u32> {
    let b = standard_book()?;
    let (cols, rows, _) = pack_shape(b.len(), WIDTH, b.max_name_len());
    Ok(rows.saturating_mul(16).saturating_add(cols))
}

/// 规划正文长度。
fn plan_len() -> Result<usize, u32> {
    let b = standard_book()?;
    let req = parse(&[b"help", b"ls"])?;
    plan(&req, &b).map(|h| h.len())
}

/// 书册规模。
fn book_len() -> Result<usize, u32> {
    standard_book().map(|b| b.len())
}

/// 重复追加。
fn dup_add() -> Result<usize, u32> {
    let mut b = standard_book()?;
    let u = Usage::new(b"ls", b"ls [FILE]", b"")?;
    b.add(u)?;
    Ok(b.len())
}

/// 乱序追加。
fn order_add() -> Result<usize, u32> {
    let mut b = Book::new();
    let first = Usage::new(b"ls", b"ls [FILE]", b"")?;
    b.add(first)?;
    let second = Usage::new(b"cat", b"cat [FILE]", b"")?;
    b.add(second)?;
    Ok(b.len())
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(
        0,
        OP_PARSE,
        parse(&[]).map(|r| r.argc()),
    ));
    let _ = l.register(cell(
        1,
        OP_PARSE,
        parse(&[b"help", b"ls"]).map(|r| r.track().code() as usize),
    ));
    let _ = l.register(cell(
        2,
        OP_PARSE,
        parse(&[b"ls", b"--help"]).map(|r| r.name_bytes().len()),
    ));
    let _ = l.register(cell(3, OP_FULL, full_len()));
    let _ = l.register(cell(4, OP_USAGE, usage_len()));
    let _ = l.register(cell(5, OP_PACK, pack_code()));
    let _ = l.register(cell(6, OP_RENDER, plan_len()));
    let _ = l.register(cell(7, OP_SEAL, book_len()));

    let _ = l.register(cell(
        8,
        OP_PARSE,
        parse(&[b"help", b"--help"]).map(|r| r.argc()),
    ));
    let _ = l.register(cell(
        9,
        OP_PARSE,
        parse(&[b"help", b"ls", b"x"]).map(|r| r.argc()),
    ));
    let _ = l.register(cell(
        10,
        OP_PARSE,
        parse(&[b"help", b""]).map(|r| r.argc()),
    ));
    let _ = l.register(cell(11, OP_USAGE, dup_add()));
    let _ = l.register(cell(12, OP_USAGE, order_add()));
    let _ = l.register(cell(
        13,
        OP_USAGE,
        match standard_book() {
            Ok(b) => usage_help(&b, b"dd").map(|t| t.len()),
            Err(c) => Err(c),
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 请求解析面。
fn request_ok() -> bool {
    let a = parse(&[]).map(|r| r.is_full()) == Ok(true);
    let b = parse(&[b"help"]).map(|r| r.is_full()) == Ok(true);
    let c = parse(&[b"--help"]).map(|r| r.is_full()) == Ok(true);
    let d = parse(&[b"-h"]).map(|r| r.is_full()) == Ok(true);
    let e = parse(&[b"help", b"ls"]).map(|r| r.name() == "ls") == Ok(true);
    let f = parse(&[b"ls", b"--help"]).map(|r| r.name() == "ls") == Ok(true);
    let g = parse(&[b"ls", b"-h"]).map(|r| r.name() == "ls") == Ok(true);
    let h = parse(&[b"ls"]).err() == Some(HP_ETRACK);
    let i = parse(&[b"help", b"ls", b"x"]).err() == Some(HP_ELEN);
    let j = parse(&[b"help", b"--help"]).err() == Some(HP_ETRACK);
    let k = parse(&[b"--help", b"ls"]).err() == Some(HP_ETRACK);
    let l = parse(&[b"help", b""]).err() == Some(HP_EEMPTY);
    let m = parse(&[b""]).err() == Some(HP_EEMPTY);
    let n = parse(&[b"help", b"-x"]).err() == Some(HP_EOPT);
    a && b && c && d && e && f && g && h && i && j && k && l && m && n
}

/// 轨道面。
fn track_ok() -> bool {
    let full = Track::Full;
    let usage = Track::Usage;
    full.tag() == "full"
        && usage.tag() == "usage"
        && full.code() == 0
        && usage.code() == 1
        && full.exit() == 0
        && usage.exit() == 0
        && Request::full(0).is_full()
        && !Request::usage(b"ls", 2).map(|r| r.is_full()).unwrap_or(true)
        && Request::usage(b"", 2).err() == Some(HP_EEMPTY)
        && Request::usage(&[b'a'; NAME_CAP + 1], 2).err() == Some(HP_ELEN)
}

/// 全量表渲染面。
fn full_ok() -> bool {
    let b = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match full_help(&b) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !t.starts_with(HEADER.as_bytes()) {
        return false;
    }
    if t.len() != 105 || !t.as_str().ends_with('\n') {
        return false;
    }
    if !t.contains(b"ash") || !t.contains(b"mkdir") || !t.contains(b"wc") {
        return false;
    }
    if t.contains(b"dd") || t.contains(b"tar") || t.contains(b"yes") {
        return false;
    }
    if !t.as_str().contains("ash    cp") {
        return false;
    }
    let empty = Book::new();
    match full_help(&empty) {
        Ok(v) => v.as_str() == HEADER,
        Err(_) => false,
    }
}

/// 列排布面。
fn pack_ok() -> bool {
    let a = pack_shape(12, WIDTH, 5) == (6, 2, 7);
    let b = pack_shape(0, WIDTH, 0) == (0, 0, 0);
    let c = pack_shape(3, WIDTH, 5) == (3, 1, 7);
    let d = pack_shape(4, 6, 5) == (1, 4, 7);
    let e = pack_shape(1, WIDTH, 0) == (1, 1, 2);
    let b2 = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let (cols, rows, colw) = pack_shape(b2.len(), WIDTH, b2.max_name_len());
    cols == 6 && rows == 2 && colw == 7 && b2.max_name_len() == 5
        && a && b && c && d && e
}

/// 用法文本面。
fn usage_ok() -> bool {
    let b = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match usage_help(&b, b"ls") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t.as_str() != "Usage: ls [-l] [FILE]...\n-l\tlong\n-a\tall\n" {
        return false;
    }
    if t.len() != 40 {
        return false;
    }
    let ash = match usage_help(&b, b"ash") {
        Ok(v) => v,
        Err(_) => return false,
    };
    ash.as_str().starts_with("Usage: ash [-c CMD]")
        && ash.as_str().contains("-c\tcommand\n")
        && usage_help(&b, b"ls").map(|v| v.len()) == Ok(40)
}

/// 分发面。
fn plan_ok() -> bool {
    let b = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let full_req = match parse(&[b"--help"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let full = match plan(&full_req, &b) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if full.track() != Track::Full || full.len() != 105 || full.exit() != 0 {
        return false;
    }
    let use_req = match parse(&[b"ls", b"--help"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let u = match plan(&use_req, &b) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if u.track() != Track::Usage || u.len() != 40 || u.is_empty() {
        return false;
    }
    let bare = match parse(&[b"help"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    plan(&bare, &b).map(|h| h.track()) == Ok(Track::Full)
}

/// 缺失用法面。
fn missing_ok() -> bool {
    let b = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if usage_help(&b, b"dd").err() != Some(HP_ENOUSAGE) {
        return false;
    }
    if usage_help(&b, b"yes").err() != Some(HP_ENOUSAGE) {
        return false;
    }
    if usage_help(&b, b"zz").err() != Some(HP_EAPPLET) {
        return false;
    }
    if usage_help(&b, b"").err() != Some(HP_EEMPTY) {
        return false;
    }
    let req = match Request::usage(b"dd", 2) {
        Ok(v) => v,
        Err(_) => return false,
    };
    plan(&req, &b).err() == Some(HP_ENOUSAGE)
        && find_applet(b"dd") == Some("dd")
        && find_applet(b"zz").is_none()
        && b.has(b"ls")
        && !b.has(b"dd")
}

/// 书册闸门面。
fn book_ok() -> bool {
    let a = Usage::new(b"", b"x", b"").err() == Some(HP_EEMPTY);
    let b = Usage::new(b"ls", b"", b"").err() == Some(HP_EEMPTY);
    let c = Usage::new(&[b'a'; NAME_CAP + 1], b"x", b"").err() == Some(HP_ELEN);
    let d = Usage::new(b"ls", &[b'x'; SYN_CAP + 1], b"").err() == Some(HP_ELEN);
    let e = Usage::new(b"ls", b"ls", &[b'x'; OPT_CAP + 1]).err() == Some(HP_ELEN);
    let f = Usage::new(b"zz", b"zz", b"").err() == Some(HP_EAPPLET);
    let g = Usage::new(b"ls", b"ls\nFILE", b"").err() == Some(HP_ECOL);
    let h = Usage::new(b"ls", b"ls", b"x").err() == Some(HP_EOPT);
    let mut book = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let dup = match Usage::new(b"ls", b"ls [FILE]", b"") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if book.add(dup) != Err(HP_EDUP) {
        return false;
    }
    let mut ordered = Book::new();
    let first = match Usage::new(b"ls", b"ls", b"") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let second = match Usage::new(b"cat", b"cat", b"") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if ordered.add(first) != Ok(1) || ordered.add(second) != Err(HP_ECOL) {
        return false;
    }
    let extra = match Usage::new(b"yes", b"yes", b"") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if book.add(extra) != Err(HP_ELEN) {
        return false;
    }
    a && b && c && d && e && f && g && h && book.len() == BOOK_CAP
}

/// 文本面。
fn text_ok() -> bool {
    let mut t = Text::new();
    if !t.is_empty() || t.len() != 0 {
        return false;
    }
    if t.push_str("Usage: ").is_err() || t.len() != 7 {
        return false;
    }
    if t.push_byte(b'x').is_err() || t.as_str() != "Usage: x" {
        return false;
    }
    if t.push_bytes(b"yz").is_err() || t.as_str() != "Usage: xyz" {
        return false;
    }
    if t.owned().as_str() != t.as_str() {
        return false;
    }
    if !t.starts_with(b"Usage") || !t.contains(b"x") || t.contains(b"q") {
        return false;
    }
    if Text::from_bytes(&[b'a'; TEXT_CAP + 1]).err() != Some(HP_ELEN) {
        return false;
    }
    let mut full = match Text::from_bytes(&[b'a'; TEXT_CAP]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if full.len() != TEXT_CAP || full.push_byte(b'b') != Err(HP_ELEN) {
        return false;
    }
    full.len() == TEXT_CAP && full.as_bytes().len() == TEXT_CAP
}

/// 书册指纹面。
fn seal_ok() -> bool {
    let a = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match standard_book() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if a.seal() != b.seal() || a.seal() == 0 {
        return false;
    }
    let mut c = Book::new();
    let u = match Usage::new(b"ls", b"ls [FILE]", b"-l\tlong\n") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if c.add(u) != Ok(1) {
        return false;
    }
    let mut d = Book::new();
    let v = match Usage::new(b"ls", b"ls [-l] [FILE]...", b"-l\tlong\n") {
        Ok(v) => v,
        Err(_) => return false,
    };
    if d.add(v) != Ok(1) {
        return false;
    }
    c.seal() != d.seal() && a.at(5).map(|x| x.name()) == Some("ls")
}

/// 双轨冲突面。
fn conflict_ok() -> bool {
    let a = parse(&[b"help", b"--help"]).err() == Some(HP_ETRACK);
    let b = parse(&[b"help", b"-h"]).err() == Some(HP_ETRACK);
    let c = parse(&[b"--help", b"ls"]).err() == Some(HP_ETRACK);
    let d = parse(&[b"-h", b"cat"]).err() == Some(HP_ETRACK);
    let e = parse(&[b"ls", b"-l"]).err() == Some(HP_ETRACK);
    let f = parse(&[b"-x", b"--help"]).err() == Some(HP_ETRACK);
    let g = parse(&[b"help", b"ls"]).map(|r| r.track()) == Ok(Track::Usage);
    let h = parse(&[b"ls", b"--help"]).map(|r| r.track()) == Ok(Track::Usage);
    let i = parse(&[b"--help"]).map(|r| r.track()) == Ok(Track::Full);
    a && b && c && d && e && f && g && h && i
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
    if l2.replay() != Ok(root) {
        return false;
    }
    if l2.register(Sample::blank()) != Err(HP_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = request_ok() && track_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = full_ok() && pack_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = usage_ok() && plan_ok() && missing_ok() && book_ok() && text_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = seal_ok() && conflict_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11586_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11586.help");

    s.add("HP01-请求解析", request_ok(), "static");
    s.add("HP02-轨道识别", track_ok(), "static");
    s.add("HP03-全量清单", full_ok(), "static");
    s.add("HP04-列排布", pack_ok(), "static");
    s.add("HP05-用法文本", usage_ok(), "static");
    s.add("HP06-规划分发", plan_ok(), "static");
    s.add("HP07-缺失用法", missing_ok(), "static");
    s.add("HP08-书册闸门", book_ok(), "static");
    s.add("HP09-文本工具", text_ok(), "static");
    s.add("HP10-书册封印", seal_ok(), "static");
    s.add("HP11-双轨冲突", conflict_ok(), "static");
    s.add("HP12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "HP13-冻结重放四注入",
        {
            let four = inject_four();
            let mut all = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(v) = four.get(i) {
                    if !*v {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_request_is_full() {
        assert_eq!(parse(&[]).map(|r| r.track()), Ok(Track::Full));
        assert_eq!(parse(&[b"help"]).map(|r| r.track()), Ok(Track::Full));
        assert_eq!(parse(&[b"--help"]).map(|r| r.track()), Ok(Track::Full));
        assert_eq!(parse(&[b"-h"]).map(|r| r.track()), Ok(Track::Full));
    }

    #[test]
    fn usage_request_forms() {
        assert_eq!(parse(&[b"help", b"ls"]).map(|r| r.name() == "ls"), Ok(true));
        assert_eq!(parse(&[b"ls", b"--help"]).map(|r| r.name() == "ls"), Ok(true));
        assert_eq!(parse(&[b"ls", b"-h"]).map(|r| r.name() == "ls"), Ok(true));
        assert_eq!(parse(&[b"help", b"grep"]).map(|r| r.argc()), Ok(2));
    }

    #[test]
    fn dual_track_conflicts() {
        assert_eq!(parse(&[b"help", b"--help"]).err(), Some(HP_ETRACK));
        assert_eq!(parse(&[b"--help", b"ls"]).err(), Some(HP_ETRACK));
        assert_eq!(parse(&[b"-h", b"cat"]).err(), Some(HP_ETRACK));
        assert_eq!(parse(&[b"ls", b"-l"]).err(), Some(HP_ETRACK));
    }

    #[test]
    fn request_rejections() {
        assert_eq!(parse(&[b"help", b"ls", b"x"]).err(), Some(HP_ELEN));
        assert_eq!(parse(&[b"help", b""]).err(), Some(HP_EEMPTY));
        assert_eq!(parse(&[b""]).err(), Some(HP_EEMPTY));
        assert_eq!(parse(&[b"help", b"-x"]).err(), Some(HP_EOPT));
    }

    #[test]
    fn tracks_carry_tags_and_codes() {
        assert_eq!(Track::Full.tag(), "full");
        assert_eq!(Track::Usage.tag(), "usage");
        assert_eq!(Track::Full.code(), 0);
        assert_eq!(Track::Usage.code(), 1);
        assert_eq!(Track::Usage.exit(), 0);
    }

    #[test]
    fn text_gates_length() {
        let mut t = Text::new();
        assert!(t.is_empty());
        assert_eq!(t.push_str("abc"), Ok(()));
        assert_eq!(t.as_str(), "abc");
        assert_eq!(Text::from_bytes(&[b'a'; TEXT_CAP + 1]).err(), Some(HP_ELEN));
        let mut full = Text::from_bytes(&[b'a'; TEXT_CAP]).unwrap_or_else(|_| Text::new());
        assert_eq!(full.push_byte(b'z'), Err(HP_ELEN));
    }

    #[test]
    fn text_views_are_stable() {
        let t = Text::from_bytes(b"Usage: ls").unwrap_or_else(|_| Text::new());
        assert!(t.starts_with(b"Usage"));
        assert!(t.contains(b": l"));
        assert!(!t.contains(b"rm"));
        assert_eq!(t.as_bytes().len(), 9);
    }

    #[test]
    fn usage_gates() {
        assert_eq!(Usage::new(b"", b"x", b"").err(), Some(HP_EEMPTY));
        assert_eq!(Usage::new(b"ls", b"", b"").err(), Some(HP_EEMPTY));
        assert_eq!(Usage::new(b"zz", b"zz", b"").err(), Some(HP_EAPPLET));
        assert_eq!(Usage::new(b"ls", b"ls\nx", b"").err(), Some(HP_ECOL));
        assert_eq!(Usage::new(b"ls", b"ls", b"x").err(), Some(HP_EOPT));
    }

    #[test]
    fn usage_render_is_exact() {
        let b = standard_book().unwrap_or_else(|_| Book::new());
        let t = usage_help(&b, b"ls").unwrap_or_else(|_| Text::new());
        assert_eq!(t.as_str(), "Usage: ls [-l] [FILE]...\n-l\tlong\n-a\tall\n");
        assert_eq!(t.len(), 40);
    }

    #[test]
    fn book_add_is_sorted_and_unique() {
        let mut b = Book::new();
        let ls = Usage::new(b"ls", b"ls", b"").unwrap_or_else(|_| Usage::blank());
        assert_eq!(b.add(ls), Ok(1));
        assert_eq!(b.add(ls), Err(HP_EDUP));
        let cat = Usage::new(b"cat", b"cat", b"").unwrap_or_else(|_| Usage::blank());
        assert_eq!(b.add(cat), Err(HP_ECOL));
        assert!(b.has(b"ls"));
        assert!(!b.has(b"cat"));
    }

    #[test]
    fn book_capacity_rejects() {
        let mut b = standard_book().unwrap_or_else(|_| Book::new());
        assert_eq!(b.len(), BOOK_CAP);
        let yes = Usage::new(b"yes", b"yes", b"").unwrap_or_else(|_| Usage::blank());
        assert_eq!(b.add(yes), Err(HP_ELEN));
    }

    #[test]
    fn full_help_is_stable() {
        let b = standard_book().unwrap_or_else(|_| Book::new());
        let t = full_help(&b).unwrap_or_else(|_| Text::new());
        assert_eq!(t.len(), 105);
        assert!(t.as_str().starts_with("Currently defined functions:\n"));
        assert!(t.as_str().contains("ash    cp"));
        assert!(t.as_str().ends_with("wc\n"));
    }

    #[test]
    fn packing_is_column_major() {
        assert_eq!(pack_shape(12, WIDTH, 5), (6, 2, 7));
        assert_eq!(pack_shape(0, WIDTH, 0), (0, 0, 0));
        assert_eq!(pack_shape(4, 6, 5), (1, 4, 7));
        let b = standard_book().unwrap_or_else(|_| Book::new());
        assert_eq!(pack_shape(b.len(), WIDTH, b.max_name_len()), (6, 2, 7));
    }

    #[test]
    fn usage_lookup_reports_missing_record() {
        let b = standard_book().unwrap_or_else(|_| Book::new());
        assert_eq!(usage_help(&b, b"dd").err(), Some(HP_ENOUSAGE));
        assert_eq!(usage_help(&b, b"zz").err(), Some(HP_EAPPLET));
        assert_eq!(usage_help(&b, b"").err(), Some(HP_EEMPTY));
        assert_eq!(find_applet(b"tar"), Some("tar"));
    }

    #[test]
    fn plan_dispatches_both_tracks() {
        let b = standard_book().unwrap_or_else(|_| Book::new());
        let req = parse(&[b"--help"]).unwrap_or_else(|_| Request::full(0));
        let h = plan(&req, &b).unwrap_or_else(|_| Help { track: Track::Usage, body: Text::new() });
        assert_eq!(h.track(), Track::Full);
        assert_eq!(h.len(), 105);
        let req2 = parse(&[b"help", b"ls"]).unwrap_or_else(|_| Request::full(0));
        let h2 = plan(&req2, &b).unwrap_or_else(|_| Help { track: Track::Full, body: Text::new() });
        assert_eq!(h2.track(), Track::Usage);
        assert_eq!(h2.len(), 40);
    }

    #[test]
    fn book_seal_is_deterministic() {
        let a = standard_book().unwrap_or_else(|_| Book::new());
        let b = standard_book().unwrap_or_else(|_| Book::new());
        assert_eq!(a.seal(), b.seal());
        assert_ne!(a.seal(), 0);
    }

    #[test]
    fn ledger_shape_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(0));
        assert_eq!(l.sample(3).map(|s| s.value), Some(105));
        assert_eq!(l.sample(13).map(|s| s.code), Some(HP_ENOUSAGE));
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freezes_and_replays() {
        let mut l = build_ledger();
        let root = l.freeze().unwrap_or(0);
        assert!(root != 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.root(), root);
        assert_eq!(l.register(Sample::blank()), Err(HP_ESLOT));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11586_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
