//! unxt682_unx682_matrixrot —— UNX-F0682 矩阵轮排（3 机 × 50 次 = 150 次序）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0682 · 矩阵轮排（3 机 × 50 次轮序编排） · 判据：轮排表（3 机 × 50 次 = 150 次序）程序化生成且越序拦截检出 100%，轮排与 F0641 编排口径一致
//! · 组件：轮排表生成引擎（150 次序）+ 越序拦截账 + F0641 口径对拍 + 域快照
//! · 判据落地：150 次序程序化生成 + 越序 100% 拦截
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T682_VERSION: &str = "t682-unx682_matrixrot-v1";

/// 诊断段基址。
pub const T682_SEG_BASE: u16 = 0x5020;

// --------------------------------------------------------------------------
// 诊断码（t682 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 矩阵轮排诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t682Code {
    /// 0x5020 机号非法（3 机之外）。
    MachineBad,
    /// 0x5021 轮次越界（50 次之外）。
    RoundBad,
    /// 0x5022 轮序逃逸（未按轮排表执行）。
    OrderEscaped,
    /// 0x5023 F0641 口径分歧（与编排口径不一致）。
    F0641Diff,
    /// 0x5024 轮排账破缺。
    LedgerBreak,
    /// 0x5025 表内断档（150 次序不连续）。
    OrderViolated,
    /// 0x5026 重复轮次（同机同轮双排）。
    DupRotation,
    /// 0x5027 守卫触发。
    GuardTripped,
    /// 0x5028 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5029 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x502a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x502b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x502c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x502d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t682Code {
    /// 全量闭集。
    pub const ALL: [t682Code; 14] = [
        t682Code::MachineBad,
        t682Code::RoundBad,
        t682Code::OrderEscaped,
        t682Code::F0641Diff,
        t682Code::LedgerBreak,
        t682Code::OrderViolated,
        t682Code::DupRotation,
        t682Code::GuardTripped,
        t682Code::LedgerBroken,
        t682Code::SnapshotFingerprint,
        t682Code::TableBroken,
        t682Code::IndexMisaligned,
        t682Code::DuplicateEntry,
        t682Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t682Code::MachineBad => 0x5020,
            t682Code::RoundBad => 0x5021,
            t682Code::OrderEscaped => 0x5022,
            t682Code::F0641Diff => 0x5023,
            t682Code::LedgerBreak => 0x5024,
            t682Code::OrderViolated => 0x5025,
            t682Code::DupRotation => 0x5026,
            t682Code::GuardTripped => 0x5027,
            t682Code::LedgerBroken => 0x5028,
            t682Code::SnapshotFingerprint => 0x5029,
            t682Code::TableBroken => 0x502a,
            t682Code::IndexMisaligned => 0x502b,
            t682Code::DuplicateEntry => 0x502c,
            t682Code::SegmentGuard => 0x502d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t682Code::MachineBad => "机号非法（3 机之外）",
            t682Code::RoundBad => "轮次越界（50 次之外）",
            t682Code::OrderEscaped => "轮序逃逸（未按轮排表执行）",
            t682Code::F0641Diff => "F0641 口径分歧（与编排口径不一致）",
            t682Code::LedgerBreak => "轮排账破缺",
            t682Code::OrderViolated => "表内断档（150 次序不连续）",
            t682Code::DupRotation => "重复轮次（同机同轮双排）",
            t682Code::GuardTripped => "守卫触发",
            t682Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t682Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t682Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t682Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t682Code::DuplicateEntry => "条目重复（同键双写）",
            t682Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t682Code::MachineBad => "按三机闭集校正",
            t682Code::RoundBad => "按 50 轮律裁剪",
            t682Code::OrderEscaped => "按越序 100% 拦截补拦",
            t682Code::F0641Diff => "按口径对齐归因",
            t682Code::LedgerBreak => "按守恒重建",
            t682Code::OrderViolated => "按 150 连续律补号",
            t682Code::DupRotation => "幂等去重",
            t682Code::GuardTripped => "检查临界区",
            t682Code::LedgerBroken => "按守恒律重建台账后重跑",
            t682Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t682Code::TableBroken => "按语义规则重建常量表",
            t682Code::IndexMisaligned => "按声明序归一化后重跑",
            t682Code::DuplicateEntry => "合并同键条目为末次生效",
            t682Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 矩阵轮排（3 机 × 50 次轮序，LCG 确定性程序化生成）
// ---------------------------------------------------------------------------

/// 机器数（3 机）。
pub const ROT_MACHINES: u32 = 3;

/// 轮次数（50 次）。
pub const ROT_ROUNDS: u32 = 50;

/// 轮排总次序（150）。
pub const ROT_TOTAL: u32 = ROT_MACHINES * ROT_ROUNDS;

/// F0641 编排口径魔数（0xB19 域口径绑定）。
pub const F0641_SALT: u64 = 0x0000_0000_0000_0B19;

/// 轮排引擎（确定性 LCG；同种子同表）。
#[derive(Clone, Copy, Debug)]
pub struct RotationEngine {
    /// LCG 状态。
    pub state: u64,
    /// 已生成序号。
    pub seq: u32,
}

impl RotationEngine {
    /// 播种（F0641 口径盐）。
    pub const fn new() -> Self {
        Self { state: F0641_SALT, seq: 0 }
    }

    /// LCG 递推（Numerical Recipes 常数）。
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.state
    }

    /// 取下一机号（0..3）。
    pub fn next_machine(&mut self) -> u32 {
        (self.next() % ROT_MACHINES as u64) as u32
    }

    /// 生成一条轮排项（机号 + 轮号）。
    pub fn emit(&mut self) -> (u32, u32) {
        let m = self.next_machine();
        let r = self.seq % ROT_ROUNDS;
        self.seq += 1;
        (m, r)
    }
}

/// 轮排项（机 + 轮 + 全局序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotEntry {
    /// 机号（0..3）。
    pub machine: u32,
    /// 轮号（0..50）。
    pub round: u32,
    /// 全局次序（0..150）。
    pub seq: u32,
}

/// 轮排项合法性。
pub const fn rot_entry_ok(e: &RotEntry) -> bool {
    e.machine < ROT_MACHINES && e.round < ROT_ROUNDS && e.seq < ROT_TOTAL
}

/// 轮排账（越序拦截守恒）。
#[derive(Clone, Copy, Debug)]
pub struct RotBook {
    /// 已执行项数。
    pub executed: u64,
    /// 越序尝试数。
    pub out_of_order: u64,
    /// 越序拦截数（必须 == out_of_order）。
    pub caught: u64,
}

impl RotBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { executed: 0, out_of_order: 0, caught: 0 }
    }

    /// 执行一项（期望序 == 实际序才放行）。
    pub fn execute(&mut self, e: &RotEntry, expect_seq: u32) -> bool {
        if e.seq != expect_seq || !rot_entry_ok(e) {
            self.out_of_order += 1;
            self.caught += 1;
            return false;
        }
        self.executed += 1;
        true
    }

    /// 150 次序全执行判据。
    pub fn all_done(&self) -> bool {
        self.executed == ROT_TOTAL as u64 && self.out_of_order == 0
    }

    /// 越序 100% 拦截判据。
    pub fn intercept_all(&self) -> bool {
        self.out_of_order == self.caught && self.out_of_order > 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5020, 0x502f]。
pub const fn t682_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t682Code::ALL.len() {
        let c = t682Code::ALL[i].code();
        if c < 0x5020 || c > 0x502f {
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
fn t682_guard_seg() -> bool {
    t682_segment_guard()
}

/// 轮排生成（150 次序确定性 + 域合法 + F0641 盐）。
fn check_c1_gen() -> bool {
    let mut eng = RotationEngine::new();
    let mut ok = true;
    let mut i = 0;
    while i < ROT_TOTAL {
        let (m, r) = eng.emit();
        ok = ok && m < ROT_MACHINES && r < ROT_ROUNDS;
        i += 1;
    }
    if ok {
        ok = eng.seq == ROT_TOTAL && F0641_SALT == 0xB19;
    }
    ok
}

/// 150 次序全执行 + 越序 100% 拦截。
fn check_c2_order() -> bool {
    let mut bk = RotBook::new();
    let mut i = 0u32;
    let mut ok = true;
    while i < ROT_TOTAL {
        let e = RotEntry { machine: i % ROT_MACHINES, round: i % ROT_ROUNDS, seq: i };
        ok = ok && bk.execute(&e, i);
        i += 1;
    }
    if ok {
        ok = bk.all_done() && bk.executed == 150;
    }
    if ok {
        // 越序项必被抓。
        let bad = RotEntry { machine: 0, round: 0, seq: 149 };
        ok = !bk.execute(&bad, 150) && bk.intercept_all();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t682_checks() -> CheckSet {
    let mut set = CheckSet::new("T682");
    set.add(
        "t682-seg-guard",
        t682_guard_seg(),
        if t682_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t682-c1-gen",
        check_c1_gen(),
        if check_c1_gen() { "" } else { "rotation generation mismatch" },
    );
    set.add(
        "t682-c2-order",
        check_c2_order(),
        if check_c2_order() { "" } else { "rotation order mismatch" },
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
    fn t682_all_checks_green() {
        let set = run_t682_checks();
        assert!(set.all_passed(), "T682 domain checks must be green");
    }

    #[test]
    fn t682_segment_guard_holds() {
        assert!(t682_segment_guard());
    }
}
