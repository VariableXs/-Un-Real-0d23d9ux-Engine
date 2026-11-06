//! UNX-F11573 date 时间格式 C locale 固定判据
//!
//! `date` 的输出在 C locale 下不是「随区域变化」的，而是一条**固定判据**：
//! 星期只有 `Sun..Sat` / `Sunday..Saturday` 两套英文名，月份只有
//! `Jan..Dec` / `January..December` 两套英文名，上下午只有 `AM` / `PM`，
//! 时区缩写恒为 `UTC`，其余区域名一律在闸门处被拒。本模块把这条判据做成
//! 可逐字节复算的时间模型：
//!
//! * 历法层：以 1970-01-01T00:00:00Z 为原点，用 Howard Hinnant 的
//!   `days_from_civil` / `civil_from_days` 做**纯整数**往返，闰年按
//!   「4 年一闰、100 年不闰、400 年再闰」判定，绝不借助浮点或时区库。
//! * 派生层：由纪元秒推出星期（原点为周四，故 `(days + 4) mod 7`）、
//!   年内序日（0 基，与 C 的 `tm_yday` 同义）以及 ISO 8601 的
//!   「周所属年 / 周号 / 周内日」三元组（周四锚定法）。
//! * 格式层：把格式串编译成扁平的 [`Step`] 序列（字面字节 + 字段码 +
//!   宽度 + 填充符），再按序列渲染；不支持任何隐式本地化——`%Z` 固定
//!   输出 `UTC`，`%a/%A/%b/%B/%p` 只查本模块的英文表。
//! * 参数层：解析 `date [-u] [+FORMAT] [@EPOCH]`，重复格式串、未知开关、
//!   无法解析的纪元一律落错误码，不允许「猜一个默认值」蒙混过关。
//!
//! 闸门次序：字节 → 指令 → 宽度 → 字段 → 范围 → 名表 → 区域 → 纪元 → 渲染。
//!
//! 全模块无 std、无 unsafe、无 panic：越界一律 `get()`，算术一律
//! `checked_*` / `saturating_*`，失败全部落 13 个错误码；账本 14 格
//! （8 成 6 败）在 [`Ledger::audit`] 逐格对账，冻结指纹与重放必须一致。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf900;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 格式编译步数上限。
pub const STEP_CAP: usize = 48;

/// 格式串字节上限。
pub const FMT_CAP: usize = 64;

/// 一日的秒数。
pub const DAY: i64 = 86_400;

/// 纪元下界（0001-01-01T00:00:00Z）。
pub const EPOCH_MIN: i64 = -62_135_596_800;

/// 纪元上界（9999-12-31T23:59:59Z）。
pub const EPOCH_MAX: i64 = 253_402_300_799;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 格式串含非可打印字节。
pub const DT_EBYTE: u32 = SEG_BASE + 0x01;
/// 格式串结构错误（悬空 `%`、超长、宽度溢出）。
pub const DT_EFMT: u32 = SEG_BASE + 0x02;
/// 未知转换字段。
pub const DT_EFIELD: u32 = SEG_BASE + 0x03;
/// 日期字段越界。
pub const DT_ERANGE: u32 = SEG_BASE + 0x04;
/// 区域名不被 C 锚接受。
pub const DT_ELOCALE: u32 = SEG_BASE + 0x05;
/// 英文名表查不到（表损坏）。
pub const DT_ENAME: u32 = SEG_BASE + 0x06;
/// 状态冲突（重复格式串、重复 `-u`）。
pub const DT_ESTATE: u32 = SEG_BASE + 0x07;
/// 参数无法解析。
pub const DT_EARG: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const DT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const DT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const DT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const DT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const DT_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 历法换算。
pub const OP_CIVIL: u8 = 0x01;
/// 纪元求值。
pub const OP_EPOCH: u8 = 0x02;
/// 星期/序日。
pub const OP_WDAY: u8 = 0x03;
/// ISO 周。
pub const OP_ISOWK: u8 = 0x04;
/// 格式编译。
pub const OP_FMT: u8 = 0x05;
/// 区域名。
pub const OP_LOCALE: u8 = 0x06;
/// 参数解析。
pub const OP_ARGS: u8 = 0x07;
/// 渲染。
pub const OP_RUN: u8 = 0x08;

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

/// 推入无符号十进制。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
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
            out.push(*b as char);
        }
        i += 1;
    }
}

/// 推入有符号十进制（本模块取值域远离 i64 端点，饱和保护足矣）。
fn push_i64(out: &mut String, v: i64) {
    if v < 0 {
        out.push('-');
        let m = v.saturating_neg();
        push_u64(out, m as u64);
    } else {
        push_u64(out, v as u64);
    }
}

// ---------------------------------------------------------------------------
// 历法
// ---------------------------------------------------------------------------

/// 闰年：4 年一闰、100 年不闰、400 年再闰。
pub fn is_leap(y: i32) -> bool {
    if y % 4 != 0 {
        return false;
    }
    if y % 100 != 0 {
        return true;
    }
    y % 400 == 0
}

/// 某年某月的天数；月份越界返回 0。
pub fn days_in_month(y: i32, m: u8) -> u8 {
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

/// 公历 → 距 1970-01-01 的天数（可为负）。
pub fn days_from_civil(y: i32, m: u8, d: u8) -> Result<i64, u32> {
    if m < 1 || m > 12 {
        return Err(DT_ERANGE);
    }
    let dim = days_in_month(y, m);
    if d < 1 || d > dim {
        return Err(DT_ERANGE);
    }
    let yy = y as i64;
    let y2 = if m <= 2 { yy - 1 } else { yy };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok(era * 146_097 + doe - 719_468)
}

/// 距 1970-01-01 的天数 → 公历三元组。
pub fn civil_from_days(z: i64) -> Result<(i32, u8, u8), u32> {
    let zz = z.checked_add(719_468).ok_or(DT_ERANGE)?;
    let era = if zz >= 0 { zz } else { zz - 146_096 } / 146_097;
    let doe = zz - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    if year < -9999 || year > 9999 {
        return Err(DT_ERANGE);
    }
    if m < 1 || m > 12 || d < 1 || d > 31 {
        return Err(DT_ERANGE);
    }
    Ok((year as i32, m as u8, d as u8))
}

/// 公历 + 时刻 → 纪元秒。
pub fn epoch_of(y: i32, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Result<i64, u32> {
    if h > 23 || mi > 59 || s > 59 {
        return Err(DT_ERANGE);
    }
    let days = days_from_civil(y, mo, d)?;
    let secs = days
        .checked_mul(DAY)
        .and_then(|v| v.checked_add(h as i64 * 3600))
        .and_then(|v| v.checked_add(mi as i64 * 60))
        .and_then(|v| v.checked_add(s as i64))
        .ok_or(DT_ERANGE)?;
    if secs < EPOCH_MIN || secs > EPOCH_MAX {
        return Err(DT_ERANGE);
    }
    Ok(secs)
}

// ---------------------------------------------------------------------------
// 时刻
// ---------------------------------------------------------------------------

/// 分解后的时刻（UTC）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// 年。
    pub year: i32,
    /// 月 1..=12。
    pub month: u8,
    /// 日 1..=31。
    pub day: u8,
    /// 时 0..=23。
    pub hour: u8,
    /// 分 0..=59。
    pub min: u8,
    /// 秒 0..=59。
    pub sec: u8,
    /// 星期 0=周日..6=周六。
    pub wday: u8,
    /// 年内序日 0 基。
    pub yday: u16,
    /// ISO 周所属年。
    pub iso_year: i32,
    /// ISO 周号 1..=53。
    pub iso_week: u8,
    /// ISO 周内日 1=周一..7=周日。
    pub iso_wday: u8,
}

impl Stamp {
    /// 空白时刻。
    pub const fn blank() -> Stamp {
        Stamp {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            min: 0,
            sec: 0,
            wday: 4,
            yday: 0,
            iso_year: 1970,
            iso_week: 1,
            iso_wday: 4,
        }
    }

    /// 是否同一时刻。
    pub fn same(&self, other: &Stamp) -> bool {
        self.year == other.year
            && self.month == other.month
            && self.day == other.day
            && self.hour == other.hour
            && self.min == other.min
            && self.sec == other.sec
    }
}

/// 纪元秒 → 时刻（UTC，含 ISO 周三元组）。
pub fn stamp_of_epoch(epoch: i64) -> Result<Stamp, u32> {
    if epoch < EPOCH_MIN || epoch > EPOCH_MAX {
        return Err(DT_ERANGE);
    }
    let days = epoch.div_euclid(DAY);
    let sod = epoch.rem_euclid(DAY);
    let (y, m, d) = civil_from_days(days)?;
    let hour = (sod / 3600) as u8;
    let min = ((sod % 3600) / 60) as u8;
    let sec = (sod % 60) as u8;
    let wday = (days + 4).rem_euclid(7) as u8;
    let jan1 = days_from_civil(y, 1, 1)?;
    let yday = days.saturating_sub(jan1);
    if yday < 0 || yday > 365 {
        return Err(DT_ERANGE);
    }
    let iso_wday = if wday == 0 { 7u8 } else { wday };
    let thu = days.saturating_add(4 - iso_wday as i64);
    let (iy, _, _) = civil_from_days(thu)?;
    let iso_jan1 = days_from_civil(iy, 1, 1)?;
    let week = (thu - iso_jan1) / 7 + 1;
    if week < 1 || week > 53 {
        return Err(DT_ERANGE);
    }
    Ok(Stamp {
        year: y,
        month: m,
        day: d,
        hour,
        min,
        sec,
        wday,
        yday: yday as u16,
        iso_year: iy,
        iso_week: week as u8,
        iso_wday,
    })
}

/// 时刻 → 纪元秒。
pub fn epoch_of_stamp(st: &Stamp) -> Result<i64, u32> {
    epoch_of(st.year, st.month, st.day, st.hour, st.min, st.sec)
}

// ---------------------------------------------------------------------------
// 英文名表（C locale 唯一合法内容）
// ---------------------------------------------------------------------------

/// 星期缩写。
pub const WDAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
/// 星期全名。
pub const WDAY_FULL: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
/// 月份缩写。
pub const MON_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// 月份全名。
pub const MON_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// 名表整体是否满足 C 锚：7+7+12+12 项、非空、全 ASCII 可打印。
pub fn names_c_anchored() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < 7 {
        match (WDAY_ABBR.get(i), WDAY_FULL.get(i)) {
            (Some(a), Some(b)) => {
                if !ascii_word(a.as_bytes()) || !ascii_word(b.as_bytes()) || a.len() > b.len() {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < 12 {
        match (MON_ABBR.get(k), MON_FULL.get(k)) {
            (Some(a), Some(b)) => {
                if !ascii_word(a.as_bytes()) || !ascii_word(b.as_bytes()) || a.len() > b.len() {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        k += 1;
    }
    ok
}

/// 全 ASCII 词（0x20..=0x7e 且非空）。
fn ascii_word(b: &[u8]) -> bool {
    if b.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if *c < 0x20 || *c > 0x7e {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 取星期缩写。
pub fn wday_abbr(w: u8) -> Result<&'static str, u32> {
    match WDAY_ABBR.get(w as usize) {
        Some(s) => Ok(*s),
        None => Err(DT_ENAME),
    }
}

/// 取星期全名。
pub fn wday_full(w: u8) -> Result<&'static str, u32> {
    match WDAY_FULL.get(w as usize) {
        Some(s) => Ok(*s),
        None => Err(DT_ENAME),
    }
}

/// 取月份缩写（1 基）。
pub fn mon_abbr(m: u8) -> Result<&'static str, u32> {
    if m < 1 {
        return Err(DT_ENAME);
    }
    match MON_ABBR.get((m - 1) as usize) {
        Some(s) => Ok(*s),
        None => Err(DT_ENAME),
    }
}

/// 取月份全名（1 基）。
pub fn mon_full(m: u8) -> Result<&'static str, u32> {
    if m < 1 {
        return Err(DT_ENAME);
    }
    match MON_FULL.get((m - 1) as usize) {
        Some(s) => Ok(*s),
        None => Err(DT_ENAME),
    }
}

// ---------------------------------------------------------------------------
// 区域
// ---------------------------------------------------------------------------

/// 被识别的区域名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 纯 C。
    C,
    /// POSIX 别名。
    Posix,
    /// C.UTF-8（名义 UTF-8，判据仍按字节，等价 C）。
    CUtf8,
    /// 德语区（拒绝）。
    De,
    /// 法语区（拒绝）。
    Fr,
}

impl Locale {
    /// 是否可作 C 锚。
    pub fn is_c_anchor(&self) -> bool {
        match self {
            Locale::C | Locale::Posix | Locale::CUtf8 => true,
            Locale::De | Locale::Fr => false,
        }
    }

    /// 规范名。
    pub fn text(&self) -> &'static str {
        match self {
            Locale::C => "C",
            Locale::Posix => "POSIX",
            Locale::CUtf8 => "C.UTF-8",
            Locale::De => "de_DE.UTF-8",
            Locale::Fr => "fr_FR.UTF-8",
        }
    }

    /// 时区缩写：C 锚只有 `UTC`。
    pub fn zone_abbr(&self) -> &'static str {
        "UTC"
    }
}

/// 区域名解析：只认白名单里的五个名字。
pub fn locale_from(name: &[u8]) -> Result<Locale, u32> {
    if eq_bytes(name, b"C") {
        return Ok(Locale::C);
    }
    if eq_bytes(name, b"POSIX") {
        return Ok(Locale::Posix);
    }
    if eq_bytes(name, b"C.UTF-8") {
        return Ok(Locale::CUtf8);
    }
    if eq_bytes(name, b"de_DE.UTF-8") {
        return Ok(Locale::De);
    }
    if eq_bytes(name, b"fr_FR.UTF-8") {
        return Ok(Locale::Fr);
    }
    Err(DT_ELOCALE)
}

// ---------------------------------------------------------------------------
// 格式编译
// ---------------------------------------------------------------------------

/// 一步渲染：`code == 0` 时是字面字节，否则是字段码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// 字面字节（`code == 0` 时有效）。
    pub lit: u8,
    /// 字段码（`%` 后的那个字节），0 表示字面。
    pub code: u8,
    /// 填充码：0=字段缺省，1=不填充，`b'0'`/`b' '` 显式填充。
    pub pad: u8,
    /// 宽度，0 表示字段缺省。
    pub width: u8,
}

impl Step {
    /// 空白步。
    pub const fn blank() -> Step {
        Step { lit: 0, code: 0, pad: 0, width: 0 }
    }

    /// 是否字面。
    pub fn is_literal(&self) -> bool {
        self.code == 0
    }
}

/// 字段码是否被 C 锚支持。
fn is_field(c: u8) -> bool {
    matches!(
        c,
        b'Y' | b'y'
            | b'm'
            | b'd'
            | b'e'
            | b'H'
            | b'M'
            | b'S'
            | b'j'
            | b'a'
            | b'A'
            | b'b'
            | b'B'
            | b'p'
            | b'Z'
            | b'u'
            | b'w'
            | b'U'
            | b'W'
            | b'V'
            | b'G'
            | b'%'
            | b'n'
            | b't'
    )
}

/// 字段缺省宽度。
fn default_width(c: u8) -> u8 {
    match c {
        b'Y' | b'G' => 4,
        b'y' | b'm' | b'd' | b'e' | b'H' | b'M' | b'S' | b'U' | b'W' | b'V' => 2,
        b'j' => 3,
        _ => 0,
    }
}

/// 字段缺省填充。
fn default_pad(c: u8) -> u8 {
    match c {
        b'e' => b' ',
        _ => b'0',
    }
}

/// 编译格式串。
pub fn compile(fmt: &[u8]) -> Result<([Step; STEP_CAP], usize), u32> {
    let mut out = [Step::blank(); STEP_CAP];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        let b = match fmt.get(i) {
            Some(v) => *v,
            None => return Err(DT_EFMT),
        };
        if b < 0x20 || b > 0x7e {
            return Err(DT_EBYTE);
        }
        if b != b'%' {
            if n >= STEP_CAP {
                return Err(DT_EFMT);
            }
            if let Some(dst) = out.get_mut(n) {
                *dst = Step { lit: b, code: 0, pad: 0, width: 0 };
            }
            n += 1;
            i += 1;
            continue;
        }
        i += 1;
        let mut pad = 0u8;
        if fmt.get(i) == Some(&b'-') {
            pad = 1;
            i += 1;
        } else if fmt.get(i) == Some(&b'0') {
            pad = b'0';
            i += 1;
        }
        let mut wd = 0u32;
        let mut digits = 0usize;
        while let Some(c) = fmt.get(i) {
            if *c < b'0' || *c > b'9' {
                break;
            }
            wd = wd.saturating_mul(10).saturating_add((*c - b'0') as u32);
            digits += 1;
            i += 1;
            if wd > 31 {
                return Err(DT_EFMT);
            }
        }
        let c = match fmt.get(i) {
            Some(v) => *v,
            None => return Err(DT_EFMT),
        };
        i += 1;
        if !is_field(c) {
            return Err(DT_EFIELD);
        }
        if n >= STEP_CAP {
            return Err(DT_EFMT);
        }
        let width = if digits > 0 { wd as u8 } else { 0 };
        if let Some(dst) = out.get_mut(n) {
            *dst = Step { lit: 0, code: c, pad, width };
        }
        n += 1;
    }
    Ok((out, n))
}

/// 字段数值。
fn field_value(st: &Stamp, c: u8) -> Result<i64, u32> {
    let v = match c {
        b'Y' => st.year as i64,
        b'y' => (st.year % 100) as i64,
        b'm' => st.month as i64,
        b'd' => st.day as i64,
        b'e' => st.day as i64,
        b'H' => st.hour as i64,
        b'M' => st.min as i64,
        b'S' => st.sec as i64,
        b'j' => st.yday as i64 + 1,
        b'u' => st.iso_wday as i64,
        b'w' => st.wday as i64,
        b'U' => ((st.yday as i64 + 7 - st.wday as i64) / 7) as i64,
        b'W' => {
            let monday_wday = (st.wday as i64 + 6) % 7;
            ((st.yday as i64 + 7 - monday_wday) / 7) as i64
        }
        b'V' => st.iso_week as i64,
        b'G' => st.iso_year as i64,
        _ => return Err(DT_EFIELD),
    };
    Ok(v)
}

/// 按宽度/填充追加数值。
fn push_padded(out: &mut String, v: i64, width: u8, pad: u8) {
    let mut digits = String::new();
    push_i64(&mut digits, v);
    let n = digits.len();
    if pad != 0 && n < width as usize {
        let fill = pad as char;
        let mut k = n;
        while k < width as usize {
            out.push(fill);
            k += 1;
        }
    }
    out.push_str(digits.as_str());
}

/// 单字段渲染。
fn render_one(st: &Stamp, step: &Step, out: &mut String) -> Result<(), u32> {
    let c = step.code;
    match c {
        b'a' => out.push_str(wday_abbr(st.wday)?),
        b'A' => out.push_str(wday_full(st.wday)?),
        b'b' => out.push_str(mon_abbr(st.month)?),
        b'B' => out.push_str(mon_full(st.month)?),
        b'p' => {
            if st.hour < 12 {
                out.push_str("AM");
            } else {
                out.push_str("PM");
            }
        }
        b'Z' => out.push_str("UTC"),
        b'n' => out.push('\n'),
        b't' => out.push('\t'),
        b'%' => out.push('%'),
        _ => {
            let v = field_value(st, c)?;
            let (w, p) = if step.pad == 1 {
                (0u8, 0u8)
            } else {
                let ww = if step.width == 0 {
                    default_width(c)
                } else {
                    step.width
                };
                let pp = if step.pad != 0 {
                    step.pad
                } else if step.width != 0 {
                    b' '
                } else {
                    default_pad(c)
                };
                (ww, pp)
            };
            push_padded(out, v, w, p);
        }
    }
    Ok(())
}

/// 按已编译序列渲染。
pub fn render_steps(st: &Stamp, steps: &[Step; STEP_CAP], len: usize) -> Result<String, u32> {
    if len > STEP_CAP {
        return Err(DT_EFMT);
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < len {
        let step = match steps.get(i) {
            Some(v) => *v,
            None => return Err(DT_EFMT),
        };
        if step.is_literal() {
            out.push(step.lit as char);
        } else {
            render_one(st, &step, &mut out)?;
        }
        i += 1;
    }
    Ok(out)
}

/// 一步到位：编译 + 渲染。
pub fn render(st: &Stamp, fmt: &[u8]) -> Result<String, u32> {
    let (steps, n) = compile(fmt)?;
    render_steps(st, &steps, n)
}

/// 编译步数。
pub fn step_count(fmt: &[u8]) -> Result<usize, u32> {
    match compile(fmt) {
        Ok((_, n)) => Ok(n),
        Err(c) => Err(c),
    }
}

// ---------------------------------------------------------------------------
// 参数
// ---------------------------------------------------------------------------

/// `date` 命令行选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// `-u`：UTC。
    pub utc: bool,
    /// 区域。
    pub locale: Locale,
    /// 格式串长度。
    pub fmt_len: usize,
    /// 格式串字节。
    pub fmt: [u8; FMT_CAP],
    /// 目标纪元。
    pub epoch: i64,
}

impl Opts {
    /// 缺省：UTC、C、格式 `%a %b %e %H:%M:%S %Z %Y`、纪元 0。
    pub const fn new() -> Opts {
        Opts { utc: true, locale: Locale::C, fmt_len: 0, fmt: [0u8; FMT_CAP], epoch: 0 }
    }

    /// 取格式串。
    pub fn fmt_bytes(&self) -> &[u8] {
        self.fmt.get(0..self.fmt_len).unwrap_or(&[])
    }

    /// 设置格式串。
    pub fn set_fmt(&mut self, f: &[u8]) -> Result<(), u32> {
        if f.len() > FMT_CAP {
            return Err(DT_EFMT);
        }
        if self.fmt_len != 0 {
            return Err(DT_ESTATE);
        }
        let mut i = 0usize;
        while i < f.len() {
            match (f.get(i), self.fmt.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(DT_EFMT),
            }
            i += 1;
        }
        self.fmt_len = f.len();
        Ok(())
    }
}

/// 解析 `date [-u] [-L NAME] [+FORMAT] [@EPOCH]`。
pub fn parse_argv(args: &[&[u8]]) -> Result<Opts, u32> {
    let mut o = Opts::new();
    let mut seen_fmt = false;
    let mut seen_epoch = false;
    let mut seen_u = false;
    let mut seen_locale = false;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(DT_EARG),
        };
        if a.is_empty() {
            return Err(DT_EARG);
        }
        if eq_bytes(a, b"-u") {
            if seen_u {
                return Err(DT_ESTATE);
            }
            o.utc = true;
            seen_u = true;
        } else if a.len() > 2 && a.get(0) == Some(&b'-') && a.get(1) == Some(&b'L') {
            if seen_locale {
                return Err(DT_ESTATE);
            }
            let name = a.get(2..).unwrap_or(&[]);
            let loc = locale_from(name)?;
            if !loc.is_c_anchor() {
                return Err(DT_ELOCALE);
            }
            o.locale = loc;
            seen_locale = true;
        } else if a.get(0) == Some(&b'+') {
            if seen_fmt {
                return Err(DT_ESTATE);
            }
            let f = a.get(1..).unwrap_or(&[]);
            o.set_fmt(f)?;
            seen_fmt = true;
        } else if a.get(0) == Some(&b'@') {
            if seen_epoch {
                return Err(DT_ESTATE);
            }
            o.epoch = parse_epoch(a.get(1..).unwrap_or(&[]))?;
            seen_epoch = true;
        } else if a.get(0) == Some(&b'-') {
            return Err(DT_EARG);
        } else {
            if seen_epoch {
                return Err(DT_ESTATE);
            }
            o.epoch = parse_epoch(a)?;
            seen_epoch = true;
        }
        i += 1;
    }
    Ok(o)
}

/// 解析十进制纪元秒（可带负号）。
pub fn parse_epoch(b: &[u8]) -> Result<i64, u32> {
    if b.is_empty() {
        return Err(DT_EARG);
    }
    let mut neg = false;
    let mut i = 0usize;
    if b.get(0) == Some(&b'-') {
        neg = true;
        i = 1;
    }
    if i >= b.len() {
        return Err(DT_EARG);
    }
    let mut v: i64 = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(DT_EARG),
        };
        if c < b'0' || c > b'9' {
            return Err(DT_EARG);
        }
        v = v.checked_mul(10).ok_or(DT_EARG)?;
        v = v.checked_add((c - b'0') as i64).ok_or(DT_EARG)?;
        i += 1;
    }
    let signed = if neg { v.checked_neg().ok_or(DT_EARG)? } else { v };
    if signed < EPOCH_MIN || signed > EPOCH_MAX {
        return Err(DT_ERANGE);
    }
    Ok(signed)
}

/// 解析 `YYYY-MM-DD` 并返回当日零点纪元。
pub fn parse_iso_date(b: &[u8]) -> Result<i64, u32> {
    if b.len() != 10 {
        return Err(DT_EARG);
    }
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return Err(DT_EARG);
    }
    let y = read_digits(b, 0, 4)?;
    let m = read_digits(b, 5, 2)?;
    let d = read_digits(b, 8, 2)?;
    epoch_of(y as i32, m as u8, d as u8, 0, 0, 0)
}

/// 定长十进制读取。
fn read_digits(b: &[u8], at: usize, n: usize) -> Result<u32, u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < n {
        let c = match b.get(at + i) {
            Some(v) => *v,
            None => return Err(DT_EARG),
        };
        if c < b'0' || c > b'9' {
            return Err(DT_EARG);
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
        i += 1;
    }
    Ok(v)
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
            return Err(DT_ESLOT);
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

    /// 对账：格数、成败比、槽位序、观测值表、错误码表。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DT_EBAL);
        }
        const VALS: [u32; 8] = [4, 10_957, 946_684_800, 60, 53, 8, 31, 5];
        const CODES: [u32; 6] = [DT_EFIELD, DT_EBYTE, DT_ELOCALE, DT_ERANGE, DT_EFMT, DT_EARG];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DT_EBAL),
            };
            if s.slot as usize != i {
                return Err(DT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DT_EVALUE);
                        }
                    }
                    None => return Err(DT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DT_EVALUE);
                        }
                    }
                    None => return Err(DT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DT_EBAL);
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
            return Err(DT_EBAL);
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
            Err(DT_ESEAL)
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

/// `Result<u32, u32>` → 样本。
fn u32_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u64, u32>` → 样本。
fn u64_cell(slot: u8, op: u8, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => {
            if v > u32::MAX as u64 {
                Sample { slot, op, ok: false, code: DT_ERANGE, value: 0 }
            } else {
                Sample { slot, op, ok: true, code: 0, value: v as u32 }
            }
        }
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 周几：纪元天 → 0..6。
fn wday_of_days(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

/// 账本组装：槽 0..7 成、8..13 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(u32_cell(0, OP_WDAY, Ok(wday_of_days(0))));

    let _ = l.register(u64_cell(
        1,
        OP_CIVIL,
        days_from_civil(2000, 1, 1).map(|v| v as u64),
    ));

    let _ = l.register(u64_cell(
        2,
        OP_EPOCH,
        epoch_of(2000, 1, 1, 0, 0, 0).map(|v| v as u64),
    ));

    let e3 = match epoch_of(2024, 3, 1, 0, 0, 0) {
        Ok(v) => v,
        Err(c) => return fail_tail(l, c),
    };
    let _ = l.register(u32_cell(3, OP_WDAY, stamp_of_epoch(e3).map(|s| s.yday as u32)));

    let e4 = match epoch_of(2021, 1, 1, 0, 0, 0) {
        Ok(v) => v,
        Err(c) => return fail_tail(l, c),
    };
    let _ = l.register(u32_cell(4, OP_ISOWK, stamp_of_epoch(e4).map(|s| s.iso_week as u32)));

    let _ = l.register(u32_cell(
        5,
        OP_FMT,
        render(&Stamp::blank(), b"%A").map(|s| s.len() as u32),
    ));

    let _ = l.register(u32_cell(
        6,
        OP_RUN,
        render(&Stamp::blank(), b"%a, %d %b %Y %H:%M:%S +0000").map(|s| s.len() as u32),
    ));

    let _ = l.register(u32_cell(7, OP_FMT, step_count(b"%Y-%m-%d").map(|v| v as u32)));

    let _ = l.register(u32_cell(8, OP_FMT, render(&Stamp::blank(), b"%Q").map(|_| 0)));

    let _ = l.register(u32_cell(
        9,
        OP_FMT,
        render(&Stamp::blank(), &[b'%', b'd', 0x80]).map(|_| 0),
    ));

    let _ = l.register(u32_cell(10, OP_LOCALE, locale_from(b"ja_JP.UTF-8").map(|_| 0)));

    let _ = l.register(u32_cell(
        11,
        OP_EPOCH,
        stamp_of_epoch(EPOCH_MAX + 1).map(|_| 0),
    ));

    let _ = l.register(u32_cell(12, OP_FMT, render(&Stamp::blank(), b"abc%").map(|_| 0)));

    let _ = l.register(u32_cell(13, OP_ARGS, parse_epoch(b"@x").map(|_| 0)));

    l
}

/// 构造期失败时把尾部补成失败格，保持 14 格形状。
fn fail_tail(mut l: Ledger, code: u32) -> Ledger {
    let mut slot = l.len();
    while slot < SAMPLES {
        let s = Sample { slot: slot as u8, op: OP_RUN, ok: false, code, value: 0 };
        let _ = l.register(s);
        slot += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 历法面。
fn calendar_ok() -> bool {
    let a = is_leap(2024) && is_leap(2000) && !is_leap(1900) && !is_leap(2023);
    let b = days_in_month(2024, 2) == 29 && days_in_month(2023, 2) == 28;
    let c = days_in_month(2024, 13) == 0 && days_in_month(2024, 0) == 0;
    let d = civil_from_days(0) == Ok((1970, 1, 1));
    let e = civil_from_days(-1) == Ok((1969, 12, 31));
    let f = days_from_civil(1970, 1, 1) == Ok(0);
    let g = days_from_civil(2000, 1, 1) == Ok(10_957);
    let h = epoch_of(2024, 2, 29, 0, 0, 0) == Ok(1_709_164_800);
    let i = days_from_civil(2024, 2, 30).err() == Some(DT_ERANGE);
    a && b && c && d && e && f && g && h && i
}

/// 星期与序日面。
fn week_ok() -> bool {
    let s0 = match stamp_of_epoch(0) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s0.wday == 4 && s0.year == 1970 && s0.month == 1 && s0.day == 1;
    let s1 = match epoch_of(2024, 3, 1, 12, 34, 56) {
        Ok(e) => match stamp_of_epoch(e) {
            Ok(v) => v,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    let b = s1.yday == 60 && s1.hour == 12 && s1.min == 34 && s1.sec == 56;
    let c = wday_of_days(0) == 4 && wday_of_days(3) == 0 && wday_of_days(4) == 1;
    let d = stamp_of_epoch(1_709_164_800).map(|s| s.wday) == Ok(4);
    let e = stamp_of_epoch(EPOCH_MIN).is_ok() && stamp_of_epoch(EPOCH_MAX).is_ok();
    a && b && c && d && e
}

/// ISO 周锚面。
fn iso_ok() -> bool {
    let a = stamp_of_epoch(match epoch_of(2021, 1, 1, 0, 0, 0) {
        Ok(v) => v,
        Err(_) => return false,
    })
    .map(|s| (s.iso_year, s.iso_week, s.iso_wday))
        == Ok((2020, 53, 5));
    let b = stamp_of_epoch(match epoch_of(2021, 1, 4, 0, 0, 0) {
        Ok(v) => v,
        Err(_) => return false,
    })
    .map(|s| (s.iso_year, s.iso_week, s.iso_wday))
        == Ok((2021, 1, 1));
    let c = stamp_of_epoch(match epoch_of(1970, 1, 1, 0, 0, 0) {
        Ok(v) => v,
        Err(_) => return false,
    })
    .map(|s| (s.iso_year, s.iso_week, s.iso_wday))
        == Ok((1970, 1, 4));
    let d = stamp_of_epoch(match epoch_of(2020, 12, 31, 0, 0, 0) {
        Ok(v) => v,
        Err(_) => return false,
    })
    .map(|s| s.iso_week)
        == Ok(53);
    a && b && c && d
}

/// 格式编译面。
fn compile_ok() -> bool {
    let a = step_count(b"%Y-%m-%d") == Ok(5);
    let b = step_count(b"%a %b %e %H:%M:%S %Z %Y") == Ok(15);
    let c = step_count(b"") == Ok(0);
    let d = match compile(b"%8s") {
        Ok((_, _)) => false,
        Err(c) => c == DT_EFIELD,
    };
    let e = match compile(b"%-d") {
        Ok((st, n)) => {
            n == 1
                && st
                    .get(0)
                    .map(|s| s.code == b'd' && s.pad == 1 && s.width == 0)
                    .unwrap_or(false)
        }
        Err(_) => false,
    };
    let f = match compile(b"%04Y") {
        Ok((st, n)) => {
            n == 1
                && st
                    .get(0)
                    .map(|s| s.code == b'Y' && s.pad == b'0' && s.width == 4)
                    .unwrap_or(false)
        }
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// 名表面。
fn names_ok() -> bool {
    let a = WDAY_ABBR.len() == 7 && WDAY_FULL.len() == 7;
    let b = MON_ABBR.len() == 12 && MON_FULL.len() == 12;
    let c = names_c_anchored();
    let d = wday_full(4) == Ok("Thursday") && wday_abbr(4) == Ok("Thu");
    let e = mon_full(9) == Ok("September") && mon_abbr(1) == Ok("Jan");
    let f = wday_full(7).err() == Some(DT_ENAME) && mon_full(0).err() == Some(DT_ENAME);
    let g = mon_full(12) == Ok("December") && wday_full(6) == Ok("Saturday");
    a && b && c && d && e && f && g
}

/// 渲染面。
fn render_ok() -> bool {
    let s = Stamp::blank();
    let a = render(&s, b"%Y-%m-%d").map(|v| v.as_str() == "1970-01-01") == Ok(true);
    let b = render(&s, b"%a %b %e").map(|v| v.as_str() == "Thu Jan  1") == Ok(true);
    let c = render(&s, b"%H:%M:%S").map(|v| v.as_str() == "00:00:00") == Ok(true);
    let d = render(&s, b"%j %A %B").map(|v| v.as_str() == "001 Thursday January") == Ok(true);
    let e = render(&s, b"100%%").map(|v| v.as_str() == "100%") == Ok(true);
    let f = render(&s, b"%-d|%3d|%d").map(|v| v.as_str() == "1|  1|01") == Ok(true);
    let g = render(&s, b"%Z %p %u %w").map(|v| v.as_str() == "UTC AM 4 4") == Ok(true);
    let h = render(&s, b"%y/%G-W%V").map(|v| v.as_str() == "70/1970-W01") == Ok(true);
    a && b && c && d && e && f && g && h
}

/// 区域面。
fn locale_ok() -> bool {
    let a = locale_from(b"C").map(|l| l.is_c_anchor()) == Ok(true);
    let b = locale_from(b"POSIX").map(|l| l.text()) == Ok("POSIX");
    let c = locale_from(b"C.UTF-8").map(|l| l.is_c_anchor()) == Ok(true);
    let d = locale_from(b"de_DE.UTF-8").map(|l| l.is_c_anchor()) == Ok(false);
    let e = locale_from(b"fr_FR.UTF-8").err() == None
        && locale_from(b"fr_FR.UTF-8").map(|l| l.text()) == Ok("fr_FR.UTF-8");
    let f = locale_from(b"ja_JP.UTF-8").err() == Some(DT_ELOCALE);
    let g = Locale::C.zone_abbr() == "UTC" && Locale::De.zone_abbr() == "UTC";
    a && b && c && d && e && f && g
}

/// 参数与范围面。
fn args_ok() -> bool {
    let a = match parse_argv(&[b"-u", b"+%Y"]) {
        Ok(o) => o.utc && o.fmt_bytes() == b"%Y" && o.epoch == 0,
        Err(_) => false,
    };
    let b = match parse_argv(&[b"@86400"]) {
        Ok(o) => o.epoch == 86_400,
        Err(_) => false,
    };
    let c = parse_argv(&[b"@x"]).err() == Some(DT_EARG);
    let d = parse_argv(&[b"-z"]).err() == Some(DT_EARG);
    let e = parse_argv(&[b"+%Y", b"+%m"]).err() == Some(DT_ESTATE);
    let f = parse_argv(&[b"-Lde_DE.UTF-8"]).err() == Some(DT_ELOCALE);
    let g = parse_argv(&[b"-L", b"C"]).err() == Some(DT_EARG);
    let h = parse_iso_date(b"2024-02-29") == Ok(1_709_164_800);
    let i = parse_iso_date(b"2024-2-29").err() == Some(DT_EARG);
    let j = parse_epoch(b"-1") == Ok(-1)
        && parse_epoch(&[b'9'; 19]).err() == Some(DT_EARG);
    a && b && c && d && e && f && g && h && i && j
}

/// 越界面。
fn range_ok() -> bool {
    let a = stamp_of_epoch(EPOCH_MAX + 1).err() == Some(DT_ERANGE);
    let b = stamp_of_epoch(EPOCH_MIN - 1).err() == Some(DT_ERANGE);
    let c = epoch_of(2024, 0, 1, 0, 0, 0).err() == Some(DT_ERANGE);
    let d = epoch_of(2024, 1, 1, 24, 0, 0).err() == Some(DT_ERANGE);
    let e = render(&Stamp::blank(), &[0x01]).err() == Some(DT_EBYTE);
    let f = render(&Stamp::blank(), b"%").err() == Some(DT_EFMT);
    let g = render(&Stamp::blank(), b"%12345Y").err() == Some(DT_EFMT);
    let mut bad = Stamp::blank();
    bad.wday = 9;
    let h = render(&bad, b"%A").err() == Some(DT_ENAME);
    let mut badm = Stamp::blank();
    badm.month = 13;
    let i = render(&badm, b"%B").err() == Some(DT_ENAME);
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
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    if l2.register(Sample::blank()) != Err(DT_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = calendar_ok() && week_ok();
    out[1] = iso_ok() && compile_ok();
    out[2] = names_ok() && locale_ok();
    out[3] = render_ok() && args_ok() && range_ok() && ledger_ok();
    out
}

/// 判据集合。
pub fn run_unx11573_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11573.date_fmt");

    s.add("DT01-历法换算", calendar_ok(), "static");
    s.add("DT02-星期与序日", week_ok(), "static");
    s.add("DT03-ISO 周锚", iso_ok(), "static");
    s.add("DT04-格式编译", compile_ok(), "static");
    s.add("DT05-英文名表", names_ok(), "static");
    s.add("DT06-定长渲染", render_ok(), "static");
    s.add("DT07-区域拒绝", locale_ok(), "static");
    s.add("DT08-参数解析", args_ok(), "static");
    s.add("DT09-范围越界", range_ok(), "static");
    s.add(
        "DT10-容量步数",
        {
            let a = step_count(b"%Y-%m-%dT%H:%M:%S").ok() == Some(11);
            let b = compile(&[b'%'; 200]).err() == Some(DT_EFMT);
            let c = compile(b"%12345Y").err() == Some(DT_EFMT);
            let d = match compile(&[b'x'; STEP_CAP + 1]) {
                Ok((_, n)) => n == STEP_CAP + 1,
                Err(c) => c == DT_EFMT,
            };
            a && b && c && d
        },
        "static",
    );
    s.add("DT11-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "DT12-冻结指纹",
        {
            let mut l = build_ledger();
            match l.freeze() {
                Ok(r) => r == l.seal() && l.is_frozen() && l.freeze() == Err(DT_EBAL),
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "DT13-冻结重放四注入",
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
    fn leap_rules() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2100));
        assert!(!is_leap(2023));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
        assert_eq!(days_in_month(2024, 0), 0);
    }

    #[test]
    fn civil_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), Ok(0));
        assert_eq!(days_from_civil(2000, 1, 1), Ok(10_957));
        assert_eq!(civil_from_days(0), Ok((1970, 1, 1)));
        assert_eq!(civil_from_days(-1), Ok((1969, 12, 31)));
        assert_eq!(civil_from_days(10_957), Ok((2000, 1, 1)));
        let mut d = -20_000i64;
        while d < 20_000 {
            let (y, m, dd) = civil_from_days(d).expect("civil");
            assert_eq!(days_from_civil(y, m, dd), Ok(d));
            d += 97;
        }
    }

    #[test]
    fn epoch_roundtrip() {
        assert_eq!(epoch_of(2000, 1, 1, 0, 0, 0), Ok(946_684_800));
        assert_eq!(epoch_of(2024, 2, 29, 0, 0, 0), Ok(1_709_164_800));
        assert_eq!(epoch_of(1970, 1, 1, 0, 0, 0), Ok(0));
        let st = stamp_of_epoch(1_709_164_800).expect("stamp");
        assert_eq!((st.year, st.month, st.day), (2024, 2, 29));
        assert_eq!(epoch_of_stamp(&st), Ok(1_709_164_800));
    }

    #[test]
    fn weekday_and_yday() {
        let s = stamp_of_epoch(0).expect("s");
        assert_eq!(s.wday, 4);
        assert_eq!(s.yday, 0);
        let t = stamp_of_epoch(epoch_of(2024, 3, 1, 0, 0, 0).expect("e")).expect("t");
        assert_eq!(t.yday, 60);
        assert_eq!(t.wday, 5);
    }

    #[test]
    fn iso_weeks() {
        let a = stamp_of_epoch(epoch_of(2021, 1, 1, 0, 0, 0).expect("e")).expect("s");
        assert_eq!((a.iso_year, a.iso_week, a.iso_wday), (2020, 53, 5));
        let b = stamp_of_epoch(epoch_of(2021, 1, 4, 0, 0, 0).expect("e")).expect("s");
        assert_eq!((b.iso_year, b.iso_week, b.iso_wday), (2021, 1, 1));
        let c = stamp_of_epoch(epoch_of(1970, 1, 1, 0, 0, 0).expect("e")).expect("s");
        assert_eq!((c.iso_year, c.iso_week, c.iso_wday), (1970, 1, 4));
        let d = stamp_of_epoch(epoch_of(2020, 12, 31, 0, 0, 0).expect("e")).expect("s");
        assert_eq!(d.iso_week, 53);
    }

    #[test]
    fn compile_shapes() {
        assert_eq!(step_count(b"%Y-%m-%d"), Ok(5));
        assert_eq!(step_count(b""), Ok(0));
        let (st, n) = compile(b"%-d").expect("c");
        assert_eq!(n, 1);
        assert_eq!(st[0].code, b'd');
        assert_eq!(st[0].pad, 1);
        let (st2, n2) = compile(b"%04Y").expect("c");
        assert_eq!(n2, 1);
        assert_eq!(st2[0].width, 4);
        assert_eq!(st2[0].pad, b'0');
    }

    #[test]
    fn compile_errors() {
        assert_eq!(compile(b"%Q").err(), Some(DT_EFIELD));
        assert_eq!(compile(b"abc%").err(), Some(DT_EFMT));
        assert_eq!(compile(&[0x80]).err(), Some(DT_EBYTE));
        assert_eq!(compile(b"%12345Y").err(), Some(DT_EFMT));
    }

    #[test]
    fn english_names() {
        assert_eq!(wday_abbr(0), Ok("Sun"));
        assert_eq!(wday_full(4), Ok("Thursday"));
        assert_eq!(mon_abbr(1), Ok("Jan"));
        assert_eq!(mon_full(12), Ok("December"));
        assert_eq!(wday_full(7).err(), Some(DT_ENAME));
        assert_eq!(mon_full(0).err(), Some(DT_ENAME));
        assert!(names_c_anchored());
    }

    #[test]
    fn renders_c_anchor() {
        let s = Stamp::blank();
        assert_eq!(render(&s, b"%Y-%m-%d").expect("r").as_str(), "1970-01-01");
        assert_eq!(render(&s, b"%a %b %e").expect("r").as_str(), "Thu Jan  1");
        assert_eq!(render(&s, b"%A %B").expect("r").as_str(), "Thursday January");
        assert_eq!(render(&s, b"%Z %p").expect("r").as_str(), "UTC AM");
        assert_eq!(render(&s, b"%-d|%3d|%d").expect("r").as_str(), "1|  1|01");
        assert_eq!(render(&s, b"%j").expect("r").as_str(), "001");
        assert_eq!(render(&s, b"100%%").expect("r").as_str(), "100%");
    }

    #[test]
    fn rfc_length() {
        let s = Stamp::blank();
        let r = render(&s, b"%a, %d %b %Y %H:%M:%S +0000").expect("r");
        assert_eq!(r.len(), 31);
        assert_eq!(r.as_str(), "Thu, 01 Jan 1970 00:00:00 +0000");
    }

    #[test]
    fn locales() {
        assert_eq!(locale_from(b"C"), Ok(Locale::C));
        assert_eq!(locale_from(b"POSIX"), Ok(Locale::Posix));
        assert_eq!(locale_from(b"C.UTF-8"), Ok(Locale::CUtf8));
        assert!(!Locale::De.is_c_anchor());
        assert_eq!(locale_from(b"xx_YY"), Err(DT_ELOCALE));
        assert_eq!(Locale::C.zone_abbr(), "UTC");
    }

    #[test]
    fn argv_forms() {
        let o = parse_argv(&[b"-u", b"+%Y-%m-%d", b"@86400"]).expect("o");
        assert!(o.utc);
        assert_eq!(o.fmt_bytes(), b"%Y-%m-%d");
        assert_eq!(o.epoch, 86_400);
        assert_eq!(parse_argv(&[b"-z"]).err(), Some(DT_EARG));
        assert_eq!(parse_argv(&[b"+%Y", b"+%m"]).err(), Some(DT_ESTATE));
        assert_eq!(parse_argv(&[b"-L", b"C"]).err(), Some(DT_EARG));
        assert_eq!(parse_argv(&[b"-Lde_DE.UTF-8"]).err(), Some(DT_ELOCALE));
    }

    #[test]
    fn epoch_and_iso_parse() {
        assert_eq!(parse_epoch(b"0"), Ok(0));
        assert_eq!(parse_epoch(b"-1"), Ok(-1));
        assert_eq!(parse_epoch(b"").err(), Some(DT_EARG));
        assert_eq!(parse_epoch(b"12x").err(), Some(DT_EARG));
        assert_eq!(parse_iso_date(b"2024-02-29"), Ok(1_709_164_800));
        assert_eq!(parse_iso_date(b"2024-2-29").err(), Some(DT_EARG));
        assert_eq!(parse_iso_date(b"2024-02-30").err(), Some(DT_ERANGE));
    }

    #[test]
    fn ranges() {
        assert_eq!(stamp_of_epoch(EPOCH_MAX + 1).err(), Some(DT_ERANGE));
        assert_eq!(epoch_of(2024, 1, 1, 24, 0, 0).err(), Some(DT_ERANGE));
        assert_eq!(parse_epoch(&[b'9'; 19]).err(), Some(DT_EARG));
        assert!(stamp_of_epoch(EPOCH_MAX).is_ok());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(4));
        assert_eq!(l.sample(2).map(|s| s.value), Some(946_684_800));
        assert_eq!(l.sample(8).map(|s| s.code), Some(DT_EFIELD));
        assert_eq!(l.sample(13).map(|s| s.code), Some(DT_EARG));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DT_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(DT_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11573_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 13);
    }
}
