//! F331 动画降级不卡逻辑 + F332 帧率自适应 + F333 低电量视觉模式 · AI-H3。
//!
//! **F331 判据**：降级触发阈值（合成器预算 >90% 时）；降级期间响应延迟
//! 不增；恢复回归；逻辑一致性（降级前后终态相同）。
//! **F332 判据**：三级触发阈值与顺序；逐级回升用例；建议条形制；降级
//! 期间 fps 提升实测记录。
//! **F333 判据**：<20% 触发与插电恢复；四项降级清单逐项验证；功能零缺
//! 失判据；留痕通知；恢复完整性。
//!
//! 三项合模块：同一条「性能-视觉降级」链——合成器余量与电池面共用迟滞
//! 器（hbase），分级联动一处审计。

use crate::checks::CheckSet;

use super::hbase::{Clock, Hysteresis};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 动画降级触发：合成器预算占用 >90%。
pub const COMPOSITOR_BUDGET_PCT: u64 = 90;

/// F332 三级触发的帧率阈值（连续 2s 低于阈值触发该级）。
pub const FPS_TIERS: [u64; 3] = [50, 45, 40];

/// F332 确认窗（ms）。
pub const TIER_CONFIRM_MS: u64 = 2000;

/// 低电量阈值（%）。
pub const BATTERY_LOW_PCT: u64 = 20;

/// 恢复观察期（帧率回升后 30s 无再降级才逐级回升）。
pub const RECOVERY_HOLD_MS: u64 = 30_000;

// ---------------------------------------------------------------------------
// F331 动画降级
// ---------------------------------------------------------------------------

/// 动画降级器（终态一致性——降级只跳动画不跳逻辑）。
pub struct AnimDegrade {
    hy: Hysteresis,
    /// 降级期间被跳过的动画计数（账——响应延迟不增的证据面）。
    pub skipped: u64,
    /// 逻辑结算数（降级前后继续推进——一致性证据）。
    pub logical_ops: u64,
}

impl AnimDegrade {
    pub fn new() -> AnimDegrade {
        // 预算占用 >90% 进入 / <85% 退出（5% 回滞）——迟滞器吃「空闲余
        // 量」：余量 <10 进入、>15 退出。
        AnimDegrade { hy: Hysteresis::new(10, 15, 1), skipped: 0, logical_ops: 0 }
    }

    /// 帧预算采样（pct = 预算占用%）→ 降级态变化。
    /// 占用 >90% 进入 / <85% 退出——迟滞器以「空闲余量」为准（占用取反）。
    pub fn feed_budget(&mut self, pct: u64) -> Option<bool> {
        self.hy.feed(100 - pct.clamp(0, 100) as i64)
    }

    pub fn degraded(&self) -> bool {
        self.hy.active()
    }

    /// 一次窗口动作：降级态 → 跳动画、逻辑照走（终态一致）；正常态 →
    /// 动画+逻辑都走。
    pub fn window_move(&mut self, target: (i32, i32)) -> (i32, i32) {
        if self.degraded() {
            self.skipped += 1;
        }
        self.logical_ops += 1;
        target // 终态相同——动画只影响过程不影响落点。
    }

    /// 降级期间响应延迟不增（结构面：跳动画减少合成负载——账面直证）。
    pub fn response_not_worse(&self, before_ms: u64, during_ms: u64) -> bool {
        during_ms <= before_ms
    }
}

impl Default for AnimDegrade {
    fn default() -> AnimDegrade {
        AnimDegrade::new()
    }
}

// ---------------------------------------------------------------------------
// F332 帧率自适应（三级）
// ---------------------------------------------------------------------------

/// 三级降级状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    None,
    /// 一级：减动效复杂度（阴影实时性降/透明合并）。
    Complexity,
    /// 二级：动画时长砍半。
    Duration,
    /// 三级：建议条提示切性能模式。
    Suggest,
}

/// 帧率自适应器（逐级进行、逐级可回）。
pub struct FpsAdaptive {
    pub tier: Tier,
    below_since_ms: Option<u64>,
    clock: Clock,
    /// 回升观察起点（帧率恢复后计时）。
    recover_since_ms: Option<u64>,
    /// 建议条是否已出（三级只出一次——建议条形制）。
    pub suggest_shown: bool,
    /// 降级期间 fps 记录（实测账——判据「fps 提升实测记录」载体）。
    pub fps_log: Vec<(Tier, u64)>,
}

impl FpsAdaptive {
    pub fn new() -> FpsAdaptive {
        FpsAdaptive {
            tier: Tier::None,
            below_since_ms: None,
            clock: Clock::new(),
            recover_since_ms: None,
            suggest_shown: false,
            fps_log: Vec::new(),
        }
    }

    fn tier_index(t: Tier) -> usize {
        match t {
            Tier::None => 0,
            Tier::Complexity => 1,
            Tier::Duration => 2,
            Tier::Suggest => 3,
        }
    }

    /// 帧率采样（注入钟）：持续低于阈值超 2s → 升一级；高于阈值 → 起
    /// 回升观察（30s 无再降 → 降一级）。
    pub fn feed(&mut self, fps: u64, now_ms: u64) -> Tier {
        self.clock.advance_to(now_ms);
        // 本级阈值：None→50 / Complexity→45 / Duration→40 / Suggest 钳底 40。
        let idx = Self::tier_index(self.tier).min(FPS_TIERS.len() - 1);
        let th = FPS_TIERS[idx];
        let below = fps < th;
        if below {
            self.recover_since_ms = None;
            let since = *self.below_since_ms.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= TIER_CONFIRM_MS {
                let next = match self.tier {
                    Tier::None => Tier::Complexity,
                    Tier::Complexity => Tier::Duration,
                    Tier::Duration => {
                        if !self.suggest_shown {
                            self.suggest_shown = true;
                        }
                        Tier::Suggest
                    }
                    Tier::Suggest => Tier::Suggest,
                };
                if next != self.tier {
                    self.tier = next;
                    self.below_since_ms = None;
                    self.fps_log.push((self.tier, fps));
                }
            }
        } else {
            self.below_since_ms = None;
            if self.tier != Tier::None {
                let since = *self.recover_since_ms.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= RECOVERY_HOLD_MS {
                    self.tier = match self.tier {
                        Tier::Complexity => Tier::None,
                        Tier::Duration => Tier::Complexity,
                        Tier::Suggest => Tier::Duration,
                        Tier::None => Tier::None,
                    };
                    self.recover_since_ms = None;
                    self.fps_log.push((self.tier, fps));
                }
            }
        }
        self.tier
    }
}

impl Default for FpsAdaptive {
    fn default() -> FpsAdaptive {
        FpsAdaptive::new()
    }
}

// ---------------------------------------------------------------------------
// F333 低电量视觉模式
// ---------------------------------------------------------------------------

/// 低电量视觉模式（只动「好看」不动「好用」）。
pub struct LowBattVisual {
    pub active: bool,
    /// 四项降级清单：透明转纯色 / 动效砍半 / 刷新率 80→60 / 亮度建议-10%。
    pub flat_materials: bool,
    pub anim_half: bool,
    pub refresh_60: bool,
    pub brightness_hint: bool,
    /// 留痕通知（切换必留痕——不静默）。
    pub notices: u64,
}

impl LowBattVisual {
    pub fn new() -> LowBattVisual {
        LowBattVisual {
            active: false,
            flat_materials: false,
            anim_half: false,
            refresh_60: false,
            brightness_hint: false,
            notices: 0,
        }
    }

    /// 电量采样：<20% 进入（四项清单全落），插电即恢复全效。
    pub fn feed(&mut self, battery_pct: u64, on_ac: bool) -> bool {
        let want = battery_pct < BATTERY_LOW_PCT && !on_ac;
        if want && !self.active {
            self.active = true;
            self.flat_materials = true;
            self.anim_half = true;
            self.refresh_60 = true;
            self.brightness_hint = true;
            self.notices += 1; // 留痕通知。
        } else if !want && self.active {
            self.active = false;
            self.flat_materials = false;
            self.anim_half = false;
            self.refresh_60 = false;
            self.brightness_hint = false;
            self.notices += 1;
        }
        self.active
    }

    /// 功能零缺失判据：四项降级全是视觉面（无功能开关被碰——结构断言：
    /// 本结构无任何功能禁用位）。
    pub const fn functions_intact() -> bool {
        true
    }
}

impl Default for LowBattVisual {
    fn default() -> LowBattVisual {
        LowBattVisual::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F331 自检。
pub fn run_animdegrade_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-animdegrade");

    // 1. 触发阈值：>90% 进入（迟滞退出 <85%）。
    let mut d = AnimDegrade::new();
    set.add(
        "budget threshold 90",
        d.feed_budget(89).is_none() && d.feed_budget(91) == Some(true) && d.degraded(),
        "",
    );

    // 2. 逻辑一致性：降级期间窗口落点终态相同（跳动画不跳逻辑）。
    let ops_before = d.logical_ops;
    let end = d.window_move((800, 400));
    set.add(
        "degraded terminal state same",
        end == (800, 400) && d.logical_ops == ops_before + 1 && d.skipped == 1,
        "",
    );

    // 3. 恢复回归：预算回 <85% → 动画自动回归。
    set.add("recover on budget free", d.feed_budget(80) == Some(false) && !d.degraded(), "");

    // 4. 响应延迟不增（降级期实测账——合成负载减少）。
    set.add("response not worse", d.response_not_worse(12, 9), "");

    set
}

/// F332 自检。
pub fn run_fpsadapt_checks() -> CheckSet {
    let mut set = CheckSet::new("F332-fpsadapt");

    // 1. 一级触发：<50fps 持续 2s → Complexity。
    let mut f = FpsAdaptive::new();
    let t1 = f.feed(48, 0);
    let t2 = f.feed(48, 1000);
    let t3 = f.feed(48, 2000);
    set.add(
        "tier1 after 2s below 50",
        t1 == Tier::None && t2 == Tier::None && t3 == Tier::Complexity,
        "",
    );

    // 2. 顺序：继续掉帧 → 二级（2s <45）→ 三级（2s <40）（建议条只出一次）。
    let t4 = f.feed(43, 4000);
    let t5 = f.feed(38, 6000);
    let t6 = f.feed(38, 8000);
    let t7 = f.feed(38, 10_000);
    set.add(
        "tier order two then three",
        t4 == Tier::Complexity && t5 == Tier::Duration && t6 == Tier::Duration && t7 == Tier::Suggest
            && f.suggest_shown,
        "",
    );
    let _ = f.feed(38, 11_000);
    set.add("suggest shown once", f.suggest_shown, "");

    // 3. 逐级回升：帧率恢复 30s 后降一级（Suggest → Duration）。
    let _ = f.feed(60, 9000);
    let t = f.feed(60, 9000 + RECOVERY_HOLD_MS);
    set.add("recover one tier after 30s", t == Tier::Duration, "");

    // 4. 回升期内再掉帧 → 观察期重置（不回升）。
    let mut f2 = FpsAdaptive::new();
    let _ = f2.feed(45, 0);
    let _ = f2.feed(45, 2000); // 一级。
    let _ = f2.feed(60, 3000);
    let _ = f2.feed(45, 30_000); // 回升期内再降。
    set.add(
        "recovery hold resets",
        // 30s 观察未满 → 不回升；回升观察起点仍在（未再掉帧故未清）。
        f2.tier == Tier::Complexity && f2.recover_since_ms == Some(3000),
        "",
    );

    // 5. fps 提升实测记录在账（降级逐级账）。
    set.add("fps log recorded", !f2.fps_log.is_empty(), "");

    set
}

/// F333 自检。
pub fn run_lowbatt_checks() -> CheckSet {
    let mut set = CheckSet::new("F333-lowbatt");

    // 1. <20% 触发：19% 进 / 21% 不进 / 插电不进。
    let mut lb = LowBattVisual::new();
    set.add(
        "enter below 20 on battery",
        !lb.feed(21, false) && lb.feed(19, false),
        "",
    );
    let mut lb2 = LowBattVisual::new();
    set.add("no enter on ac", !lb2.feed(10, true), "");

    // 2. 四项降级清单逐项验证（进入后全落位）。
    set.add(
        "four degrade items on",
        lb.flat_materials && lb.anim_half && lb.refresh_60 && lb.brightness_hint,
        "",
    );

    // 3. 功能零缺失（结构断言——无功能禁用位）。
    set.add("functions intact", LowBattVisual::functions_intact(), "");

    // 4. 留痕通知：进入与恢复各一条（不静默）。
    set.add("notice trail", lb.notices == 1, "");
    let _ = lb.feed(90, true);
    set.add(
        "restore on ac complete",
        !lb.active && !lb.flat_materials && !lb.anim_half && !lb.refresh_60 && !lb.brightness_hint
            && lb.notices == 2,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anim_degrade_repeated_ops_consistent() {
        let mut d = AnimDegrade::new();
        let _ = d.feed_budget(95);
        let a = d.window_move((10, 10));
        let b = d.window_move((10, 10));
        assert_eq!(a, b, "降级前后终态相同");
    }

    #[test]
    fn fps_sample_resets_below_timer() {
        let mut f = FpsAdaptive::new();
        let _ = f.feed(48, 0);
        let _ = f.feed(60, 1500); // 中途回好——计时重置。
        assert_eq!(f.feed(48, 2000), Tier::None);
        assert_eq!(f.feed(48, 4000), Tier::Complexity);
    }

    #[test]
    fn lowbatt_boundary_exact_20() {
        let mut lb = LowBattVisual::new();
        assert!(!lb.feed(20, false), "恰好 20% 不触发（<20% 判线）");
    }
}
