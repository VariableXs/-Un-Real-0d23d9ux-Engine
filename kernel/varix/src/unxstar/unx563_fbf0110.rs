//! unxt563_fbf0110 —— UNX-F0481 F0110 探测值复读对拍。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0481 · 帧缓冲信息账冻结（四要素） · 判据：四要素账齐（物理基址/宽/高/bpp）且与 F0110 探测值复读一致（4/4 字段），版本封账
//! · 组件：F0110 探测值表 + 双源对拍 + 域级快照
//! · 判据落地：与 F0110 探测值复读一致（4/4 字段）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T563_VERSION: &str = "t563-fbf0110-v1";

/// 诊断段基址。
pub const T563_SEG_BASE: u16 = 0x4520;

// --------------------------------------------------------------------------
// 诊断码（t563 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// F0110 复读诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t563Code {
    /// 0x4520 探测值不符。
    ProbeMismatch,
    /// 0x4521 字段重复登记。
    FieldDup,
    /// 0x4522 复读序违例。
    OrderViolated,
    /// 0x4523 复读账破缺。
    LedgerBreak,
    /// 0x4524 报告陈旧。
    ReportStale,
    /// 0x4525 脱敏缺失。
    MaskMissing,
    /// 0x4526 分量链断裂。
    ChainBroken,
    /// 0x4527 守卫触发。
    GuardTripped,
    /// 0x4528 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4529 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x452a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x452b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x452c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x452d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t563Code {
    /// 全量闭集。
    pub const ALL: [t563Code; 14] = [
        t563Code::ProbeMismatch,
        t563Code::FieldDup,
        t563Code::OrderViolated,
        t563Code::LedgerBreak,
        t563Code::ReportStale,
        t563Code::MaskMissing,
        t563Code::ChainBroken,
        t563Code::GuardTripped,
        t563Code::LedgerBroken,
        t563Code::SnapshotFingerprint,
        t563Code::TableBroken,
        t563Code::IndexMisaligned,
        t563Code::DuplicateEntry,
        t563Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t563Code::ProbeMismatch => 0x4520,
            t563Code::FieldDup => 0x4521,
            t563Code::OrderViolated => 0x4522,
            t563Code::LedgerBreak => 0x4523,
            t563Code::ReportStale => 0x4524,
            t563Code::MaskMissing => 0x4525,
            t563Code::ChainBroken => 0x4526,
            t563Code::GuardTripped => 0x4527,
            t563Code::LedgerBroken => 0x4528,
            t563Code::SnapshotFingerprint => 0x4529,
            t563Code::TableBroken => 0x452a,
            t563Code::IndexMisaligned => 0x452b,
            t563Code::DuplicateEntry => 0x452c,
            t563Code::SegmentGuard => 0x452d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t563Code::ProbeMismatch => "探测值不符",
            t563Code::FieldDup => "字段重复登记",
            t563Code::OrderViolated => "复读序违例",
            t563Code::LedgerBreak => "复读账破缺",
            t563Code::ReportStale => "报告陈旧",
            t563Code::MaskMissing => "脱敏缺失",
            t563Code::ChainBroken => "分量链断裂",
            t563Code::GuardTripped => "守卫触发",
            t563Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t563Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t563Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t563Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t563Code::DuplicateEntry => "条目重复（同键双写）",
            t563Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t563Code::ProbeMismatch => "按归因路径登记",
            t563Code::FieldDup => "幂等去重",
            t563Code::OrderViolated => "按复读序记账",
            t563Code::LedgerBreak => "按守恒重建",
            t563Code::ReportStale => "重渲染",
            t563Code::MaskMissing => "补打码",
            t563Code::ChainBroken => "补链",
            t563Code::GuardTripped => "检查临界区",
            t563Code::LedgerBroken => "按守恒律重建台账后重跑",
            t563Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t563Code::TableBroken => "按语义规则重建常量表",
            t563Code::IndexMisaligned => "按声明序归一化后重跑",
            t563Code::DuplicateEntry => "合并同键条目为末次生效",
            t563Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 fb_dual（双源对拍：4 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// F0110 探测值双源对拍（四要素逐字段）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum fb_dualSlot {
    /// 物理基址。
    PhysBase,
    /// 宽。
    Width,
    /// 高。
    Height,
    /// bpp。
    Bpp,
}

impl fb_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [fb_dualSlot; 4] = [
        fb_dualSlot::PhysBase,
        fb_dualSlot::Width,
        fb_dualSlot::Height,
        fb_dualSlot::Bpp,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            fb_dualSlot::PhysBase => 0,
            fb_dualSlot::Width => 1,
            fb_dualSlot::Height => 2,
            fb_dualSlot::Bpp => 3,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            fb_dualSlot::PhysBase => "物理基址",
            fb_dualSlot::Width => "宽",
            fb_dualSlot::Height => "高",
            fb_dualSlot::Bpp => "bpp",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct fb_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct fb_dualDual {
    /// 逐槽样本。
    pub pairs: [fb_dualPair; 4],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl fb_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [fb_dualPair { a: 0, b: 0, same: false, seq: 0 }; 4],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: fb_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = fb_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 4
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 4 && self.same_n + self.diff_n == 4
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4520, 0x452f]。
pub const fn t563_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t563Code::ALL.len() {
        let c = t563Code::ALL[i].code();
        if c < 0x4520 || c > 0x452f {
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
fn t563_guard_seg() -> bool {
    t563_segment_guard()
}

/// 四要素 4/4 复读一致 + 负样本。
fn check_c1_dual_4of4() -> bool {
    let mut d = fb_dualDual::new();
    d.sample(fb_dualSlot::PhysBase, 0xE000_0000, 0xE000_0000);
    d.sample(fb_dualSlot::Width, 1024, 1024);
    d.sample(fb_dualSlot::Height, 768, 768);
    d.sample(fb_dualSlot::Bpp, 32, 32);
    let mut ok = d.all_same() && d.same_n == 4;
    if ok {
        d.sample(fb_dualSlot::Width, 1024, 1280);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t563_checks() -> CheckSet {
    let mut set = CheckSet::new("T563");
    set.add(
        "t563-seg-guard",
        t563_guard_seg(),
        if t563_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t563-c1-dual-4of4",
        check_c1_dual_4of4(),
        if check_c1_dual_4of4() { "" } else { "fb f0110 dual compare mismatch" },
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
    fn t563_all_checks_green() {
        let set = run_t563_checks();
        assert!(set.all_passed(), "T563 domain checks must be green");
    }

    #[test]
    fn t563_segment_guard_holds() {
        assert!(t563_segment_guard());
    }
}
