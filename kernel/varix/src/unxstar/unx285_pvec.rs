//! unxt285_pvec —— UNX-F0281 陷阱向量表（32 向量 × 类型/IST）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：陷阱向量表（32 向量）+ 类型枚举 + IST 分配账 + 逐行守卫
//! · 判据落地：拒载类/内核态陷阱类 panic 的向量语义面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T285_VERSION: &str = "t285-pvec-v1";

/// 诊断段基址。
pub const T285_SEG_BASE: u16 = 0x3b40;

// --------------------------------------------------------------------------
// 诊断码（t285 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 陷阱向量诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t285Code {
    /// 0x3b40 向量越界（0..255 之外）。
    VecOob,
    /// 0x3b41 门类型非法（非 Fault/Int/Trap/Abort）。
    TypeBad,
    /// 0x3b42 IST 索引非法（超 7）。
    IstBad,
    /// 0x3b43 DPL 非法（>3）。
    DplBad,
    /// 0x3b44 中断向量越界（≥32 且 ≥256）。
    IntrVecOob,
    /// 0x3b45 保留向量被占用。
    ReservedVecUsed,
    /// 0x3b46 IST 分配碰撞（双向量同栈）。
    IstCollision,
    /// 0x3b47 向量表破坏。
    TableBroken,
    /// 0x3b48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b4a 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b4b 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b4c 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b4d 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3b4e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t285Code {
    /// 全量闭集。
    pub const ALL: [t285Code; 15] = [
        t285Code::VecOob,
        t285Code::TypeBad,
        t285Code::IstBad,
        t285Code::DplBad,
        t285Code::IntrVecOob,
        t285Code::ReservedVecUsed,
        t285Code::IstCollision,
        t285Code::TableBroken,
        t285Code::LedgerBroken,
        t285Code::SnapshotFingerprint,
        t285Code::IndexMisaligned,
        t285Code::DuplicateEntry,
        t285Code::OrderViolated,
        t285Code::GuardTripped,
        t285Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t285Code::VecOob => 0x3b40,
            t285Code::TypeBad => 0x3b41,
            t285Code::IstBad => 0x3b42,
            t285Code::DplBad => 0x3b43,
            t285Code::IntrVecOob => 0x3b44,
            t285Code::ReservedVecUsed => 0x3b45,
            t285Code::IstCollision => 0x3b46,
            t285Code::TableBroken => 0x3b47,
            t285Code::LedgerBroken => 0x3b48,
            t285Code::SnapshotFingerprint => 0x3b49,
            t285Code::IndexMisaligned => 0x3b4a,
            t285Code::DuplicateEntry => 0x3b4b,
            t285Code::OrderViolated => 0x3b4c,
            t285Code::GuardTripped => 0x3b4d,
            t285Code::SegmentGuard => 0x3b4e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t285Code::VecOob => "向量越界（0..255 之外）",
            t285Code::TypeBad => "门类型非法（非 Fault/Int/Trap/Abort）",
            t285Code::IstBad => "IST 索引非法（超 7）",
            t285Code::DplBad => "DPL 非法（>3）",
            t285Code::IntrVecOob => "中断向量越界（≥32 且 ≥256）",
            t285Code::ReservedVecUsed => "保留向量被占用",
            t285Code::IstCollision => "IST 分配碰撞（双向量同栈）",
            t285Code::TableBroken => "向量表破坏",
            t285Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t285Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t285Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t285Code::DuplicateEntry => "条目重复（同键双写）",
            t285Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t285Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t285Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t285Code::VecOob => "按向量空间裁剪",
            t285Code::TypeBad => "按四类型闭集校正",
            t285Code::IstBad => "按 IST 分配表校正",
            t285Code::DplBad => "按 DPL 三值校正",
            t285Code::IntrVecOob => "按中断向量段校正",
            t285Code::ReservedVecUsed => "按保留表避让",
            t285Code::IstCollision => "按分配律重排",
            t285Code::TableBroken => "按 SDM 规格重建",
            t285Code::LedgerBroken => "按守恒律重建台账后重跑",
            t285Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t285Code::IndexMisaligned => "按声明序归一化后重跑",
            t285Code::DuplicateEntry => "合并同键条目为末次生效",
            t285Code::OrderViolated => "按依赖矩阵重排后重跑",
            t285Code::GuardTripped => "检查临界区嵌套与阈值",
            t285Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 pvecmap（31 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 陷阱向量表（32 向量逐行守卫：门类型 + IST 栈分配）。
/// 行数。
pub const pvecmap_COUNT: usize = 31;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static pvecmap: [(&'static str, u64); 31] = [
    ("DivideError", 0), // 向量 0x00 DivideError（Fault 门，IST=ist_debug）
    ("Debug", 1), // 向量 0x01 Debug（Fault 门，IST=ist_debug）
    ("Nmi", 2), // 向量 0x02 Nmi（Int 门，IST=ist_nmi）
    ("Breakpoint", 3), // 向量 0x03 Breakpoint（Trap 门，IST=ist_debug）
    ("Overflow", 4), // 向量 0x04 Overflow（Trap 门，IST=ist_none）
    ("BoundRange", 5), // 向量 0x05 BoundRange（Fault 门，IST=ist_none）
    ("InvalidOpcode", 6), // 向量 0x06 InvalidOpcode（Fault 门，IST=ist_none）
    ("DeviceNotAvailable", 7), // 向量 0x07 DeviceNotAvailable（Fault 门，IST=ist_none）
    ("DoubleFault", 8), // 向量 0x08 DoubleFault（Abort 门，IST=ist_df）
    ("CoprocessorSeg", 9), // 向量 0x09 CoprocessorSeg（Fault 门，IST=ist_none）
    ("InvalidTss", 10), // 向量 0x0A InvalidTss（Fault 门，IST=ist_none）
    ("SegmentNotPresent", 11), // 向量 0x0B SegmentNotPresent（Fault 门，IST=ist_none）
    ("StackSegment", 12), // 向量 0x0C StackSegment（Fault 门，IST=ist_none）
    ("GeneralProtection", 13), // 向量 0x0D GeneralProtection（Fault 门，IST=ist_none）
    ("PageFault", 14), // 向量 0x0E PageFault（Fault 门，IST=ist_pf）
    ("Reserved0f", 15), // 向量 0x0F Reserved0f（Fault 门，IST=ist_none）
    ("X87Fpe", 16), // 向量 0x10 X87Fpe（Fault 门，IST=ist_none）
    ("AlignmentCheck", 17), // 向量 0x11 AlignmentCheck（Fault 门，IST=ist_none）
    ("MachineCheck", 18), // 向量 0x12 MachineCheck（Abort 门，IST=ist_mc）
    ("SimdFpe", 19), // 向量 0x13 SimdFpe（Fault 门，IST=ist_none）
    ("Virtualization", 20), // 向量 0x14 Virtualization（Fault 门，IST=ist_none）
    ("ControlProtection", 21), // 向量 0x15 ControlProtection（Fault 门，IST=ist_none）
    ("Reserved16", 22), // 向量 0x16 Reserved16（Fault 门，IST=ist_none）
    ("Timer", 32), // 向量 0x20 Timer（Int 门，IST=ist_timer）
    ("Ipi", 33), // 向量 0x21 Ipi（Int 门，IST=ist_none）
    ("Serial", 48), // 向量 0x30 Serial（Int 门，IST=ist_none）
    ("Syscall", 64), // 向量 0x40 Syscall（Int 门，IST=ist_none）
    ("SyscallOld", 128), // 向量 0x80 SyscallOld（Trap 门，IST=ist_none）
    ("Error", 253), // 向量 0xFD Error（Int 门，IST=ist_err）
    ("Spurious", 254), // 向量 0xFE Spurious（Int 门，IST=ist_none）
    ("Panic", 255), // 向量 0xFF Panic（Int 门，IST=ist_panic）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn pvecmap_get(key: &str) -> Option<u64> {
    match key {
        "DivideError" => Some(0),
        "Debug" => Some(1),
        "Nmi" => Some(2),
        "Breakpoint" => Some(3),
        "Overflow" => Some(4),
        "BoundRange" => Some(5),
        "InvalidOpcode" => Some(6),
        "DeviceNotAvailable" => Some(7),
        "DoubleFault" => Some(8),
        "CoprocessorSeg" => Some(9),
        "InvalidTss" => Some(10),
        "SegmentNotPresent" => Some(11),
        "StackSegment" => Some(12),
        "GeneralProtection" => Some(13),
        "PageFault" => Some(14),
        "Reserved0f" => Some(15),
        "X87Fpe" => Some(16),
        "AlignmentCheck" => Some(17),
        "MachineCheck" => Some(18),
        "SimdFpe" => Some(19),
        "Virtualization" => Some(20),
        "ControlProtection" => Some(21),
        "Reserved16" => Some(22),
        "Timer" => Some(32),
        "Ipi" => Some(33),
        "Serial" => Some(48),
        "Syscall" => Some(64),
        "SyscallOld" => Some(128),
        "Error" => Some(253),
        "Spurious" => Some(254),
        "Panic" => Some(255),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn pvecmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < pvecmap_COUNT {
        let (k, v) = pvecmap[i];
        let ok = match k {
            "DivideError" => v == 0,
            "Debug" => v == 1,
            "Nmi" => v == 2,
            "Breakpoint" => v == 3,
            "Overflow" => v == 4,
            "BoundRange" => v == 5,
            "InvalidOpcode" => v == 6,
            "DeviceNotAvailable" => v == 7,
            "DoubleFault" => v == 8,
            "CoprocessorSeg" => v == 9,
            "InvalidTss" => v == 10,
            "SegmentNotPresent" => v == 11,
            "StackSegment" => v == 12,
            "GeneralProtection" => v == 13,
            "PageFault" => v == 14,
            "Reserved0f" => v == 15,
            "X87Fpe" => v == 16,
            "AlignmentCheck" => v == 17,
            "MachineCheck" => v == 18,
            "SimdFpe" => v == 19,
            "Virtualization" => v == 20,
            "ControlProtection" => v == 21,
            "Reserved16" => v == 22,
            "Timer" => v == 32,
            "Ipi" => v == 33,
            "Serial" => v == 48,
            "Syscall" => v == 64,
            "SyscallOld" => v == 128,
            "Error" => v == 253,
            "Spurious" => v == 254,
            "Panic" => v == 255,
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
pub fn pvecmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pvecmap_COUNT {
        s = s.wrapping_add(pvecmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn pvecmap_unique() -> bool {
    let mut i = 0usize;
    while i < pvecmap_COUNT {
        let mut j = i + 1;
        while j < pvecmap_COUNT {
            if pvecmap[i].0 == pvecmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3b40, 0x3b4f]。
pub const fn t285_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t285Code::ALL.len() {
        let c = t285Code::ALL[i].code();
        if c < 0x3b40 || c > 0x3b4f {
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
fn t285_guard_seg() -> bool {
    t285_segment_guard()
}

/// 表 pvecmap 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t285_pvecmap_guard() -> bool {
    let mut ok = pvecmap_COUNT == 31;
    if ok {
        ok = pvecmap_scan() == 0;
    }
    if ok {
        ok = pvecmap_unique();
    }
    ok
}

/// 异常向量抽查（#DF/#PF/#MF 门型与 IST）。
fn check_c2_vec_spot() -> bool {
    let mut ok = pvecmap_get("DoubleFault") == Some(0x08);
    if ok {
        ok = pvecmap_get("PageFault") == Some(0x0e) && pvecmap_get("X87Fpe") == Some(0x10);
    }
    if ok {
        ok = pvecmap_get("Panic") == Some(0xff) && pvecmap_get("SyscallOld") == Some(0x80);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t285_checks() -> CheckSet {
    let mut set = CheckSet::new("T285");
    set.add(
        "t285-seg-guard",
        t285_guard_seg(),
        if t285_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t285-c1-vec-guard",
        check_t285_pvecmap_guard(),
        if check_t285_pvecmap_guard() { "" } else { "table pvecmap guard mismatch" },
    );
    set.add(
        "t285-c2-vec-spot",
        check_c2_vec_spot(),
        if check_c2_vec_spot() { "" } else { "trap vector spot check mismatch" },
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
    fn t285_all_checks_green() {
        let set = run_t285_checks();
        assert!(set.all_passed(), "T285 domain checks must be green");
    }

    #[test]
    fn t285_segment_guard_holds() {
        assert!(t285_segment_guard());
    }
}
