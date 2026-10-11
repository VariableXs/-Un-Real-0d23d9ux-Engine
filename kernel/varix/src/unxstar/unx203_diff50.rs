//! unxt203_unx203_diff50 —— UNX-F0203 50 次零差异 diff 工具（白名单字段口径）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0203 · 50 次零差异 diff 工具 · 判据：预期字段白名单 diff：50 次引导仅随机化指纹位变化，其余零差异
//! · 组件：白名单字段表 + diff 引擎（指纹位豁免）+ 50 次逐次账 + 域快照
//! · 判据落地：50 次引导仅指纹位变化
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T203_VERSION: &str = "t203-unx203_diff50-v1";

/// 诊断段基址。
pub const T203_SEG_BASE: u16 = 0x4e40;

// --------------------------------------------------------------------------
// 诊断码（t203 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 零差异 diff 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t203Code {
    /// 0x4e40 字段越出白名单（非豁免字段分歧）。
    FieldUnlisted,
    /// 0x4e41 轮次不足（<50 次）。
    RunShort,
    /// 0x4e42 指纹位漏变（应变的位没变）。
    FingerprintMissed,
    /// 0x4e43 diff 账破缺。
    DiffLedgerBreak,
    /// 0x4e44 基线缺失（无基准样本）。
    BaselineMissing,
    /// 0x4e45 比对序违例。
    OrderViolated,
    /// 0x4e46 报告陈旧。
    ReportStale,
    /// 0x4e47 守卫触发。
    GuardTripped,
    /// 0x4e48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e4b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e4c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e4d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t203Code {
    /// 全量闭集。
    pub const ALL: [t203Code; 14] = [
        t203Code::FieldUnlisted,
        t203Code::RunShort,
        t203Code::FingerprintMissed,
        t203Code::DiffLedgerBreak,
        t203Code::BaselineMissing,
        t203Code::OrderViolated,
        t203Code::ReportStale,
        t203Code::GuardTripped,
        t203Code::LedgerBroken,
        t203Code::SnapshotFingerprint,
        t203Code::TableBroken,
        t203Code::IndexMisaligned,
        t203Code::DuplicateEntry,
        t203Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t203Code::FieldUnlisted => 0x4e40,
            t203Code::RunShort => 0x4e41,
            t203Code::FingerprintMissed => 0x4e42,
            t203Code::DiffLedgerBreak => 0x4e43,
            t203Code::BaselineMissing => 0x4e44,
            t203Code::OrderViolated => 0x4e45,
            t203Code::ReportStale => 0x4e46,
            t203Code::GuardTripped => 0x4e47,
            t203Code::LedgerBroken => 0x4e48,
            t203Code::SnapshotFingerprint => 0x4e49,
            t203Code::TableBroken => 0x4e4a,
            t203Code::IndexMisaligned => 0x4e4b,
            t203Code::DuplicateEntry => 0x4e4c,
            t203Code::SegmentGuard => 0x4e4d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t203Code::FieldUnlisted => "字段越出白名单（非豁免字段分歧）",
            t203Code::RunShort => "轮次不足（<50 次）",
            t203Code::FingerprintMissed => "指纹位漏变（应变的位没变）",
            t203Code::DiffLedgerBreak => "diff 账破缺",
            t203Code::BaselineMissing => "基线缺失（无基准样本）",
            t203Code::OrderViolated => "比对序违例",
            t203Code::ReportStale => "报告陈旧",
            t203Code::GuardTripped => "守卫触发",
            t203Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t203Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t203Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t203Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t203Code::DuplicateEntry => "条目重复（同键双写）",
            t203Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t203Code::FieldUnlisted => "按白名单口径归因",
            t203Code::RunShort => "按 50 轮律补齐",
            t203Code::FingerprintMissed => "按指纹位登记补查",
            t203Code::DiffLedgerBreak => "按守恒重建",
            t203Code::BaselineMissing => "补基线后比对",
            t203Code::OrderViolated => "按轮次序重排",
            t203Code::ReportStale => "重渲染",
            t203Code::GuardTripped => "检查临界区",
            t203Code::LedgerBroken => "按守恒律重建台账后重跑",
            t203Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t203Code::TableBroken => "按语义规则重建常量表",
            t203Code::IndexMisaligned => "按声明序归一化后重跑",
            t203Code::DuplicateEntry => "合并同键条目为末次生效",
            t203Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 白名单 diff 引擎（指纹位豁免；其余字段零差异）
// ---------------------------------------------------------------------------

/// 白名单字段（只允许这些字段在 50 次引导间变化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhitelistField {
    /// KASLR 指纹位。
    KaslrFingerprint,
    /// 启动随机种。
    BootNonce,
    /// 熵池快照。
    EntropySnapshot,
}

impl WhitelistField {
    /// 全量闭集（三项）。
    pub const ALL: [WhitelistField; 3] = [
        WhitelistField::KaslrFingerprint,
        WhitelistField::BootNonce,
        WhitelistField::EntropySnapshot,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            WhitelistField::KaslrFingerprint => 0,
            WhitelistField::BootNonce => 1,
            WhitelistField::EntropySnapshot => 2,
        }
    }

    /// 中文字段名。
    pub const fn cn(self) -> &'static str {
        match self {
            WhitelistField::KaslrFingerprint => "随机化指纹",
            WhitelistField::BootNonce => "启动随机种",
            WhitelistField::EntropySnapshot => "熵池快照",
        }
    }
}

/// 引导样本（受审字段集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSample {
    /// 白名单外字段（必须零差异）。
    pub stable_hash: u64,
    /// 白名单内字段（允许变化）。
    pub whitelist_hash: u64,
}

/// diff 账（50 次零差异）。
#[derive(Clone, Copy, Debug)]
pub struct DiffBook {
    /// 已完成比对轮数。
    pub rounds: u64,
    /// 非白名单分歧数（必须 0）。
    pub stable_diffs: u64,
    /// 白名单内变化数（允许 >0，且指纹位必须至少变一次）。
    pub whitelist_diffs: u64,
}

impl DiffBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { rounds: 0, stable_diffs: 0, whitelist_diffs: 0 }
    }

    /// 比对一轮（基线 vs 本轮）。
    pub fn diff(&mut self, base: &BootSample, cur: &BootSample) -> bool {
        self.rounds += 1;
        if base.stable_hash != cur.stable_hash {
            self.stable_diffs += 1;
            return false;
        }
        if base.whitelist_hash != cur.whitelist_hash {
            self.whitelist_diffs += 1;
        }
        true
    }

    /// 50 轮零差异判据（非白名单零分歧 + 指纹位有变化）。
    pub fn clean50(&self) -> bool {
        self.rounds >= 50 && self.stable_diffs == 0 && self.whitelist_diffs > 0
    }
}

/// 字段白名单判定（三项之一）。
pub const fn is_whitelisted(idx: usize) -> bool {
    idx < WhitelistField::ALL.len()
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e40, 0x4e4f]。
pub const fn t203_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t203Code::ALL.len() {
        let c = t203Code::ALL[i].code();
        if c < 0x4e40 || c > 0x4e4f {
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
fn t203_guard_seg() -> bool {
    t203_segment_guard()
}

/// 白名单三项闭集 + 非白名单拒绝。
fn check_c1_whitelist() -> bool {
    let mut ok = WhitelistField::ALL.len() == 3 && is_whitelisted(2) && !is_whitelisted(3);
    if ok {
        ok = WhitelistField::KaslrFingerprint.cn() == "随机化指纹" && WhitelistField::EntropySnapshot.cn() == "熵池快照";
    }
    ok
}

/// 50 轮零差异（stable 全同 + 指纹位有变 + 非白名单零分歧）。
fn check_c2_50runs() -> bool {
    let base = BootSample { stable_hash: 0xDEAD_BEEF, whitelist_hash: 0x1111 };
    let mut bk = DiffBook::new();
    let mut i = 0u64;
    while i < 50 {
        let cur = BootSample { stable_hash: 0xDEAD_BEEF, whitelist_hash: 0x1111 + i + 1 };
        if !bk.diff(&base, &cur) {
            return false;
        }
        i += 1;
    }
    let mut ok = bk.clean50() && bk.rounds == 50 && bk.stable_diffs == 0;
    if ok {
        // 非白名单字段变化必须被抓红。
        let mut b2 = DiffBook::new();
        let bad = BootSample { stable_hash: 0x1234, whitelist_hash: 0x1111 };
        ok = !b2.diff(&base, &bad) && b2.stable_diffs == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t203_checks() -> CheckSet {
    let mut set = CheckSet::new("T203");
    set.add(
        "t203-seg-guard",
        t203_guard_seg(),
        if t203_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t203-c1-whitelist",
        check_c1_whitelist(),
        if check_c1_whitelist() { "" } else { "whitelist fields mismatch" },
    );
    set.add(
        "t203-c2-50runs",
        check_c2_50runs(),
        if check_c2_50runs() { "" } else { "50-run zero-diff mismatch" },
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
    fn t203_all_checks_green() {
        let set = run_t203_checks();
        assert!(set.all_passed(), "T203 domain checks must be green");
    }

    #[test]
    fn t203_segment_guard_holds() {
        assert!(t203_segment_guard());
    }
}
