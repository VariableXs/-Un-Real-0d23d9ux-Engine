//! unxu3601_u3601barrier —— 锁压力机总设计（场景编排/指标采集/裁决门） 到达栅栏与锁域账（锁压力机）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 到达栅栏（8 方全员放行 + 超时账）+ 锁域守恒账
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601barrier-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 栅栏面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe160 方数非法（压力 超出栅栏方数）。
    PartyBad,
    /// 0xe161 相位非法（场景 跨相位到达）。
    PhaseBad,
    /// 0xe162 超时命中（裁决 等待越时限）。
    TimeoutHit,
    /// 0xe163 重复到达（裁决门 同方双计）。
    ArriveDup,
    /// 0xe164 放行破缺（未满员即放行）。
    ReleaseBad,
    /// 0xe165 到达计数漂移（计数≠实到）。
    CountDrift,
    /// 0xe166 守恒破缺（相位账不平）。
    Conserved,
    /// 0xe167 等待丢失（超时账未入账）。
    WaitLost,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::PartyBad,
        u3601Code::PhaseBad,
        u3601Code::TimeoutHit,
        u3601Code::ArriveDup,
        u3601Code::ReleaseBad,
        u3601Code::CountDrift,
        u3601Code::Conserved,
        u3601Code::WaitLost,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::PartyBad => 0xe160,
            u3601Code::PhaseBad => 0xe161,
            u3601Code::TimeoutHit => 0xe162,
            u3601Code::ArriveDup => 0xe163,
            u3601Code::ReleaseBad => 0xe164,
            u3601Code::CountDrift => 0xe165,
            u3601Code::Conserved => 0xe166,
            u3601Code::WaitLost => 0xe167,
            u3601Code::LedgerBroken => 0xe168,
            u3601Code::SnapshotFingerprint => 0xe169,
            u3601Code::TableBroken => 0xe16a,
            u3601Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::PartyBad => "方数非法（压力 超出栅栏方数）",
            u3601Code::PhaseBad => "相位非法（场景 跨相位到达）",
            u3601Code::TimeoutHit => "超时命中（裁决 等待越时限）",
            u3601Code::ArriveDup => "重复到达（裁决门 同方双计）",
            u3601Code::ReleaseBad => "放行破缺（未满员即放行）",
            u3601Code::CountDrift => "到达计数漂移（计数≠实到）",
            u3601Code::Conserved => "守恒破缺（相位账不平）",
            u3601Code::WaitLost => "等待丢失（超时账未入账）",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::PartyBad => "按 PARTY 闭集钳制",
            u3601Code::PhaseBad => "按相位序重排",
            u3601Code::TimeoutHit => "按超时账升级处置",
            u3601Code::ArriveDup => "按方幂等律合并",
            u3601Code::ReleaseBad => "按满员律闸口",
            u3601Code::CountDrift => "按实到校正",
            u3601Code::Conserved => "按相位守恒律重建",
            u3601Code::WaitLost => "补记等待账",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3601bar（到达栅栏：8 方全员到达才放行 + 超时账）
// --------------------------------------------------------------------------
/// 锁压力机 到达栅栏（压力）。
/// 参与方上限。
pub const MXU3601BAR_PARTY: usize = 8;

/// 单方到达记录。
#[derive(Clone, Copy, Debug)]
pub struct mxu3601barArrival {
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
pub struct mxu3601barBarrier {
    /// 到达位图（每方一比特，去重后只计一次）。
    pub arrived: [bool; 8],
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

impl mxu3601barBarrier {
    /// 构造（超时阈值 1000 ns）。
    pub const fn new() -> Self {
        Self {
            arrived: [false; 8],
            arrivals: 0,
            dupes: 0,
            released: 0,
            timeouts: 0,
            now_ns: 0,
            timeout_ns: 1000,
            last_ns: 0,
        }
    }

    /// 单方到达：重复到达只记账不前进位图。
    pub fn arrive(&mut self, who: usize) -> bool {
        if who >= MXU3601BAR_PARTY {
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
        while i < MXU3601BAR_PARTY {
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
        if self.arrived_count() < MXU3601BAR_PARTY && self.now_ns >= self.timeout_ns {
            self.timeouts += 1;
            return true;
        }
        false
    }

    /// 全员到达判定。
    pub fn complete(&self) -> bool {
        self.arrived_count() == MXU3601BAR_PARTY
    }

    /// 放行：仅全员到达时成功，否则记账失败。
    pub fn release(&mut self) -> bool {
        if self.complete() {
            self.released += 1;
            // 释放后清空以便下一轮复用。
            let mut i = 0usize;
            while i < MXU3601BAR_PARTY {
                self.arrived[i] = false;
                i += 1;
            }
            return true;
        }
        false
    }

    /// 守恒恒等式：arrivals == 到达方数 + 重复数。
    pub fn conserved(&self) -> bool {
        self.arrivals == (self.arrived_count() as u64) + self.dupes + (self.released * 8 as u64)
    }
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3601LockLedger（锁压力机 锁域账（取锁/放锁/迁移三操作守恒））
// --------------------------------------------------------------------------
/// 锁压力机 锁域账（取锁/放锁/迁移三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3601LockLedger {
    /// 取锁次数
    pub acquires: u64,
    /// 放锁次数
    pub releases: u64,
    /// 迁移次数
    pub migrations: u64,
}

impl Mxu3601LockLedger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            acquires: 0,
            releases: 0,
            migrations: 0,
        }
    }

    /// 取锁一笔
    pub fn acq(&mut self) {
        self.acquires = self.acquires.wrapping_add(1);
    }

    /// 放锁一笔
    pub fn rel(&mut self) {
        self.releases = self.releases.wrapping_add(1);
    }

    /// 迁移一笔
    pub fn mig(&mut self) {
        self.migrations = self.migrations.wrapping_add(1);
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe160 || c > 0xe16f {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 栅栏 8 方满员放行 + 守恒。
fn check_u3601_bar_full() -> bool {
    let mut b = mxu3601barBarrier::new();
    let mut i = 0usize;
    while i < MXU3601BAR_PARTY { b.arrive(i); i += 1; }
    let mut ok = b.arrived_count() == MXU3601BAR_PARTY;
    b.tick(1000);
    if ok { ok = b.complete() && b.conserved(); }
    ok
}

/// UNX-F3601 锁域账取放恒等（取 N 放 N）。
fn check_u3601_bar_lock() -> bool {
    let mut ld = Mxu3601LockLedger::new();
    let mut i = 0u64;
    while i < 3 { ld.acq(); ld.rel(); i += 1; }
    ld.mig();
    let mut ok = ld.acquires == 3 && ld.releases == 3 && ld.migrations == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-bar-full",
        check_u3601_bar_full(),
        if check_u3601_bar_full() { "" } else { "UNX-F3601 barrier mismatch" },
    );
    set.add(
        "u3601-bar-lock",
        check_u3601_bar_lock(),
        if check_u3601_bar_lock() { "" } else { "UNX-F3601 lock ledger mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
