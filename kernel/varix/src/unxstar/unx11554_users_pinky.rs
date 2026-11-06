//! UNX-F11554 users/pinky 登录查询
//!
//! `users` 与 `pinky` 看的是同一份 utmp，但呈现方式完全相反：
//!
//! - **`users`** 只要一行——把所有登录名用**单个空格**串起来，不去重、不换行、
//!   不以空格结尾。它的可用性全靠这一行：字符串一长，脚本里的 `read` 就会
//!   悄悄截断，所以这里既给 `render` 也留了长度闸门（超 `USERS_MAX` 报
//!   `PU_ELEN`），并把「不能有尾空格」当成硬判据。
//! - **`pinky`** 是短表六列（Login / Name / TTY / Idle / When / Where），列宽
//!   固定，靠补空格对齐；`-l` 切成竖排长档，每行一个字段；`-b`/`-f` 去表头；
//!   `-w` 只挑一个人。短表的 When 是 `Mon DD HH:MM`，Idle 是「分:秒」，
//!   未登录满一分钟的显示 `0:00`，未知则留空。
//!
//! 记录行的解析是全部判据的公共底座：五段空白分隔 `user tty HH:MM idle where`，
//! 段数不对是 `PU_EBYTE`，时钟越界是 `PU_ETIME`，idle 负数或超长是 `PU_ERANGE`。
//! 空白段只认 `0x20`，制表符在基准机的 utmp 文本档里就是坏字节。
//!
//! 闸门次序：选项字面（`PU_EINVAL`）→ 字段长度（`PU_ELEN`）→ 字段字节
//! （`PU_EBYTE`）→ 数值范围（`PU_ERANGE`）→ 时钟合法（`PU_ETIME`）→
//! 重复终端（`PU_EDUP`）→ 查无此人（`PU_ENOENT`）→ 选项冲突（`PU_ESTATE`）。
//! 时钟在范围之后：`24:00` 是「值本身越界」，先把数值合法性坐实再谈时分。
//!
//! 账本语义：十四格 = 八格成功（短表行、表头、空格串、idle 文本、when 文本、
//! 记录解析、长档行数、筛选命中）+ 六格失败（段数、idle 负数、`24:00`、
//! 重复终端、查无此人、坏选项）。`audit` 认「八值六码」，`seal` 折叠指纹，
//! `replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3b0;

/// 选项字面非法。
pub const PU_EINVAL: u32 = SEG_BASE + 0x01;
/// 字段过长。
pub const PU_ELEN: u32 = SEG_BASE + 0x02;
/// 字段字节非法。
pub const PU_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界。
pub const PU_ERANGE: u32 = SEG_BASE + 0x04;
/// 终端重复。
pub const PU_EDUP: u32 = SEG_BASE + 0x05;
/// 查无此人。
pub const PU_ENOENT: u32 = SEG_BASE + 0x06;
/// 时钟不合法。
pub const PU_ETIME: u32 = SEG_BASE + 0x07;
/// 选项冲突。
pub const PU_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const PU_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const PU_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const PU_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const PU_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const PU_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 名字容量。
pub const NAMECAP: usize = 32;

/// `users` 单行上限。
pub const USERS_MAX: usize = 256;

/// 表里最多几条记录。
pub const ROWS: usize = 8;

/// Login 列宽。
pub const W_LOGIN: usize = 8;
/// Name 列宽。
pub const W_NAME: usize = 10;
/// TTY 列宽。
pub const W_TTY: usize = 8;
/// Idle 列宽。
pub const W_IDLE: usize = 6;
/// When 列宽。
pub const W_WHEN: usize = 16;

/// 操作码：短表行。
pub const OP_SHORT: u8 = 0x01;
/// 操作码：表头。
pub const OP_HEAD: u8 = 0x02;
/// 操作码：空格串。
pub const OP_USERS: u8 = 0x03;
/// 操作码：idle 文本。
pub const OP_IDLE: u8 = 0x04;
/// 操作码：when 文本。
pub const OP_WHEN: u8 = 0x05;
/// 操作码：记录解析。
pub const OP_REC: u8 = 0x06;
/// 操作码：长档。
pub const OP_LONG: u8 = 0x07;
/// 操作码：筛选。
pub const OP_SELECT: u8 = 0x08;

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

/// 定长名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAMECAP],
    len: u8,
}

impl Name {
    /// 空名。
    pub const fn empty() -> Name {
        Name { buf: [0; NAMECAP], len: 0 }
    }

    /// 解析。
    pub fn parse(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() {
            return Err(PU_EBYTE);
        }
        if raw.len() > NAMECAP {
            return Err(PU_ELEN);
        }
        let mut buf = [0u8; NAMECAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(PU_EBYTE),
            };
            if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'/') {
                return Err(PU_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: raw.len() as u8 })
    }

    /// 从文本解析。
    pub fn parse_str(s: &str) -> Result<Name, u32> {
        Name::parse(s.as_bytes())
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
        match self.buf.get(..self.len as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 追加到文本。
    pub fn push_into(&self, s: &mut String) {
        let b = self.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
    }

    /// 转文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_into(&mut s);
        s
    }
}

/// 时分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    hour: u8,
    min: u8,
}

impl Clock {
    /// 构造（越界给 `PU_ETIME`）。
    pub fn new(hour: u32, min: u32) -> Result<Clock, u32> {
        if hour > 23 || min > 59 {
            return Err(PU_ETIME);
        }
        Ok(Clock { hour: hour as u8, min: min as u8 })
    }

    /// 解析 `HH:MM`。
    pub fn parse(raw: &[u8]) -> Result<Clock, u32> {
        if raw.len() != 5 {
            return Err(PU_EBYTE);
        }
        let h = match raw.get(0..2) {
            Some(v) => v,
            None => return Err(PU_EBYTE),
        };
        let sep = match raw.get(2) {
            Some(v) => *v,
            None => return Err(PU_EBYTE),
        };
        if sep != b':' {
            return Err(PU_EBYTE);
        }
        let m = match raw.get(3..5) {
            Some(v) => v,
            None => return Err(PU_EBYTE),
        };
        let hv = two_digits(h)?;
        let mv = two_digits(m)?;
        Clock::new(hv, mv)
    }

    /// 时。
    pub fn hour(&self) -> u8 {
        self.hour
    }

    /// 分。
    pub fn min(&self) -> u8 {
        self.min
    }

    /// 文本 `HH:MM`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push((b'0' + self.hour / 10) as char);
        s.push((b'0' + self.hour % 10) as char);
        s.push(':');
        s.push((b'0' + self.min / 10) as char);
        s.push((b'0' + self.min % 10) as char);
        s
    }
}

/// 两位十进制。
fn two_digits(raw: &[u8]) -> Result<u32, u32> {
    let a = match raw.get(0) {
        Some(v) => *v,
        None => return Err(PU_EBYTE),
    };
    let b = match raw.get(1) {
        Some(v) => *v,
        None => return Err(PU_EBYTE),
    };
    if !a.is_ascii_digit() || !b.is_ascii_digit() {
        return Err(PU_EBYTE);
    }
    Ok(((a - b'0') as u32) * 10 + (b - b'0') as u32)
}

/// 月日时分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct When {
    mon: u8,
    day: u8,
    clock: Clock,
}

impl When {
    /// 构造。
    pub fn new(mon: u32, day: u32, clock: Clock) -> Result<When, u32> {
        if mon < 1 || mon > 12 || day < 1 || day > 31 {
            return Err(PU_ERANGE);
        }
        Ok(When { mon: mon as u8, day: day as u8, clock })
    }

    /// 月。
    pub fn mon(&self) -> u8 {
        self.mon
    }

    /// 日。
    pub fn day(&self) -> u8 {
        self.day
    }

    /// 时。
    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// 文本 `Mon DD HH:MM`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push(month_char(self.mon, 0));
        s.push(month_char(self.mon, 1));
        s.push(month_char(self.mon, 2));
        s.push(' ');
        if self.day < 10 {
            s.push(' ');
            s.push((b'0' + self.day) as char);
        } else {
            s.push((b'0' + self.day / 10) as char);
            s.push((b'0' + self.day % 10) as char);
        }
        s.push(' ');
        let t = self.clock.text();
        let b = t.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        s
    }
}

/// 月份三字母。
fn month_char(mon: u8, idx: usize) -> char {
    let tag = match mon {
        1 => b"Jan",
        2 => b"Feb",
        3 => b"Mar",
        4 => b"Apr",
        5 => b"May",
        6 => b"Jun",
        7 => b"Jul",
        8 => b"Aug",
        9 => b"Sep",
        10 => b"Oct",
        11 => b"Nov",
        12 => b"Dec",
        _ => b"???",
    };
    match tag.get(idx) {
        Some(v) => *v as char,
        None => '?',
    }
}

/// 空闲时长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Idle {
    known: bool,
    secs: u32,
}

impl Idle {
    /// 未知。
    pub const fn unknown() -> Idle {
        Idle { known: false, secs: 0 }
    }

    /// 已知秒数。
    pub fn from_secs(secs: u32) -> Idle {
        Idle { known: true, secs }
    }

    /// 解析：`-` 表示未知，否则十进制秒。
    pub fn parse(raw: &[u8]) -> Result<Idle, u32> {
        if raw.is_empty() {
            return Err(PU_EBYTE);
        }
        if raw == b"-" {
            return Ok(Idle::unknown());
        }
        if raw.len() > 9 {
            return Err(PU_ERANGE);
        }
        let mut v = 0u32;
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(x) => *x,
                None => return Err(PU_EBYTE),
            };
            if !b.is_ascii_digit() {
                return Err(PU_ERANGE);
            }
            v = v.saturating_mul(10).saturating_add((b - b'0') as u32);
            i += 1;
        }
        Ok(Idle { known: true, secs: v })
    }

    /// 是否已知。
    pub fn known(&self) -> bool {
        self.known
    }

    /// 秒数。
    pub fn secs(&self) -> u32 {
        self.secs
    }

    /// 文本：未知留空，不足一分钟给 `0:00`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if !self.known {
            return s;
        }
        let m = self.secs / 60;
        let sec = self.secs % 60;
        if m >= 100 {
            s.push('?');
            return s;
        }
        if m >= 10 {
            s.push((b'0' + (m / 10) as u8) as char);
        }
        s.push((b'0' + (m % 10) as u8) as char);
        s.push(':');
        s.push((b'0' + (sec / 10) as u8) as char);
        s.push((b'0' + (sec % 10) as u8) as char);
        s
    }
}

/// 一条登录记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    user: Name,
    tty: Name,
    when: When,
    idle: Idle,
    host: Name,
}

impl Record {
    /// 构造。
    pub fn new(user: Name, tty: Name, when: When, idle: Idle, host: Name) -> Record {
        Record { user, tty, when, idle, host }
    }

    /// 用户。
    pub fn user(&self) -> &Name {
        &self.user
    }

    /// 终端。
    pub fn tty(&self) -> &Name {
        &self.tty
    }

    /// 时间。
    pub fn when(&self) -> When {
        self.when
    }

    /// 空闲。
    pub fn idle(&self) -> Idle {
        self.idle
    }

    /// 来源主机。
    pub fn host(&self) -> &Name {
        &self.host
    }

    /// 解析一行。
    pub fn parse_line(line: &str) -> Result<Record, u32> {
        let b = line.as_bytes();
        if b.is_empty() {
            return Err(PU_EBYTE);
        }
        let mut fields: [[u8; NAMECAP]; 5] = [[0; NAMECAP]; 5];
        let mut lens: [usize; 5] = [0; 5];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < b.len() {
            let c = match b.get(i) {
                Some(v) => *v,
                None => return Err(PU_EBYTE),
            };
            if c == b' ' {
                i += 1;
                continue;
            }
            if c == b'\t' {
                return Err(PU_EBYTE);
            }
            if n >= 5 {
                return Err(PU_EBYTE);
            }
            let mut k = 0usize;
            while i < b.len() {
                let d = match b.get(i) {
                    Some(v) => *v,
                    None => return Err(PU_EBYTE),
                };
                if d == b' ' || d == b'\t' {
                    break;
                }
                if k >= NAMECAP {
                    return Err(PU_ELEN);
                }
                if let Some(slot) = fields.get_mut(n) {
                    if let Some(dst) = slot.get_mut(k) {
                        *dst = d;
                    }
                }
                k += 1;
                i += 1;
            }
            if let Some(l) = lens.get_mut(n) {
                *l = k;
            }
            n += 1;
        }
        if n != 5 {
            return Err(PU_EBYTE);
        }
        let user = Name::parse(cut(&fields, &lens, 0)?)?;
        let tty = Name::parse(cut(&fields, &lens, 1)?)?;
        let clock = Clock::parse(cut(&fields, &lens, 2)?)?;
        let idle = Idle::parse(cut(&fields, &lens, 3)?)?;
        let host = Name::parse(cut(&fields, &lens, 4)?)?;
        let when = When::new(10, 30, clock)?;
        Ok(Record { user, tty, when, idle, host })
    }
}

/// 取第 i 个字段的切片。
fn cut<'a>(fields: &'a [[u8; NAMECAP]; 5], lens: &[usize; 5], i: usize) -> Result<&'a [u8], u32> {
    let row = match fields.get(i) {
        Some(v) => v,
        None => return Err(PU_ENOBS),
    };
    let n = match lens.get(i) {
        Some(v) => *v,
        None => return Err(PU_ENOBS),
    };
    match row.get(..n) {
        Some(v) => Ok(v),
        None => Err(PU_EBYTE),
    }
}

/// 补空格到指定列宽。
fn pad(s: &mut String, text: &Name, width: usize) {
    text.push_into(s);
    let mut fill = width.saturating_sub(text.len());
    while fill > 0 {
        s.push(' ');
        fill -= 1;
    }
}

/// 补文本到指定列宽。
fn pad_text(s: &mut String, text: &str, width: usize) {
    let b = text.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    let mut fill = width.saturating_sub(b.len());
    while fill > 0 {
        s.push(' ');
        fill -= 1;
    }
}

/// 短表表头。
pub fn short_head() -> String {
    let mut s = String::new();
    pad_text(&mut s, "Login", W_LOGIN);
    pad_text(&mut s, " Name", W_NAME);
    pad_text(&mut s, "TTY", W_TTY);
    pad_text(&mut s, "Idle", W_IDLE);
    pad_text(&mut s, "When", W_WHEN);
    s.push_str("Where");
    s
}

/// 短表一行。
pub fn short_line(rec: &Record, real: Option<&str>) -> Result<String, u32> {
    if rec.user.is_empty() {
        return Err(PU_ENOBS);
    }
    let mut s = String::new();
    pad(&mut s, &rec.user, W_LOGIN);
    match real {
        Some(v) => pad_text(&mut s, v, W_NAME),
        None => pad(&mut s, &Name::empty(), W_NAME),
    }
    pad(&mut s, &rec.tty, W_TTY);
    pad_text(&mut s, &rec.idle.text(), W_IDLE);
    pad_text(&mut s, &rec.when.text(), W_WHEN);
    rec.host.push_into(&mut s);
    if s.len() > USERS_MAX {
        return Err(PU_ELEN);
    }
    Ok(s)
}

/// 长档（每行一个字段）。
pub fn long_block(rec: &Record, real: Option<&str>) -> Result<String, u32> {
    if rec.user.is_empty() {
        return Err(PU_ENOBS);
    }
    let mut s = String::new();
    s.push_str("Login name: ");
    rec.user.push_into(&mut s);
    s.push('\n');
    s.push_str("In real life: ");
    match real {
        Some(v) => pad_text(&mut s, v, v.len()),
        None => {}
    }
    s.push('\n');
    s.push_str("Terminal: ");
    rec.tty.push_into(&mut s);
    s.push('\n');
    s.push_str("Idle: ");
    pad_text(&mut s, &rec.idle.text(), rec.idle.text().len());
    s.push('\n');
    s.push_str("When: ");
    pad_text(&mut s, &rec.when.text(), rec.when.text().len());
    s.push('\n');
    s.push_str("Where: ");
    rec.host.push_into(&mut s);
    if s.len() > USERS_MAX {
        return Err(PU_ELEN);
    }
    Ok(s)
}

/// `users` 一行。
pub fn render_users(rows: &[Record]) -> Result<String, u32> {
    if rows.is_empty() {
        return Err(PU_ENOBS);
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(PU_ENOBS),
        };
        if r.user.is_empty() {
            return Err(PU_EBYTE);
        }
        if i > 0 {
            s.push(' ');
        }
        r.user.push_into(&mut s);
        if s.len() > USERS_MAX {
            return Err(PU_ELEN);
        }
        i += 1;
    }
    if let Some(last) = s.as_bytes().last() {
        if *last == b' ' {
            return Err(PU_EBYTE);
        }
    }
    Ok(s)
}

/// 登录名计数（同名多次登录分开数）。
pub fn count_names(rows: &[Record]) -> usize {
    rows.len()
}

/// 短表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    rows: [Record; ROWS],
    len: usize,
}

impl Table {
    /// 空表。
    pub const fn new() -> Table {
        Table {
            rows: [Record {
                user: Name::empty(),
                tty: Name::empty(),
                when: When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } },
                idle: Idle::unknown(),
                host: Name::empty(),
            }; ROWS],
            len: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Record> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 追加（同终端重复给 `PU_EDUP`，满了给 `PU_ERANGE`）。
    pub fn push(&mut self, r: Record) -> Result<(), u32> {
        if self.len >= ROWS {
            return Err(PU_ERANGE);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if v.tty == r.tty {
                    return Err(PU_EDUP);
                }
            }
            i += 1;
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = r;
        }
        self.len += 1;
        Ok(())
    }

    /// 按用户筛。
    pub fn select(&self, user: &str) -> Result<Table, u32> {
        if user.is_empty() {
            return Err(PU_EINVAL);
        }
        let mut out = Table::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if v.user.eq_bytes(user.as_bytes()) {
                    out.push(*v)?;
                }
            }
            i += 1;
        }
        if out.is_empty() {
            return Err(PU_ENOENT);
        }
        Ok(out)
    }

    /// 按终端查。
    pub fn find_tty(&self, tty: &str) -> Result<Record, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if v.tty.eq_bytes(tty.as_bytes()) {
                    return Ok(*v);
                }
            }
            i += 1;
        }
        Err(PU_ENOENT)
    }

    /// 渲染短表。
    pub fn render(&self, head: bool) -> Result<String, u32> {
        if self.is_empty() {
            return Err(PU_ENOBS);
        }
        let mut s = String::new();
        if head {
            let h = short_head();
            let hb = h.as_bytes();
            let mut i = 0usize;
            while i < hb.len() {
                if let Some(c) = hb.get(i) {
                    s.push(*c as char);
                }
                i += 1;
            }
            s.push('\n');
        }
        let mut k = 0usize;
        while k < self.len {
            let r = match self.rows.get(k) {
                Some(v) => *v,
                None => return Err(PU_ENOBS),
            };
            let line = short_line(&r, None)?;
            let lb = line.as_bytes();
            let mut j = 0usize;
            while j < lb.len() {
                if let Some(c) = lb.get(j) {
                    s.push(*c as char);
                }
                j += 1;
            }
            s.push('\n');
            k += 1;
        }
        Ok(s)
    }
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    long: bool,
    short: bool,
    head: bool,
    real: bool,
    only: bool,
    user: Name,
}

impl Opts {
    /// 空选项。
    pub const fn blank() -> Opts {
        Opts { long: false, short: false, head: true, real: true, only: false, user: Name::empty() }
    }

    /// 解析。
    pub fn parse(line: &str) -> Result<Opts, u32> {
        let mut o = Opts::blank();
        let b = line.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            let c = match b.get(i) {
                Some(v) => *v,
                None => return Err(PU_EBYTE),
            };
            if c == b' ' {
                i += 1;
                continue;
            }
            if c != b'-' {
                return Err(PU_EINVAL);
            }
            let f = match b.get(i + 1) {
                Some(v) => *v,
                None => return Err(PU_EINVAL),
            };
            i += 2;
            match f {
                b'l' => o.long = true,
                b's' => o.short = true,
                b'b' | b'f' => o.head = false,
                b'i' => o.real = false,
                b'w' => {
                    let v = take_word(b, &mut i)?;
                    o.user = Name::parse(v)?;
                    o.only = true;
                }
                _ => return Err(PU_EINVAL),
            }
        }
        if o.long && o.short {
            return Err(PU_ESTATE);
        }
        if o.only && o.user.is_empty() {
            return Err(PU_EINVAL);
        }
        Ok(o)
    }

    /// 是否长档。
    pub fn is_long(&self) -> bool {
        self.long
    }

    /// 是否要表头。
    pub fn wants_head(&self) -> bool {
        self.head && !self.long
    }

    /// 是否显示真名。
    pub fn wants_real(&self) -> bool {
        self.real
    }

    /// 是否只挑一人。
    pub fn is_only(&self) -> bool {
        self.only
    }

    /// 筛选的人名。
    pub fn user(&self) -> &Name {
        &self.user
    }
}

/// 取一个空格分隔的词。
fn take_word<'a>(bytes: &'a [u8], i: &mut usize) -> Result<&'a [u8], u32> {
    let mut start = *i;
    while start < bytes.len() {
        match bytes.get(start) {
            Some(b' ') => start += 1,
            _ => break,
        }
    }
    let mut end = start;
    while end < bytes.len() {
        match bytes.get(end) {
            Some(b' ') => break,
            _ => end += 1,
        }
    }
    if end == start {
        return Err(PU_EINVAL);
    }
    *i = end;
    match bytes.get(start..end) {
        Some(v) => Ok(v),
        None => Err(PU_EBYTE),
    }
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
            return Err(PU_ESLOT);
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
            return Err(PU_EBAL);
        }
        const VALS: [u32; 8] = [54, 53, 15, 4, 12, 5, 6, 1];
        const CODES: [u32; 6] = [PU_EBYTE, PU_ERANGE, PU_ETIME, PU_EDUP, PU_ENOENT, PU_EINVAL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PU_EBAL),
            };
            if s.slot as usize != i {
                return Err(PU_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PU_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PU_EVALUE);
                        }
                    }
                    None => return Err(PU_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PU_EVALUE);
                        }
                    }
                    None => return Err(PU_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PU_EBAL);
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
            return Err(PU_EBAL);
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
            Err(PU_ESEAL)
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

/// 三条登录记录。
fn build_table() -> Table {
    let mut t = Table::new();
    let a = match Record::parse_line("alice pts/2 13:23 90 host-a") {
        Ok(v) => v,
        Err(_) => return t,
    };
    let b = match Record::parse_line("bob tty1 09:05 - localhost") {
        Ok(v) => v,
        Err(_) => return t,
    };
    let c = match Record::parse_line("carol ttyS0 11:40 1800 host-b") {
        Ok(v) => v,
        Err(_) => return t,
    };
    let _ = t.push(a);
    let _ = t.push(b);
    let _ = t.push(c);
    t
}

/// 表里的全部记录（不足补空记录）。
pub fn table_rows() -> [Record; 3] {
    let t = build_table();
    let blank = Record::new(
        Name::empty(),
        Name::empty(),
        When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } },
        Idle::unknown(),
        Name::empty(),
    );
    let mut out = [blank; 3];
    let mut i = 0usize;
    while i < 3 {
        if let Some(v) = t.at(i) {
            if let Some(dst) = out.get_mut(i) {
                *dst = v;
            }
        }
        i += 1;
    }
    out
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let t = build_table();
    let rows = table_rows();
    let first = match rows.first() {
        Some(v) => *v,
        None => Record::new(
            Name::empty(),
            Name::empty(),
            When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } },
            Idle::unknown(),
            Name::empty(),
        ),
    };

    let _ = l.register(count_cell(0, OP_SHORT, short_line(&first, None).map(|s| s.len())));

    let _ = l.register(count_cell(1, OP_HEAD, Ok(short_head().len())));

    let _ = l.register(count_cell(2, OP_USERS, render_users(&rows).map(|s| s.len())));

    let _ = l.register(count_cell(3, OP_IDLE, Ok(Idle::from_secs(90).text().len())));

    let _ = l.register(count_cell(4, OP_WHEN, Ok(first.when().text().len())));

    let _ = l.register(word_cell(5, OP_REC, Record::parse_line("dave pts/3 08:15 30 host-c").map(|r| r.tty().len() as u32)));

    let _ = l.register(count_cell(6, OP_LONG, long_block(&first, Some("Alice A")).map(|s| s.lines().count())));

    let _ = l.register(count_cell(7, OP_SELECT, t.select("bob").map(|v| v.len())));

    let _ = l.register(count_cell(8, OP_REC, Record::parse_line("alice pts/2 13:23").map(|r| r.tty().len())));

    let _ = l.register(count_cell(9, OP_IDLE, Idle::parse(b"-5").map(|i| i.secs() as usize)));

    let _ = l.register(word_cell(10, OP_WHEN, Clock::parse(b"24:00").map(|c| c.hour() as u32)));

    let _ = l.register(word_cell(11, OP_REC, {
        let mut t2 = Table::new();
        let dup = Record::parse_line("alice pts/2 13:23 90 host-a");
        match dup {
            Ok(r) => match t2.push(r) {
                Ok(()) => match t2.push(r) {
                    Ok(()) => Ok(0),
                    Err(c) => Err(c),
                },
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(count_cell(12, OP_SELECT, t.select("nobody").map(|v| v.len())));

    let _ = l.register(word_cell(13, OP_REC, Opts::parse("-Z").map(|o| o.wants_head() as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let t = build_table();

    let a = match t.at(0) {
        Some(r) => match short_line(&r, None) {
            Ok(line) => line.starts_with("alice   ") && line.len() == 54,
            Err(_) => false,
        },
        None => false,
    };
    out[0] = a;

    let b = match render_users(&table_rows()) {
        Ok(v) => v == String::from("alice bob carol"),
        Err(_) => false,
    };
    out[1] = b;

    let c = match Idle::parse(b"3600") {
        Ok(i) => i.text() == String::from("60:00"),
        Err(_) => false,
    };
    out[2] = c;

    let d = match Opts::parse("-l") {
        Ok(o) => o.is_long() && !o.wants_head(),
        Err(_) => false,
    };
    out[3] = d;

    out
}

/// 判据集合。
pub fn run_unx11554_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11554.users");

    s.add(
        "PU01-记录解析",
        {
            let a = Record::parse_line("alice pts/2 13:23 90 host-a");
            let b = match a {
                Ok(r) => {
                    r.user().eq_bytes(b"alice")
                        && r.tty().eq_bytes(b"pts/2")
                        && r.idle().known()
                        && r.idle().secs() == 90
                        && r.host().eq_bytes(b"host-a")
                }
                Err(_) => false,
            };
            let c = Record::parse_line("alice pts/2 13:23").is_err()
                && Record::parse_line("alice\tpts/2 13:23 90 host-a") == Err(PU_EBYTE)
                && Record::parse_line("") == Err(PU_EBYTE);
            b && c
        },
        "static",
    );

    s.add(
        "PU02-时钟解析",
        {
            let a = Clock::parse(b"00:00").map(|c| (c.hour(), c.min())) == Ok((0, 0));
            let b = Clock::parse(b"23:59").map(|c| (c.hour(), c.min())) == Ok((23, 59));
            let c = Clock::parse(b"24:00") == Err(PU_ETIME);
            let d = Clock::parse(b"12:60") == Err(PU_ETIME);
            let e = Clock::parse(b"1:23") == Err(PU_EBYTE)
                && Clock::parse(b"12-30") == Err(PU_EBYTE)
                && Clock::parse(b"ab:cd") == Err(PU_EBYTE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "PU03-when 文本",
        {
            let a = match Clock::new(13, 23) {
                Ok(c) => match When::new(10, 30, c) {
                    Ok(w) => w.text() == String::from("Oct 30 13:23"),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let b = match Clock::new(9, 5) {
                Ok(c) => match When::new(1, 5, c) {
                    Ok(w) => w.text() == String::from("Jan  5 09:05"),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let c = match Clock::new(0, 0) {
                Ok(cl) => When::new(13, 1, cl) == Err(PU_ERANGE) && When::new(12, 32, cl) == Err(PU_ERANGE),
                Err(_) => false,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "PU04-idle 文本",
        {
            let a = Idle::from_secs(0).text() == String::from("0:00");
            let b = Idle::from_secs(90).text() == String::from("1:30");
            let c = Idle::from_secs(3600).text() == String::from("60:00");
            let d = Idle::unknown().text() == String::from("");
            let e = Idle::parse(b"-") == Ok(Idle::unknown())
                && Idle::parse(b"45") == Ok(Idle::from_secs(45))
                && Idle::parse(b"-5") == Err(PU_ERANGE)
                && Idle::parse(b"") == Err(PU_EBYTE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "PU05-空格串",
        {
            let t = build_table();
            let rows = [
                match t.at(0) {
                    Some(v) => v,
                    None => return s,
                },
                match t.at(1) {
                    Some(v) => v,
                    None => return s,
                },
                match t.at(2) {
                    Some(v) => v,
                    None => return s,
                },
            ];
            let a = render_users(&rows) == Ok(String::from("alice bob carol"));
            let b = match render_users(&rows) {
                Ok(v) => !v.ends_with(' ') && !v.contains('\n') && v.len() == 15,
                Err(_) => false,
            };
            a && b
        },
        "static",
    );

    s.add(
        "PU06-短表",
        {
            let t = build_table();
            let a = t.len() == 3;
            let b = t.render(true).map(|v| v.lines().count()) == Ok(4);
            let c = t.render(false).map(|v| v.lines().count()) == Ok(3);
            let d = match t.at(0) {
                Some(r) => short_line(&r, Some("Alice A")).map(|l| l.len()) == Ok(54),
                None => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "PU07-表头列宽",
        {
            let h = short_head();
            let a = h.starts_with("Login   ");
            let b = h.ends_with("When            Where");
            let c = h.len() == W_LOGIN + W_NAME + W_TTY + W_IDLE + W_WHEN + 5;
            let d = short_head() == h;
            a && b && c && d
        },
        "static",
    );

    s.add(
        "PU08-终端唯一",
        {
            let t = build_table();
            let a = t.find_tty("tty1").map(|r| r.user().eq_bytes(b"bob")) == Ok(true);
            let b = t.find_tty("pts/9") == Err(PU_ENOENT);
            let mut t2 = Table::new();
            let r = match Record::parse_line("alice pts/2 13:23 90 host-a") {
                Ok(v) => v,
                Err(_) => Record::new(Name::empty(), Name::empty(), When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } }, Idle::unknown(), Name::empty()),
            };
            let c = t2.push(r).is_ok() && t2.push(r) == Err(PU_EDUP);
            a && b && c
        },
        "static",
    );

    s.add(
        "PU09-按人筛选",
        {
            let t = build_table();
            let a = t.select("bob").map(|v| v.len()) == Ok(1);
            let b = t.select("nobody") == Err(PU_ENOENT);
            let c = t.select("") == Err(PU_EINVAL);
            let d = match t.select("alice") {
                Ok(v) => match v.at(0) {
                    Some(r) => r.tty().eq_bytes(b"pts/2"),
                    None => false,
                },
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "PU10-选项",
        {
            let a = Opts::parse("-l").map(|o| o.is_long() && !o.wants_head()) == Ok(true);
            let b = Opts::parse("-s").map(|o| !o.is_long() && o.wants_head()) == Ok(true);
            let c = Opts::parse("-b").map(|o| !o.wants_head()) == Ok(true);
            let d = Opts::parse("-l -s") == Err(PU_ESTATE);
            let e = Opts::parse("-Z") == Err(PU_EINVAL)
                && Opts::parse("-w") == Err(PU_EINVAL)
                && Opts::parse("x") == Err(PU_EINVAL);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "PU11-长档",
        {
            let t = build_table();
            let r = match t.at(0) {
                Some(v) => v,
                None => Record::new(Name::empty(), Name::empty(), When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } }, Idle::unknown(), Name::empty()),
            };
            let a = long_block(&r, Some("Alice A")).map(|v| v.lines().count()) == Ok(6);
            let b = match long_block(&r, Some("Alice A")) {
                Ok(v) => v.starts_with("Login name: alice\n") && v.contains("Terminal: pts/2\n"),
                Err(_) => false,
            };
            let c = long_block(&r, None).map(|v| v.lines().count()) == Ok(6);
            a && b && c
        },
        "static",
    );

    s.add(
        "PU12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(54)
                && l.sample(13).map(|x| x.code) == Some(PU_EINVAL)
                && l.sample(14).is_none()
        },
        "static",
    );

    s.add(
        "PU13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        match Record::parse_line(line) {
            Ok(v) => v,
            Err(_) => Record::new(
                Name::empty(),
                Name::empty(),
                When { mon: 1, day: 1, clock: Clock { hour: 0, min: 0 } },
                Idle::unknown(),
                Name::empty(),
            ),
        }
    }

    #[test]
    fn parse_vectors() {
        let r = rec("alice pts/2 13:23 90 host-a");
        assert!(r.user().eq_bytes(b"alice"));
        assert_eq!(r.idle().secs(), 90);
        assert_eq!(Record::parse_line("a b 12:00"), Err(PU_EBYTE));
        assert_eq!(Record::parse_line("a b 25:00 1 c"), Err(PU_ETIME));
    }

    #[test]
    fn clock_vectors() {
        assert_eq!(Clock::parse(b"07:05").map(|c| c.text()), Ok(String::from("07:05")));
        assert_eq!(Clock::parse(b"24:00"), Err(PU_ETIME));
        assert_eq!(Clock::parse(b"12:5"), Err(PU_EBYTE));
    }

    #[test]
    fn idle_vectors() {
        assert_eq!(Idle::from_secs(30).text(), String::from("0:30"));
        assert_eq!(Idle::from_secs(61).text(), String::from("1:01"));
        assert_eq!(Idle::unknown().text(), String::from(""));
        assert_eq!(Idle::parse(b"abc"), Err(PU_ERANGE));
    }

    #[test]
    fn users_line() {
        let a = rec("alice pts/2 13:23 90 host-a");
        let b = rec("bob tty1 09:05 - localhost");
        let s = match render_users(&[a, b]) {
            Ok(v) => v,
            Err(_) => String::new(),
        };
        assert_eq!(s, String::from("alice bob"));
        assert_eq!(render_users(&[]), Err(PU_ENOBS));
    }

    #[test]
    fn table_vectors() {
        let t = build_table();
        assert_eq!(t.len(), 3);
        assert_eq!(t.find_tty("ttyS0").map(|r| r.user().eq_bytes(b"carol")), Ok(true));
        assert_eq!(t.select("nobody"), Err(PU_ENOENT));
    }

    #[test]
    fn head_vectors() {
        let h = short_head();
        assert_eq!(h.len(), 53);
        assert!(h.starts_with("Login"));
        assert!(h.ends_with("Where"));
    }

    #[test]
    fn ledger_vectors() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.replay(), Ok(l.seal()));
        assert!(l.sample(13).is_some());
    }
}
