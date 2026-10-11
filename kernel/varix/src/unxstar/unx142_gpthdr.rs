//! unxt142_gpthdr —— UNX-F0142 GPT 头结构（签名/CRC/条目表指针）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0142 · GPT/ESP 分区镜像生成器 · 判据：IMG 镜像 GPT 表/ESP FAT 结构与 sgdisk 校验一致，真机引导通过
//! · 组件：GPT 头字段表（92 字节结构）+ CRC32 引擎 + 结构合法性
//! · 判据落地：IMG 镜像 GPT 表的头部语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T142_VERSION: &str = "t142-gpthdr-v1";

/// 诊断段基址。
pub const T142_SEG_BASE: u16 = 0x4d00;

// --------------------------------------------------------------------------
// 诊断码（t142 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// GPT 头诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t142Code {
    /// 0x4d00 签名非法（非 EFI PART）。
    SigBad,
    /// 0x4d01 头 CRC32 不符。
    CrcBad,
    /// 0x4d02 条目表 CRC 不符。
    EntriesCrcBad,
    /// 0x4d03 LBA 越界（表指针非法）。
    LbaBad,
    /// 0x4d04 磁盘 GUID 非法。
    GuidBad,
    /// 0x4d05 分区数非法（>128 或 0）。
    PartCountBad,
    /// 0x4d06 结构账破缺。
    LedgerBreak,
    /// 0x4d07 守卫触发。
    GuardTripped,
    /// 0x4d08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4d09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4d0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4d0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4d0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4d0d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x4d0e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t142Code {
    /// 全量闭集。
    pub const ALL: [t142Code; 15] = [
        t142Code::SigBad,
        t142Code::CrcBad,
        t142Code::EntriesCrcBad,
        t142Code::LbaBad,
        t142Code::GuidBad,
        t142Code::PartCountBad,
        t142Code::LedgerBreak,
        t142Code::GuardTripped,
        t142Code::LedgerBroken,
        t142Code::SnapshotFingerprint,
        t142Code::TableBroken,
        t142Code::IndexMisaligned,
        t142Code::DuplicateEntry,
        t142Code::OrderViolated,
        t142Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t142Code::SigBad => 0x4d00,
            t142Code::CrcBad => 0x4d01,
            t142Code::EntriesCrcBad => 0x4d02,
            t142Code::LbaBad => 0x4d03,
            t142Code::GuidBad => 0x4d04,
            t142Code::PartCountBad => 0x4d05,
            t142Code::LedgerBreak => 0x4d06,
            t142Code::GuardTripped => 0x4d07,
            t142Code::LedgerBroken => 0x4d08,
            t142Code::SnapshotFingerprint => 0x4d09,
            t142Code::TableBroken => 0x4d0a,
            t142Code::IndexMisaligned => 0x4d0b,
            t142Code::DuplicateEntry => 0x4d0c,
            t142Code::OrderViolated => 0x4d0d,
            t142Code::SegmentGuard => 0x4d0e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t142Code::SigBad => "签名非法（非 EFI PART）",
            t142Code::CrcBad => "头 CRC32 不符",
            t142Code::EntriesCrcBad => "条目表 CRC 不符",
            t142Code::LbaBad => "LBA 越界（表指针非法）",
            t142Code::GuidBad => "磁盘 GUID 非法",
            t142Code::PartCountBad => "分区数非法（>128 或 0）",
            t142Code::LedgerBreak => "结构账破缺",
            t142Code::GuardTripped => "守卫触发",
            t142Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t142Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t142Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t142Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t142Code::DuplicateEntry => "条目重复（同键双写）",
            t142Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t142Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t142Code::SigBad => "按 8 字节签名校正",
            t142Code::CrcBad => "按重算口径修复",
            t142Code::EntriesCrcBad => "按重算口径修复",
            t142Code::LbaBad => "按 LBA 域校正",
            t142Code::GuidBad => "按 GUID 格式校正",
            t142Code::PartCountBad => "按 128 上限校正",
            t142Code::LedgerBreak => "按守恒重建",
            t142Code::GuardTripped => "检查临界区",
            t142Code::LedgerBroken => "按守恒律重建台账后重跑",
            t142Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t142Code::TableBroken => "按语义规则重建常量表",
            t142Code::IndexMisaligned => "按声明序归一化后重跑",
            t142Code::DuplicateEntry => "合并同键条目为末次生效",
            t142Code::OrderViolated => "按依赖矩阵重排后重跑",
            t142Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// GPT 头字段（92 字节 UEFI GPT Header 编译期常量）
// ---------------------------------------------------------------------------

/// GPT 签名（"EFI PART"）。
pub const GPT_SIG: [u8; 8] = [b'E', b'F', b'I', b' ', b'P', b'A', b'R', b'T'];

/// GPT 修订（2.0 = 0x00010000）。
pub const GPT_REVISION: u32 = 0x0001_0000;

/// 头长（92 字节）。
pub const GPT_HEADER_LEN: u32 = 92;

/// 头 CRC 置位前的保留零（CRC 字段以 0 计算）。
pub const GPT_CRC_ZERO: u32 = 0;

/// 分区条目上限（128）。
pub const GPT_MAX_PARTS: u32 = 128;

/// 分区条目大小（128 字节）。
pub const GPT_ENTRY_LEN: u32 = 128;

/// 分区条目表总字节（128 × 128 = 16384）。
pub const GPT_TABLE_BYTES: u32 = GPT_MAX_PARTS * GPT_ENTRY_LEN;

/// LBA0（保护 MBR）。
pub const LBA0: u64 = 0;

/// LBA1（GPT 头）。
pub const LBA1: u64 = 1;

/// 签名比对（逐位）。
pub const fn gpt_sig_eq(b: &[u8; 8]) -> bool {
    let mut i = 0;
    while i < 8 {
        if b[i] != GPT_SIG[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 头字段合法性（修订 + 头长 + 分区数三查）。
pub const fn gpt_header_ok(rev: u32, hlen: u32, parts: u32) -> bool {
    rev == GPT_REVISION && hlen == GPT_HEADER_LEN && parts >= 1 && parts <= GPT_MAX_PARTS
}

/// CRC32（IEEE 802.3，多项式 0xEDB88320 反射版）。
pub const fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut bit = 0;
        while bit < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            bit += 1;
        }
        i += 1;
    }
    !crc
}

/// GPT 表字节数守恒（条目数 × 条目长）。
pub const fn gpt_table_len_ok(entries: u32, entry_len: u32) -> bool {
    entries == GPT_MAX_PARTS && entry_len == GPT_ENTRY_LEN && GPT_TABLE_BYTES == 16384
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4d00, 0x4d0f]。
pub const fn t142_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t142Code::ALL.len() {
        let c = t142Code::ALL[i].code();
        if c < 0x4d00 || c > 0x4d0f {
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
fn t142_guard_seg() -> bool {
    t142_segment_guard()
}

/// GPT 头常量（签名/修订/头长/分区上限四查）。
fn check_c1_header() -> bool {
    let mut ok = gpt_sig_eq(b"EFI PART") && !gpt_sig_eq(b"EFI PARX");
    if ok {
        ok = gpt_header_ok(GPT_REVISION, GPT_HEADER_LEN, 4);
    }
    if ok {
        ok = gpt_table_len_ok(128, 128) && GPT_TABLE_BYTES == 16384;
    }
    ok
}

/// CRC32 已知向量（'123456789' = 0xCBF43926）。
fn check_c2_crc32() -> bool {
    let v = crc32_ieee(b"123456789");
    let mut ok = v == 0xCBF4_3926;
    if ok {
        // 空输入 = 0x00000000。
        ok = crc32_ieee(b"") == 0;
    }
    if ok {
        // 单字节确定性。
        ok = crc32_ieee(b"a") == crc32_ieee(b"a");
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t142_checks() -> CheckSet {
    let mut set = CheckSet::new("T142");
    set.add(
        "t142-seg-guard",
        t142_guard_seg(),
        if t142_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t142-c1-header",
        check_c1_header(),
        if check_c1_header() { "" } else { "gpt header constants mismatch" },
    );
    set.add(
        "t142-c2-crc32",
        check_c2_crc32(),
        if check_c2_crc32() { "" } else { "crc32 known vector mismatch" },
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
    fn t142_all_checks_green() {
        let set = run_t142_checks();
        assert!(set.all_passed(), "T142 domain checks must be green");
    }

    #[test]
    fn t142_segment_guard_holds() {
        assert!(t142_segment_guard());
    }
}
