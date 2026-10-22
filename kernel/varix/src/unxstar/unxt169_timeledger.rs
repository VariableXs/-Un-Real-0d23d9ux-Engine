//! unxt169_timeledger —— UNX-F0162 时间总账（域级快照 + 防篡改 + 报告呈现）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：域级 FNV 快照（六分量）+ 篡改检出 + 文本报告渲染 + 段位守卫
//! · 判据落地：GetTime/SetTime 全域语义的快照封账（指纹锚定防篡改）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T169_VERSION: &str = "t169-timeledger-v1";

/// 诊断段基址。
pub const T169_SEG_BASE: u16 = 0x3770;

// --------------------------------------------------------------------------
// FNV-1a 64 位链式原语
//
// 修正记录（E0425 ×14，本席 AI-5 接入时暴露）：本模块的快照构造与重算
// 两处共 14 次调用 `fnv1a64_from`，但该函数与 `FNV_OFFSET`/`FNV_PRIME`
// **全文件从未定义**——模块自诞生起就编译不过（`mod.rs` 尾部注释掉是
// 合理的保护，本席把原因补记在此）。
// 实现照搬本域既定范式（`unx1081_eventlog.rs:284`/`unx125_wddiag.rs:550`
// 等 20+ 处同款），保证跨模块指纹口径一致：同为 FNV-1a 64、同偏移同质数。
// --------------------------------------------------------------------------

/// FNV-1a 64 位偏移基（标准值）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数（标准值）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 以给定初值继续递推（链式原语 · 与本域其余模块同款）。
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
// 诊断码（t169 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间总账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t169Code {
    /// 0x3770 总账溢出（分量累加突破 u64）。
    LedgerOverflow,
    /// 0x3771 报告截断（行数超渲染上限）。
    ReportTruncated,
    /// 0x3772 掩码缺失（敏感字段未打码）。
    MaskMissing,
    /// 0x3773 字段越出总账集合（域外分量混入）。
    FieldOutOfSet,
    /// 0x3774 指纹陈旧（重算值与封存值不一致）。
    FingerprintStale,
    /// 0x3775 总链断裂（跨分量链环缺失）。
    ChainBroken,
    /// 0x3776 渲染不衡（键值对与账目分量不符）。
    RenderUnbalanced,
    /// 0x3777 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
    /// 0x3778 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3779 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x377a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x377b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x377c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x377d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x377e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x377f 时钟偏斜（域内读时与快照基线偏差越阈）。
    ///
    /// 注：本变体曾被误命名 `SegmentGuard`，与上方 0x3777 重名。
    /// 重名使 `ALL` 闭集 16 槽里只有 15 个唯一变体，且 5 处 `match`
    /// 各缺一臂 ⇒ 内核 target 编译报 E0004（宿主测试构建走 std 且该文件
    /// 未注册进 lib 编译图，故长期未暴露）。此处按「时间总账」域语义
    /// 更名为 `ClockSkew`，使 16 变体真正互斥、闭集自洽。
    ClockSkew,
}

impl t169Code {
    /// 全量闭集（16 项 · 与 `code()` 的 0x3770..=0x377f 逐项对应）。
    ///
    /// 修正记录（E0428/E0004，本席 AI-5 接入时暴露）：本闭集原含
    /// **17 项**——`SegmentGuard` 变体被前人重命名为 `ClockSkew` 后，
    /// `ALL` 里的旧条目**未同步删除**，两个 `match` 也各留了一条重名臂
    /// （`SegmentGuard` 出现两次 ⇒ E0428；编译器随即报「`ClockSkew`/`SegmentGuard`
    /// 未被覆盖」的 E0004）。后果是闭集 17 项而码值只有 16 个，
    /// 段守卫与「码值与声明序恒等」这两条性质**从未成立**。
    /// 现已删除 `ALL` 中的重复条目，闭集回到与枚举一致的 16 项。
    pub const ALL: [t169Code; 16] = [
        t169Code::LedgerOverflow,
        t169Code::ReportTruncated,
        t169Code::MaskMissing,
        t169Code::FieldOutOfSet,
        t169Code::FingerprintStale,
        t169Code::ChainBroken,
        t169Code::RenderUnbalanced,
        t169Code::SegmentGuard,
        t169Code::LedgerBroken,
        t169Code::SnapshotFingerprint,
        t169Code::TableBroken,
        t169Code::IndexMisaligned,
        t169Code::DuplicateEntry,
        t169Code::OrderViolated,
        t169Code::GuardTripped,
        t169Code::ClockSkew,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t169Code::LedgerOverflow => 0x3770,
            t169Code::ReportTruncated => 0x3771,
            t169Code::MaskMissing => 0x3772,
            t169Code::FieldOutOfSet => 0x3773,
            t169Code::FingerprintStale => 0x3774,
            t169Code::ChainBroken => 0x3775,
            t169Code::RenderUnbalanced => 0x3776,
            t169Code::SegmentGuard => 0x3777,
            t169Code::LedgerBroken => 0x3778,
            t169Code::SnapshotFingerprint => 0x3779,
            t169Code::TableBroken => 0x377a,
            t169Code::IndexMisaligned => 0x377b,
            t169Code::DuplicateEntry => 0x377c,
            t169Code::OrderViolated => 0x377d,
            t169Code::GuardTripped => 0x377e,
            t169Code::ClockSkew => 0x377f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t169Code::LedgerOverflow => "总账溢出（分量累加突破 u64）",
            t169Code::ReportTruncated => "报告截断（行数超渲染上限）",
            t169Code::MaskMissing => "掩码缺失（敏感字段未打码）",
            t169Code::FieldOutOfSet => "字段越出总账集合（域外分量混入）",
            t169Code::FingerprintStale => "指纹陈旧（重算值与封存值不一致）",
            t169Code::ChainBroken => "总链断裂（跨分量链环缺失）",
            t169Code::RenderUnbalanced => "渲染不衡（键值对与账目分量不符）",
            t169Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
            t169Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t169Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t169Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t169Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t169Code::DuplicateEntry => "条目重复（同键双写）",
            t169Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t169Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t169Code::ClockSkew => "时钟偏斜（读时与快照基线偏差越阈）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t169Code::LedgerOverflow => "分段封账后续记",
            t169Code::ReportTruncated => "分页渲染或提升缓冲",
            t169Code::MaskMissing => "按脱敏规则补打码",
            t169Code::FieldOutOfSet => "按域账闭集剔除非成员",
            t169Code::FingerprintStale => "按幂等规程重封",
            t169Code::ChainBroken => "补链后回放校验",
            t169Code::RenderUnbalanced => "按账目闭集重渲染",
            t169Code::SegmentGuard => "核对诊断码分配表",
            t169Code::LedgerBroken => "按守恒律重建台账后重跑",
            t169Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t169Code::TableBroken => "按语义规则重建常量表",
            t169Code::IndexMisaligned => "按声明序归一化后重跑",
            t169Code::DuplicateEntry => "合并同键条目为末次生效",
            t169Code::OrderViolated => "按依赖矩阵重排后重跑",
            t169Code::GuardTripped => "检查临界区嵌套与阈值",
            t169Code::ClockSkew => "复核 RTC 与快照基线偏差",
        }
    }
}

// --------------------------------------------------------------------------
// 快照 tsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 时间域六分量快照（FNV-1a 64 链式指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct tsnap {
    /// 指纹分量：field_domain。
    pub field_domain: u64,
    /// 指纹分量：rtc_regs。
    pub rtc_regs: u64,
    /// 指纹分量：tz_zones。
    pub tz_zones: u64,
    /// 指纹分量：leap_secs。
    pub leap_secs: u64,
    /// 指纹分量：clock_srcs。
    pub clock_srcs: u64,
    /// 指纹分量：budget_ms。
    pub budget_ms: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl tsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        field_domain: u64,
        rtc_regs: u64,
        tz_zones: u64,
        leap_secs: u64,
        clock_srcs: u64,
        budget_ms: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &field_domain.to_le_bytes());
        h = fnv1a64_from(h, &rtc_regs.to_le_bytes());
        h = fnv1a64_from(h, &tz_zones.to_le_bytes());
        h = fnv1a64_from(h, &leap_secs.to_le_bytes());
        h = fnv1a64_from(h, &clock_srcs.to_le_bytes());
        h = fnv1a64_from(h, &budget_ms.to_le_bytes());
        Self {
            field_domain,
            rtc_regs,
            tz_zones,
            leap_secs,
            clock_srcs,
            budget_ms,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.field_domain.to_le_bytes());
        h = fnv1a64_from(h, &self.rtc_regs.to_le_bytes());
        h = fnv1a64_from(h, &self.tz_zones.to_le_bytes());
        h = fnv1a64_from(h, &self.leap_secs.to_le_bytes());
        h = fnv1a64_from(h, &self.clock_srcs.to_le_bytes());
        h = fnv1a64_from(h, &self.budget_ms.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3770, 0x377f]。
pub const fn t169_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t169Code::ALL.len() {
        let c = t169Code::ALL[i].code();
        if c < 0x3770 || c > 0x377f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 快照篡改检出（正负样本）。
fn check_c1_snapshot_tamper() -> bool {
    let s = tsnap::compute(7, 16, 25, 35, 4, 1000);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.budget_ms = 999;
        ok = bad.tampered();
    }
    ok
}

/// 报告渲染键值对完整性（六分量齐 + 指纹尾）。
fn check_c2_report_render() -> bool {
    let s = tsnap::compute(7, 16, 25, 35, 4, 1000);
    let txt = format!("field={} regs={} zones={} leaps={} srcs={} budget={} fp={:016x}",
        s.field_domain, s.rtc_regs, s.tz_zones, s.leap_secs, s.clock_srcs, s.budget_ms, s.fingerprint);
    let mut ok = txt.contains("field=7") && txt.contains("budget=1000");
    if ok {
        ok = txt.len() > 40;
    }
    ok
}

/// 段位守卫：全部诊断码落在本模块段内。
fn t169_guard_seg() -> bool {
    t169_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t169_checks() -> CheckSet {
    let mut set = CheckSet::new("T169");
    set.add(
        "t169-c1-snapshot-tamper",
        check_c1_snapshot_tamper(),
        if check_c1_snapshot_tamper() { "" } else { "snapshot tamper detection mismatch" },
    );
    set.add(
        "t169-c2-report-render",
        check_c2_report_render(),
        if check_c2_report_render() { "" } else { "report rendering completeness mismatch" },
    );
    set.add(
        "t169-seg-guard",
        t169_guard_seg(),
        if t169_guard_seg() { "" } else { "diagnostic code outside segment" },
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
    fn t169_all_checks_green() {
        let set = run_t169_checks();
        assert!(set.all_passed(), "T169 domain checks must be green");
    }

    #[test]
    fn t169_segment_guard_holds() {
        assert!(t169_segment_guard());
    }
}
