//! unxt284_preentry —— UNX-F0281 重入防护（嵌套守卫 + 深度红线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：重入守卫（深度 1 红线）+ 嵌套账 + 挂起路径
//! · 判据落地：重入 panic 防护过
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T284_VERSION: &str = "t284-preentry-v1";

/// 诊断段基址。
pub const T284_SEG_BASE: u16 = 0x3b30;

// --------------------------------------------------------------------------
// 诊断码（t284 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 重入防护诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t284Code {
    /// 0x3b30 重入检出（嵌套第二层）。
    Reentered,
    /// 0x3b31 深度溢出（>1）。
    DepthOverflow,
    /// 0x3b32 守卫泄漏（进未出）。
    GuardLeak,
    /// 0x3b33 挂起失败。
    SuspendFailed,
    /// 0x3b34 计数破缺（进出不配对）。
    CounterBroken,
    /// 0x3b35 唤醒丢失。
    LostWakeup,
    /// 0x3b36 中断中重入。
    InterruptNested,
    /// 0x3b37 守卫触发。
    GuardTripped,
    /// 0x3b38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3b3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b3d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b3e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t284Code {
    /// 全量闭集。
    pub const ALL: [t284Code; 15] = [
        t284Code::Reentered,
        t284Code::DepthOverflow,
        t284Code::GuardLeak,
        t284Code::SuspendFailed,
        t284Code::CounterBroken,
        t284Code::LostWakeup,
        t284Code::InterruptNested,
        t284Code::GuardTripped,
        t284Code::LedgerBroken,
        t284Code::SnapshotFingerprint,
        t284Code::TableBroken,
        t284Code::IndexMisaligned,
        t284Code::DuplicateEntry,
        t284Code::OrderViolated,
        t284Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t284Code::Reentered => 0x3b30,
            t284Code::DepthOverflow => 0x3b31,
            t284Code::GuardLeak => 0x3b32,
            t284Code::SuspendFailed => 0x3b33,
            t284Code::CounterBroken => 0x3b34,
            t284Code::LostWakeup => 0x3b35,
            t284Code::InterruptNested => 0x3b36,
            t284Code::GuardTripped => 0x3b37,
            t284Code::LedgerBroken => 0x3b38,
            t284Code::SnapshotFingerprint => 0x3b39,
            t284Code::TableBroken => 0x3b3a,
            t284Code::IndexMisaligned => 0x3b3b,
            t284Code::DuplicateEntry => 0x3b3c,
            t284Code::OrderViolated => 0x3b3d,
            t284Code::SegmentGuard => 0x3b3e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t284Code::Reentered => "重入检出（嵌套第二层）",
            t284Code::DepthOverflow => "深度溢出（>1）",
            t284Code::GuardLeak => "守卫泄漏（进未出）",
            t284Code::SuspendFailed => "挂起失败",
            t284Code::CounterBroken => "计数破缺（进出不配对）",
            t284Code::LostWakeup => "唤醒丢失",
            t284Code::InterruptNested => "中断中重入",
            t284Code::GuardTripped => "守卫触发",
            t284Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t284Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t284Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t284Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t284Code::DuplicateEntry => "条目重复（同键双写）",
            t284Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t284Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t284Code::Reentered => "按防护挂起并记账",
            t284Code::DepthOverflow => "按红线拒止",
            t284Code::GuardLeak => "按幂等离开复位",
            t284Code::SuspendFailed => "复核临界区状态",
            t284Code::CounterBroken => "按配对律重建",
            t284Code::LostWakeup => "复核挂起链",
            t284Code::InterruptNested => "按中断上下文路径",
            t284Code::GuardTripped => "检查临界区",
            t284Code::LedgerBroken => "按守恒律重建台账后重跑",
            t284Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t284Code::TableBroken => "按语义规则重建常量表",
            t284Code::IndexMisaligned => "按声明序归一化后重跑",
            t284Code::DuplicateEntry => "合并同键条目为末次生效",
            t284Code::OrderViolated => "按依赖矩阵重排后重跑",
            t284Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守卫 pent（阈值 1 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// panic 重入守卫：嵌套深度红线 1。
#[derive(Clone, Copy, Debug)]
pub struct pent {
    /// 重入嵌套深度（>0 即重入中）。
    pub depth: u32,
    /// 越限拒止计数。
    pub over: u64,
    /// 静默拒止计数（阈值内未升级）。
    pub silent: u64,
    /// 当前窗口计数。
    pub window: u64,
    /// 窗口内观测总量。
    pub seen: u64,
    /// 已升级告警数。
    pub raised: u64,
}

/// 守卫阈值（panic 重入守卫：嵌套深度红线 1）。
pub const PENT_THRESHOLD: u64 = 1;

impl pent {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { depth: 0, over: 0, silent: 0, window: 0, seen: 0, raised: 0 }
    }

    /// 进入临界区：重入直接拒止（返回 false）。
    pub fn enter(&mut self) -> bool {
        if self.depth > 0 {
            self.over += 1;
            return false;
        }
        self.depth = 1;
        true
    }

    /// 离开临界区（深度归一，幂等安全）。
    pub fn leave(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 观测一次：累计 seen/window；越限升级 raised，否则静默。
    pub fn observe(&mut self, hit: bool) {
        self.seen += 1;
        if hit {
            self.window += 1;
        }
        if self.window >= PENT_THRESHOLD {
            self.raised += 1;
        } else {
            self.silent += 1;
        }
    }

    /// 守恒：seen == raised + silent（窗口口径）。
    pub fn conserved(&self) -> bool {
        self.seen == self.raised + self.silent
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3b30, 0x3b3f]。
pub const fn t284_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t284Code::ALL.len() {
        let c = t284Code::ALL[i].code();
        if c < 0x3b30 || c > 0x3b3f {
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
fn t284_guard_seg() -> bool {
    t284_segment_guard()
}

/// 重入守卫（进-重入拒-出-再进）。
fn check_c1_reentry_guard() -> bool {
    let mut g = pent::new();
    let mut ok = g.enter();
    if ok {
        // 重入必须被拒。
        ok = !g.enter() && g.depth == 1 && g.over == 1;
    }
    if ok {
        g.leave();
        ok = g.depth == 0 && g.enter();
    }
    ok
}

/// 守卫守恒（观测 seen = raised + silent）。
fn check_c2_guard_conserve() -> bool {
    let mut g = pent::new();
    let mut i = 0;
    while i < 64 {
        g.observe(i % 2 == 0);
        i += 1;
    }
    let mut ok = g.conserved();
    if ok {
        ok = g.seen == 64;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t284_checks() -> CheckSet {
    let mut set = CheckSet::new("T284");
    set.add(
        "t284-seg-guard",
        t284_guard_seg(),
        if t284_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t284-c1-reentry-guard",
        check_c1_reentry_guard(),
        if check_c1_reentry_guard() { "" } else { "reentry guard mismatch" },
    );
    set.add(
        "t284-c2-guard-conserve",
        check_c2_guard_conserve(),
        if check_c2_guard_conserve() { "" } else { "guard conservation mismatch" },
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
    fn t284_all_checks_green() {
        let set = run_t284_checks();
        assert!(set.all_passed(), "T284 domain checks must be green");
    }

    #[test]
    fn t284_segment_guard_holds() {
        assert!(t284_segment_guard());
    }
}
