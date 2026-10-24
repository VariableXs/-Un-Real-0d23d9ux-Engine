//! cga57_perfregress —— CGPU-F0057 性能回归守卫扩展。
//! 锚点（CGPU 施工书）：CGPU-F0057 · 性能回归守卫扩展 · 目标 320 行
//! 判据摘录：基准样本预算账（步数预算 + 拒收越线）；回归判定
//!   三档闭集（绿 ≤ 基线 × 1.05 / 黄 ≤ × 1.20 / 红 > × 1.20）；
//!   基线漂移双确认（连续两轮红才立案——单轮抖动不裁决）；
//!   巡检记录追加单调（时间戳严格递增，乱序拒绝）；预算余量
//!   不得为负（扣减下溢显性拒绝）。
//! 组件：样本账本（used/预算）+ 回归三档判定器 + 双确认状态机
//!   （绿/黄/红 × 待确认态）+ 巡检账（append-only 严格递增）+
//!   基线快照对拍核。
//! 职责定位：A 域 SIMD 库的性能守卫——「优化不能倒退」不是口号
//!   而是账面纪律：每算子一条基线，超阈自动立案，双确认防抖。
//! 性能逐项分解：判定 O(1)；账本追加 O(1)；巡检重放 O(账长)。
//! 全整数确定性（步数计数制，不用计时——计时受调度噪声污染）。
//! 诊断码段：0xE5E0（0xE5E1~0xE5E9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PR_VERSION: &str = "cga57_perfregress-v1";

/// 诊断段基址（0xE5E0 段 · 本模块独占）。
pub const PR_SEG_BASE: u16 = 0xE5E0;

/// XcCode 闭集（0xE5E1~0xE5E9）。
pub const XC_BUDGET_OOR: u32 = 0xE5E1;
pub const XC_UNDERFLOW: u32 = 0xE5E2;
pub const XC_TS_NONINC: u32 = 0xE5E3;
pub const XC_BASE_ZERO: u32 = 0xE5E4;
pub const XC_LEDGER_OPEN: u32 = 0xE5E5;
pub const XC_SNAPSHOT_DRIFT: u32 = 0xE5E6;
pub const XC_RATE_BAD: u32 = 0xE5E7;
pub const XC_STATE_BAD: u32 = 0xE5E8;
pub const XC_TICK_OOB: u32 = 0xE5E9;

/// 回归三档闭集。
pub const VERDICT_GREEN: u8 = 0;
pub const VERDICT_YELLOW: u8 = 1;
pub const VERDICT_RED: u8 = 2;

/// 回归判定：ratio = 样本步数 × 1000 / 基线步数（千分比）。
/// 绿 ≤ 1050 / 黄 ≤ 1200 / 红 > 1200（闭集边界冻结）。
pub fn verdict(sample_steps: u64, baseline_steps: u64) -> Result<u8, u32> {
    if baseline_steps == 0 {
        return Err(XC_BASE_ZERO);
    }
    let ratio = sample_steps.saturating_mul(1000) / baseline_steps;
    if ratio <= 1050 {
        Ok(VERDICT_GREEN)
    } else if ratio <= 1200 {
        Ok(VERDICT_YELLOW)
    } else {
        Ok(VERDICT_RED)
    }
}

/// 双确认状态机：单轮红 → 「待确认」；再红 → 立案；其间任一
/// 绿/黄 → 复位。抖动不裁决，连续两红才立案。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    Normal,
    Suspect,
    Filed,
}

pub struct DoubleConfirm {
    pub state: GuardState,
}

impl DoubleConfirm {
    pub fn new() -> DoubleConfirm {
        DoubleConfirm { state: GuardState::Normal }
    }

    /// 喂一轮判定，返回是否立案（沿迁移）。
    pub fn feed(&mut self, v: u8) -> Result<bool, u32> {
        if v > VERDICT_RED {
            return Err(XC_STATE_BAD);
        }
        match self.state {
            GuardState::Normal => {
                if v == VERDICT_RED {
                    self.state = GuardState::Suspect;
                }
                Ok(false)
            }
            GuardState::Suspect => {
                if v == VERDICT_RED {
                    self.state = GuardState::Filed;
                    Ok(true)
                } else {
                    self.state = GuardState::Normal;
                    Ok(false)
                }
            }
            GuardState::Filed => Ok(false),
        }
    }

    pub fn filed(&self) -> bool {
        self.state == GuardState::Filed
    }
}

/// 样本账本：步数预算 + 扣减（下溢显性拒绝——预算余量不得为负）。
pub struct StepBudget {
    pub total: u64,
    pub used: u64,
}

impl StepBudget {
    pub fn new(total: u64) -> Result<StepBudget, u32> {
        if total == 0 || total > 1 << 40 {
            return Err(XC_BUDGET_OOR);
        }
        Ok(StepBudget { total, used: 0 })
    }

    pub fn charge(&mut self, steps: u64) -> Result<(), u32> {
        if self.used + steps > self.total {
            return Err(XC_UNDERFLOW);
        }
        self.used += steps;
        Ok(())
    }

    pub fn remaining(&self) -> u64 {
        self.total - self.used
    }
}

/// 巡检账：append-only，时间戳（tick）严格递增。
pub struct PatrolLedger {
    entries: Vec<(u64, u8)>, // (tick, verdict)
    closed: bool,
}

impl PatrolLedger {
    pub fn new() -> PatrolLedger {
        PatrolLedger { entries: Vec::new(), closed: false }
    }

    pub fn append(&mut self, tick: u64, v: u8) -> Result<(), u32> {
        if self.closed {
            return Err(XC_LEDGER_OPEN);
        }
        if v > VERDICT_RED {
            return Err(XC_STATE_BAD);
        }
        if let Some(&(last, _)) = self.entries.last() {
            if tick <= last {
                return Err(XC_TS_NONINC);
            }
        }
        self.entries.push((tick, v));
        Ok(())
    }

    /// 冻结账（冻结后拒绝再写——审计只读）。
    pub fn freeze(&mut self) {
        self.closed = true;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 重放：连续双红检出（账面立案数 == 双确认重放结果）。
    pub fn replay_filings(&self) -> Result<usize, u32> {
        let mut g = DoubleConfirm::new();
        let mut n = 0usize;
        for &(_, v) in self.entries.iter() {
            if g.feed(v)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 快照对拍：账首尾摘要在冻结后不得变化。
    pub fn snapshot_check(&self, head: u64, tail_tick: u64) -> Result<(), u32> {
        match (self.entries.first(), self.entries.last()) {
            (Some(&(h, _)), Some(&(t, _))) => {
                if h != head || t != tail_tick {
                    return Err(XC_SNAPSHOT_DRIFT);
                }
                Ok(())
            }
            _ => Err(XC_LEDGER_OPEN),
        }
    }
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga57_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0057");
    // 族一：三档判定边界（闭集边界冻结：1050/1200 千分比）。
    cs.add("绿界内", matches!(verdict(1050, 1000), Ok(VERDICT_GREEN)), "1050‰ 恰绿");
    cs.add("黄界内", matches!(verdict(1200, 1000), Ok(VERDICT_YELLOW)), "1200‰ 恰黄");
    cs.add("红界外", matches!(verdict(1201, 1000), Ok(VERDICT_RED)), "1201‰ 即红");
    cs.add("零基线拒", verdict(100, 0).is_err(), "BASE_ZERO");
    // 族二：双确认状态机（抖动不裁决）。
    let mut g = DoubleConfirm::new();
    let _ = g.feed(VERDICT_RED).map_err(|e| e as u16);
    cs.add("单红待确认", !g.filed(), "Suspect 态");
    let _ = g.feed(VERDICT_GREEN).map_err(|e| e as u16);
    cs.add("绿复位", !g.filed() && g.state == GuardState::Normal, "抖动复位");
    let _ = g.feed(VERDICT_RED).map_err(|e| e as u16);
    let filed = g.feed(VERDICT_RED).map_err(|e| e as u16);
    cs.add("双红立案", matches!(filed, Ok(true)), "连续两红");
    cs.add("立案后保持", g.filed(), "Filed 终态");
    // 族三：预算账下溢防线。
    cs.add("预算建账", StepBudget::new(1000).is_ok(), "1000 步预算");
    let mut b = match StepBudget::new(1000) {
        Ok(x) => x,
        Err(_) => return cs,
    };
    let _ = b.charge(600).map_err(|e| e as u16);
    cs.add("扣减 600", b.remaining() == 400, "余 400");
    cs.add("下溢拒", b.charge(500).is_err(), "600+500>1000");
    let _ = b.charge(400).map_err(|e| e as u16);
    cs.add("扣满", b.remaining() == 0, "预算归零");
    cs.add("零预算拒", StepBudget::new(0).is_err(), "BUDGET_OOR");
    // 族四：巡检账严格递增 + 冻结。
    let mut led = PatrolLedger::new();
    let _ = led.append(1, VERDICT_GREEN).map_err(|e| e as u16);
    let _ = led.append(2, VERDICT_RED).map_err(|e| e as u16);
    let _ = led.append(3, VERDICT_RED).map_err(|e| e as u16);
    let _ = led.append(4, VERDICT_GREEN).map_err(|e| e as u16);
    cs.add("账长 4", led.len() == 4, "四条巡检");
    cs.add("乱序拒", led.append(4, VERDICT_GREEN).is_err(), "tick 非严格递增");
    cs.add("回退拒", led.append(3, VERDICT_GREEN).is_err(), "tick 倒退");
    let _ = led.freeze();
    cs.add("冻结拒写", led.append(9, VERDICT_GREEN).is_err(), "LEDGER_OPEN");
    cs.add("快照对拍", led.snapshot_check(1, 4).is_ok(), "首尾锚定");
    cs.add("快照漂移检出", led.snapshot_check(2, 4).is_err(), "SNAPSHOT_DRIFT");
    // 族五：重放一致（账面 1 立案 == 双确认重放）。
    cs.add("重放立案数", matches!(led.replay_filings(), Ok(1)), "tick2-3 连双红");
    // 族六：越界码拒绝。
    cs.add("判定码越界拒", DoubleConfirm::new().feed(9).is_err(), "STATE_BAD");
    cs.add("巡检码越界拒", PatrolLedger::new().append(1, 9).is_err(), "STATE_BAD");
    deepen_perf::merge_into(&mut cs);
    deepen_perf2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga57_perfregress）：真功能面 + 判据并入主核 =====
pub mod deepen_perf {
    use super::*;

    /// 扩展一：滑动基线更新（EWMA 整数化：new = old×7/8 + s/8——
    /// 平滑基线防单点抖动，位移域全整数）。
    pub fn ewma_baseline(old: u64, sample: u64) -> u64 {
        old.saturating_mul(7) / 8 + sample / 8
    }

    /// 扩展二：基线冻结窗（N 轮内不许更新——冻结窗纪律）。
    pub struct FreezeWin {
        pub remain: u64,
    }

    impl FreezeWin {
        pub fn new(n: u64) -> Result<FreezeWin, u32> {
            if n == 0 {
                return Err(XC_BUDGET_OOR);
            }
            Ok(FreezeWin { remain: n })
        }
        pub fn tick(&mut self) -> bool {
            if self.remain > 0 {
                self.remain -= 1;
                true
            } else {
                false
            }
        }
    }

    /// 扩展三：黄档冷却账（黄档连击计数 ≥3 升红——黄档放大器）。
    pub fn yellow_escalate(history: &[u8]) -> bool {
        let mut run = 0usize;
        for &v in history.iter() {
            if v == VERDICT_YELLOW {
                run += 1;
                if run >= 3 {
                    return true;
                }
            } else {
                run = 0;
            }
        }
        false
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("EWMA 平滑", ewma_baseline(1000, 2000) == 1125, "1000×7/8+2000/8");
        cs.add("EWMA 不越", ewma_baseline(u64::MAX, 8) >= u64::MAX - 1, "饱和界");
        let mut f = match FreezeWin::new(3) {
            Ok(x) => x,
            Err(_) => return,
        };
        cs.add("冻结窗三拍", f.tick() && f.tick() && f.tick(), "3 轮内冻结");
        cs.add("冻结窗耗尽", !f.tick(), "窗满放行");
        cs.add("零窗拒", FreezeWin::new(0).is_err(), "BUDGET_OOR");
        cs.add("黄档升级", yellow_escalate(&[VERDICT_YELLOW, VERDICT_YELLOW, VERDICT_YELLOW]), "三连黄升红");
        cs.add("黄档断链", !yellow_escalate(&[VERDICT_YELLOW, VERDICT_GREEN, VERDICT_YELLOW]), "断链重计");
    }

}

/// ===== 深化扩展块 2（cga57_perfregress）=====
pub mod deepen_perf2 {
    use super::*;

    /// 扩展四：算子注册账（基线表——算子名 + 基线步数 + 冻结旗）。
    pub struct Baseline {
        pub entries: Vec<(&'static str, u64, bool)>,
    }

    impl Baseline {
        pub fn new() -> Baseline {
            Baseline { entries: Vec::new() }
        }
        pub fn enroll(&mut self, op: &'static str, steps: u64) -> Result<(), u32> {
            if steps == 0 {
                return Err(XC_BASE_ZERO);
            }
            if self.entries.iter().any(|&(n, _, _)| n == op) {
                return Err(XC_SNAPSHOT_DRIFT);
            }
            self.entries.push((op, steps, false));
            Ok(())
        }
        pub fn freeze(&mut self, op: &'static str) -> Result<(), u32> {
            for e in self.entries.iter_mut() {
                if e.0 == op {
                    e.2 = true;
                    return Ok(());
                }
            }
            Err(XC_BASE_ZERO)
        }
        pub fn frozen(&self, op: &'static str) -> bool {
            self.entries.iter().any(|&(n, _, f)| n == op && f)
        }
    }

    /// 扩展五：趋势账（连续 N 轮同向——恶化趋势检出器）。
    pub fn worsening_trend(samples: &[u64], rounds: usize) -> bool {
        if samples.len() < rounds || rounds < 2 {
            return false;
        }
        let tail = &samples[samples.len() - rounds..];
        for w in tail.windows(2) {
            if w[1] <= w[0] {
                return false;
            }
        }
        true
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut b = Baseline::new();
        cs.add("基线注册", b.enroll("shift4", 1000).is_ok(), "首注");
        cs.add("基线重号拒", b.enroll("shift4", 2000).is_err(), "重号立案");
        cs.add("基线零步拒", b.enroll("ghost", 0).is_err(), "BASE_ZERO");
        let _ = b.freeze("shift4").map_err(|e| e as u16);
        cs.add("冻结旗置位", b.frozen("shift4"), "冻结后只读");
        cs.add("冻结未置位", !b.frozen("ghost"), "未注册非冻结");
        cs.add("恶化趋势", worsening_trend(&[10, 11, 12], 3), "三连升");
        cs.add("平稳非恶化", !worsening_trend(&[10, 10, 10], 3), "等值不算");
    }

}
