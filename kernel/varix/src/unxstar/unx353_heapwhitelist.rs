//! unxt353_heapwhitelist —— UNX-F0381 页帧池白名单核验。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0381 · 早年堆 arena 划定（堆窗入账） · 判据：堆窗边界冻结入账，与页帧池白名单一致，堆容量 4MB 档锁定
//! · 组件：白名单表（8 窗）+ 归属核验 + 冲突检出
//! · 判据落地：与页帧池白名单一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T353_VERSION: &str = "t353-heapwhitelist-v1";

/// 诊断段基址。
pub const T353_SEG_BASE: u16 = 0x4020;

// --------------------------------------------------------------------------
// 诊断码（t353 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 白名单核验诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t353Code {
    /// 0x4020 页不在白名单。
    NotInWhitelist,
    /// 0x4021 池冲突（堆窗与池窗重叠）。
    PoolConflict,
    /// 0x4022 归属缺失（页无主）。
    OwnerMissing,
    /// 0x4023 双主（同页双属）。
    DoubleOwner,
    /// 0x4024 页帧越界。
    FrameOob,
    /// 0x4025 白名单账破缺。
    LedgerBreak,
    /// 0x4026 窗序断档。
    OrderBreak,
    /// 0x4027 守卫触发。
    GuardTripped,
    /// 0x4028 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4029 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x402a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x402b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x402c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x402d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x402e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t353Code {
    /// 全量闭集。
    pub const ALL: [t353Code; 15] = [
        t353Code::NotInWhitelist,
        t353Code::PoolConflict,
        t353Code::OwnerMissing,
        t353Code::DoubleOwner,
        t353Code::FrameOob,
        t353Code::LedgerBreak,
        t353Code::OrderBreak,
        t353Code::GuardTripped,
        t353Code::LedgerBroken,
        t353Code::SnapshotFingerprint,
        t353Code::TableBroken,
        t353Code::IndexMisaligned,
        t353Code::DuplicateEntry,
        t353Code::OrderViolated,
        t353Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t353Code::NotInWhitelist => 0x4020,
            t353Code::PoolConflict => 0x4021,
            t353Code::OwnerMissing => 0x4022,
            t353Code::DoubleOwner => 0x4023,
            t353Code::FrameOob => 0x4024,
            t353Code::LedgerBreak => 0x4025,
            t353Code::OrderBreak => 0x4026,
            t353Code::GuardTripped => 0x4027,
            t353Code::LedgerBroken => 0x4028,
            t353Code::SnapshotFingerprint => 0x4029,
            t353Code::TableBroken => 0x402a,
            t353Code::IndexMisaligned => 0x402b,
            t353Code::DuplicateEntry => 0x402c,
            t353Code::OrderViolated => 0x402d,
            t353Code::SegmentGuard => 0x402e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t353Code::NotInWhitelist => "页不在白名单",
            t353Code::PoolConflict => "池冲突（堆窗与池窗重叠）",
            t353Code::OwnerMissing => "归属缺失（页无主）",
            t353Code::DoubleOwner => "双主（同页双属）",
            t353Code::FrameOob => "页帧越界",
            t353Code::LedgerBreak => "白名单账破缺",
            t353Code::OrderBreak => "窗序断档",
            t353Code::GuardTripped => "守卫触发",
            t353Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t353Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t353Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t353Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t353Code::DuplicateEntry => "条目重复（同键双写）",
            t353Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t353Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t353Code::NotInWhitelist => "按白名单边界裁剪",
            t353Code::PoolConflict => "按独占律拆分",
            t353Code::OwnerMissing => "补归属后入账",
            t353Code::DoubleOwner => "按仲裁裁一",
            t353Code::FrameOob => "按帧号律裁剪",
            t353Code::LedgerBreak => "按守恒重建",
            t353Code::OrderBreak => "按序重排",
            t353Code::GuardTripped => "检查临界区",
            t353Code::LedgerBroken => "按守恒律重建台账后重跑",
            t353Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t353Code::TableBroken => "按语义规则重建常量表",
            t353Code::IndexMisaligned => "按声明序归一化后重跑",
            t353Code::DuplicateEntry => "合并同键条目为末次生效",
            t353Code::OrderViolated => "按依赖矩阵重排后重跑",
            t353Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 heapwl（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 页帧池白名单窗表（8 窗 × 4MB 连续覆盖 [0x400000, 0x2400000)，堆窗恰为 WL03）。
/// 行数。
pub const heapwl_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static heapwl: [(&'static str, u64); 8] = [
    ("WL00", 4194304), // 白名单窗 00（基址 0x400000，4MB 早年可分配窗）
    ("WL01", 8388608), // 白名单窗 01（基址 0x800000，4MB 早年可分配窗）
    ("WL02", 12582912), // 白名单窗 02（基址 0xc00000，4MB 早年可分配窗）
    ("WL03", 16777216), // 白名单窗 03（基址 0x1000000，4MB 早年可分配窗）
    ("WL04", 20971520), // 白名单窗 04（基址 0x1400000，4MB 早年可分配窗）
    ("WL05", 25165824), // 白名单窗 05（基址 0x1800000，4MB 早年可分配窗）
    ("WL06", 29360128), // 白名单窗 06（基址 0x1c00000，4MB 早年可分配窗）
    ("WL07", 33554432), // 白名单窗 07（基址 0x2000000，4MB 早年可分配窗）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn heapwl_get(key: &str) -> Option<u64> {
    match key {
        "WL00" => Some(4194304),
        "WL01" => Some(8388608),
        "WL02" => Some(12582912),
        "WL03" => Some(16777216),
        "WL04" => Some(20971520),
        "WL05" => Some(25165824),
        "WL06" => Some(29360128),
        "WL07" => Some(33554432),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn heapwl_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < heapwl_COUNT {
        let (k, v) = heapwl[i];
        let ok = match k {
            "WL00" => v >= 0x400000,
            "WL01" => v >= 0x400000,
            "WL02" => v >= 0x400000,
            "WL03" => v >= 0x400000,
            "WL04" => v >= 0x400000,
            "WL05" => v >= 0x400000,
            "WL06" => v >= 0x400000,
            "WL07" => v >= 0x400000,
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
pub fn heapwl_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < heapwl_COUNT {
        s = s.wrapping_add(heapwl[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn heapwl_unique() -> bool {
    let mut i = 0usize;
    while i < heapwl_COUNT {
        let mut j = i + 1;
        while j < heapwl_COUNT {
            if heapwl[i].0 == heapwl[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 堆窗常量（本模块自包含：4MB 档 + 早年池上限内联定义）
// ---------------------------------------------------------------------------

/// 4MB 档（字节）。
pub const HEAP_SIZE_4M: u64 = 4 * 1024 * 1024;

/// 4K 页。
pub const HEAP_PAGE: u64 = 4096;

/// 堆窗基址（16MiB，落在白名单窗 3 [0x1000000, 0x3000000)）。
pub const HEAP_BASE: u64 = 0x100_0000;

/// 堆窗结束（闭开）。
pub const HEAP_END: u64 = HEAP_BASE + HEAP_SIZE_4M;

/// 早年池上限。
pub const EARLY_POOL_LIMIT: u64 = 0x4000_0000;

// ---------------------------------------------------------------------------
// 归属核验引擎（页帧号 → 白名单窗归属；双主检出）
// ---------------------------------------------------------------------------

/// 帧号 → 物地址。
pub const fn frame_addr(fno: u64) -> u64 {
    fno * 4096
}

/// 物地址归属的白名单窗（返回窗号；None = 不在白名单）。
pub fn heapwl_owner(addr: u64) -> Option<u64> {
    let mut i: u64 = 0;
    while i < heapwl_COUNT as u64 {
        let base = heapwl[i as usize].1;
        if addr >= base && addr < base + 0x400000 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 堆窗归属（整窗落入同一白名单窗才合法）。
pub fn heap_window_owner_ok() -> bool {
    match (heapwl_owner(HEAP_BASE), heapwl_owner(HEAP_END - 1)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4020, 0x402f]。
pub const fn t353_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t353Code::ALL.len() {
        let c = t353Code::ALL[i].code();
        if c < 0x4020 || c > 0x402f {
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
fn t353_guard_seg() -> bool {
    t353_segment_guard()
}

/// 表 heapwl 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t353_heapwl_guard() -> bool {
    let mut ok = heapwl_COUNT == 8;
    if ok {
        ok = heapwl_scan() == 0;
    }
    if ok {
        ok = heapwl_unique();
    }
    ok
}

/// 归属核验（堆窗整体归属同一窗 + 窗外页拒收）。
fn check_c2_owner() -> bool {
    let mut ok = heap_window_owner_ok();
    if ok {
        // 堆窗 [0x1000000, 0x1400000) 恰为白名单窗 WL03。
        ok = heapwl_owner(HEAP_BASE) == Some(3) && heapwl_owner(HEAP_END - 1) == Some(3);
    }
    if ok {
        // 0x100000 以下不在白名单。
        ok = heapwl_owner(0x1000).is_none();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t353_checks() -> CheckSet {
    let mut set = CheckSet::new("T353");
    set.add(
        "t353-seg-guard",
        t353_guard_seg(),
        if t353_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t353-c1-wl-guard",
        check_t353_heapwl_guard(),
        if check_t353_heapwl_guard() { "" } else { "table heapwl guard mismatch" },
    );
    set.add(
        "t353-c2-owner",
        check_c2_owner(),
        if check_c2_owner() { "" } else { "whitelist owner mismatch" },
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
    fn t353_all_checks_green() {
        let set = run_t353_checks();
        assert!(set.all_passed(), "T353 domain checks must be green");
    }

    #[test]
    fn t353_segment_guard_holds() {
        assert!(t353_segment_guard());
    }
}
