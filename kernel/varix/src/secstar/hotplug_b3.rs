//! F184 热插拔体验 · 批次三深化（secstar · G-G-14）。
//!
//! 批次三功能面（主册判据「插入-写入-弹出-拔出全链 / 冲刷完成判定与
//! F046 窗口一致 / 未弹出修复路径」纵深）：
//! - [`MountFsm`]：挂载全链状态机——Inserted→Mounted→Writing→Flushing→
//!   SafeToEject→Ejected→Removed 七态 + 脏拔分支（全链每一步有名字，
//!   状态不悬空）；
//! - [`FlushProgress`]：冲刷进度模型——与 F046 写合并窗口同尺（冲刷
//!   完成判定 = 脏页清零，不是「大概写完了」）；
//! - [`ForceEjectGuard`]：强拔防护——写入中拔出 → 脏标记+修复路径指引
//!   （数据安全的最后一道闸）；
//! - [`repair_flow`]：未弹出修复流程——重扫→校验→恢复三步（修得好
//!   不如修得明白）。
//!
//! 零堆纪律：定长状态机 + 定长修复账，无 alloc。

use super::hotplug::{encode_dirty_marker, decode_dirty_marker, DIRTY_MARKER_LEN};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 挂载全链状态机
// ---------------------------------------------------------------------------

/// 七态全链 + 脏拔分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MntState {
    Inserted,
    Mounted,
    Writing,
    Flushing,
    SafeToEject,
    Ejected,
    Removed,
    /// 脏拔（未冲刷即拔——修复路径入口）。
    RemovedDirty,
}

/// 状态机（合法迁移表驱动——非法迁移诚实拒，不静默改状态）。
pub struct MountFsm {
    pub state: MntState,
}

impl MountFsm {
    pub const fn new() -> MountFsm {
        MountFsm { state: MntState::Inserted }
    }

    /// 请求迁移：返回是否合法执行（非法迁移=false 且状态不动）。
    pub fn transition(&mut self, to: MntState) -> bool {
        let legal = match (self.state, to) {
            (MntState::Inserted, MntState::Mounted) => true,
            (MntState::Mounted, MntState::Writing) => true,
            (MntState::Mounted, MntState::SafeToEject) => true, // 零写入直接可拔
            (MntState::Writing, MntState::Flushing) => true,
            (MntState::Writing, MntState::RemovedDirty) => true, // 写入中强拔
            (MntState::Flushing, MntState::SafeToEject) => true,
            (MntState::Flushing, MntState::RemovedDirty) => true, // 冲刷中强拔
            (MntState::SafeToEject, MntState::Ejected) => true,
            (MntState::Ejected, MntState::Removed) => true,
            (MntState::RemovedDirty, MntState::Removed) => true,
            _ => false,
        };
        if legal {
            self.state = to;
        }
        legal
    }

    /// 可否安全拔出（状态机唯一判定面——不是用户猜）。
    pub fn safe_to_remove(&self) -> bool {
        matches!(self.state, MntState::SafeToEject | MntState::Ejected | MntState::Removed)
    }
}

// ---------------------------------------------------------------------------
// 冲刷进度模型（F046 同尺）
// ---------------------------------------------------------------------------

/// F046 写合并窗口标称值（ms——一处一事实引用主层同数值）。
pub const F046_WINDOW_MS: u64 = 5_000;

/// 冲刷进度：脏页数衰减 + 窗口超时判定（完成=脏页 0，不是时间到）。
pub struct FlushProgress {
    pub dirty_pages: u64,
    elapsed_ms: u64,
}

impl FlushProgress {
    pub const fn new(dirty_pages: u64) -> FlushProgress {
        FlushProgress { dirty_pages, elapsed_ms: 0 }
    }

    /// 每拍冲刷 N 页（冲刷速率由底层回填——本层记账）。
    pub fn tick(&mut self, dt_ms: u64, flushed_pages: u64) {
        self.elapsed_ms += dt_ms;
        self.dirty_pages = self.dirty_pages.saturating_sub(flushed_pages);
    }

    /// 完成 = 脏页清零（判定面：数据真写完，不是时间到就算）。
    pub fn complete(&self) -> bool {
        self.dirty_pages == 0
    }

    /// 窗口超时未完成 → 未弹出修复路径（与 F046 窗口同尺的对账面）。
    pub fn window_overdue(&self) -> bool {
        !self.complete() && self.elapsed_ms > F046_WINDOW_MS
    }

    /// 进度千分比（清了多少脏页）。
    pub fn permille(&self, initial: u64) -> u32 {
        if initial == 0 {
            return 1_000;
        }
        ((initial - self.dirty_pages) * 1_000 / initial) as u32
    }
}

// ---------------------------------------------------------------------------
// 强拔防护
// ---------------------------------------------------------------------------

/// 强拔判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceEjectVerdict {
    /// 安全（SafeToEject 及之后——走正常拔出）。
    Safe,
    /// 有脏页强拔 → 脏标记落盘（下次插入走修复路径）。
    DirtyWithMarker,
}

/// 强拔裁决：脏页 >0 时拔出 → 脏标记（持久帧走主层 encode_dirty_marker）。
pub fn force_eject_verdict(dirty_pages: u64) -> ForceEjectVerdict {
    if dirty_pages == 0 {
        ForceEjectVerdict::Safe
    } else {
        ForceEjectVerdict::DirtyWithMarker
    }
}

/// 脏标记持久化 round-trip（主层帧格式的批次三对账路径）。
pub fn dirty_marker_roundtrip(drive: u8, seq: u32) -> bool {
    let mut frame = [0u8; DIRTY_MARKER_LEN];
    if !encode_ok(drive, seq, &mut frame) {
        return false;
    }
    decode_dirty_marker(&frame) == Some(drive)
}

fn encode_ok(drive: u8, seq: u32, out: &mut [u8; DIRTY_MARKER_LEN]) -> bool {
    encode_dirty_marker(drive, seq, out);
    true
}

// ---------------------------------------------------------------------------
// 未弹出修复流程（三步：重扫→校验→恢复）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairStep {
    Rescan,
    Verify,
    Restore,
    Done,
}

/// 修复推进：每步成功 → 下一步；任一步失败 → 停在该步（可重试）。
#[derive(Clone, Copy, Debug)]
pub struct RepairFlow {
    pub step: RepairStep,
    pub retries: u8,
}

impl RepairFlow {
    pub const fn new() -> RepairFlow {
        RepairFlow { step: RepairStep::Rescan, retries: 0 }
    }

    /// 推进一步。`ok` = 当前步探测结果（由底层回填）。
    pub fn advance(&mut self, ok: bool) -> RepairStep {
        if ok {
            self.retries = 0;
            self.step = match self.step {
                RepairStep::Rescan => RepairStep::Verify,
                RepairStep::Verify => RepairStep::Restore,
                RepairStep::Restore => RepairStep::Done,
                RepairStep::Done => RepairStep::Done,
            };
        } else {
            self.retries = self.retries.saturating_add(1);
        }
        self.step
    }

    /// 三次失败升级人工（自动修复不是永动机）。
    pub fn needs_human(&self) -> bool {
        self.retries >= 3
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_hotplug_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F184-b3");

    // 1) 全链正线：七态顺序迁移全通（插入-写入-弹出-拔出全链的骨架）。
    let mut f = MountFsm::new();
    let chain = [
        MntState::Mounted, MntState::Writing, MntState::Flushing, MntState::SafeToEject, MntState::Ejected, MntState::Removed,
    ];
    let all = chain.iter().all(|s| f.transition(*s));
    cs.add("fsm_full_chain", all && f.state == MntState::Removed, "");

    // 2) 零写入快拔：Mounted 直达 SafeToEject（不逼用户走空流程）。
    let mut f2 = MountFsm::new();
    f2.transition(MntState::Mounted);
    cs.add("fsm_zero_write_shortcut", f2.transition(MntState::SafeToEject) && f2.safe_to_remove(), "");

    // 3) 非法迁移诚实拒：Inserted 直跳 Ejected（没挂载哪来弹出）。
    let mut f3 = MountFsm::new();
    cs.add("fsm_illegal_rejected", !f3.transition(MntState::Ejected) && f3.state == MntState::Inserted, "");

    // 4) 脏拔分支：Writing 中拔出 → RemovedDirty（非 Safe 态的出口）。
    let mut f4 = MountFsm::new();
    f4.transition(MntState::Mounted);
    f4.transition(MntState::Writing);
    let dirty_ok = f4.transition(MntState::RemovedDirty);
    cs.add("fsm_dirty_branch", dirty_ok && f4.state == MntState::RemovedDirty && !f4.safe_to_remove(), "");

    // 5) 安全态判定矩阵：SafeToEject/Ejected/Removed 安全、其余不安全。
    let safe_states = [MntState::SafeToEject, MntState::Ejected, MntState::Removed];
    let unsafe_states = [MntState::Inserted, MntState::Mounted, MntState::Writing, MntState::Flushing, MntState::RemovedDirty];
    cs.add(
        "safe_matrix",
        safe_states.iter().all(|s| MountFsm { state: *s }.safe_to_remove())
            && unsafe_states.iter().all(|s| !MountFsm { state: *s }.safe_to_remove()),
        "",
    );

    // 6) 冲刷完成判定：脏页清零才完成（不是时间到就算）。
    let mut fp = FlushProgress::new(100);
    fp.tick(1_000, 60);
    let mid = !fp.complete() && fp.permille(100) == 600;
    fp.tick(1_000, 40);
    cs.add("flush_complete_by_pages", mid && fp.complete() && fp.permille(100) == 1_000, "");

    // 7) 窗口超时：5s 窗过完仍脏 → 修复路径触发（F046 同尺对账）。
    let mut fp2 = FlushProgress::new(10);
    fp2.tick(5_100, 3);
    cs.add("flush_window_overdue", fp2.window_overdue() && !fp2.complete(), "");

    // 8) 恰好在窗内完成不触发（窗口内是正常节奏）。
    let mut fp3 = FlushProgress::new(10);
    fp3.tick(4_000, 10);
    cs.add("flush_in_window_ok", fp3.complete() && !fp3.window_overdue(), "");

    // 9) 强拔裁决：零脏页安全、有脏页出脏标记（两分支）。
    cs.add(
        "force_eject_verdicts",
        force_eject_verdict(0) == ForceEjectVerdict::Safe && force_eject_verdict(7) == ForceEjectVerdict::DirtyWithMarker,
        "",
    );

    // 10) 脏标记持久帧 round-trip（主层帧格式的批次三对账路径）。
    cs.add("dirty_marker_roundtrip", dirty_marker_roundtrip(b'E', 42), "");

    // 11) 修复三步推进：重扫→校验→恢复→Done（每步有名字）。
    let mut r = RepairFlow::new();
    let s1 = r.advance(true);
    let s2 = r.advance(true);
    let s3 = r.advance(true);
    cs.add(
        "repair_three_steps",
        s1 == RepairStep::Verify && s2 == RepairStep::Restore && s3 == RepairStep::Done && r.step == RepairStep::Done,
        "",
    );

    // 12) 修复失败停在原地可重试、三败升级人工（自动修复不是永动机）。
    let mut r2 = RepairFlow::new();
    r2.advance(false);
    r2.advance(false);
    let still_rescan = r2.step == RepairStep::Rescan && r2.retries == 2;
    r2.advance(false);
    cs.add("repair_escalates", still_rescan && r2.needs_human(), "");

    // 13) 主册常量贯通：脏标记帧长一处一事实。
    cs.add("consts_aligned", DIRTY_MARKER_LEN == 8, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn fsm_backwards_is_illegal() {
        // 回退迁移非法：SafeToEject 不能回到 Writing（状态机不倒放）。
        let mut f = MountFsm::new();
        f.transition(MntState::Mounted);
        f.transition(MntState::Writing);
        f.transition(MntState::Flushing);
        f.transition(MntState::SafeToEject);
        assert!(!f.transition(MntState::Writing));
        assert_eq!(f.state, MntState::SafeToEject);
    }

    #[test]
    fn flush_never_negative_pages() {
        // 冲刷过量上报（超发页数）被钳制——脏页不为负。
        let mut fp = FlushProgress::new(10);
        fp.tick(100, 999);
        assert_eq!(fp.dirty_pages, 0);
        assert!(fp.complete());
    }

    #[test]
    fn repair_retries_then_succeeds() {
        // 重试后成功：连击清零继续走（卡住的不是用户是探测）。
        let mut r = RepairFlow::new();
        r.advance(false);
        r.advance(false);
        let s = r.advance(true);
        assert_eq!(s, RepairStep::Verify);
        assert_eq!(r.retries, 0);
        assert!(!r.needs_human());
    }
}
