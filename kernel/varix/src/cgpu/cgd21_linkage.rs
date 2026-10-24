//! cgd21_linkage — CGPU-F0501 抖动抑制联动（检测触发预算修正→回评→多级手段链）。
//!
//! 判据原文（施工书锚点 CGPU-F0501）：联动：检测触发预算修正（抖动检测→
//! C 域预算修正建议——C06 集成复用）；修正回评（修正后抖动改善度量——闭环
//! 验证）；联动降级（预算修正无效→策略层手段升级——多级手段链）。
//! 判据：触发、回评、多级、判据。
//!
//! 模型：BudgetLink 把抖动检测结果翻译为 C 域预算修正建议（每级幅度有
//! 上限、须留痕）；修正生效后按窗口回评抖动改善量（改善不足判定修正无
//! 效）；无效则沿多级手段链升级（预算→深度→模式→降级呈现），每级升级
//! 须记录触发因与回评证据，禁止跳级。全部判定有界、无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x8D00（cgpu 域内唯一）。
pub const XC_LINK_NO_EVIDENCE: u32 = 0x8D01;
pub const XC_LINK_STEP_OOB: u32 = 0x8D02;
pub const XC_LINK_REVIEW_UNGROUNDED: u32 = 0x8D03;
pub const XC_LINK_SKIP_LEVEL: u32 = 0x8D04;
pub const XC_LINK_CHAIN_EXHAUSTED: u32 = 0x8D05;

/// 手段链五级：预算修正→缓冲加深→策略切换→模式迁移→降级呈现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemedyLevel {
    /// L0 预算修正（C 域帧预算重分配）。
    Budget = 0,
    /// L1 缓冲加深（呈现队列 +1）。
    Depth = 1,
    /// L2 策略切换（呈现策略换挡）。
    Policy = 2,
    /// L3 模式迁移（窗口模式适配）。
    Mode = 3,
    /// L4 降级呈现（效果保全帧率）。
    Degrade = 4,
}

impl RemedyLevel {
    /// 上一级（更轻）。
    pub const fn lighter(self) -> Option<Self> {
        match self {
            RemedyLevel::Budget => None,
            RemedyLevel::Depth => Some(RemedyLevel::Budget),
            RemedyLevel::Policy => Some(RemedyLevel::Depth),
            RemedyLevel::Mode => Some(RemedyLevel::Policy),
            RemedyLevel::Degrade => Some(RemedyLevel::Mode),
        }
    }

    /// 下一级（更重）。
    pub const fn heavier(self) -> Option<Self> {
        match self {
            RemedyLevel::Budget => Some(RemedyLevel::Depth),
            RemedyLevel::Depth => Some(RemedyLevel::Policy),
            RemedyLevel::Policy => Some(RemedyLevel::Mode),
            RemedyLevel::Mode => Some(RemedyLevel::Degrade),
            RemedyLevel::Degrade => None,
        }
    }
}

/// 预算修正建议：幅度以帧预算微秒计，单次上限 500µs。
#[derive(Clone, Copy, Debug)]
pub struct BudgetProposal {
    /// 目标段（0=输入 1=模拟 2=渲染准备 3=提交）。
    pub segment: u8,
    /// 建议增量（µs，可为负）。
    pub delta_us: i32,
}

impl BudgetProposal {
    /// 单次幅度硬上限。
    pub const MAX_DELTA_US: i32 = 500;

    /// 构造并钳制幅度。
    pub fn new(segment: u8, delta_us: i32) -> Result<Self, u32> {
        if segment > 3 {
            return Err(XC_LINK_STEP_OOB);
        }
        if delta_us == 0 {
            return Err(XC_LINK_NO_EVIDENCE);
        }
        Ok(BudgetProposal {
            segment,
            delta_us: delta_us.clamp(-Self::MAX_DELTA_US, Self::MAX_DELTA_US),
        })
    }
}

/// 回评记录：一次修正生效后的窗口度量。
#[derive(Clone, Copy, Debug)]
pub struct ReviewRecord {
    /// 修正前窗口 stddev（µs）。
    pub before_us: u32,
    /// 修正后窗口 stddev（µs）。
    pub after_us: u32,
}

impl ReviewRecord {
    /// 改善率（千分比）：(before-after)/before。before=0 视为已稳。
    pub fn improve_permille(&self) -> u32 {
        if self.before_us == 0 {
            return 1000;
        }
        let drop = self.before_us.saturating_sub(self.after_us);
        (drop as u64 * 1000 / self.before_us as u64) as u32
    }

    /// 回评有效判定：改善 ≥ 150‰ 且不劣化。
    pub fn is_effective(&self) -> bool {
        self.after_us <= self.before_us && self.improve_permille() >= 150
    }
}

/// 升级事件留痕。
#[derive(Clone, Copy, Debug)]
pub struct Escalation {
    /// 手段级。
    pub level: RemedyLevel,
    /// 触发因：回评无效的 before/after。
    pub review: ReviewRecord,
}

/// 联动引擎：检测→建议→回评→升级。
#[derive(Debug)]
pub struct LinkEngine {
    /// 当前手段级。
    level: RemedyLevel,
    /// 待回评的修正（未回评前禁止再修正）。
    pending: Option<BudgetProposal>,
    /// 升级留痕。
    trail: Vec<Escalation>,
    /// 建议计数。
    proposals: u32,
    /// 无效回评计数。
    failed_reviews: u32,
    /// 链耗尽（L4 仍无效）标记。
    exhausted: bool,
}

impl LinkEngine {
    /// 新引擎，从 L0 起。
    pub fn new() -> Self {
        LinkEngine {
            level: RemedyLevel::Budget,
            pending: None,
            trail: Vec::new(),
            proposals: 0,
            failed_reviews: 0,
            exhausted: false,
        }
    }

    /// 当前级。
    pub fn level(&self) -> RemedyLevel {
        self.level
    }

    /// 升级留痕条数。
    pub fn trail_len(&self) -> usize {
        self.trail.len()
    }

    /// 是否已到 L4 且标记耗尽。
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// 检测触发：抖动超标→产出预算修正建议（仅 L0 直出；高级别须先回评）。
    pub fn propose(&mut self, segment: u8, delta_us: i32) -> Result<BudgetProposal, u32> {
        if self.pending.is_some() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        let p = BudgetProposal::new(segment, delta_us)?;
        self.pending = Some(p);
        self.proposals += 1;
        Ok(p)
    }

    /// 回评：修正生效后的窗口度量。有效→留在本级；无效→升级一级。
    pub fn review(&mut self, r: ReviewRecord) -> Result<Option<RemedyLevel>, u32> {
        if self.pending.is_none() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        self.pending = None;
        if r.is_effective() {
            return Ok(None);
        }
        self.failed_reviews += 1;
        // 升级：禁止跳级（一次只升一级），L4 仍无效→耗尽。
        let next = match self.level.heavier() {
            Some(n) => n,
            None => {
                self.exhausted = true;
                return Err(XC_LINK_CHAIN_EXHAUSTED);
            }
        };
        self.trail.push(Escalation {
            level: next,
            review: r,
        });
        self.level = next;
        Ok(Some(next))
    }

    /// 降级回退：上级手段生效后可逐级回落（一次一级，须有留痕）。
    pub fn demote(&mut self) -> Result<RemedyLevel, u32> {
        let prev = match self.level.lighter() {
            Some(p) => p,
            None => return Err(XC_LINK_STEP_OOB),
        };
        if self.trail.is_empty() {
            return Err(XC_LINK_NO_EVIDENCE);
        }
        self.trail.pop();
        self.level = prev;
        Ok(prev)
    }

    /// 留痕对账：每条 Escalation 的级号必须比前一条恰好 +1（防跳级）。
    pub fn trail_is_monotonic(&self) -> bool {
        let mut i = 1;
        while i < self.trail.len() {
            let a = self.trail[i - 1].level as u8;
            let b = self.trail[i].level as u8;
            if b != a + 1 {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for LinkEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkEngine {
    /// 自检一：触发留痕——建议必须留 pending，重复建议被拒。
    pub fn selfcheck_trigger(&mut self) -> Result<(), u32> {
        let p = self.propose(2, 300)?;
        if p.delta_us != 300 || p.segment != 2 {
            return Err(XC_LINK_STEP_OOB);
        }
        // 未回评前再建议→拒。
        if self.propose(1, 100).is_ok() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        // 幅度钳制与非法段。
        if BudgetProposal::new(4, 100).is_err() || BudgetProposal::new(0, 0).is_err() {
            return Err(XC_LINK_NO_EVIDENCE);
        }
        let big = BudgetProposal::new(0, 5000)?;
        if big.delta_us != BudgetProposal::MAX_DELTA_US {
            return Err(XC_LINK_STEP_OOB);
        }
        Ok(())
    }

    /// 自检二：回评闭环——有效回评留级、无效回评升一级、无 pending 拒评。
    pub fn selfcheck_review(&mut self) -> Result<(), u32> {
        let mut e = LinkEngine::new();
        e.propose(1, 200)?;
        // 有效：改善 60%。
        if e.review(ReviewRecord { before_us: 1000, after_us: 400 })?.is_some() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        if e.level() != RemedyLevel::Budget {
            return Err(XC_LINK_STEP_OOB);
        }
        // 无 pending 回评→拒。
        if e.review(ReviewRecord { before_us: 1000, after_us: 100 }).is_ok() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        // 无效：劣化→升 L1。
        e.propose(0, 100)?;
        let up = e.review(ReviewRecord { before_us: 500, after_us: 600 })?;
        if up != Some(RemedyLevel::Depth) || e.level() != RemedyLevel::Depth {
            return Err(XC_LINK_SKIP_LEVEL);
        }
        Ok(())
    }

    /// 自检三：多级链——连续无效逐级升至 L4，跳级即拒，L4 无效→耗尽。
    pub fn selfcheck_chain(&mut self) -> Result<(), u32> {
        let mut e = LinkEngine::new();
        let mut expect = RemedyLevel::Depth;
        let mut i = 0;
        while i < 4 {
            e.propose(3, 400)?;
            let up = e.review(ReviewRecord { before_us: 800, after_us: 800 })?;
            if up != Some(expect) {
                return Err(XC_LINK_SKIP_LEVEL);
            }
            match expect.heavier() {
                Some(n) => expect = n,
                None => break,
            }
            i += 1;
        }
        if e.level() != RemedyLevel::Degrade || !e.trail_is_monotonic() {
            return Err(XC_LINK_STEP_OOB);
        }
        // L4 仍无效→耗尽。
        e.propose(0, 100)?;
        if e.review(ReviewRecord { before_us: 900, after_us: 900 }).is_ok() {
            return Err(XC_LINK_CHAIN_EXHAUSTED);
        }
        if !e.is_exhausted() {
            return Err(XC_LINK_CHAIN_EXHAUSTED);
        }
        Ok(())
    }

    /// 自检四：降级回退——逐级回落且留痕递减、空留痕拒退。
    pub fn selfcheck_demote(&mut self) -> Result<(), u32> {
        let mut e = LinkEngine::new();
        // 无升级先退→拒。
        if e.demote().is_ok() {
            return Err(XC_LINK_NO_EVIDENCE);
        }
        e.propose(1, 100)?;
        let _ = e.review(ReviewRecord { before_us: 700, after_us: 700 })?;
        let back = e.demote()?;
        if back != RemedyLevel::Budget || e.level() != RemedyLevel::Budget {
            return Err(XC_LINK_SKIP_LEVEL);
        }
        if e.demote().is_ok() {
            return Err(XC_LINK_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 自检五：总账与判据收口。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = LinkEngine::new();
        e.propose(2, 250)?;
        if e.review(ReviewRecord { before_us: 1000, after_us: 850 })?.is_some() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        // 改善 149‰（drop=150 → 150‰ 恰好达标；148‰ 不达标边界）。
        let r = ReviewRecord { before_us: 1000, after_us: 852 };
        if r.is_effective() {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        let r2 = ReviewRecord { before_us: 1000, after_us: 850 };
        if !r2.is_effective() || r2.improve_permille() != 150 {
            return Err(XC_LINK_REVIEW_UNGROUNDED);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8D00..0x8E00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_LINK_NO_EVIDENCE,
            XC_LINK_STEP_OOB,
            XC_LINK_REVIEW_UNGROUNDED,
            XC_LINK_SKIP_LEVEL,
            XC_LINK_CHAIN_EXHAUSTED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8D00 || codes[i] >= 0x8E00 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd21_selfchecks() {
        let mut e = LinkEngine::new();
        e.selfcheck_trigger().unwrap();
        e.selfcheck_review().unwrap();
        e.selfcheck_chain().unwrap();
        e.selfcheck_demote().unwrap();
        e.selfcheck_verdict().unwrap();
        assert_eq!(LinkEngine::segment_guard(), Ok(()));
    }
}
