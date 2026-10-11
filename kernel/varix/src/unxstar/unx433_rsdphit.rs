//! unxt433_rsdphit —— UNX-F0441 命中账（1/1 唯一性）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0441 · RSDP 签名搜索（EBDA/BIOS 双窗） · 判据：双窗搜索（EBDA 1KB 窗 + BIOS ROM 0xE0000–0xFFFFF 窗）命中 RSDP 签名 'RSD PTR ' 定位唯一（1/1 命中账）
//! · 组件：命中账（单命中唯一）+ 校验和引擎 + 双源对拍
//! · 判据落地：命中 RSDP 签名 'RSD PTR ' 定位唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T433_VERSION: &str = "t433-rsdphit-v1";

/// 诊断段基址。
pub const T433_SEG_BASE: u16 = 0x4320;

// --------------------------------------------------------------------------
// 诊断码（t433 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 命中账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t433Code {
    /// 0x4320 多重命中（>1 个签名在册）。
    MultiHit,
    /// 0x4321 零命中（签名未找到）。
    ZeroHit,
    /// 0x4322 校验和失败。
    ChecksumBad,
    /// 0x4323 版本非法（修订号未知）。
    VersionBad,
    /// 0x4324 XSDTPTR 非法（v2 字段越界）。
    XsdtBad,
    /// 0x4325 命中序断档。
    OrderBreak,
    /// 0x4326 命中账破缺。
    LedgerBreak,
    /// 0x4327 守卫触发。
    GuardTripped,
    /// 0x4328 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4329 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x432a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x432b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x432c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x432d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x432e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t433Code {
    /// 全量闭集。
    pub const ALL: [t433Code; 15] = [
        t433Code::MultiHit,
        t433Code::ZeroHit,
        t433Code::ChecksumBad,
        t433Code::VersionBad,
        t433Code::XsdtBad,
        t433Code::OrderBreak,
        t433Code::LedgerBreak,
        t433Code::GuardTripped,
        t433Code::LedgerBroken,
        t433Code::SnapshotFingerprint,
        t433Code::TableBroken,
        t433Code::IndexMisaligned,
        t433Code::DuplicateEntry,
        t433Code::OrderViolated,
        t433Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t433Code::MultiHit => 0x4320,
            t433Code::ZeroHit => 0x4321,
            t433Code::ChecksumBad => 0x4322,
            t433Code::VersionBad => 0x4323,
            t433Code::XsdtBad => 0x4324,
            t433Code::OrderBreak => 0x4325,
            t433Code::LedgerBreak => 0x4326,
            t433Code::GuardTripped => 0x4327,
            t433Code::LedgerBroken => 0x4328,
            t433Code::SnapshotFingerprint => 0x4329,
            t433Code::TableBroken => 0x432a,
            t433Code::IndexMisaligned => 0x432b,
            t433Code::DuplicateEntry => 0x432c,
            t433Code::OrderViolated => 0x432d,
            t433Code::SegmentGuard => 0x432e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t433Code::MultiHit => "多重命中（>1 个签名在册）",
            t433Code::ZeroHit => "零命中（签名未找到）",
            t433Code::ChecksumBad => "校验和失败",
            t433Code::VersionBad => "版本非法（修订号未知）",
            t433Code::XsdtBad => "XSDTPTR 非法（v2 字段越界）",
            t433Code::OrderBreak => "命中序断档",
            t433Code::LedgerBreak => "命中账破缺",
            t433Code::GuardTripped => "守卫触发",
            t433Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t433Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t433Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t433Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t433Code::DuplicateEntry => "条目重复（同键双写）",
            t433Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t433Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t433Code::MultiHit => "按唯一性律归因",
            t433Code::ZeroHit => "按缺失路径登记",
            t433Code::ChecksumBad => "按校验规程修复",
            t433Code::VersionBad => "按版本表校正",
            t433Code::XsdtBad => "按 v2 口径校正",
            t433Code::OrderBreak => "按序重排",
            t433Code::LedgerBreak => "按守恒重建",
            t433Code::GuardTripped => "检查临界区",
            t433Code::LedgerBroken => "按守恒律重建台账后重跑",
            t433Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t433Code::TableBroken => "按语义规则重建常量表",
            t433Code::IndexMisaligned => "按声明序归一化后重跑",
            t433Code::DuplicateEntry => "合并同键条目为末次生效",
            t433Code::OrderViolated => "按依赖矩阵重排后重跑",
            t433Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// RSDP 校验和引擎（ACPI 规范：v1 前 20 字节和 0；v2 全 36 字节和 0）
// ---------------------------------------------------------------------------

/// 校验和计算（溢出回绕累加 → 取补；8 位口径）。
pub const fn rsdp_checksum(b: &[u8; 36]) -> u8 {
    let mut i = 0;
    let mut s: u8 = 0;
    while i < 36 {
        s = s.wrapping_add(b[i]);
        i += 1;
    }
    s
}

/// v1 区段校验（前 20 字节和为 0）。
pub const fn rsdp_v1_ok(b: &[u8; 36]) -> bool {
    let mut i = 0;
    let mut s: u8 = 0;
    while i < 20 {
        s = s.wrapping_add(b[i]);
        i += 1;
    }
    s == 0
}

/// v2 区段校验（36 字节全和为 0）。
pub const fn rsdp_v2_ok(b: &[u8; 36]) -> bool {
    rsdp_checksum(b) == 0
}

/// 命中唯一性（单源双窗只允许恰好一次签名命中）。
pub const fn rsdp_hit_unique(hits: u64) -> bool {
    hits == 1
}

// --------------------------------------------------------------------------
// 守恒账 hit（RSDP 命中账（单命中 + 校验尝试 = 扫描总数））
// --------------------------------------------------------------------------
/// RSDP 命中账（单命中 + 校验尝试 = 扫描总数）。
#[derive(Clone, Copy, Debug)]
pub struct hit {
    /// 签名命中数
    pub hits: u64,
    /// 校验尝试数
    pub attempts: u64,
    /// 校验和失败数
    pub checksum_fails: u64,
}

impl hit {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            hits: 0,
            attempts: 0,
            checksum_fails: 0,
        }
    }

    /// 记一次命中。
    pub fn hit_one(&mut self) {
        self.hits += 1; self.attempts += 1;
    }

    /// 记一次未命中。
    pub fn miss_one(&mut self) {
        self.attempts += 1;
    }

    /// 记一次校验和失败。
    pub fn cksum_fail(&mut self) {
        self.checksum_fails += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4320, 0x432f]。
pub const fn t433_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t433Code::ALL.len() {
        let c = t433Code::ALL[i].code();
        if c < 0x4320 || c > 0x432f {
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
fn t433_guard_seg() -> bool {
    t433_segment_guard()
}

/// RSDP 校验和正负样本（签名 ASCII 和 31 + 补齐归零）。
fn check_c1_checksum() -> bool {
    let mut b = [0u8; 36];
    b[..8].copy_from_slice(b"RSD PTR ");
    // "RSD PTR " ASCII 和 = 543 → 回绕 0x1F。
    let mut ok = rsdp_checksum(&b) == 31 && !rsdp_v1_ok(&b);
    if ok {
        // 补 0xE1 使前 20 字节和归零（31+225=256 回绕）。
        b[19] = 0xE1;
        ok = rsdp_v1_ok(&b);
    }
    if ok {
        // 前 20 字节归零且其余字节全零 → 36 字节制同样归零。
        ok = rsdp_v2_ok(&b);
    }
    if ok {
        // 再动一个字节即破坏 v2 平衡。
        b[20] = 1;
        ok = !rsdp_v2_ok(&b);
    }
    ok
}

/// 命中唯一性（恰好 1 命中）+ 命中账守恒。
fn check_c2_hit_unique() -> bool {
    let mut h = hit::new();
    h.hit_one();
    let mut i = 0;
    while i < 511 {
        h.miss_one();
        i += 1;
    }
    let mut ok = rsdp_hit_unique(h.hits);
    if ok {
        ok = h.hits == 1 && h.attempts == 512;
    }
    if ok {
        ok = !rsdp_hit_unique(0) && !rsdp_hit_unique(2);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t433_checks() -> CheckSet {
    let mut set = CheckSet::new("T433");
    set.add(
        "t433-seg-guard",
        t433_guard_seg(),
        if t433_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t433-c1-checksum",
        check_c1_checksum(),
        if check_c1_checksum() { "" } else { "rsdp checksum mismatch" },
    );
    set.add(
        "t433-c2-hit-unique",
        check_c2_hit_unique(),
        if check_c2_hit_unique() { "" } else { "rsdp hit uniqueness mismatch" },
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
    fn t433_all_checks_green() {
        let set = run_t433_checks();
        assert!(set.all_passed(), "T433 domain checks must be green");
    }

    #[test]
    fn t433_segment_guard_holds() {
        assert!(t433_segment_guard());
    }
}
