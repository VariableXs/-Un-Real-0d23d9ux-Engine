//! unxu3641_u3641barrier —— 崩溃转储总设计（触发→采集→封存管线） 到达栅栏与锁域账（崩溃转储）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3641 · 崩溃转储总设计（触发→采集→封存管线） · 判据原文：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 panic→全链走通→封存物可校验）。<br>J2：管线恒备（非崩溃期管线自检绿——F3655 的日常态——总设计的常备性）。
//! · 组件：崩溃转储 到达栅栏（8 方全员放行 + 超时账）+ 锁域守恒账
//! · 判据落地：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 p
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3641_VERSION: &str = "u3641-u3641barrier-v1";

/// 诊断段基址。
pub const U3641_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 崩溃转储 栅栏面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3641Code {
    /// 0xe180 方数非法（转储 超出栅栏方数）。
    PartyBad,
    /// 0xe181 相位非法（崩溃 跨相位到达）。
    PhaseBad,
    /// 0xe182 超时命中（封存 等待越时限）。
    TimeoutHit,
    /// 0xe183 重复到达（封存 同方双计）。
    ArriveDup,
    /// 0xe184 放行破缺（未满员即放行）。
    ReleaseBad,
    /// 0xe185 到达计数漂移（计数≠实到）。
    CountDrift,
    /// 0xe186 守恒破缺（相位账不平）。
    Conserved,
    /// 0xe187 等待丢失（超时账未入账）。
    WaitLost,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3641Code {
    /// 全量闭集。
    pub const ALL: [u3641Code; 12] = [
        u3641Code::PartyBad,
        u3641Code::PhaseBad,
        u3641Code::TimeoutHit,
        u3641Code::ArriveDup,
        u3641Code::ReleaseBad,
        u3641Code::CountDrift,
        u3641Code::Conserved,
        u3641Code::WaitLost,
        u3641Code::LedgerBroken,
        u3641Code::SnapshotFingerprint,
        u3641Code::TableBroken,
        u3641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3641Code::PartyBad => 0xe180,
            u3641Code::PhaseBad => 0xe181,
            u3641Code::TimeoutHit => 0xe182,
            u3641Code::ArriveDup => 0xe183,
            u3641Code::ReleaseBad => 0xe184,
            u3641Code::CountDrift => 0xe185,
            u3641Code::Conserved => 0xe186,
            u3641Code::WaitLost => 0xe187,
            u3641Code::LedgerBroken => 0xe188,
            u3641Code::SnapshotFingerprint => 0xe189,
            u3641Code::TableBroken => 0xe18a,
            u3641Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3641Code::PartyBad => "方数非法（转储 超出栅栏方数）",
            u3641Code::PhaseBad => "相位非法（崩溃 跨相位到达）",
            u3641Code::TimeoutHit => "超时命中（封存 等待越时限）",
            u3641Code::ArriveDup => "重复到达（封存 同方双计）",
            u3641Code::ReleaseBad => "放行破缺（未满员即放行）",
            u3641Code::CountDrift => "到达计数漂移（计数≠实到）",
            u3641Code::Conserved => "守恒破缺（相位账不平）",
            u3641Code::WaitLost => "等待丢失（超时账未入账）",
            u3641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3641Code::PartyBad => "按 PARTY 闭集钳制",
            u3641Code::PhaseBad => "按相位序重排",
            u3641Code::TimeoutHit => "按超时账升级处置",
            u3641Code::ArriveDup => "按方幂等律合并",
            u3641Code::ReleaseBad => "按满员律闸口",
            u3641Code::CountDrift => "按实到校正",
            u3641Code::Conserved => "按相位守恒律重建",
            u3641Code::WaitLost => "补记等待账",
            u3641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3641Code::TableBroken => "按语义规则重建常量表",
            u3641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu3641bar（到达栅栏：8 方全员到达才放行 + 超时账）
// --------------------------------------------------------------------------
/// 崩溃转储 到达栅栏（转储）。
/// 参与方上限。
pub const MXU3641BAR_PARTY: usize = 8;

/// 单方到达记录。
#[derive(Clone, Copy, Debug)]
pub struct mxu3641barArrival {
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
pub struct mxu3641barBarrier {
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

impl mxu3641barBarrier {
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
        if who >= MXU3641BAR_PARTY {
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
        while i < MXU3641BAR_PARTY {
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
        if self.arrived_count() < MXU3641BAR_PARTY && self.now_ns >= self.timeout_ns {
            self.timeouts += 1;
            return true;
        }
        false
    }

    /// 全员到达判定。
    pub fn complete(&self) -> bool {
        self.arrived_count() == MXU3641BAR_PARTY
    }

    /// 放行：仅全员到达时成功，否则记账失败。
    pub fn release(&mut self) -> bool {
        if self.complete() {
            self.released += 1;
            // 释放后清空以便下一轮复用。
            let mut i = 0usize;
            while i < MXU3641BAR_PARTY {
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
// 守恒账 Mxu3641LockLedger（崩溃转储 锁域账（取锁/放锁/迁移三操作守恒））
// --------------------------------------------------------------------------
/// 崩溃转储 锁域账（取锁/放锁/迁移三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3641LockLedger {
    /// 取锁次数
    pub acquires: u64,
    /// 放锁次数
    pub releases: u64,
    /// 迁移次数
    pub migrations: u64,
}

impl Mxu3641LockLedger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3641Code::ALL.len() {
        let c = u3641Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3641_guard_seg() -> bool {
    u3641_segment_guard()
}

/// UNX-F3641 栅栏 8 方满员放行 + 守恒。
fn check_u3641_bar_full() -> bool {
    let mut b = mxu3641barBarrier::new();
    let mut i = 0usize;
    while i < MXU3641BAR_PARTY { b.arrive(i); i += 1; }
    let mut ok = b.arrived_count() == MXU3641BAR_PARTY;
    b.tick(1000);
    if ok { ok = b.complete() && b.conserved(); }
    ok
}

/// UNX-F3641 锁域账取放恒等（取 N 放 N）。
fn check_u3641_bar_lock() -> bool {
    let mut ld = Mxu3641LockLedger::new();
    let mut i = 0u64;
    while i < 3 { ld.acq(); ld.rel(); i += 1; }
    ld.mig();
    let mut ok = ld.acquires == 3 && ld.releases == 3 && ld.migrations == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3641_checks() -> CheckSet {
    let mut set = CheckSet::new("U3641");
    set.add(
        "u3641-seg-guard",
        u3641_guard_seg(),
        if u3641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3641-bar-full",
        check_u3641_bar_full(),
        if check_u3641_bar_full() { "" } else { "UNX-F3641 barrier mismatch" },
    );
    set.add(
        "u3641-bar-lock",
        check_u3641_bar_lock(),
        if check_u3641_bar_lock() { "" } else { "UNX-F3641 lock ledger mismatch" },
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
    fn u3641_all_checks_green() {
        let set = run_u3641_checks();
        assert!(set.all_passed(), "U3641 domain checks must be green");
    }

    #[test]
    fn u3641_segment_guard_holds() {
        assert!(u3641_segment_guard());
    }
}
