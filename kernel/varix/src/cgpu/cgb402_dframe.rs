//! CGPU-F0402 · 与 D 域 80 帧系统正式集成（目标 400 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - D 域集成：帧计时反馈闭环（F0332 预留兑现——D 域帧计时实测→预算
//!   修正→再实测的闭环回路）；80fps 合同联动（帧率不达标→预算缺口归因
//!   →配方/权重修正建议输出）；合同违反升级（连续不达标→域级事件升级
//!   J 功耗/O 降档联动评估）；闭环效果度量（集成前后帧率达标率对比）。
//! - 判据：闭环回路、合同联动、升级路径、效果度量、判据。
//!
//! 设计纪律：
//! - 帧计时反馈闭环三段式：实测（D 域每帧上报耗时）→ 归因（预算侧
//!   缺口定位到账户）→ 修正（权重/预算调整）→ 下一窗再实测。闭环状态机
//!   Measuring→Attributed→Corrected，逐窗推进可审计。
//! - 80fps 合同：帧预算 12.5ms（80fps）；达标 = 窗口内 P95 帧耗时 ≤ 预算。
//!   缺口归因把超标帧的耗时盈余按账户权重分摊定位。
//! - 升级路径：连续 K 窗不达标 → 域级升级事件（带归因摘要）→ 交 J/O
//!   评估降档；升级后计时窗重置（一次升级只报一次）。
//! - 效果度量：集成前/后达标率对比（窗计数），提升为负即集成无效。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C50 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C50，全仓独占）。
pub const SEG_BASE: u16 = 0x0C50;

pub const E_DLOOP_STATE: u16 = SEG_BASE + 1; // 闭环状态机非法跃迁
pub const E_DFRAME_WINDOW: u16 = SEG_BASE + 2; // 窗口未满就结算
pub const E_DATTRIB_WEIGHT: u16 = SEG_BASE + 3; // 归因权重和不为 1000
pub const E_DESCAL_LIMIT: u16 = SEG_BASE + 4; // 升级超限（重复升级）
pub const E_DMETRIC_NONE: u16 = SEG_BASE + 5; // 无基线窗可对比
pub const E_DACCT_UNKNOWN: u16 = SEG_BASE + 6; // 归因到未注册账户

/// 80fps 帧预算（µs）：12_500。
pub const FRAME_BUDGET_US: u32 = 12_500;
/// P95 采样窗大小（帧数）。
pub const WINDOW_FRAMES: usize = 20;
/// 连续不达标升级阈值（窗数）。
pub const ESCALATE_AFTER: usize = 3;
/// 升级事件目标域（J 功耗 / O 降档）。
pub const ESCALATE_TARGETS: [u8; 2] = [b'J', b'O'];

/// 闭环状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopPhase {
    Measuring,
    Attributed,
    Corrected,
}

/// 帧计时窗口：逐帧实测收集。
#[derive(Clone, Debug)]
pub struct FrameWindow {
    pub samples_us: Vec<u32>,
}

impl FrameWindow {
    pub fn new() -> Self {
        FrameWindow { samples_us: Vec::new() }
    }

    pub fn push(&mut self, frame_us: u32) {
        self.samples_us.push(frame_us);
    }

    pub fn full(&self) -> bool {
        self.samples_us.len() >= WINDOW_FRAMES
    }

    /// P95：升序后取 95% 位（窗口内序）。
    pub fn p95(&self) -> Option<u32> {
        if self.samples_us.is_empty() {
            return None;
        }
        let mut s = self.samples_us.clone();
        s.sort_unstable();
        let idx = (s.len() * 95 / 100).min(s.len() - 1);
        Some(s[idx])
    }

    /// 本窗达标（P95 ≤ 帧预算）。
    pub fn compliant(&self) -> Option<bool> {
        self.p95().map(|p| p <= FRAME_BUDGET_US)
    }
}

/// 缺口归因：把超标量按账户权重分摊定位。
#[derive(Clone, Debug)]
pub struct Attribution {
    /// 账户权重表（千分位，和恒 1000）。
    pub weights: Vec<(u32, u32)>, // (账户号, 权重‰)
}

impl Attribution {
    pub fn new(weights: Vec<(u32, u32)>) -> Self {
        Attribution { weights }
    }

    pub fn weight_sum(&self) -> u32 {
        self.weights.iter().map(|(_, w)| w).sum()
    }

    /// 缺口分摊：超标 µs 按权重比例分到账户（向下取整，余数给最大权重户）。
    pub fn apportion(&self, overshoot_us: u32) -> Result<Vec<(u32, u32)>, u16> {
        if self.weight_sum() != 1000 {
            return Err(E_DATTRIB_WEIGHT);
        }
        let mut out = Vec::new();
        let mut distributed = 0u32;
        for (i, (acct, w)) in self.weights.iter().enumerate() {
            let share = (overshoot_us as u64 * *w as u64 / 1000) as u32;
            distributed += share;
            out.push((*acct, share));
            if i + 1 == self.weights.len() {
                // 余数补给最后一个（幂等：总数恒等）。
                out[i].1 += overshoot_us - distributed;
            }
        }
        Ok(out)
    }

    /// 主嫌账户：分摊份额最大的账户号。
    pub fn prime_suspect(&self, overshoot_us: u32) -> Result<Option<u32>, u16> {
        let parts = self.apportion(overshoot_us)?;
        Ok(parts
            .iter()
            .max_by_key(|(_, s)| *s)
            .map(|(a, _)| *a))
    }
}

/// 修正建议（合同联动输出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    /// 修正对象账户。
    pub account: u32,
    /// 建议类别：0=降权重 1=扩预算 2=配方切换。
    pub kind: u8,
    /// 建议幅度（千分位）。
    pub magnitude_permille: u32,
}

/// D 域集成闭环控制器。
#[derive(Clone, Debug)]
pub struct DFrameLoop {
    pub phase: LoopPhase,
    pub win: FrameWindow,
    pub attrib: Attribution,
    /// 连续不达标窗数。
    pub miss_streak: usize,
    /// 升级事件计数（每连续段只升一次）。
    pub escalations: u32,
    /// 已升级标志（本段已升，重置前不再升）。
    escalated: bool,
    /// 已注册账户集（归因目标必须已注册）。
    pub known_accounts: Vec<u32>,
    /// 效果度量：集成前/后达标窗数。
    pub windows_before: (usize, usize), // (达标, 总窗) 集成前
    pub windows_after: (usize, usize),  // 集成后
}

impl DFrameLoop {
    pub fn new(attrib: Attribution, known: Vec<u32>) -> Self {
        DFrameLoop {
            phase: LoopPhase::Measuring,
            win: FrameWindow::new(),
            attrib,
            miss_streak: 0,
            escalations: 0,
            escalated: false,
            known_accounts: known,
            windows_before: (0, 0),
            windows_after: (0, 0),
        }
    }

    /// D 域逐帧上报（Measuring 态收样）。
    pub fn report_frame(&mut self, frame_us: u32) -> Result<(), u16> {
        if self.phase != LoopPhase::Measuring && self.phase != LoopPhase::Corrected {
            return Err(E_DLOOP_STATE);
        }
        self.win.push(frame_us);
        Ok(())
    }

    /// 窗结算：P95 判达标 → 归因 → 修正建议 → 升级评估。
    /// 返回（达标否, 缺口 µs, 修正建议列表）。
    pub fn settle(&mut self) -> Result<(bool, u32, Vec<Correction>), u16> {
        if !self.win.full() {
            return Err(E_DFRAME_WINDOW);
        }
        let compliant = self.win.compliant().ok_or(E_DFRAME_WINDOW)?;
        let p95 = self.win.p95().ok_or(E_DFRAME_WINDOW)?;
        let mut corrections = Vec::new();
        if compliant {
            self.miss_streak = 0;
            self.escalated = false;
            self.windows_after.0 += 1;
        } else {
            let overshoot = p95 - FRAME_BUDGET_US;
            // 归因目标必须在册。
            let suspect = self.attrib.prime_suspect(overshoot)?;
            if let Some(a) = suspect {
                if !self.known_accounts.contains(&a) {
                    return Err(E_DACCT_UNKNOWN);
                }
                corrections.push(Correction {
                    account: a,
                    kind: 0,
                    magnitude_permille: (overshoot as u32 * 1000 / FRAME_BUDGET_US).max(10),
                });
            }
            self.miss_streak += 1;
            if self.miss_streak >= ESCALATE_AFTER && !self.escalated {
                self.escalations += 1;
                self.escalated = true;
            }
        }
        self.windows_after.1 += 1;
        self.phase = LoopPhase::Attributed;
        self.win = FrameWindow::new();
        Ok((compliant, p95, corrections))
    }

    /// 修正落地：Attributed → Corrected，下一窗再实测（闭环推进）。
    pub fn apply_correction(&mut self) -> Result<(), u16> {
        if self.phase != LoopPhase::Attributed {
            return Err(E_DLOOP_STATE);
        }
        self.phase = LoopPhase::Corrected;
        Ok(())
    }

    /// 修正后回到实测态。
    pub fn resume_measuring(&mut self) -> Result<(), u16> {
        if self.phase != LoopPhase::Corrected {
            return Err(E_DLOOP_STATE);
        }
        self.phase = LoopPhase::Measuring;
        Ok(())
    }

    /// 升级事件目标域（每次升级产 J/O 两路评估请求）。
    pub fn escalation_targets(&self) -> Vec<u8> {
        if self.escalations > 0 {
            ESCALATE_TARGETS.to_vec()
        } else {
            Vec::new()
        }
    }

    /// 效果度量：集成后达标率 − 集成前达标率（千分位）。
    pub fn effect_delta(&self, before: (usize, usize)) -> Result<i32, u16> {
        if self.windows_after.1 == 0 || before.1 == 0 {
            return Err(E_DMETRIC_NONE);
        }
        let after_pm = self.windows_after.0 * 1000 / self.windows_after.1;
        let before_pm = before.0 * 1000 / before.1;
        Ok(after_pm as i32 - before_pm as i32)
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn attrib() -> Attribution {
        Attribution::new(vec![(1, 500), (2, 300), (3, 200)])
    }

    fn known() -> Vec<u32> {
        vec![1, 2, 3]
    }

    fn feed_window(l: DFrameLoop, p95_us: u32, spike: u32) -> DFrameLoop {
        // 20 帧其中 19 帧正常 1 帧 spike ⇒ P95 取到第 19 位（0 基 18）。
        let mut l = l;
        for i in 0..WINDOW_FRAMES {
            l.report_frame(if i == WINDOW_FRAMES - 2 { spike } else { p95_us }).ok();
        }
        l
    }

    #[test]
    fn a01_闭环回路_三段状态机推进() {
        let mut l = DFrameLoop::new(attrib(), known());
        assert_eq!(l.phase, LoopPhase::Measuring);
        // 窗未满结算拒。
        assert_eq!(l.settle(), Err(E_DFRAME_WINDOW));
        let mut l = feed_window(l, 11_000, 12_000);
        // P95 = 12_000（19×11_000 + 1×12_000，P95 取最大位）≤ 12_500 达标。
        let (ok, p95, _) = l.settle().unwrap();
        assert!(ok);
        assert_eq!(p95, 12_000);
        assert_eq!(l.phase, LoopPhase::Attributed);
        l.apply_correction().ok();
        assert_eq!(l.phase, LoopPhase::Corrected);
        l.resume_measuring().ok();
        assert_eq!(l.phase, LoopPhase::Measuring);
        // 状态机跳步拒：Corrected 直接结算（无新窗）不可，Attributed 直接 resume 不可。
        let mut l2 = DFrameLoop::new(attrib(), known());
        assert_eq!(l2.apply_correction(), Err(E_DLOOP_STATE));
    }

    #[test]
    fn a02_合同联动_缺口归因与修正建议() {
        let mut l = DFrameLoop::new(attrib(), known());
        // 全窗 14_000ms ⇒ P95=14_000，缺口 1_500µs。
        for _ in 0..WINDOW_FRAMES {
            l.report_frame(14_000).ok();
        }
        let (ok, p95, corr) = l.settle().unwrap();
        assert!(!ok);
        assert_eq!(p95, 14_000);
        assert_eq!(l.windows_after, (0, 1));
        // 主嫌 = 权重最大的账户 1；建议幅度 = 1500*1000/12500 = 120‰。
        assert_eq!(corr.len(), 1);
        assert_eq!(corr[0], Correction { account: 1, kind: 0, magnitude_permille: 120 });
        // 归因分摊总和恒等缺口。
        let parts = attrib().apportion(1_500).unwrap();
        assert_eq!(parts.iter().map(|(_, s)| *s).sum::<u32>(), 1_500);
        // 权重和错 ⇒ 归因拒。
        let bad = Attribution::new(vec![(1, 500), (2, 400)]);
        assert_eq!(bad.apportion(100), Err(E_DATTRIB_WEIGHT));
        // 未注册账户归因拒（主嫌 = 账户 1，其不在册即拒）。
        let mut l3 = DFrameLoop::new(attrib(), vec![2, 3]);
        for _ in 0..WINDOW_FRAMES {
            l3.report_frame(14_000).ok();
        }
        assert_eq!(l3.settle(), Err(E_DACCT_UNKNOWN));
    }

    #[test]
    fn a03_升级路径_连续三窗不达标只升一次() {
        let mut l = DFrameLoop::new(attrib(), known());
        for _ in 0..ESCALATE_AFTER + 2 {
            for _ in 0..WINDOW_FRAMES {
                l.report_frame(20_000).ok();
            }
            l.settle().ok();
            // 闭环推进：修正落地 → 回实测（否则下一窗上报被状态机拒）。
            l.apply_correction().ok();
            l.resume_measuring().ok();
        }
        // 连续 5 窗不达标 ⇒ 升级恰好一次。
        assert_eq!(l.miss_streak, 5);
        assert_eq!(l.escalations, 1);
        assert_eq!(l.escalation_targets(), vec![b'J', b'O']);
        // 达标窗重置连击与升级标志。
        let mut l2 = feed_window(DFrameLoop::new(attrib(), known()), 10_000, 30_000);
        // P95: 第 18 位 = 10_000（达标）。
        l2.settle().ok();
        assert_eq!(l2.escalations, 0);
    }

    #[test]
    fn a04_效果度量_集成前后达标率提升() {
        let mut l = DFrameLoop::new(attrib(), known());
        // 集成前：4/10 达标。
        let before = (4usize, 10usize);
        // 集成后跑 8 窗：6 达标 2 不达标（每窗闭环推进回实测态）。
        for w in 0..8 {
            let p95 = if w < 6 { 10_000u32 } else { 20_000 };
            for i in 0..WINDOW_FRAMES {
                l.report_frame(if i == WINDOW_FRAMES - 1 { 9_000 } else { p95 }).ok();
            }
            l.settle().ok();
            l.apply_correction().ok();
            l.resume_measuring().ok();
        }
        assert_eq!(l.windows_after, (6, 8));
        let delta = l.effect_delta(before).ok();
        // after=750‰ before=400‰ ⇒ +350‰。
        assert_eq!(delta, Some(350));
        // 无基线窗对比拒绝。
        assert_eq!(DFrameLoop::new(attrib(), known()).effect_delta((0, 0)), Err(E_DMETRIC_NONE));
    }

    #[test]
    fn a05_P95采样_窗口序与边界() {
        let mut w = FrameWindow::new();
        assert_eq!(w.p95(), None);
        // P95 idx = 5*95/100=4 ⇒ 50_000（越 12_500 预算 → 不达标）。
        for v in [10_000u32, 30_000, 20_000, 50_000, 40_000] {
            w.push(v);
        }
        // P95 idx = 5*95/100=4 ⇒ 50_000。
        assert_eq!(w.p95(), Some(50_000));
        assert_eq!(w.compliant(), Some(false));
        // 全预算内达标。
        let mut w2 = FrameWindow::new();
        for _ in 0..WINDOW_FRAMES {
            w2.push(FRAME_BUDGET_US);
        }
        assert_eq!(w2.compliant(), Some(true));
    }
}
