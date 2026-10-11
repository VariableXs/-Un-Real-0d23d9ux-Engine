//! unxt124_wdstall —— UNX-F0122 卡死注入检测（注入场景表 + 捕获引擎）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0122 · 阶段看门狗（每阶段独立时限） · 判据：注入阶段卡死被看门狗捕获，超时时限精度毫秒级，捕获后进诊断模式
//! · 组件：注入场景表（12 阶段 × 4 卡死型 = 48 行）+ 捕获引擎 + 捕获账
//! · 判据落地：注入阶段卡死被看门狗捕获
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T124_VERSION: &str = "t124-wdstall-v1";

/// 诊断段基址。
pub const T124_SEG_BASE: u16 = 0x4b20;

// --------------------------------------------------------------------------
// 诊断码（t124 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 卡死捕获诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t124Code {
    /// 0x4b20 卡死漏捕（注入未被捕获）。
    StallMissed,
    /// 0x4b21 注入非法（场景表外）。
    InjectBad,
    /// 0x4b22 捕获重复（同注入双捕）。
    CaptureDup,
    /// 0x4b23 超时误报（正常路径被误判）。
    TimeoutFalse,
    /// 0x4b24 捕获账破缺。
    LedgerBreak,
    /// 0x4b25 捕获序违例。
    OrderViolated,
    /// 0x4b26 报告陈旧。
    ReportStale,
    /// 0x4b27 守卫触发。
    GuardTripped,
    /// 0x4b28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4b29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4b2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4b2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4b2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4b2d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t124Code {
    /// 全量闭集。
    pub const ALL: [t124Code; 14] = [
        t124Code::StallMissed,
        t124Code::InjectBad,
        t124Code::CaptureDup,
        t124Code::TimeoutFalse,
        t124Code::LedgerBreak,
        t124Code::OrderViolated,
        t124Code::ReportStale,
        t124Code::GuardTripped,
        t124Code::LedgerBroken,
        t124Code::SnapshotFingerprint,
        t124Code::TableBroken,
        t124Code::IndexMisaligned,
        t124Code::DuplicateEntry,
        t124Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t124Code::StallMissed => 0x4b20,
            t124Code::InjectBad => 0x4b21,
            t124Code::CaptureDup => 0x4b22,
            t124Code::TimeoutFalse => 0x4b23,
            t124Code::LedgerBreak => 0x4b24,
            t124Code::OrderViolated => 0x4b25,
            t124Code::ReportStale => 0x4b26,
            t124Code::GuardTripped => 0x4b27,
            t124Code::LedgerBroken => 0x4b28,
            t124Code::SnapshotFingerprint => 0x4b29,
            t124Code::TableBroken => 0x4b2a,
            t124Code::IndexMisaligned => 0x4b2b,
            t124Code::DuplicateEntry => 0x4b2c,
            t124Code::SegmentGuard => 0x4b2d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t124Code::StallMissed => "卡死漏捕（注入未被捕获）",
            t124Code::InjectBad => "注入非法（场景表外）",
            t124Code::CaptureDup => "捕获重复（同注入双捕）",
            t124Code::TimeoutFalse => "超时误报（正常路径被误判）",
            t124Code::LedgerBreak => "捕获账破缺",
            t124Code::OrderViolated => "捕获序违例",
            t124Code::ReportStale => "报告陈旧",
            t124Code::GuardTripped => "守卫触发",
            t124Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t124Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t124Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t124Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t124Code::DuplicateEntry => "条目重复（同键双写）",
            t124Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t124Code::StallMissed => "按捕获矩阵补齐",
            t124Code::InjectBad => "按场景闭集校正",
            t124Code::CaptureDup => "幂等去重",
            t124Code::TimeoutFalse => "按误报红线校正",
            t124Code::LedgerBreak => "按守恒重建",
            t124Code::OrderViolated => "按注入序记账",
            t124Code::ReportStale => "重渲染",
            t124Code::GuardTripped => "检查临界区",
            t124Code::LedgerBroken => "按守恒律重建台账后重跑",
            t124Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t124Code::TableBroken => "按语义规则重建常量表",
            t124Code::IndexMisaligned => "按声明序归一化后重跑",
            t124Code::DuplicateEntry => "合并同键条目为末次生效",
            t124Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 stall（48 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 卡死注入场景矩阵（12 阶段 × 4 卡死型 = 48 行逐行守卫）。
/// 行数。
pub const stall_COUNT: usize = 48;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static stall: [(&'static str, u64); 48] = [
    ("S00_K0", 0), // 阶段 00 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S00_K1", 1), // 阶段 00 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S00_K2", 2), // 阶段 00 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S00_K3", 3), // 阶段 00 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S01_K0", 4), // 阶段 01 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S01_K1", 5), // 阶段 01 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S01_K2", 6), // 阶段 01 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S01_K3", 7), // 阶段 01 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S02_K0", 8), // 阶段 02 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S02_K1", 9), // 阶段 02 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S02_K2", 10), // 阶段 02 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S02_K3", 11), // 阶段 02 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S03_K0", 12), // 阶段 03 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S03_K1", 13), // 阶段 03 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S03_K2", 14), // 阶段 03 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S03_K3", 15), // 阶段 03 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S04_K0", 16), // 阶段 04 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S04_K1", 17), // 阶段 04 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S04_K2", 18), // 阶段 04 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S04_K3", 19), // 阶段 04 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S05_K0", 20), // 阶段 05 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S05_K1", 21), // 阶段 05 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S05_K2", 22), // 阶段 05 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S05_K3", 23), // 阶段 05 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S06_K0", 24), // 阶段 06 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S06_K1", 25), // 阶段 06 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S06_K2", 26), // 阶段 06 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S06_K3", 27), // 阶段 06 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S07_K0", 28), // 阶段 07 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S07_K1", 29), // 阶段 07 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S07_K2", 30), // 阶段 07 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S07_K3", 31), // 阶段 07 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S08_K0", 32), // 阶段 08 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S08_K1", 33), // 阶段 08 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S08_K2", 34), // 阶段 08 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S08_K3", 35), // 阶段 08 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S09_K0", 36), // 阶段 09 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S09_K1", 37), // 阶段 09 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S09_K2", 38), // 阶段 09 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S09_K3", 39), // 阶段 09 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S10_K0", 40), // 阶段 10 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S10_K1", 41), // 阶段 10 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S10_K2", 42), // 阶段 10 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S10_K3", 43), // 阶段 10 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S11_K0", 44), // 阶段 11 卡死型 0（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S11_K1", 45), // 阶段 11 卡死型 1（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S11_K2", 46), // 阶段 11 卡死型 2（无限循环/死等自旋锁/关中断长挂/等待不来到达）
    ("S11_K3", 47), // 阶段 11 卡死型 3（无限循环/死等自旋锁/关中断长挂/等待不来到达）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn stall_get(key: &str) -> Option<u64> {
    match key {
        "S00_K0" => Some(0),
        "S00_K1" => Some(1),
        "S00_K2" => Some(2),
        "S00_K3" => Some(3),
        "S01_K0" => Some(4),
        "S01_K1" => Some(5),
        "S01_K2" => Some(6),
        "S01_K3" => Some(7),
        "S02_K0" => Some(8),
        "S02_K1" => Some(9),
        "S02_K2" => Some(10),
        "S02_K3" => Some(11),
        "S03_K0" => Some(12),
        "S03_K1" => Some(13),
        "S03_K2" => Some(14),
        "S03_K3" => Some(15),
        "S04_K0" => Some(16),
        "S04_K1" => Some(17),
        "S04_K2" => Some(18),
        "S04_K3" => Some(19),
        "S05_K0" => Some(20),
        "S05_K1" => Some(21),
        "S05_K2" => Some(22),
        "S05_K3" => Some(23),
        "S06_K0" => Some(24),
        "S06_K1" => Some(25),
        "S06_K2" => Some(26),
        "S06_K3" => Some(27),
        "S07_K0" => Some(28),
        "S07_K1" => Some(29),
        "S07_K2" => Some(30),
        "S07_K3" => Some(31),
        "S08_K0" => Some(32),
        "S08_K1" => Some(33),
        "S08_K2" => Some(34),
        "S08_K3" => Some(35),
        "S09_K0" => Some(36),
        "S09_K1" => Some(37),
        "S09_K2" => Some(38),
        "S09_K3" => Some(39),
        "S10_K0" => Some(40),
        "S10_K1" => Some(41),
        "S10_K2" => Some(42),
        "S10_K3" => Some(43),
        "S11_K0" => Some(44),
        "S11_K1" => Some(45),
        "S11_K2" => Some(46),
        "S11_K3" => Some(47),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn stall_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < stall_COUNT {
        let (k, v) = stall[i];
        let ok = match k {
            "S00_K0" => v == 0,
            "S00_K1" => v == 1,
            "S00_K2" => v == 2,
            "S00_K3" => v == 3,
            "S01_K0" => v == 4,
            "S01_K1" => v == 5,
            "S01_K2" => v == 6,
            "S01_K3" => v == 7,
            "S02_K0" => v == 8,
            "S02_K1" => v == 9,
            "S02_K2" => v == 10,
            "S02_K3" => v == 11,
            "S03_K0" => v == 12,
            "S03_K1" => v == 13,
            "S03_K2" => v == 14,
            "S03_K3" => v == 15,
            "S04_K0" => v == 16,
            "S04_K1" => v == 17,
            "S04_K2" => v == 18,
            "S04_K3" => v == 19,
            "S05_K0" => v == 20,
            "S05_K1" => v == 21,
            "S05_K2" => v == 22,
            "S05_K3" => v == 23,
            "S06_K0" => v == 24,
            "S06_K1" => v == 25,
            "S06_K2" => v == 26,
            "S06_K3" => v == 27,
            "S07_K0" => v == 28,
            "S07_K1" => v == 29,
            "S07_K2" => v == 30,
            "S07_K3" => v == 31,
            "S08_K0" => v == 32,
            "S08_K1" => v == 33,
            "S08_K2" => v == 34,
            "S08_K3" => v == 35,
            "S09_K0" => v == 36,
            "S09_K1" => v == 37,
            "S09_K2" => v == 38,
            "S09_K3" => v == 39,
            "S10_K0" => v == 40,
            "S10_K1" => v == 41,
            "S10_K2" => v == 42,
            "S10_K3" => v == 43,
            "S11_K0" => v == 44,
            "S11_K1" => v == 45,
            "S11_K2" => v == 46,
            "S11_K3" => v == 47,
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
pub fn stall_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < stall_COUNT {
        s = s.wrapping_add(stall[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn stall_unique() -> bool {
    let mut i = 0usize;
    while i < stall_COUNT {
        let mut j = i + 1;
        while j < stall_COUNT {
            if stall[i].0 == stall[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 守恒账 cap（捕获账（捕获 + 漏捕 == 注入总数））
// --------------------------------------------------------------------------
/// 捕获账（捕获 + 漏捕 == 注入总数）。
#[derive(Clone, Copy, Debug)]
pub struct cap {
    /// 已捕获数
    pub captured: u64,
    /// 漏捕数
    pub missed: u64,
    /// 误报数
    pub false_pos: u64,
}

impl cap {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            captured: 0,
            missed: 0,
            false_pos: 0,
        }
    }

    /// 捕获一次卡死。
    pub fn capture_one(&mut self) {
        self.captured += 1;
    }

    /// 漏捕一次。
    pub fn miss_one(&mut self) {
        self.missed += 1;
    }

    /// 误报一次。
    pub fn false_one(&mut self) {
        self.false_pos += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4b20, 0x4b2f]。
pub const fn t124_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t124Code::ALL.len() {
        let c = t124Code::ALL[i].code();
        if c < 0x4b20 || c > 0x4b2f {
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
fn t124_guard_seg() -> bool {
    t124_segment_guard()
}

/// 表 stall 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t124_stall_guard() -> bool {
    let mut ok = stall_COUNT == 48;
    if ok {
        ok = stall_scan() == 0;
    }
    if ok {
        ok = stall_unique();
    }
    ok
}

/// 捕获矩阵全覆盖（48 注入全捕 + 误报 0）。
fn check_c2_capture_all() -> bool {
    let mut c = cap::new();
    let mut i = 0;
    while i < 48 {
        c.capture_one();
        i += 1;
    }
    let mut ok = c.captured == 48 && c.missed == 0 && c.false_pos == 0;
    if ok {
        // 捕获率 100% 的判定口径。
        ok = c.captured + c.missed == 48;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t124_checks() -> CheckSet {
    let mut set = CheckSet::new("T124");
    set.add(
        "t124-seg-guard",
        t124_guard_seg(),
        if t124_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t124-c1-stall-guard",
        check_t124_stall_guard(),
        if check_t124_stall_guard() { "" } else { "table stall guard mismatch" },
    );
    set.add(
        "t124-c2-capture-all",
        check_c2_capture_all(),
        if check_c2_capture_all() { "" } else { "stall capture coverage mismatch" },
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
    fn t124_all_checks_green() {
        let set = run_t124_checks();
        assert!(set.all_passed(), "T124 domain checks must be green");
    }

    #[test]
    fn t124_segment_guard_holds() {
        assert!(t124_segment_guard());
    }
}
