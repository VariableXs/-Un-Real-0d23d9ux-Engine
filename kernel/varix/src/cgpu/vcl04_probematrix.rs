//! unxl04a_vcl04_probematrix —— CGPU-F1764 guest 探测矩阵（CPUID/驱动查询双源）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F1764 · 能力探测与虚拟化感知（CGPU-L 域 · 批次 L01 · L04 组 · 判据：探测矩阵/诚实/协商协议/缓存/保守/五组）
//! · 组件：探测矩阵表（探测项 × 双源）+ 探测状态机 + 诚实复用标记
//! · 判据落地：探测矩阵/诚实
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const L04A_VERSION: &str = "l04a-vcl04_probematrix-v1";

/// 诊断段基址。
pub const L04A_SEG_BASE: u16 = 0x7100;

// --------------------------------------------------------------------------
// 诊断码（l04a 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// guest 探测诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum l04aCode {
    /// 0x7100 探测项非法（矩阵外）。
    ProbeBad,
    /// 0x7101 探测源非法（双源之外）。
    SourceBad,
    /// 0x7102 虚标检出（探测值超物理上限）。
    CpvOverclaim,
    /// 0x7103 双源分歧未裁决。
    ConflictUnresolved,
    /// 0x7104 探测缓存陈旧。
    CacheStale,
    /// 0x7105 保守模式缺失（失败未降级）。
    ConservativeMissing,
    /// 0x7106 探测账破缺。
    LedgerBreak,
    /// 0x7107 守卫触发。
    GuardTripped,
    /// 0x7108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x710a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x710b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x710c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x710d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x710e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl l04aCode {
    /// 全量闭集。
    pub const ALL: [l04aCode; 15] = [
        l04aCode::ProbeBad,
        l04aCode::SourceBad,
        l04aCode::CpvOverclaim,
        l04aCode::ConflictUnresolved,
        l04aCode::CacheStale,
        l04aCode::ConservativeMissing,
        l04aCode::LedgerBreak,
        l04aCode::GuardTripped,
        l04aCode::LedgerBroken,
        l04aCode::SnapshotFingerprint,
        l04aCode::TableBroken,
        l04aCode::IndexMisaligned,
        l04aCode::DuplicateEntry,
        l04aCode::OrderViolated,
        l04aCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            l04aCode::ProbeBad => 0x7100,
            l04aCode::SourceBad => 0x7101,
            l04aCode::CpvOverclaim => 0x7102,
            l04aCode::ConflictUnresolved => 0x7103,
            l04aCode::CacheStale => 0x7104,
            l04aCode::ConservativeMissing => 0x7105,
            l04aCode::LedgerBreak => 0x7106,
            l04aCode::GuardTripped => 0x7107,
            l04aCode::LedgerBroken => 0x7108,
            l04aCode::SnapshotFingerprint => 0x7109,
            l04aCode::TableBroken => 0x710a,
            l04aCode::IndexMisaligned => 0x710b,
            l04aCode::DuplicateEntry => 0x710c,
            l04aCode::OrderViolated => 0x710d,
            l04aCode::SegmentGuard => 0x710e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            l04aCode::ProbeBad => "探测项非法（矩阵外）",
            l04aCode::SourceBad => "探测源非法（双源之外）",
            l04aCode::CpvOverclaim => "虚标检出（探测值超物理上限）",
            l04aCode::ConflictUnresolved => "双源分歧未裁决",
            l04aCode::CacheStale => "探测缓存陈旧",
            l04aCode::ConservativeMissing => "保守模式缺失（失败未降级）",
            l04aCode::LedgerBreak => "探测账破缺",
            l04aCode::GuardTripped => "守卫触发",
            l04aCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            l04aCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            l04aCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            l04aCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            l04aCode::DuplicateEntry => "条目重复（同键双写）",
            l04aCode::OrderViolated => "次序违例（依赖序/时序破缺）",
            l04aCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            l04aCode::ProbeBad => "按探测矩阵校正",
            l04aCode::SourceBad => "按双源闭集校正",
            l04aCode::CpvOverclaim => "按诚实复用户口钳制",
            l04aCode::ConflictUnresolved => "按裁决规程择一",
            l04aCode::CacheStale => "按缓存失效规程重探",
            l04aCode::ConservativeMissing => "按保守路径补降级",
            l04aCode::LedgerBreak => "按守恒重建",
            l04aCode::GuardTripped => "检查临界区",
            l04aCode::LedgerBroken => "按守恒律重建台账后重跑",
            l04aCode::SnapshotFingerprint => "按防篡改规程重建快照",
            l04aCode::TableBroken => "按语义规则重建常量表",
            l04aCode::IndexMisaligned => "按声明序归一化后重跑",
            l04aCode::DuplicateEntry => "合并同键条目为末次生效",
            l04aCode::OrderViolated => "按依赖矩阵重排后重跑",
            l04aCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// guest 探测矩阵（双源：CPUID 超虚指令 / 驱动查询）+ 诚实复用
// ---------------------------------------------------------------------------

/// 探测源（双源闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeSource {
    /// CPUID/超虚指令源（虚拟化感知）。
    Cpuid,
    /// 驱动查询源（各厂商 API/寄存器）。
    Driver,
}

/// 探测项（八项闭集：厂商/虚拟化/显存/编解码/计算/显示/版本/驱动线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeItem {
    /// GPU 厂商。
    Vendor,
    /// 虚拟化标志与模式。
    VirtMode,
    /// 显存容量。
    VramBytes,
    /// 编解码能力位图。
    EncodeMask,
    /// 计算能力位图。
    ComputeMask,
    /// 显示输出数。
    DisplayCount,
    /// 驱动版本行。
    DriverLine,
    /// 架构代次。
    ArchGen,
}

impl ProbeItem {
    /// 全量闭集（八项）。
    pub const ALL: [ProbeItem; 8] = [
        ProbeItem::Vendor,
        ProbeItem::VirtMode,
        ProbeItem::VramBytes,
        ProbeItem::EncodeMask,
        ProbeItem::ComputeMask,
        ProbeItem::DisplayCount,
        ProbeItem::DriverLine,
        ProbeItem::ArchGen,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            ProbeItem::Vendor => 0,
            ProbeItem::VirtMode => 1,
            ProbeItem::VramBytes => 2,
            ProbeItem::EncodeMask => 3,
            ProbeItem::ComputeMask => 4,
            ProbeItem::DisplayCount => 5,
            ProbeItem::DriverLine => 6,
            ProbeItem::ArchGen => 7,
        }
    }

    /// 中文项名。
    pub const fn cn(self) -> &'static str {
        match self {
            ProbeItem::Vendor => "厂商",
            ProbeItem::VirtMode => "虚拟化",
            ProbeItem::VramBytes => "显存容量",
            ProbeItem::EncodeMask => "编解码",
            ProbeItem::ComputeMask => "计算",
            ProbeItem::DisplayCount => "显示输出",
            ProbeItem::DriverLine => "驱动版本",
            ProbeItem::ArchGen => "架构代次",
        }
    }
}

/// 单源探测结果（值 + 是否估算/诚实标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeValue {
    /// 探测值（语义化标量）。
    pub value: u64,
    /// 来源可信（false = 该源不可信，需另一源裁决）。
    pub trusted: bool,
}

/// 探测矩阵单元（双源 + 裁决值 + 诚实标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeCell {
    /// CPUID 源值（None = 该源未提供）。
    pub cpuid: Option<ProbeValue>,
    /// 驱动源值（None = 该源未提供）。
    pub driver: Option<ProbeValue>,
    /// 裁决后采纳值（None = 未裁决）。
    pub adopted: Option<u64>,
    /// 是否保守降级（失败路径标记）。
    pub conservative: bool,
}

impl ProbeCell {
    /// 空单元构造。
    pub const fn new() -> Self {
        Self { cpuid: None, driver: None, adopted: None, conservative: false }
    }

    /// 双源分歧判定（两源都有且值不同）。
    pub fn conflicting(&self) -> bool {
        match (self.cpuid, self.driver) {
            (Some(a), Some(b)) => a.value != b.value,
            _ => false,
        }
    }

    /// 裁决（保守口径：取可信源；都可信取小；都不可信降保守）。
    pub fn arbitrate(&mut self) -> bool {
        let adopted = match (self.cpuid, self.driver) {
            (Some(a), Some(b)) => {
                if !a.trusted && !b.trusted {
                    self.conservative = true;
                    0
                } else if !a.trusted {
                    b.value
                } else if !b.trusted {
                    a.value
                } else if a.value < b.value {
                    a.value
                } else {
                    b.value
                }
            }
            (Some(a), None) => a.value,
            (None, Some(b)) => b.value,
            (None, None) => return false,
        };
        self.adopted = Some(adopted);
        true
    }

    /// 诚实复用户口（虚标钳制：裁决值不得超过物理上限）。
    pub fn honest_clamp(&mut self, physical_max: u64) -> bool {
        if let Some(v) = self.adopted {
            if v > physical_max {
                self.adopted = Some(physical_max);
                return true;
            }
        }
        false
    }
}

/// 探测矩阵（8 项账）。
#[derive(Clone, Copy, Debug)]
pub struct ProbeMatrix {
    /// 逐项单元。
    pub cells: [ProbeCell; 8],
}

impl ProbeMatrix {
    /// 全空矩阵构造。
    pub const fn new() -> Self {
        Self { cells: [ProbeCell::new(); 8] }
    }

    /// 登记一源值。
    pub fn feed(&mut self, item: ProbeItem, src: ProbeSource, v: ProbeValue) -> bool {
        if item.index() >= 8 {
            return false;
        }
        let cell = &mut self.cells[item.index()];
        match src {
            ProbeSource::Cpuid => cell.cpuid = Some(v),
            ProbeSource::Driver => cell.driver = Some(v),
        }
        true
    }

    /// 全矩阵裁决 + 诚实钳制（返回成功项数）。
    pub fn settle(&mut self, physical_max: u64) -> usize {
        let mut ok = 0;
        let mut i = 0;
        while i < 8 {
            if self.cells[i].arbitrate() {
                self.cells[i].honest_clamp(physical_max);
                ok += 1;
            }
            i += 1;
        }
        ok
    }

    /// 未裁决项数（应全为 0 才放行）。
    pub fn pending_n(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 8 {
            if self.cells[i].adopted.is_none() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7100, 0x710f]。
pub const fn l04a_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < l04aCode::ALL.len() {
        let c = l04aCode::ALL[i].code();
        if c < 0x7100 || c > 0x710f {
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
fn l04a_guard_seg() -> bool {
    l04a_segment_guard()
}

/// 探测矩阵八项全裁决 + 双源一致零分歧。
fn check_c1_probe_settle() -> bool {
    let mut mx = ProbeMatrix::new();
    let mut ok = true;
    let mut i = 0;
    while i < 8 {
        ok = ok && mx.feed(ProbeItem::ALL[i], ProbeSource::Cpuid, ProbeValue { value: i as u64 * 100, trusted: true });
        i += 1;
    }
    let mut j = 0;
    while j < 8 {
        ok = ok && mx.feed(ProbeItem::ALL[j], ProbeSource::Driver, ProbeValue { value: j as u64 * 100, trusted: true });
        j += 1;
    }
    if ok {
        ok = mx.settle(1_000_000) == 8 && mx.pending_n() == 0;
    }
    if ok {
        ok = mx.cells[7].adopted == Some(700);
    }
    ok
}

/// 诚实复用（虚标钳制 + 分歧裁决 + 保守降级）。
fn check_c2_honest_clamp() -> bool {
    let mut mx = ProbeMatrix::new();
    mx.feed(ProbeItem::VramBytes, ProbeSource::Cpuid, ProbeValue { value: 64 * 1024 * 1024 * 1024, trusted: true });
    mx.feed(ProbeItem::VramBytes, ProbeSource::Driver, ProbeValue { value: 64 * 1024 * 1024 * 1024, trusted: true });
    let mut ok = mx.settle(16 * 1024 * 1024 * 1024) >= 1;
    if ok {
        // 64GB 被钳制到物理上限 16GB。
        ok = mx.cells[ProbeItem::VramBytes.index()].adopted == Some(16 * 1024 * 1024 * 1024);
    }
    if ok {
        // 双源分歧：取可信小值；都不可信走保守。
        let mut c = ProbeMatrix::new();
        c.feed(ProbeItem::EncodeMask, ProbeSource::Cpuid, ProbeValue { value: 0x0F, trusted: true });
        c.feed(ProbeItem::EncodeMask, ProbeSource::Driver, ProbeValue { value: 0x03, trusted: false });
        c.settle(0xFFFF);
        ok = c.cells[ProbeItem::EncodeMask.index()].adopted == Some(0x0F);
    }
    if ok {
        let mut d = ProbeMatrix::new();
        d.feed(ProbeItem::ComputeMask, ProbeSource::Cpuid, ProbeValue { value: 0xFF, trusted: false });
        d.feed(ProbeItem::ComputeMask, ProbeSource::Driver, ProbeValue { value: 0xF0, trusted: false });
        d.settle(0xFFFF);
        ok = d.cells[ProbeItem::ComputeMask.index()].conservative;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_l04a_checks() -> CheckSet {
    let mut set = CheckSet::new("L04A");
    set.add(
        "l04a-seg-guard",
        l04a_guard_seg(),
        if l04a_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "l04a-c1-probe-settle",
        check_c1_probe_settle(),
        if check_c1_probe_settle() { "" } else { "probe matrix settle mismatch" },
    );
    set.add(
        "l04a-c2-honest-clamp",
        check_c2_honest_clamp(),
        if check_c2_honest_clamp() { "" } else { "honest clamp mismatch" },
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
    fn l04a_all_checks_green() {
        let set = run_l04a_checks();
        assert!(set.all_passed(), "L04A domain checks must be green");
    }

    #[test]
    fn l04a_segment_guard_holds() {
        assert!(l04a_segment_guard());
    }
}
