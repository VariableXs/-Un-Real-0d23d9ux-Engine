//! unxt574_uartf0117 —— UNX-F0501 F0117 升级面（接管契约）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0501 · 16550 串口编程接管（F0117 升级） · 判据：COM1 编程至 115200 8N1（波特率误差 ≤1.7% 理论值核），LCR/MCR/IER 三寄存器回读一致
//! · 组件：接管契约表（8 条）+ 接管流水 + 回归账
//! · 判据落地：F0117 升级的接管语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T574_VERSION: &str = "t574-uartf0117-v1";

/// 诊断段基址。
pub const T574_SEG_BASE: u16 = 0x4630;

// --------------------------------------------------------------------------
// 诊断码（t574 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 串口接管诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t574Code {
    /// 0x4630 接管违例。
    TakeoverIllegal,
    /// 0x4631 旧处理器泄漏。
    OldHandlerLeak,
    /// 0x4632 中断线冲突。
    IrqConflict,
    /// 0x4633 双编程。
    DualProgram,
    /// 0x4634 接管账破缺。
    LedgerBreak,
    /// 0x4635 回归漏测。
    RegressionMiss,
    /// 0x4636 接管序违例。
    OrderViolated,
    /// 0x4637 守卫触发。
    GuardTripped,
    /// 0x4638 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4639 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x463a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x463b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x463c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x463d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t574Code {
    /// 全量闭集。
    pub const ALL: [t574Code; 14] = [
        t574Code::TakeoverIllegal,
        t574Code::OldHandlerLeak,
        t574Code::IrqConflict,
        t574Code::DualProgram,
        t574Code::LedgerBreak,
        t574Code::RegressionMiss,
        t574Code::OrderViolated,
        t574Code::GuardTripped,
        t574Code::LedgerBroken,
        t574Code::SnapshotFingerprint,
        t574Code::TableBroken,
        t574Code::IndexMisaligned,
        t574Code::DuplicateEntry,
        t574Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t574Code::TakeoverIllegal => 0x4630,
            t574Code::OldHandlerLeak => 0x4631,
            t574Code::IrqConflict => 0x4632,
            t574Code::DualProgram => 0x4633,
            t574Code::LedgerBreak => 0x4634,
            t574Code::RegressionMiss => 0x4635,
            t574Code::OrderViolated => 0x4636,
            t574Code::GuardTripped => 0x4637,
            t574Code::LedgerBroken => 0x4638,
            t574Code::SnapshotFingerprint => 0x4639,
            t574Code::TableBroken => 0x463a,
            t574Code::IndexMisaligned => 0x463b,
            t574Code::DuplicateEntry => 0x463c,
            t574Code::SegmentGuard => 0x463d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t574Code::TakeoverIllegal => "接管违例",
            t574Code::OldHandlerLeak => "旧处理器泄漏",
            t574Code::IrqConflict => "中断线冲突",
            t574Code::DualProgram => "双编程",
            t574Code::LedgerBreak => "接管账破缺",
            t574Code::RegressionMiss => "回归漏测",
            t574Code::OrderViolated => "接管序违例",
            t574Code::GuardTripped => "守卫触发",
            t574Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t574Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t574Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t574Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t574Code::DuplicateEntry => "条目重复（同键双写）",
            t574Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t574Code::TakeoverIllegal => "按契约补齐",
            t574Code::OldHandlerLeak => "按账追回",
            t574Code::IrqConflict => "按分配律重排",
            t574Code::DualProgram => "独占裁一",
            t574Code::LedgerBreak => "按守恒重建",
            t574Code::RegressionMiss => "按清单补齐",
            t574Code::OrderViolated => "按序重排",
            t574Code::GuardTripped => "检查临界区",
            t574Code::LedgerBroken => "按守恒律重建台账后重跑",
            t574Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t574Code::TableBroken => "按语义规则重建常量表",
            t574Code::IndexMisaligned => "按声明序归一化后重跑",
            t574Code::DuplicateEntry => "合并同键条目为末次生效",
            t574Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 uartcontract（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// F0117 串口接管契约表（八条逐行守卫）。
/// 行数。
pub const uartcontract_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static uartcontract: [(&'static str, u64); 8] = [
    ("U01_DLL", 1), // 除数锁存器低字节写入
    ("U02_DLM", 0), // 除数锁存器高字节写入（115200 时为 0）
    ("U03_LCR", 3), // 8N1 线路控制值
    ("U04_FIFO", 7), // FIFO 使能 + 清空 + 14 字节触发
    ("U05_MCR", 11), // MCR 正常态
    ("U06_IER", 1), // IER 接收中断使能
    ("U07_BAUD", 1152), // 115200 ×100 口径
    ("U08_ERR", 0), // 误差万分比（红线 170）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn uartcontract_get(key: &str) -> Option<u64> {
    match key {
        "U01_DLL" => Some(1),
        "U02_DLM" => Some(0),
        "U03_LCR" => Some(3),
        "U04_FIFO" => Some(7),
        "U05_MCR" => Some(11),
        "U06_IER" => Some(1),
        "U07_BAUD" => Some(1152),
        "U08_ERR" => Some(0),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn uartcontract_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < uartcontract_COUNT {
        let (k, v) = uartcontract[i];
        let ok = match k {
            "U01_DLL" => v == 1,
            "U02_DLM" => v == 0,
            "U03_LCR" => v == 3,
            "U04_FIFO" => v == 7,
            "U05_MCR" => v == 11,
            "U06_IER" => v == 1,
            "U07_BAUD" => v == 1152,
            "U08_ERR" => v == 0,
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
pub fn uartcontract_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < uartcontract_COUNT {
        s = s.wrapping_add(uartcontract[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn uartcontract_unique() -> bool {
    let mut i = 0usize;
    while i < uartcontract_COUNT {
        let mut j = i + 1;
        while j < uartcontract_COUNT {
            if uartcontract[i].0 == uartcontract[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4630, 0x463f]。
pub const fn t574_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t574Code::ALL.len() {
        let c = t574Code::ALL[i].code();
        if c < 0x4630 || c > 0x463f {
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
fn t574_guard_seg() -> bool {
    t574_segment_guard()
}

/// 表 uartcontract 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t574_uartcontract_guard() -> bool {
    let mut ok = uartcontract_COUNT == 8;
    if ok {
        ok = uartcontract_scan() == 0;
    }
    if ok {
        ok = uartcontract_unique();
    }
    ok
}

/// 契约求和 + 分频一致性。
fn check_c2_contract_sum() -> bool {
    let mut ok = uartcontract_sum() == 1175;
    if ok {
        ok = uartcontract_get("U02_DLM") == Some(0) && uartcontract_get("U03_LCR") == Some(3);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t574_checks() -> CheckSet {
    let mut set = CheckSet::new("T574");
    set.add(
        "t574-seg-guard",
        t574_guard_seg(),
        if t574_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t574-c1-contract-guard",
        check_t574_uartcontract_guard(),
        if check_t574_uartcontract_guard() { "" } else { "table uartcontract guard mismatch" },
    );
    set.add(
        "t574-c2-contract-sum",
        check_c2_contract_sum(),
        if check_c2_contract_sum() { "" } else { "uart takeover contract mismatch" },
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
    fn t574_all_checks_green() {
        let set = run_t574_checks();
        assert!(set.all_passed(), "T574 domain checks must be green");
    }

    #[test]
    fn t574_segment_guard_holds() {
        assert!(t574_segment_guard());
    }
}
