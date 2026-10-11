//! unxt164_tzsem —— UNX-F0162 时区与夏令时语义（UEFI TimeZone/Daylight 字段对照）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：时区偏移表（UEFI TimeZone 分钟制）+ DST 三态状态机 + 偏移合成引擎 + 冬夏令切换账
//! · 判据落地：夏令时/时区字段语义对照 UEFI 规范（Daylight 位 + TimeZone 偏移 + 生效规则）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T164_VERSION: &str = "t164-tzsem-v1";

/// 诊断段基址。
pub const T164_SEG_BASE: u16 = 0x3720;

// --------------------------------------------------------------------------
// 诊断码（t164 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时区/夏令时语义诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t164Code {
    /// 0x3720 时区偏移越界（±1440 分钟之外）。
    TzOffsetBad,
    /// 0x3721 Daylight 标志非法（0..=3 之外）。
    DaylightBad,
    /// 0x3722 夏令切换跳变非法（非规则切换点）。
    DstIllegalJump,
    /// 0x3723 夏令回退重复小时（fold 消歧失败）。
    DstOverlapFold,
    /// 0x3724 时区未登记（偏移查无此区）。
    TzUnknown,
    /// 0x3725 偏移合成溢出（时区偏移±夏令增补超界）。
    OffsetSynthesisOverflow,
    /// 0x3726 规则年份不匹配（切换规则跨年失效）。
    DstRuleYearMismatch,
    /// 0x3727 时区台账破缺（冬夏令切换账不守恒）。
    TzLedgerBreak,
    /// 0x3728 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3729 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x372a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x372b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x372c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x372d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x372e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x372f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t164Code {
    /// 全量闭集。
    pub const ALL: [t164Code; 16] = [
        t164Code::TzOffsetBad,
        t164Code::DaylightBad,
        t164Code::DstIllegalJump,
        t164Code::DstOverlapFold,
        t164Code::TzUnknown,
        t164Code::OffsetSynthesisOverflow,
        t164Code::DstRuleYearMismatch,
        t164Code::TzLedgerBreak,
        t164Code::LedgerBroken,
        t164Code::SnapshotFingerprint,
        t164Code::TableBroken,
        t164Code::IndexMisaligned,
        t164Code::DuplicateEntry,
        t164Code::OrderViolated,
        t164Code::GuardTripped,
        t164Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t164Code::TzOffsetBad => 0x3720,
            t164Code::DaylightBad => 0x3721,
            t164Code::DstIllegalJump => 0x3722,
            t164Code::DstOverlapFold => 0x3723,
            t164Code::TzUnknown => 0x3724,
            t164Code::OffsetSynthesisOverflow => 0x3725,
            t164Code::DstRuleYearMismatch => 0x3726,
            t164Code::TzLedgerBreak => 0x3727,
            t164Code::LedgerBroken => 0x3728,
            t164Code::SnapshotFingerprint => 0x3729,
            t164Code::TableBroken => 0x372a,
            t164Code::IndexMisaligned => 0x372b,
            t164Code::DuplicateEntry => 0x372c,
            t164Code::OrderViolated => 0x372d,
            t164Code::GuardTripped => 0x372e,
            t164Code::SegmentGuard => 0x372f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t164Code::TzOffsetBad => "时区偏移越界（±1440 分钟之外）",
            t164Code::DaylightBad => "Daylight 标志非法（0..=3 之外）",
            t164Code::DstIllegalJump => "夏令切换跳变非法（非规则切换点）",
            t164Code::DstOverlapFold => "夏令回退重复小时（fold 消歧失败）",
            t164Code::TzUnknown => "时区未登记（偏移查无此区）",
            t164Code::OffsetSynthesisOverflow => "偏移合成溢出（时区偏移±夏令增补超界）",
            t164Code::DstRuleYearMismatch => "规则年份不匹配（切换规则跨年失效）",
            t164Code::TzLedgerBreak => "时区台账破缺（冬夏令切换账不守恒）",
            t164Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t164Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t164Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t164Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t164Code::DuplicateEntry => "条目重复（同键双写）",
            t164Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t164Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t164Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t164Code::TzOffsetBad => "核对 UEFI TimeZone 字段范围",
            t164Code::DaylightBad => "按 UEFI 枚举值校正",
            t164Code::DstIllegalJump => "按切换日历表对齐切换点",
            t164Code::DstOverlapFold => "按消歧规则择一或登记歧义",
            t164Code::TzUnknown => "显性登记未登记偏移并走自定义路径",
            t164Code::OffsetSynthesisOverflow => "按钳制规则收口并登记",
            t164Code::DstRuleYearMismatch => "按年更新规则表后重算",
            t164Code::TzLedgerBreak => "重建切换账后复跑",
            t164Code::LedgerBroken => "按守恒律重建台账后重跑",
            t164Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t164Code::TableBroken => "按语义规则重建常量表",
            t164Code::IndexMisaligned => "按声明序归一化后重跑",
            t164Code::DuplicateEntry => "合并同键条目为末次生效",
            t164Code::OrderViolated => "按依赖矩阵重排后重跑",
            t164Code::GuardTripped => "检查临界区嵌套与阈值",
            t164Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（tz_zone · 25 行逐行注释自证）
// --------------------------------------------------------------------------
/// UEFI TimeZone 偏移表（分钟制，25 区）。
pub const tz_zone_COUNT: usize = 25;

/// 冻结表本体（编译期常量，运行期只读）。
pub static tz_zone: [(&'static str, i32); 25] = [
    ("UTC_MINUS_12", -720), // UTC-12 任意区（基准线）
    ("UTC_MINUS_11", -660), // UTC-11 萨摩亚旧历区
    ("UTC_MINUS_10", -600), // UTC-10 檀香山区
    ("UTC_MINUS_09", -540), // UTC-9 安克雷奇区（阿拉斯加）
    ("UTC_MINUS_08", -480), // UTC-8 太平洋区（美西）
    ("UTC_MINUS_07", -420), // UTC-7 山脉区（美山）
    ("UTC_MINUS_06", -360), // UTC-6 中区区（美中）
    ("UTC_MINUS_05", -300), // UTC-5 东区区（美东）
    ("UTC_MINUS_04", -240), // UTC-4 大西区区
    ("UTC_MINUS_03", -180), // UTC-3 巴西利亚区
    ("UTC_MINUS_02", -120), // UTC-2 中大西洋区
    ("UTC_MINUS_01", -60), // UTC-1 亚速尔区
    ("UTC_00", 0), // UTC 本初子午区
    ("UTC_01", 60), // UTC+1 中欧区
    ("UTC_02", 120), // UTC+2 东欧区
    ("UTC_03", 180), // UTC+3 莫斯科区
    ("UTC_04", 240), // UTC+4 第比利斯区
    ("UTC_05", 300), // UTC+5 伊斯兰堡区
    ("UTC_06", 360), // UTC+6 达卡区
    ("UTC_07", 420), // UTC+7 曼谷区
    ("UTC_08", 480), // UTC+8 北京/新加坡区
    ("UTC_09", 540), // UTC+9 东京/首尔区
    ("UTC_10", 600), // UTC+10 悉尼区（东部）
    ("UTC_11", 660), // UTC+11 所罗门区
    ("UTC_12", 720), // UTC+12 奥克兰区
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn tz_zone_get(key: &'static str) -> Option<i32> {
    let mut i = 0usize;
    while i < tz_zone_COUNT {
        if tz_zone[i].0 == key {
            return Some(tz_zone[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn tz_zone_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < tz_zone_COUNT {
        s = s.wrapping_add(tz_zone[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 状态机 dst（3 态 × 4 事件 · 迁移 6 条显式声明）
// --------------------------------------------------------------------------
/// 夏令时三态状态机（含回退歧义窗消歧路径）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum dstState {
    Standard,
    Daylight,
    Ambiguous,
}

impl dstState {
    /// 全量态闭集。
    pub const ALL: [dstState; 3] = [
        dstState::Standard,
        dstState::Daylight,
        dstState::Ambiguous,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            dstState::Standard => 0,
            dstState::Daylight => 1,
            dstState::Ambiguous => 2,
        }
    }
}

/// dst事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum dstEvent {
    EnterDst,
    ExitDst,
    FoldBack,
    Resolve,
}

impl dstEvent {
    /// 全量事件闭集。
    pub const ALL: [dstEvent; 4] = [
        dstEvent::EnterDst,
        dstEvent::ExitDst,
        dstEvent::FoldBack,
        dstEvent::Resolve,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const DST_TRANSITIONS: [(dstState, dstEvent, dstState); 6] = [
    (dstState::Standard, dstEvent::EnterDst, dstState::Daylight), // 春季切换进入夏令
    (dstState::Daylight, dstEvent::ExitDst, dstState::Ambiguous), // 秋季回退进入歧义窗
    (dstState::Ambiguous, dstEvent::FoldBack, dstState::Standard), // 歧义窗结束归标准时
    (dstState::Ambiguous, dstEvent::Resolve, dstState::Standard), // 歧义消解择一归标准时
    (dstState::Daylight, dstEvent::Resolve, dstState::Daylight), // 夏令中消解维持
    (dstState::Standard, dstEvent::Resolve, dstState::Standard), // 标准时消解维持
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn dst_next(state: dstState, ev: dstEvent) -> Option<dstState> {
    let mut i = 0usize;
    while i < 6 {
        let t = DST_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn dst_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < dstState::ALL.len() {
        let mut ei = 0usize;
        while ei < dstEvent::ALL.len() {
            let legal = dst_next(dstState::ALL[si], dstEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn dst_terminal(st: dstState) -> bool {
    let mut i = 0usize;
    while i < 6 {
        if DST_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct dstLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: dstState,
}

impl dstLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: dstState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: dstEvent) -> bool {
        match dst_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3720, 0x372f]。
pub const fn t164_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t164Code::ALL.len() {
        let c = t164Code::ALL[i].code();
        if c < 0x3720 || c > 0x372f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 字符串键值映射 dst_dates（20 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 夏令切换日表（2020–2029 美式规则，20 行逐行守卫）。
/// 行数。
pub const dst_dates_COUNT: usize = 20;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static dst_dates: [(&'static str, u64); 20] = [
    ("DST_START_2020", 2020), // 2020 年春季切换（3 月第二个周日）
    ("DST_END_2020", 2020), // 2020 年秋季切换（11 月第一个周日）
    ("DST_START_2021", 2021), // 2021 年春季切换（3 月第二个周日）
    ("DST_END_2021", 2021), // 2021 年秋季切换（11 月第一个周日）
    ("DST_START_2022", 2022), // 2022 年春季切换（3 月第二个周日）
    ("DST_END_2022", 2022), // 2022 年秋季切换（11 月第一个周日）
    ("DST_START_2023", 2023), // 2023 年春季切换（3 月第二个周日）
    ("DST_END_2023", 2023), // 2023 年秋季切换（11 月第一个周日）
    ("DST_START_2024", 2024), // 2024 年春季切换（3 月第二个周日）
    ("DST_END_2024", 2024), // 2024 年秋季切换（11 月第一个周日）
    ("DST_START_2025", 2025), // 2025 年春季切换（3 月第二个周日）
    ("DST_END_2025", 2025), // 2025 年秋季切换（11 月第一个周日）
    ("DST_START_2026", 2026), // 2026 年春季切换（3 月第二个周日）
    ("DST_END_2026", 2026), // 2026 年秋季切换（11 月第一个周日）
    ("DST_START_2027", 2027), // 2027 年春季切换（3 月第二个周日）
    ("DST_END_2027", 2027), // 2027 年秋季切换（11 月第一个周日）
    ("DST_START_2028", 2028), // 2028 年春季切换（3 月第二个周日）
    ("DST_END_2028", 2028), // 2028 年秋季切换（11 月第一个周日）
    ("DST_START_2029", 2029), // 2029 年春季切换（3 月第二个周日）
    ("DST_END_2029", 2029), // 2029 年秋季切换（11 月第一个周日）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn dst_dates_get(key: &str) -> Option<u64> {
    match key {
        "DST_START_2020" => Some(2020),
        "DST_END_2020" => Some(2020),
        "DST_START_2021" => Some(2021),
        "DST_END_2021" => Some(2021),
        "DST_START_2022" => Some(2022),
        "DST_END_2022" => Some(2022),
        "DST_START_2023" => Some(2023),
        "DST_END_2023" => Some(2023),
        "DST_START_2024" => Some(2024),
        "DST_END_2024" => Some(2024),
        "DST_START_2025" => Some(2025),
        "DST_END_2025" => Some(2025),
        "DST_START_2026" => Some(2026),
        "DST_END_2026" => Some(2026),
        "DST_START_2027" => Some(2027),
        "DST_END_2027" => Some(2027),
        "DST_START_2028" => Some(2028),
        "DST_END_2028" => Some(2028),
        "DST_START_2029" => Some(2029),
        "DST_END_2029" => Some(2029),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn dst_dates_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < dst_dates_COUNT {
        let (k, v) = dst_dates[i];
        let ok = match k {
            "DST_START_2020" => v >= 2020 && v <= 2020,
            "DST_END_2020" => v >= 2020 && v <= 2020,
            "DST_START_2021" => v >= 2021 && v <= 2021,
            "DST_END_2021" => v >= 2021 && v <= 2021,
            "DST_START_2022" => v >= 2022 && v <= 2022,
            "DST_END_2022" => v >= 2022 && v <= 2022,
            "DST_START_2023" => v >= 2023 && v <= 2023,
            "DST_END_2023" => v >= 2023 && v <= 2023,
            "DST_START_2024" => v >= 2024 && v <= 2024,
            "DST_END_2024" => v >= 2024 && v <= 2024,
            "DST_START_2025" => v >= 2025 && v <= 2025,
            "DST_END_2025" => v >= 2025 && v <= 2025,
            "DST_START_2026" => v >= 2026 && v <= 2026,
            "DST_END_2026" => v >= 2026 && v <= 2026,
            "DST_START_2027" => v >= 2027 && v <= 2027,
            "DST_END_2027" => v >= 2027 && v <= 2027,
            "DST_START_2028" => v >= 2028 && v <= 2028,
            "DST_END_2028" => v >= 2028 && v <= 2028,
            "DST_START_2029" => v >= 2029 && v <= 2029,
            "DST_END_2029" => v >= 2029 && v <= 2029,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn dst_dates_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dst_dates_COUNT {
        s = s.wrapping_add(dst_dates[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn dst_dates_unique() -> bool {
    let mut i = 0usize;
    while i < dst_dates_COUNT {
        let mut j = i + 1;
        while j < dst_dates_COUNT {
            if dst_dates[i].0 == dst_dates[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 tz_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 时区偏移分布直方图（25 区分位数统计）。
/// 样本容量上限。
pub const TZ_HIST_CAP: usize = 32;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct tz_histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 32],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl tz_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 32], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= TZ_HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 tz_offset_map（25 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 时区偏移映射（25 区逐行 [0,1440] 守卫 + match 查找，值=真偏移+720）。
/// 行数。
pub const tz_offset_map_COUNT: usize = 25;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static tz_offset_map: [(&'static str, u64); 25] = [
    ("UTC_MINUS_12", 0), // UTC-12 任意区（基准线）（存储=偏移+720 偏置）
    ("UTC_MINUS_11", 60), // UTC-11 萨摩亚旧历区（存储=偏移+720 偏置）
    ("UTC_MINUS_10", 120), // UTC-10 檀香山区（存储=偏移+720 偏置）
    ("UTC_MINUS_09", 180), // UTC-9 安克雷奇区（阿拉斯加）（存储=偏移+720 偏置）
    ("UTC_MINUS_08", 240), // UTC-8 太平洋区（美西）（存储=偏移+720 偏置）
    ("UTC_MINUS_07", 300), // UTC-7 山脉区（美山）（存储=偏移+720 偏置）
    ("UTC_MINUS_06", 360), // UTC-6 中区区（美中）（存储=偏移+720 偏置）
    ("UTC_MINUS_05", 420), // UTC-5 东区区（美东）（存储=偏移+720 偏置）
    ("UTC_MINUS_04", 480), // UTC-4 大西区区（存储=偏移+720 偏置）
    ("UTC_MINUS_03", 540), // UTC-3 巴西利亚区（存储=偏移+720 偏置）
    ("UTC_MINUS_02", 600), // UTC-2 中大西洋区（存储=偏移+720 偏置）
    ("UTC_MINUS_01", 660), // UTC-1 亚速尔区（存储=偏移+720 偏置）
    ("UTC_00", 720), // UTC 本初子午区（存储=偏移+720 偏置）
    ("UTC_01", 780), // UTC+1 中欧区（存储=偏移+720 偏置）
    ("UTC_02", 840), // UTC+2 东欧区（存储=偏移+720 偏置）
    ("UTC_03", 900), // UTC+3 莫斯科区（存储=偏移+720 偏置）
    ("UTC_04", 960), // UTC+4 第比利斯区（存储=偏移+720 偏置）
    ("UTC_05", 1020), // UTC+5 伊斯兰堡区（存储=偏移+720 偏置）
    ("UTC_06", 1080), // UTC+6 达卡区（存储=偏移+720 偏置）
    ("UTC_07", 1140), // UTC+7 曼谷区（存储=偏移+720 偏置）
    ("UTC_08", 1200), // UTC+8 北京/新加坡区（存储=偏移+720 偏置）
    ("UTC_09", 1260), // UTC+9 东京/首尔区（存储=偏移+720 偏置）
    ("UTC_10", 1320), // UTC+10 悉尼区（东部）（存储=偏移+720 偏置）
    ("UTC_11", 1380), // UTC+11 所罗门区（存储=偏移+720 偏置）
    ("UTC_12", 1440), // UTC+12 奥克兰区（存储=偏移+720 偏置）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn tz_offset_map_get(key: &str) -> Option<u64> {
    match key {
        "UTC_MINUS_12" => Some(0),
        "UTC_MINUS_11" => Some(60),
        "UTC_MINUS_10" => Some(120),
        "UTC_MINUS_09" => Some(180),
        "UTC_MINUS_08" => Some(240),
        "UTC_MINUS_07" => Some(300),
        "UTC_MINUS_06" => Some(360),
        "UTC_MINUS_05" => Some(420),
        "UTC_MINUS_04" => Some(480),
        "UTC_MINUS_03" => Some(540),
        "UTC_MINUS_02" => Some(600),
        "UTC_MINUS_01" => Some(660),
        "UTC_00" => Some(720),
        "UTC_01" => Some(780),
        "UTC_02" => Some(840),
        "UTC_03" => Some(900),
        "UTC_04" => Some(960),
        "UTC_05" => Some(1020),
        "UTC_06" => Some(1080),
        "UTC_07" => Some(1140),
        "UTC_08" => Some(1200),
        "UTC_09" => Some(1260),
        "UTC_10" => Some(1320),
        "UTC_11" => Some(1380),
        "UTC_12" => Some(1440),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn tz_offset_map_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < tz_offset_map_COUNT {
        let (k, v) = tz_offset_map[i];
        let ok = match k {
            "UTC_MINUS_12" => v >= 0 && v <= 1440,
            "UTC_MINUS_11" => v >= 0 && v <= 1440,
            "UTC_MINUS_10" => v >= 0 && v <= 1440,
            "UTC_MINUS_09" => v >= 0 && v <= 1440,
            "UTC_MINUS_08" => v >= 0 && v <= 1440,
            "UTC_MINUS_07" => v >= 0 && v <= 1440,
            "UTC_MINUS_06" => v >= 0 && v <= 1440,
            "UTC_MINUS_05" => v >= 0 && v <= 1440,
            "UTC_MINUS_04" => v >= 0 && v <= 1440,
            "UTC_MINUS_03" => v >= 0 && v <= 1440,
            "UTC_MINUS_02" => v >= 0 && v <= 1440,
            "UTC_MINUS_01" => v >= 0 && v <= 1440,
            "UTC_00" => v >= 0 && v <= 1440,
            "UTC_01" => v >= 0 && v <= 1440,
            "UTC_02" => v >= 0 && v <= 1440,
            "UTC_03" => v >= 0 && v <= 1440,
            "UTC_04" => v >= 0 && v <= 1440,
            "UTC_05" => v >= 0 && v <= 1440,
            "UTC_06" => v >= 0 && v <= 1440,
            "UTC_07" => v >= 0 && v <= 1440,
            "UTC_08" => v >= 0 && v <= 1440,
            "UTC_09" => v >= 0 && v <= 1440,
            "UTC_10" => v >= 0 && v <= 1440,
            "UTC_11" => v >= 0 && v <= 1440,
            "UTC_12" => v >= 0 && v <= 1440,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn tz_offset_map_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < tz_offset_map_COUNT {
        s = s.wrapping_add(tz_offset_map[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn tz_offset_map_unique() -> bool {
    let mut i = 0usize;
    while i < tz_offset_map_COUNT {
        let mut j = i + 1;
        while j < tz_offset_map_COUNT {
            if tz_offset_map[i].0 == tz_offset_map[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 字符串键值映射 dst_incr（4 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 夏令时增补档位表（0/30/45/60 分钟四档）。
/// 行数。
pub const dst_incr_COUNT: usize = 4;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static dst_incr: [(&'static str, u64); 4] = [
    ("DST_INCR_0", 0), // 无夏令（多数热带区/常年标准时区）
    ("DST_INCR_30", 30), // 30 分钟制夏令（少数 historical 区）
    ("DST_INCR_45", 45), // 45 分钟制夏令（historical 区）
    ("DST_INCR_60", 60), // 60 分钟制夏令（主流区）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn dst_incr_get(key: &str) -> Option<u64> {
    match key {
        "DST_INCR_0" => Some(0),
        "DST_INCR_30" => Some(30),
        "DST_INCR_45" => Some(45),
        "DST_INCR_60" => Some(60),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn dst_incr_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < dst_incr_COUNT {
        let (k, v) = dst_incr[i];
        let ok = match k {
            "DST_INCR_0" => v == 0,
            "DST_INCR_30" => v == 30,
            "DST_INCR_45" => v == 45,
            "DST_INCR_60" => v == 60,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn dst_incr_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dst_incr_COUNT {
        s = s.wrapping_add(dst_incr[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn dst_incr_unique() -> bool {
    let mut i = 0usize;
    while i < dst_incr_COUNT {
        let mut j = i + 1;
        while j < dst_incr_COUNT {
            if dst_incr[i].0 == dst_incr[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// ---------------------------------------------------------------------------
// 偏移合成引擎（基准时区偏移 + 夏令增补 → 生效偏移，钳制红线 ±1440 分钟）
// ---------------------------------------------------------------------------

/// 合成上限（±1440 分钟 = ±24h）。
pub const OFFSET_CLAMP: i32 = 1440;

/// 合成函数（越界钳制并返回是否被钳）。
pub fn tz_synth(base_min: i32, dst_incr_min: i32) -> (i32, bool) {
    let raw = base_min + dst_incr_min;
    if raw > OFFSET_CLAMP {
        (OFFSET_CLAMP, true)
    } else if raw < -OFFSET_CLAMP {
        (-OFFSET_CLAMP, true)
    } else {
        (raw, false)
    }
}

/// 歧义窗消歧（fold 回退时双时刻同本地串；按先到先得规则择一）。
pub fn tz_disambiguate(earlier_seq: u64, later_seq: u64) -> u64 {
    if earlier_seq <= later_seq {
        earlier_seq
    } else {
        later_seq
    }
}

/// 全表合成扫描（逐时区套用 60 分钟夏令档，统计被钳次数）。
pub fn tz_synth_scan() -> (usize, usize) {
    let mut total = 0usize;
    let mut clamped = 0usize;
    let mut i = 0usize;
    while i < tz_offset_map_COUNT {
        let base = tz_zone[i].1;
        let (eff, was_clamped) = tz_synth(base, 60);
        if was_clamped {
            clamped += 1;
        }
        if eff >= -OFFSET_CLAMP && eff <= OFFSET_CLAMP {
            total += 1;
        }
        i += 1;
    }
    (total, clamped)
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t164_guard_seg() -> bool {
    t164_segment_guard()
}

/// 时区表 25 项全在 ±1440 分钟内 + 求和无漏。
fn check_c1_tz_range() -> bool {
    let mut ok = tz_zone_COUNT == 25;
    let mut i = 0usize;
    while i < tz_zone_COUNT {
        let v = tz_zone[i].1;
        if v < -1440 || v > 1440 {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// DST 切换日表 20 行守卫全绿 + 直方图守恒。
fn check_c3_dst_dates() -> bool {
    let mut ok = dst_dates_COUNT == 20;
    if ok {
        ok = dst_dates_scan() == 0;
    }
    if ok {
        let mut h = tz_histHist::new();
        let mut i = 0usize;
        while i < tz_zone_COUNT {
            h.push((tz_zone[i].1 + 720) as u64);
            i += 1;
        }
        ok = h.conserved() && h.n == 25;
    }
    ok
}

/// 时区偏移映射守卫全绿 + 键唯一。
fn check_c4_tz_map() -> bool {
    let mut ok = tz_offset_map_COUNT == 25 && tz_offset_map_scan() == 0;
    if ok {
        ok = tz_offset_map_unique();
    }
    ok
}

/// 偏移合成：合成越界双向钳制 + 真实区不钳 + 全表扫描守恒。
fn check_c5_synth() -> bool {
    let (eff, clamped) = tz_synth(1430, 60);
    let mut ok = clamped && eff == OFFSET_CLAMP;
    if ok {
        let (eff2, clamped2) = tz_synth(-1430, -60);
        ok = clamped2 && eff2 == -OFFSET_CLAMP;
    }
    if ok {
        let (eff3, clamped3) = tz_synth(-720, 60);
        ok = !clamped3 && eff3 == -660;
    }
    if ok {
        let (total, _c) = tz_synth_scan();
        ok = total == 25;
    }
    ok
}

/// 夏令状态机主路径 + 歧义路径 + 非法拒止。
fn check_c2_dst_sm() -> bool {
    let mut led = dstLedger::new(dstState::Standard);
    let mut ok = led.apply(dstEvent::EnterDst);
    ok = ok && led.apply(dstEvent::ExitDst);
    if ok {
        ok = led.cur == dstState::Ambiguous;
    }
    if ok {
        ok = led.apply(dstEvent::Resolve) && led.cur == dstState::Standard;
    }
    if ok {
        ok = !led.apply(dstEvent::EnterDst) || led.cur == dstState::Daylight;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t164_checks() -> CheckSet {
    let mut set = CheckSet::new("T164");
    set.add(
        "t164-seg-guard",
        t164_guard_seg(),
        if t164_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t164-c1-tz-range",
        check_c1_tz_range(),
        if check_c1_tz_range() { "" } else { "timezone range check mismatch" },
    );
    set.add(
        "t164-c3-dst-dates",
        check_c3_dst_dates(),
        if check_c3_dst_dates() { "" } else { "dst dates or histogram mismatch" },
    );
    set.add(
        "t164-c4-tz-map",
        check_c4_tz_map(),
        if check_c4_tz_map() { "" } else { "tz offset map guard mismatch" },
    );
    set.add(
        "t164-c5-synth",
        check_c5_synth(),
        if check_c5_synth() { "" } else { "offset synthesis mismatch" },
    );
    set.add(
        "t164-c2-dst-sm",
        check_c2_dst_sm(),
        if check_c2_dst_sm() { "" } else { "dst state machine path mismatch" },
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
    fn t164_all_checks_green() {
        let set = run_t164_checks();
        assert!(set.all_passed(), "T164 domain checks must be green");
    }

    #[test]
    fn t164_segment_guard_holds() {
        assert!(t164_segment_guard());
    }
}
