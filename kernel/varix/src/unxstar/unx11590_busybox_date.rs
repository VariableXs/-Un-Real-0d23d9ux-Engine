//! UNX-F11590 busybox date 格式符子集判据
//!
//! busybox 的 `date` 只实现 strftime 的一个**子集**，且子集边界随配置漂移。
//! 本模块把这个子集固化成十条格式符的基线表，并用纯整数算术实现从 epoch 到
//! 民用时间的双向换算——不调用 libc，也不依赖任何时区数据库：
//!
//! | 格式符 | 含义 | 宽度 | 填充 |
//! |--------|------|------|------|
//! | `%Y` | 年 | 4 | `0` |
//! | `%m` | 月 | 2 | `0` |
//! | `%d` | 日 | 2 | `0` |
//! | `%H` | 时 | 2 | `0` |
//! | `%M` | 分 | 2 | `0` |
//! | `%S` | 秒 | 2 | `0` |
//! | `%j` | 年内日序 | 3 | `0` |
//! | `%s` | epoch 秒 | 10 | 不填充 |
//! | `%Z` | 时区名 | 3 | 不填充 |
//! | `%a` | 星期缩写 | 3 | 不填充 |
//!
//! `%%` 是唯一被识别的转义；其余 `%` 后跟未知字母一律拒绝（`ESPEC`），
//! 末尾孤立的 `%` 是格式串截断（`EINPUT`）。`%s` 的宽度是**上限**而不是
//! 最小宽度：epoch 0 渲染成 `0` 而不是十个空格加零，这是 %s 与定宽数字字段
//! 的关键区别，用 `pad == 0` 在基线表里显式编码。
//!
//! 民用时间换算采用 Howard Hinnant 的 `days_from_civil` / `civil_from_days`
//! 算法：以 1970-01-01 为第 0 天，先用“纪元（400 年）= 146097 天”把年份折成
//! 纪元内偏移，再用 153 天的五个月周期把月序折成日序。两条公式互逆，本模块
//! 对 1970-01-01、2000-03-01、2023-11-14、2024-02-29 四个点做双向核对。
//!
//! 星期以星期日为 0：`(days + 4) mod 7`，因为 1970-01-01 是星期四。年内日序
//! 由“当天减当年 1 月 1 日加一”得到，闰年由 `%4/%100/%400` 三规则判定。
//!
//! 拒绝面：
//! - 年不在 1970..=9999（`ERANGE`），epoch 为负或超过 9999-12-31（`ERANGE`）；
//! - 月不在 1..=12、日不在当月天数内（`EMON`）；时/分/秒越界（`EHOUR`，
//!   秒允许 60 表示闰秒）；
//! - 时区号未知（`EZONE`）；输出超过容量（`ELEN`）；数字超过字段宽度
//!   （`EWIDTH`）；格式串为空（`EINPUT`）。
//!
//! 闸门次序：`render_at`：空格式 → 时区 → 逐字节扫描（`%` 取后继 → 转义 →
//! 格式符 → 数值/字符串 → 宽度 → 容量）；`from_epoch`：下界 → 上界 → 日分解 →
//! 民用换算；`from_civil`：年 → 月 → 日 → 时分秒 → epoch 溢出。
//!
//! 账本十四格：八格成功（epoch 0 的年与星期、2023-11-14 的日序、两种渲染长度、
//! 天数换算、星期缩写长度、`%j` 宽度），六格失败（未知格式符、截断格式串、
//! 2 月 30 日、负 epoch、未知时区、容量不足）。冻结票号折叠十四格与拒绝计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa30;

/// 未知或不支持的格式符。
pub const DT_ESPEC: u32 = SEG_BASE + 0x01;
/// epoch 或年份越界。
pub const DT_ERANGE: u32 = SEG_BASE + 0x02;
/// 月或日越界。
pub const DT_EMON: u32 = SEG_BASE + 0x03;
/// 时/分/秒越界。
pub const DT_EHOUR: u32 = SEG_BASE + 0x04;
/// 数字超过字段宽度。
pub const DT_EWIDTH: u32 = SEG_BASE + 0x05;
/// 时区号未知。
pub const DT_EZONE: u32 = SEG_BASE + 0x06;
/// 格式串为空或截断。
pub const DT_EINPUT: u32 = SEG_BASE + 0x07;
/// 渲染输出超过容量。
pub const DT_ELEN: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const DT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const DT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const DT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const DT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 基线格式符条数。
pub const SPECS: usize = 10;

/// 时区：UTC（模型内建）。
pub const ZONE_UTC: u8 = 0;
/// 时区：本地占位（模型内固定为 LOC）。
pub const ZONE_LOCAL: u8 = 1;

/// 一天秒数。
pub const DAY_SECS: i64 = 86400;
/// epoch 上界：9999-12-31T23:59:59。
pub const MAX_EPOCH: i64 = 253_402_300_799;

/// 格式符字母表。
pub const SPEC_LETTERS: [u8; SPECS] =
    [b'Y', b'm', b'd', b'H', b'M', b'S', b'j', b's', b'Z', b'a'];
/// 字段宽度表。
pub const SPEC_WIDTH: [u8; SPECS] = [4, 2, 2, 2, 2, 2, 3, 10, 3, 3];
/// 填充字符表（0 表示不填充，仅 `%s` 使用）。
pub const SPEC_PAD: [u8; SPECS] = [b'0', b'0', b'0', b'0', b'0', b'0', b'0', 0, 0, 0];

/// 星期缩写表，索引 0 = 星期日。
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// 操作码：渲染。
pub const OP_RENDER: u8 = 1;
/// 操作码：民用天数换算。
pub const OP_CIVIL: u8 = 2;
/// 操作码：epoch 分解。
pub const OP_BREAK: u8 = 3;
/// 操作码：格式符宽度。
pub const OP_SPEC: u8 = 4;
/// 操作码：年月日构造。
pub const OP_FROM: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（每字节一次乘法，共八次）。
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

/// 闰年判定：四整除、百不整除、四百整除。
pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 当月天数；月号越界拒绝。
pub fn days_in_month(y: i64, m: u32) -> Result<u32, u32> {
    let base: u32 = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => return Err(DT_EMON),
    };
    Ok(base)
}

/// 民用日期 → 自 1970-01-01 起的天数（Hinnant 算法）。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> Result<i64, u32> {
    if !(1970..=9999).contains(&y) {
        return Err(DT_ERANGE);
    }
    if m < 1 || m > 12 {
        return Err(DT_EMON);
    }
    let dim = days_in_month(y, m)?;
    if d < 1 || d > dim {
        return Err(DT_EMON);
    }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era.checked_mul(146_097)
        .and_then(|v| v.checked_add(doe))
        .and_then(|v| v.checked_sub(719_468))
        .ok_or(DT_ERANGE)
}

/// 自 1970-01-01 起的天数 → 民用日期（Hinnant 算法）。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// 分解后的民用时间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tm {
    /// 年。
    pub year: i64,
    /// 月。
    pub mon: u32,
    /// 日。
    pub day: u32,
    /// 时。
    pub hour: u32,
    /// 分。
    pub min: u32,
    /// 秒。
    pub sec: u32,
    /// 年内日序（1 起）。
    pub yday: u32,
    /// 星期（0 = 星期日）。
    pub wday: u32,
    /// epoch 秒。
    pub epoch: i64,
}

impl Tm {
    /// 空时间（1970-01-01T00:00:00）。
    pub const fn empty() -> Tm {
        Tm { year: 1970, mon: 1, day: 1, hour: 0, min: 0, sec: 0, yday: 1, wday: 4, epoch: 0 }
    }
}

/// epoch 秒 → 民用时间。
pub fn from_epoch(epoch: i64) -> Result<Tm, u32> {
    if epoch < 0 {
        return Err(DT_ERANGE);
    }
    if epoch > MAX_EPOCH {
        return Err(DT_ERANGE);
    }
    let days = epoch.div_euclid(DAY_SECS);
    let secs = epoch.rem_euclid(DAY_SECS);
    let (y, m, d) = civil_from_days(days);
    let wday = (days + 4).rem_euclid(7) as u32;
    let jan1 = days_from_civil(y, 1, 1)?;
    let yday = days.saturating_sub(jan1).saturating_add(1) as u32;
    Ok(Tm {
        year: y,
        mon: m,
        day: d,
        hour: (secs / 3600) as u32,
        min: ((secs % 3600) / 60) as u32,
        sec: (secs % 60) as u32,
        yday,
        wday,
        epoch,
    })
}

/// 民用时间 → epoch 秒，逐字段校验。
pub fn from_civil(y: i64, m: u32, d: u32, h: u32, mi: u32, s: u32) -> Result<Tm, u32> {
    if !(1970..=9999).contains(&y) {
        return Err(DT_ERANGE);
    }
    if m < 1 || m > 12 {
        return Err(DT_EMON);
    }
    let dim = days_in_month(y, m)?;
    if d < 1 || d > dim {
        return Err(DT_EMON);
    }
    if h > 23 || mi > 59 || s > 60 {
        return Err(DT_EHOUR);
    }
    let days = days_from_civil(y, m, d)?;
    let tod = (h as i64)
        .checked_mul(3600)
        .and_then(|v| v.checked_add((mi as i64) * 60))
        .and_then(|v| v.checked_add(s as i64))
        .ok_or(DT_EHOUR)?;
    let epoch = days.checked_mul(DAY_SECS).and_then(|v| v.checked_add(tod)).ok_or(DT_ERANGE)?;
    let jan1 = days_from_civil(y, 1, 1)?;
    Ok(Tm {
        year: y,
        mon: m,
        day: d,
        hour: h,
        min: mi,
        sec: s,
        yday: days.saturating_sub(jan1).saturating_add(1) as u32,
        wday: (days + 4).rem_euclid(7) as u32,
        epoch,
    })
}

/// 格式符字母 → 下标。
pub fn spec_index(letter: u8) -> Result<u8, u32> {
    let mut i = 0usize;
    while i < SPECS {
        match SPEC_LETTERS.get(i) {
            Some(v) => {
                if *v == letter {
                    return Ok(i as u8);
                }
            }
            None => return Err(DT_ESPEC),
        }
        i += 1;
    }
    Err(DT_ESPEC)
}

/// 格式符宽度。
pub fn spec_width(letter: u8) -> Result<u8, u32> {
    let idx = spec_index(letter)?;
    match SPEC_WIDTH.get(idx as usize) {
        Some(v) => Ok(*v),
        None => Err(DT_ESPEC),
    }
}

/// 格式符填充字符（0 表示不填充）。
pub fn spec_pad(letter: u8) -> Result<u8, u32> {
    let idx = spec_index(letter)?;
    match SPEC_PAD.get(idx as usize) {
        Some(v) => Ok(*v),
        None => Err(DT_ESPEC),
    }
}

/// 数值型格式符取值：年/月/日/时/分/秒/日序/epoch。
pub fn numeric_spec(tm: Tm, idx: u8) -> Result<u32, u32> {
    match idx {
        0 => Ok(tm.year as u32),
        1 => Ok(tm.mon),
        2 => Ok(tm.day),
        3 => Ok(tm.hour),
        4 => Ok(tm.min),
        5 => Ok(tm.sec),
        6 => Ok(tm.yday),
        7 => {
            if tm.epoch < 0 || tm.epoch > u32::MAX as i64 {
                Err(DT_ERANGE)
            } else {
                Ok(tm.epoch as u32)
            }
        }
        _ => Err(DT_ESPEC),
    }
}

/// 星期缩写。
pub fn weekday_name(wday: u32) -> Result<&'static str, u32> {
    match WEEKDAYS.get(wday as usize) {
        Some(v) => Ok(v),
        None => Err(DT_ERANGE),
    }
}

/// 时区名。
pub fn zone_name(zone: u8) -> Result<&'static str, u32> {
    match zone {
        ZONE_UTC => Ok("UTC"),
        ZONE_LOCAL => Ok("LOC"),
        _ => Err(DT_EZONE),
    }
}

/// 十进制追加并按宽度填充。
fn push_pad(out: &mut String, value: u32, width: u8, pad: u8) -> Result<(), u32> {
    if width == 0 || width > 10 {
        return Err(DT_EWIDTH);
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = value;
    if x == 0 {
        if let Some(slot) = buf.get_mut(0) {
            *slot = b'0';
        }
        n = 1;
    } else {
        while x > 0 {
            let digit = (x % 10) as u8;
            if let Some(slot) = buf.get_mut(n) {
                *slot = b'0' + digit;
            }
            x /= 10;
            n += 1;
        }
    }
    if n > width as usize {
        return Err(DT_EWIDTH);
    }
    if pad != 0 {
        let mut k = n;
        while k < width as usize {
            out.push(pad as char);
            k += 1;
        }
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        match buf.get(i) {
            Some(c) => out.push(*c as char),
            None => return Err(DT_EWIDTH),
        }
    }
    Ok(())
}

/// 在给定分解时间上渲染格式串（含容量上限）。
pub fn render_at(tm: Tm, fmt: &str, zone: u8, cap: usize) -> Result<String, u32> {
    if fmt.is_empty() {
        return Err(DT_EINPUT);
    }
    let _ = zone_name(zone)?;
    let b = fmt.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        if c != b'%' {
            out.push(c as char);
            i += 1;
        } else {
            let n = match b.get(i + 1) {
                Some(v) => *v,
                None => return Err(DT_EINPUT),
            };
            if n == b'%' {
                out.push('%');
                i += 2;
            } else {
                let idx = spec_index(n)?;
                if idx == 8 {
                    out.push_str(zone_name(zone)?);
                } else if idx == 9 {
                    out.push_str(weekday_name(tm.wday)?);
                } else {
                    let v = numeric_spec(tm, idx)?;
                    let w = match SPEC_WIDTH.get(idx as usize) {
                        Some(x) => *x,
                        None => return Err(DT_ESPEC),
                    };
                    let p = match SPEC_PAD.get(idx as usize) {
                        Some(x) => *x,
                        None => return Err(DT_ESPEC),
                    };
                    push_pad(&mut out, v, w, p)?;
                }
                i += 2;
            }
        }
        if out.len() > cap {
            return Err(DT_ELEN);
        }
    }
    Ok(out)
}

/// epoch 秒 + 格式串 → 文本。
pub fn render(epoch: i64, fmt: &str, zone: u8) -> Result<String, u32> {
    let tm = from_epoch(epoch)?;
    render_at(tm, fmt, zone, 128)
}

/// 统计格式串里的格式符个数（校验未知符与截断）。
pub fn count_specs(fmt: &str) -> Result<u32, u32> {
    if fmt.is_empty() {
        return Err(DT_EINPUT);
    }
    let b = fmt.as_bytes();
    let mut i = 0usize;
    let mut n = 0u32;
    while i < b.len() {
        match b.get(i) {
            Some(b'%') => {
                let next = match b.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(DT_EINPUT),
                };
                if next != b'%' {
                    let _ = spec_index(next)?;
                    n += 1;
                }
                i += 2;
            }
            Some(_) => i += 1,
            None => break,
        }
    }
    Ok(n)
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
        const VALS: [u32; 8] = [1970, 4, 318, 10, 8, 19675, 3, 3];
        const CODES: [u32; 6] =
            [DT_ESPEC, DT_EINPUT, DT_EMON, DT_ERANGE, DT_EZONE, DT_ELEN];
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

    /// 票号。
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

    /// 冻结票号。
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

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(
        0,
        OP_BREAK,
        from_epoch(0).map(|t| t.year as u32),
    ));
    let _ = l.register(word_cell(1, OP_BREAK, from_epoch(0).map(|t| t.wday)));
    let _ = l.register(word_cell(
        2,
        OP_BREAK,
        from_epoch(1_700_000_000).map(|t| t.yday),
    ));
    let _ = l.register(word_cell(
        3,
        OP_RENDER,
        render(1_700_000_000, "%Y-%m-%d", ZONE_UTC).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        4,
        OP_RENDER,
        render(1_700_000_000, "%H:%M:%S", ZONE_UTC).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(5, OP_CIVIL, days_from_civil(2023, 11, 14).map(|v| v as u32)));
    let _ = l.register(word_cell(
        6,
        OP_RENDER,
        render(1_700_000_000, "%a", ZONE_UTC).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(7, OP_SPEC, spec_width(b'j').map(|v| v as u32)));

    let _ = l.register(word_cell(
        8,
        OP_RENDER,
        render(0, "%Q", ZONE_UTC).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        9,
        OP_RENDER,
        render(0, "%", ZONE_UTC).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(10, OP_CIVIL, days_from_civil(2023, 2, 30).map(|v| v as u32)));
    let _ = l.register(word_cell(11, OP_BREAK, from_epoch(-1).map(|t| t.year as u32)));
    let _ = l.register(word_cell(
        12,
        OP_RENDER,
        render_at(Tm::empty(), "%Y", 9, 64).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        13,
        OP_RENDER,
        render_at(Tm::empty(), "%Y-%m-%d", ZONE_UTC, 5).map(|s| s.len() as u32),
    ));

    l
}

/// 四路注入：基线表 / 闰月 / 双向换算 / 渲染与拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut widths_ok = true;
    let mut i = 0usize;
    while i < SPECS {
        match (SPEC_LETTERS.get(i), SPEC_WIDTH.get(i)) {
            (Some(_), Some(_)) => {}
            _ => widths_ok = false,
        }
        i += 1;
    }
    out[0] = SPECS == 10
        && widths_ok
        && spec_index(b'Y') == Ok(0)
        && spec_index(b'a') == Ok(9)
        && spec_index(b'q') == Err(DT_ESPEC)
        && spec_width(b'j') == Ok(3)
        && spec_width(b's') == Ok(10)
        && spec_pad(b's') == Ok(0)
        && spec_pad(b'd') == Ok(b'0');

    out[1] = is_leap(2024)
        && is_leap(2000)
        && !is_leap(1900)
        && !is_leap(2023)
        && days_in_month(2024, 2) == Ok(29)
        && days_in_month(2023, 2) == Ok(28)
        && days_in_month(2023, 4) == Ok(30)
        && days_in_month(2023, 13) == Err(DT_EMON)
        && days_in_month(2023, 0) == Err(DT_EMON);

    let d0 = days_from_civil(1970, 1, 1) == Ok(0);
    let d1 = days_from_civil(2000, 3, 1) == Ok(11017);
    let d2 = days_from_civil(2023, 11, 14) == Ok(19675);
    let d3 = days_from_civil(2024, 2, 29) == Ok(19782);
    let c0 = civil_from_days(0) == (1970, 1, 1);
    let c1 = civil_from_days(19675) == (2023, 11, 14);
    let c2 = civil_from_days(11017) == (2000, 3, 1);
    let bad = days_from_civil(2023, 2, 30) == Err(DT_EMON)
        && days_from_civil(1969, 12, 31) == Err(DT_ERANGE)
        && days_from_civil(2023, 13, 1) == Err(DT_EMON);
    out[2] = d0 && d1 && d2 && d3 && c0 && c1 && c2 && bad;

    let tm = match from_epoch(1_700_000_000) {
        Ok(v) => v,
        Err(_) => Tm::empty(),
    };
    let iso = render(1_700_000_000, "%Y-%m-%dT%H:%M:%SZ", ZONE_UTC);
    let iso_ok = match iso {
        Ok(ref s) => s.as_str() == "2023-11-14T22:13:20Z",
        Err(_) => false,
    };
    let date_ok = match render(0, "%Y-%m-%d", ZONE_UTC) {
        Ok(ref s) => s.as_str() == "1970-01-01",
        Err(_) => false,
    };
    let pct_ok = match render(0, "100%%", ZONE_UTC) {
        Ok(ref s) => s.as_str() == "100%",
        Err(_) => false,
    };
    let pad_ok = match render(1_700_000_046, "%H:%M:%S", ZONE_UTC) {
        Ok(ref s) => s.as_str() == "22:14:06",
        Err(_) => false,
    };
    out[3] = tm == Tm {
        year: 2023,
        mon: 11,
        day: 14,
        hour: 22,
        min: 13,
        sec: 20,
        yday: 318,
        wday: 2,
        epoch: 1_700_000_000,
    } && iso_ok
        && date_ok
        && pct_ok
        && pad_ok
        && count_specs("%Y-%m-%d") == Ok(3)
        && count_specs("100%%") == Ok(0)
        && render(0, "%Q", ZONE_UTC) == Err(DT_ESPEC)
        && render(0, "%", ZONE_UTC) == Err(DT_EINPUT)
        && render(0, "", ZONE_UTC) == Err(DT_EINPUT)
        && render_at(Tm::empty(), "%Y-%m-%d", ZONE_UTC, 5) == Err(DT_ELEN)
        && render_at(Tm::empty(), "%Y", 9, 64) == Err(DT_EZONE);

    out
}

/// 十三项判据。
pub fn run_unx11590_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11590.busybox_date");
    let l = build_ledger();

    s.add(
        "DT1-十条格式符基线",
        SPECS == 10
            && SPEC_LETTERS.len() == SPECS
            && SPEC_WIDTH == [4, 2, 2, 2, 2, 2, 3, 10, 3, 3]
            && spec_index(b'Y') == Ok(0)
            && spec_index(b's') == Ok(7)
            && spec_index(b'q') == Err(DT_ESPEC)
            && spec_width(b's') == Ok(10)
            && spec_pad(b'd') == Ok(b'0'),
        "static",
    );

    s.add(
        "DT2-闰年三规则",
        is_leap(2024)
            && is_leap(2000)
            && !is_leap(1900)
            && !is_leap(2023)
            && !is_leap(2100)
            && is_leap(2400),
        "static",
    );

    s.add(
        "DT3-每月天数",
        days_in_month(2024, 2) == Ok(29)
            && days_in_month(2023, 2) == Ok(28)
            && days_in_month(2023, 1) == Ok(31)
            && days_in_month(2023, 4) == Ok(30)
            && days_in_month(2023, 12) == Ok(31)
            && days_in_month(2023, 0) == Err(DT_EMON)
            && days_in_month(2023, 13) == Err(DT_EMON),
        "static",
    );

    s.add(
        "DT4-民用日换算正向",
        days_from_civil(1970, 1, 1) == Ok(0)
            && days_from_civil(1970, 1, 2) == Ok(1)
            && days_from_civil(2000, 3, 1) == Ok(11017)
            && days_from_civil(2023, 11, 14) == Ok(19675)
            && days_from_civil(2024, 2, 29) == Ok(19782),
        "static",
    );

    s.add(
        "DT5-民用日换算逆向",
        civil_from_days(0) == (1970, 1, 1)
            && civil_from_days(1) == (1970, 1, 2)
            && civil_from_days(11017) == (2000, 3, 1)
            && civil_from_days(19675) == (2023, 11, 14)
            && civil_from_days(19782) == (2024, 2, 29),
        "static",
    );

    s.add(
        "DT6-epoch零分解",
        from_epoch(0) == Ok(Tm::empty())
            && from_epoch(0).map(|t| t.yday) == Ok(1)
            && from_epoch(0).map(|t| t.wday) == Ok(4)
            && from_epoch(86400).map(|t| t.day) == Ok(2)
            && from_epoch(-1) == Err(DT_ERANGE),
        "static",
    );

    s.add(
        "DT7-epoch已知点分解",
        from_epoch(1_700_000_000)
            == Ok(Tm {
                year: 2023,
                mon: 11,
                day: 14,
                hour: 22,
                min: 13,
                sec: 20,
                yday: 318,
                wday: 2,
                epoch: 1_700_000_000,
            }),
        "static",
    );

    s.add(
        "DT8-日期时间渲染",
        {
            let a = match render(1_700_000_000, "%Y-%m-%d", ZONE_UTC) {
                Ok(v) => v.as_str() == "2023-11-14",
                Err(_) => false,
            };
            let b = match render(1_700_000_000, "%H:%M:%S", ZONE_UTC) {
                Ok(v) => v.as_str() == "22:13:20",
                Err(_) => false,
            };
            let c = match render(0, "%Y-%m-%d %H:%M:%S", ZONE_UTC) {
                Ok(v) => v.as_str() == "1970-01-01 00:00:00",
                Err(_) => false,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "DT9-日序epoch星期时区",
        {
            let a = match render(1_700_000_000, "%j", ZONE_UTC) {
                Ok(v) => v.as_str() == "318",
                Err(_) => false,
            };
            let b = match render(1_700_000_000, "%s", ZONE_UTC) {
                Ok(v) => v.as_str() == "1700000000",
                Err(_) => false,
            };
            let c = match render(1_700_000_000, "%a", ZONE_UTC) {
                Ok(v) => v.as_str() == "Tue",
                Err(_) => false,
            };
            let d = match render(1_700_000_000, "%Z", ZONE_UTC) {
                Ok(v) => v.as_str() == "UTC",
                Err(_) => false,
            };
            a && b && c && d && weekday_name(0) == Ok("Sun") && weekday_name(7) == Err(DT_ERANGE)
        },
        "static",
    );

    s.add(
        "DT10-未知与截断拒绝",
        render(0, "%Q", ZONE_UTC) == Err(DT_ESPEC)
            && render(0, "%", ZONE_UTC) == Err(DT_EINPUT)
            && render(0, "", ZONE_UTC) == Err(DT_EINPUT)
            && count_specs("%Q") == Err(DT_ESPEC)
            && count_specs("%") == Err(DT_EINPUT)
            && count_specs("%Y%%") == Ok(1)
            && render_at(Tm::empty(), "%Y", 9, 64) == Err(DT_EZONE)
            && zone_name(ZONE_LOCAL) == Ok("LOC"),
        "static",
    );

    s.add(
        "DT11-宽度填充与容量",
        {
            let a = match render(0, "%H:%M:%S", ZONE_UTC) {
                Ok(v) => v.as_str() == "00:00:00",
                Err(_) => false,
            };
            let b = match render(1_700_000_046, "%H:%M:%S", ZONE_UTC) {
                Ok(v) => v.as_str() == "22:14:06",
                Err(_) => false,
            };
            let c = match render(0, "%s", ZONE_UTC) {
                Ok(v) => v.as_str() == "0",
                Err(_) => false,
            };
            let d = match render(0, "100%%", ZONE_UTC) {
                Ok(v) => v.as_str() == "100%",
                Err(_) => false,
            };
            a && b && c && d
                && render_at(Tm::empty(), "%Y-%m-%d", ZONE_UTC, 5) == Err(DT_ELEN)
                && render_at(Tm::empty(), "%Y-%m-%d", ZONE_UTC, 10).is_ok()
        },
        "static",
    );

    s.add(
        "DT12-年月日构造校验",
        from_civil(2024, 2, 29, 0, 0, 0).map(|t| t.yday) == Ok(60)
            && from_civil(2023, 2, 29, 0, 0, 0) == Err(DT_EMON)
            && from_civil(2023, 13, 1, 0, 0, 0) == Err(DT_EMON)
            && from_civil(2023, 1, 1, 24, 0, 0) == Err(DT_EHOUR)
            && from_civil(2023, 1, 1, 0, 60, 0) == Err(DT_EHOUR)
            && from_civil(1969, 12, 31, 0, 0, 0) == Err(DT_ERANGE)
            && from_civil(2023, 11, 14, 22, 13, 20).map(|t| t.epoch) == Ok(1_700_000_000),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let one = f.freeze();
    let two = f.freeze();
    let inj = inject_four();
    s.add(
        "DT13-冻结重放四注入",
        l.audit().is_ok()
            && one.is_ok()
            && one == rep
            && two.is_err()
            && f.is_frozen()
            && f.root() != 0
            && f.rejects() == 1
            && inj[0]
            && inj[1]
            && inj[2]
            && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(2).map(|s| s.value), Some(318));
        assert_eq!(l.sample(11).map(|s| s.code), Some(DT_ERANGE));
    }

    #[test]
    fn spec_table_shape() {
        assert_eq!(SPECS, 10);
        assert_eq!(SPEC_LETTERS.len(), 10);
        assert_eq!(spec_index(b'Y'), Ok(0));
        assert_eq!(spec_index(b'a'), Ok(9));
        assert_eq!(spec_index(b'z'), Err(DT_ESPEC));
        assert_eq!(spec_width(b'j'), Ok(3));
        assert_eq!(spec_width(b's'), Ok(10));
        assert_eq!(spec_pad(b's'), Ok(0));
        assert_eq!(spec_pad(b'm'), Ok(b'0'));
    }

    #[test]
    fn leap_rules() {
        assert!(is_leap(2024));
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(!is_leap(2100));
        assert!(is_leap(2400));
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), Ok(29));
        assert_eq!(days_in_month(2023, 2), Ok(28));
        assert_eq!(days_in_month(2023, 1), Ok(31));
        assert_eq!(days_in_month(2023, 11), Ok(30));
        assert_eq!(days_in_month(2023, 0), Err(DT_EMON));
        assert_eq!(days_in_month(2023, 13), Err(DT_EMON));
    }

    #[test]
    fn civil_forward() {
        assert_eq!(days_from_civil(1970, 1, 1), Ok(0));
        assert_eq!(days_from_civil(2000, 3, 1), Ok(11017));
        assert_eq!(days_from_civil(2023, 11, 14), Ok(19675));
        assert_eq!(days_from_civil(2024, 2, 29), Ok(19782));
        assert_eq!(days_from_civil(2023, 2, 30), Err(DT_EMON));
        assert_eq!(days_from_civil(1969, 1, 1), Err(DT_ERANGE));
    }

    #[test]
    fn civil_backward() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19675), (2023, 11, 14));
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        assert_eq!(civil_from_days(19782), (2024, 2, 29));
        let mut d = 0i64;
        while d < 2000 {
            let (y, m, dd) = civil_from_days(d);
            if let Ok(back) = days_from_civil(y, m, dd) {
                assert_eq!(back, d);
            } else {
                assert_eq!(d, -1);
            }
            d += 37;
        }
    }

    #[test]
    fn epoch_zero() {
        assert_eq!(from_epoch(0), Ok(Tm::empty()));
        assert_eq!(from_epoch(0).map(|t| t.wday), Ok(4));
        assert_eq!(from_epoch(86_400).map(|t| t.day), Ok(2));
        assert_eq!(from_epoch(-1), Err(DT_ERANGE));
        assert_eq!(from_epoch(MAX_EPOCH + 1), Err(DT_ERANGE));
    }

    #[test]
    fn epoch_known_point() {
        let tm = match from_epoch(1_700_000_000) {
            Ok(v) => v,
            Err(_) => Tm::empty(),
        };
        assert_eq!(tm.year, 2023);
        assert_eq!(tm.mon, 11);
        assert_eq!(tm.day, 14);
        assert_eq!(tm.hour, 22);
        assert_eq!(tm.min, 13);
        assert_eq!(tm.sec, 20);
        assert_eq!(tm.yday, 318);
        assert_eq!(tm.wday, 2);
    }

    #[test]
    fn render_date_and_time() {
        assert_eq!(render(0, "%Y-%m-%d", ZONE_UTC), Ok(String::from("1970-01-01")));
        assert_eq!(
            render(1_700_000_000, "%Y-%m-%dT%H:%M:%SZ", ZONE_UTC),
            Ok(String::from("2023-11-14T22:13:20Z"))
        );
        assert_eq!(render(1_700_000_046, "%H:%M:%S", ZONE_UTC), Ok(String::from("22:14:06")));
        assert_eq!(render(0, "%H:%M:%S", ZONE_UTC), Ok(String::from("00:00:00")));
    }

    #[test]
    fn render_number_specs() {
        assert_eq!(render(1_700_000_000, "%j", ZONE_UTC), Ok(String::from("318")));
        assert_eq!(render(1_700_000_000, "%s", ZONE_UTC), Ok(String::from("1700000000")));
        assert_eq!(render(0, "%s", ZONE_UTC), Ok(String::from("0")));
        assert_eq!(render(1_700_000_000, "%a %Z", ZONE_UTC), Ok(String::from("Tue UTC")));
    }

    #[test]
    fn render_escape_and_errors() {
        assert_eq!(render(0, "100%%", ZONE_UTC), Ok(String::from("100%")));
        assert_eq!(render(0, "%Q", ZONE_UTC), Err(DT_ESPEC));
        assert_eq!(render(0, "%", ZONE_UTC), Err(DT_EINPUT));
        assert_eq!(render(0, "", ZONE_UTC), Err(DT_EINPUT));
        assert_eq!(render_at(Tm::empty(), "%Y", 9, 64), Err(DT_EZONE));
        assert_eq!(render_at(Tm::empty(), "%Y-%m-%d", ZONE_UTC, 5), Err(DT_ELEN));
    }

    #[test]
    fn spec_counting() {
        assert_eq!(count_specs("%Y-%m-%d"), Ok(3));
        assert_eq!(count_specs("plain"), Ok(0));
        assert_eq!(count_specs("100%%"), Ok(0));
        assert_eq!(count_specs("%Y%%"), Ok(1));
        assert_eq!(count_specs("%Q"), Err(DT_ESPEC));
        assert_eq!(count_specs(""), Err(DT_EINPUT));
    }

    #[test]
    fn civil_construction() {
        assert_eq!(from_civil(2024, 2, 29, 0, 0, 0).map(|t| t.yday), Ok(60));
        assert_eq!(from_civil(2023, 2, 29, 0, 0, 0), Err(DT_EMON));
        assert_eq!(from_civil(2023, 1, 1, 24, 0, 0), Err(DT_EHOUR));
        assert_eq!(from_civil(2023, 1, 1, 0, 60, 0), Err(DT_EHOUR));
        assert_eq!(from_civil(2023, 1, 1, 0, 0, 61), Err(DT_EHOUR));
        assert_eq!(
            from_civil(2023, 11, 14, 22, 13, 20).map(|t| t.epoch),
            Ok(1_700_000_000)
        );
        assert_eq!(from_civil(2023, 11, 14, 22, 13, 20).map(|t| t.wday), Ok(2));
    }

    #[test]
    fn numeric_spec_fields() {
        let tm = match from_civil(2023, 11, 14, 22, 13, 20) {
            Ok(v) => v,
            Err(_) => Tm::empty(),
        };
        assert_eq!(numeric_spec(tm, 0), Ok(2023));
        assert_eq!(numeric_spec(tm, 6), Ok(318));
        assert_eq!(numeric_spec(tm, 7), Ok(1_700_000_000));
        assert_eq!(numeric_spec(tm, 8), Err(DT_ESPEC));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DT_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.root(), root);
    }

    #[test]
    fn ledger_slot_guard() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 4, op: OP_RENDER, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad), Err(DT_ESLOT));
        let good = Sample { slot: 0, op: OP_RENDER, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good), Err(DT_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11590_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
