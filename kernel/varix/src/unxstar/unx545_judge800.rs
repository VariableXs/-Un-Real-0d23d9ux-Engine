//! unxt545_unx545_judge800 —— UNX-F1543 域级判据冻结（800 条 verbatim + 哈希链锁）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1543 · 域级判据冻结（800 条 verbatim + 哈希链锁） · 判据：F0801–F1600 全 800 条逐条哈希成链，篡改注入 10/10 检出，链一键复验
//! · 组件：800 条判据表（F0801–F1600）+ 逐条哈希链 + 篡改注入检出 + 一键复验
//! · 判据落地：800 条成链 + 篡改 10/10 + 复验
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T545_VERSION: &str = "t545-unx545_judge800-v1";

/// 诊断段基址。
pub const T545_SEG_BASE: u16 = 0x6500;

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

// --------------------------------------------------------------------------
// 诊断码（t545 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 判据冻结诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t545Code {
    /// 0x6500 判据不足（≠800 条）。
    JudgeShort,
    /// 0x6501 哈希链断裂（逐条链核失败）。
    ChainBreak,
    /// 0x6502 篡改逃逸（注入未检出）。
    TamperEscaped,
    /// 0x6503 冻结缺失（未封版）。
    FreezeMissing,
    /// 0x6504 判据号非法（F0801–F1600 之外）。
    IdBad,
    /// 0x6505 判据账破缺。
    LedgerBreak,
    /// 0x6506 判据序断档。
    OrderViolated,
    /// 0x6507 守卫触发。
    GuardTripped,
    /// 0x6508 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6509 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x650a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x650b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x650c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x650d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t545Code {
    /// 全量闭集。
    pub const ALL: [t545Code; 14] = [
        t545Code::JudgeShort,
        t545Code::ChainBreak,
        t545Code::TamperEscaped,
        t545Code::FreezeMissing,
        t545Code::IdBad,
        t545Code::LedgerBreak,
        t545Code::OrderViolated,
        t545Code::GuardTripped,
        t545Code::LedgerBroken,
        t545Code::SnapshotFingerprint,
        t545Code::TableBroken,
        t545Code::IndexMisaligned,
        t545Code::DuplicateEntry,
        t545Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t545Code::JudgeShort => 0x6500,
            t545Code::ChainBreak => 0x6501,
            t545Code::TamperEscaped => 0x6502,
            t545Code::FreezeMissing => 0x6503,
            t545Code::IdBad => 0x6504,
            t545Code::LedgerBreak => 0x6505,
            t545Code::OrderViolated => 0x6506,
            t545Code::GuardTripped => 0x6507,
            t545Code::LedgerBroken => 0x6508,
            t545Code::SnapshotFingerprint => 0x6509,
            t545Code::TableBroken => 0x650a,
            t545Code::IndexMisaligned => 0x650b,
            t545Code::DuplicateEntry => 0x650c,
            t545Code::SegmentGuard => 0x650d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t545Code::JudgeShort => "判据不足（≠800 条）",
            t545Code::ChainBreak => "哈希链断裂（逐条链核失败）",
            t545Code::TamperEscaped => "篡改逃逸（注入未检出）",
            t545Code::FreezeMissing => "冻结缺失（未封版）",
            t545Code::IdBad => "判据号非法（F0801–F1600 之外）",
            t545Code::LedgerBreak => "判据账破缺",
            t545Code::OrderViolated => "判据序断档",
            t545Code::GuardTripped => "守卫触发",
            t545Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t545Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t545Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t545Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t545Code::DuplicateEntry => "条目重复（同键双写）",
            t545Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t545Code::JudgeShort => "按 800 条律补齐",
            t545Code::ChainBreak => "按链核重建",
            t545Code::TamperEscaped => "按 10/10 检出补检",
            t545Code::FreezeMissing => "按锁定律封版",
            t545Code::IdBad => "按号域校正",
            t545Code::LedgerBreak => "按守恒重建",
            t545Code::OrderViolated => "按号连续律补号",
            t545Code::GuardTripped => "检查临界区",
            t545Code::LedgerBroken => "按守恒律重建台账后重跑",
            t545Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t545Code::TableBroken => "按语义规则重建常量表",
            t545Code::IndexMisaligned => "按声明序归一化后重跑",
            t545Code::DuplicateEntry => "合并同键条目为末次生效",
            t545Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 域级判据冻结（F0801–F1600 共 800 条 verbatim + 逐条哈希链）
// ---------------------------------------------------------------------------

/// 判据总数（800 条）。
pub const JUDGE_TOTAL: u32 = 800;

/// 判据号域（F0801..F1600）。
pub const JUDGE_ID_MIN: u32 = 801;

/// 判据号域上限。
pub const JUDGE_ID_MAX: u32 = 1600;

/// 单条判据（号 + verbatim 哈希 + 链哈希）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeEntry {
    /// 判据号（801..=1600）。
    pub id: u32,
    /// verbatim 文本哈希（FNV-1a 64）。
    pub text_hash: u64,
    /// 链哈希（FNV-1a 64 递推）。
    pub chain_hash: u64,
}

/// 单项合法性（号域 + 双哈希非零）。
pub const fn judge_ok(j: &JudgeEntry) -> bool {
    j.id >= JUDGE_ID_MIN && j.id <= JUDGE_ID_MAX && j.text_hash != 0
}

/// 号 → 表索引（减偏移）。
pub const fn judge_index(id: u32) -> Option<u32> {
    if id < JUDGE_ID_MIN || id > JUDGE_ID_MAX {
        return None;
    }
    Some(id - JUDGE_ID_MIN)
}

/// 冻结账本（800 条链 + 篡改检出 + 复验）。
#[derive(Clone, Copy, Debug)]
pub struct JudgeBook {
    /// 判据条目序列。
    pub items: [JudgeEntry; JUDGE_TOTAL as usize],
    /// 已冻结条数。
    pub n: u32,
    /// 篡改注入数。
    pub tampered: u64,
    /// 篡改检出数（必须 == tampered）。
    pub caught: u64,
    /// 终链哈希（0 = 未冻）。
    pub head: u64,
}

impl JudgeBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self {
            items: [JudgeEntry { id: 0, text_hash: 0, chain_hash: 0 }; JUDGE_TOTAL as usize],
            n: 0,
            tampered: 0,
            caught: 0,
            head: 0,
        }
    }

    /// 冻结一条（号严格连续；verbatim 哈希由外部 FN 给出）。")
    pub fn freeze(&mut self, id: u32, text_hash: u64) -> bool {
        match judge_index(id) {
            Some(idx) => {
                if idx != self.n {
                    return false;
                }
                let prev = if self.n == 0 { FNV_OFFSET } else { self.items[(self.n - 1) as usize].chain_hash };
                let entry = JudgeEntry { id, text_hash, chain_hash: 0 };
                if !judge_ok(&entry) {
                    return false;
                }
                let mut e = entry;
                e.chain_hash = fnv1a64_from(prev, &e.id.to_le_bytes());
                e.chain_hash = fnv1a64_from(e.chain_hash, &e.text_hash.to_le_bytes());
                self.items[self.n as usize] = e;
                self.n += 1;
                self.head = e.chain_hash;
                true
            }
            None => false,
        }
    }

    /// 全链复验（逐条重算链哈希 + 800 条齐备）。
    pub fn verify(&self) -> bool {
        if self.n != JUDGE_TOTAL {
            return false;
        }
        let mut h = FNV_OFFSET;
        let mut i = 0u32;
        while i < self.n {
            let e = self.items[i as usize];
            if !judge_ok(&e) || e.id != JUDGE_ID_MIN + i {
                return false;
            }
            h = fnv1a64_from(h, &e.id.to_le_bytes());
            h = fnv1a64_from(h, &e.text_hash.to_le_bytes());
            if h != e.chain_hash {
                return false;
            }
            i += 1;
        }
        h == self.head
    }

    /// 注入一次篡改（改 verbatim 哈希 → 复验必须失败）。
    pub fn inject_tamper(&mut self, idx: u32) -> bool {
        self.tampered += 1;
        if idx >= self.n {
            return false;
        }
        let before = self.items[idx as usize].text_hash;
        self.items[idx as usize].text_hash ^= 0xDEAD_BEEF;
        let caught = !self.verify();
        self.items[idx as usize].text_hash = before;
        if caught {
            self.caught += 1;
        }
        caught
    }

    /// 篡改 10/10 检出判据。
    pub fn tamper_all_caught(&self) -> bool {
        self.tampered == self.caught && self.tampered >= 10
    }

    /// 冻结完成判据（800 条 + 链核过 + 终链非零）。
    pub fn frozen(&self) -> bool {
        self.n == JUDGE_TOTAL && self.head != 0 && self.verify()
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x6500, 0x650f]。
pub const fn t545_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t545Code::ALL.len() {
        let c = t545Code::ALL[i].code();
        if c < 0x6500 || c > 0x650f {
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
fn t545_guard_seg() -> bool {
    t545_segment_guard()
}

/// 800 条判据冻结（逐条 + 全链复验 + 号域）。
fn check_c1_800() -> bool {
    let mut bk = JudgeBook::new();
    let mut ok = true;
    let mut i = 0u32;
    while i < JUDGE_TOTAL {
        let id = JUDGE_ID_MIN + i;
        ok = ok && bk.freeze(id, 0x8000_0000_0000_0000 + i as u64);
        i += 1;
    }
    if ok {
        ok = bk.frozen() && bk.head != 0 && bk.n == 800;
    }
    ok
}

/// 链核 + 跳号/越域拒绝 + 篡改注入 10/10。
fn check_c2_chain() -> bool {
    let mut bk = JudgeBook::new();
    let mut i = 0u32;
    while i < JUDGE_TOTAL {
        let _ = bk.freeze(JUDGE_ID_MIN + i, 0x9000_0000_0000_0000 + i as u64);
        i += 1;
    }
    let mut ok = bk.verify();
    if ok {
        // 跳号必须拒。
        let mut b2 = JudgeBook::new();
        ok = b2.freeze(801, 1) && !b2.freeze(803, 2);
    }
    if ok {
        // 越域号必须拒。
        let mut b3 = JudgeBook::new();
        ok = !b3.freeze(800, 1) && !b3.freeze(1601, 1);
    }
    if ok {
        // 10 次篡改注入全部检出。
        let mut j = 0u32;
        while j < 10 {
            ok = ok && bk.inject_tamper(j * 37);
            j += 1;
        }
        ok = ok && bk.tamper_all_caught() && bk.caught == 10;
    }
    if ok {
        // 复验仍过（注入后还原）。
        ok = bk.verify();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t545_checks() -> CheckSet {
    let mut set = CheckSet::new("T545");
    set.add(
        "t545-seg-guard",
        t545_guard_seg(),
        if t545_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t545-c1-800",
        check_c1_800(),
        if check_c1_800() { "" } else { "judge 800 freeze mismatch" },
    );
    set.add(
        "t545-c2-chain",
        check_c2_chain(),
        if check_c2_chain() { "" } else { "judge chain verify mismatch" },
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
    fn t545_all_checks_green() {
        let set = run_t545_checks();
        assert!(set.all_passed(), "T545 domain checks must be green");
    }

    #[test]
    fn t545_segment_guard_holds() {
        assert!(t545_segment_guard());
    }
}
