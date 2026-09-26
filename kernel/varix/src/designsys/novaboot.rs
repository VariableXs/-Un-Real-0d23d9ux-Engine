//! NOVA-BOOT · src/system/boot → 内核移植（第三批：开机视觉与启动仪式）。
//!
//! 覆盖 src 三个开机视觉子系统，全部 1:1 在内核在位：
//! 1. boot/ceremony.ts —— 启动仪式状态机（五阶段纯函数 reducer，逐行同构）
//! 2. boot/BootWordmark.tsx —— VARIABLE 八字母字标（逐字母 12.5% 进度区间）
//! 3. boot/CapsuleBar.tsx —— 胶囊进度条（扫光/前导光点/满格呼吸）
//!
//! 硬性规则（docs/ARCHITECTURE_V2.md §5，随移植一并执行）：
//! - progress 单调不减：回放旧 seq / 乱序事件绝不回退；
//! - ready 只认真实 stats 事件，无预设时间线；
//! - skip 仅在真实进度 ≥30% 时接受，跳过的是 UI 而非后台加载。
//!
//! 纯逻辑 + 固定容量：no_std / 仅 core，无分配、无 unsafe。

use crate::checks::CheckSet;
use crate::designsys::nova4k::dp;

// ===========================================================================
// 1. 启动仪式状态机（ceremony.ts 1:1）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CeremonyPhase {
    /// 入场编排
    Entering,
    /// 真实流式加载
    Streaming,
    /// 真实摘要停留
    ReadyHold,
    /// 退出编排
    Exiting,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CeremonyEvent {
    EnterDone,
    /// progress 千分比（0..1000），对应 src 0..1
    Progress(u32),
    Ready,
    Skip,
    ExitDone,
}

/// 进度千分比（1000 = 100%），保证 4K/定点无浮点。
pub const SKIP_THRESHOLD_MILLI: u32 = 300; // 30%
pub const PROGRESS_MAX_MILLI: u32 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CeremonyState {
    pub phase: CeremonyPhase,
    /// 真实进度千分比 0..1000（单调不减）
    pub progress_milli: u32,
    pub ready: bool,
    pub skipped: bool,
}

pub const INITIAL_CEREMONY: CeremonyState =
    CeremonyState { phase: CeremonyPhase::Entering, progress_milli: 0, ready: false, skipped: false };

/// 仪式 reducer：与 ceremony.ts ceremonyReducer 逐分支同构。
pub fn ceremony_reducer(state: CeremonyState, event: CeremonyEvent) -> CeremonyState {
    match event {
        CeremonyEvent::Progress(p) => {
            // 单调不减：回放旧 seq / 乱序事件被 clamp 掉（Math.max 兜底）
            let p = if p > PROGRESS_MAX_MILLI { PROGRESS_MAX_MILLI } else { p };
            let progress = state.progress_milli.max(p);
            if progress == state.progress_milli {
                return state;
            }
            CeremonyState { progress_milli: progress, ..state }
        }
        CeremonyEvent::EnterDone => {
            if state.phase != CeremonyPhase::Entering {
                return state;
            }
            // 快机：入场期间 ready 已到 → 跳过 streaming 直达 readyHold
            let phase = if state.ready { CeremonyPhase::ReadyHold } else { CeremonyPhase::Streaming };
            CeremonyState { phase, ..state }
        }
        CeremonyEvent::Ready => {
            if state.ready {
                return state; // 幂等：重复 ready（回放）不重触发
            }
            if state.phase == CeremonyPhase::Streaming {
                return CeremonyState { ready: true, phase: CeremonyPhase::ReadyHold, ..state };
            }
            // entering：等 ENTER_DONE 再进 readyHold；exiting/done：仅记账
            CeremonyState { ready: true, ..state }
        }
        CeremonyEvent::Skip => {
            if state.phase == CeremonyPhase::Exiting || state.phase == CeremonyPhase::Done {
                return state;
            }
            if state.progress_milli < SKIP_THRESHOLD_MILLI {
                return state; // <30% 拒绝（UI 另行如实提示）
            }
            CeremonyState { phase: CeremonyPhase::Exiting, skipped: true, ..state }
        }
        CeremonyEvent::ExitDone => {
            if state.phase != CeremonyPhase::Exiting && state.phase != CeremonyPhase::ReadyHold {
                return state;
            }
            CeremonyState { phase: CeremonyPhase::Done, ..state }
        }
    }
}

// ===========================================================================
// 2. 节奏档位（pacingTimings 1:1，U-06 bootPacing）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootPacing {
    /// 影院：enter 900ms / readyHold 1200ms
    Cinematic,
    /// 轻快：enter 500ms / readyHold 400ms
    Brisk,
    /// 直通：两等待归零（bootAnim=none 快路径）
    Instant,
}

pub const PACING_CINEMATIC_ENTER: u32 = 900;
pub const PACING_CINEMATIC_HOLD: u32 = 1200;
pub const PACING_BRISK_ENTER: u32 = 500;
pub const PACING_BRISK_HOLD: u32 = 400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PacingTimings {
    pub enter_ms: u32,
    pub ready_hold_ms: u32,
}

pub const fn pacing_timings(p: BootPacing) -> PacingTimings {
    match p {
        BootPacing::Brisk => PacingTimings { enter_ms: PACING_BRISK_ENTER, ready_hold_ms: PACING_BRISK_HOLD },
        BootPacing::Instant => PacingTimings { enter_ms: 0, ready_hold_ms: 0 },
        BootPacing::Cinematic => PacingTimings { enter_ms: PACING_CINEMATIC_ENTER, ready_hold_ms: PACING_CINEMATIC_HOLD },
    }
}

// ===========================================================================
// 3. VARIABLE 字标（BootWordmark.tsx 1:1）
// ===========================================================================
pub const WORDMARK_LETTERS: u32 = 8;
/// 每字母拥有 12.5% 进度区间（1000/8 = 125 毫分）
pub const LETTER_OWN_MILLI: u32 = PROGRESS_MAX_MILLI / WORDMARK_LETTERS;
/// 描边 1.5px + non-scaling-stroke：24px 高度下依旧发丝清晰
pub const WORDMARK_STROKE_DPX10: u32 = 15; // design px ×10
/// 底衬入场级联间隔
pub const WORDMARK_CASCADE_MS: u32 = 40;

/// letterProgress(i) = clamp(progress*8 - i, 0, 1)，输出毫分比。
pub const fn letter_progress_milli(progress_milli: u32, i: usize) -> u32 {
    let v = progress_milli * WORDMARK_LETTERS; // = progress*8（毫分比×8）
    let seg = (i as u32) * PROGRESS_MAX_MILLI;
    if v <= seg {
        0
    } else {
        let d = v - seg;
        if d > PROGRESS_MAX_MILLI {
            PROGRESS_MAX_MILLI
        } else {
            d
        }
    }
}

/// 字标色调：light=深底浅字（默认）；dark=浅底深字。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordmarkTone {
    Light,
    Dark,
}

// ===========================================================================
// 4. 胶囊进度条（CapsuleBar.tsx 1:1）
// ===========================================================================
/// 扫光（sheen）：25% 宽斜向高光带 2.4s 循环横扫（transform-only）
pub const SHEEN_WIDTH_PERMILLE: u32 = 250;
pub const SHEEN_PERIOD_MS: u32 = 2400;
/// 前导光点：7px，同色调 80% 透明度（非彩色霓虹）
pub const LEAD_DOT_DP: u32 = 7;
pub const LEAD_DOT_ALPHA_PERMILLE: u32 = 800;
/// 满格呼吸：progress==1 时 brightness +8%、300ms 一次性
pub const FULL_BREATH_BRIGHTNESS_PERMILLE: u32 = 1080;
pub const FULL_BREATH_MS: u32 = 300;

// ===========================================================================
// 5. 4K 换算出口：开机视觉全部经 dp() 上屏
// ===========================================================================
/// 字标描边物理 px（1.5dp ×10 存档；4K = 3px）。
pub const fn wordmark_stroke_px(scale_milli: u32) -> u32 {
    dp(WORDMARK_STROKE_DPX10, scale_milli) / 10
}

/// 前导光点物理 px（4K = 14px）。
pub const fn lead_dot_px(scale_milli: u32) -> u32 {
    dp(LEAD_DOT_DP, scale_milli)
}

// ===========================================================================
// 6. CheckSet 自检
// ===========================================================================
pub fn checks(cs: &mut CheckSet) {
    fn g(cs: &mut CheckSet, n: &str, ok: bool) {
        cs.check(n, ok);
    }

    // 状态机：主链路 entering → streaming → readyHold → exiting → done
    let s0 = INITIAL_CEREMONY;
    let s1 = ceremony_reducer(s0, CeremonyEvent::Progress(500));
    g(cs, "novaboot-progress", s1.progress_milli == 500 && s1.phase == CeremonyPhase::Entering);
    let s2 = ceremony_reducer(s1, CeremonyEvent::Progress(300));
    g(cs, "novaboot-monotonic", s2.progress_milli == 500); // 单调不减：旧事件 clamp
    let s3 = ceremony_reducer(s2, CeremonyEvent::Progress(1200));
    g(cs, "novaboot-clamp-max", s3.progress_milli == 1000);
    let s4 = ceremony_reducer(s3, CeremonyEvent::EnterDone);
    g(cs, "novaboot-streaming", s4.phase == CeremonyPhase::Streaming);
    let s5 = ceremony_reducer(s4, CeremonyEvent::Ready);
    g(cs, "novaboot-readyhold", s5.phase == CeremonyPhase::ReadyHold && s5.ready);
    let s6 = ceremony_reducer(s5, CeremonyEvent::Ready);
    g(cs, "novaboot-ready-idempotent", s6.phase == CeremonyPhase::ReadyHold);
    let s7 = ceremony_reducer(s6, CeremonyEvent::ExitDone);
    g(cs, "novaboot-exit", s7.phase == CeremonyPhase::Done);
    // 快机：entering 期间 ready 已到 → EnterDone 直达 readyHold
    let fast = ceremony_reducer(ceremony_reducer(INITIAL_CEREMONY, CeremonyEvent::Ready), CeremonyEvent::EnterDone);
    g(cs, "novaboot-fastpath", fast.phase == CeremonyPhase::ReadyHold);
    // skip：<30% 拒绝；≥30% 接受并跳过 UI（后台照常）
    let low = ceremony_reducer(ceremony_reducer(INITIAL_CEREMONY, CeremonyEvent::Progress(299)), CeremonyEvent::Skip);
    g(cs, "novaboot-skip-refused", low.phase == CeremonyPhase::Entering && !low.skipped);
    let hi = ceremony_reducer(ceremony_reducer(INITIAL_CEREMONY, CeremonyEvent::Progress(300)), CeremonyEvent::Skip);
    g(cs, "novaboot-skip-accepted", hi.phase == CeremonyPhase::Exiting && hi.skipped);
    let done_skip = ceremony_reducer(ceremony_reducer(hi, CeremonyEvent::ExitDone), CeremonyEvent::Skip);
    g(cs, "novaboot-skip-done-stable", done_skip.phase == CeremonyPhase::Done);

    // 节奏档位
    let c = pacing_timings(BootPacing::Cinematic);
    let b = pacing_timings(BootPacing::Brisk);
    let i = pacing_timings(BootPacing::Instant);
    g(cs, "novaboot-pacing", c.enter_ms == 900 && c.ready_hold_ms == 1200 && b.enter_ms == 500 && b.ready_hold_ms == 400 && i.enter_ms == 0 && i.ready_hold_ms == 0);

    // 字标：8 字母 × 12.5% 区间
    g(cs, "novaboot-wordmark-8", WORDMARK_LETTERS == 8 && LETTER_OWN_MILLI == 125);
    g(cs, "novaboot-letter-0", letter_progress_milli(500, 0) == 1000 && letter_progress_milli(500, 3) == 1000);
    // progress=50%：字母 0-3 满（4×125=500），字母 4 起 0
    g(cs, "novaboot-letter-half", letter_progress_milli(500, 3) == 1000 && letter_progress_milli(500, 4) == 0);
    // 区间内：progress=56.25% → 字母 4 半亮（450*8-500*... ）= 500*... 直接算：v=562*8=4496, seg=4*1000=4000, d=496
    g(cs, "novaboot-letter-partial", letter_progress_milli(562, 4) == 496);
    g(cs, "novaboot-stroke", WORDMARK_STROKE_DPX10 == 15 && WORDMARK_CASCADE_MS == 40);

    // 胶囊进度条规格
    g(cs, "novaboot-capsule", SHEEN_WIDTH_PERMILLE == 250 && SHEEN_PERIOD_MS == 2400 && LEAD_DOT_DP == 7 && LEAD_DOT_ALPHA_PERMILLE == 800 && FULL_BREATH_MS == 300 && FULL_BREATH_BRIGHTNESS_PERMILLE == 1080);

    // 4K 换算
    g(cs, "novaboot-4k", lead_dot_px(2000) == 14 && wordmark_stroke_px(2000) == 3);
}

// ===========================================================================
// 7. 单元测试（宿主机 std 下运行）
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceremony_matches_ts_reducer() {
        // 全链路 + 各守卫分支
        let s = INITIAL_CEREMONY;
        let s = ceremony_reducer(s, CeremonyEvent::Progress(999));
        assert_eq!(s.progress_milli, 999);
        let s = ceremony_reducer(s, CeremonyEvent::Progress(999)); // 无变化返回原状
        assert_eq!(s.progress_milli, 999);
        let s = ceremony_reducer(s, CeremonyEvent::EnterDone);
        assert_eq!(s.phase, CeremonyPhase::Streaming);
        let s = ceremony_reducer(s, CeremonyEvent::Ready);
        assert_eq!(s.phase, CeremonyPhase::ReadyHold);
        let s = ceremony_reducer(s, CeremonyEvent::Skip); // readyHold 也允许 skip
        assert!(s.skipped);
        let s = ceremony_reducer(s, CeremonyEvent::ExitDone);
        assert_eq!(s.phase, CeremonyPhase::Done);
    }

    #[test]
    fn letter_progress_boundaries() {
        assert_eq!(letter_progress_milli(0, 0), 0);
        assert_eq!(letter_progress_milli(1000, 7), 1000);
        assert_eq!(letter_progress_milli(125, 0), 1000);
        assert_eq!(letter_progress_milli(124, 0), 992);
        assert_eq!(letter_progress_milli(500, 4), 0);
        assert_eq!(letter_progress_milli(562, 4), 496);
    }

    #[test]
    fn pacing_matches_src() {
        assert_eq!(pacing_timings(BootPacing::Cinematic).enter_ms, 900);
        assert_eq!(pacing_timings(BootPacing::Cinematic).ready_hold_ms, 1200);
        assert_eq!(pacing_timings(BootPacing::Brisk), PacingTimings { enter_ms: 500, ready_hold_ms: 400 });
        assert_eq!(pacing_timings(BootPacing::Instant), PacingTimings { enter_ms: 0, ready_hold_ms: 0 });
    }

    #[test]
    fn boot_visuals_4k() {
        assert_eq!(lead_dot_px(2000), 14);
        assert_eq!(lead_dot_px(1000), 7);
        assert_eq!(SHEEN_PERIOD_MS, 2400);
    }
}
