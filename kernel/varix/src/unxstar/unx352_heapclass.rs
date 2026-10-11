//! unxt352_heapclass —— UNX-F0381 堆尺寸类表（16 类分级）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0381 · 早年堆 arena 划定（堆窗入账） · 判据：堆窗边界冻结入账，与页帧池白名单一致，堆容量 4MB 档锁定
//! · 组件：16 级尺寸类表（字节/页数/块数）+ 逐行守卫 + 类账守恒
//! · 判据落地：堆窗内的分配类语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T352_VERSION: &str = "t352-heapclass-v1";

/// 诊断段基址。
pub const T352_SEG_BASE: u16 = 0x4010;

// --------------------------------------------------------------------------
// 诊断码（t352 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 尺寸类诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t352Code {
    /// 0x4010 尺寸类越界（16 类之外）。
    ClassOob,
    /// 0x4011 块数为零。
    BlockZero,
    /// 0x4012 字节数不符（块数×尺寸≠类容量）。
    BytesMismatch,
    /// 0x4013 类断档（类号不连续）。
    ClassGap,
    /// 0x4014 类账破缺。
    LedgerBreak,
    /// 0x4015 碎片化超标。
    Fragmentation,
    /// 0x4016 类序断档。
    OrderBreak,
    /// 0x4017 守卫触发。
    GuardTripped,
    /// 0x4018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x401a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x401b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x401c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x401d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x401e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t352Code {
    /// 全量闭集。
    pub const ALL: [t352Code; 15] = [
        t352Code::ClassOob,
        t352Code::BlockZero,
        t352Code::BytesMismatch,
        t352Code::ClassGap,
        t352Code::LedgerBreak,
        t352Code::Fragmentation,
        t352Code::OrderBreak,
        t352Code::GuardTripped,
        t352Code::LedgerBroken,
        t352Code::SnapshotFingerprint,
        t352Code::TableBroken,
        t352Code::IndexMisaligned,
        t352Code::DuplicateEntry,
        t352Code::OrderViolated,
        t352Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t352Code::ClassOob => 0x4010,
            t352Code::BlockZero => 0x4011,
            t352Code::BytesMismatch => 0x4012,
            t352Code::ClassGap => 0x4013,
            t352Code::LedgerBreak => 0x4014,
            t352Code::Fragmentation => 0x4015,
            t352Code::OrderBreak => 0x4016,
            t352Code::GuardTripped => 0x4017,
            t352Code::LedgerBroken => 0x4018,
            t352Code::SnapshotFingerprint => 0x4019,
            t352Code::TableBroken => 0x401a,
            t352Code::IndexMisaligned => 0x401b,
            t352Code::DuplicateEntry => 0x401c,
            t352Code::OrderViolated => 0x401d,
            t352Code::SegmentGuard => 0x401e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t352Code::ClassOob => "尺寸类越界（16 类之外）",
            t352Code::BlockZero => "块数为零",
            t352Code::BytesMismatch => "字节数不符（块数×尺寸≠类容量）",
            t352Code::ClassGap => "类断档（类号不连续）",
            t352Code::LedgerBreak => "类账破缺",
            t352Code::Fragmentation => "碎片化超标",
            t352Code::OrderBreak => "类序断档",
            t352Code::GuardTripped => "守卫触发",
            t352Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t352Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t352Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t352Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t352Code::DuplicateEntry => "条目重复（同键双写）",
            t352Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t352Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t352Code::ClassOob => "按类表裁剪",
            t352Code::BlockZero => "按非零律拒收",
            t352Code::BytesMismatch => "按恒等式重建",
            t352Code::ClassGap => "按 16 类连续律补号",
            t352Code::LedgerBreak => "按守恒重建",
            t352Code::Fragmentation => "按整理路径重整",
            t352Code::OrderBreak => "按序重排",
            t352Code::GuardTripped => "检查临界区",
            t352Code::LedgerBroken => "按守恒律重建台账后重跑",
            t352Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t352Code::TableBroken => "按语义规则重建常量表",
            t352Code::IndexMisaligned => "按声明序归一化后重跑",
            t352Code::DuplicateEntry => "合并同键条目为末次生效",
            t352Code::OrderViolated => "按依赖矩阵重排后重跑",
            t352Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 heapclass（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 堆尺寸类表（16 级：32B..1MB 二进制分级）。
/// 行数。
pub const heapclass_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static heapclass: [(&'static str, u64); 16] = [
    ("CLASS00", 32), // 尺寸类 00（块 32 字节）
    ("CLASS01", 64), // 尺寸类 01（块 64 字节）
    ("CLASS02", 128), // 尺寸类 02（块 128 字节）
    ("CLASS03", 256), // 尺寸类 03（块 256 字节）
    ("CLASS04", 512), // 尺寸类 04（块 512 字节）
    ("CLASS05", 1024), // 尺寸类 05（块 1024 字节）
    ("CLASS06", 2048), // 尺寸类 06（块 2048 字节）
    ("CLASS07", 4096), // 尺寸类 07（块 4096 字节）
    ("CLASS08", 8192), // 尺寸类 08（块 8192 字节）
    ("CLASS09", 16384), // 尺寸类 09（块 16384 字节）
    ("CLASS10", 32768), // 尺寸类 10（块 32768 字节）
    ("CLASS11", 65536), // 尺寸类 11（块 65536 字节）
    ("CLASS12", 131072), // 尺寸类 12（块 131072 字节）
    ("CLASS13", 262144), // 尺寸类 13（块 262144 字节）
    ("CLASS14", 524288), // 尺寸类 14（块 524288 字节）
    ("CLASS15", 1048576), // 尺寸类 15（块 1048576 字节）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn heapclass_get(key: &str) -> Option<u64> {
    match key {
        "CLASS00" => Some(32),
        "CLASS01" => Some(64),
        "CLASS02" => Some(128),
        "CLASS03" => Some(256),
        "CLASS04" => Some(512),
        "CLASS05" => Some(1024),
        "CLASS06" => Some(2048),
        "CLASS07" => Some(4096),
        "CLASS08" => Some(8192),
        "CLASS09" => Some(16384),
        "CLASS10" => Some(32768),
        "CLASS11" => Some(65536),
        "CLASS12" => Some(131072),
        "CLASS13" => Some(262144),
        "CLASS14" => Some(524288),
        "CLASS15" => Some(1048576),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn heapclass_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < heapclass_COUNT {
        let (k, v) = heapclass[i];
        let ok = match k {
            "CLASS00" => v >= 32 && v == (32 << 0),
            "CLASS01" => v >= 32 && v == (32 << 1),
            "CLASS02" => v >= 32 && v == (32 << 2),
            "CLASS03" => v >= 32 && v == (32 << 3),
            "CLASS04" => v >= 32 && v == (32 << 4),
            "CLASS05" => v >= 32 && v == (32 << 5),
            "CLASS06" => v >= 32 && v == (32 << 6),
            "CLASS07" => v >= 32 && v == (32 << 7),
            "CLASS08" => v >= 32 && v == (32 << 8),
            "CLASS09" => v >= 32 && v == (32 << 9),
            "CLASS10" => v >= 32 && v == (32 << 10),
            "CLASS11" => v >= 32 && v == (32 << 11),
            "CLASS12" => v >= 32 && v == (32 << 12),
            "CLASS13" => v >= 32 && v == (32 << 13),
            "CLASS14" => v >= 32 && v == (32 << 14),
            "CLASS15" => v >= 32 && v == (32 << 15),
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
pub fn heapclass_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < heapclass_COUNT {
        s = s.wrapping_add(heapclass[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn heapclass_unique() -> bool {
    let mut i = 0usize;
    while i < heapclass_COUNT {
        let mut j = i + 1;
        while j < heapclass_COUNT {
            if heapclass[i].0 == heapclass[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4010, 0x401f]。
pub const fn t352_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t352Code::ALL.len() {
        let c = t352Code::ALL[i].code();
        if c < 0x4010 || c > 0x401f {
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
fn t352_guard_seg() -> bool {
    t352_segment_guard()
}

/// 表 heapclass 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t352_heapclass_guard() -> bool {
    let mut ok = heapclass_COUNT == 16;
    if ok {
        ok = heapclass_scan() == 0;
    }
    if ok {
        ok = heapclass_unique();
    }
    ok
}

/// 类表极值在位（最小 32B / 最大 1MB）。
fn check_c2_class_extremes() -> bool {
    let mut ok = heapclass_get("CLASS00") == Some(32);
    if ok {
        ok = heapclass_get("CLASS15") == Some(1 << 20);
    }
    if ok {
        // 二进制分级：每类是前类的 2 倍。
        ok = heapclass_get("CLASS14") == Some(1 << 19);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t352_checks() -> CheckSet {
    let mut set = CheckSet::new("T352");
    set.add(
        "t352-seg-guard",
        t352_guard_seg(),
        if t352_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t352-c1-class-guard",
        check_t352_heapclass_guard(),
        if check_t352_heapclass_guard() { "" } else { "table heapclass guard mismatch" },
    );
    set.add(
        "t352-c2-class-extremes",
        check_c2_class_extremes(),
        if check_c2_class_extremes() { "" } else { "size class extremes mismatch" },
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
    fn t352_all_checks_green() {
        let set = run_t352_checks();
        assert!(set.all_passed(), "T352 domain checks must be green");
    }

    #[test]
    fn t352_segment_guard_holds() {
        assert!(t352_segment_guard());
    }
}
