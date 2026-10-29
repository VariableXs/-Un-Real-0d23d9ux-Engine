//! CGPU-F0719 · 滚动可感验证（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0719，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动可感验证——从人眼可感角度量化滚动流畅度：
//!   帧间隔账、卡顿（jank）判定、整数抖动（jerk）指标、
//!   流畅度千分比评分与感知档位判定。
//!
//! 设计纪律：
//! - 帧账定容：FRAME_CAP=128 帧窗口，超窗覆盖最旧并计数。
//! - 卡顿闭集判定：间隔 > 2×目标帧时长 或 位移为 0 而命令非 0
//!   → 该帧记卡顿；无第三种口径。
//! - 抖动整数化：jerk[i] = |Δ位移[i] - Δ位移[i-1]|，
//  平滑度评分 = 1000 - min(1000, Σmin(jerk,JERK_CAP)*1000/Σ位移预算)。
//! - 感知档位四档闭集：Buttery(≥950)/Smooth(≥850)/Tolerable(≥700)/
//  Choppy(其余)——阈值写死注释来源（可感验证规格默认档）。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4820 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4820，批内 CGPU-F0719 独占）。
pub const SEG_BASE: u16 = 0x4820;

/// 帧账为空即评分。
pub const E_EMPTY: u16 = SEG_BASE + 1;
/// 帧间隔非法（0 帧时长）。
pub const E_FRAME: u16 = SEG_BASE + 2;
/// 目标帧时长非法（0）。
pub const E_TARGET: u16 = SEG_BASE + 3;
/// 位移与间隔条数不一致。
pub const E_MISMATCH: u16 = SEG_BASE + 4;
/// 感知档位越界引用。
pub const E_TIER: u16 = SEG_BASE + 5;

/// 帧账容量。
pub const FRAME_CAP: usize = 128;
/// 抖动封顶（行/帧²，超出按封顶计）。
pub const JERK_CAP: u32 = 64;
/// 档位阈值：Buttery。
pub const TIER_BUTTERY_PM: u32 = 950;
/// 档位阈值：Smooth。
pub const TIER_SMOOTH_PM: u32 = 850;
/// 档位阈值：Tolerable。
pub const TIER_TOLERABLE_PM: u32 = 700;

/// 单帧观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameObs {
    /// 帧间隔（微秒）。
    pub dt_us: u32,
    /// 本帧滚动位移（行，0 表示没滚）。
    pub disp: u32,
    /// 本帧是否有滚动命令在身（区分"主动停"与"卡死"）。
    pub commanded: bool,
}

/// 感知档位（闭集四档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeelTier {
    /// 丝滑。
    Buttery,
    /// 流畅。
    Smooth,
    /// 可忍受。
    Tolerable,
    /// 卡顿感明显。
    Choppy,
}

impl FeelTier {
    /// 按千分比评分定档。
    pub fn from_score(score_pm: u32) -> Result<Self, u16> {
        if score_pm > 1000 {
            return Err(E_TIER);
        }
        Ok(if score_pm >= TIER_BUTTERY_PM {
            FeelTier::Buttery
        } else if score_pm >= TIER_SMOOTH_PM {
            FeelTier::Smooth
        } else if score_pm >= TIER_TOLERABLE_PM {
            FeelTier::Tolerable
        } else {
            FeelTier::Choppy
        })
    }
    /// 档位序号。
    pub fn ord(self) -> u8 {
        match self {
            FeelTier::Buttery => 0,
            FeelTier::Smooth => 1,
            FeelTier::Tolerable => 2,
            FeelTier::Choppy => 3,
        }
    }
}

/// 帧账（环形）。
#[derive(Clone, Debug)]
pub struct FrameLedger {
    ring: [Option<FrameObs>; FRAME_CAP],
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
}

impl FrameLedger {
    pub fn new() -> Self {
        Self {
            ring: core::array::from_fn(|_| None),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, o: FrameObs) {
        self.ring[self.w] = Some(o);
        self.w = (self.w + 1) % FRAME_CAP;
        if self.held < FRAME_CAP {
            self.held += 1;
        } else {
            self.overwritten += 1;
        }
        self.total += 1;
    }
    /// 守恒：total == held + overwritten。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_FRAME);
        }
        Ok(())
    }
    /// 视图（旧→新）。
    pub fn view(&self) -> Vec<FrameObs> {
        let start = if self.held < FRAME_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % FRAME_CAP;
            if let Some(o) = self.ring[idx] {
                out.push(o);
            }
        }
        out
    }
    pub fn held(&self) -> usize {
        self.held
    }
}

/// 卡顿判定：单帧是否卡顿。
/// 闭集口径：dt > 2×target_us（超时卡）或 (commanded && disp==0)（卡死）。
pub fn is_jank(o: &FrameObs, target_us: u32) -> Result<bool, u16> {
    if target_us == 0 || o.dt_us == 0 {
        return Err(E_TARGET);
    }
    Ok(o.dt_us > target_us.saturating_mul(2) || (o.commanded && o.disp == 0))
}

/// 可感验证报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeelReport {
    /// 观测帧数。
    pub frames: u32,
    /// 卡顿帧数。
    pub janks: u32,
    /// 总抖动（封顶后累计）。
    pub jerk_sum: u64,
    /// 总位移（行）。
    pub disp_sum: u64,
    /// 流畅度评分（千分比）。
    pub score_pm: u32,
    /// 感知档位。
    pub tier: FeelTier,
}

/// 评分口径（显式）：
///   jank 罚分：每卡顿帧罚 1000/frames；
///   jerk 罚分：Σmin(jerk,JERK_CAP)*500/Σ位移（位移为 0 时按全罚）；
///   score = 1000 - jank罚 - jerk罚，下限 0。
pub fn verify(ledger: &FrameLedger, target_us: u32) -> Result<FeelReport, u16> {
    if target_us == 0 {
        return Err(E_TARGET);
    }
    let obs = ledger.view();
    if obs.is_empty() {
        return Err(E_EMPTY);
    }
    let frames = obs.len() as u32;
    // 1) 卡顿计数。
    let mut janks = 0u32;
    for o in &obs {
        if is_jank(o, target_us)? {
            janks += 1;
        }
    }
    // 2) 抖动累计：jerk[i] = |disp[i]-disp[i-1]|（首帧不计）。
    let mut jerk_sum = 0u64;
    let mut disp_sum = 0u64;
    for (i, o) in obs.iter().enumerate() {
        disp_sum += o.disp as u64;
        if i > 0 {
            let prev = obs[i - 1].disp as i64;
            let cur = o.disp as i64;
            let j = (cur - prev).abs();
            jerk_sum += j.min(JERK_CAP as i64) as u64;
        }
    }
    // 3) 罚分。
    let jank_pen = (janks as u64 * 1000) / frames as u64;
    let jerk_pen = if disp_sum == 0 {
        500 // 全程零位移且在观测窗口：按半罚
    } else {
        jerk_sum * 500 / disp_sum
    };
    let score = 1000i64 - jank_pen as i64 - jerk_pen as i64;
    let score = score.clamp(0, 1000) as u32;
    Ok(FeelReport {
        frames,
        janks,
        jerk_sum,
        disp_sum,
        score_pm: score,
        tier: FeelTier::from_score(score)?,
    })
}

/// 双段验证：把帧账切成两半分别评分，两半档位必须一致
/// （可感一致性：半窗不突变）。不一致返回 Err(E_TIER)。
pub fn verify_split_consistent(ledger: &FrameLedger, target_us: u32) -> Result<FeelTier, u16> {
    let obs = ledger.view();
    if obs.len() < 4 {
        return Err(E_EMPTY);
    }
    let half = obs.len() / 2;
    let mut l1 = FrameLedger::new();
    let mut l2 = FrameLedger::new();
    for o in &obs[..half] {
        l1.push(*o);
    }
    for o in &obs[half..] {
        l2.push(*o);
    }
    let r1 = verify(&l1, target_us)?;
    let r2 = verify(&l2, target_us)?;
    if r1.tier.ord() != r2.tier.ord() {
        return Err(E_TIER);
    }
    Ok(r1.tier)
}

/// 自例程：主路径（平滑流 → 高分）+ 卡顿流（低分）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let target = 16_667u32; // 60Hz 帧时长（us）
    // 主路径 1：均匀滚动流（dt=16600，disp=16/帧）。
    let mut l = FrameLedger::new();
    for _ in 0..60 {
        l.push(FrameObs {
            dt_us: 16_600,
            disp: 16,
            commanded: true,
        });
    }
    l.audit()?;
    let r = verify(&l, target)?;
    // 无卡顿；jerk 全 0 → jerk 罚 0；jank 罚 0 → score=1000。
    if r.janks != 0 || r.score_pm != 1000 || r.tier != FeelTier::Buttery {
        return Err(E_EMPTY);
    }
    // 半窗一致性：均匀流应过。
    let t = verify_split_consistent(&l, target)?;
    if t != FeelTier::Buttery {
        return Err(E_TIER);
    }
    // 主路径 2：卡顿流（每 10 帧一次 dt=50000 的超时卡）。
    let mut l2 = FrameLedger::new();
    for i in 0..40u32 {
        let dt = if i % 10 == 5 { 50_000 } else { 16_600 };
        l2.push(FrameObs {
            dt_us: dt,
            disp: 16,
            commanded: true,
        });
    }
    let r2 = verify(&l2, target)?;
    // 4 次卡顿：jank 罚 = 4*1000/40 = 100；jerk：50ms 帧后位移仍 16 → jerk=0。
    if r2.janks != 4 || r2.score_pm != 900 {
        return Err(E_EMPTY);
    }
    if r2.tier != FeelTier::Smooth {
        return Err(E_EMPTY);
    }
    // 主路径 3：卡死流（commanded 但 disp=0）。
    let mut l3 = FrameLedger::new();
    for _ in 0..20 {
        l3.push(FrameObs {
            dt_us: 16_600,
            disp: 0,
            commanded: true,
        });
    }
    let r3 = verify(&l3, target)?;
    // 20 帧全卡：jank 罚 1000；jerk 0（位移恒 0）→ score=0 → Choppy。
    if r3.score_pm != 0 || r3.tier != FeelTier::Choppy {
        return Err(E_EMPTY);
    }
    // 档位边界：900→Smooth，850→Smooth，849→Tolerable，700→Tolerable，699→Choppy。
    if FeelTier::from_score(850)? != FeelTier::Smooth
        || FeelTier::from_score(849)? != FeelTier::Tolerable
        || FeelTier::from_score(700)? != FeelTier::Tolerable
        || FeelTier::from_score(699)? != FeelTier::Choppy
    {
        return Err(E_TIER);
    }
    // 错误路径 1：空账评分。
    let l0 = FrameLedger::new();
    if verify(&l0, target) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 2：目标帧时长 0。
    if verify(&l, 0) != Err(E_TARGET) || is_jank(&FrameObs { dt_us: 1, disp: 1, commanded: false }, 0) != Err(E_TARGET) {
        return Err(E_TARGET);
    }
    // 错误路径 3：帧间隔 0。
    if is_jank(&FrameObs { dt_us: 0, disp: 1, commanded: false }, target) != Err(E_TARGET) {
        return Err(E_TARGET);
    }
    // 错误路径 4：评分越界。
    if FeelTier::from_score(1001) != Err(E_TIER) {
        return Err(E_TIER);
    }
    // 错误路径 5：半窗样本不足。
    let mut l4 = FrameLedger::new();
    for _ in 0..3 {
        l4.push(FrameObs { dt_us: 16_600, disp: 8, commanded: true });
    }
    if verify_split_consistent(&l4, target) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    Ok(0)
}
