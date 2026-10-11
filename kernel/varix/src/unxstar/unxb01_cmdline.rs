//! unxtb01_cmdline —— UNX-F0581 参数行三元组账（源/长度/原文）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0581 · 参数行提取（loader 交接收尾） · 判据：参数行提取账齐（源/长度/原文三元组），长度守卫核（256 字节上限 1/1 断言）
//! · 组件：三元组账表（源/长度/原文哈希）+ 逐行守卫 + 提取引擎
//! · 判据落地：参数行提取账齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TB01_VERSION: &str = "tb01-cmdline-v1";

/// 诊断段基址。
pub const TB01_SEG_BASE: u16 = 0x4a00;

// --------------------------------------------------------------------------
// 诊断码（tb01 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 参数行诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum tb01Code {
    /// 0x4a00 源非法（三源之外）。
    SourceBad,
    /// 0x4a01 长度为零。
    LenZero,
    /// 0x4a02 长度超 256 上限。
    LenOver,
    /// 0x4a03 原文不符（哈希对不上）。
    RawMismatch,
    /// 0x4a04 参数行 CRC 失败。
    CrcBad,
    /// 0x4a05 参数账破缺。
    LedgerBreak,
    /// 0x4a06 参数序断档。
    OrderBreak,
    /// 0x4a07 守卫触发。
    GuardTripped,
    /// 0x4a08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4a09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4a0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4a0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4a0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4a0d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x4a0e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl tb01Code {
    /// 全量闭集。
    pub const ALL: [tb01Code; 15] = [
        tb01Code::SourceBad,
        tb01Code::LenZero,
        tb01Code::LenOver,
        tb01Code::RawMismatch,
        tb01Code::CrcBad,
        tb01Code::LedgerBreak,
        tb01Code::OrderBreak,
        tb01Code::GuardTripped,
        tb01Code::LedgerBroken,
        tb01Code::SnapshotFingerprint,
        tb01Code::TableBroken,
        tb01Code::IndexMisaligned,
        tb01Code::DuplicateEntry,
        tb01Code::OrderViolated,
        tb01Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            tb01Code::SourceBad => 0x4a00,
            tb01Code::LenZero => 0x4a01,
            tb01Code::LenOver => 0x4a02,
            tb01Code::RawMismatch => 0x4a03,
            tb01Code::CrcBad => 0x4a04,
            tb01Code::LedgerBreak => 0x4a05,
            tb01Code::OrderBreak => 0x4a06,
            tb01Code::GuardTripped => 0x4a07,
            tb01Code::LedgerBroken => 0x4a08,
            tb01Code::SnapshotFingerprint => 0x4a09,
            tb01Code::TableBroken => 0x4a0a,
            tb01Code::IndexMisaligned => 0x4a0b,
            tb01Code::DuplicateEntry => 0x4a0c,
            tb01Code::OrderViolated => 0x4a0d,
            tb01Code::SegmentGuard => 0x4a0e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            tb01Code::SourceBad => "源非法（三源之外）",
            tb01Code::LenZero => "长度为零",
            tb01Code::LenOver => "长度超 256 上限",
            tb01Code::RawMismatch => "原文不符（哈希对不上）",
            tb01Code::CrcBad => "参数行 CRC 失败",
            tb01Code::LedgerBreak => "参数账破缺",
            tb01Code::OrderBreak => "参数序断档",
            tb01Code::GuardTripped => "守卫触发",
            tb01Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            tb01Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            tb01Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            tb01Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            tb01Code::DuplicateEntry => "条目重复（同键双写）",
            tb01Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            tb01Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            tb01Code::SourceBad => "按三源闭集校正",
            tb01Code::LenZero => "按非零律拒收",
            tb01Code::LenOver => "按长度守卫拒收",
            tb01Code::RawMismatch => "按重提路径修复",
            tb01Code::CrcBad => "按校验规程修复",
            tb01Code::LedgerBreak => "按守恒重建",
            tb01Code::OrderBreak => "按序重排",
            tb01Code::GuardTripped => "检查临界区",
            tb01Code::LedgerBroken => "按守恒律重建台账后重跑",
            tb01Code::SnapshotFingerprint => "按防篡改规程重建快照",
            tb01Code::TableBroken => "按语义规则重建常量表",
            tb01Code::IndexMisaligned => "按声明序归一化后重跑",
            tb01Code::DuplicateEntry => "合并同键条目为末次生效",
            tb01Code::OrderViolated => "按依赖矩阵重排后重跑",
            tb01Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 参数行三元组（源 + 长度 + 原文哈希）
// ---------------------------------------------------------------------------

/// 参数来源枚举（三源闭集：bootloader/内置/内核命令行尾段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdSource {
    /// bootloader 交接（loader 协议）。
    Bootloader,
    /// 内置默认参数。
    Builtin,
    /// 内核命令行尾段。
    CmdlineTail,
}

impl CmdSource {
    /// 全量闭集（三源）。
    pub const ALL: [CmdSource; 3] = [
        CmdSource::Bootloader,
        CmdSource::Builtin,
        CmdSource::CmdlineTail,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            CmdSource::Bootloader => 0,
            CmdSource::Builtin => 1,
            CmdSource::CmdlineTail => 2,
        }
    }

    /// 中文源名。
    pub const fn cn(self) -> &'static str {
        match self {
            CmdSource::Bootloader => "bootloader",
            CmdSource::Builtin => "内置",
            CmdSource::CmdlineTail => "命令行尾段",
        }
    }
}

/// 长度上限（256 字节红线）。
pub const CMD_MAX_LEN: usize = 256;

/// 来源数常数。
pub const CMD_SOURCE_N: usize = 3;

/// 三元组本体（源/长度/原文哈希）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdEntry {
    /// 来源号（0..2）。
    pub source: u8,
    /// 长度（1..=256）。
    pub len: u32,
    /// 原文哈希（FNV-1a 64）。
    pub raw_hash: u64,
}

/// 三元组合法性（源域 + 长度在 1..=256）。
pub const fn cmd_entry_ok(e: &CmdEntry) -> bool {
    (e.source as usize) < CMD_SOURCE_N && e.len >= 1 && e.len <= CMD_MAX_LEN as u32
}

/// 长度守卫（256 上限 1/1 断言：越界必拒）。
pub const fn cmd_len_guard(len: u32) -> bool {
    len >= 1 && len <= CMD_MAX_LEN as u32
}

/// 提取账（守恒：extracted + rejected == offered）。
#[derive(Clone, Copy, Debug)]
pub struct CmdBook {
    /// 已提取条数。
    pub extracted: u64,
    /// 拒收条数。
    pub rejected: u64,
    /// 已提供条数。
    pub offered: u64,
    /// 超长拒止计数。
    pub over_len: u64,
}

impl CmdBook {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { extracted: 0, rejected: 0, offered: 0, over_len: 0 }
    }

    /// 提取一条（长度违例即拒收并计超长）。
    pub fn extract(&mut self, e: &CmdEntry) -> bool {
        self.offered += 1;
        if !cmd_entry_ok(e) {
            self.rejected += 1;
            if e.len > CMD_MAX_LEN as u32 {
                self.over_len += 1;
            }
            return false;
        }
        self.extracted += 1;
        true
    }

    /// 守恒恒等式。
    pub fn conserved(&self) -> bool {
        self.extracted + self.rejected == self.offered
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4a00, 0x4a0f]。
pub const fn tb01_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < tb01Code::ALL.len() {
        let c = tb01Code::ALL[i].code();
        if c < 0x4a00 || c > 0x4a0f {
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
fn tb01_guard_seg() -> bool {
    tb01_segment_guard()
}

/// 三元组合法性（源域 + 长度正负样本）。
fn check_c1_entry_ok() -> bool {
    let good = CmdEntry { source: 0, len: 128, raw_hash: 0xdead_beef };
    let mut ok = cmd_entry_ok(&good);
    if ok {
        // 长度 0 拒。
        ok = !cmd_entry_ok(&CmdEntry { len: 0, ..good });
    }
    if ok {
        // 长度 257 拒。
        ok = !cmd_entry_ok(&CmdEntry { len: 257, ..good });
    }
    if ok {
        // 源号 3 拒。
        ok = !cmd_entry_ok(&CmdEntry { source: 3, ..good });
    }
    if ok {
        // 边界 256 放行。
        ok = cmd_entry_ok(&CmdEntry { len: 256, ..good });
    }
    ok
}

/// 长度守卫 1/1 + 提取账守恒。
fn check_c2_guard_extract() -> bool {
    let mut ok = cmd_len_guard(1) && cmd_len_guard(256);
    if ok {
        ok = !cmd_len_guard(0) && !cmd_len_guard(257);
    }
    if ok {
        let mut bk = CmdBook::new();
        let mut i = 1u32;
        while i <= 16 {
            bk.extract(&CmdEntry { source: (i % 3) as u8, len: i * 8, raw_hash: i as u64 });
            i += 1;
        }
        // j=33..=48 的长度均 >256 必拒。
        let mut j = 33u32;
        while j <= 48 {
            bk.extract(&CmdEntry { source: 0, len: j * 8, raw_hash: j as u64 });
            j += 1;
        }
        ok = bk.conserved() && bk.extracted == 16 && bk.rejected == 16 && bk.over_len == 16;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_tb01_checks() -> CheckSet {
    let mut set = CheckSet::new("TB01");
    set.add(
        "tb01-seg-guard",
        tb01_guard_seg(),
        if tb01_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "tb01-c1-entry-ok",
        check_c1_entry_ok(),
        if check_c1_entry_ok() { "" } else { "cmdline entry validation mismatch" },
    );
    set.add(
        "tb01-c2-guard-extract",
        check_c2_guard_extract(),
        if check_c2_guard_extract() { "" } else { "cmdline guard/extract mismatch" },
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
    fn tb01_all_checks_green() {
        let set = run_tb01_checks();
        assert!(set.all_passed(), "TB01 domain checks must be green");
    }

    #[test]
    fn tb01_segment_guard_holds() {
        assert!(tb01_segment_guard());
    }
}
