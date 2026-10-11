//! unxt591_loglevel —— UNX-F0541 四级别枚举冻结与优先级。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0541 · 日志面总装（emit 原语） · 判据：日志原语 emit（级别/组件/格式化文）接入核（10/10 用例全链），四级别枚举冻结
//! · 组件：四级别表（枚举+优先级+掩码）+ 冻结验证 + 级别合成
//! · 判据落地：四级别枚举冻结
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T591_VERSION: &str = "t591-loglevel-v1";

/// 诊断段基址。
pub const T591_SEG_BASE: u16 = 0x4800;

// --------------------------------------------------------------------------
// 诊断码（t591 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 日志级别诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t591Code {
    /// 0x4800 级别非法（四级之外）。
    LevelBad,
    /// 0x4801 枚举未冻结。
    LevelUnfrozen,
    /// 0x4802 优先级非法。
    PriorityBad,
    /// 0x4803 掩码非法（非四级位组合）。
    MaskBad,
    /// 0x4804 过滤下沉（高级别被吞）。
    FilterUnder,
    /// 0x4805 级别账破缺。
    LedgerBreak,
    /// 0x4806 级别序断档。
    OrderBreak,
    /// 0x4807 守卫触发。
    GuardTripped,
    /// 0x4808 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4809 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x480a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x480b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x480c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x480d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x480e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t591Code {
    /// 全量闭集。
    pub const ALL: [t591Code; 15] = [
        t591Code::LevelBad,
        t591Code::LevelUnfrozen,
        t591Code::PriorityBad,
        t591Code::MaskBad,
        t591Code::FilterUnder,
        t591Code::LedgerBreak,
        t591Code::OrderBreak,
        t591Code::GuardTripped,
        t591Code::LedgerBroken,
        t591Code::SnapshotFingerprint,
        t591Code::TableBroken,
        t591Code::IndexMisaligned,
        t591Code::DuplicateEntry,
        t591Code::OrderViolated,
        t591Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t591Code::LevelBad => 0x4800,
            t591Code::LevelUnfrozen => 0x4801,
            t591Code::PriorityBad => 0x4802,
            t591Code::MaskBad => 0x4803,
            t591Code::FilterUnder => 0x4804,
            t591Code::LedgerBreak => 0x4805,
            t591Code::OrderBreak => 0x4806,
            t591Code::GuardTripped => 0x4807,
            t591Code::LedgerBroken => 0x4808,
            t591Code::SnapshotFingerprint => 0x4809,
            t591Code::TableBroken => 0x480a,
            t591Code::IndexMisaligned => 0x480b,
            t591Code::DuplicateEntry => 0x480c,
            t591Code::OrderViolated => 0x480d,
            t591Code::SegmentGuard => 0x480e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t591Code::LevelBad => "级别非法（四级之外）",
            t591Code::LevelUnfrozen => "枚举未冻结",
            t591Code::PriorityBad => "优先级非法",
            t591Code::MaskBad => "掩码非法（非四级位组合）",
            t591Code::FilterUnder => "过滤下沉（高级别被吞）",
            t591Code::LedgerBreak => "级别账破缺",
            t591Code::OrderBreak => "级别序断档",
            t591Code::GuardTripped => "守卫触发",
            t591Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t591Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t591Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t591Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t591Code::DuplicateEntry => "条目重复（同键双写）",
            t591Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t591Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t591Code::LevelBad => "按四级闭集校正",
            t591Code::LevelUnfrozen => "按冻结规程封版",
            t591Code::PriorityBad => "按优先级表校正",
            t591Code::MaskBad => "按掩码律校正",
            t591Code::FilterUnder => "按过滤阈值校正",
            t591Code::LedgerBreak => "按守恒重建",
            t591Code::OrderBreak => "按序重排",
            t591Code::GuardTripped => "检查临界区",
            t591Code::LedgerBroken => "按守恒律重建台账后重跑",
            t591Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t591Code::TableBroken => "按语义规则重建常量表",
            t591Code::IndexMisaligned => "按声明序归一化后重跑",
            t591Code::DuplicateEntry => "合并同键条目为末次生效",
            t591Code::OrderViolated => "按依赖矩阵重排后重跑",
            t591Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 四级别枚举（ERROR/WARN/INFO/DEBUG）+ 优先级与掩码
// ---------------------------------------------------------------------------

/// 日志级别（闭集四级，数值即严重度：值越小越严重）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    /// 错误（致命路径）。"
    Error,
    /// 警告（可恢复异常）。"
    Warn,
    /// 信息（正常运行面）。"
    Info,
    /// 调试（开发期明细）。"
    Debug,
}

impl LogLevel {
    /// 全量闭集（按严重度排序）。"
    pub const ALL: [LogLevel; 4] = [LogLevel::Error, LogLevel::Warn, LogLevel::Info, LogLevel::Debug];

    /// 数值（0..3，即优先级序号）。"
    pub const fn num(self) -> u8 {
        match self {
            LogLevel::Error => 0,
            LogLevel::Warn => 1,
            LogLevel::Info => 2,
            LogLevel::Debug => 3,
        }
    }

    /// 中文名。"
    pub const fn cn(self) -> &'static str {
        match self {
            LogLevel::Error => "错误",
            LogLevel::Warn => "警告",
            LogLevel::Info => "信息",
            LogLevel::Debug => "调试",
        }
    }

    /// 位掩码（四级各占一比特）。"
    pub const fn mask(self) -> u32 {
        1u32 << self.num()
    }
}

/// 级别数常数（冻结值）。"
pub const LOG_LEVEL_N: usize = 4;

/// 级别冻结校验（闭集大小 + 序号连续性）。"
pub const fn log_levels_frozen() -> bool {
    LogLevel::ALL.len() == LOG_LEVEL_N
        && LogLevel::Error.num() == 0
        && LogLevel::Debug.num() == 3
        && LogLevel::Error.mask() == 1
        && LogLevel::Debug.mask() == 8
}

/// 过滤判定（阈值级别及以上方可通过：级别数值 <= 阈值）。"
pub const fn log_passes(level: LogLevel, threshold: LogLevel) -> bool {
    level.num() <= threshold.num()
}

/// 组合掩码（多级并集；只接受合法掩码）。"
pub const fn log_combine(mask: u32) -> Option<u32> {
    let legal = 1 | 2 | 4 | 8;
    if mask & !legal != 0 {
        None
    } else {
        Some(mask & legal)
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4800, 0x480f]。
pub const fn t591_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t591Code::ALL.len() {
        let c = t591Code::ALL[i].code();
        if c < 0x4800 || c > 0x480f {
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
fn t591_guard_seg() -> bool {
    t591_segment_guard()
}

/// 四级别冻结（闭集/序号/掩码三查）。
fn check_c1_frozen() -> bool {
    let mut ok = log_levels_frozen();
    if ok {
        ok = LogLevel::ALL.len() == 4 && LogLevel::Info.num() == 2;
    }
    ok
}

/// 过滤阈值 + 掩码合法性（正负样本）。
fn check_c2_filter() -> bool {
    let mut ok = log_passes(LogLevel::Error, LogLevel::Warn);
    if ok {
        // INFO 不低于 WARN 阈值 → 拒。
        ok = !log_passes(LogLevel::Info, LogLevel::Warn);
    }
    if ok {
        ok = log_combine(0xF) == Some(0xF);
    }
    if ok {
        // 非法比特位必须拒。
        ok = log_combine(0x10).is_none();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t591_checks() -> CheckSet {
    let mut set = CheckSet::new("T591");
    set.add(
        "t591-seg-guard",
        t591_guard_seg(),
        if t591_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t591-c1-frozen",
        check_c1_frozen(),
        if check_c1_frozen() { "" } else { "log level freeze mismatch" },
    );
    set.add(
        "t591-c2-filter",
        check_c2_filter(),
        if check_c2_filter() { "" } else { "log filter/mask mismatch" },
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
    fn t591_all_checks_green() {
        let set = run_t591_checks();
        assert!(set.all_passed(), "T591 domain checks must be green");
    }

    #[test]
    fn t591_segment_guard_holds() {
        assert!(t591_segment_guard());
    }
}
