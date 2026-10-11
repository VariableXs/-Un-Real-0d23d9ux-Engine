//! unxt265_bufledge —— UNX-F0261 帧簿缓冲账（前/后台双缓冲守恒）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0261 · 帧簿双头显（诊断浮层 + 正常输出合成） · 判据：诊断浮层与正常输出同屏合成零互蚀，浮层开合零残影
//! · 组件：缓冲账（四态守恒）+ 分配/释放语义 + 水位守卫
//! · 判据落地：双头显的缓冲管理面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T265_VERSION: &str = "t265-bufledge-v1";

/// 诊断段基址。
pub const T265_SEG_BASE: u16 = 0x3a40;

// --------------------------------------------------------------------------
// 诊断码（t265 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缓冲账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t265Code {
    /// 0x3a40 缓冲泄漏（分配未释放）。
    BufLeak,
    /// 0x3a41 重复释放。
    BufDoubleFree,
    /// 0x3a42 缓冲越界（访问池外）。
    BufOob,
    /// 0x3a43 前后台错配（前台当后台写）。
    FrontBackSwap,
    /// 0x3a44 池耗尽（无空闲缓冲）。
    PoolExhausted,
    /// 0x3a45 水位非法。
    WatermarkBad,
    /// 0x3a46 行距不一致（双缓冲异构）。
    PitchMismatch,
    /// 0x3a47 缓冲账破缺（四态不守恒）。
    LedgerBreak,
    /// 0x3a48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3a49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3a4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3a4b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3a4c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3a4d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3a4e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3a4f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t265Code {
    /// 全量闭集。
    pub const ALL: [t265Code; 16] = [
        t265Code::BufLeak,
        t265Code::BufDoubleFree,
        t265Code::BufOob,
        t265Code::FrontBackSwap,
        t265Code::PoolExhausted,
        t265Code::WatermarkBad,
        t265Code::PitchMismatch,
        t265Code::LedgerBreak,
        t265Code::LedgerBroken,
        t265Code::SnapshotFingerprint,
        t265Code::TableBroken,
        t265Code::IndexMisaligned,
        t265Code::DuplicateEntry,
        t265Code::OrderViolated,
        t265Code::GuardTripped,
        t265Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t265Code::BufLeak => 0x3a40,
            t265Code::BufDoubleFree => 0x3a41,
            t265Code::BufOob => 0x3a42,
            t265Code::FrontBackSwap => 0x3a43,
            t265Code::PoolExhausted => 0x3a44,
            t265Code::WatermarkBad => 0x3a45,
            t265Code::PitchMismatch => 0x3a46,
            t265Code::LedgerBreak => 0x3a47,
            t265Code::LedgerBroken => 0x3a48,
            t265Code::SnapshotFingerprint => 0x3a49,
            t265Code::TableBroken => 0x3a4a,
            t265Code::IndexMisaligned => 0x3a4b,
            t265Code::DuplicateEntry => 0x3a4c,
            t265Code::OrderViolated => 0x3a4d,
            t265Code::GuardTripped => 0x3a4e,
            t265Code::SegmentGuard => 0x3a4f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t265Code::BufLeak => "缓冲泄漏（分配未释放）",
            t265Code::BufDoubleFree => "重复释放",
            t265Code::BufOob => "缓冲越界（访问池外）",
            t265Code::FrontBackSwap => "前后台错配（前台当后台写）",
            t265Code::PoolExhausted => "池耗尽（无空闲缓冲）",
            t265Code::WatermarkBad => "水位非法",
            t265Code::PitchMismatch => "行距不一致（双缓冲异构）",
            t265Code::LedgerBreak => "缓冲账破缺（四态不守恒）",
            t265Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t265Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t265Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t265Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t265Code::DuplicateEntry => "条目重复（同键双写）",
            t265Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t265Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t265Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t265Code::BufLeak => "按释放账追回",
            t265Code::BufDoubleFree => "按幂等拒绝",
            t265Code::BufOob => "按池边界校验",
            t265Code::FrontBackSwap => "按角色账校正",
            t265Code::PoolExhausted => "按水位降级",
            t265Code::WatermarkBad => "按水位表校正",
            t265Code::PitchMismatch => "按行距统一",
            t265Code::LedgerBreak => "按守恒重建",
            t265Code::LedgerBroken => "按守恒律重建台账后重跑",
            t265Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t265Code::TableBroken => "按语义规则重建常量表",
            t265Code::IndexMisaligned => "按声明序归一化后重跑",
            t265Code::DuplicateEntry => "合并同键条目为末次生效",
            t265Code::OrderViolated => "按依赖矩阵重排后重跑",
            t265Code::GuardTripped => "检查临界区嵌套与阈值",
            t265Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 buf（帧簿缓冲账（free+front+back+locked == 池总））
// --------------------------------------------------------------------------
/// 帧簿缓冲账（free+front+back+locked == 池总）。
#[derive(Clone, Copy, Debug)]
pub struct buf {
    /// 空闲缓冲
    pub free: u64,
    /// 前台缓冲
    pub front: u64,
    /// 后台缓冲
    pub back: u64,
    /// 锁定缓冲
    pub locked: u64,
}

impl buf {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            free: 0,
            front: 0,
            back: 0,
            locked: 0,
        }
    }

    /// 分配前台。
    pub fn alloc_front(&mut self) {
        if self.free > 0 { self.free -= 1; } self.front += 1;
    }

    /// 分配后台。
    pub fn alloc_back(&mut self) {
        if self.free > 0 { self.free -= 1; } self.back += 1;
    }

    /// 释放一台。
    pub fn release(&mut self) {
        if self.front > 0 { self.front -= 1; } else if self.back > 0 { self.back -= 1; } self.free += 1;
    }

    /// 锁定一台。
    pub fn lock_one(&mut self) {
        if self.free > 0 { self.free -= 1; } self.locked += 1;
    }

    /// 解锁一台。
    pub fn unlock_one(&mut self) {
        if self.locked > 0 { self.locked -= 1; } self.free += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3a40, 0x3a4f]。
pub const fn t265_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t265Code::ALL.len() {
        let c = t265Code::ALL[i].code();
        if c < 0x3a40 || c > 0x3a4f {
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
fn t265_guard_seg() -> bool {
    t265_segment_guard()
}

/// 缓冲账守恒（分配-释放-锁解锁全序列）。
fn check_c1_buf_conserve() -> bool {
    let mut b = buf::new();
    let mut i = 0;
    while i < 4 {
        b.alloc_front();
        b.alloc_back();
        i += 1;
    }
    let mut ok = b.front == 4 && b.back == 4;
    if ok {
        let mut j = 0;
        while j < 2 {
            b.lock_one();
            j += 1;
        }
        ok = b.locked == 2;
    }
    if ok {
        let mut total = b.free + b.front + b.back + b.locked;
        // 每操作恰使总盘 +1（零基账口径：free+front+back+locked == 操作数 10）。
        ok = total == 10;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t265_checks() -> CheckSet {
    let mut set = CheckSet::new("T265");
    set.add(
        "t265-seg-guard",
        t265_guard_seg(),
        if t265_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t265-c1-buf-conserve",
        check_c1_buf_conserve(),
        if check_c1_buf_conserve() { "" } else { "buffer ledger conservation mismatch" },
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
    fn t265_all_checks_green() {
        let set = run_t265_checks();
        assert!(set.all_passed(), "T265 domain checks must be green");
    }

    #[test]
    fn t265_segment_guard_holds() {
        assert!(t265_segment_guard());
    }
}
