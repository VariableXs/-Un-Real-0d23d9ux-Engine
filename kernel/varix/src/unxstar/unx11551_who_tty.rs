//! UNX-F11551 who/tty 终端身份件
//!
//! 这一卡把「谁在哪台终端上」这件事拆成四层，从字节到行文本逐层收紧：
//!
//! - **终端名**（`Dev`）：utmp 里存的是 `pts/3`、`tty1`、`console` 这种**去掉了
//!   `/dev/` 前缀**的设备名。用户输入 `/dev/pts/3` 也能认，但要归一化回同一种
//!   写法；`/tmp/x` 这类「看起来像设备、其实不在 /dev 下」的输入必须拒掉，否则
//!   后面拼路径就会指到别处。空名、双斜杠、首尾斜杠、空白与控制字节一律拒绝——
//!   utmp 记录是定长字段，任何不可打印字节都会让 `who` 的输出错列。
//! - **登录时刻**（`Stamp`）：存的是 UTC 秒。日历换算用 Howard Hinnant 的
//!   `days_from_civil` / `civil_from_days` 互逆算法，不查表、不依赖闰年数组，
//!   闰年判定用 `4/100/400` 三条规则。月份、日、时、分、秒全部逐项校验，
//!   日期越界报范围错而不是悄悄滚动到下个月。
//! - **空闲时长**（`Idle`）：`who -u` 的第四列。三段语义——不足一分钟是 `.`、
//!   超过一天是 `old`、其余是 `HH:MM`；「未知」单独用 `?` 表示，与「正好零秒」
//!   区分开。负数时长是非法输入（时间倒流），报时间错。
//! - **会话表**（`Table`）：定长数组，按 tty 唯一。重复 tty 报重、越界报范围错；
//!   删除找不到报不存在；排序用字节序（`LC_ALL=C` 下 `who` 的行序就是字节序）。
//!
//! `who` 的行格式是**定宽列**：用户 8、终端 12、登录 12，之后按开关追加空闲、
//! 消息状态、PID、远端主机。表头行用同一套宽度函数生成，保证表头与数据永远对齐；
//! 单行超过 `COLCAP` 报长度错，避免把终端撑破。
//!
//! `tty` 只做一件事：把标准输入对应的设备名打出来（带 `/dev/` 前缀），不是终端
//! 就报不存在；`-s` 档只给退出状态、不输出。
//!
//! 闸门次序：终端名（`WH_EINVAL`/`WH_ELEN`/`WH_EBYTE`）→ 登录时刻
//! （`WH_ERANGE`）→ 空闲（`WH_ETIME`）→ 会话表（`WH_EDUP`/`WH_ENOENT`）→
//! 选项组合（`WH_ESTATE`）。先名字后内容：名字不合法时，后面算什么都不作数。
//!
//! 账本语义：十四格 = 八格成功（三格历法、两格空闲、一格 tty、一格计数、
//! 一格排序）+ 六格失败（重复终端、月份越界、负空闲、无终端、字段坏字节、
//! 选项互斥）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，
//! `freeze` 后拒绝。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf380;

/// 参数不合法（空名、绝对路径不在 /dev、首尾或重复斜杠）。
pub const WH_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字超过定长字段。
pub const WH_ELEN: u32 = SEG_BASE + 0x02;
/// 名字里出现不可打印或分隔用字节。
pub const WH_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界（日期、分秒、容量、PID）。
pub const WH_ERANGE: u32 = SEG_BASE + 0x04;
/// 终端重复登记。
pub const WH_EDUP: u32 = SEG_BASE + 0x05;
/// 查不到（终端不在表里、标准输入不是终端）。
pub const WH_ENOENT: u32 = SEG_BASE + 0x06;
/// 时间非法（负数空闲）。
pub const WH_ETIME: u32 = SEG_BASE + 0x07;
/// 状态不允许（选项互斥）。
pub const WH_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本不平衡。
pub const WH_EBAL: u32 = SEG_BASE + 0x09;
/// 账本槽位不连续。
pub const WH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 账本观测值不符。
pub const WH_EVALUE: u32 = SEG_BASE + 0x0b;
/// 重放指纹不符。
pub const WH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 槽位没有观测。
pub const WH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 用户名容量（不含结尾）。
pub const NAMECAP: usize = 32;
/// 终端名容量（不含 `/dev/` 前缀）。
pub const TTYCAP: usize = 24;
/// 主机名容量。
pub const HOSTCAP: usize = 40;
/// 会话表容量。
pub const ROWCAP: usize = 16;
/// 单行输出上限。
pub const COLCAP: usize = 120;
/// 空闲档位：不足一分钟显示点。
pub const IDLE_DOT: i64 = 60;
/// 空闲档位：超过一天显示 old。
pub const IDLE_OLD: i64 = 86_400;
/// 一天的秒数。
pub const DAYSEC: i64 = 86_400;
/// 用户列宽。
pub const W_USER: usize = 8;
/// 终端列宽。
pub const W_TTY: usize = 12;
/// 登录时刻列宽。
pub const W_TIME: usize = 12;
/// 空闲列宽。
pub const W_IDLE: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：登录时刻。
pub const OP_STAMP: u8 = 1;
/// 操作码：文本渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：tty 设备。
pub const OP_TTY: u8 = 3;
/// 操作码：登记。
pub const OP_ADD: u8 = 4;
/// 操作码：解析。
pub const OP_PARSE: u8 = 5;
/// 操作码：计数。
pub const OP_COUNT: u8 = 6;
/// 操作码：排序。
pub const OP_SORT: u8 = 7;

/// FNV-1a 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 八字节折叠。
fn fold(mut h: u64, v: u64) -> u64 {
    let mut x = v;
    let mut i = 0usize;
    while i < 8 {
        h ^= x & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        x >>= 8;
        i += 1;
    }
    h
}

/// 两位十进制（0..=99，越界按 0 处理）。
fn push2(s: &mut String, v: i64) {
    let x = if v < 0 || v > 99 { 0 } else { v };
    let hi = (x / 10) as u8;
    let lo = (x % 10) as u8;
    s.push((b'0' + hi) as char);
    s.push((b'0' + lo) as char);
}

/// 无符号十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 && n < 20 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
    }
}

/// 有符号十进制（年份用）。
fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_u64(s, (0i64.wrapping_sub(v)) as u64);
    } else {
        push_u64(s, v as u64);
    }
}

/// 把 `s` 补空格到第 `col` 列。
fn pad_to(s: &mut String, col: usize) {
    while s.len() < col {
        s.push(' ');
    }
}

/// 定宽左对齐。
fn push_pad(s: &mut String, bytes: &[u8], width: usize) {
    let start = s.len();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(c) = bytes.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    pad_to(s, start + width);
}

/// 用户名：可打印、无空格、无冒号。
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

    /// 从字节解析。
    pub fn parse_bytes(raw: &[u8]) -> Result<Name, u32> {
        if raw.is_empty() {
            return Err(WH_EINVAL);
        }
        if raw.len() > NAMECAP {
            return Err(WH_ELEN);
        }
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(WH_EBYTE),
            };
            if b <= 0x20 || b >= 0x7f || b == b':' {
                return Err(WH_EBYTE);
            }
            i += 1;
        }
        let mut buf = [0u8; NAMECAP];
        let mut k = 0usize;
        while k < raw.len() {
            if let (Some(dst), Some(src)) = (buf.get_mut(k), raw.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        Ok(Name { buf, len: raw.len() as u8 })
    }

    /// 从字符串解析。
    pub fn parse(s: &str) -> Result<Name, u32> {
        Name::parse_bytes(s.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空。
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

    /// 与字节串比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 与字符串比较。
    pub fn eq_str(&self, other: &str) -> bool {
        self.eq_bytes(other.as_bytes())
    }

    /// 追加到缓冲区。
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

    /// 转成字符串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_into(&mut s);
        s
    }
}

/// 终端设备名（不含 `/dev/` 前缀）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dev {
    buf: [u8; TTYCAP],
    len: u8,
}

/// 终端名允许的字节。
fn dev_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'/' || b == b'-' || b == b'_' || b == b'.'
}

impl Dev {
    /// 空设备。
    pub const fn empty() -> Dev {
        Dev { buf: [0; TTYCAP], len: 0 }
    }

    /// 归一化解析：剥掉 `/dev/`，拒绝 `/dev` 之外的绝对路径。
    pub fn parse_bytes(raw: &[u8]) -> Result<Dev, u32> {
        let mut body = raw;
        if body.starts_with(b"/dev/") {
            body = match body.get(5..) {
                Some(v) => v,
                None => return Err(WH_EINVAL),
            };
        } else if body.first() == Some(&b'/') {
            return Err(WH_EINVAL);
        }
        if body.is_empty() {
            return Err(WH_EINVAL);
        }
        if body.len() > TTYCAP {
            return Err(WH_ELEN);
        }
        let mut buf = [0u8; TTYCAP];
        let mut i = 0usize;
        while i < body.len() {
            let b = match body.get(i) {
                Some(v) => *v,
                None => return Err(WH_EBYTE),
            };
            if !dev_byte(b) {
                return Err(WH_EBYTE);
            }
            if b == b'/' {
                if i == 0 {
                    return Err(WH_EINVAL);
                }
                if i + 1 == body.len() {
                    return Err(WH_EINVAL);
                }
                if let Some(prev) = body.get(i - 1) {
                    if *prev == b'/' {
                        return Err(WH_EINVAL);
                    }
                }
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Dev { buf, len: body.len() as u8 })
    }

    /// 从字符串解析。
    pub fn parse(s: &str) -> Result<Dev, u32> {
        Dev::parse_bytes(s.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空。
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

    /// 与字节串比较。
    pub fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_bytes() == other
    }

    /// 与字符串比较。
    pub fn eq_str(&self, other: &str) -> bool {
        self.eq_bytes(other.as_bytes())
    }

    /// 带 `/dev/` 前缀的完整路径。
    pub fn path(&self) -> String {
        let mut s = String::new();
        s.push_str("/dev/");
        self.push_into(&mut s);
        s
    }

    /// 追加裸名。
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

    /// 转成字符串。
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_into(&mut s);
        s
    }

    /// 设备类别。
    pub fn kind(&self) -> Kind {
        kind_of(self)
    }
}

/// 终端类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 系统控制台。
    Console,
    /// 伪终端从端（`pts/N`）。
    Pts,
    /// 串口（`ttyS0`）。
    Serial,
    /// 虚拟控制台（`tty1`）。
    Vc,
    /// 其它。
    Unknown,
}

impl Kind {
    /// 类别标签。
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Console => "console",
            Kind::Pts => "pts",
            Kind::Serial => "serial",
            Kind::Vc => "vc",
            Kind::Unknown => "other",
        }
    }
}

/// 判定终端类别。
pub fn kind_of(d: &Dev) -> Kind {
    let b = d.as_bytes();
    if b == b"console" {
        return Kind::Console;
    }
    if b.starts_with(b"pts/") {
        return Kind::Pts;
    }
    if b.starts_with(b"ttyS") {
        let rest = match b.get(4..) {
            Some(v) => v,
            None => return Kind::Serial,
        };
        let mut ok = !rest.is_empty();
        let mut i = 0usize;
        while i < rest.len() {
            match rest.get(i) {
                Some(c) => {
                    if !c.is_ascii_digit() {
                        ok = false;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if ok {
            return Kind::Serial;
        }
    }
    if b.starts_with(b"tty") {
        let rest = match b.get(3..) {
            Some(v) => v,
            None => return Kind::Vc,
        };
        let mut ok = !rest.is_empty();
        let mut i = 0usize;
        while i < rest.len() {
            match rest.get(i) {
                Some(c) => {
                    if !c.is_ascii_digit() {
                        ok = false;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if ok {
            return Kind::Vc;
        }
    }
    Kind::Unknown
}

/// 闰年。
pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 某月天数；非法月份返回 0。
pub fn month_days(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 公历年月日 → 自 1970-01-01 起的天数。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let yy = if m <= 2 { y - 1 } else { y };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 天数 → 公历年月日。
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    (yr, m as u32, d as u32)
}

/// 拆开的日历时刻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    /// 年。
    pub year: i64,
    /// 月。
    pub month: u32,
    /// 日。
    pub day: u32,
    /// 时。
    pub hour: u32,
    /// 分。
    pub minute: u32,
    /// 秒。
    pub second: u32,
}

/// 登录时刻（UTC 秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    secs: i64,
}

/// 月份缩写。
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl Stamp {
    /// 由秒构造。
    pub const fn from_secs(s: i64) -> Stamp {
        Stamp { secs: s }
    }

    /// 年月日时分秒构造，逐项校验。
    pub fn from_ymdhms(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Result<Stamp, u32> {
        if y < 1970 || y > 2100 {
            return Err(WH_ERANGE);
        }
        if mo < 1 || mo > 12 {
            return Err(WH_ERANGE);
        }
        let md = month_days(y, mo);
        if d < 1 || d > md {
            return Err(WH_ERANGE);
        }
        if h > 23 || mi > 59 || s > 59 {
            return Err(WH_ERANGE);
        }
        let days = days_from_civil(y, mo, d);
        let secs = days * DAYSEC + (h as i64) * 3600 + (mi as i64) * 60 + s as i64;
        Ok(Stamp { secs })
    }

    /// 秒。
    pub fn secs(&self) -> i64 {
        self.secs
    }

    /// 自纪元起的天数。
    pub fn days(&self) -> i64 {
        self.secs.div_euclid(DAYSEC)
    }

    /// 当天秒数。
    pub fn within_day(&self) -> i64 {
        self.secs.rem_euclid(DAYSEC)
    }

    /// 拆开。
    pub fn parts(&self) -> Date {
        let (y, m, d) = civil_from_days(self.days());
        let rem = self.within_day();
        Date {
            year: y,
            month: m,
            day: d,
            hour: (rem / 3600) as u32,
            minute: ((rem % 3600) / 60) as u32,
            second: (rem % 60) as u32,
        }
    }

    /// 星期（0 = 周日）。
    pub fn weekday(&self) -> u32 {
        (((self.days() + 4) % 7 + 7) % 7) as u32
    }

    /// 星期缩写。
    pub fn weekday_label(&self) -> &'static str {
        const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        match NAMES.get(self.weekday() as usize) {
            Some(v) => v,
            None => "???",
        }
    }

    /// `YYYY-MM-DD HH:MM`。
    pub fn long(&self) -> String {
        let mut s = String::new();
        self.long_into(&mut s);
        s
    }

    /// 追加 `YYYY-MM-DD HH:MM`。
    pub fn long_into(&self, s: &mut String) {
        let p = self.parts();
        push_i64(s, p.year);
        s.push('-');
        push2(s, p.month as i64);
        s.push('-');
        push2(s, p.day as i64);
        s.push(' ');
        push2(s, p.hour as i64);
        s.push(':');
        push2(s, p.minute as i64);
    }

    /// `MMM DD HH:MM`。
    pub fn short(&self) -> String {
        let mut s = String::new();
        self.short_into(&mut s);
        s
    }

    /// 追加 `MMM DD HH:MM`。
    pub fn short_into(&self, s: &mut String) {
        let p = self.parts();
        let idx = if p.month >= 1 && p.month <= 12 { (p.month - 1) as usize } else { 0 };
        match MONTHS.get(idx) {
            Some(v) => s.push_str(v),
            None => s.push_str("???"),
        }
        s.push(' ');
        push2(s, p.day as i64);
        s.push(' ');
        push2(s, p.hour as i64);
        s.push(':');
        push2(s, p.minute as i64);
    }
}

/// 空闲时长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Idle {
    secs: i64,
    known: bool,
}

impl Idle {
    /// 未知（`?`）。
    pub const fn unknown() -> Idle {
        Idle { secs: 0, known: false }
    }

    /// 已知秒数。
    pub const fn from_secs(s: i64) -> Idle {
        Idle { secs: s, known: true }
    }

    /// 秒数。
    pub fn secs(&self) -> i64 {
        self.secs
    }

    /// 是否已知。
    pub fn known(&self) -> bool {
        self.known
    }

    /// 是否处于「点」档。
    pub fn is_fresh(&self) -> bool {
        self.known && self.secs >= 0 && self.secs < IDLE_DOT
    }

    /// 是否处于「old」档。
    pub fn is_old(&self) -> bool {
        self.known && self.secs >= IDLE_OLD
    }

    /// 渲染第四列。
    pub fn render(&self) -> Result<String, u32> {
        if !self.known {
            return Ok(String::from("?"));
        }
        if self.secs < 0 {
            return Err(WH_ETIME);
        }
        if self.secs < IDLE_DOT {
            return Ok(String::from("."));
        }
        if self.secs >= IDLE_OLD {
            return Ok(String::from("old"));
        }
        let mut s = String::new();
        push2(&mut s, self.secs / 3600);
        s.push(':');
        push2(&mut s, (self.secs % 3600) / 60);
        Ok(s)
    }
}

/// 写消息状态（`who -T`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesg {
    /// 允许写。
    On,
    /// 禁止写。
    Off,
    /// 未知。
    Unknown,
}

impl Mesg {
    /// 单字符标记。
    pub fn mark(&self) -> char {
        match self {
            Mesg::On => '+',
            Mesg::Off => '-',
            Mesg::Unknown => '?',
        }
    }
}

/// 一条会话。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    user: Name,
    tty: Dev,
    host: Name,
    login: Stamp,
    idle: Idle,
    pid: u32,
    mesg: Mesg,
}

impl Entry {
    /// 空白格。
    pub const fn blank() -> Entry {
        Entry {
            user: Name::empty(),
            tty: Dev::empty(),
            host: Name::empty(),
            login: Stamp::from_secs(0),
            idle: Idle::unknown(),
            pid: 0,
            mesg: Mesg::Unknown,
        }
    }

    /// 构造。
    pub fn new(
        user: &str,
        tty: &str,
        host: &str,
        login: Stamp,
        idle: Idle,
        pid: u32,
    ) -> Result<Entry, u32> {
        let u = Name::parse(user)?;
        let t = Dev::parse(tty)?;
        let h = if host.is_empty() { Name::empty() } else { Name::parse(host)? };
        if pid == 0 {
            return Err(WH_ERANGE);
        }
        Ok(Entry { user: u, tty: t, host: h, login, idle, pid, mesg: Mesg::Unknown })
    }

    /// 用户名。
    pub fn user(&self) -> &Name {
        &self.user
    }

    /// 终端。
    pub fn tty(&self) -> &Dev {
        &self.tty
    }

    /// 远端主机。
    pub fn host(&self) -> &Name {
        &self.host
    }

    /// 登录时刻。
    pub fn login(&self) -> Stamp {
        self.login
    }

    /// 空闲。
    pub fn idle(&self) -> Idle {
        self.idle
    }

    /// PID。
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 消息状态。
    pub fn mesg(&self) -> Mesg {
        self.mesg
    }

    /// 设置消息状态。
    pub fn with_mesg(mut self, m: Mesg) -> Entry {
        self.mesg = m;
        self
    }

    /// 设置空闲。
    pub fn with_idle(mut self, i: Idle) -> Entry {
        self.idle = i;
        self
    }

    /// 渲染一行。
    pub fn line(&self, opts: &WhoOpts) -> Result<String, u32> {
        let mut s = String::new();
        let c0 = s.len();
        self.user.push_into(&mut s);
        pad_to(&mut s, c0 + W_USER);
        s.push(' ');
        let c1 = s.len();
        self.tty.push_into(&mut s);
        pad_to(&mut s, c1 + W_TTY);
        s.push(' ');
        let c2 = s.len();
        self.login.short_into(&mut s);
        pad_to(&mut s, c2 + W_TIME);
        if opts.idle {
            s.push(' ');
            let t = self.idle.render()?;
            let c3 = s.len();
            s.push_str(&t);
            pad_to(&mut s, c3 + W_IDLE);
        }
        if opts.mesg {
            s.push(' ');
            s.push(self.mesg.mark());
        }
        if opts.pid {
            s.push(' ');
            push_u64(&mut s, self.pid as u64);
        }
        if !self.host.is_empty() {
            s.push(' ');
            self.host.push_into(&mut s);
        }
        if s.len() > COLCAP {
            return Err(WH_ELEN);
        }
        Ok(s)
    }
}

/// 会话表。
#[derive(Clone, Copy, Debug)]
pub struct Table {
    rows: [Entry; ROWCAP],
    len: usize,
}

impl Table {
    /// 空表。
    pub const fn new() -> Table {
        Table { rows: [Entry::blank(); ROWCAP], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 登记。
    pub fn push(&mut self, e: Entry) -> Result<(), u32> {
        if self.find_idx(e.tty.as_bytes()).is_some() {
            return Err(WH_EDUP);
        }
        if self.len >= ROWCAP {
            return Err(WH_ERANGE);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(())
    }

    /// 终端下标。
    fn find_idx(&self, tty: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.tty.as_bytes() == tty {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按终端查。
    pub fn find_tty(&self, tty: &str) -> Option<Entry> {
        let d = Dev::parse(tty).ok()?;
        match self.find_idx(d.as_bytes()) {
            Some(i) => self.rows.get(i).copied(),
            None => None,
        }
    }

    /// 按用户查（第一条）。
    pub fn find_user(&self, user: &str) -> Option<Entry> {
        let n = Name::parse(user).ok()?;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.user.eq_bytes(n.as_bytes()) {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }

    /// 某用户会话数。
    pub fn count_user(&self, user: &str) -> usize {
        let n = match Name::parse(user) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        let mut n2 = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.user.eq_bytes(n.as_bytes()) {
                    n2 += 1;
                }
            }
            i += 1;
        }
        n2
    }

    /// 删除并返回。
    pub fn remove(&mut self, tty: &str) -> Result<Entry, u32> {
        let d = Dev::parse(tty)?;
        let idx = match self.find_idx(d.as_bytes()) {
            Some(v) => v,
            None => return Err(WH_ENOENT),
        };
        let got = match self.rows.get(idx).copied() {
            Some(v) => v,
            None => return Err(WH_ENOENT),
        };
        let mut i = idx;
        while i + 1 < self.len {
            let next = self.rows.get(i + 1).copied();
            if let (Some(dst), Some(v)) = (self.rows.get_mut(i), next) {
                *dst = v;
            }
            i += 1;
        }
        self.len -= 1;
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = Entry::blank();
        }
        Ok(got)
    }

    /// 按终端字节序稳定排序。
    pub fn sort(&mut self) {
        let mut i = 1usize;
        while i < self.len {
            let mut j = i;
            while j > 0 {
                let a = match self.rows.get(j - 1).copied() {
                    Some(v) => v,
                    None => break,
                };
                let b = match self.rows.get(j).copied() {
                    Some(v) => v,
                    None => break,
                };
                if a.tty.as_bytes() <= b.tty.as_bytes() {
                    break;
                }
                self.rows[j - 1] = b;
                self.rows[j] = a;
                j -= 1;
            }
            i += 1;
        }
    }

    /// 排序后的第 i 个终端。
    pub fn order_at(&self, i: usize) -> Option<Dev> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).map(|e| e.tty)
    }

    /// 全部终端名（空格分隔）。
    pub fn ttys(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                e.tty.push_into(&mut s);
            }
            i += 1;
        }
        s
    }

    /// `who -q` 的快速清单。
    pub fn quick(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                e.user.push_into(&mut s);
            }
            i += 1;
        }
        s.push_str("\n# ");
        push_u64(&mut s, self.len as u64);
        s.push('\n');
        s
    }

    /// 整表渲染。
    pub fn render(&self, opts: &WhoOpts, own: Option<&Dev>, boot: Option<Stamp>) -> Result<String, u32> {
        if opts.quick {
            return Ok(self.quick());
        }
        let mut picked = [Entry::blank(); ROWCAP];
        let mut n = 0usize;
        if opts.am_i {
            let d = match own {
                Some(v) => v,
                None => return Err(WH_EINVAL),
            };
            let idx = match self.find_idx(d.as_bytes()) {
                Some(v) => v,
                None => return Err(WH_ENOENT),
            };
            if let Some(e) = self.rows.get(idx).copied() {
                picked[n] = e;
                n += 1;
            }
        } else {
            let mut i = 0usize;
            while i < self.len {
                if let Some(e) = self.rows.get(i).copied() {
                    picked[n] = e;
                    n += 1;
                }
                i += 1;
            }
        }
        let mut s = String::new();
        if opts.heading {
            s.push_str(&opts.heading());
            s.push('\n');
        }
        if opts.boot {
            let b = match boot {
                Some(v) => v,
                None => return Err(WH_EINVAL),
            };
            s.push_str(&boot_line(&b));
            s.push('\n');
        }
        let mut k = 0usize;
        while k < n {
            if let Some(e) = picked.get(k) {
                let line = e.line(opts)?;
                s.push_str(&line);
                s.push('\n');
            }
            k += 1;
        }
        Ok(s)
    }
}

/// `who -b` 的系统启动行（前缀固定，便于与表头对齐）。
pub fn boot_line(stamp: &Stamp) -> String {
    let mut s = String::from("         system boot  ");
    stamp.long_into(&mut s);
    s
}

/// `who` 的开关组合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhoOpts {
    /// 表头。
    pub heading: bool,
    /// 空闲列。
    pub idle: bool,
    /// 快速清单。
    pub quick: bool,
    /// 只看自己。
    pub am_i: bool,
    /// 启动行。
    pub boot: bool,
    /// 消息状态列。
    pub mesg: bool,
    /// PID 列。
    pub pid: bool,
}

impl WhoOpts {
    /// 缺省（无开关）。
    pub const fn plain() -> WhoOpts {
        WhoOpts {
            heading: false,
            idle: false,
            quick: false,
            am_i: false,
            boot: false,
            mesg: false,
            pid: false,
        }
    }

    /// 解析开关串，支持 `-H -u -T` 与合并写 `-HuT`。
    pub fn parse(flags: &str) -> Result<WhoOpts, u32> {
        let mut o = WhoOpts::plain();
        let b = flags.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            let c = match b.get(i) {
                Some(v) => *v,
                None => break,
            };
            if c == b' ' || c == b'\t' {
                i += 1;
                continue;
            }
            if c != b'-' {
                return Err(WH_EINVAL);
            }
            i += 1;
            let mut got = false;
            while i < b.len() {
                let d = match b.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                if d == b' ' || d == b'\t' {
                    break;
                }
                match d {
                    b'H' => o.heading = true,
                    b'u' => o.idle = true,
                    b'q' => o.quick = true,
                    b'm' => o.am_i = true,
                    b'b' => o.boot = true,
                    b'T' => o.mesg = true,
                    b'p' => o.pid = true,
                    _ => return Err(WH_EINVAL),
                }
                got = true;
                i += 1;
            }
            if !got {
                return Err(WH_EINVAL);
            }
        }
        if o.quick && (o.am_i || o.heading || o.boot || o.idle || o.mesg || o.pid) {
            return Err(WH_ESTATE);
        }
        Ok(o)
    }

    /// 表头行。
    pub fn heading(&self) -> String {
        let mut s = String::new();
        push_pad(&mut s, b"NAME", W_USER);
        s.push(' ');
        push_pad(&mut s, b"LINE", W_TTY);
        s.push(' ');
        push_pad(&mut s, b"TIME", W_TIME);
        if self.idle {
            s.push(' ');
            push_pad(&mut s, b"IDLE", W_IDLE);
        }
        if self.mesg {
            s.push(' ');
            s.push('M');
        }
        if self.pid {
            s.push(' ');
            s.push_str("PID");
        }
        s.push_str(" COMMENT");
        s
    }

    /// 渲染整表。
    pub fn build(&self, t: &Table, own: Option<&Dev>, boot: Option<Stamp>) -> Result<String, u32> {
        t.render(self, own, boot)
    }
}

/// `tty` 输出（带 `/dev/` 前缀与换行）。
pub fn tty_cmd(dev: Option<&Dev>) -> Result<String, u32> {
    match dev {
        Some(d) => {
            let mut s = d.path();
            s.push('\n');
            Ok(s)
        }
        None => Err(WH_ENOENT),
    }
}

/// `tty -s` 的退出状态：0 是终端，1 不是。
pub fn tty_exit(dev: Option<&Dev>) -> i32 {
    if dev.is_some() {
        0
    } else {
        1
    }
}

/// `tty` 命令：静默档只给状态（返回空串）。
pub fn tty_run(dev: Option<&Dev>, silent: bool) -> Result<String, u32> {
    if dev.is_none() {
        return Err(WH_ENOENT);
    }
    if silent {
        return Ok(String::new());
    }
    tty_cmd(dev)
}

/// 是否是终端（stdin 判定的纯函数形态）。
pub fn is_tty(dev: Option<&Dev>) -> bool {
    match dev {
        Some(d) => !d.is_empty(),
        None => false,
    }
}

/// 三行样表。
fn build_table() -> Table {
    let mut t = Table::new();
    let a = Entry::new(
        "alice",
        "pts/2",
        "host-a",
        Stamp::from_secs(1_790_000_000),
        Idle::from_secs(120),
        4001,
    );
    if let Ok(e) = a {
        let _ = t.push(e.with_mesg(Mesg::On));
    }
    let b = Entry::new(
        "bob",
        "tty1",
        "",
        Stamp::from_secs(1_790_000_400),
        Idle::from_secs(0),
        4002,
    );
    if let Ok(e) = b {
        let _ = t.push(e.with_mesg(Mesg::Off));
    }
    let c = Entry::new(
        "carol",
        "ttyS0",
        "host-b",
        Stamp::from_secs(1_790_000_800),
        Idle::unknown(),
        4003,
    );
    if let Ok(e) = c {
        let _ = t.push(e.with_mesg(Mesg::Unknown));
    }
    t
}

/// 账本样本。
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
            return Err(WH_ESLOT);
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

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(WH_EBAL);
        }
        const VALS: [u32; 8] = [30, 10, 2026, 5, 1, 11, 3, 5];
        const CODES: [u32; 6] =
            [WH_EDUP, WH_ERANGE, WH_ETIME, WH_ENOENT, WH_EBYTE, WH_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(WH_EBAL),
            };
            if s.slot as usize != i {
                return Err(WH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(WH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(WH_EVALUE);
                        }
                    }
                    None => return Err(WH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(WH_EVALUE);
                        }
                    }
                    None => return Err(WH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(WH_EBAL);
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
            return Err(WH_EBAL);
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
            Err(WH_ESEAL)
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

/// 终端名 → `tty` 输出长度。
fn tty_len(name: &str) -> Result<u32, u32> {
    let d = Dev::parse(name)?;
    tty_cmd(Some(&d)).map(|s| s.len() as u32)
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let stamp = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0);

    let _ = l.register(word_cell(0, OP_STAMP, stamp.map(|s| s.parts().day)));
    let _ = l.register(word_cell(1, OP_STAMP, stamp.map(|s| s.parts().month)));
    let _ = l.register(word_cell(2, OP_STAMP, stamp.map(|s| s.parts().year as u32)));
    let _ = l.register(word_cell(3, OP_RENDER, Idle::from_secs(3725).render().map(|s| s.len() as u32)));
    let _ = l.register(word_cell(4, OP_RENDER, Idle::from_secs(0).render().map(|s| s.len() as u32)));
    let _ = l.register(word_cell(5, OP_TTY, tty_len("pts/0")));

    let table = build_table();
    let _ = l.register(count_cell(6, OP_COUNT, Ok(table.len())));

    let mut sorted = build_table();
    sorted.sort();
    let first = sorted.order_at(0).map(|d| d.len() as u32);
    let _ = l.register(count_cell(7, OP_SORT, match first {
        Some(v) => Ok(v as usize),
        None => Err(WH_ENOENT),
    }));

    let mut dup = build_table();
    let again = Entry::new(
        "dave",
        "pts/2",
        "",
        Stamp::from_secs(1_790_001_000),
        Idle::from_secs(1),
        4004,
    );
    let _ = l.register(match again {
        Ok(e) => word_cell(8, OP_ADD, dup.push(e).map(|_| 0)),
        Err(c) => word_cell(8, OP_ADD, Err(c)),
    });

    let _ = l.register(word_cell(9, OP_STAMP, Stamp::from_ymdhms(2026, 13, 1, 0, 0, 0).map(|s| s.parts().day)));
    let _ = l.register(word_cell(10, OP_RENDER, Idle::from_secs(-5).render().map(|s| s.len() as u32)));
    let _ = l.register(word_cell(11, OP_TTY, tty_cmd(None).map(|s| s.len() as u32)));
    let _ = l.register(word_cell(12, OP_PARSE, Name::parse("bad\u{1}name").map(|n| n.len() as u32)));
    let _ = l.register(word_cell(13, OP_RENDER, WhoOpts::parse("-q -m").map(|_| 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：名字归一化与历法。
    let d = match Dev::parse("/dev/pts/7") {
        Ok(v) => v,
        Err(_) => Dev::empty(),
    };
    let s = match Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0) {
        Ok(v) => v,
        Err(_) => Stamp::from_secs(0),
    };
    let p = s.parts();
    out[1] = d.path() == String::from("/dev/pts/7")
        && d.eq_bytes(b"pts/7")
        && d.kind() == Kind::Pts
        && p.year == 2026
        && p.month == 10
        && p.day == 30
        && s.weekday() == 5
        && s.long() == String::from("2026-10-30 09:05")
        && s.short() == String::from("Oct 30 09:05");

    // 二路：会话表的登记、去重、删除与排序。
    let mut t = build_table();
    let a = t.len() == 3 && t.count_user("alice") == 1 && t.count_user("nobody") == 0;
    t.sort();
    let b = t.order_at(0).map(|x| x.eq_bytes(b"pts/2")) == Some(true)
        && t.order_at(1).map(|x| x.eq_bytes(b"tty1")) == Some(true)
        && t.order_at(2).map(|x| x.eq_bytes(b"ttyS0")) == Some(true);
    let c = t.find_tty("tty1").map(|e| e.user.eq_bytes(b"bob")) == Some(true);
    let rem = t.remove("tty1").is_ok() && t.len() == 2 && t.remove("tty1") == Err(WH_ENOENT);
    let dup = Entry::new(
        "dave",
        "pts/2",
        "",
        Stamp::from_secs(1_790_001_000),
        Idle::from_secs(1),
        4004,
    );
    let d2 = match dup {
        Ok(e) => t.push(e) == Err(WH_EDUP),
        Err(_) => false,
    };
    out[2] = a && b && c && rem && d2;

    // 三路：who 的开关、渲染与冻结重放。
    let t2 = build_table();
    let own = Dev::parse("pts/2").ok();
    let boots = WhoOpts::parse("-H -u -T -p").ok();
    let line_ok = match boots {
        Some(o) => o.build(&t2, own.as_ref(), None).map(|s| s.starts_with("NAME     ")) == Ok(true),
        None => false,
    };
    let quick_ok = t2.quick() == String::from("alice bob carol\n# 3\n");
    let am_i_ok = match WhoOpts::parse("-m") {
        Ok(o) => {
            o.build(&t2, own.as_ref(), None).is_ok()
                && o.build(&t2, None, None) == Err(WH_EINVAL)
        }
        Err(_) => false,
    };
    let boot_ok = match WhoOpts::parse("-b") {
        Ok(o) => {
            o.build(&t2, None, Some(Stamp::from_secs(0))).map(|s| {
                s.starts_with("         system boot  ")
            }) == Ok(true)
        }
        Err(_) => false,
    };
    let mut l2 = build_ledger();
    let one = l2.freeze();
    let rep2 = l2.replay();
    let two = l2.freeze();
    let reg = l2.register(Sample::blank());
    out[3] = line_ok
        && quick_ok
        && am_i_ok
        && boot_ok
        && one.is_ok()
        && rep2 == one
        && two.is_err()
        && reg == Err(WH_ESLOT)
        && l2.is_frozen()
        && l2.rejects() == 2
        && l2.root() == one.unwrap_or(0);

    out
}

/// 十三项判据。
pub fn run_unx11551_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11551.who");

    s.add(
        "WH01-终端名归一化",
        {
            let a = Dev::parse("/dev/pts/3").map(|d| d.path()) == Ok(String::from("/dev/pts/3"));
            let b = Dev::parse("tty1").map(|d| d.path()) == Ok(String::from("/dev/tty1"));
            let c = Dev::parse("console").is_ok();
            let d = Dev::parse("/dev/pts/3").ok().map(|x| x.eq_bytes(b"pts/3")) == Some(true);
            let e = Dev::parse("pts/0").map(|d| d.as_bytes() == b"pts/0") == Ok(true);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "WH02-终端名与用户名字节闸门",
        {
            Dev::parse("") == Err(WH_EINVAL)
                && Dev::parse("/dev/") == Err(WH_EINVAL)
                && Dev::parse("/tmp/x") == Err(WH_EINVAL)
                && Dev::parse("pts//0") == Err(WH_EINVAL)
                && Dev::parse("pts/") == Err(WH_EINVAL)
                && Dev::parse("a b") == Err(WH_EBYTE)
                && Dev::parse("pts/\u{1}") == Err(WH_EBYTE)
                && Name::parse("") == Err(WH_EINVAL)
                && Name::parse("abcdefghijklmnopqrstuvwxyz0123456789") == Err(WH_ELEN)
                && Name::parse("a:b") == Err(WH_EBYTE)
                && Name::parse("a b") == Err(WH_EBYTE)
                && Name::parse("alice").map(|n| n.len()) == Ok(5)
        },
        "static",
    );

    s.add(
        "WH03-登录时刻历法",
        {
            let a = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0)
                .map(|s| (s.parts().year, s.parts().month, s.parts().day))
                == Ok((2026, 10, 30));
            let b = Stamp::from_secs(0).weekday() == 4;
            let c = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0).map(|s| s.weekday()) == Ok(5);
            let d = Stamp::from_ymdhms(2028, 2, 29, 0, 0, 0).is_ok()
                && Stamp::from_ymdhms(2027, 2, 29, 0, 0, 0) == Err(WH_ERANGE)
                && Stamp::from_ymdhms(2100, 2, 29, 0, 0, 0) == Err(WH_ERANGE)
                && Stamp::from_ymdhms(2000, 2, 29, 0, 0, 0).is_ok();
            let e = Stamp::from_ymdhms(2026, 0, 1, 0, 0, 0) == Err(WH_ERANGE)
                && Stamp::from_ymdhms(2026, 11, 31, 0, 0, 0) == Err(WH_ERANGE)
                && Stamp::from_ymdhms(2026, 10, 1, 24, 0, 0) == Err(WH_ERANGE)
                && Stamp::from_ymdhms(1969, 12, 31, 0, 0, 0) == Err(WH_ERANGE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "WH04-登录时刻格式",
        {
            let a = Stamp::from_secs(0).long() == String::from("1970-01-01 00:00");
            let b = Stamp::from_ymdhms(2026, 10, 5, 9, 5, 0).map(|s| s.short())
                == Ok(String::from("Oct 05 09:05"));
            let c = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0).map(|s| s.long().len()) == Ok(16);
            let d = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0).map(|s| s.short().len()) == Ok(12);
            let e = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 7).map(|s| s.parts().second) == Ok(7);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "WH05-空闲时长分档",
        {
            Idle::from_secs(0).render() == Ok(String::from("."))
                && Idle::from_secs(59).render() == Ok(String::from("."))
                && Idle::from_secs(60).render() == Ok(String::from("00:01"))
                && Idle::from_secs(3725).render() == Ok(String::from("01:02"))
                && Idle::from_secs(86_399).render() == Ok(String::from("23:59"))
                && Idle::from_secs(86_400).render() == Ok(String::from("old"))
                && Idle::unknown().render() == Ok(String::from("?"))
                && Idle::from_secs(-1).render() == Err(WH_ETIME)
                && Idle::from_secs(0).is_fresh()
                && Idle::from_secs(90_000).is_old()
        },
        "static",
    );

    s.add(
        "WH06-会话登记与去重",
        {
            let mut t = Table::new();
            let a = t.is_empty() && t.len() == 0;
            let mut ok = true;
            let mut i = 0usize;
            while i < 3 {
                let e = Entry::new(
                    ["alice", "bob", "carol"].get(i).copied().unwrap_or("x"),
                    ["pts/1", "pts/2", "pts/3"].get(i).copied().unwrap_or("x"),
                    "",
                    Stamp::from_secs(1_790_000_000),
                    Idle::from_secs(i as i64),
                    4100 + i as u32,
                );
                if let Ok(v) = e {
                    if t.push(v).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            let b = ok && t.len() == 3;
            let dup = Entry::new(
                "dave",
                "pts/2",
                "",
                Stamp::from_secs(1_790_000_000),
                Idle::from_secs(1),
                4200,
            );
            let c = match dup {
                Ok(e) => t.push(e) == Err(WH_EDUP),
                Err(_) => false,
            };
            let d = t.count_user("alice") == 1 && t.count_user("zz") == 0;
            let e = t.at(0).map(|x| x.user.eq_bytes(b"alice")) == Some(true);
            let f = Entry::new("alice", "pts/9", "", Stamp::from_secs(0), Idle::unknown(), 0)
                == Err(WH_ERANGE);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "WH07-会话删除与查找",
        {
            let mut t = build_table();
            let a = t.find_tty("tty1").map(|e| e.user.eq_bytes(b"bob")) == Some(true);
            let b = t.find_user("carol").map(|e| e.tty.eq_bytes(b"ttyS0")) == Some(true);
            let c = t.find_tty("pts/9").is_none() && t.find_user("nobody").is_none();
            let d = t.remove("ttyS0").is_ok() && t.len() == 2 && t.find_tty("ttyS0").is_none();
            let e = t.remove("ttyS0") == Err(WH_ENOENT) && t.remove("bad name") == Err(WH_EBYTE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "WH08-会话表字节序排序",
        {
            let mut t = Table::new();
            let names = ["ttyS0", "pts/2", "tty1"];
            let users = ["carol", "alice", "bob"];
            let mut i = 0usize;
            while i < 3 {
                let e = Entry::new(
                    users.get(i).copied().unwrap_or("x"),
                    names.get(i).copied().unwrap_or("x"),
                    "",
                    Stamp::from_secs(1_790_000_000 + i as i64),
                    Idle::unknown(),
                    4300 + i as u32,
                );
                if let Ok(v) = e {
                    let _ = t.push(v);
                }
                i += 1;
            }
            let a = t.order_at(0).map(|d| d.eq_bytes(b"ttyS0")) == Some(true);
            t.sort();
            let b = t.order_at(0).map(|d| d.eq_bytes(b"pts/2")) == Some(true)
                && t.order_at(1).map(|d| d.eq_bytes(b"tty1")) == Some(true)
                && t.order_at(2).map(|d| d.eq_bytes(b"ttyS0")) == Some(true);
            let c = t.ttys() == String::from("pts/2 tty1 ttyS0");
            let d = t.order_at(3).is_none();
            a && b && c && d
        },
        "static",
    );

    s.add(
        "WH09-who 行渲染",
        {
            let t = build_table();
            let o = WhoOpts::parse("-H").ok();
            let a = t.at(0).map(|e| e.line(&WhoOpts::plain()).map(|s| s.len())) == Some(Ok(41));
            let b = match o {
                Some(v) => v.build(&t, None, None).map(|s| s.starts_with("NAME     ")) == Ok(true),
                None => false,
            };
            let plain = WhoOpts::plain();
            let full = WhoOpts::parse("-u -T -p").ok();
            let c = match full {
                Some(v) => {
                    let l0 = t.at(0).map(|e| e.line(&plain).map(|s| s.len()));
                    let l1 = t.at(0).map(|e| e.line(&v).map(|s| s.len()));
                    matches!((l0, l1), (Some(Ok(x)), Some(Ok(y))) if y > x)
                }
                None => false,
            };
            let d = match WhoOpts::parse("-uTp") {
                Ok(v) => v.idle && v.mesg && v.pid,
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "WH10-who 的快速清单/只看自己/启动行",
        {
            let t = build_table();
            let a = t.quick() == String::from("alice bob carol\n# 3\n");
            let own = Dev::parse("pts/2").ok();
            let b = match WhoOpts::parse("-m") {
                Ok(o) => {
                    o.build(&t, own.as_ref(), None).map(|s| s.lines().count()) == Ok(1)
                        && o.build(&t, None, None) == Err(WH_EINVAL)
                }
                Err(_) => false,
            };
            let c = match WhoOpts::parse("-b") {
                Ok(o) => o
                    .build(&t, None, Some(Stamp::from_secs(0)))
                    .map(|s| s.starts_with("         system boot  1970-01-01 00:00"))
                    == Ok(true),
                Err(_) => false,
            };
            let d = WhoOpts::parse("-q -H") == Err(WH_ESTATE)
                && WhoOpts::parse("-q -m") == Err(WH_ESTATE)
                && WhoOpts::parse("--long") == Err(WH_EINVAL)
                && WhoOpts::parse("-Z") == Err(WH_EINVAL)
                && WhoOpts::parse("-") == Err(WH_EINVAL);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "WH11-tty 命令",
        {
            let d = Dev::parse("pts/0").ok();
            let a = tty_cmd(d.as_ref()) == Ok(String::from("/dev/pts/0\n"));
            let b = tty_cmd(None) == Err(WH_ENOENT);
            let c = tty_exit(d.as_ref()) == 0 && tty_exit(None) == 1;
            let e = tty_run(d.as_ref(), true) == Ok(String::new())
                && tty_run(None, true) == Err(WH_ENOENT);
            let f = is_tty(d.as_ref()) && !is_tty(None);
            a && b && c && e && f
        },
        "static",
    );

    s.add(
        "WH12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(30)
                && l.sample(1).map(|x| x.value) == Some(10)
                && l.sample(2).map(|x| x.value) == Some(2026)
                && l.sample(13).map(|x| x.code) == Some(WH_ESTATE)
                && l.sample(14).is_none()
        },
        "static",
    );

    s.add(
        "WH13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_vectors() {
        let s = Stamp::from_ymdhms(2026, 10, 30, 9, 5, 0).ok();
        assert!(s.is_some());
        if let Some(v) = s {
            assert_eq!(v.parts().year, 2026);
            assert_eq!(v.parts().month, 10);
            assert_eq!(v.parts().day, 30);
            assert_eq!(v.parts().hour, 9);
            assert_eq!(v.parts().minute, 5);
            assert_eq!(v.parts().second, 0);
            assert_eq!(v.weekday(), 5);
            assert_eq!(v.weekday_label(), "Fri");
            assert_eq!(v.long(), String::from("2026-10-30 09:05"));
            assert_eq!(v.short(), String::from("Oct 30 09:05"));
        }
        assert_eq!(Stamp::from_secs(0).weekday(), 4);
        assert_eq!(Stamp::from_secs(0).weekday_label(), "Thu");
    }

    #[test]
    fn calendar_roundtrip() {
        let mut y = 1970i64;
        while y <= 2100 {
            let mut m = 1u32;
            while m <= 12 {
                let dmax = month_days(y, m);
                let mut d = 1u32;
                while d <= dmax {
                    let days = days_from_civil(y, m, d);
                    let (yy, mm, dd) = civil_from_days(days);
                    assert_eq!((yy, mm, dd), (y, m, d));
                    d += 1;
                }
                m += 1;
            }
            y += 1;
        }
        assert!(is_leap(2000) && !is_leap(2100) && is_leap(2024));
        assert_eq!(month_days(2027, 2), 28);
        assert_eq!(month_days(2028, 2), 29);
    }

    #[test]
    fn idle_buckets() {
        assert_eq!(Idle::from_secs(-1).render(), Err(WH_ETIME));
        assert_eq!(Idle::unknown().render(), Ok(String::from("?")));
        assert_eq!(Idle::from_secs(0).render(), Ok(String::from(".")));
        assert_eq!(Idle::from_secs(59).render(), Ok(String::from(".")));
        assert_eq!(Idle::from_secs(60).render(), Ok(String::from("00:01")));
        assert_eq!(Idle::from_secs(3599).render(), Ok(String::from("00:59")));
        assert_eq!(Idle::from_secs(3600).render(), Ok(String::from("01:00")));
        assert_eq!(Idle::from_secs(86_399).render(), Ok(String::from("23:59")));
        assert_eq!(Idle::from_secs(86_400).render(), Ok(String::from("old")));
    }

    #[test]
    fn dev_parse_and_kind() {
        assert_eq!(Dev::parse("/dev/pts/3").map(|d| d.text()), Ok(String::from("pts/3")));
        assert_eq!(Dev::parse("tty1").map(|d| d.path()), Ok(String::from("/dev/tty1")));
        assert_eq!(Dev::parse("").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("/dev/").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("/tmp/x").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("pts//0").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("pts/").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("/pts/0").err(), Some(WH_EINVAL));
        assert_eq!(Dev::parse("a b").err(), Some(WH_EBYTE));
        assert_eq!(Dev::parse("pts/\u{7f}").err(), Some(WH_EBYTE));
        assert_eq!(
            Dev::parse("abcdefghijklmnopqrstuvwxyz").err(),
            Some(WH_ELEN)
        );
        let c = Dev::parse("console").map(|d| d.kind());
        assert_eq!(c, Ok(Kind::Console));
        let p = Dev::parse("pts/12").map(|d| d.kind());
        assert_eq!(p, Ok(Kind::Pts));
        let v = Dev::parse("tty3").map(|d| d.kind());
        assert_eq!(v, Ok(Kind::Vc));
        let s = Dev::parse("ttyS0").map(|d| d.kind());
        assert_eq!(s, Ok(Kind::Serial));
        let u = Dev::parse("ttyUSB0").map(|d| d.kind());
        assert_eq!(u, Ok(Kind::Unknown));
    }

    #[test]
    fn table_ops() {
        let mut t = build_table();
        assert_eq!(t.len(), 3);
        assert_eq!(t.count_user("bob"), 1);
        let dup = Entry::new(
            "dave",
            "pts/2",
            "",
            Stamp::from_secs(1_790_001_000),
            Idle::from_secs(1),
            4004,
        );
        match dup {
            Ok(e) => assert_eq!(t.push(e), Err(WH_EDUP)),
            Err(_) => assert!(false),
        }
        t.sort();
        assert_eq!(t.order_at(0).map(|d| d.text()), Some(String::from("pts/2")));
        assert_eq!(t.order_at(1).map(|d| d.text()), Some(String::from("tty1")));
        assert_eq!(t.order_at(2).map(|d| d.text()), Some(String::from("ttyS0")));
        assert!(t.remove("tty1").is_ok());
        assert_eq!(t.len(), 2);
        assert_eq!(t.remove("tty1"), Err(WH_ENOENT));
        assert!(t.find_user("bob").is_none());
    }

    #[test]
    fn table_capacity() {
        let mut t = Table::new();
        let names = [
            "pts/0", "pts/1", "pts/2", "pts/3", "pts/4", "pts/5", "pts/6", "pts/7", "pts/8",
            "pts/9", "pts/10", "pts/11", "pts/12", "pts/13", "pts/14", "pts/15",
        ];
        let mut i = 0usize;
        while i < names.len() {
            let e = Entry::new(
                "u",
                names.get(i).copied().unwrap_or("x"),
                "",
                Stamp::from_secs(1_790_000_000),
                Idle::unknown(),
                5000 + i as u32,
            );
            if let Ok(v) = e {
                assert_eq!(t.push(v), Ok(()));
            }
            i += 1;
        }
        assert_eq!(t.len(), ROWCAP);
        let extra = Entry::new("u", "pts/16", "", Stamp::from_secs(0), Idle::unknown(), 6000);
        match extra {
            Ok(e) => assert_eq!(t.push(e), Err(WH_ERANGE)),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn who_render_and_opts() {
        let t = build_table();
        let o = WhoOpts::parse("-H -u -T -p");
        assert!(o.is_ok());
        if let Ok(v) = o {
            assert!(v.heading && v.idle && v.mesg && v.pid && !v.quick && !v.am_i);
            let out = v.build(&t, None, None);
            assert!(out.is_ok());
            if let Ok(text) = out {
                let mut lines = text.lines();
                let head = v.heading();
                assert_eq!(lines.next(), Some(head.as_str()));
                assert_eq!(lines.count(), 3);
            }
        }
        assert_eq!(WhoOpts::parse("-q -m"), Err(WH_ESTATE));
        assert_eq!(WhoOpts::parse("-Z"), Err(WH_EINVAL));
        assert_eq!(WhoOpts::parse("---"), Err(WH_EINVAL));
        assert_eq!(t.quick(), String::from("alice bob carol\n# 3\n"));
    }

    #[test]
    fn tty_paths_and_ledger() {
        let d = Dev::parse("pts/0").ok();
        assert_eq!(tty_cmd(d.as_ref()), Ok(String::from("/dev/pts/0\n")));
        assert_eq!(tty_cmd(None), Err(WH_ENOENT));
        assert_eq!(tty_exit(d.as_ref()), 0);
        assert_eq!(tty_exit(None), 1);
        assert_eq!(tty_run(d.as_ref(), true), Ok(String::new()));
        assert!(is_tty(d.as_ref()));
        assert!(!is_tty(None));

        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        let root = l.seal();
        assert_ne!(root, 0);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Ok(root));
        assert!(l.is_frozen());
        assert_eq!(l.freeze(), Err(WH_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(WH_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.root(), root);
    }
}
