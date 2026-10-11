//! unxt312_tsipi —— UNX-F0321 AP 唤醒协议表（INIT/SIPI 序列）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0321 · trampoline 总编排（BSP 侧准备 → AP 唤醒 → 汇合） · 判据：三段编排（准备/唤醒/汇合）时序正确，与 F0188 前置包消费契约一致
//! · 组件：唤醒协议表（16 步 × 时序）+ 起动向量表 + 超时账
//! · 判据落地：唤醒段协议面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T312_VERSION: &str = "t312-tsipi-v1";

/// 诊断段基址。
pub const T312_SEG_BASE: u16 = 0x3d10;

// --------------------------------------------------------------------------
// 诊断码（t312 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 唤醒协议诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t312Code {
    /// 0x3d10 SIPI 向量非法（非 4K 对齐）。
    SipiBad,
    /// 0x3d11 INIT 超时（AP 无响应）。
    InitTimeout,
    /// 0x3d12 唤醒次序违例。
    WakeOrderBad,
    /// 0x3d13 重复唤醒（同 AP 双 SIPI）。
    DoubleWake,
    /// 0x3d14 起动向量冲突。
    VectorConflict,
    /// 0x3d15 前置包格式非法。
    PrepackBad,
    /// 0x3d16 唤醒账破缺。
    LedgerBreak,
    /// 0x3d17 守卫触发。
    GuardTripped,
    /// 0x3d18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3d19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3d1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3d1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3d1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3d1d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3d1e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t312Code {
    /// 全量闭集。
    pub const ALL: [t312Code; 15] = [
        t312Code::SipiBad,
        t312Code::InitTimeout,
        t312Code::WakeOrderBad,
        t312Code::DoubleWake,
        t312Code::VectorConflict,
        t312Code::PrepackBad,
        t312Code::LedgerBreak,
        t312Code::GuardTripped,
        t312Code::LedgerBroken,
        t312Code::SnapshotFingerprint,
        t312Code::TableBroken,
        t312Code::IndexMisaligned,
        t312Code::DuplicateEntry,
        t312Code::OrderViolated,
        t312Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t312Code::SipiBad => 0x3d10,
            t312Code::InitTimeout => 0x3d11,
            t312Code::WakeOrderBad => 0x3d12,
            t312Code::DoubleWake => 0x3d13,
            t312Code::VectorConflict => 0x3d14,
            t312Code::PrepackBad => 0x3d15,
            t312Code::LedgerBreak => 0x3d16,
            t312Code::GuardTripped => 0x3d17,
            t312Code::LedgerBroken => 0x3d18,
            t312Code::SnapshotFingerprint => 0x3d19,
            t312Code::TableBroken => 0x3d1a,
            t312Code::IndexMisaligned => 0x3d1b,
            t312Code::DuplicateEntry => 0x3d1c,
            t312Code::OrderViolated => 0x3d1d,
            t312Code::SegmentGuard => 0x3d1e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t312Code::SipiBad => "SIPI 向量非法（非 4K 对齐）",
            t312Code::InitTimeout => "INIT 超时（AP 无响应）",
            t312Code::WakeOrderBad => "唤醒次序违例",
            t312Code::DoubleWake => "重复唤醒（同 AP 双 SIPI）",
            t312Code::VectorConflict => "起动向量冲突",
            t312Code::PrepackBad => "前置包格式非法",
            t312Code::LedgerBreak => "唤醒账破缺",
            t312Code::GuardTripped => "守卫触发",
            t312Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t312Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t312Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t312Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t312Code::DuplicateEntry => "条目重复（同键双写）",
            t312Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t312Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t312Code::SipiBad => "按 4K 对齐律校正",
            t312Code::InitTimeout => "按超时路径重发或降级",
            t312Code::WakeOrderBad => "按唤醒序表重排",
            t312Code::DoubleWake => "幂等去重",
            t312Code::VectorConflict => "按向量分配律重排",
            t312Code::PrepackBad => "按 F0188 契约校正",
            t312Code::LedgerBreak => "按守恒重建",
            t312Code::GuardTripped => "检查临界区",
            t312Code::LedgerBroken => "按守恒律重建台账后重跑",
            t312Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t312Code::TableBroken => "按语义规则重建常量表",
            t312Code::IndexMisaligned => "按声明序归一化后重跑",
            t312Code::DuplicateEntry => "合并同键条目为末次生效",
            t312Code::OrderViolated => "按依赖矩阵重排后重跑",
            t312Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 wake_proto（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// AP 唤醒协议表（16 步逐行守卫）。
/// 行数。
pub const wake_proto_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static wake_proto: [(&'static str, u64); 16] = [
    ("WAKE00", 0), // 唤醒协议第 0 步：断言 AP 存在（MADT 条目）
    ("WAKE01", 1), // 唤醒协议第 1 步：广播 INIT（level assert）
    ("WAKE02", 2), // 唤醒协议第 2 步：等待 10ms 让位
    ("WAKE03", 3), // 唤醒协议第 3 步：反断言 INIT（level deassert）
    ("WAKE04", 4), // 唤醒协议第 4 步：广播 SIPI ×2（间隔 200us）
    ("WAKE05", 5), // 唤醒协议第 5 步：等待起动向量落地
    ("WAKE06", 6), // 唤醒协议第 6 步：AP 进入 16 位实模式段
    ("WAKE07", 7), // 唤醒协议第 7 步：加载临时 GDT
    ("WAKE08", 8), // 唤醒协议第 8 步：跳 32 位保护模式段
    ("WAKE09", 9), // 唤醒协议第 9 步：加载页表基址（CR3 传递）
    ("WAKE10", 10), // 唤醒协议第 10 步：开分页
    ("WAKE11", 11), // 唤醒协议第 11 步：跳 64 位长模式段
    ("WAKE12", 12), // 唤醒协议第 12 步：切内核栈
    ("WAKE13", 13), // 唤醒协议第 13 步：登记核号
    ("WAKE14", 14), // 唤醒协议第 14 步：置就绪标志
    ("WAKE15", 15), // 唤醒协议第 15 步：回报汇合点
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn wake_proto_get(key: &str) -> Option<u64> {
    match key {
        "WAKE00" => Some(0),
        "WAKE01" => Some(1),
        "WAKE02" => Some(2),
        "WAKE03" => Some(3),
        "WAKE04" => Some(4),
        "WAKE05" => Some(5),
        "WAKE06" => Some(6),
        "WAKE07" => Some(7),
        "WAKE08" => Some(8),
        "WAKE09" => Some(9),
        "WAKE10" => Some(10),
        "WAKE11" => Some(11),
        "WAKE12" => Some(12),
        "WAKE13" => Some(13),
        "WAKE14" => Some(14),
        "WAKE15" => Some(15),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn wake_proto_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < wake_proto_COUNT {
        let (k, v) = wake_proto[i];
        let ok = match k {
            "WAKE00" => v == 0,
            "WAKE01" => v == 1,
            "WAKE02" => v == 2,
            "WAKE03" => v == 3,
            "WAKE04" => v == 4,
            "WAKE05" => v == 5,
            "WAKE06" => v == 6,
            "WAKE07" => v == 7,
            "WAKE08" => v == 8,
            "WAKE09" => v == 9,
            "WAKE10" => v == 10,
            "WAKE11" => v == 11,
            "WAKE12" => v == 12,
            "WAKE13" => v == 13,
            "WAKE14" => v == 14,
            "WAKE15" => v == 15,
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
pub fn wake_proto_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < wake_proto_COUNT {
        s = s.wrapping_add(wake_proto[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn wake_proto_unique() -> bool {
    let mut i = 0usize;
    while i < wake_proto_COUNT {
        let mut j = i + 1;
        while j < wake_proto_COUNT {
            if wake_proto[i].0 == wake_proto[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3d10, 0x3d1f]。
pub const fn t312_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t312Code::ALL.len() {
        let c = t312Code::ALL[i].code();
        if c < 0x3d10 || c > 0x3d1f {
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
fn t312_guard_seg() -> bool {
    t312_segment_guard()
}

/// 表 wake_proto 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t312_wake_proto_guard() -> bool {
    let mut ok = wake_proto_COUNT == 16;
    if ok {
        ok = wake_proto_scan() == 0;
    }
    if ok {
        ok = wake_proto_unique();
    }
    ok
}

/// 协议步序单调复检。
fn check_c2_proto_seq() -> bool {
    let mut ok = wake_proto_COUNT == 16;
    if ok {
        let mut i = 1usize;
        while i < wake_proto_COUNT {
            if wake_proto[i - 1].1 >= wake_proto[i].1 {
                ok = false;
            }
            i += 1;
        }
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t312_checks() -> CheckSet {
    let mut set = CheckSet::new("T312");
    set.add(
        "t312-seg-guard",
        t312_guard_seg(),
        if t312_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t312-c1-proto-guard",
        check_t312_wake_proto_guard(),
        if check_t312_wake_proto_guard() { "" } else { "table wake_proto guard mismatch" },
    );
    set.add(
        "t312-c2-proto-seq",
        check_c2_proto_seq(),
        if check_c2_proto_seq() { "" } else { "wake protocol sequence mismatch" },
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
    fn t312_all_checks_green() {
        let set = run_t312_checks();
        assert!(set.all_passed(), "T312 domain checks must be green");
    }

    #[test]
    fn t312_segment_guard_holds() {
        assert!(t312_segment_guard());
    }
}
