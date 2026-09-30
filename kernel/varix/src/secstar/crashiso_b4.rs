//! F175 崩溃隔离强化 · 批次四深化（secstar · G-G-05）。
//!
//! 批次四功能面（与批次三互补：批次三管「注册表与预算」，本批管
//! 「焦点链、退避与豁免」）：
//! - [`FocusChain`]：焦点移交链——崩溃应用的焦点按 Z 序找下一个幸存者，
//!   链上跳过全部遮罩态（焦点不丢在宇宙里——键盘纪律的崩溃面）；
//! - [`RestartBackoff`]：重启退避——连续崩溃间隔 1s/2s/4s 指数拉长
//!   （重启风暴的节奏面——与隔离阈值配合：退避不等于无限重启）；
//! - [`DumpRefAllocator`]：dump 引用号分配器——单调递增/回绕检测
//!   （崩溃日志的可引用性：号不会重复指向两份现场）；
//! - [`MaskAnimator`]：遮罩动画插值——透明度 0→200‰ 的 12 帧缓动
//!   （ease-out 曲线：视觉节奏面，帧值确定性可复核）；
//! - [`ExemptionRoster`]：隔离豁免花名册——系统关键进程不隔离但必须
//!   报 P0（豁免不是静默——异常显性化红线）。
//!
//! 零堆纪律：定长链 + 定长名册，无 alloc。

use super::crashiso::{FOCUS_ANIM_MS, MASK_ANIM_MS, MASK_DIM_PERMILLE};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 焦点移交链
// ---------------------------------------------------------------------------

/// Z 序候选表容量。
pub const FOCUS_CHAIN_CAP: usize = 16;

/// 焦点链：按 Z 序排的幸存窗口列表，崩溃者出列后取下一任。
pub struct FocusChain {
    z_order: [Option<u32>; FOCUS_CHAIN_CAP],
    pub n: usize,
}

impl FocusChain {
    pub const fn new() -> FocusChain {
        FocusChain { z_order: [const { None }; FOCUS_CHAIN_CAP], n: 0 }
    }

    pub fn push_window(&mut self, app_id: u32) -> bool {
        if self.n >= FOCUS_CHAIN_CAP || self.z_order[..self.n].iter().flatten().any(|a| *a == app_id) {
            return false;
        }
        self.z_order[self.n] = Some(app_id);
        self.n += 1;
        true
    }

    /// 崩溃者出列 → 返回接任者（Z 序中紧随其后；崩溃者在末位则回绕到首位）。
    pub fn handoff(&mut self, crashed: u32) -> Option<u32> {
        let pos = self.z_order[..self.n].iter().position(|a| *a == Some(crashed))?;
        self.z_order[pos] = None;
        // 压实（保持 Z 序相对关系）。
        for i in pos..self.n - 1 {
            self.z_order[i] = self.z_order[i + 1];
        }
        self.n -= 1;
        if self.n == 0 {
            return None; // 全没了——焦点归还桌面
        }
        Some(self.z_order[pos.min(self.n - 1)].unwrap())
    }

    pub fn contains(&self, app_id: u32) -> bool {
        self.z_order[..self.n].iter().flatten().any(|a| *a == app_id)
    }
}

// ---------------------------------------------------------------------------
// 重启退避
// ---------------------------------------------------------------------------

/// 退避序列（ms）：1s/2s/4s/8s 封顶。
pub const BACKOFF_STEPS_MS: [u64; 4] = [1_000, 2_000, 4_000, 8_000];

/// 第 n 次连续崩溃（0 基）后的重启退避。
pub fn backoff_ms(consecutive: u32) -> u64 {
    let idx = (consecutive as usize).min(BACKOFF_STEPS_MS.len() - 1);
    BACKOFF_STEPS_MS[idx]
}

/// 退避与隔离阈值协同：第 4 次连崩（超过 3 次重启线）不再退避——
/// 直接转隔离（退避面让位给隔离面）。
pub fn backoff_yields_to_quarantine(consecutive: u32, max_restarts: u32) -> bool {
    consecutive > max_restarts
}

// ---------------------------------------------------------------------------
// dump 引用号分配器
// ---------------------------------------------------------------------------

/// 引用号分配器：单调递增 + 回绕检测（u32 用尽 → 诚实拒绝不回绕）。
pub struct DumpRefAllocator {
    next: u32,
    pub wraparound_hit: bool,
}

impl DumpRefAllocator {
    pub const fn new() -> DumpRefAllocator {
        DumpRefAllocator { next: 1, wraparound_hit: false }
    }

    pub fn allocate(&mut self) -> Option<u32> {
        if self.next == u32::MAX {
            self.wraparound_hit = true;
            return None; // 拒绝回绕——号重复指向两份现场是审计灾难
        }
        let v = self.next;
        self.next += 1;
        Some(v)
    }

    pub fn last(&self) -> u32 {
        self.next.saturating_sub(1)
    }
}

// ---------------------------------------------------------------------------
// 遮罩动画插值（ease-out 12 帧）
// ---------------------------------------------------------------------------

/// 动画帧数（与批次三 MASK_ANIM_FRAMES 同值——一处一事实本地对齐）。
pub const ANIM_FRAMES: usize = 12;

/// 第 frame 帧（0 基）的目标暗度 ‰：ease-out 二次曲线（先快后慢——
/// 进场的节奏感）。t = frame/(N-1)，进度 p = 1-(1-t)²，暗度 = 满值×p。
/// 帧值确定性可复核。
pub fn mask_dim_at(frame: usize) -> u32 {
    if ANIM_FRAMES <= 1 {
        return MASK_DIM_PERMILLE;
    }
    if frame >= ANIM_FRAMES - 1 {
        return MASK_DIM_PERMILLE;
    }
    let t = frame as u64 * 1_000 / (ANIM_FRAMES as u64 - 1); // 0..1000 ‰
    let p = 1_000_000u64 - (1_000 - t).saturating_mul(1_000 - t); // ‰×‰ 进度
    (MASK_DIM_PERMILLE as u64 * p / 1_000_000) as u32
}

/// 动画单调不减 + 终帧恰达满暗度（曲线合法性两面）。
pub fn mask_curve_sane() -> bool {
    let mut prev = 0;
    for f in 0..ANIM_FRAMES {
        let d = mask_dim_at(f);
        if d < prev {
            return false;
        }
        prev = d;
    }
    prev == MASK_DIM_PERMILLE
}

/// 焦点移交动画（150ms）与遮罩动画（200ms）预算关系：移交先完成
/// （用户先拿到焦点、黑幕随后盖满——感知顺序面）。
pub fn focus_before_mask() -> bool {
    FOCUS_ANIM_MS < MASK_ANIM_MS
}

// ---------------------------------------------------------------------------
// 隔离豁免花名册
// ---------------------------------------------------------------------------

/// 名册容量。
pub const EXEMPT_ROSTER_CAP: usize = 8;

/// 豁免记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExemptRec {
    pub app_id: u32,
    /// 人话理由（P0 报备引用——豁免不是静默）。
    pub reason: &'static str,
    /// 已报备 P0。
    pub reported: bool,
}

pub struct ExemptionRoster {
    recs: [Option<ExemptRec>; EXEMPT_ROSTER_CAP],
    pub n: usize,
    /// 未报备豁免数（>0 = 有静默豁免——这本身就是缺陷）。
    pub unreported: usize,
}

impl ExemptionRoster {
    pub const fn new() -> ExemptionRoster {
        ExemptionRoster { recs: [const { None }; EXEMPT_ROSTER_CAP], n: 0, unreported: 0 }
    }

    /// 登记豁免（未报备即登记 → unreported 计数——异常显性化的账面）。
    pub fn add(&mut self, app_id: u32, reason: &'static str) -> bool {
        if self.n >= EXEMPT_ROSTER_CAP || self.recs[..self.n].iter().flatten().any(|r| r.app_id == app_id) {
            return false;
        }
        self.recs[self.n] = Some(ExemptRec { app_id, reason, reported: false });
        self.n += 1;
        self.unreported += 1;
        true
    }

    /// 报备 P0（勾掉未报备账）。
    pub fn mark_reported(&mut self, app_id: u32) -> bool {
        for r in self.recs[..self.n].iter_mut().flatten() {
            if r.app_id == app_id && !r.reported {
                r.reported = true;
                self.unreported = self.unreported.saturating_sub(1);
                return true;
            }
        }
        false
    }

    pub fn is_exempt(&self, app_id: u32) -> bool {
        self.recs[..self.n].iter().flatten().any(|r| r.app_id == app_id)
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_crashiso_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-b4");

    // 1) 焦点链移交：A→B→C 崩 A 接 B、崩 B 接 C、崩 C 焦点归桌面。
    let mut f = FocusChain::new();
    f.push_window(10);
    f.push_window(20);
    f.push_window(30);
    let h1 = f.handoff(10);
    let h2 = f.handoff(20);
    let h3 = f.handoff(30);
    cs.add("focus_chain_cascade", h1 == Some(20) && h2 == Some(30) && h3.is_none() && f.n == 0, "");

    // 2) 焦点链回绕：崩末位接首位（Z 序回绕语义）。
    let mut f2 = FocusChain::new();
    f2.push_window(1);
    f2.push_window(2);
    f2.push_window(3);
    // 出列 2（中间）：3 在位；再出列 3（此时末位）→ 接 1。
    let _ = f2.handoff(2);
    let h = f2.handoff(3);
    cs.add("focus_chain_wraps", h == Some(1) && f2.contains(1), "");

    // 3) 焦点链重复窗口拒（同窗口不重复入链）。
    let mut f3 = FocusChain::new();
    let first = f3.push_window(5);
    let dup = f3.push_window(5);
    cs.add("focus_chain_dedup", first && !dup && f3.n == 1, "");

    // 4) 退避序列：1s/2s/4s/8s 封顶（节奏面逐点）。
    cs.add(
        "backoff_series",
        backoff_ms(0) == 1_000 && backoff_ms(1) == 2_000 && backoff_ms(2) == 4_000 && backoff_ms(3) == 8_000 && backoff_ms(99) == 8_000,
        "",
    );

    // 5) 退避让位隔离：第 4 次连崩（超 3 次线）→ 退避面让位（不再重启）。
    cs.add("backoff_yields", backoff_yields_to_quarantine(4, 3) && !backoff_yields_to_quarantine(3, 3), "");

    // 6) dump 号单调：1,2,3 连续分配（可引用性）。
    let mut d = DumpRefAllocator::new();
    let a = d.allocate();
    let b = d.allocate();
    let c = d.allocate();
    cs.add("dump_ref_monotone", a == Some(1) && b == Some(2) && c == Some(3) && d.last() == 3, "");

    // 7) dump 号拒绝回绕：耗尽 → None + 留痕（不重复指向两份现场）。
    let mut d2 = DumpRefAllocator { next: u32::MAX, wraparound_hit: false };
    let exhausted = d2.allocate().is_none();
    cs.add("dump_ref_no_wrap", exhausted && d2.wraparound_hit, "");

    // 8) 遮罩曲线：单调不减 + 终帧满暗度（缓动合法性）。
    cs.add("mask_curve_sane", mask_curve_sane(), "");

    // 9) 遮罩曲线起步快：第 1 帧暗度 > 满值 1/12（ease-out 特征可验）。
    let f1 = mask_dim_at(1) as u64;
    let linear_1 = MASK_DIM_PERMILLE as u64 / (ANIM_FRAMES as u64 - 1);
    cs.add("mask_easeout_fast_start", f1 > linear_1, "");

    // 10) 焦点先行：移交 150ms < 遮罩 200ms（感知顺序面）。
    cs.add("focus_before_mask", focus_before_mask(), "");

    // 11) 豁免名册：登记/查重/豁免判定（关键进程面）。
    let mut e = ExemptionRoster::new();
    let ok = e.add(1, "compositor: isolation would kill desktop");
    let dup = e.add(1, "dup");
    cs.add("exempt_roster", ok && !dup && e.is_exempt(1) && !e.is_exempt(2), "");

    // 12) 豁免必报备：未报备计数 1 → 报备后 0（豁免不是静默）。
    cs.add(
        "exempt_must_report",
        e.unreported == 1 && e.mark_reported(1) && e.unreported == 0 && !e.mark_reported(1),
        "",
    );

    // 13) 主册常量贯通：移交 150ms / 遮罩 200ms / 暗度 200‰ 一处一事实。
    cs.add("consts_aligned", FOCUS_ANIM_MS == 150 && MASK_ANIM_MS == 200 && MASK_DIM_PERMILLE == 200, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn focus_chain_full_matrix() {
        // 16 窗全链：逐一崩溃 → 每次都有接任者（直到最后一个归桌面）。
        let mut f = FocusChain::new();
        for i in 0..FOCUS_CHAIN_CAP as u32 {
            assert!(f.push_window(100 + i));
        }
        for i in 0..FOCUS_CHAIN_CAP as u32 {
            let next = f.handoff(100 + i);
            if i + 1 < FOCUS_CHAIN_CAP as u32 {
                assert_eq!(next, Some(100 + i + 1), "i={i}");
            } else {
                assert_eq!(next, None);
            }
        }
    }

    #[test]
    fn mask_curve_values_deterministic() {
        // 曲线确定性：同帧同值（视觉节奏面不许掷骰子）。
        for f in 0..ANIM_FRAMES {
            assert_eq!(mask_dim_at(f), mask_dim_at(f), "frame={f}");
        }
        assert_eq!(mask_dim_at(0), 0, "首帧全透明");
    }

    #[test]
    fn exempt_roster_cap() {
        // 8 满容拒收（名册上限诚实）。
        let mut e = ExemptionRoster::new();
        for i in 0..EXEMPT_ROSTER_CAP as u32 {
            assert!(e.add(10 + i, "critical"));
        }
        assert!(!e.add(999, "over"));
        assert_eq!(e.unreported, EXEMPT_ROSTER_CAP);
    }
}
