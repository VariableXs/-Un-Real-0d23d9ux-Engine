//! UNX-F11555 vdir/dircolors 目录呈现件
//!
//! `vdir` 是 `ls -l` 的长格式别名，`dircolors` 是「色彩数据库 → `LS_COLORS` 串」
//! 的编译器。两者在基准机上其实是同一套东西的两面：`vdir` 决定**每一行怎么排**，
//! `dircolors` 决定**每种条目染什么色**。因此本件把两半做进一张账本：
//!
//! 1. **SGR 解析/渲染**：`01;34` 这类属性串要逐段校验——非数字、超三位、越界
//!    （>107）分别是不同闸门；渲染回成定宽两段式（不足两位补零），才能和基准机
//!    逐字节相等。空串是「选项没给值」，和坏字节必须分开报。
//! 2. **色彩库**：类目名只认小写字母，重复登记必须 `VD_EDUP` 而不是后写覆盖；
//!    输出有 `LS_COLORS='...'; export LS_COLORS` 与 csh 的 `setenv` 两种壳式，
//!    顺序严格按登记序，冒号既不领先也不结尾。
//! 3. **长格式列**：模式串十字位（含 `s/S/t/T` 替换规则）、链接数、属主/属组、
//!    尺寸（`-h` 走 1K 进位且向上取整）、时间、名字（软链追加 ` -> target`）；
//!    列宽取全场最大值再右对齐，`total` 行按 1K 块逐个向上取整后累加。
//!
//! 闸门次序：选项字面（`VD_EINVAL`）→ 选项/行长度（`VD_ELEN`）→ 字节合法性
//! （`VD_EBYTE`）→ 数值范围（`VD_ERANGE`）→ 重复（`VD_EDUP`）→ 类目未知
//! （`VD_ENOENT`）→ 色彩串不自洽（`VD_ECOLOR`）→ 选项冲突（`VD_ESTATE`）。
//! 长度在字节之前：`1234` 是「段太长」而不是「有坏字符」，先报前者才对得上。
//!
//! 账本语义：十四格 = 八格成功（属性段数、属性渲染、默认库类目、`LS_COLORS`
//! 串、sh 壳式、csh 壳式、类目判定、模式串）+ 六格失败（坏字节、超范围、空串、
//! 重复类目、未知类目、选项冲突）。`audit` 认「八值六码」，`seal` 做 FNV-1a
//! 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3c0;

/// 选项字面非法。
pub const VD_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项或行过长。
pub const VD_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法。
pub const VD_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界。
pub const VD_ERANGE: u32 = SEG_BASE + 0x04;
/// 类目重复。
pub const VD_EDUP: u32 = SEG_BASE + 0x05;
/// 类目未知。
pub const VD_ENOENT: u32 = SEG_BASE + 0x06;
/// 色彩串不自洽。
pub const VD_ECOLOR: u32 = SEG_BASE + 0x07;
/// 选项组合冲突。
pub const VD_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const VD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const VD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const VD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const VD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const VD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 类目名容量。
pub const NAMECAP: usize = 24;

/// 色彩库容量。
pub const DBCAP: usize = 32;

/// SGR 段数上限。
pub const SGMAX: usize = 8;

/// SGR 串长上限。
pub const SGLCAP: usize = 48;

/// 单行配置长度上限。
pub const LINECAP: usize = 96;

/// 参数个数上限。
pub const ARGCAP: usize = 16;

/// 长格式行数上限。
pub const ROWCAP: usize = 8;

/// SGR 数值上限（background bright white）。
pub const SGR_MAX: u32 = 107;

/// 操作码：属性解析。
pub const OP_SGR: u8 = 0x01;
/// 操作码：属性渲染。
pub const OP_RENDER: u8 = 0x02;
/// 操作码：色彩库。
pub const OP_DB: u8 = 0x03;
/// 操作码：LS_COLORS 串。
pub const OP_LSCOLORS: u8 = 0x04;
/// 操作码：sh 壳式。
pub const OP_SH: u8 = 0x05;
/// 操作码：csh 壳式。
pub const OP_CSH: u8 = 0x06;
/// 操作码：类目判定。
pub const OP_CLASS: u8 = 0x07;
/// 操作码：模式串。
pub const OP_MODE: u8 = 0x08;
/// 操作码：长格式。
pub const OP_LONG: u8 = 0x09;
/// 操作码：选项。
pub const OP_OPT: u8 = 0x0a;

/// 条目种类：普通文件。
pub const K_REG: u8 = 0x01;
/// 条目种类：目录。
pub const K_DIR: u8 = 0x02;
/// 条目种类：符号链接。
pub const K_LINK: u8 = 0x03;
/// 条目种类：命名管道。
pub const K_FIFO: u8 = 0x04;
/// 条目种类：套接字。
pub const K_SOCK: u8 = 0x05;
/// 条目种类：块设备。
pub const K_BLK: u8 = 0x06;
/// 条目种类：字符设备。
pub const K_CHR: u8 = 0x07;

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

/// 十进制拼接（无符号）。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(d) = buf.get(k) {
            s.push(*d as char);
        }
        k += 1;
    }
}

/// 十进制文本。
fn dec_text(v: u64) -> String {
    let mut s = String::new();
    push_dec(&mut s, v);
    s
}

/// 字节串文本（仅接纳 ASCII 可打印，其余按 `?`）。
fn bytes_text(raw: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        match raw.get(i) {
            Some(b) => {
                let c = *b;
                if c >= 0x20 && c < 0x7f {
                    s.push(c as char);
                } else {
                    s.push('?');
                }
            }
            None => break,
        }
        i += 1;
    }
    s
}

/// 右对齐填充。
fn pad_left(s: &mut String, text: &str, width: usize) {
    let n = text.len();
    if n < width {
        let mut k = n;
        while k < width {
            s.push(' ');
            k += 1;
        }
    }
    s.push_str(text);
}

/// 左对齐填充。
fn pad_right(s: &mut String, text: &str, width: usize) {
    s.push_str(text);
    let n = text.len();
    if n < width {
        let mut k = n;
        while k < width {
            s.push(' ');
            k += 1;
        }
    }
}

/// 类目名（`rs`/`di`/`ln` 这类两字母键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cat {
    buf: [u8; NAMECAP],
    len: u8,
}

impl Cat {
    /// 空类目。
    pub const fn empty() -> Cat {
        Cat { buf: [0; NAMECAP], len: 0 }
    }

    /// 已知类目直构（用于判定结果）。
    pub fn of(tag: &[u8; 2]) -> Cat {
        let mut buf = [0u8; NAMECAP];
        if let (Some(d), Some(s)) = (buf.get_mut(0), tag.first()) {
            *d = *s;
        }
        if let (Some(d), Some(s)) = (buf.get_mut(1), tag.get(1)) {
            *d = *s;
        }
        Cat { buf, len: 2 }
    }

    /// 解析类目名：非空、≤`NAMECAP`、全小写字母。
    pub fn parse(raw: &[u8]) -> Result<Cat, u32> {
        if raw.is_empty() {
            return Err(VD_EINVAL);
        }
        if raw.len() > NAMECAP {
            return Err(VD_ELEN);
        }
        let mut buf = [0u8; NAMECAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    let c = *b;
                    if c < b'a' || c > b'z' {
                        return Err(VD_EBYTE);
                    }
                    if let Some(d) = buf.get_mut(i) {
                        *d = c;
                    }
                }
                None => return Err(VD_EBYTE),
            }
            i += 1;
        }
        Ok(Cat { buf, len: raw.len() as u8 })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<Cat, u32> {
        Cat::parse(text.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len as usize).unwrap_or(&[])
    }

    /// 字节相等。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 文本。
    pub fn text(&self) -> String {
        bytes_text(self.as_bytes())
    }

    /// 已知类目的稳定序号；未知类目落到 15。
    pub fn code(&self) -> u8 {
        if self.eq_bytes(b"rs") {
            0
        } else if self.eq_bytes(b"fi") {
            1
        } else if self.eq_bytes(b"di") {
            2
        } else if self.eq_bytes(b"ln") {
            3
        } else if self.eq_bytes(b"ex") {
            4
        } else if self.eq_bytes(b"so") {
            5
        } else if self.eq_bytes(b"bd") {
            6
        } else if self.eq_bytes(b"cd") {
            7
        } else if self.eq_bytes(b"or") {
            8
        } else if self.eq_bytes(b"mi") {
            9
        } else if self.eq_bytes(b"su") {
            10
        } else if self.eq_bytes(b"sg") {
            11
        } else if self.eq_bytes(b"tw") {
            12
        } else if self.eq_bytes(b"ow") {
            13
        } else if self.eq_bytes(b"st") {
            14
        } else {
            15
        }
    }
}

/// SGR 属性串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sgr {
    codes: [u8; SGMAX],
    len: u8,
}

impl Sgr {
    /// 空属性。
    pub const fn empty() -> Sgr {
        Sgr { codes: [0; SGMAX], len: 0 }
    }

    /// 单段直构（越界给 `VD_ERANGE`）。
    pub fn one(code: u32) -> Result<Sgr, u32> {
        if code > SGR_MAX {
            return Err(VD_ERANGE);
        }
        let mut codes = [0u8; SGMAX];
        if let Some(d) = codes.get_mut(0) {
            *d = code as u8;
        }
        Ok(Sgr { codes, len: 1 })
    }

    /// 逐段解析：`;` 分隔、每段 1..=3 位数字、值 ≤ `SGR_MAX`、段数 ≤ `SGMAX`。
    pub fn parse(raw: &[u8]) -> Result<Sgr, u32> {
        if raw.is_empty() {
            return Err(VD_EINVAL);
        }
        if raw.len() > SGLCAP {
            return Err(VD_ELEN);
        }
        let mut codes = [0u8; SGMAX];
        let mut len = 0usize;
        let mut cur = 0u32;
        let mut digits = 0u32;
        let mut i = 0usize;
        loop {
            let at_end = i >= raw.len();
            let is_sep = match raw.get(i) {
                Some(b) => *b == b';',
                None => false,
            };
            if at_end || is_sep {
                if digits == 0 {
                    return Err(VD_EBYTE);
                }
                if cur > SGR_MAX {
                    return Err(VD_ERANGE);
                }
                if len >= SGMAX {
                    return Err(VD_ELEN);
                }
                if let Some(d) = codes.get_mut(len) {
                    *d = cur as u8;
                }
                len += 1;
                cur = 0;
                digits = 0;
            } else {
                match raw.get(i) {
                    Some(b) => {
                        let c = *b;
                        if c < b'0' || c > b'9' {
                            return Err(VD_EBYTE);
                        }
                        if digits >= 3 {
                            return Err(VD_EBYTE);
                        }
                        cur = cur * 10 + (c - b'0') as u32;
                        digits += 1;
                    }
                    None => return Err(VD_EBYTE),
                }
            }
            if at_end {
                break;
            }
            i += 1;
        }
        Ok(Sgr { codes, len: len as u8 })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<Sgr, u32> {
        Sgr::parse(text.as_bytes())
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 段。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len as usize {
            return None;
        }
        self.codes.get(i).copied()
    }

    /// 是否含某段。
    pub fn has(&self, code: u8) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if self.codes.get(i) == Some(&code) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 两段定宽渲染：每段不足两位补前导零。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(c) = self.codes.get(i) {
                if i > 0 {
                    s.push(';');
                }
                if *c < 10 {
                    s.push('0');
                }
                push_dec(&mut s, *c as u64);
            }
            i += 1;
        }
        s
    }

    /// 转义渲染：`\e[01;34m`。
    pub fn render_escape(&self) -> String {
        let mut s = String::new();
        s.push('\\');
        s.push('e');
        s.push('[');
        s.push_str(&self.render());
        s.push('m');
        s
    }
}

/// 一条「类目 → 属性」登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 类目。
    pub cat: Cat,
    /// 属性。
    pub sgr: Sgr,
}

impl Entry {
    /// 空登记。
    pub const fn blank() -> Entry {
        Entry { cat: Cat::empty(), sgr: Sgr::empty() }
    }

    /// 构造。
    pub fn new(cat: &str, sgr: &str) -> Result<Entry, u32> {
        let c = Cat::parse_text(cat)?;
        let g = Sgr::parse_text(sgr)?;
        Ok(Entry { cat: c, sgr: g })
    }
}

/// 色彩库。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Db {
    rows: [Entry; DBCAP],
    len: usize,
}

impl Db {
    /// 空库。
    pub const fn new() -> Db {
        Db { rows: [Entry::blank(); DBCAP], len: 0 }
    }

    /// 类目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 条。
    pub fn entry(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按名查。
    pub fn find(&self, name: &[u8]) -> Option<&Entry> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.cat.eq_bytes(name) {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条：重复给 `VD_EDUP`，满库给 `VD_ELEN`。
    pub fn add(&mut self, e: Entry) -> Result<(), u32> {
        if self.find(e.cat.as_bytes()).is_some() {
            return Err(VD_EDUP);
        }
        if self.len >= DBCAP {
            return Err(VD_ELEN);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(())
    }

    /// 合并：逐条搬，冲突即失败（已搬的保留）。
    pub fn merge(&mut self, other: &Db) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < other.len {
            if let Some(e) = other.rows.get(i) {
                self.add(*e)?;
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 覆盖登记：已存在则原位替换（用于 `--database` 叠加）。
    pub fn set(&mut self, e: Entry) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.len {
            let hit = match self.rows.get(i) {
                Some(r) => r.cat.eq_bytes(e.cat.as_bytes()),
                None => false,
            };
            if hit {
                if let Some(dst) = self.rows.get_mut(i) {
                    *dst = e;
                }
                return Ok(());
            }
            i += 1;
        }
        self.add(e)
    }

    /// `LS_COLORS=...` 的值体：`cat=sgr` 以冒号相连。
    pub fn lscolors(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if i > 0 {
                    s.push(':');
                }
                s.push_str(&e.cat.text());
                s.push('=');
                s.push_str(&e.sgr.render());
            }
            i += 1;
        }
        s
    }

    /// Bourne 壳式输出。
    pub fn render_sh(&self) -> String {
        let mut s = String::new();
        s.push_str("LS_COLORS='");
        s.push_str(&self.lscolors());
        s.push_str("';\n");
        s.push_str("export LS_COLORS\n");
        s
    }

    /// C 壳式输出。
    pub fn render_csh(&self) -> String {
        let mut s = String::new();
        s.push_str("setenv ");
        s.push_str("LS_COLORS '");
        s.push_str(&self.lscolors());
        s.push_str("'\n");
        s
    }
}

/// 去首尾空白（空格、制表、回车）。
fn trim_bytes(raw: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = raw.len();
    while a < b {
        match raw.get(a) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' || *c == b'\r' {
                    a += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    while b > a {
        match raw.get(b - 1) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' || *c == b'\r' {
                    b -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    raw.get(a..b).unwrap_or(&[])
}

/// 在首个空白处切成两段。
fn split_line<'a>(line: &'a [u8]) -> Result<(&'a [u8], &'a [u8]), u32> {
    let mut i = 0usize;
    while i < line.len() {
        match line.get(i) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    let key = line.get(..i).unwrap_or(&[]);
                    let rest = trim_bytes(line.get(i..).unwrap_or(&[]));
                    return Ok((key, rest));
                }
            }
            None => break,
        }
        i += 1;
    }
    Ok((line, &[]))
}

/// 解析配置：空行与 `#` 注释跳过，每行 `类目 空白 SGR`。
pub fn parse_db(raw: &[u8]) -> Result<Db, u32> {
    let mut db = Db::new();
    if raw.is_empty() {
        return Ok(db);
    }
    let mut i = 0usize;
    loop {
        let start = i;
        while i < raw.len() {
            match raw.get(i) {
                Some(c) => {
                    if *c == b'\n' {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        let line = raw.get(start..i).unwrap_or(&[]);
        let t = trim_bytes(line);
        if !t.is_empty() {
            if t.len() > LINECAP {
                return Err(VD_ELEN);
            }
            if t.first() != Some(&b'#') {
                let (key, rest) = split_line(t)?;
                if rest.is_empty() {
                    return Err(VD_EINVAL);
                }
                let cat = Cat::parse(key)?;
                let sgr = Sgr::parse(rest)?;
                db.add(Entry { cat, sgr })?;
            }
        }
        if i >= raw.len() {
            break;
        }
        i += 1;
    }
    Ok(db)
}

/// 默认库：十二类，顺序与基准机一致。
pub fn default_db() -> Db {
    let mut db = Db::new();
    let rows = [
        ("rs", "00"),
        ("fi", "00"),
        ("di", "01;34"),
        ("ln", "01;36"),
        ("ex", "01;32"),
        ("so", "01;35"),
        ("bd", "01;33"),
        ("cd", "01;33"),
        ("or", "01;31"),
        ("mi", "01;31"),
        ("su", "37;41"),
        ("sg", "30;43"),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((c, g)) = rows.get(i) {
            if let Ok(e) = Entry::new(c, g) {
                let _ = db.add(e);
            }
        }
        i += 1;
    }
    db
}

/// 条目类目判定。
pub fn classify(kind: u8, mode: u32, dangling: bool) -> Cat {
    if kind == K_DIR {
        return Cat::of(b"di");
    }
    if kind == K_LINK {
        if dangling {
            return Cat::of(b"or");
        }
        return Cat::of(b"ln");
    }
    if kind == K_FIFO {
        return Cat::of(b"pi");
    }
    if kind == K_SOCK {
        return Cat::of(b"so");
    }
    if kind == K_BLK {
        return Cat::of(b"bd");
    }
    if kind == K_CHR {
        return Cat::of(b"cd");
    }
    if mode & 0o111 != 0 {
        return Cat::of(b"ex");
    }
    Cat::of(b"fi")
}

/// 种类首字符。
fn kind_char(kind: u8) -> char {
    if kind == K_DIR {
        'd'
    } else if kind == K_LINK {
        'l'
    } else if kind == K_FIFO {
        'p'
    } else if kind == K_SOCK {
        's'
    } else if kind == K_BLK {
        'b'
    } else if kind == K_CHR {
        'c'
    } else {
        '-'
    }
}

/// 权限三连：普通位与特殊位替换规则。
fn push_triad(s: &mut String, bits: u32, special: u8, special_char: char) {
    s.push(if bits & 0o4 != 0 { 'r' } else { '-' });
    s.push(if bits & 0o2 != 0 { 'w' } else { '-' });
    let x = bits & 0o1 != 0;
    if special != 0 {
        if special_char == 't' || special_char == 'T' {
            s.push(if x { 't' } else { 'T' });
        } else {
            s.push(if x { 's' } else { 'S' });
        }
    } else {
        s.push(if x { 'x' } else { '-' });
    }
}

/// 十位模式串。
pub fn mode_text(kind: u8, bits: u32) -> String {
    let mut s = String::new();
    s.push(kind_char(kind));
    let suid = if bits & 0o4000 != 0 { 4 } else { 0 };
    let sgid = if bits & 0o2000 != 0 { 2 } else { 0 };
    let sticky = if bits & 0o1000 != 0 { 1 } else { 0 };
    push_triad(&mut s, (bits >> 6) & 0o7, suid, 's');
    push_triad(&mut s, (bits >> 3) & 0o7, sgid, 's');
    push_triad(&mut s, bits & 0o7, sticky, 't');
    s
}

/// 人类可读尺寸：1K 向上取整。
pub fn human_size(v: u64) -> String {
    if v < 1024 {
        return dec_text(v);
    }
    if v < 1024 * 1024 {
        return {
            let mut s = dec_text(v.saturating_add(1023) / 1024);
            s.push('K');
            s
        };
    }
    if v < 1024 * 1024 * 1024 {
        let mut s = dec_text(v.saturating_add(1024 * 1024 - 1) / (1024 * 1024));
        s.push('M');
        return s;
    }
    let mut s = dec_text(v.saturating_add(1024 * 1024 * 1024 - 1) / (1024 * 1024 * 1024));
    s.push('G');
    s
}

/// 普通尺寸文本。
pub fn plain_size(v: u64) -> String {
    dec_text(v)
}

/// 一条长格式行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// 模式串（十位）。
    pub mode: String,
    /// 链接数。
    pub links: u32,
    /// 属主。
    pub owner: String,
    /// 属组。
    pub group: String,
    /// 尺寸文本（是否人类可读由构造方决定）。
    pub size: String,
    /// 时间（十二字符 `Mon DD HH:MM`）。
    pub when: String,
    /// 名字。
    pub name: String,
    /// 软链目标。
    pub target: Option<String>,
}

impl Row {
    /// 空行。
    pub fn blank() -> Row {
        Row {
            mode: String::new(),
            links: 0,
            owner: String::new(),
            group: String::new(),
            size: String::new(),
            when: String::new(),
            name: String::new(),
            target: None,
        }
    }

    /// 构造。
    pub fn new(
        kind: u8,
        bits: u32,
        links: u32,
        owner: &str,
        group: &str,
        bytes: u64,
        human: bool,
        when: &str,
        name: &str,
        target: Option<&str>,
    ) -> Row {
        let size = if human { human_size(bytes) } else { plain_size(bytes) };
        Row {
            mode: mode_text(kind, bits),
            links,
            owner: bytes_text(owner.as_bytes()),
            group: bytes_text(group.as_bytes()),
            size,
            when: bytes_text(when.as_bytes()),
            name: bytes_text(name.as_bytes()),
            target: match target {
                Some(t) => Some(bytes_text(t.as_bytes())),
                None => None,
            },
        }
    }

    /// 自身字节数（用于 `total`，人类可读不影响块数，由调用方另给）。
    pub fn tail(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.name);
        if let Some(t) = &self.target {
            s.push_str(" -> ");
            s.push_str(t);
        }
        s
    }
}

fn blank_rows() -> [Row; ROWCAP] {
    [
        Row::blank(),
        Row::blank(),
        Row::blank(),
        Row::blank(),
        Row::blank(),
        Row::blank(),
        Row::blank(),
        Row::blank(),
    ]
}

/// 长格式清单。
#[derive(Clone, Debug)]
pub struct Listing {
    rows: [Row; ROWCAP],
    len: usize,
    blocks: u64,
}

impl Listing {
    /// 空清单。
    pub fn new() -> Listing {
        Listing { rows: blank_rows(), len: 0, blocks: 0 }
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
    pub fn row(&self, i: usize) -> Option<&Row> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
    }

    /// 追加一行；`bytes` 是落盘字节数（`-h` 时只影响显示，块数照算）。
    pub fn push(&mut self, row: Row, bytes: u64) -> Result<(), u32> {
        if self.len >= ROWCAP {
            return Err(VD_ELEN);
        }
        let blocks = bytes.saturating_add(1023) / 1024;
        self.blocks = self.blocks.saturating_add(blocks);
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
        }
        self.len += 1;
        Ok(())
    }

    /// `total` 行。
    pub fn total_line(&self) -> String {
        let mut s = String::new();
        s.push_str("total ");
        push_dec(&mut s, self.blocks);
        s.push('\n');
        s
    }

    /// 列宽：链接数、属主、属组、尺寸。
    fn widths(&self) -> (usize, usize, usize, usize) {
        let mut wl = 0usize;
        let mut wo = 0usize;
        let mut wg = 0usize;
        let mut ws = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                let a = dec_text(r.links as u64).len();
                if a > wl {
                    wl = a;
                }
                if r.owner.len() > wo {
                    wo = r.owner.len();
                }
                if r.group.len() > wg {
                    wg = r.group.len();
                }
                if r.size.len() > ws {
                    ws = r.size.len();
                }
            }
            i += 1;
        }
        (wl, wo, wg, ws)
    }

    /// 渲染整表（含 `total` 行）。
    pub fn render(&self) -> String {
        let mut s = self.total_line();
        let (wl, wo, wg, ws) = self.widths();
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                s.push_str(&r.mode);
                s.push(' ');
                pad_left(&mut s, &dec_text(r.links as u64), wl);
                s.push(' ');
                pad_right(&mut s, &r.owner, wo);
                s.push(' ');
                pad_right(&mut s, &r.group, wg);
                s.push(' ');
                pad_left(&mut s, &r.size, ws);
                s.push(' ');
                s.push_str(&r.when);
                s.push(' ');
                s.push_str(&r.tail());
                s.push('\n');
            }
            i += 1;
        }
        s
    }

    /// 名字升序的下标表。
    pub fn order(&self) -> [usize; ROWCAP] {
        let mut idx = [0usize; ROWCAP];
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = idx.get_mut(i) {
                *d = i;
            }
            i += 1;
        }
        let mut a = 1usize;
        while a < self.len {
            let mut b = a;
            while b > 0 {
                let (x, y) = match (idx.get(b - 1), idx.get(b)) {
                    (Some(p), Some(q)) => (*p, *q),
                    _ => break,
                };
                let swap = match (self.rows.get(x), self.rows.get(y)) {
                    (Some(rx), Some(ry)) => rx.name.as_bytes() > ry.name.as_bytes(),
                    _ => false,
                };
                if !swap {
                    break;
                }
                if let Some(d) = idx.get_mut(b - 1) {
                    *d = y;
                }
                if let Some(d) = idx.get_mut(b) {
                    *d = x;
                }
                b -= 1;
            }
            a += 1;
        }
        idx
    }

    /// 按名字升序渲染。
    pub fn render_sorted(&self) -> String {
        let mut s = self.total_line();
        let (wl, wo, wg, ws) = self.widths();
        let idx = self.order();
        let mut i = 0usize;
        while i < self.len {
            let pos = match idx.get(i) {
                Some(v) => *v,
                None => break,
            };
            if let Some(r) = self.rows.get(pos) {
                s.push_str(&r.mode);
                s.push(' ');
                pad_left(&mut s, &dec_text(r.links as u64), wl);
                s.push(' ');
                pad_right(&mut s, &r.owner, wo);
                s.push(' ');
                pad_right(&mut s, &r.group, wg);
                s.push(' ');
                pad_left(&mut s, &r.size, ws);
                s.push(' ');
                s.push_str(&r.when);
                s.push(' ');
                s.push_str(&r.tail());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 着色时机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorWhen {
    /// 仅终端。
    Auto,
    /// 总是。
    Always,
    /// 从不。
    Never,
}

impl ColorWhen {
    /// 文本。
    pub fn text(&self) -> &'static str {
        if *self == ColorWhen::Always {
            "always"
        } else if *self == ColorWhen::Never {
            "never"
        } else {
            "auto"
        }
    }

    /// 是否落地转义。
    pub fn paints(&self, tty: bool) -> bool {
        if *self == ColorWhen::Always {
            true
        } else if *self == ColorWhen::Never {
            false
        } else {
            tty
        }
    }
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 含隐藏项。
    pub all: bool,
    /// 长格式。
    pub long: bool,
    /// 人类可读。
    pub human: bool,
    /// 单列。
    pub one: bool,
    /// 递归。
    pub recursive: bool,
    /// 着色时机。
    pub color: ColorWhen,
    /// 是否显式给过 `--color`。
    pub color_seen: bool,
    /// 参数个数。
    pub argc: usize,
}

impl Opts {
    /// 空选项。
    pub const fn blank() -> Opts {
        Opts {
            all: false,
            long: false,
            human: false,
            one: false,
            recursive: false,
            color: ColorWhen::Auto,
            color_seen: false,
            argc: 0,
        }
    }

    /// 是否长格式落地。
    pub fn shows_long(&self) -> bool {
        self.long && !self.one
    }
}

/// 解析选项：字面 → 长度 → 重复 → 色彩 → 冲突。
pub fn parse_opts(args: &[&str]) -> Result<Opts, u32> {
    let mut o = Opts::blank();
    let mut i = 0usize;
    while i < args.len() {
        if i >= ARGCAP {
            return Err(VD_ELEN);
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a.len() > 32 {
            return Err(VD_ELEN);
        }
        if a == "-a" || a == "--all" {
            if o.all {
                return Err(VD_EDUP);
            }
            o.all = true;
        } else if a == "-l" {
            if o.long {
                return Err(VD_EDUP);
            }
            o.long = true;
        } else if a == "-h" || a == "--human-readable" {
            if o.human {
                return Err(VD_EDUP);
            }
            o.human = true;
        } else if a == "-1" {
            if o.one {
                return Err(VD_EDUP);
            }
            o.one = true;
        } else if a == "-R" || a == "--recursive" {
            if o.recursive {
                return Err(VD_EDUP);
            }
            o.recursive = true;
        } else if let Some(rest) = a.strip_prefix("--color=") {
            if o.color_seen {
                return Err(VD_EDUP);
            }
            if rest == "auto" || rest == "tty" {
                o.color = ColorWhen::Auto;
            } else if rest == "always" || rest == "yes" || rest == "force" {
                o.color = ColorWhen::Always;
            } else if rest == "never" || rest == "no" || rest == "none" {
                o.color = ColorWhen::Never;
            } else {
                return Err(VD_ECOLOR);
            }
            o.color_seen = true;
        } else {
            return Err(VD_EINVAL);
        }
        o.argc += 1;
        i += 1;
    }
    if o.long && o.one {
        return Err(VD_ESTATE);
    }
    Ok(o)
}

/// 一个账本样本。
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
            return Err(VD_ESLOT);
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
            return Err(VD_EBAL);
        }
        const VALS: [u32; 8] = [2, 5, 12, 101, 132, 121, 2, 10];
        const CODES: [u32; 6] = [VD_EBYTE, VD_ERANGE, VD_EINVAL, VD_EDUP, VD_ENOENT, VD_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VD_EBAL),
            };
            if s.slot as usize != i {
                return Err(VD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VD_EVALUE);
                        }
                    }
                    None => return Err(VD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VD_EVALUE);
                        }
                    }
                    None => return Err(VD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VD_EBAL);
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
            return Err(VD_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(VD_ESEAL)
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

/// 三行样本清单（目录/文件/软链）。
pub fn sample_listing(human: bool) -> Listing {
    let mut l = Listing::new();
    let _ = l.push(
        Row::new(K_DIR, 0o040755, 2, "root", "root", 4096, human, "Oct 30 13:23", "docs", None),
        4096,
    );
    let _ = l.push(
        Row::new(K_REG, 0o100644, 1, "varia", "users", 1536, human, "Oct 30 13:23", "notes.txt", None),
        1536,
    );
    let _ = l.push(
        Row::new(K_LINK, 0o120777, 1, "root", "root", 12, human, "Oct 30 13:23", "latest", Some("notes.txt")),
        12,
    );
    l
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let base = default_db();

    let _ = l.register(count_cell(0, OP_SGR, Sgr::parse(b"01;34").map(|s| s.len())));

    let _ = l.register(count_cell(1, OP_RENDER, Sgr::parse(b"01;34").map(|s| s.render().len())));

    let _ = l.register(count_cell(2, OP_DB, Ok(base.len())));

    let _ = l.register(count_cell(3, OP_LSCOLORS, Ok(base.lscolors().len())));

    let _ = l.register(count_cell(4, OP_SH, Ok(base.render_sh().len())));

    let _ = l.register(count_cell(5, OP_CSH, Ok(base.render_csh().len())));

    let _ = l.register(word_cell(6, OP_CLASS, Ok(classify(K_DIR, 0o040755, false).code() as u32)));

    let _ = l.register(count_cell(7, OP_MODE, Ok(mode_text(K_DIR, 0o040755).len())));

    let _ = l.register(word_cell(8, OP_SGR, Sgr::parse(b"01;3q").map(|s| s.len() as u32)));

    let _ = l.register(word_cell(9, OP_SGR, Sgr::parse(b"200").map(|s| s.len() as u32)));

    let _ = l.register(word_cell(10, OP_SGR, Sgr::parse(b"").map(|s| s.len() as u32)));

    let _ = l.register(word_cell(11, OP_DB, {
        let mut db = base;
        match Entry::new("di", "01;34") {
            Ok(e) => match db.add(e) {
                Ok(()) => Ok(0),
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(word_cell(12, OP_DB, match base.find(b"zz") {
        Some(e) => Ok(e.cat.code() as u32),
        None => Err(VD_ENOENT),
    }));

    let _ = l.register(word_cell(13, OP_OPT, parse_opts(&["-l", "-1"]).map(|o| o.argc as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = match Sgr::parse(b"1;34") {
        Ok(s) => s.render().as_str() == "01;34" && s.len() == 2 && s.has(34),
        Err(_) => false,
    };

    out[1] = {
        let db = default_db();
        let a = db.len() == 12;
        let b = match db.find(b"di") {
            Some(e) => e.sgr.render().as_str() == "01;34",
            None => false,
        };
        let c = classify(K_REG, 0o100755, false).eq_bytes(b"ex");
        a && b && c
    };

    out[2] = mode_text(K_REG, 0o104755).as_str() == "-rwsr-xr-x"
        && mode_text(K_DIR, 0o041777).as_str() == "drwxrwxrwt";

    out[3] = {
        let l = sample_listing(true);
        let text = l.render();
        text.contains("total 7")
            && text.contains(" -> notes.txt")
            && text.contains("drwxr-xr-x")
    };

    out
}

/// 判据集合。
pub fn run_unx11555_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11555.vdir");

    s.add(
        "VD01-属性解析与定宽渲染",
        {
            let a = Sgr::parse(b"01;34").map(|g| g.render()) == Ok(String::from("01;34"));
            let b = Sgr::parse(b"1;34").map(|g| g.render()) == Ok(String::from("01;34"));
            let c = Sgr::parse(b"0").map(|g| g.render()) == Ok(String::from("00"));
            let d = Sgr::parse(b"37;41").map(|g| g.render()) == Ok(String::from("37;41"));
            let e = Sgr::parse(b"01;3q") == Err(VD_EBYTE)
                && Sgr::parse(b"200") == Err(VD_ERANGE)
                && Sgr::parse(b"") == Err(VD_EINVAL)
                && Sgr::parse(b"1;2;3;4;5;6;7;8;9") == Err(VD_ELEN);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "VD02-色彩库登记与去重",
        {
            let mut db = Db::new();
            let a = match Entry::new("di", "01;34") {
                Ok(e) => db.add(e).is_ok() && db.len() == 1,
                Err(_) => false,
            };
            let b = match Entry::new("di", "01;35") {
                Ok(e) => db.add(e) == Err(VD_EDUP) && db.len() == 1,
                Err(_) => false,
            };
            let c = match Entry::new("di", "01;35") {
                Ok(e) => db.set(e).is_ok() && db.len() == 1,
                Err(_) => false,
            };
            let d = match db.find(b"di") {
                Some(e) => e.sgr.render().as_str() == "01;35",
                None => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "VD03-LS_COLORS 串",
        {
            let db = default_db();
            let t = db.lscolors();
            t.as_str().starts_with("rs=00:fi=00:di=01;34")
                && t.as_str().ends_with("su=37;41:sg=30;43")
                && t.as_str().matches(':').count() == 11
        },
        "static",
    );

    s.add(
        "VD04-sh/csh 两式",
        {
            let db = default_db();
            let sh = db.render_sh();
            let csh = db.render_csh();
            sh.as_str().starts_with("LS_COLORS='")
                && sh.as_str().ends_with("';\nexport LS_COLORS\n")
                && csh.as_str().starts_with("setenv LS_COLORS '")
                && csh.as_str().ends_with("'\n")
                && sh.len() == csh.len() + 11
        },
        "static",
    );

    s.add(
        "VD05-默认库十二类",
        {
            let db = default_db();
            let names = [b"rs", b"fi", b"di", b"ln", b"ex", b"so", b"bd", b"cd", b"or", b"mi", b"su", b"sg"];
            let mut n = 0usize;
            let mut i = 0usize;
            while i < names.len() {
                if let Some(c) = names.get(i) {
                    if db.find(*c).is_some() {
                        n += 1;
                    }
                }
                i += 1;
            }
            let a = db.len() == 12 && n == 12;
            let b = match db.find(b"so") {
                Some(e) => e.sgr.render().as_str() == "01;35",
                None => false,
            };
            let c = match db.find(b"sg") {
                Some(e) => e.sgr.render().as_str() == "30;43",
                None => false,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "VD06-类目判定",
        {
            classify(K_DIR, 0o040755, false).eq_bytes(b"di")
                && classify(K_LINK, 0o120777, false).eq_bytes(b"ln")
                && classify(K_LINK, 0o120777, true).eq_bytes(b"or")
                && classify(K_REG, 0o100755, false).eq_bytes(b"ex")
                && classify(K_REG, 0o100644, false).eq_bytes(b"fi")
                && classify(K_FIFO, 0o010644, false).eq_bytes(b"pi")
                && classify(K_SOCK, 0o140755, false).eq_bytes(b"so")
                && classify(K_BLK, 0o060644, false).eq_bytes(b"bd")
                && classify(K_CHR, 0o020644, false).eq_bytes(b"cd")
        },
        "static",
    );

    s.add(
        "VD07-模式串十字位",
        {
            let a = mode_text(K_DIR, 0o040755).as_str() == "drwxr-xr-x";
            let b = mode_text(K_REG, 0o104755).as_str() == "-rwsr-xr-x";
            let c = mode_text(K_REG, 0o102644).as_str() == "-rw-r-Sr--";
            let d = mode_text(K_DIR, 0o041777).as_str() == "drwxrwxrwt";
            let e = mode_text(K_LINK, 0o120777).as_str() == "lrwxrwxrwx";
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "VD08-长格式列对齐",
        {
            let l = sample_listing(true);
            let t = l.render();
            l.len() == 3
                && t.as_str().starts_with("total 7\n")
                && t.contains("drwxr-xr-x 2 root  root  4K Oct 30 13:23 docs")
                && t.contains("-rw-r--r-- 1 varia users 2K Oct 30 13:23 notes.txt")
                && t.contains(" -> notes.txt")
        },
        "static",
    );

    s.add(
        "VD09-人类可读尺寸",
        {
            human_size(0).as_str() == "0"
                && human_size(1023).as_str() == "1023"
                && human_size(1024).as_str() == "1K"
                && human_size(1536).as_str() == "2K"
                && human_size(1024 * 1024).as_str() == "1M"
                && human_size(5 * 1024 * 1024).as_str() == "5M"
                && human_size(1024 * 1024 * 1024).as_str() == "1G"
        },
        "static",
    );

    s.add(
        "VD10-选项矩阵",
        {
            let a = match parse_opts(&["-a", "-l", "-h", "--color=always"]) {
                Ok(o) => o.all && o.long && o.human && o.color == ColorWhen::Always && o.shows_long(),
                Err(_) => false,
            };
            let b = parse_opts(&["-l", "-1"]) == Err(VD_ESTATE);
            let c = parse_opts(&["-a", "-a"]) == Err(VD_EDUP);
            let d = parse_opts(&["--color=purple"]) == Err(VD_ECOLOR);
            let e = parse_opts(&["-x"]) == Err(VD_EINVAL);
            let f = match parse_opts(&["-1"]) {
                Ok(o) => o.one && !o.shows_long(),
                Err(_) => false,
            };
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "VD11-闸门次序",
        {
            let a = Cat::parse(b"") == Err(VD_EINVAL);
            let b = Cat::parse(b"D1") == Err(VD_EBYTE);
            let c = Sgr::parse(b"1234") == Err(VD_EBYTE);
            let d = Sgr::parse(b"999") == Err(VD_ERANGE);
            let e = Sgr::parse(b"1;;2") == Err(VD_EBYTE);
            let f = parse_db(b"di 01;34\ndi 01;35\n") == Err(VD_EDUP);
            let g = parse_db(b"# c\n\ndi 01;34\n") .map(|db| db.len()) == Ok(1);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "VD12-账本对账与重放",
        {
            let l = build_ledger();
            l.audit().is_ok()
                && l.replay().is_ok()
                && l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.seal() != 0
        },
        "static",
    );

    s.add(
        "VD13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let frozen = l.freeze().is_ok();
            let rejected = l.register(Sample::blank()).is_err() && l.rejects() == 1;
            let root = l.root() != 0 && l.is_frozen();
            let inj = inject_four();
            frozen && rejected && root && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_roundtrip() {
        assert_eq!(Sgr::parse(b"01;34").map(|s| s.render()), Ok(String::from("01;34")));
        assert_eq!(Sgr::parse(b"7").map(|s| s.render()), Ok(String::from("07")));
        assert_eq!(Sgr::parse(b"37;41;01").map(|s| s.render()), Ok(String::from("37;41;01")));
        assert_eq!(Sgr::parse(b"01;").map(|s| s.len()), Err(VD_EBYTE));
    }

    #[test]
    fn sgr_bounds() {
        assert_eq!(Sgr::parse(b"108"), Err(VD_ERANGE));
        assert_eq!(Sgr::parse(b"107"), Ok(Sgr::one(107).unwrap_or(Sgr::empty())));
        assert_eq!(Sgr::parse(b"1234"), Err(VD_EBYTE));
        assert_eq!(Sgr::parse(b"abc"), Err(VD_EBYTE));
        assert_eq!(Sgr::parse(b""), Err(VD_EINVAL));
    }

    #[test]
    fn db_order_and_merge() {
        let db = default_db();
        assert_eq!(db.len(), 12);
        assert_eq!(db.entry(0).map(|e| e.cat.text()), Some(String::from("rs")));
        let mut extra = Db::new();
        let _ = extra.add(Entry::new("tw", "30;42").unwrap_or(Entry::blank()));
        let mut base = db;
        assert_eq!(base.merge(&extra), Ok(1));
        assert_eq!(base.len(), 13);
        assert_eq!(base.merge(&extra), Err(VD_EDUP));
    }

    #[test]
    fn db_parse_lines() {
        let raw = b"# ls colors\n\nrs 00\ndi 01;34\nln 01;36\n";
        let db = parse_db(raw).unwrap_or(Db::new());
        assert_eq!(db.len(), 3);
        assert_eq!(db.find(b"di").map(|e| e.sgr.render()), Some(String::from("01;34")));
        assert_eq!(parse_db(b"di\n"), Err(VD_EINVAL));
        assert_eq!(parse_db(b"di 01;3x\n"), Err(VD_EBYTE));
    }

    #[test]
    fn shell_forms() {
        let db = default_db();
        let sh = db.render_sh();
        let csh = db.render_csh();
        assert!(sh.starts_with("LS_COLORS='rs=00"));
        assert!(sh.ends_with("export LS_COLORS\n"));
        assert!(csh.starts_with("setenv LS_COLORS 'rs=00"));
        assert_eq!(sh.matches('\'').count(), 2);
    }

    #[test]
    fn mode_strings() {
        assert_eq!(mode_text(K_REG, 0o100600), "-rw-------");
        assert_eq!(mode_text(K_REG, 0o104755), "-rwsr-xr-x");
        assert_eq!(mode_text(K_REG, 0o102644), "-rw-r-Sr--");
        assert_eq!(mode_text(K_DIR, 0o041777), "drwxrwxrwt");
        assert_eq!(mode_text(K_FIFO, 0o010644), "prw-r--r--");
    }

    #[test]
    fn listing_alignment() {
        let l = sample_listing(false);
        let t = l.render();
        assert_eq!(l.len(), 3);
        assert!(t.starts_with("total 7\n"));
        assert!(t.contains("4096 Oct 30 13:23 docs"));
        assert!(t.contains("1536 Oct 30 13:23 notes.txt"));
        assert!(t.contains("latest -> notes.txt"));
    }

    #[test]
    fn sorted_listing() {
        let l = sample_listing(false);
        let t = l.render_sorted();
        let docs = t.find("docs").unwrap_or(0);
        let latest = t.find("latest").unwrap_or(0);
        let notes = t.find("notes.txt").unwrap_or(0);
        assert!(docs < latest && latest < notes);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(13).map(|s| s.code), Some(VD_ESTATE));
    }

    #[test]
    fn options_gates() {
        assert_eq!(parse_opts(&["-l", "-1"]), Err(VD_ESTATE));
        assert_eq!(parse_opts(&["--color=auto", "--color=never"]), Err(VD_EDUP));
        assert_eq!(parse_opts(&["--color=rainbow"]), Err(VD_ECOLOR));
        assert_eq!(parse_opts(&["--"]), Err(VD_EINVAL));
        let o = parse_opts(&["-R", "-h"]).unwrap_or(Opts::blank());
        assert!(o.recursive && o.human);
    }
}
