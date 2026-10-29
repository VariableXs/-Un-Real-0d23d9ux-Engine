//! CGPU-F0740 · 网页性能影响可见（目标 410 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0740，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：网页性能影响可见——把"改动前/改动后"的指标对
//!    （帧率/帧预算/内存/延迟）转成用户可感的可见性判定：
//!    变化率千分比、感知阈值、回归/改善标记与可见性报告。
//!
//! 设计纪律：
//! - 指标四维闭集：Fps/FrameBudget/Mem/Latency；
//  方向语义写死：Fps/FrameBudget 越大越好（More），
//  Mem/Latency 越小越好（Less）。
// - 变化率：delta_pm = (after-before)*1000/before（有向），
//  before=0 拒绝。
// - 感知阈值（千分比，规格默认档）：Fps 100 / FrameBudget 150 /
//  Mem 80 / Latency 120——低于阈值判"不可感"。
// - 可见性三态闭集：Better/Worse/Imperceptible。
// - 影响账守恒：assessed == better + worse + imperceptible。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4ac0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4ac0，批内 CGPU-F0740 独占）。
pub const SEG_BASE: u16 = 0x4ac0;

/// 基值为 0（变化率不可定义）。
pub const E_ZERO_BASE: u16 = SEG_BASE + 1;
/// 指标序越界。
pub const E_METRIC: u16 = SEG_BASE + 2;
/// 影响账已满。
pub const E_FULL: u16 = SEG_BASE + 3;
/// 影响账守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 报告无对象。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// 变化率参数非法。
pub const E_DELTA: u16 = SEG_BASE + 6;

/// 影响账容量。
pub const IMPACT_CAP: usize = 32;

/// 感知阈值（千分比，按指标序：Fps/FrameBudget/Mem/Latency）。
pub const FEEL_THRESH: [u32; 4] = [100, 150, 80, 120];

/// 指标方向（闭集两向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sense {
    /// 越大越好。
    More,
    /// 越小越好。
    Less,
}

/// 指标（闭集四维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    /// 帧率。
    Fps,
    /// 帧预算余量。
    FrameBudget,
    /// 内存占用。
    Mem,
    /// 输入延迟。
    Latency,
}

impl Metric {
    /// 序。
    pub fn ord(self) -> usize {
        match self {
            Metric::Fps => 0,
            Metric::FrameBudget => 1,
            Metric::Mem => 2,
            Metric::Latency => 3,
        }
    }
    /// 方向。
    pub fn sense(self) -> Sense {
        match self {
            Metric::Fps | Metric::FrameBudget => Sense::More,
            Metric::Mem | Metric::Latency => Sense::Less,
        }
    }
    /// 感知阈值（千分比）。
    pub fn thresh_pm(self) -> u32 {
        FEEL_THRESH[self.ord()]
    }
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            Metric::Fps => "fps",
            Metric::FrameBudget => "budget",
            Metric::Mem => "mem",
            Metric::Latency => "latency",
        }
    }
}

/// 有向变化率：delta_pm = (after-before)*1000/before。
/// 返回有符号千分比（i32）。
pub fn delta_pm(before: u64, after: u64) -> Result<i32, u16> {
    if before == 0 {
        return Err(E_ZERO_BASE);
    }
    let d = after as i128 - before as i128;
    let pm = d * 1000 / before as i128;
    if pm > i32::MAX as i128 || pm < i32::MIN as i128 {
        return Err(E_DELTA);
    }
    Ok(pm as i32)
}

/// 可见性三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visible {
    /// 改善（可感正向）。
    Better,
    /// 退化（可感负向）。
    Worse,
    /// 不可感。
    Imperceptible,
}

/// 可见性判定：方向语义 + 感知阈值。
pub fn visibility(m: Metric, before: u64, after: u64) -> Result<Visible, u16> {
    let d = delta_pm(before, after)?;
    let th = m.thresh_pm() as i32;
    match m.sense() {
        Sense::More => {
            if d >= th {
                Ok(Visible::Better)
            } else if d <= -th {
                Ok(Visible::Worse)
            } else {
                Ok(Visible::Imperceptible)
            }
        }
        Sense::Less => {
            if d <= -th {
                Ok(Visible::Better)
            } else if d >= th {
                Ok(Visible::Worse)
            } else {
                Ok(Visible::Imperceptible)
            }
        }
    }
}

/// 单条影响记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpactRec {
    /// 指标。
    pub metric: Metric,
    /// 改动前。
    pub before: u64,
    /// 改动后。
    pub after: u64,
    /// 判定。
    pub vis: Visible,
}

/// 影响账。
#[derive(Clone, Debug, Default)]
pub struct ImpactBook {
    recs: Vec<ImpactRec>,
    better: u32,
    worse: u32,
    imperceptible: u32,
}

impl ImpactBook {
    pub fn new() -> Self {
        Self {
            recs: Vec::new(),
            better: 0,
            worse: 0,
            imperceptible: 0,
        }
    }
    /// 记一条影响（账满拒绝）。
    pub fn assess(&mut self, m: Metric, before: u64, after: u64) -> Result<Visible, u16> {
        if self.recs.len() >= IMPACT_CAP {
            return Err(E_FULL);
        }
        let vis = visibility(m, before, after)?;
        match vis {
            Visible::Better => self.better += 1,
            Visible::Worse => self.worse += 1,
            Visible::Imperceptible => self.imperceptible += 1,
        }
        self.recs.push(ImpactRec {
            metric: m,
            before,
            after,
            vis,
        });
        Ok(vis)
    }
    /// 守恒：assessed == better + worse + imperceptible。
    pub fn audit(&self) -> Result<(), u16> {
        let sum = self.better as u64 + self.worse as u64 + self.imperceptible as u64;
        if sum != self.recs.len() as u64 {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }
    /// 三态计数。
    pub fn counts(&self) -> (u32, u32, u32) {
        (self.better, self.worse, self.imperceptible)
    }
    /// 记录视图。
    pub fn view(&self) -> &[ImpactRec] {
        &self.recs
    }
    /// 综合判定：有 Worse → Blocked；否则有 Better → Improved；
    /// 全不可感 → Neutral。
    pub fn verdict(&self) -> Result<&'static str, u16> {
        if self.recs.is_empty() {
            return Err(E_EMPTY);
        }
        if self.worse > 0 {
            Ok("blocked")
        } else if self.better > 0 {
            Ok("improved")
        } else {
            Ok("neutral")
        }
    }
}

/// 报告行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisRow {
    /// 指标名。
    pub metric: &'static str,
    /// 有向变化率（千分比）。
    pub dpm: i32,
    /// 感知阈值。
    pub thresh: u32,
    /// 判定。
    pub vis: Visible,
}

/// 报告化（按记录序，复算变化率保证一致）。
pub fn report(book: &ImpactBook) -> Result<Vec<VisRow>, u16> {
    book.audit()?;
    if book.len() == 0 {
        return Err(E_EMPTY);
    }
    let mut out = Vec::with_capacity(book.len());
    for r in book.view() {
        out.push(VisRow {
            metric: r.metric.name(),
            dpm: delta_pm(r.before, r.after)?,
            thresh: r.metric.thresh_pm(),
            vis: r.vis,
        });
    }
    Ok(out)
}

/// 快捷对比：四维一键评估（before/after 各四元组）。
pub fn quick_assess(
    before: [u64; 4],
    after: [u64; 4],
) -> Result<([Visible; 4], &'static str), u16> {
    let mut book = ImpactBook::new();
    let metrics = [Metric::Fps, Metric::FrameBudget, Metric::Mem, Metric::Latency];
    let mut vis = [Visible::Imperceptible; 4];
    for i in 0..4 {
        vis[i] = book.assess(metrics[i], before[i], after[i])?;
    }
    let v = book.verdict()?;
    Ok((vis, v))
}

/// 自例程：主路径（改善例/退化例/不可感例→账守恒→报告）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut book = ImpactBook::new();
    // 主路径 0：帧率 60→66（+100‰ 恰达阈值）→ Better。
    if book.assess(Metric::Fps, 60, 66)? != Visible::Better {
        return Err(E_LEDGER);
    }
    // 主路径 1：延迟 200→170（-150‰，Less 向负即改善）→ Better。
    if book.assess(Metric::Latency, 200, 170)? != Visible::Better {
        return Err(E_LEDGER);
    }
    // 主路径 2：内存 1000→1050（+50‰ < 80 阈值）→ Imperceptible。
    if book.assess(Metric::Mem, 1000, 1050)? != Visible::Imperceptible {
        return Err(E_LEDGER);
    }
    // 主路径 3：帧预算 500→410（-90‰ < 150）→ Imperceptible。
    if book.assess(Metric::FrameBudget, 500, 410)? != Visible::Imperceptible {
        return Err(E_LEDGER);
    }
    book.audit()?;
    let (b, w, i) = book.counts();
    if (b, w, i) != (2, 0, 2) {
        return Err(E_LEDGER);
    }
    // 综合判定：无 Worse 有 Better → improved。
    if book.verdict()? != "improved" {
        return Err(E_LEDGER);
    }
    // 主路径 4：退化例（新账）：Fps 60→50（-166‰）→ Worse。
    let mut book2 = ImpactBook::new();
    if book2.assess(Metric::Fps, 60, 50)? != Visible::Worse {
        return Err(E_LEDGER);
    }
    if book2.verdict()? != "blocked" {
        return Err(E_LEDGER);
    }
    // 主路径 5：全不可感 → neutral。
    let mut book3 = ImpactBook::new();
    book3.assess(Metric::Mem, 1000, 1010)?;
    if book3.verdict()? != "neutral" {
        return Err(E_LEDGER);
    }
    // 一键评估：混合例（fps 略降不可感，延迟大降改善）→ improved。
    let (vis, v) = quick_assess([60, 500, 1000, 200], [59, 495, 1000, 140])?;
    if vis[3] != Visible::Better || v != "improved" {
        return Err(E_LEDGER);
    }
    // 变化率：+50‰ 与 -50‰。
    if delta_pm(1000, 1050)? != 50 || delta_pm(1000, 950)? != -50 {
        return Err(E_DELTA);
    }
    // 错误路径 0：基值 0。
    if delta_pm(0, 10) != Err(E_ZERO_BASE)
        || visibility(Metric::Fps, 0, 10) != Err(E_ZERO_BASE)
    {
        return Err(E_ZERO_BASE);
    }
    // 错误路径 1：变化率溢出防护（u64::MAX → 2 倍级变化仍可表达；
    // 构造超 i32 的 pm：before=1, after=u64::MAX）。
    if delta_pm(1, u64::MAX) != Err(E_DELTA) {
        return Err(E_DELTA);
    }
    // 错误路径 2：空报告。
    let b0 = ImpactBook::new();
    if report(&b0) != Err(E_EMPTY) || b0.verdict() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 3：账满（灌 32 条后拒绝）。
    let mut full = ImpactBook::new();
    for _ in 0..IMPACT_CAP {
        full.assess(Metric::Mem, 1000, 1000)?;
    }
    if full.assess(Metric::Mem, 1000, 1000) != Err(E_FULL) {
        return Err(E_FULL);
    }
    // 主路径 6：报告一致性。
    let rows = report(&book)?;
    if rows.len() != 4 || rows[0].dpm != 100 || rows[0].vis != Visible::Better {
        return Err(E_LEDGER);
    }
    // 感知阈值闭集：100/150/80/120。
    if FEEL_THRESH != [100, 150, 80, 120] {
        return Err(E_DELTA);
    }
    Ok(0)
}
