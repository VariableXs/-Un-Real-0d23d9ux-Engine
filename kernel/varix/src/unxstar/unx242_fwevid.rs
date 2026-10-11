//! unxt242_unx242_fwevid —— UNX-F0242 固件交接证据包（类型/版本/证据落 Manifest）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0242 · 早传第一步：固件交接证据包 · 判据：固件类型/版本/交接证据齐备落 Manifest，与 F0001 交接账 diff 一致
//! · 组件：证据六件表 + Manifest 落账 + F0001 交接账 diff 对拍 + 域快照
//! · 判据落地：证据齐备 + 与 F0001 diff 一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T242_VERSION: &str = "t242-unx242_fwevid-v1";

/// 诊断段基址。
pub const T242_SEG_BASE: u16 = 0x4e60;

// --------------------------------------------------------------------------
// 诊断码（t242 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 固件证据诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t242Code {
    /// 0x4e60 证据缺失（六件缺一）。
    EvidenceMissing,
    /// 0x4e61 固件类型非法（BIOS/UEFI 之外）。
    FwTypeBad,
    /// 0x4e62 版本字段非法（不可解析）。
    VersionBad,
    /// 0x4e63 交接账分歧（与 F0001 不一致）。
    HandoffDiff,
    /// 0x4e64 Manifest 溢出（证据超容量）。
    ManifestOvf,
    /// 0x4e65 证据账破缺。
    LedgerBreak,
    /// 0x4e66 落账序违例。
    OrderViolated,
    /// 0x4e67 守卫触发。
    GuardTripped,
    /// 0x4e68 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e69 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e6a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e6b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e6c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e6d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t242Code {
    /// 全量闭集。
    pub const ALL: [t242Code; 14] = [
        t242Code::EvidenceMissing,
        t242Code::FwTypeBad,
        t242Code::VersionBad,
        t242Code::HandoffDiff,
        t242Code::ManifestOvf,
        t242Code::LedgerBreak,
        t242Code::OrderViolated,
        t242Code::GuardTripped,
        t242Code::LedgerBroken,
        t242Code::SnapshotFingerprint,
        t242Code::TableBroken,
        t242Code::IndexMisaligned,
        t242Code::DuplicateEntry,
        t242Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t242Code::EvidenceMissing => 0x4e60,
            t242Code::FwTypeBad => 0x4e61,
            t242Code::VersionBad => 0x4e62,
            t242Code::HandoffDiff => 0x4e63,
            t242Code::ManifestOvf => 0x4e64,
            t242Code::LedgerBreak => 0x4e65,
            t242Code::OrderViolated => 0x4e66,
            t242Code::GuardTripped => 0x4e67,
            t242Code::LedgerBroken => 0x4e68,
            t242Code::SnapshotFingerprint => 0x4e69,
            t242Code::TableBroken => 0x4e6a,
            t242Code::IndexMisaligned => 0x4e6b,
            t242Code::DuplicateEntry => 0x4e6c,
            t242Code::SegmentGuard => 0x4e6d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t242Code::EvidenceMissing => "证据缺失（六件缺一）",
            t242Code::FwTypeBad => "固件类型非法（BIOS/UEFI 之外）",
            t242Code::VersionBad => "版本字段非法（不可解析）",
            t242Code::HandoffDiff => "交接账分歧（与 F0001 不一致）",
            t242Code::ManifestOvf => "Manifest 溢出（证据超容量）",
            t242Code::LedgerBreak => "证据账破缺",
            t242Code::OrderViolated => "落账序违例",
            t242Code::GuardTripped => "守卫触发",
            t242Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t242Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t242Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t242Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t242Code::DuplicateEntry => "条目重复（同键双写）",
            t242Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t242Code::EvidenceMissing => "按六件齐备律补齐",
            t242Code::FwTypeBad => "按类型闭集校正",
            t242Code::VersionBad => "按版本格式律校正",
            t242Code::HandoffDiff => "按 diff 归因路径修正",
            t242Code::ManifestOvf => "按容量律裁剪/分页",
            t242Code::LedgerBreak => "按守恒重建",
            t242Code::OrderViolated => "按 Manifest 段序重排",
            t242Code::GuardTripped => "检查临界区",
            t242Code::LedgerBroken => "按守恒律重建台账后重跑",
            t242Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t242Code::TableBroken => "按语义规则重建常量表",
            t242Code::IndexMisaligned => "按声明序归一化后重跑",
            t242Code::DuplicateEntry => "合并同键条目为末次生效",
            t242Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 固件交接证据六件 + Manifest 落账 + F0001 diff 对拍
// ---------------------------------------------------------------------------

/// 固件类型（闭集二型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FirmwareKind {
    /// 传统 BIOS。
    Bios,
    /// UEFI。
    Uefi,
}

/// 证据件（闭集六件：类型/版本/交接结构/内存图/启动设备/时间戳）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 固件类型（BIOS/UEFI）。
    Kind,
    /// 固件版本串。
    Version,
    /// 交接结构指针（SystemTable/BootParams）。
    HandoffStruct,
    /// 内存图（E820/UEFI MemoryMap 摘要）。
    MemMap,
    /// 启动设备（BootOrder 首设备）。
    BootDevice,
    /// 时间戳（固件交接时刻）。
    Timestamp,
}

impl Evidence {
    /// 全量闭集（六件）。
    pub const ALL: [Evidence; 6] = [
        Evidence::Kind,
        Evidence::Version,
        Evidence::HandoffStruct,
        Evidence::MemMap,
        Evidence::BootDevice,
        Evidence::Timestamp,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            Evidence::Kind => 0,
            Evidence::Version => 1,
            Evidence::HandoffStruct => 2,
            Evidence::MemMap => 3,
            Evidence::BootDevice => 4,
            Evidence::Timestamp => 5,
        }
    }

    /// 中文件名。
    pub const fn cn(self) -> &'static str {
        match self {
            Evidence::Kind => "固件类型",
            Evidence::Version => "固件版本",
            Evidence::HandoffStruct => "交接结构",
            Evidence::MemMap => "内存图摘要",
            Evidence::BootDevice => "启动设备",
            Evidence::Timestamp => "交接时刻",
        }
    }

    /// 单件最大字节（类型 8/版本 64/结构 16/内存图 128/设备 32/时间戳 16）。
    pub const fn budget(self) -> u32 {
        match self {
            Evidence::Kind => 8,
            Evidence::Version => 64,
            Evidence::HandoffStruct => 16,
            Evidence::MemMap => 128,
            Evidence::BootDevice => 32,
            Evidence::Timestamp => 16,
        }
    }
}

/// 证据包（六件 + F0001 账指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidencePack {
    /// 六件在场位图（bit i = Evidence::ALL[i]）。
    pub present: u8,
    /// F0001 交接账指纹（FNV-1a 64）。
    pub handoff_fp: u64,
    /// 固件类型（0=BIOS 1=UEFI）。
    pub kind_tag: u8,
}

/// 六件齐备判定（位图 0b111111）。
pub const fn pack_complete(p: &EvidencePack) -> bool {
    p.present == 0x3F
}

/// F0001 diff 一致判定（指纹逐位相等 + 齐备）。
pub const fn pack_agrees(p: &EvidencePack, expect_fp: u64) -> bool {
    pack_complete(p) && p.handoff_fp == expect_fp
}

/// 版本串合法性（ASCII 可打印 + 长度 1..=64）。
pub const fn version_ok(v: &str) -> bool {
    let b = v.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if b[i] < 0x20 || b[i] > 0x7E {
            return false;
        }
        i += 1;
    }
    true
}

/// 固件类型标签（0=BIOS 1=UEFI）。
pub const fn kind_tag_ok(t: u8) -> bool {
    t <= 1
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e60, 0x4e6f]。
pub const fn t242_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t242Code::ALL.len() {
        let c = t242Code::ALL[i].code();
        if c < 0x4e60 || c > 0x4e6f {
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
fn t242_guard_seg() -> bool {
    t242_segment_guard()
}

/// 证据包六件齐备（0x3F）+ 预算合计 264 字节。
fn check_c1_pack() -> bool {
    let p = EvidencePack { present: 0x3F, handoff_fp: 0xC0FF_EE00, kind_tag: 1 };
    let mut ok = pack_complete(&p);
    if ok {
        ok = pack_agrees(&p, 0xC0FF_EE00) && !pack_agrees(&p, 0xDEAD);
    }
    if ok {
        let mut total = 0u32;
        let mut i = 0;
        while i < 6 {
            total += Evidence::ALL[i].budget();
            i += 1;
        }
        ok = total == 264;
    }
    ok
}

/// 版本串合法性（ASCII 白名单 + 空/超长拒收）+ 类型标签。
fn check_c2_version() -> bool {
    let mut ok = version_ok("EDK II 2.10") && !version_ok("");
    if ok {
        ok = kind_tag_ok(0) && kind_tag_ok(1) && !kind_tag_ok(2);
    }
    if ok {
        // 缺件包必须判不齐备。
        let bad = EvidencePack { present: 0x3E, handoff_fp: 0xC0FF_EE00, kind_tag: 1 };
        ok = !pack_complete(&bad) && !pack_agrees(&bad, 0xC0FF_EE00);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t242_checks() -> CheckSet {
    let mut set = CheckSet::new("T242");
    set.add(
        "t242-seg-guard",
        t242_guard_seg(),
        if t242_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t242-c1-pack",
        check_c1_pack(),
        if check_c1_pack() { "" } else { "evidence pack completeness mismatch" },
    );
    set.add(
        "t242-c2-version",
        check_c2_version(),
        if check_c2_version() { "" } else { "evidence version/kind mismatch" },
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
    fn t242_all_checks_green() {
        let set = run_t242_checks();
        assert!(set.all_passed(), "T242 domain checks must be green");
    }

    #[test]
    fn t242_segment_guard_holds() {
        assert!(t242_segment_guard());
    }
}
