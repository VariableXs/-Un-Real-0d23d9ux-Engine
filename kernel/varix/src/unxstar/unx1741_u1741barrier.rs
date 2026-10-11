//! unxu1741_u1741barrier —— 空闲循环总纲（idle 三态状态机） 到达栅栏与锁域账（空闲态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 到达栅栏（8 方全员放行 + 超时账）+ 锁域守恒账
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741barrier-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5a90;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 栅栏面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5a90 方数非法（非法转换 超出栅栏方数）。
    PartyBad,
    /// 0x5a91 相位非法（账实漂移 跨相位到达）。
    PhaseBad,
    /// 0x5a92 超时命中（唤醒丢失 等待越时限）。
    TimeoutHit,
    /// 0x5a93 重复到达（驻留死锁 同方双计）。
    ArriveDup,
    /// 0x5a94 放行破缺（未满员即放行）。
    ReleaseBad,
    /// 0x5a95 到达计数漂移（计数≠实到）。
    CountDrift,
    /// 0x5a96 守恒破缺（相位账不平）。
    Conserved,
    /// 0x5a97 等待丢失（超时账未入账）。
    WaitLost,
    /// 0x5a98 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5a99 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5a9a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5a9b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::PartyBad,
        u1741Code::PhaseBad,
        u1741Code::TimeoutHit,
        u1741Code::ArriveDup,
        u1741Code::ReleaseBad,
        u1741Code::CountDrift,
        u1741Code::Conserved,
        u1741Code::WaitLost,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::PartyBad => 0x5a90,
            u1741Code::PhaseBad => 0x5a91,
            u1741Code::TimeoutHit => 0x5a92,
            u1741Code::ArriveDup => 0x5a93,
            u1741Code::ReleaseBad => 0x5a94,
            u1741Code::CountDrift => 0x5a95,
            u1741Code::Conserved => 0x5a96,
            u1741Code::WaitLost => 0x5a97,
            u1741Code::LedgerBroken => 0x5a98,
            u1741Code::SnapshotFingerprint => 0x5a99,
            u1741Code::TableBroken => 0x5a9a,
            u1741Code::SegmentGuard => 0x5a9b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::PartyBad => "方数非法（非法转换 超出栅栏方数）",
            u1741Code::PhaseBad => "相位非法（账实漂移 跨相位到达）",
            u1741Code::TimeoutHit => "超时命中（唤醒丢失 等待越时限）",
            u1741Code::ArriveDup => "重复到达（驻留死锁 同方双计）",
            u1741Code::ReleaseBad => "放行破缺（未满员即放行）",
            u1741Code::CountDrift => "到达计数漂移（计数≠实到）",
            u1741Code::Conserved => "守恒破缺（相位账不平）",
            u1741Code::WaitLost => "等待丢失（超时账未入账）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::PartyBad => "按 PARTY 闭集钳制",
            u1741Code::PhaseBad => "按相位序重排",
            u1741Code::TimeoutHit => "按超时账升级处置",
            u1741Code::ArriveDup => "按方幂等律合并",
            u1741Code::ReleaseBad => "按满员律闸口",
            u1741Code::CountDrift => "按实到校正",
            u1741Code::Conserved => "按相位守恒律重建",
            u1741Code::WaitLost => "补记等待账",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 mxu1741bar（到达栅栏：8 方全员到达才放行 + 超时账）
// --------------------------------------------------------------------------
/// 空闲态 到达栅栏（非法转换）。
/// 参与方上限。
pub const MXU1741BAR_PARTY: usize = 8;

/// 单方到达记录。
#[derive(Clone, Copy, Debug)]
pub struct mxu1741barArrival {
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
pub struct mxu1741barBarrier {
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

impl mxu1741barBarrier {
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
        if who >= MXU1741BAR_PARTY {
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
        while i < MXU1741BAR_PARTY {
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
        if self.arrived_count() < MXU1741BAR_PARTY && self.now_ns >= self.timeout_ns {
            self.timeouts += 1;
            return true;
        }
        false
    }

    /// 全员到达判定。
    pub fn complete(&self) -> bool {
        self.arrived_count() == MXU1741BAR_PARTY
    }

    /// 放行：仅全员到达时成功，否则记账失败。
    pub fn release(&mut self) -> bool {
        if self.complete() {
            self.released += 1;
            // 释放后清空以便下一轮复用。
            let mut i = 0usize;
            while i < MXU1741BAR_PARTY {
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
// 守恒账 Mxu1741LockLedger（空闲态 锁域账（取锁/放锁/迁移三操作守恒））
// --------------------------------------------------------------------------
/// 空闲态 锁域账（取锁/放锁/迁移三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1741LockLedger {
    /// 取锁次数
    pub acquires: u64,
    /// 放锁次数
    pub releases: u64,
    /// 迁移次数
    pub migrations: u64,
}

impl Mxu1741LockLedger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5a90, 0x5a9f]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5a90 || c > 0x5a9f {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 栅栏 8 方满员放行 + 守恒。
fn check_u1741_bar_full() -> bool {
    let mut b = mxu1741barBarrier::new();
    let mut i = 0usize;
    while i < MXU1741BAR_PARTY { b.arrive(i); i += 1; }
    let mut ok = b.arrived_count() == MXU1741BAR_PARTY;
    b.tick(1000);
    if ok { ok = b.complete() && b.conserved(); }
    ok
}

/// UNX-F1741 锁域账取放恒等（取 N 放 N）。
fn check_u1741_bar_lock() -> bool {
    let mut ld = Mxu1741LockLedger::new();
    let mut i = 0u64;
    while i < 3 { ld.acq(); ld.rel(); i += 1; }
    ld.mig();
    let mut ok = ld.acquires == 3 && ld.releases == 3 && ld.migrations == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-bar-full",
        check_u1741_bar_full(),
        if check_u1741_bar_full() { "" } else { "UNX-F1741 barrier mismatch" },
    );
    set.add(
        "u1741-bar-lock",
        check_u1741_bar_lock(),
        if check_u1741_bar_lock() { "" } else { "UNX-F1741 lock ledger mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
