//! UNX-F11507 date 格式串与 -d/-u 档（0xf0c0 段）
//!
//! `date` 的核心不是"打印当前时间"，而是把一个大整数（纪元秒）翻译成
//! 公历七元组，再按格式串逐符展开。本卡把这条链路拆成三段：**换算**
//! （纪元秒 + 时区偏移 → 年/月/日/时/分/秒/星期/年序日）、**格式**
//! （`%` 引导的转换符 → 字节）、**选项**（`-u` 选 UTC，`-d` 送入一个
//! 待解释的时刻串）。三段各自有独立的闸门，互不代偿。
//!
//! 换算采用 Howard Hinnant 的 civil-from-days 算法：以 400 年为一个纪元，
//! 把 1970-01-01 之后的日数折成"纪元内日序"，再解出年月日。它没有循环，
//! 没有查表，对 1970..=2038 的整个区间都成立；星期由 `(days + 4) mod 7`
//! 得出（1970-01-01 是星期四），年序日由月长表累加得出，闰年只认
//! "被 4 整除且不被 100 整除，或被 400 整除"这一条。
//!
//! 换算闸门：纪元秒不得超过 32 位有符号上限（2038 问题），时区偏移
//! 限制在 ±14 小时，偏移后落到负时刻也一律 `ERANGE`。格式闸门：`%` 后
//! 必须紧跟一个认识的转换符，串尾孤立的 `%` 是 `ETRUNC`（截断），
//! 不认识的字母是 `EBADFMT`；两者都在写缓冲之前判完，失败时一格不写。
//! 渲染前先用 `fmt_len` 算总长，缓冲不足即 `EBUF`。
//!
//! `-d` 接受两种写法：`@<纪元秒>` 与 `YYYY-MM-DD[ hh:mm:ss]`（空格或 `T`
//! 分隔）。字段宽度、分隔符位置、月长、时分秒上界逐项校验，非闰年的
//! 2 月 29 日是 `EBADDATE` 而不是"顺手进位"。选项解析的闸门：`-u` 重复
//! 是 `EDUP`，`-d` 缺操作数是 `EOPERAND`，`-d` 重复是 `EDUP`，其余带横线
//! 的词一律 `EBADOPT`；裸词也拒，因为本卡的选项表里没有位置参数。
//!
//! 账本十四格：八格成功（零点年序、已知时刻年序、日期长度、完整渲染、
//! 年序日渲染、星期长度、纪元秒渲染、`-d @...` 解析），六格拒绝
//! （未知转换符、尾部百分号、非法日期、纪元越界、缓冲不足、缺操作数）。
//! 审计逐格复算值与码，封印折叠全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0c0;

/// 未知转换符。
pub const DT_EBADFMT: u32 = SEG_BASE + 0x01;
/// 串尾孤立百分号。
pub const DT_ETRUNC: u32 = SEG_BASE + 0x02;
/// 日期串非法。
pub const DT_EBADDATE: u32 = SEG_BASE + 0x03;
/// 纪元或偏移越界。
pub const DT_ERANGE: u32 = SEG_BASE + 0x04;
/// 渲染缓冲不足。
pub const DT_EBUF: u32 = SEG_BASE + 0x05;
/// 选项缺操作数。
pub const DT_EOPERAND: u32 = SEG_BASE + 0x06;
/// 未知选项。
pub const DT_EBADOPT: u32 = SEG_BASE + 0x07;
/// 重复选项。
pub const DT_EDUP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 32 位有符号纪元上限。
pub const MAX_EPOCH: u32 = 0x7fff_ffff;
/// 时区偏移上界（±14 小时）。
pub const ZONE_MAX: i32 = 14 * 3600;
/// 一天的秒数。
pub const DAY: i32 = 86_400;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：取七元组。
pub const OP_FIELDS: u8 = 1;
/// 操作码：算格式长度。
pub const OP_FMT: u8 = 2;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 3;
/// 操作码：解析日期串。
pub const OP_PARSE: u8 = 4;
/// 操作码：解析选项。
pub const OP_PLAN: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 闰年。
pub fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 某月天数（非法月份返回 0）。
pub fn days_in_month(y: u32, m: u32) -> u32 {
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

/// 日数 → 公历年月日。
pub fn civil_from_days(z0: i32) -> (i32, u32, u32) {
    let z = z0 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// 公历年月日 → 日数（1970-01-01 为 0）。
pub fn days_from_civil(y0: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y0 - 1 } else { y0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i32;
    let doy = (153 * mp + 2) / 5 + d as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 年内日序（1 起）。
pub fn day_of_year(y: u32, m: u32, d: u32) -> u32 {
    let mut n = d;
    let mut i = 1u32;
    while i < m {
        n = n.saturating_add(days_in_month(y, i));
        i += 1;
    }
    n
}

/// 公历七元组加实际纪元秒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fields {
    /// 年。
    pub year: u32,
    /// 月。
    pub month: u32,
    /// 日。
    pub day: u32,
    /// 时。
    pub hh: u32,
    /// 分。
    pub mm: u32,
    /// 秒。
    pub ss: u32,
    /// 星期（0=周日）。
    pub wday: u32,
    /// 年序日。
    pub yday: u32,
    /// 偏移后的纪元秒（`%s` 用）。
    pub stamp: u32,
}

/// 纪元秒 + 偏移 → 七元组。
pub fn fields(epoch: u32, off: i32) -> Result<Fields, u32> {
    if epoch > MAX_EPOCH || off > ZONE_MAX || off < -ZONE_MAX {
        return Err(DT_ERANGE);
    }
    let shift = epoch as i32 + off;
    if shift < 0 || shift > MAX_EPOCH as i32 {
        return Err(DT_ERANGE);
    }
    let days = shift / DAY;
    let rem = shift % DAY;
    let (y, m, d) = civil_from_days(days);
    if y < 1970 || y > 2038 {
        return Err(DT_ERANGE);
    }
    Ok(Fields {
        year: y as u32,
        month: m,
        day: d,
        hh: (rem / 3600) as u32,
        mm: (rem % 3600 / 60) as u32,
        ss: (rem % 60) as u32,
        wday: ((days + 4) % 7) as u32,
        yday: day_of_year(y as u32, m, d),
        stamp: shift as u32,
    })
}

/// 十进制位数。
fn digits_of(mut v: u32) -> u32 {
    let mut n = 1u32;
    while v >= 10 {
        v /= 10;
        n += 1;
    }
    n
}

/// 单个转换符的输出宽度。
fn spec_width(c: u8, f: &Fields) -> Result<u32, u32> {
    match c {
        b'Y' => Ok(4),
        b'm' | b'd' | b'H' | b'M' | b'S' => Ok(2),
        b'j' => Ok(3),
        b's' => Ok(digits_of(f.stamp)),
        b'F' => Ok(10),
        b'T' => Ok(8),
        b'a' | b'b' => Ok(3),
        b'%' => Ok(1),
        _ => Err(DT_EBADFMT),
    }
}

/// 格式串展开后的总长。
pub fn fmt_len(fmt: &[u8], f: &Fields) -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < fmt.len() {
        match fmt.get(i).copied() {
            Some(b'%') => {
                let c = match fmt.get(i + 1).copied() {
                    Some(v) => v,
                    None => return Err(DT_ETRUNC),
                };
                n = n.saturating_add(spec_width(c, f)?);
                i += 2;
            }
            Some(_) => {
                n = n.saturating_add(1);
                i += 1;
            }
            None => break,
        }
    }
    Ok(n)
}

/// 星期缩写。
fn weekday_name(w: u32) -> &'static [u8] {
    match w {
        0 => b"Sun",
        1 => b"Mon",
        2 => b"Tue",
        3 => b"Wed",
        4 => b"Thu",
        5 => b"Fri",
        _ => b"Sat",
    }
}

/// 月名缩写。
fn month_name(m: u32) -> &'static [u8] {
    match m {
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
        _ => b"Dec",
    }
}

/// 写字节串。
fn put(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    let mut i = 0usize;
    while i < src.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    at + src.len()
}

/// 零填充右对齐写十进制。
fn pad(dst: &mut [u8], at: usize, v: u32, width: u32) -> usize {
    let mut tmp = [b'0'; 10];
    let mut vv = v;
    let n = digits_of(vv) as usize;
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(cell) = tmp.get_mut(i) {
            *cell = b'0' + (vv % 10) as u8;
        }
        vv /= 10;
    }
    let mut at2 = at;
    let mut fill = n as u32;
    while fill < width {
        at2 = put(dst, at2, b"0");
        fill += 1;
    }
    let src: &[u8] = match tmp.get(..n) {
        Some(s) => s,
        None => return at2,
    };
    put(dst, at2, src)
}

/// 一个转换符写入。
fn emit(dst: &mut [u8], at: usize, c: u8, f: &Fields) -> usize {
    match c {
        b'Y' => pad(dst, at, f.year, 4),
        b'm' => pad(dst, at, f.month, 2),
        b'd' => pad(dst, at, f.day, 2),
        b'H' => pad(dst, at, f.hh, 2),
        b'M' => pad(dst, at, f.mm, 2),
        b'S' => pad(dst, at, f.ss, 2),
        b'j' => pad(dst, at, f.yday, 3),
        b's' => pad(dst, at, f.stamp, digits_of(f.stamp)),
        b'F' => {
            let a = pad(dst, at, f.year, 4);
            let b1 = put(dst, a, b"-");
            let c1 = pad(dst, b1, f.month, 2);
            let d1 = put(dst, c1, b"-");
            pad(dst, d1, f.day, 2)
        }
        b'T' => {
            let a = pad(dst, at, f.hh, 2);
            let b1 = put(dst, a, b":");
            let c1 = pad(dst, b1, f.mm, 2);
            let d1 = put(dst, c1, b":");
            pad(dst, d1, f.ss, 2)
        }
        b'a' => put(dst, at, weekday_name(f.wday)),
        b'b' => put(dst, at, month_name(f.month)),
        b'%' => put(dst, at, b"%"),
        _ => at,
    }
}

/// 按格式串渲染。
pub fn render(fmt: &[u8], f: &Fields, dst: &mut [u8]) -> Result<u32, u32> {
    let need = fmt_len(fmt, f)? as usize;
    if dst.len() < need {
        return Err(DT_EBUF);
    }
    let mut at = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        match fmt.get(i).copied() {
            Some(b'%') => {
                let c = match fmt.get(i + 1).copied() {
                    Some(v) => v,
                    None => return Err(DT_ETRUNC),
                };
                at = emit(dst, at, c, f);
                i += 2;
            }
            Some(b) => {
                at = put(dst, at, &[b]);
                i += 1;
            }
            None => break,
        }
    }
    Ok(at as u32)
}

/// 取一段定宽十进制。
fn num_at(text: &[u8], at: usize, n: usize) -> Option<u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < n {
        let b = *text.get(at + i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as u32;
        i += 1;
    }
    Some(v)
}

/// 解析十进制纪元秒。
fn dec_u32(text: &[u8]) -> Result<u32, u32> {
    if text.is_empty() {
        return Err(DT_EBADDATE);
    }
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < text.len() {
        let b = match text.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(DT_EBADDATE);
        }
        acc = acc * 10 + (b - b'0') as u64;
        if acc > MAX_EPOCH as u64 {
            return Err(DT_ERANGE);
        }
        i += 1;
    }
    Ok(acc as u32)
}

/// 解析 `-d` 的时刻串。
pub fn parse_date(text: &[u8]) -> Result<u32, u32> {
    if text.is_empty() {
        return Err(DT_EBADDATE);
    }
    if text.get(0).copied() == Some(b'@') {
        let tail: &[u8] = match text.get(1..) {
            Some(v) => v,
            None => return Err(DT_EBADDATE),
        };
        return dec_u32(tail);
    }
    if text.len() < 10 {
        return Err(DT_EBADDATE);
    }
    if text.get(4).copied() != Some(b'-') || text.get(7).copied() != Some(b'-') {
        return Err(DT_EBADDATE);
    }
    let y = match num_at(text, 0, 4) {
        Some(v) => v,
        None => return Err(DT_EBADDATE),
    };
    let m = match num_at(text, 5, 2) {
        Some(v) => v,
        None => return Err(DT_EBADDATE),
    };
    let d = match num_at(text, 8, 2) {
        Some(v) => v,
        None => return Err(DT_EBADDATE),
    };
    let mut hh = 0u32;
    let mut mm = 0u32;
    let mut ss = 0u32;
    if text.len() > 10 {
        if text.len() != 19 {
            return Err(DT_EBADDATE);
        }
        match text.get(10).copied() {
            Some(b' ') | Some(b'T') => {}
            _ => return Err(DT_EBADDATE),
        }
        if text.get(13).copied() != Some(b':') || text.get(16).copied() != Some(b':') {
            return Err(DT_EBADDATE);
        }
        hh = match num_at(text, 11, 2) {
            Some(v) => v,
            None => return Err(DT_EBADDATE),
        };
        mm = match num_at(text, 14, 2) {
            Some(v) => v,
            None => return Err(DT_EBADDATE),
        };
        ss = match num_at(text, 17, 2) {
            Some(v) => v,
            None => return Err(DT_EBADDATE),
        };
    }
    if y < 1970 || y > 2038 {
        return Err(DT_ERANGE);
    }
    if m < 1 || m > 12 {
        return Err(DT_EBADDATE);
    }
    let dim = days_in_month(y, m);
    if d < 1 || d > dim {
        return Err(DT_EBADDATE);
    }
    if hh > 23 || mm > 59 || ss > 59 {
        return Err(DT_EBADDATE);
    }
    let days = days_from_civil(y as i32, m, d);
    let total = days as i64 * DAY as i64 + (hh * 3600 + mm * 60 + ss) as i64;
    if total < 0 || total > MAX_EPOCH as i64 {
        return Err(DT_ERANGE);
    }
    Ok(total as u32)
}

/// 选项计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 是否用 UTC。
    pub utc: bool,
    /// `-d` 送入的纪元秒。
    pub given: Option<u32>,
}

/// 解析选项表。
pub fn parse_args(args: &[&[u8]]) -> Result<Plan, u32> {
    let mut p = Plan { utc: false, given: None };
    let mut i = 0usize;
    while i < args.len() {
        let a: &[u8] = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a == b"-u" {
            if p.utc {
                return Err(DT_EDUP);
            }
            p.utc = true;
            i += 1;
        } else if a == b"-d" {
            if p.given.is_some() {
                return Err(DT_EDUP);
            }
            let v: &[u8] = match args.get(i + 1) {
                Some(v) => *v,
                None => return Err(DT_EOPERAND),
            };
            p.given = Some(parse_date(v)?);
            i += 2;
        } else {
            return Err(DT_EBADOPT);
        }
    }
    Ok(p)
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
            self.rejects += 1;
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DT_EBAL);
        }
        const VALS: [u32; 8] = [1, 318, 10, 19, 3, 3, 10, 1_000_000_000];
        const CODES: [u32; 6] =
            [DT_EBADFMT, DT_ETRUNC, DT_EBADDATE, DT_ERANGE, DT_EBUF, DT_EOPERAND];
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
            if s.op == 0 {
                return Err(DT_EVALUE);
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
                if s.value != 0 {
                    return Err(DT_EVALUE);
                }
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
            self.rejects += 1;
            return Err(DT_EBAL);
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

/// 十四格账本：八个成功观测 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let zero = match fields(0, 0) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let now = match fields(1_700_000_000, 0) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_FIELDS, Ok(zero.yday)));
    let _ = l.register(word_cell(1, OP_FIELDS, Ok(now.yday)));
    let _ = l.register(word_cell(2, OP_FMT, fmt_len(b"%Y-%m-%d", &now)));

    let mut buf = [0u8; 32];
    let _ = l.register(word_cell(3, OP_RENDER, render(b"%F %T", &now, &mut buf)));
    let _ = l.register(word_cell(4, OP_RENDER, render(b"%j", &now, &mut buf)));
    let _ = l.register(word_cell(5, OP_FMT, fmt_len(b"%a", &now)));
    let _ = l.register(word_cell(6, OP_RENDER, render(b"%s", &now, &mut buf)));
    let _ = l.register(word_cell(7, OP_PARSE, parse_date(b"@1000000000")));

    let _ = l.register(word_cell(8, OP_FMT, fmt_len(b"%Q", &now)));
    let _ = l.register(word_cell(9, OP_FMT, fmt_len(b"abc%", &now)));
    let _ = l.register(word_cell(10, OP_PARSE, parse_date(b"2023-02-30")));
    let _ = l.register(word_cell(11, OP_FIELDS, fields(0x8000_0000, 0).map(|f| f.year)));

    let mut tiny = [0u8; 4];
    let _ = l.register(word_cell(12, OP_RENDER, render(b"%F %T", &now, &mut tiny)));

    let dflag: &[u8] = b"-d";
    let _ = l.register(word_cell(
        13,
        OP_PLAN,
        parse_args(&[dflag]).map(|p| match p.given {
            Some(v) => v,
            None => 0,
        }),
    ));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：已知时刻的七元组。
    let f = match fields(1_700_000_000, 0) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = f.year == 2023
        && f.month == 11
        && f.day == 14
        && f.hh == 22
        && f.mm == 13
        && f.ss == 20
        && f.wday == 2
        && f.yday == 318
        && f.stamp == 1_700_000_000;

    // 二路：格式展开逐字节。
    let mut b1 = [0u8; 32];
    let mut b2 = [0u8; 8];
    let mut b3 = [0u8; 8];
    let mut b4 = [0u8; 16];
    let r1 = render(b"%F %T", &f, &mut b1);
    let r2 = render(b"%a", &f, &mut b2);
    let r3 = render(b"%%", &f, &mut b3);
    let r4 = render(b"%s", &f, &mut b4);
    out[2] = r1 == Ok(19)
        && b1.get(..19) == Some(&b"2023-11-14 22:13:20"[..])
        && r2 == Ok(3)
        && b2.get(..3) == Some(&b"Tue"[..])
        && r3 == Ok(1)
        && b3.get(..1) == Some(&b"%"[..])
        && r4 == Ok(10)
        && b4.get(..10) == Some(&b"1700000000"[..]);

    // 三路：各类拒绝各自可辨。
    let dflag: &[u8] = b"-d";
    let uflag: &[u8] = b"-u";
    let xflag: &[u8] = b"-x";
    let mut tiny = [0u8; 4];
    out[3] = fmt_len(b"%Q", &f) == Err(DT_EBADFMT)
        && fmt_len(b"abc%", &f) == Err(DT_ETRUNC)
        && parse_date(b"2023-02-30") == Err(DT_EBADDATE)
        && matches!(fields(0x8000_0000, 0), Err(DT_ERANGE))
        && render(b"%F %T", &f, &mut tiny) == Err(DT_EBUF)
        && parse_args(&[dflag]) == Err(DT_EOPERAND)
        && parse_args(&[xflag]) == Err(DT_EBADOPT)
        && parse_args(&[uflag, uflag]) == Err(DT_EDUP);

    out
}

/// 十三项判据。
pub fn run_unx11507_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11507.date_fmt_opt");
    let l = build_ledger();

    s.add_dyn(
        "DT01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DT02-纪元零点为周四",
        {
            match fields(0, 0) {
                Ok(f) => {
                    f.year == 1970
                        && f.month == 1
                        && f.day == 1
                        && f.hh == 0
                        && f.mm == 0
                        && f.ss == 0
                        && f.wday == 4
                        && f.yday == 1
                        && l.sample(0).map(|x| x.value) == Some(1)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "DT03-已知时刻换算",
        {
            match fields(1_700_000_000, 0) {
                Ok(f) => {
                    f.year == 2023
                        && f.month == 11
                        && f.day == 14
                        && f.hh == 22
                        && f.mm == 13
                        && f.ss == 20
                        && f.wday == 2
                        && l.sample(1).map(|x| x.value) == Some(318)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "DT04-闰年二月边界",
        is_leap(2000)
            && is_leap(2024)
            && !is_leap(2023)
            && days_in_month(2000, 2) == 29
            && days_in_month(2023, 2) == 28
            && parse_date(b"2000-02-29") == Ok(951_782_400)
            && parse_date(b"2024-02-29") == Ok(1_709_164_800)
            && parse_date(b"2023-02-29") == Err(DT_EBADDATE),
        "static",
    );

    s.add(
        "DT05-年内日序",
        {
            let a = match fields(0, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match fields(1_700_000_000, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match fields(1_704_067_199, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.yday == 1 && b.yday == 318 && c.yday == 365 && day_of_year(2024, 12, 31) == 366
        },
        "static",
    );

    s.add(
        "DT06-星期与月名",
        {
            let mut b1 = [0u8; 8];
            let mut b2 = [0u8; 8];
            let z = match fields(0, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let n = match fields(1_700_000_000, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            render(b"%a", &z, &mut b1) == Ok(3)
                && b1.get(..3) == Some(&b"Thu"[..])
                && render(b"%b", &n, &mut b2) == Ok(3)
                && b2.get(..3) == Some(&b"Nov"[..])
                && weekday_name(2) == b"Tue"
                && month_name(1) == b"Jan"
        },
        "static",
    );

    s.add(
        "DT07-纪元秒与百分号",
        {
            let n = match fields(1_700_000_000, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut b1 = [0u8; 16];
            let mut b2 = [0u8; 4];
            render(b"%s", &n, &mut b1) == Ok(10)
                && b1.get(..10) == Some(&b"1700000000"[..])
                && render(b"%%", &n, &mut b2) == Ok(1)
                && b2.get(..1) == Some(&b"%"[..])
                && fmt_len(b"%Y-%m-%d", &n) == Ok(10)
                && fmt_len(b"%F %T", &n) == Ok(19)
        },
        "static",
    );

    s.add(
        "DT08-d串解析",
        {
            let dflag: &[u8] = b"-d";
            let uflag: &[u8] = b"-u";
            parse_args(&[dflag, b"@0"]) == Ok(Plan { utc: false, given: Some(0) })
                && parse_args(&[uflag, dflag, b"2023-11-14 22:13:20"])
                    == Ok(Plan { utc: true, given: Some(1_700_000_000) })
                && parse_args(&[dflag, b"1970-01-01T00:00:00"])
                    == Ok(Plan { utc: false, given: Some(0) })
                && parse_date(b"@1000000000") == Ok(1_000_000_000)
                && l.sample(7).map(|x| x.value) == Some(1_000_000_000)
        },
        "static",
    );

    s.add(
        "DT09-偏移与年限",
        {
            match fields(1_700_000_000, 3600) {
                Ok(f) => {
                    f.hh == 23
                        && f.stamp == 1_700_003_600
                        && fields(0, -1).is_err()
                        && fields(0x8000_0000, 0).is_err()
                        && fields(0, ZONE_MAX + 1).is_err()
                        && fields(0, -ZONE_MAX - 1).is_err()
                        && parse_date(b"1969-12-31") == Err(DT_ERANGE)
                        && parse_date(b"2039-01-01") == Err(DT_ERANGE)
                        && l.sample(11).map(|x| x.code) == Some(DT_ERANGE)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "DT10-未知转换符",
        {
            let n = match fields(0, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            fmt_len(b"%Q", &n) == Err(DT_EBADFMT)
                && fmt_len(b"%Y%q", &n) == Err(DT_EBADFMT)
                && fmt_len(b"%", &n) == Err(DT_ETRUNC)
                && fmt_len(b"abc%", &n) == Err(DT_ETRUNC)
                && l.sample(8).map(|x| x.code) == Some(DT_EBADFMT)
        },
        "static",
    );

    s.add(
        "DT11-日期串拒绝",
        parse_date(b"2023-2-30") == Err(DT_EBADDATE)
            && parse_date(b"2023-13-01") == Err(DT_EBADDATE)
            && parse_date(b"2023-00-10") == Err(DT_EBADDATE)
            && parse_date(b"2023-01-00") == Err(DT_EBADDATE)
            && parse_date(b"2023-01-01 24:00:00") == Err(DT_EBADDATE)
            && parse_date(b"2023-01-01 12:60:00") == Err(DT_EBADDATE)
            && parse_date(b"2023-01-01x12:00:00") == Err(DT_EBADDATE)
            && parse_date(b"") == Err(DT_EBADDATE)
            && l.sample(10).map(|x| x.code) == Some(DT_EBADDATE),
        "static",
    );

    s.add(
        "DT12-缓冲边界",
        {
            let n = match fields(1_700_000_000, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut exact = [0u8; 19];
            let mut short = [0u8; 18];
            render(b"%F %T", &n, &mut exact) == Ok(19)
                && exact.get(..19) == Some(&b"2023-11-14 22:13:20"[..])
                && render(b"%F %T", &n, &mut short) == Err(DT_EBUF)
                && l.sample(12).map(|x| x.code) == Some(DT_EBUF)
        },
        "static",
    );

    s.add(
        "DT13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(epoch: u32) -> Fields {
        match fields(epoch, 0) {
            Ok(v) => v,
            Err(_) => panic!("fields"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11507_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn epoch_zero() {
        let z = at(0);
        assert_eq!(z.year, 1970);
        assert_eq!(z.month, 1);
        assert_eq!(z.day, 1);
        assert_eq!(z.wday, 4);
        assert_eq!(z.yday, 1);
        assert_eq!(z.stamp, 0);
    }

    #[test]
    fn known_instant() {
        let n = at(1_700_000_000);
        assert_eq!((n.year, n.month, n.day), (2023, 11, 14));
        assert_eq!((n.hh, n.mm, n.ss), (22, 13, 20));
        assert_eq!(n.wday, 2);
        assert_eq!(n.yday, 318);
    }

    #[test]
    fn format_specs() {
        let n = at(1_700_000_000);
        let mut b = [0u8; 32];
        assert_eq!(render(b"%F %T", &n, &mut b), Ok(19));
        assert_eq!(&b[..19], b"2023-11-14 22:13:20");
        assert_eq!(render(b"%Y/%m/%d", &n, &mut b), Ok(10));
        assert_eq!(&b[..10], b"2023/11/14");
        assert_eq!(render(b"%a %b %j", &n, &mut b), Ok(11));
        assert_eq!(&b[..11], b"Tue Nov 318");
    }

    #[test]
    fn format_gates() {
        let z = at(0);
        let mut b = [0u8; 4];
        assert_eq!(fmt_len(b"%Q", &z), Err(DT_EBADFMT));
        assert_eq!(fmt_len(b"%", &z), Err(DT_ETRUNC));
        assert_eq!(fmt_len(b"abc%", &z), Err(DT_ETRUNC));
        assert_eq!(render(b"%F", &z, &mut b), Err(DT_EBUF));
    }

    #[test]
    fn date_parsing() {
        assert_eq!(parse_date(b"@0"), Ok(0));
        assert_eq!(parse_date(b"1970-01-01"), Ok(0));
        assert_eq!(parse_date(b"1970-01-02 00:00:01"), Ok(86_401));
        assert_eq!(parse_date(b"2000-02-29"), Ok(951_782_400));
        assert_eq!(parse_date(b"2023-02-29"), Err(DT_EBADDATE));
        assert_eq!(parse_date(b"2023-02-30"), Err(DT_EBADDATE));
        assert_eq!(parse_date(b"2039-01-01"), Err(DT_ERANGE));
        assert_eq!(parse_date(b"@2147483648"), Err(DT_ERANGE));
    }

    #[test]
    fn option_plan() {
        let d: &[u8] = b"-d";
        let u: &[u8] = b"-u";
        let x: &[u8] = b"-x";
        assert_eq!(parse_args(&[]), Ok(Plan { utc: false, given: None }));
        assert_eq!(parse_args(&[u]), Ok(Plan { utc: true, given: None }));
        assert_eq!(
            parse_args(&[d, b"2023-11-14 22:13:20"]),
            Ok(Plan { utc: false, given: Some(1_700_000_000) })
        );
        assert_eq!(parse_args(&[d]), Err(DT_EOPERAND));
        assert_eq!(parse_args(&[x]), Err(DT_EBADOPT));
        assert_eq!(parse_args(&[u, u]), Err(DT_EDUP));
        assert_eq!(parse_args(&[d, b"@0", d, b"@1"]), Err(DT_EDUP));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
