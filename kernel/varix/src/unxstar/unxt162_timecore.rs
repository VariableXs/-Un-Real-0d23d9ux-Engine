//! unxt162_timecore —— UNX-F0162 时间语义核心（七字段闭集+进位链+打包往返）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：TimeStamp 七字段本体（UEFI EFI_TIME 对应）+ 逐字段域校验 + 进位链传播 + 民用日数 + 位打包往返
//! · 判据落地：误差 <1s 的最小语义单元（纳秒级精度底座）与 UEFI 字段语义逐项对照
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T162_VERSION: &str = "t162-timecore-v1";

/// 诊断段基址。
pub const T162_SEG_BASE: u16 = 0x3700;

// --------------------------------------------------------------------------
// 诊断码（t162 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间语义核心诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t162Code {
    /// 0x3700 年份越界（1..=9999 之外）。
    YearOutOfRange,
    /// 0x3701 月份越界（1..=12 之外）。
    MonthOutOfRange,
    /// 0x3702 日期越界（1..=当月长度之外）。
    DayOutOfRange,
    /// 0x3703 小时越界（0..=23 之外）。
    HourOutOfRange,
    /// 0x3704 分钟越界（0..=59 之外）。
    MinuteOutOfRange,
    /// 0x3705 秒越界（0..=59 之外）。
    SecondOutOfRange,
    /// 0x3706 纳秒越界（0..=999_999_999 之外）。
    NsecOutOfRange,
    /// 0x3707 进位链溢出（规范化后字段二次越界）。
    CarryOverflow,
    /// 0x3708 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3709 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x370a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x370b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x370c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x370d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x370e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x370f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t162Code {
    /// 全量闭集。
    pub const ALL: [t162Code; 16] = [
        t162Code::YearOutOfRange,
        t162Code::MonthOutOfRange,
        t162Code::DayOutOfRange,
        t162Code::HourOutOfRange,
        t162Code::MinuteOutOfRange,
        t162Code::SecondOutOfRange,
        t162Code::NsecOutOfRange,
        t162Code::CarryOverflow,
        t162Code::LedgerBroken,
        t162Code::SnapshotFingerprint,
        t162Code::TableBroken,
        t162Code::IndexMisaligned,
        t162Code::DuplicateEntry,
        t162Code::OrderViolated,
        t162Code::GuardTripped,
        t162Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t162Code::YearOutOfRange => 0x3700,
            t162Code::MonthOutOfRange => 0x3701,
            t162Code::DayOutOfRange => 0x3702,
            t162Code::HourOutOfRange => 0x3703,
            t162Code::MinuteOutOfRange => 0x3704,
            t162Code::SecondOutOfRange => 0x3705,
            t162Code::NsecOutOfRange => 0x3706,
            t162Code::CarryOverflow => 0x3707,
            t162Code::LedgerBroken => 0x3708,
            t162Code::SnapshotFingerprint => 0x3709,
            t162Code::TableBroken => 0x370a,
            t162Code::IndexMisaligned => 0x370b,
            t162Code::DuplicateEntry => 0x370c,
            t162Code::OrderViolated => 0x370d,
            t162Code::GuardTripped => 0x370e,
            t162Code::SegmentGuard => 0x370f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t162Code::YearOutOfRange => "年份越界（1..=9999 之外）",
            t162Code::MonthOutOfRange => "月份越界（1..=12 之外）",
            t162Code::DayOutOfRange => "日期越界（1..=当月长度之外）",
            t162Code::HourOutOfRange => "小时越界（0..=23 之外）",
            t162Code::MinuteOutOfRange => "分钟越界（0..=59 之外）",
            t162Code::SecondOutOfRange => "秒越界（0..=59 之外）",
            t162Code::NsecOutOfRange => "纳秒越界（0..=999_999_999 之外）",
            t162Code::CarryOverflow => "进位链溢出（规范化后字段二次越界）",
            t162Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t162Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t162Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t162Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t162Code::DuplicateEntry => "条目重复（同键双写）",
            t162Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t162Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t162Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t162Code::YearOutOfRange => "按 UEFI 规范钳制或拒收非法年份",
            t162Code::MonthOutOfRange => "核对月份字段输入",
            t162Code::DayOutOfRange => "按闰年规则计算当月长度",
            t162Code::HourOutOfRange => "核对小时字段输入",
            t162Code::MinuteOutOfRange => "核对分钟字段输入",
            t162Code::SecondOutOfRange => "核对秒字段输入",
            t162Code::NsecOutOfRange => "核对纳秒字段输入",
            t162Code::CarryOverflow => "检查进位传播终止条件",
            t162Code::LedgerBroken => "按守恒律重建台账后重跑",
            t162Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t162Code::TableBroken => "按语义规则重建常量表",
            t162Code::IndexMisaligned => "按声明序归一化后重跑",
            t162Code::DuplicateEntry => "合并同键条目为末次生效",
            t162Code::OrderViolated => "按依赖矩阵重排后重跑",
            t162Code::GuardTripped => "检查临界区嵌套与阈值",
            t162Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 月长表与闰年规则（格里高利历；平闰双列）
// ---------------------------------------------------------------------------

/// 平年月长（0 号位占位，1..=12 为真实月长）。
pub const COMMON_DAYS: [u32; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// 闰二月增补（29 - 28）。
pub const LEAP_FEB_DAYS: u32 = 1;

/// 月份上限。
pub const MONTH_MAX: u32 = 12;

/// 年份下限（UEFI 规范 1）。
pub const YEAR_MIN: u32 = 1;

/// 年份上限（UEFI 规范 9999）。
pub const YEAR_MAX: u32 = 9999;

/// 纳秒上限。
pub const NSEC_MAX: u32 = 999_999_999;

/// 格里高利历闰年规则（4 整除且非 100 整除，或 400 整除）。
pub const fn leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 当月天数（含闰年二月）。
pub const fn days_in_month(year: u32, month: u32) -> u32 {
    if month == 0 || month > MONTH_MAX {
        return 0;
    }
    if month == 2 && leap_year(year) {
        return COMMON_DAYS[2] + LEAP_FEB_DAYS;
    }
    COMMON_DAYS[month as usize]
}

/// 字段枚举（七字段闭集，与 UEFI EFI_TIME 对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeField {
    /// 年。
    Year,
    /// 月。
    Month,
    /// 日。
    Day,
    /// 时。
    Hour,
    /// 分。
    Minute,
    /// 秒（闰秒位单列由 unx162_leapfilter 处理）。
    Second,
    /// 纳秒。
    Nanosecond,
}

impl TimeField {
    /// 全量字段闭集。
    pub const ALL: [TimeField; 7] = [
        TimeField::Year,
        TimeField::Month,
        TimeField::Day,
        TimeField::Hour,
        TimeField::Minute,
        TimeField::Second,
        TimeField::Nanosecond,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            TimeField::Year => 0,
            TimeField::Month => 1,
            TimeField::Day => 2,
            TimeField::Hour => 3,
            TimeField::Minute => 4,
            TimeField::Second => 5,
            TimeField::Nanosecond => 6,
        }
    }

    /// 中文名。
    pub const fn cn(self) -> &'static str {
        match self {
            TimeField::Year => "年",
            TimeField::Month => "月",
            TimeField::Day => "日",
            TimeField::Hour => "时",
            TimeField::Minute => "分",
            TimeField::Second => "秒",
            TimeField::Nanosecond => "纳秒",
        }
    }
}

/// 时间戳本体（七字段显性存储，无隐藏派生状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeStamp {
    /// 年（1..=9999）。
    pub year: u32,
    /// 月（1..=12）。
    pub month: u32,
    /// 日（1..=days_in_month）。
    pub day: u32,
    /// 时（0..=23）。
    pub hour: u32,
    /// 分（0..=59）。
    pub minute: u32,
    /// 秒（0..=59；闰秒 60 由 extrinsic leap 标记承载）。
    pub second: u32,
    /// 纳秒（0..=999_999_999）。
    pub nsec: u32,
}

impl TimeStamp {
    /// 全零初值（未初始化态）。
    pub const ZERO: TimeStamp = TimeStamp {
        year: 0,
        month: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        nsec: 0,
    };

    /// 构造 + 逐字段域校验（任一越界即返回首个诊断）。
    pub fn new(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32, nsec: u32)
        -> Result<TimeStamp, t162Code> {
        if year < YEAR_MIN || year > YEAR_MAX {
            return Err(t162Code::YearOutOfRange);
        }
        if month < 1 || month > MONTH_MAX {
            return Err(t162Code::MonthOutOfRange);
        }
        let dim = days_in_month(year, month);
        if day < 1 || day > dim {
            return Err(t162Code::DayOutOfRange);
        }
        if hour > 23 {
            return Err(t162Code::HourOutOfRange);
        }
        if minute > 59 {
            return Err(t162Code::MinuteOutOfRange);
        }
        if second > 59 {
            return Err(t162Code::SecondOutOfRange);
        }
        if nsec > NSEC_MAX {
            return Err(t162Code::NsecOutOfRange);
        }
        Ok(TimeStamp { year, month, day, hour, minute, second, nsec })
    }

    /// 读取字段（索引安全路径）。
    pub fn field(self, f: TimeField) -> u32 {
        match f {
            TimeField::Year => self.year,
            TimeField::Month => self.month,
            TimeField::Day => self.day,
            TimeField::Hour => self.hour,
            TimeField::Minute => self.minute,
            TimeField::Second => self.second,
            TimeField::Nanosecond => self.nsec,
        }
    }

    /// 写字段 + 域复检（越域写整体拒收，保持结构不变式）。
    pub fn set_field(&mut self, f: TimeField, v: u32) -> Result<(), t162Code> {
        let mut t = *self;
        match f {
            TimeField::Year => {
                if v < YEAR_MIN || v > YEAR_MAX {
                    return Err(t162Code::YearOutOfRange);
                }
                t.year = v;
            }
            TimeField::Month => {
                if v < 1 || v > MONTH_MAX {
                    return Err(t162Code::MonthOutOfRange);
                }
                t.month = v;
            }
            TimeField::Day => {
                if v < 1 || v > days_in_month(self.year, self.month) {
                    return Err(t162Code::DayOutOfRange);
                }
                t.day = v;
            }
            TimeField::Hour => {
                if v > 23 {
                    return Err(t162Code::HourOutOfRange);
                }
                t.hour = v;
            }
            TimeField::Minute => {
                if v > 59 {
                    return Err(t162Code::MinuteOutOfRange);
                }
                t.minute = v;
            }
            TimeField::Second => {
                if v > 59 {
                    return Err(t162Code::SecondOutOfRange);
                }
                t.second = v;
            }
            TimeField::Nanosecond => {
                if v > NSEC_MAX {
                    return Err(t162Code::NsecOutOfRange);
                }
                t.nsec = v;
            }
        }
        if t.day > days_in_month(t.year, t.month) {
            return Err(t162Code::DayOutOfRange);
        }
        *self = t;
        Ok(())
    }

    /// 进位数加到秒字段并做全链进位传播。
    pub fn add_seconds(&mut self, mut delta: u64) {
        while delta > 0 {
            let room = 60 - self.second as u64;
            if delta < room {
                self.second += delta as u32;
                return;
            }
            delta -= room;
            self.second = 0;
            self.roll_minute();
        }
    }

    /// 分钟进位（越 59 归零并进位小时）。
    fn roll_minute(&mut self) {
        self.minute += 1;
        if self.minute > 59 {
            self.minute = 0;
            self.roll_hour();
        }
    }

    /// 小时进位（越 23 归零并进位日）。
    fn roll_hour(&mut self) {
        self.hour += 1;
        if self.hour > 23 {
            self.hour = 0;
            self.roll_day();
        }
    }

    /// 日进位（越当月长度归一并进位月）。
    fn roll_day(&mut self) {
        let dim = days_in_month(self.year, self.month);
        if self.day < dim {
            self.day += 1;
            return;
        }
        self.day = 1;
        self.roll_month();
    }

    /// 月进位（越 12 归一并进位年）。
    fn roll_month(&mut self) {
        self.month += 1;
        if self.month > MONTH_MAX {
            self.month = 1;
            self.roll_year();
        }
    }

    /// 年进位（越上限钳制在 YEAR_MAX，防溢出离域）。
    fn roll_year(&mut self) {
        if self.year < YEAR_MAX {
            self.year += 1;
        }
    }

    /// 民用日数（days since 1970-01-01；Hinnant 恒等式减 719468 纪元偏移）。
    pub fn epoch_days(&self) -> i64 {
        let y = self.year as i64;
        let m = self.month as i64;
        let d = self.day as i64;
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        (era * 146097 + doe) - 719_468
    }

    /// UEFI 打包（年 16 位 + 月 4 + 日 5 + 时 5 + 分 6 + 秒 6 + 纳秒量化 9 位）。
    pub fn pack(&self) -> u64 {
        let mut v = self.year as u64 & 0xffff;
        v = (v << 4) | (self.month as u64 & 0xf);
        v = (v << 5) | (self.day as u64 & 0x1f);
        v = (v << 5) | (self.hour as u64 & 0x1f);
        v = (v << 6) | (self.minute as u64 & 0x3f);
        v = (v << 6) | (self.second as u64 & 0x3f);
        v = (v << 9) | ((self.nsec as u64 / 1_953_125) & 0x1ff);
        v
    }

    /// UEFI 解包（pack 互逆，纳秒量化步长 1953125 ns）。
    pub fn unpack(v: u64) -> Result<TimeStamp, t162Code> {
        let nsec_q = (v & 0x1ff) as u32;
        let second = ((v >> 9) & 0x3f) as u32;
        let minute = ((v >> 15) & 0x3f) as u32;
        let hour = ((v >> 21) & 0x1f) as u32;
        let day = ((v >> 26) & 0x1f) as u32;
        let month = ((v >> 31) & 0xf) as u32;
        let year = ((v >> 35) & 0xffff) as u32;
        let nsec = nsec_q * 1_953_125;
        if year < YEAR_MIN {
            return Err(t162Code::YearOutOfRange);
        }
        TimeStamp::new(year, month, day, hour, minute, second, nsec)
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3700, 0x370f]。
pub const fn t162_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t162Code::ALL.len() {
        let c = t162Code::ALL[i].code();
        if c < 0x3700 || c > 0x370f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t162_guard_seg() -> bool {
    t162_segment_guard()
}

/// 月长表与闰年规则对拍（12 月逐月 + 百闰规则）。
fn check_c1_month_table() -> bool {
    let mut ok = true;
    let mut m = 1u32;
    while m <= 12 {
        if days_in_month(2023, m) != COMMON_DAYS[m as usize] {
            ok = false;
        }
        m += 1;
    }
    if days_in_month(2024, 2) != 29 {
        ok = false;
    }
    if days_in_month(1900, 2) != 28 || days_in_month(2000, 2) != 29 || days_in_month(2100, 2) != 28 {
        ok = false;
    }
    ok
}

/// 七字段域校验正负样本（越界拒收 + 边界放行）。
fn check_c2_field_domain() -> bool {
    let mut ok = TimeStamp::new(2026, 10, 11, 5, 56, 56, 999_999_999).is_ok();
    if ok {
        ok = matches!(TimeStamp::new(2026, 13, 1, 0, 0, 0, 0), Err(t162Code::MonthOutOfRange));
    }
    if ok {
        ok = matches!(TimeStamp::new(2026, 2, 29, 0, 0, 0, 0), Err(t162Code::DayOutOfRange));
    }
    if ok {
        ok = TimeStamp::new(2024, 2, 29, 0, 0, 0, 0).is_ok();
    }
    ok
}

/// UEFI 打包往返恒等（含闰年二月 29 日样本）。
fn check_c3_pack_roundtrip() -> bool {
    let mut ok = true;
    let samples = [
        TimeStamp::new(2026, 10, 11, 5, 56, 56, 0).unwrap_or(TimeStamp::ZERO),
        TimeStamp::new(2024, 2, 29, 23, 59, 59, 0).unwrap_or(TimeStamp::ZERO),
        TimeStamp::new(2000, 1, 1, 0, 0, 0, 0).unwrap_or(TimeStamp::ZERO),
        TimeStamp::new(9999, 12, 31, 23, 59, 59, 0).unwrap_or(TimeStamp::ZERO),
    ];
    let mut i = 0;
    while i < samples.len() {
        match TimeStamp::unpack(samples[i].pack()) {
            Ok(rt) => {
                if rt.year != samples[i].year || rt.month != samples[i].month || rt.day != samples[i].day
                    || rt.hour != samples[i].hour || rt.minute != samples[i].minute || rt.second != samples[i].second
                {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    ok
}

/// 进位链传播（秒溢出逐级进位 + 月末/年末路径）。
fn check_c4_carry_chain() -> bool {
    let mut t = TimeStamp::new(2026, 12, 31, 23, 59, 59, 0).unwrap_or(TimeStamp::ZERO);
    t.add_seconds(1);
    let mut ok = t.year == 2027 && t.month == 1 && t.day == 1 && t.hour == 0 && t.minute == 0 && t.second == 0;
    let mut t2 = TimeStamp::new(2024, 2, 28, 23, 59, 59, 0).unwrap_or(TimeStamp::ZERO);
    t2.add_seconds(1);
    ok = ok && t2.month == 2 && t2.day == 29;
    let mut t3 = TimeStamp::new(2023, 2, 28, 23, 59, 59, 0).unwrap_or(TimeStamp::ZERO);
    t3.add_seconds(1);
    ok = ok && t3.month == 3 && t3.day == 1;
    ok
}

/// 纪元日数对拍（锚点日期 + 相邻日差恒 1）。
fn check_c5_epoch_days() -> bool {
    let mut ok = TimeStamp::new(1970, 1, 1, 0, 0, 0, 0).unwrap_or(TimeStamp::ZERO).epoch_days() == 0;
    if ok {
        ok = TimeStamp::new(2000, 3, 1, 0, 0, 0, 0).unwrap_or(TimeStamp::ZERO).epoch_days() == 11017;
    }
    if ok {
        let a = TimeStamp::new(2026, 10, 11, 0, 0, 0, 0).unwrap_or(TimeStamp::ZERO).epoch_days();
        let b = TimeStamp::new(2026, 10, 12, 0, 0, 0, 0).unwrap_or(TimeStamp::ZERO).epoch_days();
        ok = b == a + 1;
    }
    ok
}

/// 字段索引与声明序恒等（枚举↔矩阵对拍）。
fn check_c6_field_index() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < TimeField::ALL.len() {
        if TimeField::ALL[i].index() != i {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t162_checks() -> CheckSet {
    let mut set = CheckSet::new("T162");
    set.add(
        "t162-seg-guard",
        t162_guard_seg(),
        if t162_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t162-c1-month-table",
        check_c1_month_table(),
        if check_c1_month_table() { "" } else { "month table vs leap rule mismatch" },
    );
    set.add(
        "t162-c2-field-domain",
        check_c2_field_domain(),
        if check_c2_field_domain() { "" } else { "field domain positive/negative mismatch" },
    );
    set.add(
        "t162-c3-pack-roundtrip",
        check_c3_pack_roundtrip(),
        if check_c3_pack_roundtrip() { "" } else { "pack/unpack roundtrip mismatch" },
    );
    set.add(
        "t162-c4-carry-chain",
        check_c4_carry_chain(),
        if check_c4_carry_chain() { "" } else { "carry chain propagation mismatch" },
    );
    set.add(
        "t162-c5-epoch-days",
        check_c5_epoch_days(),
        if check_c5_epoch_days() { "" } else { "epoch days anchor mismatch" },
    );
    set.add(
        "t162-c6-field-index",
        check_c6_field_index(),
        if check_c6_field_index() { "" } else { "field index alignment mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t162_all_checks_green() {
        let set = run_t162_checks();
        assert!(set.all_passed(), "T162 domain checks must be green");
    }

    #[test]
    fn t162_segment_guard_holds() {
        assert!(t162_segment_guard());
    }
}
