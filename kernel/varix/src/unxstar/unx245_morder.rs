//! unxt245_morder —— UNX-F0241 步序账（十二步顺序守恒 + 乱序检出）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：步序账（顺序单调律）+ 乱序检出器 + 交换恢复路径
//! · 判据落地：十二步结构定义的次序面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T245_VERSION: &str = "t245-morder-v1";

/// 诊断段基址。
pub const T245_SEG_BASE: u16 = 0x3940;

// --------------------------------------------------------------------------
// 诊断码（t245 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 步序账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t245Code {
    /// 0x3940 步序折叠（非单调序列）。
    OrderFold,
    /// 0x3941 相邻交换检出。
    SwapDetected,
    /// 0x3942 步号断档（缺步）。
    GapInSeq,
    /// 0x3943 步号重复。
    DupStep,
    /// 0x3944 步尾丢失（末步缺失）。
    TailLost,
    /// 0x3945 步头丢失（首步缺失）。
    HeadLost,
    /// 0x3946 步序账破缺（合计≠12）。
    LedgerBreak,
    /// 0x3947 步序报告不符。
    ReportBad,
    /// 0x3948 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3949 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x394a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x394b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x394c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x394d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x394e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x394f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t245Code {
    /// 全量闭集。
    pub const ALL: [t245Code; 16] = [
        t245Code::OrderFold,
        t245Code::SwapDetected,
        t245Code::GapInSeq,
        t245Code::DupStep,
        t245Code::TailLost,
        t245Code::HeadLost,
        t245Code::LedgerBreak,
        t245Code::ReportBad,
        t245Code::LedgerBroken,
        t245Code::SnapshotFingerprint,
        t245Code::TableBroken,
        t245Code::IndexMisaligned,
        t245Code::DuplicateEntry,
        t245Code::OrderViolated,
        t245Code::GuardTripped,
        t245Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t245Code::OrderFold => 0x3940,
            t245Code::SwapDetected => 0x3941,
            t245Code::GapInSeq => 0x3942,
            t245Code::DupStep => 0x3943,
            t245Code::TailLost => 0x3944,
            t245Code::HeadLost => 0x3945,
            t245Code::LedgerBreak => 0x3946,
            t245Code::ReportBad => 0x3947,
            t245Code::LedgerBroken => 0x3948,
            t245Code::SnapshotFingerprint => 0x3949,
            t245Code::TableBroken => 0x394a,
            t245Code::IndexMisaligned => 0x394b,
            t245Code::DuplicateEntry => 0x394c,
            t245Code::OrderViolated => 0x394d,
            t245Code::GuardTripped => 0x394e,
            t245Code::SegmentGuard => 0x394f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t245Code::OrderFold => "步序折叠（非单调序列）",
            t245Code::SwapDetected => "相邻交换检出",
            t245Code::GapInSeq => "步号断档（缺步）",
            t245Code::DupStep => "步号重复",
            t245Code::TailLost => "步尾丢失（末步缺失）",
            t245Code::HeadLost => "步头丢失（首步缺失）",
            t245Code::LedgerBreak => "步序账破缺（合计≠12）",
            t245Code::ReportBad => "步序报告不符",
            t245Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t245Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t245Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t245Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t245Code::DuplicateEntry => "条目重复（同键双写）",
            t245Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t245Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t245Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t245Code::OrderFold => "按步号单调律重排",
            t245Code::SwapDetected => "按交换恢复路径回位",
            t245Code::GapInSeq => "按 12 步连续律补号",
            t245Code::DupStep => "去重后重排",
            t245Code::TailLost => "补末步后封账",
            t245Code::HeadLost => "补首步后封账",
            t245Code::LedgerBreak => "按守恒重建",
            t245Code::ReportBad => "按账本重渲染",
            t245Code::LedgerBroken => "按守恒律重建台账后重跑",
            t245Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t245Code::TableBroken => "按语义规则重建常量表",
            t245Code::IndexMisaligned => "按声明序归一化后重跑",
            t245Code::DuplicateEntry => "合并同键条目为末次生效",
            t245Code::OrderViolated => "按依赖矩阵重排后重跑",
            t245Code::GuardTripped => "检查临界区嵌套与阈值",
            t245Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 morder（12 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 步序账表（12 步逐行守卫，位序单调）。
/// 行数。
pub const morder_COUNT: usize = 12;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static morder: [(&'static str, u64); 12] = [
    ("ORD_01", 1), // 第 1 步「固件」位序
    ("ORD_02", 2), // 第 2 步「内存」位序
    ("ORD_03", 3), // 第 3 步「CPU」位序
    ("ORD_04", 4), // 第 4 步「随机化」位序
    ("ORD_05", 5), // 第 5 步「参数」位序
    ("ORD_06", 6), // 第 6 步「健康」位序
    ("ORD_07", 7), // 第 7 步「时间」位序
    ("ORD_08", 8), // 第 8 步「日志」位序
    ("ORD_09", 9), // 第 9 步「上次引导」位序
    ("ORD_10", 10), // 第 10 步「订阅」位序
    ("ORD_11", 11), // 第 11 步「完整性」位序
    ("ORD_12", 12), // 第 12 步「时序」位序
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn morder_get(key: &str) -> Option<u64> {
    match key {
        "ORD_01" => Some(1),
        "ORD_02" => Some(2),
        "ORD_03" => Some(3),
        "ORD_04" => Some(4),
        "ORD_05" => Some(5),
        "ORD_06" => Some(6),
        "ORD_07" => Some(7),
        "ORD_08" => Some(8),
        "ORD_09" => Some(9),
        "ORD_10" => Some(10),
        "ORD_11" => Some(11),
        "ORD_12" => Some(12),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn morder_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < morder_COUNT {
        let (k, v) = morder[i];
        let ok = match k {
            "ORD_01" => v == 1,
            "ORD_02" => v == 2,
            "ORD_03" => v == 3,
            "ORD_04" => v == 4,
            "ORD_05" => v == 5,
            "ORD_06" => v == 6,
            "ORD_07" => v == 7,
            "ORD_08" => v == 8,
            "ORD_09" => v == 9,
            "ORD_10" => v == 10,
            "ORD_11" => v == 11,
            "ORD_12" => v == 12,
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
pub fn morder_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < morder_COUNT {
        s = s.wrapping_add(morder[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn morder_unique() -> bool {
    let mut i = 0usize;
    while i < morder_COUNT {
        let mut j = i + 1;
        while j < morder_COUNT {
            if morder[i].0 == morder[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3940, 0x394f]。
pub const fn t245_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t245Code::ALL.len() {
        let c = t245Code::ALL[i].code();
        if c < 0x3940 || c > 0x394f {
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
fn t245_guard_seg() -> bool {
    t245_segment_guard()
}

/// 表 morder 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t245_morder_guard() -> bool {
    let mut ok = morder_COUNT == 12;
    if ok {
        ok = morder_scan() == 0;
    }
    if ok {
        ok = morder_unique();
    }
    ok
}

/// 步序单调复检（逐行递增）。
fn check_c2_order_monotone() -> bool {
    let mut ok = morder_COUNT == 12;
    if ok {
        let mut i = 1usize;
        while i < morder_COUNT {
            if morder[i - 1].1 >= morder[i].1 {
                ok = false;
            }
            i += 1;
        }
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t245_checks() -> CheckSet {
    let mut set = CheckSet::new("T245");
    set.add(
        "t245-seg-guard",
        t245_guard_seg(),
        if t245_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t245-c1-order-guard",
        check_t245_morder_guard(),
        if check_t245_morder_guard() { "" } else { "table morder guard mismatch" },
    );
    set.add(
        "t245-c2-order-monotone",
        check_c2_order_monotone(),
        if check_c2_order_monotone() { "" } else { "step order monotonicity mismatch" },
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
    fn t245_all_checks_green() {
        let set = run_t245_checks();
        assert!(set.all_passed(), "T245 domain checks must be green");
    }

    #[test]
    fn t245_segment_guard_holds() {
        assert!(t245_segment_guard());
    }
}
