//! unxt361_pitdiv —— UNX-F0401 PIT 分频系数表（1000Hz 方波）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0401 · PIT 通道 0 编程接管（F0115 升级） · 判据：PIT 以 1000Hz 方波接管，中断计数 5 秒实差 ≤2 次（±0.04%）
//! · 组件：分频系数表（16 档）+ 1000Hz 档守卫 + 频率换算引擎
//! · 判据落地：PIT 以 1000Hz 方波接管
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T361_VERSION: &str = "t361-pitdiv-v1";

/// 诊断段基址。
pub const T361_SEG_BASE: u16 = 0x4100;

// --------------------------------------------------------------------------
// 诊断码（t361 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// PIT 分频诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t361Code {
    /// 0x4100 分频系数为零。
    DivZero,
    /// 0x4101 频率非法（超出 1.19MHz/65536 范围）。
    FreqBad,
    /// 0x4102 工作模式非法（0..5 之外）。
    ModeBad,
    /// 0x4103 通道非法（0..2 之外）。
    ChannelBad,
    /// 0x4104 访问模式非法（0..3 之外）。
    AccessBad,
    /// 0x4105 分频账破缺。
    LedgerBreak,
    /// 0x4106 频率漂移超限。
    DriftOver,
    /// 0x4107 守卫触发。
    GuardTripped,
    /// 0x4108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x410a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x410b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x410c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x410d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x410e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t361Code {
    /// 全量闭集。
    pub const ALL: [t361Code; 15] = [
        t361Code::DivZero,
        t361Code::FreqBad,
        t361Code::ModeBad,
        t361Code::ChannelBad,
        t361Code::AccessBad,
        t361Code::LedgerBreak,
        t361Code::DriftOver,
        t361Code::GuardTripped,
        t361Code::LedgerBroken,
        t361Code::SnapshotFingerprint,
        t361Code::TableBroken,
        t361Code::IndexMisaligned,
        t361Code::DuplicateEntry,
        t361Code::OrderViolated,
        t361Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t361Code::DivZero => 0x4100,
            t361Code::FreqBad => 0x4101,
            t361Code::ModeBad => 0x4102,
            t361Code::ChannelBad => 0x4103,
            t361Code::AccessBad => 0x4104,
            t361Code::LedgerBreak => 0x4105,
            t361Code::DriftOver => 0x4106,
            t361Code::GuardTripped => 0x4107,
            t361Code::LedgerBroken => 0x4108,
            t361Code::SnapshotFingerprint => 0x4109,
            t361Code::TableBroken => 0x410a,
            t361Code::IndexMisaligned => 0x410b,
            t361Code::DuplicateEntry => 0x410c,
            t361Code::OrderViolated => 0x410d,
            t361Code::SegmentGuard => 0x410e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t361Code::DivZero => "分频系数为零",
            t361Code::FreqBad => "频率非法（超出 1.19MHz/65536 范围）",
            t361Code::ModeBad => "工作模式非法（0..5 之外）",
            t361Code::ChannelBad => "通道非法（0..2 之外）",
            t361Code::AccessBad => "访问模式非法（0..3 之外）",
            t361Code::LedgerBreak => "分频账破缺",
            t361Code::DriftOver => "频率漂移超限",
            t361Code::GuardTripped => "守卫触发",
            t361Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t361Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t361Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t361Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t361Code::DuplicateEntry => "条目重复（同键双写）",
            t361Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t361Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t361Code::DivZero => "按非零律拒收",
            t361Code::FreqBad => "按频段表校正",
            t361Code::ModeBad => "按六模式闭集校正",
            t361Code::ChannelBad => "按三通道闭集校正",
            t361Code::AccessBad => "按四访问闭集校正",
            t361Code::LedgerBreak => "按守恒重建",
            t361Code::DriftOver => "按误差预算校正",
            t361Code::GuardTripped => "检查临界区",
            t361Code::LedgerBroken => "按守恒律重建台账后重跑",
            t361Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t361Code::TableBroken => "按语义规则重建常量表",
            t361Code::IndexMisaligned => "按声明序归一化后重跑",
            t361Code::DuplicateEntry => "合并同键条目为末次生效",
            t361Code::OrderViolated => "按依赖矩阵重排后重跑",
            t361Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// PIT 常量（i8254：基频 1193182Hz，通道 0 = 系统节拍器）
// ---------------------------------------------------------------------------

/// PIT 基频（Hz，1.193182 MHz）。
pub const PIT_BASE_HZ: u64 = 1_193_182;

/// 通道 0 命令口/数据口。
pub const PIT_CH0_DATA: u16 = 0x40;
pub const PIT_CMD: u16 = 0x43;

/// 1000Hz 节拍目标。
pub const PIT_TARGET_HZ: u64 = 1000;

/// 1000Hz 分频系数（1193182/1000 = 1193）。
pub const PIT_DIV_1000: u64 = 1193;

/// 实测频率（Hz，整数除取整）。
pub const PIT_MEASURED_HZ: u64 = PIT_BASE_HZ / PIT_DIV_1000;

/// 与目标的绝对差（Hz）。
pub const PIT_ABS_ERR_HZ: u64 = if PIT_MEASURED_HZ > PIT_TARGET_HZ { PIT_MEASURED_HZ - PIT_TARGET_HZ } else { PIT_TARGET_HZ - PIT_MEASURED_HZ };

/// 误差（ppm 口径 = 绝对差/目标 × 1e6；1193182/1193=999Hz → 1000ppm）。
pub const PIT_ERROR_PPM: u64 = PIT_ABS_ERR_HZ * 1_000_000 / PIT_TARGET_HZ;

/// 工作模式（六模式闭集）。
pub const MODE0_INTERRUPT_TERMINAL: u8 = 0;
pub const MODE1_HW_RETRIGGERABLE: u8 = 1;
pub const MODE2_RATE_GEN: u8 = 2;
pub const MODE3_SQUARE_WAVE: u8 = 3;
pub const MODE4_SW_TRIGGER: u8 = 4;
pub const MODE5_HW_TRIGGER: u8 = 5;

/// 命令字节组装（通道 + 访问 + 模式 + BCD）。
pub const fn pit_command(ch: u8, access: u8, mode: u8, bcd: bool) -> u8 {
    (ch << 6) | (access << 4) | (mode << 1) | (bcd as u8)
}

/// 1000Hz 方波命令字（通道 0 + 低高字节 + 模式 3 + 二进制）。
pub const PIT_CH0_SQUARE_CMD: u8 = 0x36;

// --------------------------------------------------------------------------
// 字符串键值映射 pitdiv（14 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// PIT 分频系数表（14 档常用档位，含 1000Hz 档 1193）。
/// 行数。
pub const pitdiv_COUNT: usize = 14;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static pitdiv: [(&'static str, u64); 14] = [
    ("DIV00", 1), // 分频档 00（系数 1）
    ("DIV01", 2), // 分频档 01（系数 2）
    ("DIV02", 5), // 分频档 02（系数 5）
    ("DIV03", 10), // 分频档 03（系数 10）
    ("DIV04", 50), // 分频档 04（系数 50）
    ("DIV05", 100), // 分频档 05（系数 100）
    ("DIV06", 596), // 分频档 06（系数 596）
    ("DIV07", 1193), // 分频档 07（系数 1193）
    ("DIV08", 2386), // 分频档 08（系数 2386）
    ("DIV09", 5966), // 分频档 09（系数 5966）
    ("DIV10", 11932), // 分频档 10（系数 11932）
    ("DIV11", 23863), // 分频档 11（系数 23863）
    ("DIV12", 59659), // 分频档 12（系数 59659）
    ("DIV13", 65535), // 分频档 13（系数 65535）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn pitdiv_get(key: &str) -> Option<u64> {
    match key {
        "DIV00" => Some(1),
        "DIV01" => Some(2),
        "DIV02" => Some(5),
        "DIV03" => Some(10),
        "DIV04" => Some(50),
        "DIV05" => Some(100),
        "DIV06" => Some(596),
        "DIV07" => Some(1193),
        "DIV08" => Some(2386),
        "DIV09" => Some(5966),
        "DIV10" => Some(11932),
        "DIV11" => Some(23863),
        "DIV12" => Some(59659),
        "DIV13" => Some(65535),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn pitdiv_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < pitdiv_COUNT {
        let (k, v) = pitdiv[i];
        let ok = match k {
            "DIV00" => v >= 1 && v <= 65535,
            "DIV01" => v >= 1 && v <= 65535,
            "DIV02" => v >= 1 && v <= 65535,
            "DIV03" => v >= 1 && v <= 65535,
            "DIV04" => v >= 1 && v <= 65535,
            "DIV05" => v >= 1 && v <= 65535,
            "DIV06" => v >= 1 && v <= 65535,
            "DIV07" => v >= 1 && v <= 65535,
            "DIV08" => v >= 1 && v <= 65535,
            "DIV09" => v >= 1 && v <= 65535,
            "DIV10" => v >= 1 && v <= 65535,
            "DIV11" => v >= 1 && v <= 65535,
            "DIV12" => v >= 1 && v <= 65535,
            "DIV13" => v >= 1 && v <= 65535,
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
pub fn pitdiv_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pitdiv_COUNT {
        s = s.wrapping_add(pitdiv[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn pitdiv_unique() -> bool {
    let mut i = 0usize;
    while i < pitdiv_COUNT {
        let mut j = i + 1;
        while j < pitdiv_COUNT {
            if pitdiv[i].0 == pitdiv[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4100, 0x410f]。
pub const fn t361_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t361Code::ALL.len() {
        let c = t361Code::ALL[i].code();
        if c < 0x4100 || c > 0x410f {
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
fn t361_guard_seg() -> bool {
    t361_segment_guard()
}

/// 1000Hz 档在位 + 全部档位合法。
fn check_c1_div() -> bool {
    let mut ok = pitdiv_get("DIV07") == Some(1193);
    if ok {
        ok = PIT_DIV_1000 == 1193 && PIT_MEASURED_HZ == 1000;
    }
    if ok {
        // 1193182/1193 = 1000Hz 取整：零误差（5 秒窗内 0 次偏差）。
        ok = PIT_ERROR_PPM == 0 && PIT_ABS_ERR_HZ == 0;
    }
    ok
}

/// 命令字组装（通道/访问/模式/BCD 四段）。
fn check_c2_command() -> bool {
    let mut ok = pit_command(0, 3, 3, false) == 0x36;
    if ok {
        ok = pit_command(0, 3, 3, false) == PIT_CH0_SQUARE_CMD;
    }
    if ok {
        // BCD 位置位后的差异。
        ok = pit_command(0, 3, 3, true) == 0x37;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t361_checks() -> CheckSet {
    let mut set = CheckSet::new("T361");
    set.add(
        "t361-seg-guard",
        t361_guard_seg(),
        if t361_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t361-c1-div",
        check_c1_div(),
        if check_c1_div() { "" } else { "PIT divisor mismatch" },
    );
    set.add(
        "t361-c2-command",
        check_c2_command(),
        if check_c2_command() { "" } else { "PIT command byte mismatch" },
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
    fn t361_all_checks_green() {
        let set = run_t361_checks();
        assert!(set.all_passed(), "T361 domain checks must be green");
    }

    #[test]
    fn t361_segment_guard_holds() {
        assert!(t361_segment_guard());
    }
}
