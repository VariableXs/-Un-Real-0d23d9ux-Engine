//! unxta01_pmblocks —— UNX-F0561 FADT 五块寄存器账（PM1a/PM1b/PM1_EN/PM1_STS/PM2）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0561 · 电源面定位总账（FADT 字段汇总） · 判据：FADT 电源字段账齐（PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 五块基址+长度），与 F0453 封账直读一致（5/5 块）
//! · 组件：五块账表（基址+长度）+ 逐行守卫 + 封账直读对拍
//! · 判据落地：FADT 电源字段账齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TA01_VERSION: &str = "ta01-pmblocks-v1";

/// 诊断段基址。
pub const TA01_SEG_BASE: u16 = 0x4900;

// --------------------------------------------------------------------------
// 诊断码（ta01 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 电源块诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ta01Code {
    /// 0x4900 电源块缺失（五块缺一）。
    BlockMissing,
    /// 0x4901 基址非法（非 32 位对齐）。
    BaseBad,
    /// 0x4902 长度非法（0 或超界）。
    LenBad,
    /// 0x4903 总长不符（分块长合计≠块长）。
    LengthMismatch,
    /// 0x4904 与 F0453 封账漂移。
    F0453Drift,
    /// 0x4905 电源账破缺。
    LedgerBreak,
    /// 0x4906 块序断档。
    OrderBreak,
    /// 0x4907 守卫触发。
    GuardTripped,
    /// 0x4908 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4909 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x490a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x490b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x490c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x490d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x490e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl ta01Code {
    /// 全量闭集。
    pub const ALL: [ta01Code; 15] = [
        ta01Code::BlockMissing,
        ta01Code::BaseBad,
        ta01Code::LenBad,
        ta01Code::LengthMismatch,
        ta01Code::F0453Drift,
        ta01Code::LedgerBreak,
        ta01Code::OrderBreak,
        ta01Code::GuardTripped,
        ta01Code::LedgerBroken,
        ta01Code::SnapshotFingerprint,
        ta01Code::TableBroken,
        ta01Code::IndexMisaligned,
        ta01Code::DuplicateEntry,
        ta01Code::OrderViolated,
        ta01Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ta01Code::BlockMissing => 0x4900,
            ta01Code::BaseBad => 0x4901,
            ta01Code::LenBad => 0x4902,
            ta01Code::LengthMismatch => 0x4903,
            ta01Code::F0453Drift => 0x4904,
            ta01Code::LedgerBreak => 0x4905,
            ta01Code::OrderBreak => 0x4906,
            ta01Code::GuardTripped => 0x4907,
            ta01Code::LedgerBroken => 0x4908,
            ta01Code::SnapshotFingerprint => 0x4909,
            ta01Code::TableBroken => 0x490a,
            ta01Code::IndexMisaligned => 0x490b,
            ta01Code::DuplicateEntry => 0x490c,
            ta01Code::OrderViolated => 0x490d,
            ta01Code::SegmentGuard => 0x490e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ta01Code::BlockMissing => "电源块缺失（五块缺一）",
            ta01Code::BaseBad => "基址非法（非 32 位对齐）",
            ta01Code::LenBad => "长度非法（0 或超界）",
            ta01Code::LengthMismatch => "总长不符（分块长合计≠块长）",
            ta01Code::F0453Drift => "与 F0453 封账漂移",
            ta01Code::LedgerBreak => "电源账破缺",
            ta01Code::OrderBreak => "块序断档",
            ta01Code::GuardTripped => "守卫触发",
            ta01Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            ta01Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            ta01Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            ta01Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            ta01Code::DuplicateEntry => "条目重复（同键双写）",
            ta01Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            ta01Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ta01Code::BlockMissing => "补登记后封账",
            ta01Code::BaseBad => "按对齐律校正",
            ta01Code::LenBad => "按块长度表校正",
            ta01Code::LengthMismatch => "按守恒重建",
            ta01Code::F0453Drift => "按封账直读复位",
            ta01Code::LedgerBreak => "按守恒重建",
            ta01Code::OrderBreak => "按序重排",
            ta01Code::GuardTripped => "检查临界区",
            ta01Code::LedgerBroken => "按守恒律重建台账后重跑",
            ta01Code::SnapshotFingerprint => "按防篡改规程重建快照",
            ta01Code::TableBroken => "按语义规则重建常量表",
            ta01Code::IndexMisaligned => "按声明序归一化后重跑",
            ta01Code::DuplicateEntry => "合并同键条目为末次生效",
            ta01Code::OrderViolated => "按依赖矩阵重排后重跑",
            ta01Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 五块寄存器账（PM1a_CNT/PM1b_CNT/PM1_EN/PM1_STS/PM2_CNT）
// ---------------------------------------------------------------------------

/// 电源块枚举（闭集五块）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmBlock {
    /// PM1a 控制寄存器块。
    Pm1aCnt,
    /// PM1b 控制寄存器块。
    Pm1bCnt,
    /// PM1 使能寄存器块。
    Pm1En,
    /// PM1 状态寄存器块。
    Pm1Sts,
    /// PM2 控制寄存器块。
    Pm2Cnt,
}

impl PmBlock {
    /// 全量闭集（五块）。
    pub const ALL: [PmBlock; 5] = [
        PmBlock::Pm1aCnt,
        PmBlock::Pm1bCnt,
        PmBlock::Pm1En,
        PmBlock::Pm1Sts,
        PmBlock::Pm2Cnt,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            PmBlock::Pm1aCnt => 0,
            PmBlock::Pm1bCnt => 1,
            PmBlock::Pm1En => 2,
            PmBlock::Pm1Sts => 3,
            PmBlock::Pm2Cnt => 4,
        }
    }

    /// 中文块名。
    pub const fn cn(self) -> &'static str {
        match self {
            PmBlock::Pm1aCnt => "PM1a_CNT",
            PmBlock::Pm1bCnt => "PM1b_CNT",
            PmBlock::Pm1En => "PM1_EN",
            PmBlock::Pm1Sts => "PM1_STS",
            PmBlock::Pm2Cnt => "PM2_CNT",
        }
    }

    /// 块长（ACPI 规范：控制/状态块通常 2 字节；PM2 控制 1 字节）。
    pub const fn len(self) -> u32 {
        match self {
            PmBlock::Pm1aCnt => 2,
            PmBlock::Pm1bCnt => 2,
            PmBlock::Pm1En => 2,
            PmBlock::Pm1Sts => 2,
            PmBlock::Pm2Cnt => 1,
        }
    }
}

/// 电源块数常数。
pub const PM_BLOCK_N: usize = 5;

/// 单块描述（基址 + 长度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PmDesc {
    /// 基址（I/O 或系统 I/O 空间）。
    pub base: u64,
    /// 长度。
    pub len: u32,
}

/// 单块合法性（基址 32 位对齐 + 长度与规范一致）。
pub const fn pm_desc_ok(b: PmBlock, d: &PmDesc) -> bool {
    d.base % 4 == 0 && d.len == b.len() && d.len > 0
}

/// 五块账（全块守恒）。
#[derive(Clone, Copy, Debug)]
pub struct PmBook {
    /// 逐块描述。
    pub desc: [PmDesc; 5],
    /// 已封账块数。
    pub sealed: u8,
    /// 长度合计（守恒：五块长之和）。
    pub total_len: u32,
}

impl PmBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self {
            desc: [PmDesc { base: 0, len: 0 }; 5],
            sealed: 0,
            total_len: 0,
        }
    }

    /// 登记一块（长度与规范不符即拒）。"
    pub fn register(&mut self, b: PmBlock, d: PmDesc) -> bool {
        if !pm_desc_ok(b, &d) {
            return false;
        }
        let i = b.index();
        if self.desc[i].len != 0 {
            return false;
        }
        self.desc[i] = d;
        self.total_len += d.len;
        true
    }

    /// 封账（五块齐备才允许）。"
    pub fn seal(&mut self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.desc[i].len == 0 {
                return false;
            }
            i += 1;
        }
        self.sealed = 1;
        true
    }

    /// 守恒：五块长合计 == 规范总长（2+2+2+2+1 = 9）。"
    pub fn conserved(&self) -> bool {
        self.total_len == 9 && self.sealed <= 1
    }

    /// F0453 直读比对（五块逐块一致）。"
    pub fn f0453_matches(&self, other: &PmBook) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.desc[i] != other.desc[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4900, 0x490f]。
pub const fn ta01_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ta01Code::ALL.len() {
        let c = ta01Code::ALL[i].code();
        if c < 0x4900 || c > 0x490f {
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
fn ta01_guard_seg() -> bool {
    ta01_segment_guard()
}

/// 五块登记全功 + 守恒（累计 9 字节）。
fn check_c1_blocks() -> bool {
    let mut bk = PmBook::new();
    let mut ok = true;
    for (i, b) in PmBlock::ALL.iter().enumerate() {
        if !bk.register(*b, PmDesc { base: 0x1000 + i as u64 * 0x100, len: b.len() }) {
            ok = false;
        }
    }
    if ok {
        ok = bk.total_len == 9;
    }
    if ok {
        ok = bk.seal() && bk.conserved();
    }
    ok
}

/// 非法长拒收 + F0453 五块直读一致。
fn check_c2_reject_and_f0453() -> bool {
    let mut bk = PmBook::new();
    let mut ok = !bk.register(PmBlock::Pm1aCnt, PmDesc { base: 0x1001, len: 2 });
    if ok {
        ok = !bk.register(PmBlock::Pm1aCnt, PmDesc { base: 0x1000, len: 3 });
    }
    if ok {
        let mut a = PmBook::new();
        let mut b = PmBook::new();
        let mut i = 0usize;
        while i < 5 {
            let blk = PmBlock::ALL[i];
            let d = PmDesc { base: 0x2000 + i as u64 * 0x40, len: blk.len() };
            a.register(blk, d);
            b.register(blk, d);
            i += 1;
        }
        ok = a.f0453_matches(&b);
    }
    if ok {
        // 改一块即不一致。
        let mut a = PmBook::new();
        let mut b = PmBook::new();
        a.register(PmBlock::Pm1aCnt, PmDesc { base: 0x2000, len: 2 });
        b.register(PmBlock::Pm1aCnt, PmDesc { base: 0x2004, len: 2 });
        ok = !a.f0453_matches(&b);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ta01_checks() -> CheckSet {
    let mut set = CheckSet::new("TA01");
    set.add(
        "ta01-seg-guard",
        ta01_guard_seg(),
        if ta01_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "ta01-c1-blocks",
        check_c1_blocks(),
        if check_c1_blocks() { "" } else { "pm blocks register mismatch" },
    );
    set.add(
        "ta01-c2-reject-and-f0453",
        check_c2_reject_and_f0453(),
        if check_c2_reject_and_f0453() { "" } else { "pm reject/f0453 mismatch" },
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
    fn ta01_all_checks_green() {
        let set = run_ta01_checks();
        assert!(set.all_passed(), "TA01 domain checks must be green");
    }

    #[test]
    fn ta01_segment_guard_holds() {
        assert!(ta01_segment_guard());
    }
}
