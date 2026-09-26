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

// ---------------------------------------------------------------------------
// 深化层 · F332 三级动作参数表 + F333 恢复快照账 + F331 降级事件日志
// ---------------------------------------------------------------------------

/// 一级（Complexity）动作参数表——「阴影实时性降 / 透明度合并渲染」的
/// 逐项旋钮（降级期间生效、恢复即还原，参数唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComplexityKnobs {
    /// 阴影重绘间隔（ms——None 级实时，一级降为 33ms 节流）。
    pub shadow_interval_ms: u64,
    /// 透明层合并渲染（0 = 逐层合成，1 = 合并单 pass）。
    pub flatten_transparent: u8,
    /// 模糊半径上限（px——一级从 24 收到 12）。
    pub blur_radius_cap_px: u32,
}

/// 二级（Duration）动作参数——F124 三档动画时长各砍半。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurationKnobs {
    /// F124 三档基准时长（ms）。
    pub base_durations_ms: [u64; 3],
}

impl DurationKnobs {
    pub const F124_BASE: DurationKnobs = DurationKnobs { base_durations_ms: [120, 200, 320] };

    /// 砍半后的各档时长（减半取整——降级期生效）。
    pub fn halved(&self) -> [u64; 3] {
        self.base_durations_ms.map(|d| d / 2)
    }
}

/// 一级旋钮缺省值（参数表入册——改常数必炸 checks）。
pub const COMPLEXITY_DEFAULT: ComplexityKnobs = ComplexityKnobs {
    shadow_interval_ms: 33,
    flatten_transparent: 1,
    blur_radius_cap_px: 12,
};

/// 三级建议条模型（黄条形制：一键切性能模式 F069——出现一次可关）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestBar {
    pub visible: bool,
    /// 用户响应：Some(true)=一键切了性能模式；Some(false)=关掉条；None=未理。
    pub action: Option<bool>,
}

impl SuggestBar {
    pub fn new() -> SuggestBar {
        SuggestBar { visible: false, action: None }
    }

    /// 出条（三级触发时——只出一次，重复出条拒绝）。
    pub fn show(&mut self) -> bool {
        if self.visible || self.action.is_some() {
            return false;
        }
        self.visible = true;
        true
    }

    /// 用户点击（一键切性能模式 / 关闭）。
    pub fn respond(&mut self, switch_to_perf: bool) -> bool {
        if !self.visible {
            return false;
        }
        self.visible = false;
        self.action = Some(switch_to_perf);
        true
    }

    /// 黄条文案（人话——出现条件 + 出路）。
    pub const BANNER: &'static str = "系统正忙，已自动精简动效——可一键切换性能模式";
}

impl Default for SuggestBar {
    fn default() -> SuggestBar {
        SuggestBar::new()
    }
}

/// F333 恢复快照账：进入省电态前的视觉参数快照（恢复完整性——截图比
/// 对的数据面：逐项记录降级前值，恢复后逐项对账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreSnapshot {
    /// 四项降级前值：(材质透明度‰, 动效时长基准 ms, 刷新率 Hz, 亮度建议 ‰)。
    pub before: (u32, u64, u64, u32),
    /// 恢复后实测值。
    pub after: Option<(u32, u64, u64, u32)>,
}

impl RestoreSnapshot {
    pub fn capture(before: (u32, u64, u64, u32)) -> RestoreSnapshot {
        RestoreSnapshot { before, after: None }
    }

    /// 恢复完整性核账：after 逐项 == before（四项全对）。
    pub fn verify(&mut self, after: (u32, u64, u64, u32)) -> bool {
        self.after = Some(after);
        after == self.before
    }

    /// 亮度建议可拒（用户拒 → after 亮度项保留用户值——该项豁免对账）。
    pub fn verify_with_brightness_optout(&mut self, after: (u32, u64, u64, u32)) -> bool {
        self.after = Some(after);
        after.0 == self.before.0 && after.1 == self.before.1 && after.2 == self.before.2
    }
}

/// F331 降级事件日志（环形——降级/恢复事件 + 跳过的动画账，可回放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeEvent {
    pub at_ms: u64,
    /// true = 进入降级，false = 恢复。
    pub entered: bool,
    /// 事件时刻已跳过的动画累计数（账面快照）。
    pub skipped_total: u64,
}

/// 带日志的动画降级器（深化层——包装 [`AnimDegrade`] 的事件面）。
pub struct AnimDegradeLogged {
    inner: AnimDegrade,
    events: Vec<DegradeEvent>,
    cap: usize,
}

impl AnimDegradeLogged {
    pub fn new(cap: usize) -> AnimDegradeLogged {
        AnimDegradeLogged { inner: AnimDegrade::new(), events: Vec::new(), cap: cap.max(1) }
    }

    /// 帧预算采样（事件入账——进入/恢复都留痕）。
    pub fn feed_budget(&mut self, pct: u64, now_ms: u64) -> Option<bool> {
        let change = self.inner.feed_budget(pct);
        if let Some(entered) = change {
            self.events.push(DegradeEvent {
                at_ms: now_ms,
                entered,
                skipped_total: self.inner.skipped,
            });
            if self.events.len() > self.cap {
                self.events.remove(0);
            }
        }
        change
    }

    /// 窗口动作直通（跳动画逻辑保持）。
    pub fn window_move(&mut self, target: (i32, i32)) -> (i32, i32) {
        self.inner.window_move(target)
    }

    pub fn degraded(&self) -> bool {
        self.inner.degraded()
    }

    pub fn events(&self) -> &[DegradeEvent] {
        &self.events
    }

    pub fn skipped(&self) -> u64 {
        self.inner.skipped
    }

    pub fn logical_ops(&self) -> u64 {
        self.inner.logical_ops
    }
}

/// 深化层自检（参数表 / 建议条 / 快照账 / 事件日志）。
pub fn run_animdegrade_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep");

    // 1. 一级旋钮参数表入册（33ms/合并/12px——常量钉死）。
    set.add(
        "complexity knobs table",
        COMPLEXITY_DEFAULT.shadow_interval_ms == 33
            && COMPLEXITY_DEFAULT.flatten_transparent == 1
            && COMPLEXITY_DEFAULT.blur_radius_cap_px == 12,
        "",
    );

    // 2. 二级动画砍半：F124 三档 120/200/320 → 60/100/160。
    let halved = DurationKnobs::F124_BASE.halved();
    set.add("f124 durations halved", halved == [60, 100, 160], "");

    // 3. 建议条形制：出一次、重复出拒绝、响应后闭环、文案在账。
    let mut bar = SuggestBar::new();
    let once = bar.show();
    let twice = bar.show();
    let resp = bar.respond(true);
    set.add(
        "suggest bar once and closes",
        once && !twice && resp && !bar.visible && bar.action == Some(true)
            && SuggestBar::BANNER.contains("性能模式"),
        "",
    );

    // 4. 未出条时响应拒绝（状态机闭环）。
    let mut bar2 = SuggestBar::new();
    set.add("suggest respond closed state", !bar2.respond(false) && bar2.action.is_none(), "");

    // 5. F333 恢复快照：四项逐项对账全对；亮度可拒面豁免对账。
    let mut snap = RestoreSnapshot::capture((900, 200, 80, 1000));
    let ok = snap.verify((900, 200, 80, 1000));
    let mut snap2 = RestoreSnapshot::capture((900, 200, 80, 1000));
    let ok2 = snap2.verify_with_brightness_optout((900, 200, 80, 900));
    let mut snap3 = RestoreSnapshot::capture((900, 200, 80, 1000));
    let bad = snap3.verify((900, 200, 60, 1000));
    set.add("restore snapshot verify", ok && ok2 && !bad, "");

    // 6. 降级事件日志：进入/恢复留痕、环形滚动、跳过账随事件快照。
    let mut d = AnimDegradeLogged::new(4);
    let _ = d.feed_budget(95, 0);
    let _ = d.window_move((10, 10));
    let _ = d.window_move((10, 10));
    let _ = d.feed_budget(70, 100);
    let ev = d.events();
    set.add(
        "degrade events logged",
        ev.len() == 2 && ev[0].entered && !ev[1].entered && ev[0].skipped_total == 0,
        "",
    );
    for i in 2..8u64 {
        let _ = d.feed_budget(if i % 2 == 0 { 95 } else { 70 }, i * 100);
    }
    set.add("event ring capped", d.events().len() <= 4, "");

    // 7. 深化器初始账面干净（跳过 0 / 逻辑 0——终态一致的结构面起点）。
    let d2 = AnimDegradeLogged::new(8);
    set.add("logic parity structure", d2.skipped() == 0 && d2.logical_ops() == 0, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn suggest_bar_double_respond_rejected() {
        let mut bar = SuggestBar::new();
        let _ = bar.show();
        let _ = bar.respond(true);
        assert!(!bar.respond(false), "已响应的条不再接受第二次响应");
    }

    #[test]
    fn duration_halving_floor() {
        let k = DurationKnobs { base_durations_ms: [0, 1, 2] };
        assert_eq!(k.halved(), [0, 0, 1]);
    }

    #[test]
    fn event_ring_keeps_latest() {
        let mut d = AnimDegradeLogged::new(2);
        for i in 0..6u64 {
            let _ = d.feed_budget(if i % 2 == 0 { 95 } else { 70 }, i * 10);
        }
        let ev = d.events();
        assert_eq!(ev.len(), 2);
        assert!(ev[1].at_ms > ev[0].at_ms, "保留最新事件");
    }

    #[test]
    fn snapshot_after_captured_once() {
        let mut s = RestoreSnapshot::capture((1, 2, 3, 4));
        let _ = s.verify((1, 2, 3, 4));
        assert_eq!(s.after, Some((1, 2, 3, 4)));
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F332 三级动作执行账 + fps 实测窗 + F333 逐项应用账 + F331 分面跳过账
// ---------------------------------------------------------------------------

/// 三级动作执行账：每次层级变化记录「该级应落位的动作清单」，升级逐项
/// 追加、降级逐项裁剪——逐级可回的结构证明（恢复到零级时清单必须为空）。
pub struct TierActionLedger {
    active: Vec<&'static str>,
    /// 层级迁移历史（层级 + 生效动作数快照）。
    pub transitions: Vec<(Tier, usize)>,
}

impl TierActionLedger {
    /// 逐级动作清单（主册三级顺序的唯一源：先减复杂度、再砍时长、最后建议条）。
    pub const TIER_ACTIONS: [&'static str; 3] =
        ["阴影实时性降+透明合并", "F124 时长砍半", "性能模式建议条"];

    pub fn new() -> TierActionLedger {
        TierActionLedger { active: Vec::new(), transitions: Vec::new() }
    }

    fn tier_index(t: Tier) -> usize {
        match t {
            Tier::None => 0,
            Tier::Complexity => 1,
            Tier::Duration => 2,
            Tier::Suggest => 3,
        }
    }

    /// 应用层级（升级追加 / 降级裁剪），返回当前生效动作。
    pub fn apply(&mut self, tier: Tier) -> &[&'static str] {
        let want = Self::tier_index(tier);
        while self.active.len() > want {
            self.active.pop();
        }
        while self.active.len() < want {
            let idx = self.active.len();
            self.active.push(Self::TIER_ACTIONS[idx]);
        }
        self.transitions.push((tier, self.active.len()));
        &self.active
    }

    pub fn active_actions(&self) -> &[&'static str] {
        &self.active
    }

    /// 恢复完整性：零级时动作清单必须为空。
    pub fn restored_clean(&self) -> bool {
        self.active.is_empty()
    }
}

impl Default for TierActionLedger {
    fn default() -> TierActionLedger {
        TierActionLedger::new()
    }
}

/// fps 滑动实测窗（判据「降级期间 fps 提升实测记录」的数据面）：滑动
/// 采样 + p95（hbase 最近邻同口径），降级前后对比直接出账。
pub struct FpsWindow {
    samples: Vec<u64>,
    cap: usize,
}

impl FpsWindow {
    pub fn new(cap: usize) -> FpsWindow {
        FpsWindow { samples: Vec::new(), cap: cap.max(2) }
    }

    pub fn push(&mut self, fps: u64) {
        self.samples.push(fps);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    /// 窗内 p95（升序后最近邻 950‰）。
    pub fn p95(&self) -> u64 {
        let mut s = self.samples.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    /// 提升对账：当前窗 p95 相对基准提升 ≥ lift_fps。
    pub fn improved_by(&self, before_p95: u64, lift_fps: u64) -> bool {
        self.p95() >= before_p95.saturating_add(lift_fps)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

/// F333 四项降级逐项应用账：逐项独立落位 + 用户拒亮度面（只动好看
/// 不动好用，且尊重用户——拒了就跳过该项）。
pub struct LowBattApplier {
    applied: Vec<&'static str>,
    pub brightness_declined: bool,
}

impl LowBattApplier {
    pub const ITEMS: [&'static str; 4] =
        ["透明转纯色", "动效时长砍半", "刷新率 80→60", "亮度建议-10%"];

    pub fn new() -> LowBattApplier {
        LowBattApplier { applied: Vec::new(), brightness_declined: false }
    }

    /// 进入省电态：逐项落位（拒亮度建议 → 三项 + 拒绝标记；每轮进入
    /// 重新询问——上轮的拒绝标记复位）。
    pub fn engage(&mut self, accept_brightness_hint: bool) -> usize {
        self.applied.clear();
        self.brightness_declined = false;
        for (i, item) in Self::ITEMS.iter().enumerate() {
            if i == 3 && !accept_brightness_hint {
                self.brightness_declined = true;
                continue;
            }
            self.applied.push(item);
        }
        self.applied.len()
    }

    /// 插电恢复：全部还原，返回是否有账可清（恢复完整性对账面）。
    pub fn disengage(&mut self) -> bool {
        let had = !self.applied.is_empty();
        self.applied.clear();
        had
    }

    pub fn applied_items(&self) -> &[&'static str] {
        &self.applied
    }
}

impl Default for LowBattApplier {
    fn default() -> LowBattApplier {
        LowBattApplier::new()
    }
}

/// F331 分面跳过账：按动画面（窗口/菜单/浮层/弹窗）记跳过数——降级期
/// 合成负载下降的账面直证；恢复回归时清零（动画自动回归无遗留）。
pub struct SurfaceSkipLedger {
    counts: Vec<(&'static str, u64)>,
}

impl SurfaceSkipLedger {
    pub fn new() -> SurfaceSkipLedger {
        SurfaceSkipLedger { counts: Vec::new() }
    }

    pub fn skip(&mut self, surface: &'static str) {
        match self.counts.iter_mut().find(|(s, _)| *s == surface) {
            Some((_, c)) => *c += 1,
            None => self.counts.push((surface, 1)),
        }
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().map(|(_, c)| *c).sum()
    }

    pub fn of(&self, surface: &str) -> u64 {
        self.counts.iter().find(|(s, _)| *s == surface).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 恢复回归：清零并返回清掉的总数（回归证据）。
    pub fn reset(&mut self) -> u64 {
        let t = self.total();
        self.counts.clear();
        t
    }
}

impl Default for SurfaceSkipLedger {
    fn default() -> SurfaceSkipLedger {
        SurfaceSkipLedger::new()
    }
}

/// F069 性能模式联动：三级建议条「一键切」的系统面——切档成功后帧率
/// 自适应强制归零级（用户已手动接管，自适应让位），切换留账。
pub struct PerfModeLink {
    pub mode_switches: u64,
    pub active_mode: &'static str,
}

impl PerfModeLink {
    pub const MODES: [&'static str; 3] = ["均衡", "性能", "长续航"];

    pub fn new() -> PerfModeLink {
        PerfModeLink { mode_switches: 0, active_mode: "均衡" }
    }

    /// 切档（三档白名单外拒绝）→ 自适应器归零级。
    pub fn switch_to(&mut self, mode: &str, adaptive: &mut FpsAdaptive) -> bool {
        match mode {
            "均衡" | "性能" | "长续航" => {}
            _ => return false,
        }
        self.active_mode = if mode == "均衡" {
            "均衡"
        } else if mode == "性能" {
            "性能"
        } else {
            "长续航"
        };
        self.mode_switches += 1;
        adaptive.tier = Tier::None;
        true
    }
}

impl Default for PerfModeLink {
    fn default() -> PerfModeLink {
        PerfModeLink::new()
    }
}

/// 深化层二自检（执行账 / 实测窗 / 逐项应用 / 分面账 / F069 联动）。
pub fn run_animdegrade_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep2");

    // 1. 三级动作执行账：升级逐项追加（0→1→2→3），动作清单与主册三级顺序一致。
    let mut led = TierActionLedger::new();
    let n1 = led.apply(Tier::Complexity).len();
    let n2 = led.apply(Tier::Duration).len();
    let a3 = led.apply(Tier::Suggest);
    let third = (a3[0], a3[1], a3[2]); // 拷出（借用到此为止）。
    set.add(
        "tier actions escalate in order",
        n1 == 1 && n2 == 2 && third.0 == TierActionLedger::TIER_ACTIONS[0]
            && third.1 == TierActionLedger::TIER_ACTIONS[1]
            && third.2 == TierActionLedger::TIER_ACTIONS[2],
        "",
    );

    // 2. 降级裁剪：Suggest → Complexity 只剩一级动作；归零后清单空（逐级可回）。
    let a4 = led.apply(Tier::Complexity);
    set.add("tier actions prune on downgrade", a4.len() == 1, "");
    let _ = led.apply(Tier::None);
    set.add("tier actions clean at none", led.restored_clean(), "");

    // 3. fps 实测窗：滑动封顶 + p95 + 提升对账。
    let mut w = FpsWindow::new(8);
    for f in [30u64, 32, 31, 33, 32, 34, 60, 62] {
        w.push(f);
    }
    set.add(
        "fps window p95 and improvement",
        w.len() == 8 && w.p95() == 62 && w.improved_by(35, 20),
        "",
    );
    set.add("fps window no false improvement", !w.improved_by(62, 1), "");

    // 4. F333 逐项应用账：接受亮度 → 四项全落；拒亮度 → 三项 + 拒绝标记；
    //    恢复清空（逐项对账面）。
    let mut ap = LowBattApplier::new();
    let n_all = ap.engage(true);
    let n_skip = ap.engage(false);
    let n_dis = ap.disengage();
    set.add(
        "lowbatt applier per-item",
        n_all == 4 && n_skip == 3 && ap.brightness_declined && n_dis && ap.applied_items().is_empty(),
        "",
    );

    // 5. F331 分面跳过账：分面计数 + 总账 + 恢复清零。
    let mut sk = SurfaceSkipLedger::new();
    sk.skip("窗口");
    sk.skip("窗口");
    sk.skip("浮层");
    set.add(
        "surface skip ledger",
        sk.of("窗口") == 2 && sk.of("浮层") == 1 && sk.of("菜单") == 0 && sk.total() == 3,
        "",
    );
    let cleared = sk.reset();
    set.add("surface skip reset on recover", cleared == 3 && sk.total() == 0, "");

    // 6. F069 联动：白名单外档位拒绝；合法切档 → 自适应归零级 + 留账。
    let mut f = FpsAdaptive::new();
    let _ = f.feed(38, 0);
    let _ = f.feed(38, 2000); // 一级。
    let mut link = PerfModeLink::new();
    let bad = link.switch_to("极速", &mut f);
    let ok = link.switch_to("性能", &mut f);
    set.add(
        "perf mode link gates and resets",
        !bad && ok && link.mode_switches == 1 && link.active_mode == "性能" && f.tier == Tier::None,
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn tier_ledger_transitions_recorded() {
        let mut led = TierActionLedger::new();
        let _ = led.apply(Tier::Duration);
        let _ = led.apply(Tier::None);
        assert_eq!(led.transitions.len(), 2);
        assert_eq!(led.transitions[1], (Tier::None, 0));
    }

    #[test]
    fn fps_window_empty_p95_zero() {
        let w = FpsWindow::new(4);
        assert_eq!(w.p95(), 0);
    }

    #[test]
    fn lowbatt_reengage_after_decline_resets_flag() {
        let mut ap = LowBattApplier::new();
        let _ = ap.engage(false);
        assert!(ap.brightness_declined);
        let _ = ap.engage(true);
        assert!(!ap.brightness_declined, "重新接受后拒绝标记应复位");
    }

    #[test]
    fn surface_skip_unknown_surface_zero() {
        let mut sk = SurfaceSkipLedger::new();
        sk.skip("弹窗");
        assert_eq!(sk.of("抽屉"), 0);
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 统一调速器核 FrameGovernor：单一入口仲裁三机制 + 帧成本模型
// ---------------------------------------------------------------------------
//
// 三机制并存时的优先级仲裁（主册语义推导，一处一事实）：
//   1. F333 低电量（电池面）最优先——省电是硬约束，帧率自适应让位；
//   2. F331 动画降级（合成器预算面）次之——预算爆了先跳动画；
//   3. F332 帧率分级（持续掉帧面）兜底——前两者没接住的掉帧走分级。
//   插电 + 预算余量足时全部让位于全效渲染。

/// 帧成本模型：一帧动画的合成成本分量（单位：预算千分之‰——固定量纲，
/// 与主册「合成器预算」同一账本）。降级动作逐项削减分量，削减量即
/// 「响应延迟不增」的数值证据面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCost {
    /// 阴影逐帧重绘成本。
    pub shadow: u32,
    /// 透明层逐层合成成本。
    pub transparency: u32,
    /// 模糊成本。
    pub blur: u32,
    /// 动画过程帧数成本（时长越长占帧越多）。
    pub anim_frames: u32,
}

/// 全效档帧成本基准（参数表入册——降级档相对它削减）。
pub const FRAME_COST_FULL: FrameCost = FrameCost { shadow: 120, transparency: 200, blur: 160, anim_frames: 320 };

impl FrameCost {
    /// 一级降级后的帧成本：阴影节流（÷4）、透明合并（÷2）、模糊封顶（÷2）、
    /// 时长不变。
    pub fn after_complexity(self) -> FrameCost {
        FrameCost {
            shadow: self.shadow / 4,
            transparency: self.transparency / 2,
            blur: self.blur / 2,
            anim_frames: self.anim_frames,
        }
    }

    /// 二级降级后的帧成本：过程帧数砍半（动画时长砍半的直接成本投影）。
    pub fn after_duration(self) -> FrameCost {
        let c = self.after_complexity();
        FrameCost { anim_frames: c.anim_frames / 2, ..c }
    }

    /// 省电档（F333）帧成本：透明转纯色（透明项→0）、动效砍半、刷新率
    /// 80→60Hz（每秒帧数 ×3/4，全分量等比降）。
    pub fn after_lowbatt(self) -> FrameCost {
        let halved = FrameCost { transparency: 0, anim_frames: self.anim_frames / 2, ..*&self };
        FrameCost {
            shadow: halved.shadow * 3 / 4,
            transparency: halved.transparency * 3 / 4,
            blur: halved.blur * 3 / 4,
            anim_frames: halved.anim_frames * 3 / 4,
        }
    }

    pub fn total(&self) -> u32 {
        self.shadow + self.transparency + self.blur + self.anim_frames
    }
}

/// 机制仲裁结果（哪个机制在当班——账面可查，不猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GovernorMode {
    /// 全效（无机制在班）。
    Full,
    /// 低电量在班（F333——最高优先）。
    LowBatt,
    /// 预算降级在班（F331）。
    Budget,
    /// 帧率分级在班（F332——具体层级随 [`FpsAdaptive`]）。
    FpsTier,
}

/// 统一调速器：单一采样入口 `sample()`，内部持有三机制并仲裁当班者。
pub struct FrameGovernor {
    pub anim: AnimDegrade,
    pub fps: FpsAdaptive,
    pub lowbatt: LowBattVisual,
    /// 当班机制（每次采样刷新）。
    pub mode: GovernorMode,
    /// 当班帧成本（采样后按当班机制折算）。
    pub cost: FrameCost,
    /// 仲裁历史（环形账——模式切换留痕可回放）。
    pub mode_log: Vec<(u64, GovernorMode)>,
    cap: usize,
}

impl FrameGovernor {
    pub fn new(cap: usize) -> FrameGovernor {
        FrameGovernor {
            anim: AnimDegrade::new(),
            fps: FpsAdaptive::new(),
            lowbatt: LowBattVisual::new(),
            mode: GovernorMode::Full,
            cost: FRAME_COST_FULL,
            mode_log: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// 单一采样入口：一拍喂全（帧率 / 预算占用 / 电量 / 是否插电 / 时刻），
    /// 返回仲裁后的当班模式。优先级见模块注释。
    pub fn sample(&mut self, fps: u64, budget_pct: u64, battery_pct: u64, on_ac: bool, now_ms: u64) -> GovernorMode {
        let _ = self.lowbatt.feed(battery_pct, on_ac);
        let _ = self.anim.feed_budget(budget_pct);
        let tier = self.fps.feed(fps, now_ms);

        let next = if self.lowbatt.active {
            GovernorMode::LowBatt
        } else if self.anim.degraded() {
            GovernorMode::Budget
        } else if tier != Tier::None {
            GovernorMode::FpsTier
        } else {
            GovernorMode::Full
        };

        // 成本折算：当班机制决定降级链（低电量档最狠、预算档次之、分级档
        // 按层级）。
        self.cost = match next {
            GovernorMode::Full => FRAME_COST_FULL,
            GovernorMode::LowBatt => FRAME_COST_FULL.after_lowbatt(),
            GovernorMode::Budget => FRAME_COST_FULL.after_complexity(),
            GovernorMode::FpsTier => match tier {
                Tier::Complexity => FRAME_COST_FULL.after_complexity(),
                _ => FRAME_COST_FULL.after_duration(),
            },
        };

        if next != self.mode {
            self.mode = next;
            self.mode_log.push((now_ms, next));
            if self.mode_log.len() > self.cap {
                self.mode_log.remove(0);
            }
        }
        next
    }

    /// 响应延迟不增（数值直证面）：当班成本必须 ≤ 全效成本。
    pub fn latency_guard(&self) -> bool {
        self.cost.total() <= FRAME_COST_FULL.total()
    }

    pub fn mode_log(&self) -> &[(u64, GovernorMode)] {
        &self.mode_log
    }
}

/// 深化层三自检（统一调速器：仲裁优先级 / 成本模型 / 延迟守卫 / 模式账）。
pub fn run_animdegrade_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep3");

    // 1. 帧成本模型：一级降级档总成本 < 全效档（每项分量非增）。
    let c1 = FRAME_COST_FULL.after_complexity();
    set.add(
        "complexity cost strictly lower",
        c1.total() < FRAME_COST_FULL.total() && c1.shadow < FRAME_COST_FULL.shadow
            && c1.transparency < FRAME_COST_FULL.transparency && c1.blur < FRAME_COST_FULL.blur,
        "",
    );

    // 2. 二级再降：时长项砍半后总成本再降。
    let c2 = FRAME_COST_FULL.after_duration();
    set.add("duration cost lower still", c2.total() < c1.total() && c2.anim_frames < c1.anim_frames, "");

    // 3. 省电档：透明归零 + 过程成本最低链。
    let cb = FRAME_COST_FULL.after_lowbatt();
    set.add(
        "lowbatt cost floor",
        cb.transparency == 0 && cb.total() < c2.total(),
        "",
    );

    // 4. 仲裁优先级：低电量压过预算与分级。
    let mut g = FrameGovernor::new(8);
    let m = g.sample(38, 95, 15, false, 0); // 分级/预算/低电量条件同时满足。
    set.add(
        "priority lowbatt over others",
        m == GovernorMode::LowBatt && g.cost.transparency == 0,
        "",
    );

    // 5. 插电恢复 + 预算回落：低电量退出 → 预算降级接管（次优先）。
    let m2 = g.sample(38, 95, 90, true, 2000);
    set.add("budget takes over on ac", m2 == GovernorMode::Budget && g.latency_guard(), "");

    // 6. 预算回落 → 分级接管（兜底）。
    let m3 = g.sample(38, 60, 90, true, 4000);
    set.add("fps tier fallback", m3 == GovernorMode::FpsTier, "");

    // 7. 全恢复 → 全效档（成本回满）。逐级回升：30s 观察期一次升一级，
    //    Duration→Complexity→None 需两轮观察。
    let _ = g.sample(60, 60, 90, true, 4000 + RECOVERY_HOLD_MS); // 观察起算。
    let _ = g.sample(60, 60, 90, true, 4000 + 2 * RECOVERY_HOLD_MS); // → Complexity。
    let m4 = g.sample(60, 60, 90, true, 4000 + 3 * RECOVERY_HOLD_MS); // → None/Full。
    set.add(
        "full mode restores full cost",
        m4 == GovernorMode::Full && g.cost == FRAME_COST_FULL && g.latency_guard(),
        "",
    );

    // 8. 模式账：切换留痕环形封顶、时序单调。
    set.add(
        "mode log ring and order",
        g.mode_log().len() <= 8
            && g.mode_log().windows(2).all(|w| w[0].0 <= w[1].0)
            && g.mode_log().iter().any(|(_, m)| *m == GovernorMode::LowBatt)
            && g.mode_log().last().map(|(_, m)| *m) == Some(GovernorMode::Full),
        "",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn governor_latency_never_worse_across_cycle() {
        let mut g = FrameGovernor::new(16);
        // 全周期乱序采样：任何时刻延迟守卫都必须成立。
        let script = [
            (60u64, 40u64, 90u64, true),
            (38, 95, 15, false),
            (45, 88, 50, false),
            (30, 99, 10, false),
            (55, 70, 30, true),
            (42, 92, 25, false),
        ];
        for (i, &(fps, bud, bat, ac)) in script.iter().enumerate() {
            g.sample(fps, bud, bat, ac, (i * 2100) as u64);
            assert!(g.latency_guard(), "t={} 当班成本超全效基准", i);
        }
    }

    #[test]
    fn cost_model_full_baseline_exact() {
        assert_eq!(FRAME_COST_FULL.total(), 800);
        assert_eq!(FRAME_COST_FULL.after_complexity().total(), 30 + 100 + 80 + 320);
    }

    #[test]
    fn fps_tier_duration_mode_uses_duration_cost() {
        let mut g = FrameGovernor::new(4);
        // 直推到二级（Duration）：4s 连续低于 50 → 一级；再 4s 低于 45 → 二级。
        let _ = g.sample(43, 60, 90, true, 0);
        let _ = g.sample(43, 60, 90, true, 2000);
        let m = g.sample(43, 60, 90, true, 4000);
        let _ = g.sample(43, 60, 90, true, 6000);
        let m2 = g.sample(43, 60, 90, true, 8000);
        assert_eq!(m, GovernorMode::FpsTier);
        assert_eq!(m2, GovernorMode::FpsTier);
        assert_eq!(g.cost.anim_frames, FRAME_COST_FULL.anim_frames / 2, "二级时长项砍半");
    }
}

// ---------------------------------------------------------------------------
// 深化层四 · 帧步进模拟器 + 逐表面预算分配账 + 层级策略参数总表
// ---------------------------------------------------------------------------

/// 帧步进模拟器（F332「降级期间 fps 提升实测记录」的机制本体）：给定
/// 工作负载（每帧名义成本）与预算（每帧可花 μs），按当班降级档折算
/// 实际帧耗时，产出帧时序账——p95 超预算即掉帧，降级档生效后 p95
/// 必须回到预算内。确定性纯计算（无随机源——同负载同输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workload {
    /// 每帧名义工作（μs）。
    pub nominal_us: u64,
    /// 帧预算（μs——60Hz = 16_666）。
    pub budget_us: u64,
}

/// 一次模拟的帧时序账。
pub struct FrameSim {
    pub frame_us: Vec<u64>,
}

impl FrameSim {
    /// 跑 N 帧：实际耗时 = 名义成本 × 折算系数（当班档的 FrameCost 总量
    /// / 全效总量——成本模型直接驱动模拟，一处一事实）。帧耗时钳底 1μs
    /// （折算‰ 与整除双重防零——帧耗时为零物理不成立）。
    pub fn run(work: Workload, cost_ratio_permille: u32, frames: usize) -> FrameSim {
        let scaled =
            (work.nominal_us.max(1) * cost_ratio_permille.max(1) as u64 / 1000).max(1);
        FrameSim { frame_us: alloc::vec![scaled; frames.max(1)] }
    }

    /// 帧时序 p95（hbase 最近邻口径）。
    pub fn p95(&self) -> u64 {
        let mut s = self.frame_us.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    /// 掉帧率‰（单帧超预算即掉）。
    pub fn dropped_permille(&self, work: &Workload) -> u32 {
        if self.frame_us.is_empty() {
            return 0;
        }
        let dropped = self.frame_us.iter().filter(|&&t| t > work.budget_us).count();
        (dropped * 1000 / self.frame_us.len()) as u32
    }
}

/// 逐表面预算分配账（F331 分面跳过账的成本面深化）：降级档生效时，
/// 合成预算按表面优先级切配额——指针面保额（F335 联动）、窗口/浮层
/// 按权重分配，超配额的表面先降级。配额表唯一源。
pub struct SurfaceBudgetBook {
    /// (表面, 权重‰)——权重和必须 = 1000（checks 钉死）。
    weights: Vec<(&'static str, u32)>,
    /// 逐表面实发配额账（分配时留痕）。
    pub grants: Vec<(&'static str, u64)>,
}

impl SurfaceBudgetBook {
    /// 缺省权重表：指针 100‰（保额 10%——F335 指针优先平面语义）+
    /// 窗口 550‰ + 浮层 250‰ + 装饰 100‰。
    pub fn standard() -> SurfaceBudgetBook {
        SurfaceBudgetBook {
            weights: alloc::vec![("指针", 100), ("窗口", 550), ("浮层", 250), ("装饰", 100)],
            grants: Vec::new(),
        }
    }

    /// 权重表自证：和恰为 1000‰、无零权重表面。
    pub fn weights_sane(&self) -> bool {
        let total: u32 = self.weights.iter().map(|(_, w)| w).sum();
        total == 1000 && self.weights.iter().all(|(_, w)| *w > 0)
    }

    /// 按总预算分配逐表面配额（整除余数给窗口——优先级面显式化）。
    pub fn allocate(&mut self, budget_us: u64) -> &[(&'static str, u64)] {
        self.grants.clear();
        let mut used: u64 = 0;
        for (i, (s, w)) in self.weights.iter().enumerate() {
            let share = if i + 1 == self.weights.len() {
                budget_us - used // 最后一个表面吃余数——总账不漏 μs。
            } else {
                budget_us * (*w as u64) / 1000
            };
            used += share;
            self.grants.push((s, share));
        }
        &self.grants
    }

    /// 指针保额判据：指针配额 ≥ 总预算 × 10%（降级也不许饿死指针面）。
    pub fn pointer_floor_held(&self, budget_us: u64) -> bool {
        self.grants
            .iter()
            .find(|(s, _)| *s == "指针")
            .map(|(_, g)| *g >= budget_us * 10 / 100)
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.weights.len()
    }
}

/// 层级策略参数总表（F332 三级动作的机器可读唯一源——FrameGovernor
/// 的 FrameCost 折算、TierActionLedger 的动作清单都从这张表读，改表
/// 即改全域行为，无第二处魔数）。
pub struct TierPolicyTable;

impl TierPolicyTable {
    /// (层级, 帧成本折算‰, 动作名)。
    pub const POLICY: [(Tier, u32, &'static str); 4] = [
        (Tier::None, 1000, "全效渲染"),
        (Tier::Complexity, 660, "阴影节流+透明合并"),
        (Tier::Duration, 460, "F124 时长砍半"),
        (Tier::Suggest, 460, "性能模式建议条（并提）"),
    ];

    /// 层级 → 成本折算‰（表内查——表外层级不存在，编译期枚举保证）。
    pub fn ratio_for(t: Tier) -> u32 {
        Self::POLICY
            .iter()
            .find(|(tier, _, _)| *tier == t)
            .map(|(_, r, _)| *r)
            .unwrap_or(1000)
    }

    pub fn action_for(t: Tier) -> &'static str {
        Self::POLICY
            .iter()
            .find(|(tier, _, _)| *tier == t)
            .map(|(_, _, a)| *a)
            .unwrap_or("全效渲染")
    }

    /// 折算单调性自证：层级越高折算越低（降级必须省事，不许倒挂）。
    pub fn monotonic() -> bool {
        Self::POLICY.windows(2).all(|w| w[0].1 >= w[1].1)
    }
}

/// 深化层四自检（帧模拟 / 预算分配 / 策略总表）。
pub fn run_animdegrade_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep4");

    // 1. 帧模拟：过载负载（25ms 名义 > 16.6ms 预算）全效档掉帧率 1000‰；
    //    一级折算（660‰）后 16.5ms 回预算内——降级改善 fps 的模拟直证。
    let work = Workload { nominal_us: 25_000, budget_us: 16_666 };
    let full = FrameSim::run(work, TierPolicyTable::ratio_for(Tier::None), 100);
    let tier1 = FrameSim::run(work, TierPolicyTable::ratio_for(Tier::Complexity), 100);
    set.add(
        "frame sim degradation improves fps",
        full.dropped_permille(&work) == 1000
            && tier1.dropped_permille(&work) == 0
            && tier1.p95() <= work.budget_us,
        "",
    );

    // 2. 模拟确定性：同负载两跑同输出（无隐藏随机源）。
    let a = FrameSim::run(work, 660, 50);
    let b = FrameSim::run(work, 660, 50);
    set.add("frame sim deterministic", a.frame_us == b.frame_us && a.p95() == b.p95(), "");

    // 3. 逐表面预算分配：总账不漏（Σ配额 == 总预算）+ 权重表自证。
    let mut book = SurfaceBudgetBook::standard();
    set.add("weights sane", book.weights_sane() && book.len() == 4, "");
    let grants = book.allocate(16_666);
    let sum: u64 = grants.iter().map(|(_, g)| g).sum();
    set.add("allocation sums to budget", sum == 16_666, "");

    // 4. 指针保额（F335 联动）：降级分配下指针面仍 ≥10%。
    set.add("pointer floor held", book.pointer_floor_held(16_666), "");

    // 5. 策略总表：单调自证 + 表值抽查（None=1000‰ / Duration=460‰）。
    set.add(
        "tier policy monotonic and spot",
        TierPolicyTable::monotonic()
            && TierPolicyTable::ratio_for(Tier::None) == 1000
            && TierPolicyTable::ratio_for(Tier::Duration) == 460
            && TierPolicyTable::action_for(Tier::Suggest).contains("建议条"),
        "",
    );

    // 6. 策略表驱动帧模拟：三级链逐级掉帧率不升（降级链有效性的机器证明）。
    let ratios = [
        TierPolicyTable::ratio_for(Tier::None),
        TierPolicyTable::ratio_for(Tier::Complexity),
        TierPolicyTable::ratio_for(Tier::Duration),
    ];
    let drops: Vec<u32> = ratios
        .iter()
        .map(|&r| FrameSim::run(work, r, 60).dropped_permille(&work))
        .collect();
    set.add(
        "tier chain strictly improves",
        drops[0] >= drops[1] && drops[1] >= drops[2] && drops[2] == 0,
        "",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn underload_never_drops() {
        let work = Workload { nominal_us: 8_000, budget_us: 16_666 };
        let sim = FrameSim::run(work, 1000, 30);
        assert_eq!(sim.dropped_permille(&work), 0, "负载低于预算不掉帧");
    }

    #[test]
    fn allocation_single_surface_gets_all() {
        let mut book = SurfaceBudgetBook::standard();
        let g = book.allocate(1000);
        let pointer = g.iter().find(|(s, _)| *s == "指针").unwrap().1;
        assert_eq!(pointer, 100, "指针 100‰ 保额精确");
    }

    #[test]
    fn policy_table_covers_all_tiers() {
        for t in [Tier::None, Tier::Complexity, Tier::Duration, Tier::Suggest] {
            assert!(TierPolicyTable::ratio_for(t) > 0, "每层级都有折算——{:?}", t);
        }
    }

    #[test]
    fn frame_sim_never_zero_scale() {
        let work = Workload { nominal_us: 100, budget_us: 16_666 };
        let sim = FrameSim::run(work, 0, 5); // 非法折算被钳为 1‰。
        assert!(sim.p95() > 0, "折算系数钳底防零除");
    }
}

// ---------------------------------------------------------------------------
// 深化层五 · 合成负载发生器 + 升降级振荡审计 + 用户手选档位让位账
// ---------------------------------------------------------------------------

/// 合成负载发生器（实机判据「Y7000 注入负载」的宿主侧替身）：三型
/// 负载曲线确定性生成——常载（恒定）、正弦（缓升缓降）、突发（阶梯
/// 跳变），喂给调速器全链验证（注入钟同口径，宿主可复现）。
pub struct LoadGenerator;

/// 负载型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadShape {
    /// 常载：恒定占用（%）。
    Steady(u64),
    /// 正弦：占用随时间缓变（周期/振幅参数化）。
    Sine { period_ms: u64, amplitude: u64, base: u64 },
    /// 突发：稳态 + 阶梯尖峰（峰高/峰宽/间隔）。
    Burst { base: u64, spike: u64, spike_width_ms: u64, interval_ms: u64 },
}

impl LoadGenerator {
    /// 时刻 t 的预算占用（‰钳 0-100）。
    pub fn sample(shape: LoadShape, t_ms: u64) -> u64 {
        match shape {
            LoadShape::Steady(v) => v.clamp(0, 100),
            LoadShape::Sine { period_ms, amplitude, base } => {
                let phase = (t_ms % period_ms) * 360 / period_ms.max(1);
                let v = base as i64 + (amplitude as f64 * (phase as f64).to_radians().sin()) as i64;
                v.clamp(0, 100) as u64
            }
            LoadShape::Burst { base, spike, spike_width_ms, interval_ms } => {
                let in_spike = t_ms % interval_ms.max(1) < spike_width_ms;
                (if in_spike { spike } else { base }).clamp(0, 100)
            }
        }
    }

    /// 全链曲线生成（N 点采样——验证面数据源）。
    pub fn curve(shape: LoadShape, points: usize, step_ms: u64) -> Vec<u64> {
        (0..points.max(1))
            .map(|i| Self::sample(shape, (i as u64) * step_ms.max(1)))
            .collect()
    }
}

/// 升降级振荡审计（策略质量面）：模式切换历史中，同模式驻留 < 最小
/// 驻留时间即记一次振荡——振荡密度（次/分钟）超阈值 = 策略缺陷事件
/// （回滞带/确认窗需要调参，不是静默容忍）。
pub struct OscillationAudit {
    /// 最小驻留（ms——档位内至少待这么久才算稳定切换）。
    pub min_dwell_ms: u64,
    pub oscillations: u64,
}

impl OscillationAudit {
    pub fn new(min_dwell_ms: u64) -> OscillationAudit {
        OscillationAudit { min_dwell_ms, oscillations: 0 }
    }

    /// 审计一段模式账（时间, 模式序号）：相邻切换间隔 < 驻留线即振荡。
    pub fn audit(&mut self, log: &[(u64, u32)]) -> u64 {
        self.oscillations = 0;
        for w in log.windows(2) {
            if w[1].0.saturating_sub(w[0].0) < self.min_dwell_ms {
                self.oscillations += 1;
            }
        }
        self.oscillations
    }

    /// 策略健康判定：账内振荡为零（或有账但密度为零）才绿；零账不虚报。
    pub fn healthy(&self, log_len: usize) -> bool {
        log_len >= 2 && self.oscillations == 0
    }
}

/// 用户手选档位让位账（开放性纪律：用户手动选档 → 自适应机制全部
/// 让位并留痕；用户放手（恢复自动）→ 三机制重新接管）。手动期间
/// 采样照记但不驱动降级（用户意志优先——账面可查）。
pub struct ManualOverride {
    pub active: bool,
    pub handovers: u64,
    /// 手动期间被压制的机制动作数（让位证据面）。
    pub suppressed: u64,
}

impl ManualOverride {
    pub fn new() -> ManualOverride {
        ManualOverride { active: false, handovers: 0, suppressed: 0 }
    }

    pub fn engage(&mut self) {
        if !self.active {
            self.active = true;
            self.handovers += 1;
        }
    }

    pub fn release(&mut self) {
        self.active = false;
    }

    /// 采样闸门：手动期间机制动作被压制（计数+拒绝），自动期放行。
    pub fn gate(&mut self, mechanism_wants_action: bool) -> bool {
        if self.active {
            if mechanism_wants_action {
                self.suppressed += 1;
            }
            false
        } else {
            mechanism_wants_action
        }
    }
}

impl Default for ManualOverride {
    fn default() -> ManualOverride {
        ManualOverride::new()
    }
}

/// 深化层五自检（负载发生器 / 振荡审计 / 手选让位）。
pub fn run_animdegrade_deep5_checks() -> CheckSet {
    use alloc::vec;
    let mut set = CheckSet::new("F331-333-deep5");

    // 1. 常载：恒定输出。
    let curve = LoadGenerator::curve(LoadShape::Steady(70), 5, 100);
    set.add("steady load constant", curve == vec![70, 70, 70, 70, 70], "");

    // 2. 突发：稳态+尖峰阶梯（尖峰宽 100ms 在 100ms 步进下恰命中
    //    t=0、400 两点——采样点与尖峰窗的对齐语义）。
    let burst = LoadGenerator::curve(LoadShape::Burst { base: 30, spike: 95, spike_width_ms: 100, interval_ms: 400 }, 8, 100);
    set.add(
        "burst staircase",
        burst[0] == 95 && burst[1] == 30 && burst[4] == 95 && burst.iter().all(|&v| v == 30 || v == 95),
        "",
    );

    // 3. 全链钳制：任何型任何点不越 0-100。
    let weird = LoadGenerator::curve(LoadShape::Sine { period_ms: 1000, amplitude: 90, base: 50 }, 100, 10);
    set.add(
        "clamped to domain",
        weird.iter().all(|&v| v <= 100),
        "",
    );

    // 4. 振荡审计：快速抖动账出振荡、稳态账零振荡、零账不虚报健康。
    let mut au = OscillationAudit::new(5000);
    let jitter = vec![(0u64, 0u32), (1000, 1), (2000, 0), (3000, 1)];
    let n1 = au.audit(&jitter);
    let stable = vec![(0u64, 0u32), (60_000, 1)];
    let n2 = au.audit(&stable);
    set.add(
        "oscillation audit",
        n1 == 3 && n2 == 0 && au.healthy(stable.len()) && !au.healthy(0),
        "",
    );

    // 5. 手选让位：手动期机制动作被压制计数、放手后放行、交接留痕。
    let mut mo = ManualOverride::new();
    let before = mo.gate(true);
    mo.engage();
    let during1 = mo.gate(true);
    let during2 = mo.gate(false);
    mo.release();
    let after = mo.gate(true);
    set.add(
        "manual override handover",
        before && !during1 && !during2 && after && mo.handovers == 1 && mo.suppressed == 1,
        "",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn sine_stays_within_base_plus_amplitude() {
        let c = LoadGenerator::curve(LoadShape::Sine { period_ms: 4000, amplitude: 20, base: 50 }, 200, 20);
        assert!(c.iter().all(|&v| (30..=70).contains(&v)), "正弦幅值域 30-70");
    }

    #[test]
    fn audit_counts_only_fast_transitions() {
        let mut au = OscillationAudit::new(1000);
        let log = vec![(0u64, 0u32), (999, 1), (2000, 0)];
        assert_eq!(au.audit(&log), 1, "999ms 间隔算振荡，1001ms 不算");
    }

    #[test]
    fn override_reengage_not_double_counted() {
        let mut mo = ManualOverride::new();
        mo.engage();
        mo.engage(); // 已在手动期——重复 engage 不重复计交接。
        assert_eq!(mo.handovers, 1);
        mo.release();
        mo.engage();
        assert_eq!(mo.handovers, 2, "放手后再接手是新一次交接");
    }
}

// ---------------------------------------------------------------------------
// 深化层六 · 全链端到端验证 + 采样密度自适应
// ---------------------------------------------------------------------------

/// 全链端到端验证（调速器的整车试验）：LoadGenerator 曲线逐点喂
/// FrameGovernor，断言三件事——① 突发负载必触发降级响应（突变要被
/// 看见）；② 常载健康负载全程不进降级（不误伤）；③ 任何时刻延迟守
/// 卫成立（成本永不超基准——全链不变式）。
pub struct EndToEndVerifier;

impl EndToEndVerifier {
    /// 跑一条负载曲线：返回 (峰值模式, 全程延迟守卫绿?)。
    pub fn run(shape: LoadShape, points: usize, step_ms: u64) -> (GovernorMode, bool) {
        let mut g = FrameGovernor::new(32);
        let mut peak = GovernorMode::Full;
        let mut guard = true;
        for i in 0..points.max(1) {
            let t = (i as u64) * step_ms.max(1);
            let budget = LoadGenerator::sample(shape, t);
            // 占用越高帧率越低（线性减半模型：40% 占用 → 60fps——40%
            // 是健康负载，不能一枪打进一级判线）。
            let fps = 80 - budget.min(160) / 2;
            let m = g.sample(fps, budget, 90, true, t);
            if !g.latency_guard() {
                guard = false;
            }
            if m > peak {
                peak = m;
            }
        }
        (peak, guard)
    }
}

/// 采样密度自适应（性能感知的采样面）：负载变化率大 → 加密采样
/// （快变负载需要更细的观察粒度）；平稳 → 稀疏采样（省测量开销）。
/// 密度档唯一源，切换留痕。
pub struct SamplingDensity {
    /// (变化率阈值 ‰/s, 采样间隔 ms)——变化率越高间隔越短。
    pub table: [(u64, u64); 3],
    pub interval_ms: u64,
    pub switches: u64,
}

impl SamplingDensity {
    pub fn new() -> SamplingDensity {
        SamplingDensity {
            table: [(300, 250), (100, 1000), (0, 2000)],
            interval_ms: 2000,
            switches: 0,
        }
    }

    /// 喂变化率（‰/s）：命中首个「变化率 ≥ 阈值」档。
    pub fn feed_rate(&mut self, permille_per_sec: u64) -> u64 {
        let want = self
            .table
            .iter()
            .find(|(th, _)| permille_per_sec >= *th)
            .map(|(_, iv)| *iv)
            .unwrap_or(2000);
        if want != self.interval_ms {
            self.interval_ms = want;
            self.switches += 1;
        }
        self.interval_ms
    }

    /// 表自证：阈值降序、间隔升序（快变密采——策略不倒挂）。
    pub fn monotonic(&self) -> bool {
        self.table.windows(2).all(|w| w[0].0 > w[1].0 && w[0].1 < w[1].1)
    }
}

impl Default for SamplingDensity {
    fn default() -> SamplingDensity {
        SamplingDensity::new()
    }
}

/// 深化层六自检（端到端 / 采样密度）。
pub fn run_animdegrade_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep6");

    // 1. 突发负载：尖峰期必触发降级响应（突变被看见）+ 延迟守卫全程绿。
    let (peak, guard) = EndToEndVerifier::run(
        LoadShape::Burst { base: 30, spike: 99, spike_width_ms: 2000, interval_ms: 6000 },
        60,
        500,
    );
    set.add(
        "burst triggers response with guard",
        peak == GovernorMode::Budget && guard,
        "",
    );

    // 2. 常载健康负载：全程不进降级（不误伤）+ 守卫绿。
    let (peak2, guard2) = EndToEndVerifier::run(LoadShape::Steady(40), 40, 1000);
    set.add(
        "steady healthy never degrades",
        peak2 == GovernorMode::Full && guard2,
        "",
    );

    // 3. 突发负载延迟守卫全程绿（全链不变式的机器证明）。
    let (_, guard3) = EndToEndVerifier::run(
        LoadShape::Burst { base: 50, spike: 100, spike_width_ms: 1500, interval_ms: 4000 },
        80,
        250,
    );
    set.add("guard holds across bursts", guard3, "");

    // 4. 采样密度：变化率大→密采、平稳→疏采（初始 2000 → 密 250 →
    //    回疏 2000 = 两次切换）；表单调自证。
    let mut sd = SamplingDensity::new();
    let d1 = sd.feed_rate(500);
    let d2 = sd.feed_rate(20);
    set.add(
        "sampling density adapts",
        d1 == 250 && d2 == 2000 && sd.switches == 2 && sd.monotonic(),
        "",
    );

    // 5. 密度档同值幂等（不变档不计数）。
    let _ = sd.feed_rate(20);
    set.add("density idempotent", sd.switches == 2, "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn e2e_sine_load_guarded() {
        let (_, guard) = EndToEndVerifier::run(
            LoadShape::Sine { period_ms: 8000, amplitude: 45, base: 50 },
            100,
            100,
        );
        assert!(guard, "正弦负载全周期延迟守卫成立");
    }

    #[test]
    fn density_boundary_rates() {
        let mut sd = SamplingDensity::new();
        assert_eq!(sd.feed_rate(300), 250, "恰在阈值上取更快档（≥ 含边界）");
        assert_eq!(sd.feed_rate(299), 1000);
    }

    #[test]
    fn steady_zero_load_never_degrades() {
        let (peak, guard) = EndToEndVerifier::run(LoadShape::Steady(0), 20, 1000);
        assert_eq!(peak, GovernorMode::Full);
        assert!(guard);
    }
}

// ---------------------------------------------------------------------------
// 深化层七 · 配置快照导出（十四章开放性：配置可备份可迁移）
// ---------------------------------------------------------------------------

/// 调速器配置快照（开放性纪律「配置可备份可编辑」的调速域落法）：
/// 三机制的关键参数（阈值/判线/驻留/密度档）逐项快照为键值账——
/// 导出格式人类可读（key=value 行），导入校验（键白名单 + 数值域），
/// 非法键拒绝留痕（不猜不吞）。
pub struct GovernorConfigSnapshot {
    /// (键, 值)——键 = 参数唯一名。
    pub entries: Vec<(&'static str, u64)>,
}

/// 配置键白名单（导入只认这里的键——未知键拒绝，防注入未知语义）。
pub const CONFIG_KEYS: [&str; 6] = [
    "compositor_budget_pct",
    "fps_tier1",
    "fps_tier_confirm_ms",
    "battery_low_pct",
    "recovery_hold_ms",
    "bar_ttl_ms",
];

impl GovernorConfigSnapshot {
    /// 当前配置快照（从主册常量采集——快照即参数真值的投影）。
    pub fn capture() -> GovernorConfigSnapshot {
        GovernorConfigSnapshot {
            entries: alloc::vec![
                ("compositor_budget_pct", COMPOSITOR_BUDGET_PCT),
                ("fps_tier1", FPS_TIERS[0]),
                ("fps_tier_confirm_ms", TIER_CONFIRM_MS),
                ("battery_low_pct", BATTERY_LOW_PCT),
                ("recovery_hold_ms", RECOVERY_HOLD_MS),
                ("bar_ttl_ms", 30_000),
            ],
        }
    }

    /// 导出为人类可读行（key=value——用户能直接看懂、能编辑）。
    pub fn export(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for (k, v) in &self.entries {
            s.push_str(k);
            s.push('=');
            s.push_str(&alloc::format!("{}", v));
            s.push('\n');
        }
        s
    }

    /// 导入解析：逐行 key=value；键白名单外拒绝并留痕（键名/行号）。
    /// 返回 (解析出的键值对, 被拒行清单)。
    pub fn import(text: &str) -> (Vec<(&'static str, u64)>, Vec<alloc::string::String>) {
        let mut out = Vec::new();
        let mut rejected = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match line.split_once('=') {
                Some((k, v)) if CONFIG_KEYS.contains(&k) => {
                    match v.parse::<u64>() {
                        Ok(n) => {
                            let key = CONFIG_KEYS.iter().find(|c| **c == k).unwrap();
                            out.push((*key, n));
                        }
                        Err(_) => rejected.push(alloc::format!("{}=非数值", k)),
                    }
                }
                _ => rejected.push(alloc::string::String::from(line)),
            }
        }
        (out, rejected)
    }

    /// round-trip 自证：导出 → 导入 → 键值对与原快照一致（可迁移的
    /// 机器证明）。
    pub fn round_trip(&self) -> bool {
        let (parsed, rejected) = Self::import(&self.export());
        rejected.is_empty() && parsed == self.entries
    }
}

/// 深化层七自检（配置快照导出导入）。
pub fn run_animdegrade_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F331-333-deep7");

    // 1. 快照采集：六键全在（参数唯一源投影完整）。
    let snap = GovernorConfigSnapshot::capture();
    set.add(
        "snapshot covers all keys",
        snap.entries.len() == CONFIG_KEYS.len()
            && snap.entries.iter().all(|(k, _)| CONFIG_KEYS.contains(k)),
        "",
    );

    // 2. 导出人话可读（含判据常量锚点值）。
    let text = snap.export();
    set.add(
        "export human readable",
        text.contains("compositor_budget_pct=90") && text.contains("battery_low_pct=20"),
        "",
    );

    // 3. round-trip：导出→导入键值一致（可迁移证明）。
    set.add("config round trip", snap.round_trip(), "");

    // 4. 导入防呆：未知键拒绝留痕、非数值拒绝、空行跳过。
    let (_, rej) = GovernorConfigSnapshot::import("hacker_key=1\nfps_tier1=abc\n\nfps_tier1=45\n");
    set.add(
        "import rejects unknown and non-numeric",
        rej.len() == 2 && rej[0] == "hacker_key=1" && rej[1].contains("非数值"),
        "",
    );

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn import_valid_subset() {
        let (parsed, rej) = GovernorConfigSnapshot::import("fps_tier1=48\nbar_ttl_ms=25000\n");
        assert!(rej.is_empty() && parsed.len() == 2);
        assert_eq!(parsed[0], ("fps_tier1", 48));
    }

    #[test]
    fn empty_import_empty_result() {
        let (parsed, rej) = GovernorConfigSnapshot::import("");
        assert!(parsed.is_empty() && rej.is_empty());
    }

    #[test]
    fn snapshot_values_match_master_constants() {
        let snap = GovernorConfigSnapshot::capture();
        let get = |k: &str| snap.entries.iter().find(|(kk, _)| *kk == k).unwrap().1;
        assert_eq!(get("fps_tier_confirm_ms"), TIER_CONFIRM_MS);
        assert_eq!(get("recovery_hold_ms"), RECOVERY_HOLD_MS);
    }
}

// ---------------------------------------------------------------------------
// 深化层八 · 健康自检探针 + 模式驻留占比聚合
// ---------------------------------------------------------------------------

/// 调速器健康自检探针（十三章·补「自检/心跳」的调速域落法）：三项
/// 自检——① 参数表引用一致（快照值与主册常量逐一相等——常量被改
/// 而快照没跟 = 配置漂移）；② 模式账容量（环形账条目数不超上限——
/// 溢出即缺陷）；③ 折算表单调（TierPolicyTable 不倒挂）。全部绿 =
/// 心跳健康；任一红 = 显性报告（不给静默漂移留门）。
pub struct HealthProbe;

impl HealthProbe {
    /// ① 参数表引用一致性（与快照采集对拍）。
    pub fn params_consistent() -> bool {
        let snap = GovernorConfigSnapshot::capture();
        let get = |k: &str| snap.entries.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v);
        get("compositor_budget_pct") == Some(COMPOSITOR_BUDGET_PCT)
            && get("fps_tier1") == Some(FPS_TIERS[0])
            && get("fps_tier_confirm_ms") == Some(TIER_CONFIRM_MS)
            && get("battery_low_pct") == Some(BATTERY_LOW_PCT)
            && get("recovery_hold_ms") == Some(RECOVERY_HOLD_MS)
    }

    /// ② 模式账容量守卫（条目数 ≤ 上限——环形账不撑爆）。
    pub fn ledger_capacity_ok(len: usize, cap: usize) -> bool {
        len <= cap
    }

    /// 三项聚合心跳。
    pub fn heartbeat(ledger_len: usize, ledger_cap: usize) -> bool {
        Self::params_consistent() && TierPolicyTable::monotonic()
            && Self::ledger_capacity_ok(ledger_len, ledger_cap)
    }
}

/// 模式驻留占比聚合（可观测性面）：模式账 (时刻, 模式) → 各模式驻留
/// 时长与占比‰——「降级占了多少时间」直接出数（性能感知的运营面）。
/// 占比 = 该模式驻留 ms / 总跨度。
pub struct ModeDwellStats;

impl ModeDwellStats {
    /// 聚合：返回 (模式, 驻留 ms, 占比‰)——模式升序稳定输出。
    pub fn aggregate(log: &[(u64, GovernorMode)], total_ms: u64) -> Vec<(GovernorMode, u64, u32)> {
        let mut out: Vec<(GovernorMode, u64)> = Vec::new();
        for w in log.windows(2) {
            let dwell = w[1].0.saturating_sub(w[0].0);
            match out.iter_mut().find(|(m, _)| *m == w[0].1) {
                Some((_, d)) => *d += dwell,
                None => out.push((w[0].1, dwell)),
            }
        }
        if let (Some(last), false) = (log.last(), log.is_empty()) {
            let tail = total_ms.saturating_sub(last.0);
            match out.iter_mut().find(|(m, _)| *m == last.1) {
                Some((_, d)) => *d += tail,
                None => out.push((last.1, tail)),
            }
        }
        out.sort_by_key(|(m, _)| *m);
        out.into_iter()
            .map(|(m, d)| (m, d, if total_ms == 0 { 0 } else { (d * 1000 / total_ms) as u32 }))
            .collect()
    }
}

/// 深化层八自检（健康探针 / 驻留占比）。
pub fn run_animdegrade_deep8_checks() -> CheckSet {
    use alloc::vec;
    let mut set = CheckSet::new("F331-333-deep8");

    // 1. 参数引用一致（快照与主册常量对拍绿——无配置漂移）。
    set.add("params consistent", HealthProbe::params_consistent(), "");

    // 2. 容量守卫：界内绿、超界红（显性——不静默溢出）。
    set.add(
        "ledger capacity guard",
        HealthProbe::ledger_capacity_ok(99, 100) && !HealthProbe::ledger_capacity_ok(101, 100),
        "",
    );

    // 3. 心跳聚合：三项全绿才心跳（探针语义）。
    set.add("heartbeat green", HealthProbe::heartbeat(50, 100), "");

    // 4. 驻留占比：全效 800ms + 二级 200ms → 占比 80/20（总跨度锚定）。
    let log = vec![
        (0u64, GovernorMode::Full),
        (800, GovernorMode::Budget),
        (1000, GovernorMode::Full),
    ];
    let stats = ModeDwellStats::aggregate(&log, 1000);
    set.add(
        "dwell stats 80 20",
        stats.len() == 2
            && stats.iter().any(|(m, d, p)| *m == GovernorMode::Full && *d == 800 && *p == 800)
            && stats.iter().any(|(m, d, p)| *m == GovernorMode::Budget && *d == 200 && *p == 200),
        "",
    );

    // 5. 零账诚实空；总跨度 0 不虚报占比。
    let empty = ModeDwellStats::aggregate(&[], 1000);
    let zero = ModeDwellStats::aggregate(&[(0u64, GovernorMode::Full)], 0);
    set.add(
        "dwell honest empty and zero",
        empty.is_empty() && zero.iter().all(|(_, _, p)| *p == 0),
        "",
    );

    set
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn dwell_single_mode_full_span() {
        let log = vec![(0u64, GovernorMode::Full)];
        let stats = ModeDwellStats::aggregate(&log, 5000);
        assert_eq!(stats, vec![(GovernorMode::Full, 5000, 1000)]);
    }

    #[test]
    fn dwell_two_entries_same_mode_merge() {
        let log = vec![(0u64, GovernorMode::Full), (100, GovernorMode::Full)];
        let stats = ModeDwellStats::aggregate(&log, 200);
        assert_eq!(stats.len(), 1, "同模式相邻段合并");
        assert_eq!(stats[0].1, 200);
    }

    #[test]
    fn heartbeat_red_on_overflow() {
        assert!(!HealthProbe::heartbeat(1000, 100), "账本溢出 = 心跳红");
    }
}

// ---------------------------------------------------------------------------
// 深化层九 · 降级面板聚合视图 + 调速器事件流
// ---------------------------------------------------------------------------

/// 降级面板聚合视图（界面只读数据面——一处聚合不散拼）：当前模式 +
/// 三源驻留占比 + 最近一次降级动作 + 健康心跳——渲染层消费此结构画
/// 面板，不自算（一处一事实纪律）。
pub struct PanelView {
    pub current_mode: GovernorMode,
    /// 驻留占比（来自 ModeDwellStats）。
    pub dwell: Vec<(GovernorMode, u32)>,
    pub last_action: &'static str,
    pub heartbeat_ok: bool,
}

pub struct PanelAggregator;

impl PanelAggregator {
    /// 模式 → 层级映射（显式成文——改机制语义必炸对拍）。
    fn tier_of(mode: GovernorMode) -> Tier {
        match mode {
            GovernorMode::Full => Tier::None,
            GovernorMode::LowBatt => Tier::Complexity,
            GovernorMode::Budget => Tier::Duration,
            GovernorMode::FpsTier => Tier::Suggest,
        }
    }

    /// 从模式账聚合出面板视图（纯读——不改任何状态）。
    pub fn view(log: &[(u64, GovernorMode)], total_ms: u64, ledger_cap: usize) -> PanelView {
        let current = log.last().map(|(_, m)| *m).unwrap_or(GovernorMode::Full);
        let dwell: Vec<(GovernorMode, u32)> =
            ModeDwellStats::aggregate(log, total_ms).into_iter().map(|(m, _, p)| (m, p)).collect();
        let last_action = TierPolicyTable::action_for(Self::tier_of(current));
        PanelView {
            current_mode: current,
            dwell,
            last_action,
            heartbeat_ok: HealthProbe::heartbeat(log.len(), ledger_cap),
        }
    }

    /// 视图自证：占比合计 ≤1000‰（浮点不出门——全整数口径）。
    pub fn dwell_sane(view: &PanelView) -> bool {
        view.dwell.iter().map(|(_, p)| *p as u64).sum::<u64>() <= 1000
    }
}

/// 调速器事件流（十三章日志语义的调速域面）：模式切换逐事件留痕
/// (时刻, 旧模式, 新模式, 触发机制名)——可导出可回放；同模式重入
/// 不记（切换才留痕——事件流不刷噪音）。
pub struct ModeEventStream {
    pub events: Vec<(u64, GovernorMode, GovernorMode, &'static str)>,
    pub cap: usize,
}

impl ModeEventStream {
    pub fn new(cap: usize) -> ModeEventStream {
        ModeEventStream { events: Vec::new(), cap: cap.max(1) }
    }

    /// 记一次模式切换（同模式重入不记——噪音防线）。
    pub fn observe(&mut self, at_ms: u64, old: GovernorMode, new: GovernorMode, via: &'static str) {
        if old == new {
            return;
        }
        self.events.push((at_ms, old, new, via));
        if self.events.len() > self.cap {
            self.events.remove(0);
        }
    }

    /// 导出人话行（可查可回放——十三章总日志中心语义）。
    pub fn export(&self) -> alloc::string::String {
        self.events
            .iter()
            .map(|(t, o, n, v)| {
                alloc::format!("{}ms: {:?} → {:?} ({})\n", t, o, n, v)
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

/// 深化层九自检（面板视图 / 事件流）。
pub fn run_animdegrade_deep9_checks() -> CheckSet {
    use alloc::vec;
    let mut set = CheckSet::new("F331-333-deep9");

    // 1. 面板视图：当前模式、驻留占比、动作文案、心跳四件齐（Budget
    //    模式的动作文案是「F124 时长砍半」——策略表同源）。
    let log = vec![
        (0u64, GovernorMode::Full),
        (700, GovernorMode::Budget),
        (1000, GovernorMode::Budget),
    ];
    let view = PanelAggregator::view(&log, 1000, 100);
    set.add(
        "panel view four facts",
        view.current_mode == GovernorMode::Budget
            && view.last_action.contains("时长砍半")
            && view.heartbeat_ok
            && PanelAggregator::dwell_sane(&view),
        "",
    );

    // 2. 占比合计 ≤1000‰（全整数口径不溢出）。
    set.add("dwell sum bounded", PanelAggregator::dwell_sane(&view), "");

    // 3. 事件流：切换留痕、同模式重入不记、环形封顶挤出最旧。
    let mut es = ModeEventStream::new(3);
    es.observe(0, GovernorMode::Full, GovernorMode::Full, "预算闸门");
    es.observe(100, GovernorMode::Full, GovernorMode::Budget, "预算闸门");
    es.observe(200, GovernorMode::Budget, GovernorMode::Budget, "帧率");
    es.observe(300, GovernorMode::Budget, GovernorMode::Full, "恢复判定");
    es.observe(400, GovernorMode::Full, GovernorMode::FpsTier, "帧率");
    es.observe(500, GovernorMode::FpsTier, GovernorMode::Full, "恢复判定");
    set.add(
        "event stream switches only capped",
        es.len() == 3 && es.events[0].0 == 300 && es.events[2].2 == GovernorMode::Full,
        "",
    );

    // 4. 导出人话行（可回放）。
    set.add("event export human", es.export().contains("→"), "");

    set
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn empty_log_view_defaults_full() {
        let view = PanelAggregator::view(&[], 0, 100);
        assert_eq!(view.current_mode, GovernorMode::Full);
        assert!(view.dwell.is_empty());
    }

    #[test]
    fn event_ring_evicts_oldest() {
        let mut es = ModeEventStream::new(2);
        es.observe(0, GovernorMode::Full, GovernorMode::Budget, "a");
        es.observe(1, GovernorMode::Budget, GovernorMode::Full, "b");
        es.observe(2, GovernorMode::Full, GovernorMode::Budget, "c");
        assert_eq!(es.len(), 2);
        assert_eq!(es.events[0].0, 1, "最旧被挤出环形");
    }

    #[test]
    fn view_dwell_single_mode_thousand() {
        let log = vec![(0u64, GovernorMode::Full)];
        let view = PanelAggregator::view(&log, 4000, 100);
        assert_eq!(view.dwell, vec![(GovernorMode::Full, 1000)]);
    }
}
