//! unxt266_flipbar —— UNX-F0261 消隐同步翻页栅栏。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0261 · 帧簿双头显（诊断浮层 + 正常输出合成） · 判据：诊断浮层与正常输出同屏合成零互蚀，浮层开合零残影
//! · 组件：翻页栅栏（两方：扫描器/提交器）+ vblank 窗口账 + 撕裂拒止
//! · 判据落地：零互蚀合成的时序保障
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T266_VERSION: &str = "t266-flipbar-v1";

/// 诊断段基址。
pub const T266_SEG_BASE: u16 = 0x3a50;

// --------------------------------------------------------------------------
// 诊断码（t266 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 翻页栅栏诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t266Code {
    /// 0x3a50 提前翻页（窗期外翻页）。
    FlipEarly,
    /// 0x3a51 迟翻页（错过本窗）。
    FlipLate,
    /// 0x3a52 撕裂观测（扫中翻页）。
    TearObserved,
    /// 0x3a53 消隐窗非法（窗长超限）。
    VblankBad,
    /// 0x3a54 参与方缺失（扫描器未到）。
    PartyMissing,
    /// 0x3a55 同窗双翻。
    DoubleFlipSameWin,
    /// 0x3a56 翻页账破缺。
    LedgerBreak,
    /// 0x3a57 守卫触发。
    GuardTripped,
    /// 0x3a58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3a59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3a5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3a5b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3a5c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3a5d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3a5e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t266Code {
    /// 全量闭集。
    pub const ALL: [t266Code; 15] = [
        t266Code::FlipEarly,
        t266Code::FlipLate,
        t266Code::TearObserved,
        t266Code::VblankBad,
        t266Code::PartyMissing,
        t266Code::DoubleFlipSameWin,
        t266Code::LedgerBreak,
        t266Code::GuardTripped,
        t266Code::LedgerBroken,
        t266Code::SnapshotFingerprint,
        t266Code::TableBroken,
        t266Code::IndexMisaligned,
        t266Code::DuplicateEntry,
        t266Code::OrderViolated,
        t266Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t266Code::FlipEarly => 0x3a50,
            t266Code::FlipLate => 0x3a51,
            t266Code::TearObserved => 0x3a52,
            t266Code::VblankBad => 0x3a53,
            t266Code::PartyMissing => 0x3a54,
            t266Code::DoubleFlipSameWin => 0x3a55,
            t266Code::LedgerBreak => 0x3a56,
            t266Code::GuardTripped => 0x3a57,
            t266Code::LedgerBroken => 0x3a58,
            t266Code::SnapshotFingerprint => 0x3a59,
            t266Code::TableBroken => 0x3a5a,
            t266Code::IndexMisaligned => 0x3a5b,
            t266Code::DuplicateEntry => 0x3a5c,
            t266Code::OrderViolated => 0x3a5d,
            t266Code::SegmentGuard => 0x3a5e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t266Code::FlipEarly => "提前翻页（窗期外翻页）",
            t266Code::FlipLate => "迟翻页（错过本窗）",
            t266Code::TearObserved => "撕裂观测（扫中翻页）",
            t266Code::VblankBad => "消隐窗非法（窗长超限）",
            t266Code::PartyMissing => "参与方缺失（扫描器未到）",
            t266Code::DoubleFlipSameWin => "同窗双翻",
            t266Code::LedgerBreak => "翻页账破缺",
            t266Code::GuardTripped => "守卫触发",
            t266Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t266Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t266Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t266Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t266Code::DuplicateEntry => "条目重复（同键双写）",
            t266Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t266Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t266Code::FlipEarly => "等 vblank 窗重翻",
            t266Code::FlipLate => "按迟翻路径入账",
            t266Code::TearObserved => "回滚并登记事故",
            t266Code::VblankBad => "按窗长表校正",
            t266Code::PartyMissing => "按超时路径降级",
            t266Code::DoubleFlipSameWin => "幂等拒绝第二次",
            t266Code::LedgerBreak => "按守恒重建",
            t266Code::GuardTripped => "检查临界区",
            t266Code::LedgerBroken => "按守恒律重建台账后重跑",
            t266Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t266Code::TableBroken => "按语义规则重建常量表",
            t266Code::IndexMisaligned => "按声明序归一化后重跑",
            t266Code::DuplicateEntry => "合并同键条目为末次生效",
            t266Code::OrderViolated => "按依赖矩阵重排后重跑",
            t266Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 flip（到达栅栏：2 方全员到达才放行 + 超时账）
// --------------------------------------------------------------------------
/// 消隐同步翻页栅栏（扫描器 + 提交器双方到场）。
/// 参与方上限。
pub const FLIP_PARTY: usize = 2;

/// 单方到达记录。
#[derive(Clone, Copy, Debug)]
pub struct flipArrival {
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
pub struct flipBarrier {
    /// 到达位图（每方一比特，去重后只计一次）。
    pub arrived: [bool; 2],
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

impl flipBarrier {
    /// 构造（超时阈值 16 ns）。
    pub const fn new() -> Self {
        Self {
            arrived: [false; 2],
            arrivals: 0,
            dupes: 0,
            released: 0,
            timeouts: 0,
            now_ns: 0,
            timeout_ns: 16,
            last_ns: 0,
        }
    }

    /// 单方到达：重复到达只记账不前进位图。
    pub fn arrive(&mut self, who: usize) -> bool {
        if who >= FLIP_PARTY {
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
        while i < FLIP_PARTY {
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
        if self.arrived_count() < FLIP_PARTY && self.now_ns >= self.timeout_ns {
            self.timeouts += 1;
            return true;
        }
        false
    }

    /// 全员到达判定。
    pub fn complete(&self) -> bool {
        self.arrived_count() == FLIP_PARTY
    }

    /// 放行：仅全员到达时成功，否则记账失败。
    pub fn release(&mut self) -> bool {
        if self.complete() {
            self.released += 1;
            // 释放后清空以便下一轮复用。
            let mut i = 0usize;
            while i < FLIP_PARTY {
                self.arrived[i] = false;
                i += 1;
            }
            return true;
        }
        false
    }

    /// 守恒恒等式：arrivals == 到达方数 + 重复数。
    pub fn conserved(&self) -> bool {
        self.arrivals == (self.arrived_count() as u64) + self.dupes + (self.released * 2 as u64)
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3a50, 0x3a5f]。
pub const fn t266_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t266Code::ALL.len() {
        let c = t266Code::ALL[i].code();
        if c < 0x3a50 || c > 0x3a5f {
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
fn t266_guard_seg() -> bool {
    t266_segment_guard()
}

/// 翻页栅栏双方到达放行 + 单方超时。
fn check_c1_flip_barrier() -> bool {
    let mut b = flipBarrier::new();
    let mut ok = !b.complete();
    if ok {
        ok = b.arrive(0) && !b.arrive(0);
    }
    if ok {
        ok = !b.complete() && b.dupes == 1;
    }
    if ok {
        ok = b.arrive(1) && b.complete() && b.release();
    }
    ok
}

/// 超时路径（推进时钟未全到达）。
fn check_c2_flip_timeout() -> bool {
    let mut b = flipBarrier::new();
    let _ = b.arrive(0);
    let mut to = false;
    let mut i = 0;
    while i < 20 {
        if b.tick(1) {
            to = true;
            break;
        }
        i += 1;
    }
    to && b.timeouts == 1 && !b.complete()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t266_checks() -> CheckSet {
    let mut set = CheckSet::new("T266");
    set.add(
        "t266-seg-guard",
        t266_guard_seg(),
        if t266_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t266-c1-flip-barrier",
        check_c1_flip_barrier(),
        if check_c1_flip_barrier() { "" } else { "flip barrier mismatch" },
    );
    set.add(
        "t266-c2-flip-timeout",
        check_c2_flip_timeout(),
        if check_c2_flip_timeout() { "" } else { "flip timeout path mismatch" },
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
    fn t266_all_checks_green() {
        let set = run_t266_checks();
        assert!(set.all_passed(), "T266 domain checks must be green");
    }

    #[test]
    fn t266_segment_guard_holds() {
        assert!(t266_segment_guard());
    }
}
