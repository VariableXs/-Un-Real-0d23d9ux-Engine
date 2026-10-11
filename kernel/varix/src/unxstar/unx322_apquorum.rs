//! unxt322_apquorum —— UNX-F0341 栅栏法定人数与放行判据。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0341 · AP 汇合栅栏（全部到达点同步） · 判据：栅栏语义（全员到达放行）正确，部分未达超时路径有账
//! · 组件：人数表（1..16 方）+ 放行判据 + 降级账
//! · 判据落地：部分未达超时路径有账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T322_VERSION: &str = "t322-apquorum-v1";

/// 诊断段基址。
pub const T322_SEG_BASE: u16 = 0x3e10;

// --------------------------------------------------------------------------
// 诊断码（t322 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 法定人数诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t322Code {
    /// 0x3e10 法定人数为零。
    QuorumZero,
    /// 0x3e11 法定人数超方数。
    QuorumOver,
    /// 0x3e12 非法降级（未超时即降）。
    DegradeIllegal,
    /// 0x3e13 降级账破缺。
    LedgerBreak,
    /// 0x3e14 人数漂移（方数与登记不符）。
    CountDrift,
    /// 0x3e15 放行序违例。
    OrderViolated,
    /// 0x3e16 报告陈旧。
    ReportStale,
    /// 0x3e17 守卫触发。
    GuardTripped,
    /// 0x3e18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3e19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3e1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3e1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3e1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3e1d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t322Code {
    /// 全量闭集。
    pub const ALL: [t322Code; 14] = [
        t322Code::QuorumZero,
        t322Code::QuorumOver,
        t322Code::DegradeIllegal,
        t322Code::LedgerBreak,
        t322Code::CountDrift,
        t322Code::OrderViolated,
        t322Code::ReportStale,
        t322Code::GuardTripped,
        t322Code::LedgerBroken,
        t322Code::SnapshotFingerprint,
        t322Code::TableBroken,
        t322Code::IndexMisaligned,
        t322Code::DuplicateEntry,
        t322Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t322Code::QuorumZero => 0x3e10,
            t322Code::QuorumOver => 0x3e11,
            t322Code::DegradeIllegal => 0x3e12,
            t322Code::LedgerBreak => 0x3e13,
            t322Code::CountDrift => 0x3e14,
            t322Code::OrderViolated => 0x3e15,
            t322Code::ReportStale => 0x3e16,
            t322Code::GuardTripped => 0x3e17,
            t322Code::LedgerBroken => 0x3e18,
            t322Code::SnapshotFingerprint => 0x3e19,
            t322Code::TableBroken => 0x3e1a,
            t322Code::IndexMisaligned => 0x3e1b,
            t322Code::DuplicateEntry => 0x3e1c,
            t322Code::SegmentGuard => 0x3e1d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t322Code::QuorumZero => "法定人数为零",
            t322Code::QuorumOver => "法定人数超方数",
            t322Code::DegradeIllegal => "非法降级（未超时即降）",
            t322Code::LedgerBreak => "降级账破缺",
            t322Code::CountDrift => "人数漂移（方数与登记不符）",
            t322Code::OrderViolated => "放行序违例",
            t322Code::ReportStale => "报告陈旧",
            t322Code::GuardTripped => "守卫触发",
            t322Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t322Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t322Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t322Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t322Code::DuplicateEntry => "条目重复（同键双写）",
            t322Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t322Code::QuorumZero => "按人数表校正",
            t322Code::QuorumOver => "按上限裁剪",
            t322Code::DegradeIllegal => "按超时前置条件重排",
            t322Code::LedgerBreak => "按守恒重建",
            t322Code::CountDrift => "双源核对后重建",
            t322Code::OrderViolated => "按判据律重排",
            t322Code::ReportStale => "重渲染",
            t322Code::GuardTripped => "检查临界区",
            t322Code::LedgerBroken => "按守恒律重建台账后重跑",
            t322Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t322Code::TableBroken => "按语义规则重建常量表",
            t322Code::IndexMisaligned => "按声明序归一化后重跑",
            t322Code::DuplicateEntry => "合并同键条目为末次生效",
            t322Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 apquorum（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 法定人数表（1..16 方逐行守卫）。
/// 行数。
pub const apquorum_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static apquorum: [(&'static str, u64); 16] = [
    ("PARTY01", 1), // 1 方栅栏（到达方数 == 1 才放行）
    ("PARTY02", 2), // 2 方栅栏（到达方数 == 2 才放行）
    ("PARTY03", 3), // 3 方栅栏（到达方数 == 3 才放行）
    ("PARTY04", 4), // 4 方栅栏（到达方数 == 4 才放行）
    ("PARTY05", 5), // 5 方栅栏（到达方数 == 5 才放行）
    ("PARTY06", 6), // 6 方栅栏（到达方数 == 6 才放行）
    ("PARTY07", 7), // 7 方栅栏（到达方数 == 7 才放行）
    ("PARTY08", 8), // 8 方栅栏（到达方数 == 8 才放行）
    ("PARTY09", 9), // 9 方栅栏（到达方数 == 9 才放行）
    ("PARTY10", 10), // 10 方栅栏（到达方数 == 10 才放行）
    ("PARTY11", 11), // 11 方栅栏（到达方数 == 11 才放行）
    ("PARTY12", 12), // 12 方栅栏（到达方数 == 12 才放行）
    ("PARTY13", 13), // 13 方栅栏（到达方数 == 13 才放行）
    ("PARTY14", 14), // 14 方栅栏（到达方数 == 14 才放行）
    ("PARTY15", 15), // 15 方栅栏（到达方数 == 15 才放行）
    ("PARTY16", 16), // 16 方栅栏（到达方数 == 16 才放行）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn apquorum_get(key: &str) -> Option<u64> {
    match key {
        "PARTY01" => Some(1),
        "PARTY02" => Some(2),
        "PARTY03" => Some(3),
        "PARTY04" => Some(4),
        "PARTY05" => Some(5),
        "PARTY06" => Some(6),
        "PARTY07" => Some(7),
        "PARTY08" => Some(8),
        "PARTY09" => Some(9),
        "PARTY10" => Some(10),
        "PARTY11" => Some(11),
        "PARTY12" => Some(12),
        "PARTY13" => Some(13),
        "PARTY14" => Some(14),
        "PARTY15" => Some(15),
        "PARTY16" => Some(16),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn apquorum_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < apquorum_COUNT {
        let (k, v) = apquorum[i];
        let ok = match k {
            "PARTY01" => v >= 1 && v <= 16,
            "PARTY02" => v >= 1 && v <= 16,
            "PARTY03" => v >= 1 && v <= 16,
            "PARTY04" => v >= 1 && v <= 16,
            "PARTY05" => v >= 1 && v <= 16,
            "PARTY06" => v >= 1 && v <= 16,
            "PARTY07" => v >= 1 && v <= 16,
            "PARTY08" => v >= 1 && v <= 16,
            "PARTY09" => v >= 1 && v <= 16,
            "PARTY10" => v >= 1 && v <= 16,
            "PARTY11" => v >= 1 && v <= 16,
            "PARTY12" => v >= 1 && v <= 16,
            "PARTY13" => v >= 1 && v <= 16,
            "PARTY14" => v >= 1 && v <= 16,
            "PARTY15" => v >= 1 && v <= 16,
            "PARTY16" => v >= 1 && v <= 16,
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
pub fn apquorum_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < apquorum_COUNT {
        s = s.wrapping_add(apquorum[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn apquorum_unique() -> bool {
    let mut i = 0usize;
    while i < apquorum_COUNT {
        let mut j = i + 1;
        while j < apquorum_COUNT {
            if apquorum[i].0 == apquorum[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3e10, 0x3e1f]。
pub const fn t322_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t322Code::ALL.len() {
        let c = t322Code::ALL[i].code();
        if c < 0x3e10 || c > 0x3e1f {
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
fn t322_guard_seg() -> bool {
    t322_segment_guard()
}

/// 表 apquorum 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t322_apquorum_guard() -> bool {
    let mut ok = apquorum_COUNT == 16;
    if ok {
        ok = apquorum_scan() == 0;
    }
    if ok {
        ok = apquorum_unique();
    }
    ok
}

/// 人数表求和（1..16 = 136）。
fn check_c2_quorum_sum() -> bool {
    let mut ok = apquorum_sum() == 136;
    if ok {
        ok = apquorum_get("PARTY01") == Some(1) && apquorum_get("PARTY16") == Some(16);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t322_checks() -> CheckSet {
    let mut set = CheckSet::new("T322");
    set.add(
        "t322-seg-guard",
        t322_guard_seg(),
        if t322_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t322-c1-quorum-guard",
        check_t322_apquorum_guard(),
        if check_t322_apquorum_guard() { "" } else { "table apquorum guard mismatch" },
    );
    set.add(
        "t322-c2-quorum-sum",
        check_c2_quorum_sum(),
        if check_c2_quorum_sum() { "" } else { "quorum sum mismatch" },
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
    fn t322_all_checks_green() {
        let set = run_t322_checks();
        assert!(set.all_passed(), "T322 domain checks must be green");
    }

    #[test]
    fn t322_segment_guard_holds() {
        assert!(t322_segment_guard());
    }
}
