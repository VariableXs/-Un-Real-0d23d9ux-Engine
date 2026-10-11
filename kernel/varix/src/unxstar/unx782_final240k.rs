//! unxt782_unx782_final240k —— UNX-F0782 域账终点宣告（240,000 终值锁定）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0782 · 域账终点宣告（240,000 终值锁定账） · 判据：终点宣告过（域账累计链 40 段终点 = 240,000 精确等 + 逐段链核 40/40）且终值哈希锁定
//! · 组件：40 段累计链（每段 6000）+ 逐段链核 + 终值 FNV 锁定 + 域快照
//! · 判据落地：40 段终点 = 240,000 精确等 + 40/40 链核
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T782_VERSION: &str = "t782-unx782_final240k-v1";

/// 诊断段基址。
pub const T782_SEG_BASE: u16 = 0x5060;

// --------------------------------------------------------------------------
// 诊断码（t782 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域账终点诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t782Code {
    /// 0x5060 段数不足（<40 段）。
    SegmentShort,
    /// 0x5061 终值漂移（≠240,000）。
    TotalDrift,
    /// 0x5062 段链断裂（逐段链核失败）。
    ChainBreak,
    /// 0x5063 终值未锁（哈希未封）。
    FinalUnsealed,
    /// 0x5064 段值非法（≠6000）。
    SegValueBad,
    /// 0x5065 域账账破缺。
    LedgerBreak,
    /// 0x5066 段序断档。
    OrderViolated,
    /// 0x5067 守卫触发。
    GuardTripped,
    /// 0x5068 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5069 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x506a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x506b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x506c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x506d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t782Code {
    /// 全量闭集。
    pub const ALL: [t782Code; 14] = [
        t782Code::SegmentShort,
        t782Code::TotalDrift,
        t782Code::ChainBreak,
        t782Code::FinalUnsealed,
        t782Code::SegValueBad,
        t782Code::LedgerBreak,
        t782Code::OrderViolated,
        t782Code::GuardTripped,
        t782Code::LedgerBroken,
        t782Code::SnapshotFingerprint,
        t782Code::TableBroken,
        t782Code::IndexMisaligned,
        t782Code::DuplicateEntry,
        t782Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t782Code::SegmentShort => 0x5060,
            t782Code::TotalDrift => 0x5061,
            t782Code::ChainBreak => 0x5062,
            t782Code::FinalUnsealed => 0x5063,
            t782Code::SegValueBad => 0x5064,
            t782Code::LedgerBreak => 0x5065,
            t782Code::OrderViolated => 0x5066,
            t782Code::GuardTripped => 0x5067,
            t782Code::LedgerBroken => 0x5068,
            t782Code::SnapshotFingerprint => 0x5069,
            t782Code::TableBroken => 0x506a,
            t782Code::IndexMisaligned => 0x506b,
            t782Code::DuplicateEntry => 0x506c,
            t782Code::SegmentGuard => 0x506d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t782Code::SegmentShort => "段数不足（<40 段）",
            t782Code::TotalDrift => "终值漂移（≠240,000）",
            t782Code::ChainBreak => "段链断裂（逐段链核失败）",
            t782Code::FinalUnsealed => "终值未锁（哈希未封）",
            t782Code::SegValueBad => "段值非法（≠6000）",
            t782Code::LedgerBreak => "域账账破缺",
            t782Code::OrderViolated => "段序断档",
            t782Code::GuardTripped => "守卫触发",
            t782Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t782Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t782Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t782Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t782Code::DuplicateEntry => "条目重复（同键双写）",
            t782Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t782Code::SegmentShort => "按 40 段律补齐",
            t782Code::TotalDrift => "按精确等口径归因",
            t782Code::ChainBreak => "按链核 40/40 重建",
            t782Code::FinalUnsealed => "按锁定律封版",
            t782Code::SegValueBad => "按段值表校正",
            t782Code::LedgerBreak => "按守恒重建",
            t782Code::OrderViolated => "按 40 段连续律补号",
            t782Code::GuardTripped => "检查临界区",
            t782Code::LedgerBroken => "按守恒律重建台账后重跑",
            t782Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t782Code::TableBroken => "按语义规则重建常量表",
            t782Code::IndexMisaligned => "按声明序归一化后重跑",
            t782Code::DuplicateEntry => "合并同键条目为末次生效",
            t782Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

/// 以给定初值继续递推（链式哈希原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

// ---------------------------------------------------------------------------
// 域账累计链（40 段 × 6000 = 240,000 终值锁定）
// ---------------------------------------------------------------------------

/// 段数（40 段）。
pub const FINAL_SEGMENTS: u32 = 40;

/// 每段值（6000）。
pub const FINAL_SEG_VALUE: u32 = 6000;

/// 终值目标（240,000 精确等）。
pub const FINAL_TARGET: u64 = 240_000;

/// 累计链节点（段号 + 段值 + 链哈希）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalSegNode {
    /// 段号（1..=40）。
    pub seg: u32,
    /// 段值。
    pub value: u32,
    /// 链哈希（FNV-1a 64 递推）。
    pub chain_hash: u64,
}

/// 段合法性（段号域 + 段值恒 6000）。
pub const fn seg_node_ok(n: &FinalSegNode) -> bool {
    n.seg >= 1 && n.seg <= FINAL_SEGMENTS && n.value == FINAL_SEG_VALUE
}

/// 累计函数（前累计 + 段值）。
pub const fn final_accumulate(prev: u64, seg: &FinalSegNode) -> u64 {
    prev + seg.value as u64
}

/// 链哈希递推（FNV-1a 64 以段号为负载）。
pub const fn final_chain_hash(prev: u64, seg: &FinalSegNode) -> u64 {
    fnv1a64_from(prev, &seg.seg.to_le_bytes())
}

/// 域账（40 段链 + 终值 + 锁定）。
#[derive(Clone, Copy, Debug)]
pub struct FinalBook {
    /// 链节点序列。
    pub chain: [FinalSegNode; FINAL_SEGMENTS as usize],
    /// 已记账段数。
    pub n: u32,
    /// 累计值。
    pub total: u64,
    /// 终值哈希（0 = 未锁）。
    pub final_hash: u64,
}

impl FinalBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self {
            chain: [FinalSegNode { seg: 0, value: 0, chain_hash: 0 }; FINAL_SEGMENTS as usize],
            n: 0,
            total: 0,
            final_hash: 0,
        }
    }

    /// 记账一段（段号必须严格递增）。
    pub fn append(&mut self, seg: u32) -> bool {
        if seg < 1 || seg > FINAL_SEGMENTS || seg != self.n + 1 {
            return false;
        }
        let node = FinalSegNode { seg, value: FINAL_SEG_VALUE, chain_hash: 0 };
        if !seg_node_ok(&node) {
            return false;
        }
        let prev_hash = if self.n == 0 { FNV_OFFSET } else { self.chain[(self.n - 1) as usize].chain_hash };
        let mut node = node;
        node.chain_hash = final_chain_hash(prev_hash, &node);
        self.chain[self.n as usize] = node;
        self.n += 1;
        self.total = final_accumulate(self.total, &node);
        true
    }

    /// 链核（逐段重算哈希 + 累计值）。
    pub fn chain_ok(&self) -> bool {
        if self.n != FINAL_SEGMENTS {
            return false;
        }
        let mut h = FNV_OFFSET;
        let mut total = 0u64;
        let mut i = 0u32;
        while i < self.n {
            let node = self.chain[i as usize];
            if !seg_node_ok(&node) {
                return false;
            }
            h = final_chain_hash(h, &node);
            if h != node.chain_hash {
                return false;
            }
            total = final_accumulate(total, &node);
            i += 1;
        }
        total == self.total && total == FINAL_TARGET
    }

    /// 终值锁定（FNV 封存）。
    pub fn seal(&mut self) -> bool {
        if self.total != FINAL_TARGET {
            return false;
        }
        self.final_hash = fnv1a64_from(FNV_OFFSET, &self.total.to_le_bytes());
        self.final_hash != 0
    }

    /// 锁定校验（重算 == 封存）。
    pub fn sealed_ok(&self) -> bool {
        self.final_hash != 0
            && self.final_hash == fnv1a64_from(FNV_OFFSET, &self.total.to_le_bytes())
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5060, 0x506f]。
pub const fn t782_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t782Code::ALL.len() {
        let c = t782Code::ALL[i].code();
        if c < 0x5060 || c > 0x506f {
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
fn t782_guard_seg() -> bool {
    t782_segment_guard()
}

/// 40 段记账 + 终值 240,000 精确等 + 链核 40/40。
fn check_c1_final() -> bool {
    let mut bk = FinalBook::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= 40 {
        ok = ok && bk.append(i);
        i += 1;
    }
    if ok {
        ok = bk.total == 240000 && bk.n == 40 && bk.chain_ok();
    }
    ok
}

/// 终值锁定（封存 + 校验 + 断段/越段拒绝）。
fn check_c2_seal() -> bool {
    let mut bk = FinalBook::new();
    let mut i = 1u32;
    while i <= 40 {
        let _ = bk.append(i);
        i += 1;
    }
    let mut ok = bk.seal() && bk.sealed_ok();
    if ok {
        // 第 41 段必须拒。
        ok = !bk.append(41);
    }
    if ok {
        // 跳段必须拒。
        let mut b2 = FinalBook::new();
        ok = b2.append(1) && !b2.append(3);
    }
    if ok {
        // 39 段未满不许封。
        let mut b3 = FinalBook::new();
        let mut j = 1u32;
        while j <= 39 {
            let _ = b3.append(j);
            j += 1;
        }
        ok = !b3.seal() && b3.total == 234000;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t782_checks() -> CheckSet {
    let mut set = CheckSet::new("T782");
    set.add(
        "t782-seg-guard",
        t782_guard_seg(),
        if t782_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t782-c1-final",
        check_c1_final(),
        if check_c1_final() { "" } else { "final 240k chain mismatch" },
    );
    set.add(
        "t782-c2-seal",
        check_c2_seal(),
        if check_c2_seal() { "" } else { "final seal mismatch" },
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
    fn t782_all_checks_green() {
        let set = run_t782_checks();
        assert!(set.all_passed(), "T782 domain checks must be green");
    }

    #[test]
    fn t782_segment_guard_holds() {
        assert!(t782_segment_guard());
    }
}
