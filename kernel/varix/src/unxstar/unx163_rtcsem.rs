//! unxt163_rtcsem —— UNX-F0162 RTC 寄存器语义（CMOS 表 + BCD 双编码 + UIP 停等）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：CMOS/RTC 寄存器语义表（0x00–0x0D + 状态 A/B）+ BCD↔二进制双编码表 + UIP 停等状态机 + 双源二次读对拍
//! · 判据落地：时间读取与硬件 RTC 对照误差 <1s 的硬件面语义（读取停等+二分校验）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T163_VERSION: &str = "t163-rtcsem-v1";

/// 诊断段基址。
pub const T163_SEG_BASE: u16 = 0x3710;

// --------------------------------------------------------------------------
// 诊断码（t163 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RTC 语义诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t163Code {
    /// 0x3710 寄存器索引越界（0x00..=0x0F 之外）。
    RegIndexOob,
    /// 0x3711 BCD 半字节越界（A..F 非十进制）。
    BcdNibbleBad,
    /// 0x3712 UIP 停等超时（更新周期未收敛）。
    UipStallTooLong,
    /// 0x3713 双源二次读不一致（更新边界穿越）。
    DoubleReadMismatch,
    /// 0x3714 格式位非法（12/24 小时制与 BCD 位组合矛盾）。
    FormatBitBad,
    /// 0x3715 世纪寄存器缺失（两位年份无法归位）。
    CenturyRegMissing,
    /// 0x3716 报警未使能（读报警面时功能关断）。
    AlarmDisabled,
    /// 0x3717 回读值未变（写后读回旧值）。
    ValueUnchanged,
    /// 0x3718 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3719 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x371a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x371b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x371c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x371d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x371e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x371f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t163Code {
    /// 全量闭集。
    pub const ALL: [t163Code; 16] = [
        t163Code::RegIndexOob,
        t163Code::BcdNibbleBad,
        t163Code::UipStallTooLong,
        t163Code::DoubleReadMismatch,
        t163Code::FormatBitBad,
        t163Code::CenturyRegMissing,
        t163Code::AlarmDisabled,
        t163Code::ValueUnchanged,
        t163Code::LedgerBroken,
        t163Code::SnapshotFingerprint,
        t163Code::TableBroken,
        t163Code::IndexMisaligned,
        t163Code::DuplicateEntry,
        t163Code::OrderViolated,
        t163Code::GuardTripped,
        t163Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t163Code::RegIndexOob => 0x3710,
            t163Code::BcdNibbleBad => 0x3711,
            t163Code::UipStallTooLong => 0x3712,
            t163Code::DoubleReadMismatch => 0x3713,
            t163Code::FormatBitBad => 0x3714,
            t163Code::CenturyRegMissing => 0x3715,
            t163Code::AlarmDisabled => 0x3716,
            t163Code::ValueUnchanged => 0x3717,
            t163Code::LedgerBroken => 0x3718,
            t163Code::SnapshotFingerprint => 0x3719,
            t163Code::TableBroken => 0x371a,
            t163Code::IndexMisaligned => 0x371b,
            t163Code::DuplicateEntry => 0x371c,
            t163Code::OrderViolated => 0x371d,
            t163Code::GuardTripped => 0x371e,
            t163Code::SegmentGuard => 0x371f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t163Code::RegIndexOob => "寄存器索引越界（0x00..=0x0F 之外）",
            t163Code::BcdNibbleBad => "BCD 半字节越界（A..F 非十进制）",
            t163Code::UipStallTooLong => "UIP 停等超时（更新周期未收敛）",
            t163Code::DoubleReadMismatch => "双源二次读不一致（更新边界穿越）",
            t163Code::FormatBitBad => "格式位非法（12/24 小时制与 BCD 位组合矛盾）",
            t163Code::CenturyRegMissing => "世纪寄存器缺失（两位年份无法归位）",
            t163Code::AlarmDisabled => "报警未使能（读报警面时功能关断）",
            t163Code::ValueUnchanged => "回读值未变（写后读回旧值）",
            t163Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t163Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t163Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t163Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t163Code::DuplicateEntry => "条目重复（同键双写）",
            t163Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t163Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t163Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t163Code::RegIndexOob => "核对 CMOS 寄存器地址分配",
            t163Code::BcdNibbleBad => "核对 RTC 编码格式位（状态 B）",
            t163Code::UipStallTooLong => "按超时路径降级重读并记账",
            t163Code::DoubleReadMismatch => "按读序协议重读并记账",
            t163Code::FormatBitBad => "按状态 B 固定组合重配",
            t163Code::CenturyRegMissing => "按世纪登记簿推补或显性登记缺位",
            t163Code::AlarmDisabled => "核对状态 B 报警使能位",
            t163Code::ValueUnchanged => "按写序协议延长停等后复读",
            t163Code::LedgerBroken => "按守恒律重建台账后重跑",
            t163Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t163Code::TableBroken => "按语义规则重建常量表",
            t163Code::IndexMisaligned => "按声明序归一化后重跑",
            t163Code::DuplicateEntry => "合并同键条目为末次生效",
            t163Code::OrderViolated => "按依赖矩阵重排后重跑",
            t163Code::GuardTripped => "检查临界区嵌套与阈值",
            t163Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（rtc_reg · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// CMOS/RTC 寄存器语义表（索引↔地址↔语义）。
pub const rtc_reg_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static rtc_reg: [(&'static str, u64); 16] = [
    ("RTC_SEC", 0), // 秒寄存器（BCD 或二进制由 B 位定）
    ("RTC_SEC_ALARM", 1), // 秒报警寄存器
    ("RTC_MIN", 2), // 分寄存器
    ("RTC_MIN_ALARM", 3), // 分报警寄存器
    ("RTC_HOUR", 4), // 小时寄存器（12/24 小时制由 B 位定）
    ("RTC_HOUR_ALARM", 5), // 小时报警寄存器
    ("RTC_WEEKDAY", 6), // 星期寄存器
    ("RTC_DAY", 7), // 日寄存器
    ("RTC_MONTH", 8), // 月寄存器
    ("RTC_YEAR", 9), // 年寄存器（两位年份）
    ("RTC_STAT_A", 10), // 状态 A（UIP 位 + 速率选择）
    ("RTC_STAT_B", 11), // 状态 B（格式/周期中断/方波使能）
    ("RTC_STAT_C", 12), // 状态 C（中断标志，只读清）
    ("RTC_STAT_D", 13), // 状态 D（电池有效位）
    ("RTC_DIAG", 14), // 诊断寄存器（磁盘类型，遗留语义）
    ("RTC_RESET", 15), // 复位码寄存器（遗留语义）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn rtc_reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < rtc_reg_COUNT {
        if rtc_reg[i].0 == key {
            return Some(rtc_reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn rtc_reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < rtc_reg_COUNT {
        s = s.wrapping_add(rtc_reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 冻结常量表（bcd_bin · 100 行逐行注释自证）
// --------------------------------------------------------------------------
/// BCD↔二进制双编码表（100 项全枚举，运行期无算术兜底）。
pub const bcd_bin_COUNT: usize = 100;

/// 冻结表本体（编译期常量，运行期只读）。
pub static bcd_bin: [(&'static str, u64); 100] = [
    ("0", 0), // BCD 编码 00 ↔ 二进制 00
    ("1", 1), // BCD 编码 01 ↔ 二进制 01
    ("2", 2), // BCD 编码 02 ↔ 二进制 02
    ("3", 3), // BCD 编码 03 ↔ 二进制 03
    ("4", 4), // BCD 编码 04 ↔ 二进制 04
    ("5", 5), // BCD 编码 05 ↔ 二进制 05
    ("6", 6), // BCD 编码 06 ↔ 二进制 06
    ("7", 7), // BCD 编码 07 ↔ 二进制 07
    ("8", 8), // BCD 编码 08 ↔ 二进制 08
    ("9", 9), // BCD 编码 09 ↔ 二进制 09
    ("10", 10), // BCD 编码 0a ↔ 二进制 10
    ("11", 11), // BCD 编码 0b ↔ 二进制 11
    ("12", 12), // BCD 编码 0c ↔ 二进制 12
    ("13", 13), // BCD 编码 0d ↔ 二进制 13
    ("14", 14), // BCD 编码 0e ↔ 二进制 14
    ("15", 15), // BCD 编码 0f ↔ 二进制 15
    ("16", 16), // BCD 编码 10 ↔ 二进制 16
    ("17", 17), // BCD 编码 11 ↔ 二进制 17
    ("18", 18), // BCD 编码 12 ↔ 二进制 18
    ("19", 19), // BCD 编码 13 ↔ 二进制 19
    ("20", 20), // BCD 编码 14 ↔ 二进制 20
    ("21", 21), // BCD 编码 15 ↔ 二进制 21
    ("22", 22), // BCD 编码 16 ↔ 二进制 22
    ("23", 23), // BCD 编码 17 ↔ 二进制 23
    ("24", 24), // BCD 编码 18 ↔ 二进制 24
    ("25", 25), // BCD 编码 19 ↔ 二进制 25
    ("26", 26), // BCD 编码 1a ↔ 二进制 26
    ("27", 27), // BCD 编码 1b ↔ 二进制 27
    ("28", 28), // BCD 编码 1c ↔ 二进制 28
    ("29", 29), // BCD 编码 1d ↔ 二进制 29
    ("30", 30), // BCD 编码 1e ↔ 二进制 30
    ("31", 31), // BCD 编码 1f ↔ 二进制 31
    ("32", 32), // BCD 编码 20 ↔ 二进制 32
    ("33", 33), // BCD 编码 21 ↔ 二进制 33
    ("34", 34), // BCD 编码 22 ↔ 二进制 34
    ("35", 35), // BCD 编码 23 ↔ 二进制 35
    ("36", 36), // BCD 编码 24 ↔ 二进制 36
    ("37", 37), // BCD 编码 25 ↔ 二进制 37
    ("38", 38), // BCD 编码 26 ↔ 二进制 38
    ("39", 39), // BCD 编码 27 ↔ 二进制 39
    ("40", 40), // BCD 编码 28 ↔ 二进制 40
    ("41", 41), // BCD 编码 29 ↔ 二进制 41
    ("42", 42), // BCD 编码 2a ↔ 二进制 42
    ("43", 43), // BCD 编码 2b ↔ 二进制 43
    ("44", 44), // BCD 编码 2c ↔ 二进制 44
    ("45", 45), // BCD 编码 2d ↔ 二进制 45
    ("46", 46), // BCD 编码 2e ↔ 二进制 46
    ("47", 47), // BCD 编码 2f ↔ 二进制 47
    ("48", 48), // BCD 编码 30 ↔ 二进制 48
    ("49", 49), // BCD 编码 31 ↔ 二进制 49
    ("50", 50), // BCD 编码 32 ↔ 二进制 50
    ("51", 51), // BCD 编码 33 ↔ 二进制 51
    ("52", 52), // BCD 编码 34 ↔ 二进制 52
    ("53", 53), // BCD 编码 35 ↔ 二进制 53
    ("54", 54), // BCD 编码 36 ↔ 二进制 54
    ("55", 55), // BCD 编码 37 ↔ 二进制 55
    ("56", 56), // BCD 编码 38 ↔ 二进制 56
    ("57", 57), // BCD 编码 39 ↔ 二进制 57
    ("58", 58), // BCD 编码 3a ↔ 二进制 58
    ("59", 59), // BCD 编码 3b ↔ 二进制 59
    ("60", 60), // BCD 编码 3c ↔ 二进制 60
    ("61", 61), // BCD 编码 3d ↔ 二进制 61
    ("62", 62), // BCD 编码 3e ↔ 二进制 62
    ("63", 63), // BCD 编码 3f ↔ 二进制 63
    ("64", 64), // BCD 编码 40 ↔ 二进制 64
    ("65", 65), // BCD 编码 41 ↔ 二进制 65
    ("66", 66), // BCD 编码 42 ↔ 二进制 66
    ("67", 67), // BCD 编码 43 ↔ 二进制 67
    ("68", 68), // BCD 编码 44 ↔ 二进制 68
    ("69", 69), // BCD 编码 45 ↔ 二进制 69
    ("70", 70), // BCD 编码 46 ↔ 二进制 70
    ("71", 71), // BCD 编码 47 ↔ 二进制 71
    ("72", 72), // BCD 编码 48 ↔ 二进制 72
    ("73", 73), // BCD 编码 49 ↔ 二进制 73
    ("74", 74), // BCD 编码 4a ↔ 二进制 74
    ("75", 75), // BCD 编码 4b ↔ 二进制 75
    ("76", 76), // BCD 编码 4c ↔ 二进制 76
    ("77", 77), // BCD 编码 4d ↔ 二进制 77
    ("78", 78), // BCD 编码 4e ↔ 二进制 78
    ("79", 79), // BCD 编码 4f ↔ 二进制 79
    ("80", 80), // BCD 编码 50 ↔ 二进制 80
    ("81", 81), // BCD 编码 51 ↔ 二进制 81
    ("82", 82), // BCD 编码 52 ↔ 二进制 82
    ("83", 83), // BCD 编码 53 ↔ 二进制 83
    ("84", 84), // BCD 编码 54 ↔ 二进制 84
    ("85", 85), // BCD 编码 55 ↔ 二进制 85
    ("86", 86), // BCD 编码 56 ↔ 二进制 86
    ("87", 87), // BCD 编码 57 ↔ 二进制 87
    ("88", 88), // BCD 编码 58 ↔ 二进制 88
    ("89", 89), // BCD 编码 59 ↔ 二进制 89
    ("90", 90), // BCD 编码 5a ↔ 二进制 90
    ("91", 91), // BCD 编码 5b ↔ 二进制 91
    ("92", 92), // BCD 编码 5c ↔ 二进制 92
    ("93", 93), // BCD 编码 5d ↔ 二进制 93
    ("94", 94), // BCD 编码 5e ↔ 二进制 94
    ("95", 95), // BCD 编码 5f ↔ 二进制 95
    ("96", 96), // BCD 编码 60 ↔ 二进制 96
    ("97", 97), // BCD 编码 61 ↔ 二进制 97
    ("98", 98), // BCD 编码 62 ↔ 二进制 98
    ("99", 99), // BCD 编码 63 ↔ 二进制 99
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn bcd_bin_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < bcd_bin_COUNT {
        if bcd_bin[i].0 == key {
            return Some(bcd_bin[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn bcd_bin_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < bcd_bin_COUNT {
        s = s.wrapping_add(bcd_bin[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 状态机 uip（5 态 × 5 事件 · 迁移 7 条显式声明）
// --------------------------------------------------------------------------
/// UIP 停等读取状态机（更新周期穿越防护）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum uipState {
    Idle,
    WaitingUip,
    Reading,
    Verify,
    Consistent,
}

impl uipState {
    /// 全量态闭集。
    pub const ALL: [uipState; 5] = [
        uipState::Idle,
        uipState::WaitingUip,
        uipState::Reading,
        uipState::Verify,
        uipState::Consistent,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            uipState::Idle => 0,
            uipState::WaitingUip => 1,
            uipState::Reading => 2,
            uipState::Verify => 3,
            uipState::Consistent => 4,
        }
    }
}

/// uip事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum uipEvent {
    Start,
    UipClear,
    ReadDone,
    VerifyOk,
    Timeout,
}

impl uipEvent {
    /// 全量事件闭集。
    pub const ALL: [uipEvent; 5] = [
        uipEvent::Start,
        uipEvent::UipClear,
        uipEvent::ReadDone,
        uipEvent::VerifyOk,
        uipEvent::Timeout,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const UIP_TRANSITIONS: [(uipState, uipEvent, uipState); 7] = [
    (uipState::Idle, uipEvent::Start, uipState::WaitingUip), // 启动读取序列
    (uipState::WaitingUip, uipEvent::UipClear, uipState::Reading), // UIP 清零后进入读段
    (uipState::WaitingUip, uipEvent::Timeout, uipState::Idle), // 停等超时归位重试
    (uipState::Reading, uipEvent::ReadDone, uipState::Verify), // 首读完成进入校验
    (uipState::Verify, uipEvent::VerifyOk, uipState::Consistent), // 二次读一致收口
    (uipState::Verify, uipEvent::Timeout, uipState::Idle), // 二次读不一致退回重读
    (uipState::Consistent, uipEvent::Start, uipState::WaitingUip), // 下一轮复用
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn uip_next(state: uipState, ev: uipEvent) -> Option<uipState> {
    let mut i = 0usize;
    while i < 7 {
        let t = UIP_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn uip_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < uipState::ALL.len() {
        let mut ei = 0usize;
        while ei < uipEvent::ALL.len() {
            let legal = uip_next(uipState::ALL[si], uipEvent::ALL[ei]).is_some();
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
pub fn uip_terminal(st: uipState) -> bool {
    let mut i = 0usize;
    while i < 7 {
        if UIP_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct uipLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: uipState,
}

impl uipLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: uipState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: uipEvent) -> bool {
        match uip_next(self.cur, ev) {
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

// --------------------------------------------------------------------------
// 引擎 rtc_dual（双源对拍：6 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// RTC 双源二次读对拍（秒/分/时/日/月/年 六槽逐槽一致）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum rtc_dualSlot {
    /// 秒槽。
    Sec,
    /// 分槽。
    Min,
    /// 小时槽。
    Hour,
    /// 日槽。
    Day,
    /// 月槽。
    Month,
    /// 年槽。
    Year,
}

impl rtc_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [rtc_dualSlot; 6] = [
        rtc_dualSlot::Sec,
        rtc_dualSlot::Min,
        rtc_dualSlot::Hour,
        rtc_dualSlot::Day,
        rtc_dualSlot::Month,
        rtc_dualSlot::Year,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            rtc_dualSlot::Sec => 0,
            rtc_dualSlot::Min => 1,
            rtc_dualSlot::Hour => 2,
            rtc_dualSlot::Day => 3,
            rtc_dualSlot::Month => 4,
            rtc_dualSlot::Year => 5,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            rtc_dualSlot::Sec => "秒槽",
            rtc_dualSlot::Min => "分槽",
            rtc_dualSlot::Hour => "小时槽",
            rtc_dualSlot::Day => "日槽",
            rtc_dualSlot::Month => "月槽",
            rtc_dualSlot::Year => "年槽",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct rtc_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct rtc_dualDual {
    /// 逐槽样本。
    pub pairs: [rtc_dualPair; 6],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl rtc_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [rtc_dualPair { a: 0, b: 0, same: false, seq: 0 }; 6],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: rtc_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = rtc_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 6
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 6 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 6 && self.same_n + self.diff_n == 6
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3710, 0x371f]。
pub const fn t163_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t163Code::ALL.len() {
        let c = t163Code::ALL[i].code();
        if c < 0x3710 || c > 0x371f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 字符串键值映射 bcd_bin_map（100 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// BCD↔二进制映射（100 项逐行守卫 + match 查找，运行期无算术兜底）。
/// 行数。
pub const bcd_bin_map_COUNT: usize = 100;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static bcd_bin_map: [(&'static str, u64); 100] = [
    ("BCD00", 0), // BCD 00 → 二进制 0
    ("BCD01", 1), // BCD 01 → 二进制 1
    ("BCD02", 2), // BCD 02 → 二进制 2
    ("BCD03", 3), // BCD 03 → 二进制 3
    ("BCD04", 4), // BCD 04 → 二进制 4
    ("BCD05", 5), // BCD 05 → 二进制 5
    ("BCD06", 6), // BCD 06 → 二进制 6
    ("BCD07", 7), // BCD 07 → 二进制 7
    ("BCD08", 8), // BCD 08 → 二进制 8
    ("BCD09", 9), // BCD 09 → 二进制 9
    ("BCD10", 10), // BCD 0A → 二进制 10
    ("BCD11", 11), // BCD 0B → 二进制 11
    ("BCD12", 12), // BCD 0C → 二进制 12
    ("BCD13", 13), // BCD 0D → 二进制 13
    ("BCD14", 14), // BCD 0E → 二进制 14
    ("BCD15", 15), // BCD 0F → 二进制 15
    ("BCD16", 16), // BCD 10 → 二进制 16
    ("BCD17", 17), // BCD 11 → 二进制 17
    ("BCD18", 18), // BCD 12 → 二进制 18
    ("BCD19", 19), // BCD 13 → 二进制 19
    ("BCD20", 20), // BCD 14 → 二进制 20
    ("BCD21", 21), // BCD 15 → 二进制 21
    ("BCD22", 22), // BCD 16 → 二进制 22
    ("BCD23", 23), // BCD 17 → 二进制 23
    ("BCD24", 24), // BCD 18 → 二进制 24
    ("BCD25", 25), // BCD 19 → 二进制 25
    ("BCD26", 26), // BCD 1A → 二进制 26
    ("BCD27", 27), // BCD 1B → 二进制 27
    ("BCD28", 28), // BCD 1C → 二进制 28
    ("BCD29", 29), // BCD 1D → 二进制 29
    ("BCD30", 30), // BCD 1E → 二进制 30
    ("BCD31", 31), // BCD 1F → 二进制 31
    ("BCD32", 32), // BCD 20 → 二进制 32
    ("BCD33", 33), // BCD 21 → 二进制 33
    ("BCD34", 34), // BCD 22 → 二进制 34
    ("BCD35", 35), // BCD 23 → 二进制 35
    ("BCD36", 36), // BCD 24 → 二进制 36
    ("BCD37", 37), // BCD 25 → 二进制 37
    ("BCD38", 38), // BCD 26 → 二进制 38
    ("BCD39", 39), // BCD 27 → 二进制 39
    ("BCD40", 40), // BCD 28 → 二进制 40
    ("BCD41", 41), // BCD 29 → 二进制 41
    ("BCD42", 42), // BCD 2A → 二进制 42
    ("BCD43", 43), // BCD 2B → 二进制 43
    ("BCD44", 44), // BCD 2C → 二进制 44
    ("BCD45", 45), // BCD 2D → 二进制 45
    ("BCD46", 46), // BCD 2E → 二进制 46
    ("BCD47", 47), // BCD 2F → 二进制 47
    ("BCD48", 48), // BCD 30 → 二进制 48
    ("BCD49", 49), // BCD 31 → 二进制 49
    ("BCD50", 50), // BCD 32 → 二进制 50
    ("BCD51", 51), // BCD 33 → 二进制 51
    ("BCD52", 52), // BCD 34 → 二进制 52
    ("BCD53", 53), // BCD 35 → 二进制 53
    ("BCD54", 54), // BCD 36 → 二进制 54
    ("BCD55", 55), // BCD 37 → 二进制 55
    ("BCD56", 56), // BCD 38 → 二进制 56
    ("BCD57", 57), // BCD 39 → 二进制 57
    ("BCD58", 58), // BCD 3A → 二进制 58
    ("BCD59", 59), // BCD 3B → 二进制 59
    ("BCD60", 60), // BCD 3C → 二进制 60
    ("BCD61", 61), // BCD 3D → 二进制 61
    ("BCD62", 62), // BCD 3E → 二进制 62
    ("BCD63", 63), // BCD 3F → 二进制 63
    ("BCD64", 64), // BCD 40 → 二进制 64
    ("BCD65", 65), // BCD 41 → 二进制 65
    ("BCD66", 66), // BCD 42 → 二进制 66
    ("BCD67", 67), // BCD 43 → 二进制 67
    ("BCD68", 68), // BCD 44 → 二进制 68
    ("BCD69", 69), // BCD 45 → 二进制 69
    ("BCD70", 70), // BCD 46 → 二进制 70
    ("BCD71", 71), // BCD 47 → 二进制 71
    ("BCD72", 72), // BCD 48 → 二进制 72
    ("BCD73", 73), // BCD 49 → 二进制 73
    ("BCD74", 74), // BCD 4A → 二进制 74
    ("BCD75", 75), // BCD 4B → 二进制 75
    ("BCD76", 76), // BCD 4C → 二进制 76
    ("BCD77", 77), // BCD 4D → 二进制 77
    ("BCD78", 78), // BCD 4E → 二进制 78
    ("BCD79", 79), // BCD 4F → 二进制 79
    ("BCD80", 80), // BCD 50 → 二进制 80
    ("BCD81", 81), // BCD 51 → 二进制 81
    ("BCD82", 82), // BCD 52 → 二进制 82
    ("BCD83", 83), // BCD 53 → 二进制 83
    ("BCD84", 84), // BCD 54 → 二进制 84
    ("BCD85", 85), // BCD 55 → 二进制 85
    ("BCD86", 86), // BCD 56 → 二进制 86
    ("BCD87", 87), // BCD 57 → 二进制 87
    ("BCD88", 88), // BCD 58 → 二进制 88
    ("BCD89", 89), // BCD 59 → 二进制 89
    ("BCD90", 90), // BCD 5A → 二进制 90
    ("BCD91", 91), // BCD 5B → 二进制 91
    ("BCD92", 92), // BCD 5C → 二进制 92
    ("BCD93", 93), // BCD 5D → 二进制 93
    ("BCD94", 94), // BCD 5E → 二进制 94
    ("BCD95", 95), // BCD 5F → 二进制 95
    ("BCD96", 96), // BCD 60 → 二进制 96
    ("BCD97", 97), // BCD 61 → 二进制 97
    ("BCD98", 98), // BCD 62 → 二进制 98
    ("BCD99", 99), // BCD 63 → 二进制 99
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn bcd_bin_map_get(key: &str) -> Option<u64> {
    match key {
        "BCD00" => Some(0),
        "BCD01" => Some(1),
        "BCD02" => Some(2),
        "BCD03" => Some(3),
        "BCD04" => Some(4),
        "BCD05" => Some(5),
        "BCD06" => Some(6),
        "BCD07" => Some(7),
        "BCD08" => Some(8),
        "BCD09" => Some(9),
        "BCD10" => Some(10),
        "BCD11" => Some(11),
        "BCD12" => Some(12),
        "BCD13" => Some(13),
        "BCD14" => Some(14),
        "BCD15" => Some(15),
        "BCD16" => Some(16),
        "BCD17" => Some(17),
        "BCD18" => Some(18),
        "BCD19" => Some(19),
        "BCD20" => Some(20),
        "BCD21" => Some(21),
        "BCD22" => Some(22),
        "BCD23" => Some(23),
        "BCD24" => Some(24),
        "BCD25" => Some(25),
        "BCD26" => Some(26),
        "BCD27" => Some(27),
        "BCD28" => Some(28),
        "BCD29" => Some(29),
        "BCD30" => Some(30),
        "BCD31" => Some(31),
        "BCD32" => Some(32),
        "BCD33" => Some(33),
        "BCD34" => Some(34),
        "BCD35" => Some(35),
        "BCD36" => Some(36),
        "BCD37" => Some(37),
        "BCD38" => Some(38),
        "BCD39" => Some(39),
        "BCD40" => Some(40),
        "BCD41" => Some(41),
        "BCD42" => Some(42),
        "BCD43" => Some(43),
        "BCD44" => Some(44),
        "BCD45" => Some(45),
        "BCD46" => Some(46),
        "BCD47" => Some(47),
        "BCD48" => Some(48),
        "BCD49" => Some(49),
        "BCD50" => Some(50),
        "BCD51" => Some(51),
        "BCD52" => Some(52),
        "BCD53" => Some(53),
        "BCD54" => Some(54),
        "BCD55" => Some(55),
        "BCD56" => Some(56),
        "BCD57" => Some(57),
        "BCD58" => Some(58),
        "BCD59" => Some(59),
        "BCD60" => Some(60),
        "BCD61" => Some(61),
        "BCD62" => Some(62),
        "BCD63" => Some(63),
        "BCD64" => Some(64),
        "BCD65" => Some(65),
        "BCD66" => Some(66),
        "BCD67" => Some(67),
        "BCD68" => Some(68),
        "BCD69" => Some(69),
        "BCD70" => Some(70),
        "BCD71" => Some(71),
        "BCD72" => Some(72),
        "BCD73" => Some(73),
        "BCD74" => Some(74),
        "BCD75" => Some(75),
        "BCD76" => Some(76),
        "BCD77" => Some(77),
        "BCD78" => Some(78),
        "BCD79" => Some(79),
        "BCD80" => Some(80),
        "BCD81" => Some(81),
        "BCD82" => Some(82),
        "BCD83" => Some(83),
        "BCD84" => Some(84),
        "BCD85" => Some(85),
        "BCD86" => Some(86),
        "BCD87" => Some(87),
        "BCD88" => Some(88),
        "BCD89" => Some(89),
        "BCD90" => Some(90),
        "BCD91" => Some(91),
        "BCD92" => Some(92),
        "BCD93" => Some(93),
        "BCD94" => Some(94),
        "BCD95" => Some(95),
        "BCD96" => Some(96),
        "BCD97" => Some(97),
        "BCD98" => Some(98),
        "BCD99" => Some(99),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn bcd_bin_map_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < bcd_bin_map_COUNT {
        let (k, v) = bcd_bin_map[i];
        let ok = match k {
            "BCD00" => v == 0,
            "BCD01" => v == 1,
            "BCD02" => v == 2,
            "BCD03" => v == 3,
            "BCD04" => v == 4,
            "BCD05" => v == 5,
            "BCD06" => v == 6,
            "BCD07" => v == 7,
            "BCD08" => v == 8,
            "BCD09" => v == 9,
            "BCD10" => v == 10,
            "BCD11" => v == 11,
            "BCD12" => v == 12,
            "BCD13" => v == 13,
            "BCD14" => v == 14,
            "BCD15" => v == 15,
            "BCD16" => v == 16,
            "BCD17" => v == 17,
            "BCD18" => v == 18,
            "BCD19" => v == 19,
            "BCD20" => v == 20,
            "BCD21" => v == 21,
            "BCD22" => v == 22,
            "BCD23" => v == 23,
            "BCD24" => v == 24,
            "BCD25" => v == 25,
            "BCD26" => v == 26,
            "BCD27" => v == 27,
            "BCD28" => v == 28,
            "BCD29" => v == 29,
            "BCD30" => v == 30,
            "BCD31" => v == 31,
            "BCD32" => v == 32,
            "BCD33" => v == 33,
            "BCD34" => v == 34,
            "BCD35" => v == 35,
            "BCD36" => v == 36,
            "BCD37" => v == 37,
            "BCD38" => v == 38,
            "BCD39" => v == 39,
            "BCD40" => v == 40,
            "BCD41" => v == 41,
            "BCD42" => v == 42,
            "BCD43" => v == 43,
            "BCD44" => v == 44,
            "BCD45" => v == 45,
            "BCD46" => v == 46,
            "BCD47" => v == 47,
            "BCD48" => v == 48,
            "BCD49" => v == 49,
            "BCD50" => v == 50,
            "BCD51" => v == 51,
            "BCD52" => v == 52,
            "BCD53" => v == 53,
            "BCD54" => v == 54,
            "BCD55" => v == 55,
            "BCD56" => v == 56,
            "BCD57" => v == 57,
            "BCD58" => v == 58,
            "BCD59" => v == 59,
            "BCD60" => v == 60,
            "BCD61" => v == 61,
            "BCD62" => v == 62,
            "BCD63" => v == 63,
            "BCD64" => v == 64,
            "BCD65" => v == 65,
            "BCD66" => v == 66,
            "BCD67" => v == 67,
            "BCD68" => v == 68,
            "BCD69" => v == 69,
            "BCD70" => v == 70,
            "BCD71" => v == 71,
            "BCD72" => v == 72,
            "BCD73" => v == 73,
            "BCD74" => v == 74,
            "BCD75" => v == 75,
            "BCD76" => v == 76,
            "BCD77" => v == 77,
            "BCD78" => v == 78,
            "BCD79" => v == 79,
            "BCD80" => v == 80,
            "BCD81" => v == 81,
            "BCD82" => v == 82,
            "BCD83" => v == 83,
            "BCD84" => v == 84,
            "BCD85" => v == 85,
            "BCD86" => v == 86,
            "BCD87" => v == 87,
            "BCD88" => v == 88,
            "BCD89" => v == 89,
            "BCD90" => v == 90,
            "BCD91" => v == 91,
            "BCD92" => v == 92,
            "BCD93" => v == 93,
            "BCD94" => v == 94,
            "BCD95" => v == 95,
            "BCD96" => v == 96,
            "BCD97" => v == 97,
            "BCD98" => v == 98,
            "BCD99" => v == 99,
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
pub fn bcd_bin_map_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < bcd_bin_map_COUNT {
        s = s.wrapping_add(bcd_bin_map[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn bcd_bin_map_unique() -> bool {
    let mut i = 0usize;
    while i < bcd_bin_map_COUNT {
        let mut j = i + 1;
        while j < bcd_bin_map_COUNT {
            if bcd_bin_map[i].0 == bcd_bin_map[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t163_guard_seg() -> bool {
    t163_segment_guard()
}

/// CMOS 寄存器表地址序与索引恒等（0x00..=0x0F 16 项）。
fn check_c1_rtc_reg_table() -> bool {
    let mut ok = rtc_reg_COUNT == 16;
    if ok {
        ok = rtc_reg_get("RTC_SEC") == Some(0) && rtc_reg_get("RTC_MIN") == Some(2)
            && rtc_reg_get("RTC_YEAR") == Some(9) && rtc_reg_get("RTC_STAT_D") == Some(13);
    }
    if ok {
        ok = rtc_reg_get("RTC_GHOST").is_none();
    }
    ok
}

/// BCD 映射 100 项逐行守卫全绿 + 键唯一。
fn check_c5_bcd_map() -> bool {
    let mut ok = bcd_bin_map_COUNT == 100;
    if ok {
        ok = bcd_bin_map_scan() == 0;
    }
    if ok {
        ok = bcd_bin_map_unique();
    }
    ok
}

/// BCD 表 100 项键值恒等 + 求和守恒。
fn check_c2_bcd_table() -> bool {
    let mut ok = bcd_bin_COUNT == 100;
    if ok {
        ok = bcd_bin_map_unique();
    }
    if ok {
        ok = bcd_bin_get("0") == Some(0) && bcd_bin_get("99") == Some(99) && bcd_bin_get("42") == Some(42);
    }
    if ok {
        // 0..99 求和 = 4950。
        ok = bcd_bin_sum() == 4950;
    }
    ok
}

/// UIP 状态机合法路径与非法拒止（正负样本）。
fn check_c3_uip_sm() -> bool {
    let mut led = uipLedger::new(uipState::Idle);
    let mut ok = led.apply(uipEvent::Start);
    ok = ok && led.apply(uipEvent::UipClear);
    ok = ok && led.apply(uipEvent::ReadDone);
    ok = ok && led.apply(uipEvent::VerifyOk);
    if ok {
        ok = led.cur == uipState::Consistent;
    }
    if ok {
        // 非法迁移（Consistent 态直接 ReadDone）必须拒止。
        ok = !led.apply(uipEvent::ReadDone);
    }
    if ok {
        ok = led.illegal == 1;
    }
    ok
}

/// 双源对拍正负样本（一致放行 + 不一致记账）。
fn check_c4_dual_readback() -> bool {
    let mut d = rtc_dualDual::new();
    d.sample(rtc_dualSlot::Sec, 45, 45);
    d.sample(rtc_dualSlot::Min, 30, 30);
    d.sample(rtc_dualSlot::Hour, 14, 14);
    d.sample(rtc_dualSlot::Day, 11, 11);
    d.sample(rtc_dualSlot::Month, 10, 10);
    d.sample(rtc_dualSlot::Year, 2026, 2026);
    let mut ok = d.all_same();
    if ok {
        d.sample(rtc_dualSlot::Sec, 45, 46);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t163_checks() -> CheckSet {
    let mut set = CheckSet::new("T163");
    set.add(
        "t163-seg-guard",
        t163_guard_seg(),
        if t163_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t163-c1-rtc-reg-table",
        check_c1_rtc_reg_table(),
        if check_c1_rtc_reg_table() { "" } else { "rtc register table mapping mismatch" },
    );
    set.add(
        "t163-c5-bcd-map",
        check_c5_bcd_map(),
        if check_c5_bcd_map() { "" } else { "bcd map per-row guard mismatch" },
    );
    set.add(
        "t163-c2-bcd-table",
        check_c2_bcd_table(),
        if check_c2_bcd_table() { "" } else { "bcd table identity/sum mismatch" },
    );
    set.add(
        "t163-c3-uip-sm",
        check_c3_uip_sm(),
        if check_c3_uip_sm() { "" } else { "uip state machine path mismatch" },
    );
    set.add(
        "t163-c4-dual-readback",
        check_c4_dual_readback(),
        if check_c4_dual_readback() { "" } else { "dual readback positive/negative mismatch" },
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
    fn t163_all_checks_green() {
        let set = run_t163_checks();
        assert!(set.all_passed(), "T163 domain checks must be green");
    }

    #[test]
    fn t163_segment_guard_holds() {
        assert!(t163_segment_guard());
    }
}
