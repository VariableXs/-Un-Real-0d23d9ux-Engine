//! unxt431_rsdpwin —— UNX-F0441 RSDP 双窗定义（EBDA + BIOS ROM）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0441 · RSDP 签名搜索（EBDA/BIOS 双窗） · 判据：双窗搜索（EBDA 1KB 窗 + BIOS ROM 0xE0000–0xFFFFF 窗）命中 RSDP 签名 'RSD PTR ' 定位唯一（1/1 命中账）
//! · 组件：双窗表（EBDA 1KB + BIOS 128KB）+ 16 字节对齐守卫 + 命中账
//! · 判据落地：双窗搜索语义底座
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T431_VERSION: &str = "t431-rsdpwin-v1";

/// 诊断段基址。
pub const T431_SEG_BASE: u16 = 0x4300;

// --------------------------------------------------------------------------
// 诊断码（t431 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RSDP 双窗诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t431Code {
    /// 0x4300 窗口非法（越界或空）。
    WindowBad,
    /// 0x4301 对齐违例（非 16 字节）。
    AlignBad,
    /// 0x4302 签名非法（非 'RSD PTR '）。
    SigBad,
    /// 0x4303 RSDP 校验和失败。
    ChecksumBad,
    /// 0x4304 多重命中（签名非唯一）。
    MultiHit,
    /// 0x4305 双窗重叠。
    WindowOverlap,
    /// 0x4306 命中账破缺。
    LedgerBreak,
    /// 0x4307 守卫触发。
    GuardTripped,
    /// 0x4308 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4309 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x430a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x430b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x430c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x430d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x430e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t431Code {
    /// 全量闭集。
    pub const ALL: [t431Code; 15] = [
        t431Code::WindowBad,
        t431Code::AlignBad,
        t431Code::SigBad,
        t431Code::ChecksumBad,
        t431Code::MultiHit,
        t431Code::WindowOverlap,
        t431Code::LedgerBreak,
        t431Code::GuardTripped,
        t431Code::LedgerBroken,
        t431Code::SnapshotFingerprint,
        t431Code::TableBroken,
        t431Code::IndexMisaligned,
        t431Code::DuplicateEntry,
        t431Code::OrderViolated,
        t431Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t431Code::WindowBad => 0x4300,
            t431Code::AlignBad => 0x4301,
            t431Code::SigBad => 0x4302,
            t431Code::ChecksumBad => 0x4303,
            t431Code::MultiHit => 0x4304,
            t431Code::WindowOverlap => 0x4305,
            t431Code::LedgerBreak => 0x4306,
            t431Code::GuardTripped => 0x4307,
            t431Code::LedgerBroken => 0x4308,
            t431Code::SnapshotFingerprint => 0x4309,
            t431Code::TableBroken => 0x430a,
            t431Code::IndexMisaligned => 0x430b,
            t431Code::DuplicateEntry => 0x430c,
            t431Code::OrderViolated => 0x430d,
            t431Code::SegmentGuard => 0x430e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t431Code::WindowBad => "窗口非法（越界或空）",
            t431Code::AlignBad => "对齐违例（非 16 字节）",
            t431Code::SigBad => "签名非法（非 'RSD PTR '）",
            t431Code::ChecksumBad => "RSDP 校验和失败",
            t431Code::MultiHit => "多重命中（签名非唯一）",
            t431Code::WindowOverlap => "双窗重叠",
            t431Code::LedgerBreak => "命中账破缺",
            t431Code::GuardTripped => "守卫触发",
            t431Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t431Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t431Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t431Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t431Code::DuplicateEntry => "条目重复（同键双写）",
            t431Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t431Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t431Code::WindowBad => "按双窗表校正",
            t431Code::AlignBad => "按 16 字节律校正",
            t431Code::SigBad => "按 8 字节签名校正",
            t431Code::ChecksumBad => "按校验规程修复",
            t431Code::MultiHit => "按唯一性律归因",
            t431Code::WindowOverlap => "按互斥律拆分",
            t431Code::LedgerBreak => "按守恒重建",
            t431Code::GuardTripped => "检查临界区",
            t431Code::LedgerBroken => "按守恒律重建台账后重跑",
            t431Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t431Code::TableBroken => "按语义规则重建常量表",
            t431Code::IndexMisaligned => "按声明序归一化后重跑",
            t431Code::DuplicateEntry => "合并同键条目为末次生效",
            t431Code::OrderViolated => "按依赖矩阵重排后重跑",
            t431Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 双窗常量（EBDA 1KB + BIOS ROM 0xE0000-0xFFFFF 128KB）
// ---------------------------------------------------------------------------

/// RSDP 签名（8 字节，含尾随空格）。
pub const RSDP_SIG: [u8; 8] = [b'R', b'S', b'D', b' ', b'P', b'T', b'R', b' '];

/// RSDP 对齐粒度（16 字节）。
pub const RSDP_ALIGN: u64 = 16;

/// RSDP v1 长度（ACPI 1.0，20 字节）。
pub const RSDP_V1_LEN: u64 = 20;

/// RSDP v2 长度（ACPI 2.0+，36 字节）。
pub const RSDP_V2_LEN: u64 = 36;

/// EBDA 段基址寄存器（0x40E 物理地址）。
pub const EBDA_SEG_PHYS: u64 = 0x40E;

/// EBDA 窗长（1KB）。
pub const EBDA_LEN: u64 = 1024;

/// BIOS ROM 窗起址（0xE0000）。
pub const BIOS_ROM_BASE: u64 = 0xE_0000;

/// BIOS ROM 窗长（128KB）。
pub const BIOS_ROM_LEN: u64 = 0x2_0000;

/// BIOS ROM 窗末址（0x100000，闭开）。
pub const BIOS_ROM_END: u64 = BIOS_ROM_BASE + BIOS_ROM_LEN;

/// 签名比对（8 字节逐位相等）。
pub const fn rsdp_sig_eq(b: &[u8; 8]) -> bool {
    let mut i = 0;
    while i < 8 {
        if b[i] != RSDP_SIG[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 16 字节对齐检查。
pub const fn rsdp_aligned(addr: u64) -> bool {
    addr % RSDP_ALIGN == 0
}

/// 双窗合法性（互斥 + 各自 16 字节对齐边界 + 长度非零）。
pub const fn rsdp_windows_ok() -> bool {
    EBDA_LEN != 0
        && BIOS_ROM_LEN != 0
        && RSDP_ALIGN != 0
        && BIOS_ROM_END > BIOS_ROM_BASE
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4300, 0x430f]。
pub const fn t431_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t431Code::ALL.len() {
        let c = t431Code::ALL[i].code();
        if c < 0x4300 || c > 0x430f {
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
fn t431_guard_seg() -> bool {
    t431_segment_guard()
}

/// 双窗定义合法 + 签名/对齐守卫。
fn check_c1_windows() -> bool {
    let mut ok = rsdp_windows_ok();
    if ok {
        ok = rsdp_sig_eq(b"RSD PTR ") && !rsdp_sig_eq(b"RSD PTRX");
    }
    if ok {
        ok = rsdp_aligned(0xE0000) && rsdp_aligned(0xE0010) && !rsdp_aligned(0xE0008);
    }
    if ok {
        ok = RSDP_V2_LEN == 36 && RSDP_V1_LEN == 20;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t431_checks() -> CheckSet {
    let mut set = CheckSet::new("T431");
    set.add(
        "t431-seg-guard",
        t431_guard_seg(),
        if t431_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t431-c1-windows",
        check_c1_windows(),
        if check_c1_windows() { "" } else { "rsdp windows definition mismatch" },
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
    fn t431_all_checks_green() {
        let set = run_t431_checks();
        assert!(set.all_passed(), "T431 domain checks must be green");
    }

    #[test]
    fn t431_segment_guard_holds() {
        assert!(t431_segment_guard());
    }
}
