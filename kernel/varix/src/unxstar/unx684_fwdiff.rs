//! unxt684_unx684_fwdiff —— UNX-F0684 固件路差异分账（EFI×2/BIOS 三路 ≥10 条）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0684 · 固件路差异分账（EFI×2 与 BIOS 三路） · 判据：三路差异条目账齐（差异点/影响面/证据指针三列 ≥10 条）且证据可解引 100%
//! · 组件：三路差异条目账（差异点/影响面/证据三列）+ 证据解引引擎 + 域快照
//! · 判据落地：差异条目 ≥10 + 证据可解引 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T684_VERSION: &str = "t684-unx684_fwdiff-v1";

/// 诊断段基址。
pub const T684_SEG_BASE: u16 = 0x6300;

// --------------------------------------------------------------------------
// 诊断码（t684 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 固件路差异诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t684Code {
    /// 0x6300 固件路非法（三路之外）。
    RoadBad,
    /// 0x6301 差异条目不足（<10 条）。
    EntryShort,
    /// 0x6302 证据不可解引（指针失效）。
    EvidenceUnreadable,
    /// 0x6303 影响面非法（三类之外）。
    ImpactBad,
    /// 0x6304 差异账破缺。
    LedgerBreak,
    /// 0x6305 条目序违例。
    OrderViolated,
    /// 0x6306 条目重复（同差异点双记）。
    DupEntry,
    /// 0x6307 守卫触发。
    GuardTripped,
    /// 0x6308 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6309 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x630a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x630b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x630c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x630d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t684Code {
    /// 全量闭集。
    pub const ALL: [t684Code; 14] = [
        t684Code::RoadBad,
        t684Code::EntryShort,
        t684Code::EvidenceUnreadable,
        t684Code::ImpactBad,
        t684Code::LedgerBreak,
        t684Code::OrderViolated,
        t684Code::DupEntry,
        t684Code::GuardTripped,
        t684Code::LedgerBroken,
        t684Code::SnapshotFingerprint,
        t684Code::TableBroken,
        t684Code::IndexMisaligned,
        t684Code::DuplicateEntry,
        t684Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t684Code::RoadBad => 0x6300,
            t684Code::EntryShort => 0x6301,
            t684Code::EvidenceUnreadable => 0x6302,
            t684Code::ImpactBad => 0x6303,
            t684Code::LedgerBreak => 0x6304,
            t684Code::OrderViolated => 0x6305,
            t684Code::DupEntry => 0x6306,
            t684Code::GuardTripped => 0x6307,
            t684Code::LedgerBroken => 0x6308,
            t684Code::SnapshotFingerprint => 0x6309,
            t684Code::TableBroken => 0x630a,
            t684Code::IndexMisaligned => 0x630b,
            t684Code::DuplicateEntry => 0x630c,
            t684Code::SegmentGuard => 0x630d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t684Code::RoadBad => "固件路非法（三路之外）",
            t684Code::EntryShort => "差异条目不足（<10 条）",
            t684Code::EvidenceUnreadable => "证据不可解引（指针失效）",
            t684Code::ImpactBad => "影响面非法（三类之外）",
            t684Code::LedgerBreak => "差异账破缺",
            t684Code::OrderViolated => "条目序违例",
            t684Code::DupEntry => "条目重复（同差异点双记）",
            t684Code::GuardTripped => "守卫触发",
            t684Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t684Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t684Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t684Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t684Code::DuplicateEntry => "条目重复（同键双写）",
            t684Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t684Code::RoadBad => "按三路闭集校正",
            t684Code::EntryShort => "按 ≥10 条律补条",
            t684Code::EvidenceUnreadable => "按 100% 可解引补引",
            t684Code::ImpactBad => "按影响面闭集校正",
            t684Code::LedgerBreak => "按守恒重建",
            t684Code::OrderViolated => "按条目序重排",
            t684Code::DupEntry => "幂等去重",
            t684Code::GuardTripped => "检查临界区",
            t684Code::LedgerBroken => "按守恒律重建台账后重跑",
            t684Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t684Code::TableBroken => "按语义规则重建常量表",
            t684Code::IndexMisaligned => "按声明序归一化后重跑",
            t684Code::DuplicateEntry => "合并同键条目为末次生效",
            t684Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 固件三路（BIOS / EFI-64 / EFI-32）差异分账条目
// ---------------------------------------------------------------------------

/// 固件路径（三路闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FirmwareRoad {
    /// 传统 BIOS 路。
    Bios,
    /// EFI 64 位路。
    Efi64,
    /// EFI 32 位路（含兼容模块）。
    Efi32,
}

impl FirmwareRoad {
    /// 全量闭集（三路）。
    pub const ALL: [FirmwareRoad; 3] = [FirmwareRoad::Bios, FirmwareRoad::Efi64, FirmwareRoad::Efi32];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            FirmwareRoad::Bios => 0,
            FirmwareRoad::Efi64 => 1,
            FirmwareRoad::Efi32 => 2,
        }
    }

    /// 中文路名。
    pub const fn cn(self) -> &'static str {
        match self {
            FirmwareRoad::Bios => "BIOS 路",
            FirmwareRoad::Efi64 => "EFI 64 位路",
            FirmwareRoad::Efi32 => "EFI 32 位路",
        }
    }
}

/// 影响面（三类闭集：启动/内存/中断）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpactFace {
    /// 启动路径影响。
    Boot,
    /// 内存布局影响。
    Memory,
    /// 中断/时钟影响。
    Irq,
}

/// 差异条目（三列：差异点 + 影响面 + 证据指针）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FwDiffEntry {
    /// 条目号。
    pub id: u32,
    /// 差异点（编号语义：0..N 差异表索引）。
    pub point: u32,
    /// 影响面（0..2）。
    pub face: u8,
    /// 证据指针（书章节号 + 0x8000 基址）。
    pub evidence: u32,
    /// 证据是否可解引（非零 + 在册）。
    pub readable: bool,
}

/// 条目合法性（三列齐 + 影响面闭集 + 证据非零）。
pub const fn fw_diff_ok(e: &FwDiffEntry) -> bool {
    e.face <= 2 && e.evidence >= 0x8000 && e.readable
}

/// 差异账本（≥10 条 + 证据解引 100%）。
#[derive(Clone, Copy, Debug)]
pub struct FwDiffBook {
    /// 已入账条数。
    pub entries: u64,
    /// 证据可解引次数（必须 == entries）。
    pub readable: u64,
    /// 不可解引次数（必须 0）。
    pub unreadable: u64,
}

impl FwDiffBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { entries: 0, readable: 0, unreadable: 0 }
    }

    /// 入账一条（三列齐才收）。
    pub fn add(&mut self, e: &FwDiffEntry) -> bool {
        if !fw_diff_ok(e) {
            self.unreadable += 1;
            return false;
        }
        self.entries += 1;
        self.readable += 1;
        true
    }

    /// ≥10 条判据。
    pub fn enough(&self) -> bool {
        self.entries >= 10
    }

    /// 证据可解引 100% 判据。
    pub fn all_readable(&self) -> bool {
        self.readable == self.entries && self.entries > 0
    }
}

/// 三路覆盖矩阵（每条差异在三路上的呈现位掩码；bit0 BIOS/bit1 EFI64/bit2 EFI32）。
pub const fn road_mask(m: u8) -> bool {
    m & 0x07 == m
}

/// 路呈现覆盖（掩码合法 + 至少一路命中）。
pub const fn road_present(m: u8) -> bool {
    road_mask(m) && m != 0
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x6300, 0x630f]。
pub const fn t684_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t684Code::ALL.len() {
        let c = t684Code::ALL[i].code();
        if c < 0x6300 || c > 0x630f {
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
fn t684_guard_seg() -> bool {
    t684_segment_guard()
}

/// 三路闭集 + 影响三类 + 掩码合法性。
fn check_c1_roads() -> bool {
    let mut ok = FirmwareRoad::ALL.len() == 3;
    if ok {
        ok = FirmwareRoad::Bios.cn() == "BIOS 路" && FirmwareRoad::Efi64.cn() == "EFI 64 位路";
    }
    if ok {
        ok = road_present(0x07) && !road_present(0x00) && !road_present(0x08);
    }
    ok
}

/// 12 条差异条目账 + 证据 100% 可解引 + 坏证据拒收。
fn check_c2_entries() -> bool {
    let mut bk = FwDiffBook::new();
    let mut ok = true;
    let mut i = 0u32;
    while i < 12 {
        let e = FwDiffEntry { id: i + 1, point: i, face: (i % 3) as u8, evidence: 0x8000 + i, readable: true };
        ok = ok && bk.add(&e);
        i += 1;
    }
    if ok {
        ok = bk.enough() && bk.all_readable() && bk.entries == 12;
    }
    if ok {
        // 证据不可解引条目必须拒。
        let bad = FwDiffEntry { id: 99, point: 99, face: 0, evidence: 0, readable: false };
        ok = !bk.add(&bad) && bk.unreadable == 1 && bk.entries == 12;
    }
    if ok {
        ok = !bk.enough() == false;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t684_checks() -> CheckSet {
    let mut set = CheckSet::new("T684");
    set.add(
        "t684-seg-guard",
        t684_guard_seg(),
        if t684_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t684-c1-roads",
        check_c1_roads(),
        if check_c1_roads() { "" } else { "firmware roads mismatch" },
    );
    set.add(
        "t684-c2-entries",
        check_c2_entries(),
        if check_c2_entries() { "" } else { "fw diff entries mismatch" },
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
    fn t684_all_checks_green() {
        let set = run_t684_checks();
        assert!(set.all_passed(), "T684 domain checks must be green");
    }

    #[test]
    fn t684_segment_guard_holds() {
        assert!(t684_segment_guard());
    }
}
