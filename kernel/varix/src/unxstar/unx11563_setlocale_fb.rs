//! UNX-F11563 setlocale 返回值与降级路径（坏 locale → "C"）
//!
//! 问题的根：`setlocale` 的返回值和「有没有改成功」是**两件事**，而 POSIX 把它们
//! 绑在一起：成功时返回**新名字**（查询时返回当前名字），失败时返回 `NULL`，
//! 并且**一个域都不许改**。工具链上最常见的错误是把失败当成功——`setlocale`
//! 返回 NULL 之后继续按 UTF-8 的假设去切字符，于是把多字节序列劈成两半。
//!
//! 本单把 C 库的这个契约做成状态机：
//!
//! 1. **名字规范化**。`POSIX`、`C` 是同义；`en_US.utf8`、`en_US.UTF8`、
//!    `en_US.UTF-8` 都归一到 `en_US.UTF-8`；修饰符 `@latin` 保留。规范化之后
//!    才进库查表，避免「同一个 locale 三种写法，命中率取决于运气」。
//! 2. **原子提交**。复合请求 `LC_CTYPE=..;LC_TIME=..` 先全量校验（域名合法、
//!    无重复、每个 locale 在库），任一段失败就整体放弃：`version` 不变、
//!    十二域一个字节不动。这正是 C 库「失败返回 NULL 且保持原域」的行为。
//! 3. **降级路径**。`fallback_set` 在库里找不到时显式落到 `"C"` 并回报
//!    `fell_back = true`；调用方因此能区分「本来就要 C」和「要的没有、退到 C」。
//!    不显式降级的 `setlocale` 则返回 `SL_ENOLOC`（等价于 NULL），两者不混。
//! 4. **返回值**。单域设置返回该域新名；`LC_ALL` 查询在十二域同值时返回短名
//!    （`"C"`），不同值时返回 `LC_X=v;LC_Y=w;…` 的复合串。这个分支是
//!    「`setlocale(LC_ALL, NULL)` 的输出长度为什么时短时长」的根因。
//!
//! 闸门次序：`SL_ESHORT`（空段/缺 '='）→ `SL_EINVAL`（域名未知）→
//! `SL_ELEN`（名字/复合串超长）→ `SL_EBYTE`（名字含非法字节）→
//! `SL_EDUP`（复合串里同一域出现两次）→ `SL_ENOLOC`（库里没有）→
//! `SL_ERANGE`（域号越界）→ `SL_ESTATE`（快照版本不符）。前六道在
//! `canonical`/`parse_plan`/`Db::find`，域号在 `cat_index`，版本在 `restore`。
//!
//! 账本语义：十四格 = 八格成功（单域返回名长度、查询长度、十二域覆盖数、
//! 同值短名长度、复合两域数、降级标志、失败后仍是 C 的域数、别名命中的库下标）
//! + 六格失败（库中没有、复合重复、缺 '='、名字超长、非法字节、域号越界）。
//! `audit` 认「八值六码」，`seal` 折叠，`replay` 重建比对，`freeze` 后拒登。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf440;

/// 域名/名字语法非法。
pub const SL_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字或复合串超长。
pub const SL_ELEN: u32 = SEG_BASE + 0x02;
/// 名字含非法字节。
pub const SL_EBYTE: u32 = SEG_BASE + 0x03;
/// 域号越界。
pub const SL_ERANGE: u32 = SEG_BASE + 0x04;
/// 复合串内重复域。
pub const SL_EDUP: u32 = SEG_BASE + 0x05;
/// 空段/缺等号。
pub const SL_ESHORT: u32 = SEG_BASE + 0x06;
/// 快照版本不符。
pub const SL_ESTATE: u32 = SEG_BASE + 0x07;
/// 库中没有该 locale（setlocale 返回 NULL）。
pub const SL_ENOLOC: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const SL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const SL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const SL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 域数。
pub const CATS: usize = 12;

/// `LC_ALL` 的域号。
pub const CAT_ALL: u8 = 0x0c;

/// 名字容量。
pub const NAME_CAP: usize = 24;

/// 复合串容量。
pub const COMPOSITE_CAP: usize = 192;

/// locale 库容量。
pub const DB_CAP: usize = 10;

/// 操作码：单域设置。
pub const OP_SET: u8 = 0x01;
/// 操作码：查询。
pub const OP_QUERY: u8 = 0x02;
/// 操作码：全设。
pub const OP_ALL: u8 = 0x03;
/// 操作码：复合。
pub const OP_COMPOSITE: u8 = 0x04;
/// 操作码：降级。
pub const OP_FALLBACK: u8 = 0x05;
/// 操作码：原子性。
pub const OP_ATOMIC: u8 = 0x06;
/// 操作码：库查表。
pub const OP_LOOKUP: u8 = 0x07;
/// 操作码：规范化。
pub const OP_CANON: u8 = 0x08;

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

/// 十二个域名。
pub const CAT_NAMES: [&str; CATS] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// 内置 locale 库（次序即库下标，账本用得到）。
pub const DB_NAMES: [&str; 8] = [
    "C",
    "C.UTF-8",
    "en_US",
    "en_US.UTF-8",
    "de_DE.UTF-8",
    "ja_JP.UTF-8",
    "fr_FR.UTF-8",
    "sr_RS.UTF-8@latin",
];

/// 域名 → 域号。
pub fn cat_index(name: &[u8]) -> Result<u8, u32> {
    if name.is_empty() {
        return Err(SL_ESHORT);
    }
    if name.len() > 20 {
        return Err(SL_ELEN);
    }
    if name == b"LC_ALL" {
        return Ok(CAT_ALL);
    }
    let mut i = 0usize;
    while i < CATS {
        if let Some(n) = CAT_NAMES.get(i) {
            if n.as_bytes() == name {
                return Ok(i as u8);
            }
        }
        i += 1;
    }
    Err(SL_EINVAL)
}

/// 域号 → 名。
pub fn cat_name(i: u8) -> Option<&'static str> {
    if i == CAT_ALL {
        return Some("LC_ALL");
    }
    CAT_NAMES.get(i as usize).copied()
}

/// 校验域号（0..=11 或 12=ALL）。
pub fn check_cat(i: u8) -> Result<u8, u32> {
    if i <= CAT_ALL {
        Ok(i)
    } else {
        Err(SL_ERANGE)
    }
}

/// 定容名字。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 空名。
    pub const fn empty() -> Name {
        Name { buf: [0u8; NAME_CAP], len: 0 }
    }

    /// 解析：只接受 locale 名字符集。
    pub fn parse(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() {
            return Err(SL_ESHORT);
        }
        if raw.len() > NAME_CAP {
            return Err(SL_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i).copied() {
                Some(v) => v,
                None => return Err(SL_ESHORT),
            };
            let ok = matches!(b,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'@' | b'-');
            if !ok {
                return Err(SL_EBYTE);
            }
            if let Some(d) = buf.get_mut(i) {
                *d = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: raw.len() })
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        let n = if self.len <= NAME_CAP { self.len } else { NAME_CAP };
        match self.buf.get(0..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        if self.len <= NAME_CAP { self.len } else { NAME_CAP }
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let v = self.bytes();
        let mut i = 0usize;
        while i < v.len() {
            if let Some(b) = v.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 与字面量比较。
    pub fn is(&self, raw: &[u8]) -> bool {
        self.bytes() == raw
    }

    /// 是否 C。
    pub fn is_c(&self) -> bool {
        self.is(b"C")
    }
}

/// ASCII 大小写不敏感比较。
pub fn ci_eq(a: &[u8], b: &[u8]) -> bool {
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

/// 规范化的字符集名。
fn canon_codeset(cs: &[u8], out: &mut [u8; NAME_CAP], o: &mut usize) -> Result<(), u32> {
    let mapped: &[u8] = if ci_eq(cs, b"utf8") || ci_eq(cs, b"utf-8") {
        b"UTF-8"
    } else if ci_eq(cs, b"ascii") || ci_eq(cs, b"us-ascii") {
        b"ASCII"
    } else {
        cs
    };
    let mut i = 0usize;
    while i < mapped.len() {
        if *o >= NAME_CAP {
            return Err(SL_ELEN);
        }
        if let (Some(d), Some(s)) = (out.get_mut(*o), mapped.get(i).copied()) {
            *d = s;
        }
        *o += 1;
        i += 1;
    }
    Ok(())
}

/// 规范化：POSIX→C，字符集别名归并，修饰符保留。
pub fn canonical(raw: &[u8]) -> Result<Name, u32> {
    let n = Name::parse(raw)?;
    let b = n.bytes();
    let mut dot = b.len();
    let mut at = b.len();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i).copied() {
            if c == b'.' && dot == b.len() {
                dot = i;
            }
            if c == b'@' && at == b.len() {
                at = i;
            }
        }
        i += 1;
    }
    if at < dot {
        return Err(SL_EINVAL);
    }
    let base_end = if dot < b.len() { dot } else { at };
    if base_end == 0 {
        return Err(SL_ESHORT);
    }
    let base = b.get(0..base_end).unwrap_or(&[]);
    let mut out = [0u8; NAME_CAP];
    let mut o = 0usize;
    if ci_eq(base, b"posix") {
        if let Some(d) = out.get_mut(0) {
            *d = b'C';
        }
        o = 1;
    } else {
        let mut k = 0usize;
        while k < base.len() {
            if o >= NAME_CAP {
                return Err(SL_ELEN);
            }
            if let (Some(d), Some(s)) = (out.get_mut(o), base.get(k).copied()) {
                *d = s;
            }
            o += 1;
            k += 1;
        }
    }
    if dot < b.len() {
        let cs_end = if at < b.len() { at } else { b.len() };
        let cs = b.get(dot + 1..cs_end).unwrap_or(&[]);
        if cs.is_empty() {
            return Err(SL_EINVAL);
        }
        if o >= NAME_CAP {
            return Err(SL_ELEN);
        }
        if let Some(d) = out.get_mut(o) {
            *d = b'.';
        }
        o += 1;
        canon_codeset(cs, &mut out, &mut o)?;
    }
    if at < b.len() {
        let md = b.get(at + 1..).unwrap_or(&[]);
        if md.is_empty() {
            return Err(SL_EINVAL);
        }
        if o >= NAME_CAP {
            return Err(SL_ELEN);
        }
        if let Some(d) = out.get_mut(o) {
            *d = b'@';
        }
        o += 1;
        let mut k = 0usize;
        while k < md.len() {
            if o >= NAME_CAP {
                return Err(SL_ELEN);
            }
            if let (Some(d), Some(s)) = (out.get_mut(o), md.get(k).copied()) {
                *d = s;
            }
            o += 1;
            k += 1;
        }
    }
    let view = out.get(0..o).unwrap_or(&[]);
    Name::parse(view)
}

/// locale 库。
#[derive(Clone, Copy, Debug)]
pub struct Db {
    names: [Name; DB_CAP],
    len: usize,
}

impl Db {
    /// 空库。
    pub const fn empty() -> Db {
        Db { names: [Name::empty(); DB_CAP], len: 0 }
    }

    /// 内置库。
    pub fn builtin() -> Db {
        let mut d = Db::empty();
        let mut i = 0usize;
        while i < DB_NAMES.len() {
            if let Some(raw) = DB_NAMES.get(i) {
                if let Ok(n) = canonical(raw.as_bytes()) {
                    let _ = d.push(n);
                }
            }
            i += 1;
        }
        d
    }

    /// 追加（拒绝重复：库里同一 locale 只留一份）。
    pub fn push(&mut self, n: Name) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.names.get(i) {
                if v.bytes() == n.bytes() {
                    return Err(SL_EDUP);
                }
            }
            i += 1;
        }
        if self.len >= DB_CAP {
            return Err(SL_ERANGE);
        }
        if let Some(d) = self.names.get_mut(self.len) {
            *d = n;
        }
        self.len += 1;
        Ok(self.len - 1)
    }

    /// 库大小。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 项。
    pub fn at(&self, i: usize) -> Option<Name> {
        if i >= self.len {
            return None;
        }
        self.names.get(i).copied()
    }

    /// 查表：返回库下标，找不到即 `SL_ENOLOC`（= setlocale 返回 NULL）。
    pub fn find(&self, raw: &[u8]) -> Result<usize, u32> {
        let c = canonical(raw)?;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.names.get(i) {
                if v.bytes() == c.bytes() {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(SL_ENOLOC)
    }

    /// 是否可用。
    pub fn available(&self, raw: &[u8]) -> bool {
        self.find(raw).is_ok()
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            newline(&mut s);
            if let Some(n) = self.names.get(i) {
                s.push_str(&n.text());
            }
            i += 1;
        }
        s
    }
}

/// 一次复合请求的提交计划（全量校验通过才允许落地）。
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    cats: [u8; CATS],
    names: [Name; CATS],
    len: usize,
}

impl Plan {
    /// 空计划。
    pub const fn blank() -> Plan {
        Plan { cats: [0; CATS], names: [Name::empty(); CATS], len: 0 }
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取段。
    pub fn at(&self, i: usize) -> Option<(u8, Name)> {
        if i >= self.len {
            return None;
        }
        let c = self.cats.get(i).copied()?;
        let n = self.names.get(i).copied()?;
        Some((c, n))
    }
}

/// 解析复合串：`LC_CTYPE=..;LC_TIME=..`。
pub fn parse_plan(db: &Db, raw: &[u8]) -> Result<Plan, u32> {
    if raw.is_empty() {
        return Err(SL_ESHORT);
    }
    if raw.len() > COMPOSITE_CAP {
        return Err(SL_ELEN);
    }
    let mut p = Plan::blank();
    let mut pos = 0usize;
    let mut guard = 0usize;
    while pos <= raw.len() && guard <= CATS * 2 {
        guard += 1;
        let mut end = pos;
        while end < raw.len() {
            match raw.get(end).copied() {
                Some(b';') => break,
                Some(_) => end += 1,
                None => break,
            }
        }
        let seg = raw.get(pos..end).unwrap_or(&[]);
        if seg.is_empty() {
            return Err(SL_ESHORT);
        }
        let mut eq = seg.len();
        let mut i = 0usize;
        while i < seg.len() {
            if let Some(b) = seg.get(i).copied() {
                if b == b'=' {
                    eq = i;
                    break;
                }
            }
            i += 1;
        }
        if eq == 0 || eq == seg.len() {
            return Err(SL_ESHORT);
        }
        let key = seg.get(0..eq).unwrap_or(&[]);
        let val = seg.get(eq + 1..).unwrap_or(&[]);
        let cat = cat_index(key)?;
        if cat == CAT_ALL {
            // 复合串里不允许再出现 LC_ALL：语义是「逐域覆盖」，不是再次总闸。
            return Err(SL_EINVAL);
        }
        let mut seen = false;
        let mut k = 0usize;
        while k < p.len {
            if p.cats.get(k).copied() == Some(cat) {
                seen = true;
            }
            k += 1;
        }
        if seen {
            return Err(SL_EDUP);
        }
        let idx = db.find(val)?;
        let canon = match db.at(idx) {
            Some(n) => n,
            None => return Err(SL_ENOLOC),
        };
        if p.len >= CATS {
            return Err(SL_ERANGE);
        }
        if let Some(d) = p.cats.get_mut(p.len) {
            *d = cat;
        }
        if let Some(d) = p.names.get_mut(p.len) {
            *d = canon;
        }
        p.len += 1;
        pos = if end < raw.len() { end + 1 } else { end + 1 };
    }
    if p.len == 0 {
        return Err(SL_ESHORT);
    }
    Ok(p)
}

/// 状态快照。
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    names: [Name; CATS],
    version: u32,
}

impl Snapshot {
    /// 版本。
    pub fn version(&self) -> u32 {
        self.version
    }
}

/// setlocale 状态：十二域 + 统计 + 版本。
#[derive(Clone, Copy, Debug)]
pub struct State {
    names: [Name; CATS],
    db: Db,
    version: u32,
    sets: u32,
    queries: u32,
    failures: u32,
    fallbacks: u32,
}

impl State {
    /// 初始状态：十二域全 C。
    pub fn new() -> State {
        let mut names = [Name::empty(); CATS];
        let mut i = 0usize;
        while i < CATS {
            if let Some(d) = names.get_mut(i) {
                *d = Name::parse(b"C").unwrap_or(Name::empty());
            }
            i += 1;
        }
        State {
            names,
            db: Db::builtin(),
            version: 0,
            sets: 0,
            queries: 0,
            failures: 0,
            fallbacks: 0,
        }
    }

    /// 库。
    pub fn db(&self) -> Db {
        self.db
    }

    /// 版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 成功设置次数。
    pub fn sets(&self) -> u32 {
        self.sets
    }

    /// 查询次数。
    pub fn queries(&self) -> u32 {
        self.queries
    }

    /// 失败次数（等价于返回 NULL 的次数）。
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// 降级次数。
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    /// 取某域名。
    pub fn name(&self, cat: usize) -> Result<Name, u32> {
        if cat >= CATS {
            return Err(SL_ERANGE);
        }
        match self.names.get(cat) {
            Some(n) => Ok(*n),
            None => Err(SL_ERANGE),
        }
    }

    /// 十二域是否同值。
    pub fn uniform(&self) -> bool {
        let first = match self.names.first() {
            Some(n) => *n,
            None => return false,
        };
        let mut i = 1usize;
        while i < CATS {
            match self.names.get(i) {
                Some(n) => {
                    if n.bytes() != first.bytes() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 复合渲染：同值返回短名，否则逐域 `LC_X=v` 用 ';' 连接。
    pub fn render_all(&self) -> String {
        if self.uniform() {
            if let Some(n) = self.names.first() {
                return n.text();
            }
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < CATS {
            if i > 0 {
                s.push(';');
            }
            if let Some(n) = cat_name(i as u8) {
                s.push_str(n);
            }
            s.push('=');
            if let Some(v) = self.names.get(i) {
                s.push_str(&v.text());
            }
            i += 1;
        }
        s
    }

    /// 单域查询。
    pub fn query(&mut self, cat: u8) -> Result<String, u32> {
        let c = check_cat(cat)?;
        self.queries = self.queries.saturating_add(1);
        if c == CAT_ALL {
            return Ok(self.render_all());
        }
        match self.names.get(c as usize) {
            Some(n) => Ok(n.text()),
            None => Err(SL_ERANGE),
        }
    }

    /// 单域落地（不记版本/计数，供单域、复合、全设三条路径复用）。
    fn apply_one(&mut self, cat: usize, n: Name) -> u32 {
        let mut changed = 0u32;
        if let Some(d) = self.names.get_mut(cat) {
            if d.bytes() != n.bytes() {
                *d = n;
                changed = 1;
            }
        }
        changed
    }

    /// 单域提交：一次设置只涨一个版本。
    fn commit_one(&mut self, cat: usize, n: Name) -> u32 {
        let changed = self.apply_one(cat, n);
        self.version = self.version.saturating_add(1);
        self.sets = self.sets.saturating_add(1);
        changed
    }

    /// 把某域设为 C（降级路径，单独记版本）。
    fn force_c(&mut self, cat: usize) -> u32 {
        let c = Name::parse(b"C").unwrap_or(Name::empty());
        let changed = self.apply_one(cat, c);
        self.version = self.version.saturating_add(1);
        self.sets = self.sets.saturating_add(1);
        changed
    }

    /// setlocale 主入口：`name == None` 为查询，`Some` 为设置。
    ///
    /// 成功返回新名字（查询时返回当前名字）；失败返回错误码，**状态不变**。
    pub fn setlocale(&mut self, cat: u8, name: Option<&[u8]>) -> Result<String, u32> {
        let c = check_cat(cat)?;
        let raw = match name {
            None => return self.query(c),
            Some(v) => v,
        };
        if raw.is_empty() {
            // `setlocale(cat, "")`：按环境解析。本实现的环境为空，故落 C。
            if c == CAT_ALL {
                let mut i = 0usize;
                let cmark = Name::parse(b"C").unwrap_or(Name::empty());
                while i < CATS {
                    self.apply_one(i, cmark);
                    i += 1;
                }
                self.version = self.version.saturating_add(1);
                self.sets = self.sets.saturating_add(1);
                return Ok(self.render_all());
            }
            self.commit_one(c as usize, Name::parse(b"C")?);
            return Ok(String::from("C"));
        }
        if raw.len() > COMPOSITE_CAP {
            self.failures = self.failures.saturating_add(1);
            return Err(SL_ELEN);
        }
        let has_semi = {
            let mut i = 0usize;
            let mut found = false;
            while i < raw.len() {
                if raw.get(i).copied() == Some(b';') {
                    found = true;
                }
                i += 1;
            }
            found
        };
        if has_semi {
            let plan = match parse_plan(&self.db, raw) {
                Ok(p) => p,
                Err(e) => {
                    self.failures = self.failures.saturating_add(1);
                    return Err(e);
                }
            };
            let mut i = 0usize;
            while i < plan.len() {
                if let Some((pc, pn)) = plan.at(i) {
                    self.apply_one(pc as usize, pn);
                }
                i += 1;
            }
            self.version = self.version.saturating_add(1);
            self.sets = self.sets.saturating_add(1);
            return Ok(self.render_all());
        }
        let canon = match canonical(raw) {
            Ok(v) => v,
            Err(e) => {
                self.failures = self.failures.saturating_add(1);
                return Err(e);
            }
        };
        if self.db.find(canon.bytes()).is_err() {
            self.failures = self.failures.saturating_add(1);
            return Err(SL_ENOLOC);
        }
        if c == CAT_ALL {
            let mut i = 0usize;
            while i < CATS {
                self.apply_one(i, canon);
                i += 1;
            }
            self.version = self.version.saturating_add(1);
            self.sets = self.sets.saturating_add(1);
            return Ok(self.render_all());
        }
        self.commit_one(c as usize, canon);
        Ok(canon.text())
    }

    /// 降级路径：库里没有就落 C，并回报是否降级。
    pub fn fallback_set(&mut self, cat: u8, raw: &[u8]) -> Result<(String, bool), u32> {
        let c = check_cat(cat)?;
        match self.setlocale(cat, Some(raw)) {
            Ok(v) => Ok((v, false)),
            Err(SL_ENOLOC) => {
                self.fallbacks = self.fallbacks.saturating_add(1);
                if c == CAT_ALL {
                    let cmark = Name::parse(b"C").unwrap_or(Name::empty());
                    let mut i = 0usize;
                    while i < CATS {
                        self.apply_one(i, cmark);
                        i += 1;
                    }
                    self.version = self.version.saturating_add(1);
                    self.sets = self.sets.saturating_add(1);
                    Ok((self.render_all(), true))
                } else {
                    self.force_c(c as usize);
                    Ok((String::from("C"), true))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// 快照。
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { names: self.names, version: self.version }
    }

    /// 回滚到快照；版本不符即拒绝（防跨状态误用）。
    pub fn restore(&mut self, snap: &Snapshot) -> Result<u32, u32> {
        if snap.version > self.version {
            return Err(SL_ESTATE);
        }
        let mut changed = 0u32;
        let mut i = 0usize;
        while i < CATS {
            if let (Some(d), Some(s)) = (self.names.get_mut(i), snap.names.get(i)) {
                if d.bytes() != s.bytes() {
                    *d = *s;
                    changed += 1;
                }
            }
            i += 1;
        }
        self.version = self.version.saturating_add(1);
        Ok(changed)
    }

    /// 十二域里等于 C 的个数。
    pub fn count_c(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CATS {
            if let Some(v) = self.names.get(i) {
                if v.is_c() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染十二域（每行一个）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < CATS {
            newline(&mut s);
            if let Some(n) = cat_name(i as u8) {
                s.push_str(n);
            }
            s.push('=');
            if let Some(v) = self.names.get(i) {
                s.push_str(&v.text());
            }
            i += 1;
        }
        s
    }
}

/// 单域设置探针。
fn probe_set() -> Result<u32, u32> {
    let mut st = State::new();
    let name = st.setlocale(2, Some(b"ja_JP.UTF-8"))?;
    Ok(name.len() as u32)
}

/// 查询探针。
fn probe_query() -> Result<u32, u32> {
    let mut st = State::new();
    let n = st.query(0)?;
    Ok(n.len() as u32)
}

/// 全设探针。
fn probe_all() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"en_US.UTF-8"))?;
    Ok((CATS - st.count_c()) as u32)
}

/// 同值短名探针。
fn probe_short_name() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"en_US.UTF-8"))?;
    let s = st.query(CAT_ALL)?;
    Ok(s.len() as u32)
}

/// 复合两域探针。
fn probe_composite() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C.UTF-8;LC_TIME=de_DE.UTF-8"))?;
    let mut n = 0u32;
    if st.name(0).map(|v| v.is(b"C.UTF-8")).unwrap_or(false) {
        n += 1;
    }
    if st.name(2).map(|v| v.is(b"de_DE.UTF-8")).unwrap_or(false) {
        n += 1;
    }
    Ok(n)
}

/// 降级探针。
fn probe_fallback() -> Result<u32, u32> {
    let mut st = State::new();
    let (_name, fell) = st.fallback_set(0, b"xx_YY.UTF-8")?;
    Ok(if fell { 1 } else { 0 })
}

/// 原子性探针。
fn probe_atomic() -> Result<u32, u32> {
    let mut st = State::new();
    st.fallback_set(0, b"xx_YY.UTF-8")?;
    Ok(st.count_c() as u32)
}

/// 库查表探针。
fn probe_lookup() -> Result<u32, u32> {
    let db = Db::builtin();
    db.find(b"en_US.utf8").map(|i| i as u32)
}

/// 库中没有探针。
fn probe_noloc() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"xx_YY.UTF-8")).map(|v| v.len() as u32)
}

/// 复合重复探针。
fn probe_dup() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C;LC_CTYPE=C")).map(|v| v.len() as u32)
}

/// 空段探针（尾部孤立的 ';'）。
fn probe_noshort() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C;")).map(|v| v.len() as u32)
}

/// 超长探针。
fn probe_len() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(0, Some(&[b'x'; NAME_CAP + 1])).map(|v| v.len() as u32)
}

/// 非法字节探针。
fn probe_byte() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(0, Some(b"en US")).map(|v| v.len() as u32)
}

/// 域号越界探针。
fn probe_range() -> Result<u32, u32> {
    let mut st = State::new();
    st.setlocale(0x0d, None).map(|v| v.len() as u32)
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
            return Err(SL_ESLOT);
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
            return Err(SL_EBAL);
        }
        const VALS: [u32; 8] = [11, 1, 12, 11, 2, 1, 12, 3];
        const CODES: [u32; 6] = [SL_ENOLOC, SL_EDUP, SL_ESHORT, SL_ELEN, SL_EBYTE, SL_ERANGE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SL_EBAL),
            };
            if s.slot as usize != i {
                return Err(SL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SL_EVALUE);
                        }
                    }
                    None => return Err(SL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SL_EVALUE);
                        }
                    }
                    None => return Err(SL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SL_EBAL);
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
            return Err(SL_EBAL);
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
            Err(SL_ESEAL)
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

    let _ = l.register(word_cell(0, OP_SET, probe_set()));
    let _ = l.register(word_cell(1, OP_QUERY, probe_query()));
    let _ = l.register(word_cell(2, OP_ALL, probe_all()));
    let _ = l.register(word_cell(3, OP_QUERY, probe_short_name()));
    let _ = l.register(word_cell(4, OP_COMPOSITE, probe_composite()));
    let _ = l.register(word_cell(5, OP_FALLBACK, probe_fallback()));
    let _ = l.register(word_cell(6, OP_ATOMIC, probe_atomic()));
    let _ = l.register(word_cell(7, OP_LOOKUP, probe_lookup()));
    let _ = l.register(word_cell(8, OP_LOOKUP, probe_noloc()));
    let _ = l.register(word_cell(9, OP_COMPOSITE, probe_dup()));
    let _ = l.register(word_cell(10, OP_COMPOSITE, probe_noshort()));
    let _ = l.register(word_cell(11, OP_SET, probe_len()));
    let _ = l.register(word_cell(12, OP_SET, probe_byte()));
    let _ = l.register(word_cell(13, OP_QUERY, probe_range()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let mut st = State::new();
        let a = st.setlocale(2, Some(b"ja_JP.UTF-8")).map(|v| v == String::from("ja_JP.UTF-8"));
        let b = st.name(2).map(|v| v.is(b"ja_JP.UTF-8")).unwrap_or(false);
        let c = st.version() == 1 && st.sets() == 1 && st.failures() == 0;
        a.unwrap_or(false) && b && c
    };

    out[1] = {
        let mut st = State::new();
        let before = st.snapshot();
        let a = st.setlocale(CAT_ALL, Some(b"no_SUCH.UTF-8")).err() == Some(SL_ENOLOC);
        let b = st.version() == 0 && st.count_c() == CATS && st.uniform();
        let c = st.setlocale(CAT_ALL, Some(b"LC_CTYPE=en_US.UTF-8;LC_CTYPE=C")).err() == Some(SL_EDUP);
        let d = st.count_c() == CATS;
        let e = st.restore(&before).map(|n| n == 0).unwrap_or(false);
        a && b && c && d && e
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
        let db = Db::builtin();
        let a = db.find(b"POSIX").map(|i| i == 0).unwrap_or(false);
        let b = db.find(b"en_US.utf8").map(|i| i == 3).unwrap_or(false);
        let c = db.find(b"C.utf8").map(|i| i == 1).unwrap_or(false);
        let d = canonical(b"sr_RS.UTF-8@latin")
            .map(|n| n.is(b"sr_RS.UTF-8@latin"))
            .unwrap_or(false);
        let mut st = State::new();
        let e = st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C.UTF-8;LC_TIME=fr_FR.UTF-8")).is_ok();
        let f = st.name(0).map(|v| v.is(b"C.UTF-8")).unwrap_or(false);
        let g = st.name(2).map(|v| v.is(b"fr_FR.UTF-8")).unwrap_or(false);
        let h = st.name(1).map(|v| v.is_c()).unwrap_or(false);
        let i = st.count_c() == 10;
        a && b && c && d && e && f && g && h && i
    };

    out
}

/// 判据集合。
pub fn run_unx11563_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11563.setlocale");

    s.add(
        "SL01-名字规范化",
        {
            let a = canonical(b"POSIX").map(|n| n.is(b"C")).unwrap_or(false);
            let b = canonical(b"posix").map(|n| n.is(b"C")).unwrap_or(false);
            let c = canonical(b"en_US.utf8").map(|n| n.is(b"en_US.UTF-8")).unwrap_or(false);
            let d = canonical(b"en_US.UTF8").map(|n| n.is(b"en_US.UTF-8")).unwrap_or(false);
            let e = canonical(b"C.utf8").map(|n| n.is(b"C.UTF-8")).unwrap_or(false);
            let f = canonical(b"sr_RS.UTF-8@latin")
                .map(|n| n.is(b"sr_RS.UTF-8@latin"))
                .unwrap_or(false);
            let g = canonical(b"").err() == Some(SL_ESHORT);
            let h = canonical(b"C.").err() == Some(SL_EINVAL);
            let i = canonical(b"C@").err() == Some(SL_EINVAL);
            let j = canonical(b"en US").err() == Some(SL_EBYTE);
            let k = canonical(&[b'x'; NAME_CAP + 1]).err() == Some(SL_ELEN);
            a && b && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "SL02-locale 库",
        {
            let db = Db::builtin();
            let mut ok = db.len() == 8 && !db.is_empty();
            ok = ok && db.find(b"C") == Ok(0);
            ok = ok && db.find(b"POSIX") == Ok(0);
            ok = ok && db.find(b"C.UTF-8") == Ok(1);
            ok = ok && db.find(b"en_US") == Ok(2);
            ok = ok && db.find(b"en_US.utf8") == Ok(3);
            ok = ok && db.find(b"de_DE.UTF-8") == Ok(4);
            ok = ok && db.find(b"ja_JP.utf8") == Ok(5);
            ok = ok && db.find(b"sr_RS.utf8@latin") == Ok(7);
            ok = ok && db.find(b"xx_YY").err() == Some(SL_ENOLOC);
            ok = ok && db.available(b"fr_FR.UTF-8") && !db.available(b"fr_FR");
            ok = ok && db.at(8).is_none() && db.at(0).map(|n| n.is(b"C")).unwrap_or(false);
            ok
        },
        "static",
    );

    s.add(
        "SL03-单域设置与查询",
        {
            let mut st = State::new();
            let a = st.setlocale(2, Some(b"ja_JP.UTF-8")) == Ok(String::from("ja_JP.UTF-8"));
            let b = st.name(2).map(|v| v.is(b"ja_JP.UTF-8")).unwrap_or(false);
            let c = st.name(0).map(|v| v.is_c()).unwrap_or(false);
            let d = st.version() == 1 && st.sets() == 1;
            let e = st.setlocale(2, None) == Ok(String::from("ja_JP.UTF-8"));
            let f = st.queries() == 1;
            let g = st.setlocale(2, Some(b"de_DE.utf8")) == Ok(String::from("de_DE.UTF-8"));
            let h = st.version() == 2;
            let i = st.setlocale(3, Some(b"C")) == Ok(String::from("C"));
            let j = st.count_c() == 11;
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "SL04-全设与短名",
        {
            let mut st = State::new();
            let a = st.setlocale(CAT_ALL, Some(b"en_US.UTF-8")) == Ok(String::from("en_US.UTF-8"));
            let b = st.uniform();
            let c = st.count_c() == 0;
            let d = st.name(11).map(|v| v.is(b"en_US.UTF-8")).unwrap_or(false);
            let e = st.setlocale(CAT_ALL, None) == Ok(String::from("en_US.UTF-8"));
            let f = st.version() == 1;
            let g = st.setlocale(CAT_ALL, Some(b"POSIX")) == Ok(String::from("C"));
            let h = st.count_c() == CATS;
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SL05-复合原子性",
        {
            let mut st = State::new();
            let a = st
                .setlocale(CAT_ALL, Some(b"LC_CTYPE=C.UTF-8;LC_TIME=fr_FR.UTF-8"))
                .is_ok();
            let b = st.name(0).map(|v| v.is(b"C.UTF-8")).unwrap_or(false);
            let c = st.name(2).map(|v| v.is(b"fr_FR.UTF-8")).unwrap_or(false);
            let d = st.count_c() == 10;
            let before = st.snapshot();
            let e = st
                .setlocale(CAT_ALL, Some(b"LC_COLLATE=de_DE.UTF-8;LC_COLLATE=C"))
                .err()
                == Some(SL_EDUP);
            let f = st.version() == before.version();
            let g = st.name(3).map(|v| v.is_c()).unwrap_or(false);
            let h = st
                .setlocale(CAT_ALL, Some(b"LC_COLLATE=de_DE.UTF-8;LC_NUMERIC=xx_YY"))
                .err()
                == Some(SL_ENOLOC);
            let i = st.name(3).map(|v| v.is_c()).unwrap_or(false);
            let j = st.restore(&before).map(|n| n == 0).unwrap_or(false);
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "SL06-降级路径",
        {
            let mut st = State::new();
            let a = st.fallback_set(0, b"xx_YY.UTF-8") == Ok((String::from("C"), true));
            let b = st.fallbacks() == 1;
            let c = st.name(0).map(|v| v.is_c()).unwrap_or(false);
            let d = st.fallback_set(4, b"de_DE.UTF-8") == Ok((String::from("de_DE.UTF-8"), false));
            let e = st.name(4).map(|v| v.is(b"de_DE.UTF-8")).unwrap_or(false);
            let f = st.fallbacks() == 1;
            let g = st.fallback_set(1, b"bad name").err() == Some(SL_EBYTE);
            let h = st.name(1).map(|v| v.is_c()).unwrap_or(false);
            let i = st.failures() == 2;
            let j = st.fallback_set(CAT_ALL, b"xx_YY") == Ok((String::from("C"), true));
            let k = st.count_c() == CATS;
            a && b && c && d && e && f && g && h && i && j && k
        },
        "static",
    );

    s.add(
        "SL07-域号闸门",
        {
            let mut st = State::new();
            let a = check_cat(0) == Ok(0);
            let b = check_cat(11) == Ok(11);
            let c = check_cat(CAT_ALL) == Ok(CAT_ALL);
            let d = check_cat(0x0d) == Err(SL_ERANGE);
            let e = st.setlocale(0x0d, None).err() == Some(SL_ERANGE);
            let f = st.setlocale(0x0d, Some(b"C")).err() == Some(SL_ERANGE);
            let g = cat_index(b"LC_ALL") == Ok(CAT_ALL);
            let h = cat_index(b"LC_CTYPE") == Ok(0);
            let i = cat_index(b"LC_FOO") == Err(SL_EINVAL);
            let j = cat_index(b"") == Err(SL_ESHORT);
            let k = cat_name(CAT_ALL) == Some("LC_ALL");
            let l = cat_name(0x0d).is_none();
            a && b && c && d && e && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "SL08-复合串语法",
        {
            let db = Db::builtin();
            let a = parse_plan(&db, b"LC_CTYPE=C").map(|p| p.len()) == Ok(1);
            let b = parse_plan(&db, b"LC_CTYPE=C;LC_TIME=C").map(|p| p.len()) == Ok(2);
            let c = parse_plan(&db, b"").err() == Some(SL_ESHORT);
            let d = parse_plan(&db, b"LC_CTYPE").err() == Some(SL_ESHORT);
            let e = parse_plan(&db, b"LC_CTYPE=").err() == Some(SL_ESHORT);
            let f = parse_plan(&db, b"=C").err() == Some(SL_ESHORT);
            let g = parse_plan(&db, b"LC_NOPE=C").err() == Some(SL_EINVAL);
            let h = parse_plan(&db, b"LC_ALL=C").err() == Some(SL_EINVAL);
            let i = parse_plan(&db, b"LC_CTYPE=C;").err() == Some(SL_ESHORT);
            let j = parse_plan(&db, b"LC_CTYPE=C;LC_CTYPE=C").err() == Some(SL_EDUP);
            let k = parse_plan(&db, b"LC_CTYPE=zz_ZZ").err() == Some(SL_ENOLOC);
            let l = match parse_plan(&db, b"LC_CTYPE=POSIX") {
                Ok(p) => p.at(0).map(|(c, n)| c == 0 && n.is(b"C")).unwrap_or(false),
                Err(_) => false,
            };
            a && b && c && d && e && f && g && h && i && j && k && l
        },
        "static",
    );

    s.add(
        "SL09-复合查询渲染",
        {
            let mut st = State::new();
            let _ = st.setlocale(0, Some(b"en_US.UTF-8"));
            let _ = st.setlocale(2, Some(b"ja_JP.UTF-8"));
            let q = st.query(CAT_ALL).unwrap_or_default();
            let a = q.split(';').count() == CATS;
            let b = q.starts_with("LC_CTYPE=en_US.UTF-8;LC_NUMERIC=C");
            let c = q.contains("LC_TIME=ja_JP.UTF-8");
            let d = !st.uniform();
            let e = st.render().split('\n').count() == CATS;
            let f = st.render().starts_with("LC_CTYPE=en_US.UTF-8");
            let g = st.setlocale(CAT_ALL, Some(b"C")).map(|v| v == String::from("C")).unwrap_or(false);
            let h = st.uniform() && st.count_c() == CATS;
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SL10-快照回滚",
        {
            let mut st = State::new();
            let snap = st.snapshot();
            let a = snap.version() == 0;
            let _ = st.setlocale(CAT_ALL, Some(b"en_US.UTF-8"));
            let b = st.version() == 1;
            let c = st.restore(&snap).map(|n| n as usize == CATS).unwrap_or(false);
            let d = st.count_c() == CATS;
            let e = st.version() == 2;
            let f = {
                let mut other = State::new();
                let _ = other.setlocale(0, Some(b"en_US.UTF-8"));
                let _ = other.setlocale(0, Some(b"C"));
                let _ = other.setlocale(0, Some(b"C"));
                let s2 = other.snapshot();
                let mut fresh = State::new();
                fresh.restore(&s2).err() == Some(SL_ESTATE)
            };
            let g = st.restore(&snap).map(|n| n == 0).unwrap_or(false);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SL11-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES && !l.is_empty();
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = l.sample(0).map(|v| v.value) == Some(11);
            let e = l.sample(8).map(|v| v.code) == Some(SL_ENOLOC);
            let f = l.sample(13).map(|v| v.code) == Some(SL_ERANGE);
            let g = l.sample(7).map(|v| v.value) == Some(3);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SL12-封印重放",
        {
            let mut l = build_ledger();
            let sealed = l.seal();
            let a = sealed != 0 && l.root() == 0;
            let b = l.replay().map(|v| v == sealed).unwrap_or(false);
            let c = l.freeze() == Ok(sealed) && l.is_frozen() && l.root() == sealed;
            let d = l.register(Sample::blank()).err() == Some(SL_ESLOT);
            let e = l.rejects() == 1;
            let f = l.freeze().err() == Some(SL_EBAL) && l.rejects() == 2;
            let mut g = build_ledger();
            if let Some(dst) = g.rows.get_mut(1) {
                *dst = Sample { slot: 1, op: OP_QUERY, ok: true, code: 0, value: 2 };
            }
            let h = g.audit().err() == Some(SL_EVALUE);
            a && b && c && d && e && f && h
        },
        "static",
    );

    s.add(
        "SL13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut st = State::new();
            ok = ok && st.setlocale(0, Some(b"C.UTF-8")).is_ok();
            ok = ok && st.setlocale(0, Some(b"POSIX")).is_ok();
            ok = ok && st.setlocale(0, None) == Ok(String::from("C"));
            let before = st.snapshot();
            ok = ok && st.setlocale(0, Some(b"xx_YY")).err() == Some(SL_ENOLOC);
            ok = ok && st.restore(&before).map(|n| n == 0).unwrap_or(false);
            ok = ok && st.fallbacks() == 0 && st.failures() == 1;
            let mut l = build_ledger();
            ok = ok && l.audit().is_ok() && l.freeze().is_ok() && l.replay().is_ok();
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
    fn canonical_names() {
        assert!(canonical(b"POSIX").expect("c").is(b"C"));
        assert!(canonical(b"C").expect("c").is(b"C"));
        assert!(canonical(b"en_US.utf8").expect("c").is(b"en_US.UTF-8"));
        assert!(canonical(b"C.UTF8").expect("c").is(b"C.UTF-8"));
        assert_eq!(canonical(b"").err(), Some(SL_ESHORT));
        assert_eq!(canonical(b"C.").err(), Some(SL_EINVAL));
        assert_eq!(canonical(b"en US").err(), Some(SL_EBYTE));
    }

    #[test]
    fn db_lookup() {
        let db = Db::builtin();
        assert_eq!(db.len(), 8);
        assert_eq!(db.find(b"POSIX"), Ok(0));
        assert_eq!(db.find(b"en_US.utf8"), Ok(3));
        assert_eq!(db.find(b"xx_YY"), Err(SL_ENOLOC));
        assert!(db.available(b"ja_JP.UTF-8"));
        assert!(!db.available(b"ja_JP"));
    }

    #[test]
    fn single_set() {
        let mut st = State::new();
        assert_eq!(
            st.setlocale(2, Some(b"ja_JP.UTF-8")),
            Ok(String::from("ja_JP.UTF-8"))
        );
        assert!(st.name(2).expect("n").is(b"ja_JP.UTF-8"));
        assert!(st.name(0).expect("n").is_c());
        assert_eq!(st.version(), 1);
    }

    #[test]
    fn query_all_uniform() {
        let mut st = State::new();
        st.setlocale(CAT_ALL, Some(b"en_US.UTF-8")).expect("set");
        assert_eq!(st.query(CAT_ALL), Ok(String::from("en_US.UTF-8")));
        assert!(st.uniform());
        assert_eq!(st.count_c(), 0);
    }

    #[test]
    fn query_all_mixed() {
        let mut st = State::new();
        st.setlocale(0, Some(b"en_US.UTF-8")).expect("set");
        st.setlocale(2, Some(b"ja_JP.UTF-8")).expect("set");
        let q = st.query(CAT_ALL).expect("q");
        assert_eq!(q.split(';').count(), CATS);
        assert!(q.contains("LC_TIME=ja_JP.UTF-8"));
        assert!(!st.uniform());
    }

    #[test]
    fn unknown_locale_keeps_state() {
        let mut st = State::new();
        st.setlocale(0, Some(b"en_US.UTF-8")).expect("set");
        let snap = st.snapshot();
        assert_eq!(st.setlocale(0, Some(b"xx_YY")), Err(SL_ENOLOC));
        assert!(st.name(0).expect("n").is(b"en_US.UTF-8"));
        assert_eq!(st.failures(), 1);
        assert_eq!(st.version(), snap.version());
    }

    #[test]
    fn composite_commit() {
        let mut st = State::new();
        st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C.UTF-8;LC_TIME=fr_FR.UTF-8"))
            .expect("set");
        assert!(st.name(0).expect("n").is(b"C.UTF-8"));
        assert!(st.name(2).expect("n").is(b"fr_FR.UTF-8"));
        assert_eq!(st.count_c(), 10);
    }

    #[test]
    fn composite_atomic_on_dup() {
        let mut st = State::new();
        let snap = st.snapshot();
        assert_eq!(
            st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C;LC_CTYPE=C")),
            Err(SL_EDUP)
        );
        assert_eq!(st.version(), snap.version());
        assert_eq!(st.count_c(), CATS);
    }

    #[test]
    fn composite_atomic_on_missing() {
        let mut st = State::new();
        assert_eq!(
            st.setlocale(CAT_ALL, Some(b"LC_CTYPE=C.UTF-8;LC_TIME=xx_YY")),
            Err(SL_ENOLOC)
        );
        assert!(st.name(0).expect("n").is_c());
        assert_eq!(st.count_c(), CATS);
    }

    #[test]
    fn fallback_paths() {
        let mut st = State::new();
        assert_eq!(
            st.fallback_set(0, b"xx_YY"),
            Ok((String::from("C"), true))
        );
        assert_eq!(st.fallbacks(), 1);
        assert_eq!(
            st.fallback_set(0, b"en_US.UTF-8"),
            Ok((String::from("en_US.UTF-8"), false))
        );
        assert_eq!(st.fallbacks(), 1);
    }

    #[test]
    fn cat_range() {
        assert_eq!(check_cat(12), Ok(CAT_ALL));
        assert_eq!(check_cat(13), Err(SL_ERANGE));
        let mut st = State::new();
        assert_eq!(st.setlocale(13, None), Err(SL_ERANGE));
        assert_eq!(st.setlocale(13, Some(b"C")), Err(SL_ERANGE));
    }

    #[test]
    fn plan_syntax() {
        let db = Db::builtin();
        assert_eq!(parse_plan(&db, b"LC_CTYPE=C").map(|p| p.len()), Ok(1));
        assert_eq!(parse_plan(&db, b"").err(), Some(SL_ESHORT));
        assert_eq!(parse_plan(&db, b"LC_CTYPE").err(), Some(SL_ESHORT));
        assert_eq!(parse_plan(&db, b"LC_ALL=C").err(), Some(SL_EINVAL));
        assert_eq!(parse_plan(&db, b"LC_NOPE=C").err(), Some(SL_EINVAL));
    }

    #[test]
    fn plan_canonicalises() {
        let db = Db::builtin();
        let p = parse_plan(&db, b"LC_TIME=POSIX").expect("plan");
        let (c, n) = p.at(0).expect("seg");
        assert_eq!(c, 2);
        assert!(n.is(b"C"));
    }

    #[test]
    fn snapshot_restore() {
        let mut st = State::new();
        let snap = st.snapshot();
        st.setlocale(CAT_ALL, Some(b"en_US.UTF-8")).expect("set");
        assert_eq!(st.restore(&snap), Ok(CATS as u32));
        assert_eq!(st.count_c(), CATS);
        assert_eq!(st.restore(&snap), Ok(0));
    }

    #[test]
    fn version_gate() {
        let mut a = State::new();
        a.setlocale(0, Some(b"C")).expect("set");
        let s = a.snapshot();
        let mut b = State::new();
        assert_eq!(b.restore(&s).err(), Some(SL_ESTATE));
    }

    #[test]
    fn empty_name_resets() {
        let mut st = State::new();
        st.setlocale(CAT_ALL, Some(b"en_US.UTF-8")).expect("set");
        assert_eq!(st.setlocale(CAT_ALL, Some(b"")), Ok(String::from("C")));
        assert_eq!(st.count_c(), CATS);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(7).map(|v| v.value), Some(3));
    }

    #[test]
    fn ledger_seal_freeze() {
        let mut l = build_ledger();
        let s = l.seal();
        assert_eq!(l.replay().expect("replay"), s);
        assert_eq!(l.freeze().expect("freeze"), s);
        assert!(l.is_frozen());
        assert_eq!(l.register(Sample::blank()).err(), Some(SL_ESLOT));
    }

    #[test]
    fn ledger_tamper_value() {
        let mut l = build_ledger();
        if let Some(d) = l.rows.get_mut(0) {
            d.value = 9;
        }
        assert_eq!(l.audit().err(), Some(SL_EVALUE));
    }

    #[test]
    fn inject_all_true() {
        let i = inject_four();
        assert!(i[0] && i[1] && i[2] && i[3]);
    }

    #[test]
    fn checks_all_pass() {
        let c = run_unx11563_checks();
        assert!(c.all_passed());
        assert_eq!(c.passed_count(), 13);
    }
}
