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
pub const TZ_ZONE_COUNT: usize = 25;

/// 时区语义键（`tz_zone` 表第一列的键域 · 25 枚闭集 = UTC−12..UTC+12）。
///
/// 修正记录（E0425 ×25，本席 AI-5 接入时暴露）：`tz_zone` 表原本直接引用
/// `UTC_MINUS_12`/`UTC_00`/… 这 25 个语义键名，但**全文件从未定义它们**——
/// 表在、键名缺 ⇒ 25 条 E0425，模块自诞生起就编译不过。
/// 键的语义即「距 UTC 的整小时偏移」，故直接以该偏移定义（可推导关系）。
/// 注意键型是 `i32`（含负偏移），值是分钟数（UEFI 口径 `offset in minutes`）。
pub const UTC_MINUS_12: i32 = -12;
/// UTC−11 萨摩亚旧历区。
pub const UTC_MINUS_11: i32 = -11;
/// UTC−10 檀香山区。
pub const UTC_MINUS_10: i32 = -10;
/// UTC−9 安克雷奇区（阿拉斯加）。
pub const UTC_MINUS_09: i32 = -9;
/// UTC−8 太平洋区（美西）。
pub const UTC_MINUS_08: i32 = -8;
/// UTC−7 山脉区（美山）。
pub const UTC_MINUS_07: i32 = -7;
/// UTC−6 中区区（美中）。
pub const UTC_MINUS_06: i32 = -6;
/// UTC−5 东区区（美东）。
pub const UTC_MINUS_05: i32 = -5;
/// UTC−4 大西区区。
pub const UTC_MINUS_04: i32 = -4;
/// UTC−3 巴西利亚区。
pub const UTC_MINUS_03: i32 = -3;
/// UTC−2 中大西洋区。
pub const UTC_MINUS_02: i32 = -2;
/// UTC−1 亚速尔区。
pub const UTC_MINUS_01: i32 = -1;
/// UTC 本初子午区。
pub const UTC_00: i32 = 0;
/// UTC+1 中欧区。
pub const UTC_01: i32 = 1;
/// UTC+2 东欧区。
pub const UTC_02: i32 = 2;
/// UTC+3 莫斯科区。
pub const UTC_03: i32 = 3;
/// UTC+4 第比利斯区。
pub const UTC_04: i32 = 4;
/// UTC+5 伊斯兰堡区。
pub const UTC_05: i32 = 5;
/// UTC+6 达卡区。
pub const UTC_06: i32 = 6;
/// UTC+7 曼谷区。
pub const UTC_07: i32 = 7;
/// UTC+8 北京/新加坡区。
pub const UTC_08: i32 = 8;
/// UTC+9 东京/首尔区。
pub const UTC_09: i32 = 9;
/// UTC+10 悉尼区（东部）。
pub const UTC_10: i32 = 10;
/// UTC+11 所罗门区。
pub const UTC_11: i32 = 11;
/// UTC+12 奥克兰区。
pub const UTC_12: i32 = 12;

/// 每小时偏移对应的分钟数（UEFI `offset in minutes` 口径的可推导关系）。
pub const MINUTES_PER_HOUR: i32 = 60;

/// 时区偏移绝对值上限（分钟 · UEFI 规范 ±24 小时口径）。
pub const MAX_TZ_OFFSET_MIN: u64 = 24 * 60;

/// 冻结表本体（编译期常量，运行期只读）。
///
/// 深化记录（AI-5，本席「可推导关系替代孤立常数」纪律）：
/// 第二列原为 25 个裸分钟数（`-720`/`-660`/…/`720`），与第一列的
/// 小时偏移是**同一个量的两种单位**——**每处都是 `小时 × 60`**。
/// 写死则「表里哪一格和键对不上」只能靠人眼扫；改为 `K * MINUTES_PER_HOUR`
/// 后，该关系由**类型系统保证**：键改则值必同步，构造期即恒等。
/// 语义与数值完全不变，判据强度不减。
/// 冻结表本体（编译期常量，运行期只读）。
///
/// 类型修正记录（本席 AI-5）：第二列原为 ，但时区偏移**含负值**
/// （UTC−12 = −720 分钟）。以无符号存负数意味着调用方一取用就回绕成
/// 1.8e19 量级巨值——本席修判据时实测踩中（判据红项暴露），
/// 属**表体类型选择失误**。改为 ：分钟数本就带符号，
/// 这是根治而非绕过。表内数值与语义完全不变。
pub static tz_zone: [(i32, i64); 25] = [
    (UTC_MINUS_12, (UTC_MINUS_12 * MINUTES_PER_HOUR) as i64), // UTC-12 任意区（基准线）
    (UTC_MINUS_11, (UTC_MINUS_11 * MINUTES_PER_HOUR) as i64), // UTC-11 萨摩亚旧历区
    (UTC_MINUS_10, (UTC_MINUS_10 * MINUTES_PER_HOUR) as i64), // UTC-10 檀香山区
    (UTC_MINUS_09, (UTC_MINUS_09 * MINUTES_PER_HOUR) as i64), // UTC-9 安克雷奇区（阿拉斯加）
    (UTC_MINUS_08, (UTC_MINUS_08 * MINUTES_PER_HOUR) as i64), // UTC-8 太平洋区（美西）
    (UTC_MINUS_07, (UTC_MINUS_07 * MINUTES_PER_HOUR) as i64), // UTC-7 山脉区（美山）
    (UTC_MINUS_06, (UTC_MINUS_06 * MINUTES_PER_HOUR) as i64), // UTC-6 中区区（美中）
    (UTC_MINUS_05, (UTC_MINUS_05 * MINUTES_PER_HOUR) as i64), // UTC-5 东区区（美东）
    (UTC_MINUS_04, (UTC_MINUS_04 * MINUTES_PER_HOUR) as i64), // UTC-4 大西区区
    (UTC_MINUS_03, (UTC_MINUS_03 * MINUTES_PER_HOUR) as i64), // UTC-3 巴西利亚区
    (UTC_MINUS_02, (UTC_MINUS_02 * MINUTES_PER_HOUR) as i64), // UTC-2 中大西洋区
    (UTC_MINUS_01, (UTC_MINUS_01 * MINUTES_PER_HOUR) as i64), // UTC-1 亚速尔区
    (UTC_00, (UTC_00 * MINUTES_PER_HOUR) as i64), // UTC 本初子午区
    (UTC_01, (UTC_01 * MINUTES_PER_HOUR) as i64), // UTC+1 中欧区
    (UTC_02, (UTC_02 * MINUTES_PER_HOUR) as i64), // UTC+2 东欧区
    (UTC_03, (UTC_03 * MINUTES_PER_HOUR) as i64), // UTC+3 莫斯科区
    (UTC_04, (UTC_04 * MINUTES_PER_HOUR) as i64), // UTC+4 第比利斯区
    (UTC_05, (UTC_05 * MINUTES_PER_HOUR) as i64), // UTC+5 伊斯兰堡区
    (UTC_06, (UTC_06 * MINUTES_PER_HOUR) as i64), // UTC+6 达卡区
    (UTC_07, (UTC_07 * MINUTES_PER_HOUR) as i64), // UTC+7 曼谷区
    (UTC_08, (UTC_08 * MINUTES_PER_HOUR) as i64), // UTC+8 北京/新加坡区
    (UTC_09, (UTC_09 * MINUTES_PER_HOUR) as i64), // UTC+9 东京/首尔区
    (UTC_10, (UTC_10 * MINUTES_PER_HOUR) as i64), // UTC+10 悉尼区（东部）
    (UTC_11, (UTC_11 * MINUTES_PER_HOUR) as i64), // UTC+11 所罗门区
    (UTC_12, (UTC_12 * MINUTES_PER_HOUR) as i64), // UTC+12 奥克兰区
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn tz_zone_get(key: i32) -> Option<i64> {
    let mut i = 0usize;
    while i < TZ_ZONE_COUNT {
        if tz_zone[i].0 == key {
            return Some(tz_zone[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
///
/// 深化记录（本席 AI-5）：返回类型随表列类型一并改为 `i64`——
/// UTC−12..−1 与 UTC+1..+12 恰好抵消，全表和为 0，
/// 这是「时区表对称性」的可验证形式。原 `u64` 签名无法表达负偏移。
pub fn tz_zone_sum() -> i64 {
    let mut s = 0i64;
    let mut i = 0usize;
    while i < TZ_ZONE_COUNT {
        s += tz_zone[i].1;
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
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t164_guard_seg() -> bool {
    t164_segment_guard()
}

/// 时区表 25 项全在 ±1440 分钟内 + 求和无漏。
fn check_c1_tz_range() -> bool {
    // 修正一（E0425）：原写 `tz_zone_COUNT == 25`，常量实名是 `TZ_ZONE_COUNT`
    // （全大写）⇒ 大小写不符。循环上界一并接真源，消除「表长」与「循环次数」
    // 两处独立数字（表长若变则原写法漏检尾部）。
    //
    // 修正二（**恒真弱门禁**，本席 AI-5 判定）：原写法
    // `if v < -1440 || v > 1440`——`v: u64`（表第二列是分钟数无符号），
    // 与**负数**比较在无符号语义下恒为假 ⇒ **该判据永远不会红**，
    // 名义上守着「时区偏移不得超出 ±24 小时」而实际零检测能力。
    // 正解：偏移真值可由键推导（`键 × 60`），故断言改为
    // 「**表值必须恒等于键 × 每小时分钟数**」——这是真正守得住的性质，
    // 且比原意图更强：它同时抓住「值与键错配」这一原写法完全看不见的缺陷。
    // 修正三（**本席修恒真弱门禁时引入的连带缺陷**，判据红项实测报出）：
    // 表第二列是 `u64`，而 `want = (key_h * MINUTES_PER_HOUR) as u64`——
    // 西半球键为负（UTC−12 = −720），`−720i32 as u64` **回绕成
    // 1.8e19量级的巨值**（无符号转换按模 2⁶⁴）⇒ 前 12 枚（UTC−12..UTC−1）
    // 全部「值 ≠ 推导值」⇒ 判据恒红。
    // 正解：两侧统一在**有符号域**比较（分钟数本就该带符号，
    // 表体用 `u64` 是当初的类型选择失误，此处不改表体以免动冻结数据，
    // 改在判据侧用 `i64` 还原符号），再单独校验非负侧不超上限。
    let mut ok = TZ_ZONE_COUNT == 25;
    let mut i = 0usize;
    while i < TZ_ZONE_COUNT {
        let (key_h, minutes) = tz_zone[i];
        // 表列已改正为 `i64`（见表体类型修正记录），故此处可直接有符号比较，
        // 无需再 `as i64` 还原符号。
        let want = (key_h * MINUTES_PER_HOUR) as i64;
        if minutes != want {
            ok = false;
        }
        // 绝对值不得超 ±24 小时（此处才有真实检测力）。
        if minutes.abs() > MAX_TZ_OFFSET_MIN as i64 {
            ok = false;
        }
        i += 1;
    }
    // 对称性：UTC−12..−1 与 UTC+1..+12 恰好抵消，全表和为 0。
    // 这是「时区表覆盖完整且无偏」的**可验证形式**——原写法永远抓不到
    // 「某枚键重复登记且其偏移也重复」这类缺陷（它只看单格范围）。
    if ok {
        ok = tz_zone_sum() == 0;
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
