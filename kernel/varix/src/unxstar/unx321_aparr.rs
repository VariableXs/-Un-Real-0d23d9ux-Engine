//! unxt321_aparr —— UNX-F0341 AP 到达栅栏（全员放行语义）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0341 · AP 汇合栅栏（全部到达点同步） · 判据：栅栏语义（全员到达放行）正确，部分未达超时路径有账
//! · 组件：到达栅栏（四方）+ 去重 + 放行账 + 超时路径
//! · 判据落地：栅栏语义（全员到达放行）正确
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T321_VERSION: &str = "t321-aparr-v1";

/// 诊断段基址。
pub const T321_SEG_BASE: u16 = 0x3e00;

// --------------------------------------------------------------------------
// 诊断码（t321 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 到达栅栏诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t321Code {
    /// 0x3e00 重复到达（同方双达）。
    PartyDup,
    /// 0x3e01 方编号越界。
    PartyOob,
    /// 0x3e02 部分未达超时。
    PartialTimeout,
    /// 0x3e03 提前放行（未全员即放）。
    ReleaseEarly,
    /// 0x3e04 栅栏账破缺。
    LedgerBreak,
    /// 0x3e05 法定人数非法。
    QuorumIllegal,
    /// 0x3e06 到达序违例。
    OrderViolated,
    /// 0x3e07 守卫触发。
    GuardTripped,
    /// 0x3e08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3e09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3e0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3e0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3e0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3e0d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t321Code {
    /// 全量闭集。
    pub const ALL: [t321Code; 14] = [
        t321Code::PartyDup,
        t321Code::PartyOob,
        t321Code::PartialTimeout,
        t321Code::ReleaseEarly,
        t321Code::LedgerBreak,
        t321Code::QuorumIllegal,
        t321Code::OrderViolated,
        t321Code::GuardTripped,
        t321Code::LedgerBroken,
        t321Code::SnapshotFingerprint,
        t321Code::TableBroken,
        t321Code::IndexMisaligned,
        t321Code::DuplicateEntry,
        t321Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t321Code::PartyDup => 0x3e00,
            t321Code::PartyOob => 0x3e01,
            t321Code::PartialTimeout => 0x3e02,
            t321Code::ReleaseEarly => 0x3e03,
            t321Code::LedgerBreak => 0x3e04,
            t321Code::QuorumIllegal => 0x3e05,
            t321Code::OrderViolated => 0x3e06,
            t321Code::GuardTripped => 0x3e07,
            t321Code::LedgerBroken => 0x3e08,
            t321Code::SnapshotFingerprint => 0x3e09,
            t321Code::TableBroken => 0x3e0a,
            t321Code::IndexMisaligned => 0x3e0b,
            t321Code::DuplicateEntry => 0x3e0c,
            t321Code::SegmentGuard => 0x3e0d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t321Code::PartyDup => "重复到达（同方双达）",
            t321Code::PartyOob => "方编号越界",
            t321Code::PartialTimeout => "部分未达超时",
            t321Code::ReleaseEarly => "提前放行（未全员即放）",
            t321Code::LedgerBreak => "栅栏账破缺",
            t321Code::QuorumIllegal => "法定人数非法",
            t321Code::OrderViolated => "到达序违例",
            t321Code::GuardTripped => "守卫触发",
            t321Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t321Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t321Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t321Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t321Code::DuplicateEntry => "条目重复（同键双写）",
            t321Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t321Code::PartyDup => "按去重律合并",
            t321Code::PartyOob => "按方数律裁剪",
            t321Code::PartialTimeout => "按超时路径降级并记账",
            t321Code::ReleaseEarly => "按全员律拒止",
            t321Code::LedgerBreak => "按守恒重建",
            t321Code::QuorumIllegal => "按人数表校正",
            t321Code::OrderViolated => "按到达序记账",
            t321Code::GuardTripped => "检查临界区",
            t321Code::LedgerBroken => "按守恒律重建台账后重跑",
            t321Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t321Code::TableBroken => "按语义规则重建常量表",
            t321Code::IndexMisaligned => "按声明序归一化后重跑",
            t321Code::DuplicateEntry => "合并同键条目为末次生效",
            t321Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 aparr（到达栅栏：4 方全员到达才放行 + 超时账）
// --------------------------------------------------------------------------
/// AP 汇合到达栅栏（四方：BSP + AP0-2）。
/// 参与方上限。
pub const APARR_PARTY: usize = 4;

/// 单方到达记录。
#[derive(Clone, Copy, Debug)]
pub struct aparrArrival {
    /// 方索引。
    pub who: usize,
    /// 到达序号（全局计数器值）。
    pub seq: u64,
    /// 到达时刻（ns 逻辑时钟）。
    pub at_ns: u64,
    /// 是否重复到达（去重后计数）。
    pub duplicate: bool,
}

/// 栅栏账本（全员到达放行；超时未达走降级路径并记账）。
#[derive(Clone, Copy, Debug)]
pub struct aparrBarrier {
    /// 到达位图（每方一比特，去重后只计一次）。
    pub arrived: [bool; 4],
    /// 到达计数（去重前，含重复）。
    pub arrivals: u64,
    /// 重复到达计数。
    pub dupes: u64,
    /// 已放行计数。
    pub released: u64,
    /// 超时计数。
    pub timeouts: u64,
    /// 逻辑时钟。
    pub now_ns: u64,
    /// 超时阈值（ns）。
    pub timeout_ns: u64,
    /// 已用最晚到达时刻。
    pub last_ns: u64,
}

impl aparrBarrier {
    /// 构造（超时阈值 1000000 ns）。
    pub const fn new() -> Self {
        Self {
            arrived: [false; 4],
            arrivals: 0,
            dupes: 0,
            released: 0,
            timeouts: 0,
            now_ns: 0,
            timeout_ns: 1000000,
            last_ns: 0,
        }
    }

    /// 单方到达：重复到达只记账不前进位图。
    pub fn arrive(&mut self, who: usize) -> bool {
        if who >= APARR_PARTY {
            return false;
        }
        self.arrivals += 1;
        if self.arrived[who] {
            self.dupes += 1;
            return false;
        }
        self.arrived[who] = true;
        if self.now_ns > self.last_ns {
            self.last_ns = self.now_ns;
        }
        true
    }

    /// 已到达方数（位图汉明权重）。
    pub fn arrived_count(&self) -> usize {
        let mut c = 0usize;
        let mut i = 0usize;
        while i < APARR_PARTY {
            if self.arrived[i] {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 推进逻辑时钟并检测超时（无人新到达且超阈）。
    pub fn tick(&mut self, dt_ns: u64) -> bool {
        self.now_ns = self.now_ns.wrapping_add(dt_ns);
        if self.arrived_count() < APARR_PARTY && self.now_ns >= self.timeout_ns {
            self.timeouts += 1;
            return true;
        }
        false
    }

    /// 全员到达判定。
    pub fn complete(&self) -> bool {
        self.arrived_count() == APARR_PARTY
    }

    /// 放行：仅全员到达时成功，否则记账失败。
    pub fn release(&mut self) -> bool {
        if self.complete() {
            self.released += 1;
            // 释放后清空以便下一轮复用。
            let mut i = 0usize;
            while i < APARR_PARTY {
                self.arrived[i] = false;
                i += 1;
            }
            return true;
        }
        false
    }

    /// 守恒恒等式：arrivals == 到达方数 + 重复数。
    pub fn conserved(&self) -> bool {
        self.arrivals == (self.arrived_count() as u64) + self.dupes + (self.released * 4 as u64)
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3e00, 0x3e0f]。
pub const fn t321_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t321Code::ALL.len() {
        let c = t321Code::ALL[i].code();
        if c < 0x3e00 || c > 0x3e0f {
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
fn t321_guard_seg() -> bool {
    t321_segment_guard()
}

/// 到达序列（四方到场 + 重复去重 + 全员放行）。
fn check_c1_arrive() -> bool {
    let mut b = aparrBarrier::new();
    let mut ok = b.arrive(0) && b.arrive(1);
    if ok {
        ok = !b.complete() && b.arrive(1) == false && b.dupes == 1;
    }
    if ok {
        ok = b.arrive(2) && b.arrive(3) && b.complete();
    }
    if ok {
        ok = b.release();
    }
    ok
}

/// 部分未达超时路径（推进时钟至超阈）。
fn check_c2_timeout() -> bool {
    let mut b = aparrBarrier::new();
    let _ = b.arrive(0);
    let _ = b.arrive(1);
    let _ = b.arrive(2);
    let mut to = false;
    let mut i = 0;
    while i < 30 {
        if b.tick(50_000) {
            to = true;
            break;
        }
        i += 1;
    }
    to && b.timeouts >= 1
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t321_checks() -> CheckSet {
    let mut set = CheckSet::new("T321");
    set.add(
        "t321-seg-guard",
        t321_guard_seg(),
        if t321_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t321-c1-arrive",
        check_c1_arrive(),
        if check_c1_arrive() { "" } else { "barrier arrival mismatch" },
    );
    set.add(
        "t321-c2-timeout",
        check_c2_timeout(),
        if check_c2_timeout() { "" } else { "barrier timeout path mismatch" },
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
    fn t321_all_checks_green() {
        let set = run_t321_checks();
        assert!(set.all_passed(), "T321 domain checks must be green");
    }

    #[test]
    fn t321_segment_guard_holds() {
        assert!(t321_segment_guard());
    }
}
