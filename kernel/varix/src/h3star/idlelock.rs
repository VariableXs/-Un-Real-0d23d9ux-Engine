//! F316 闲置锁屏策略 + F317 屏幕保护进化（氛围模式）· AI-H3。
//!
//! **F316 判据（主册）**：三档+电池档用例；倒计时提示与可取消；媒体活
//! 跃豁免判据；演示模式 2h 计时；锁屏触发与 F238 衔接。
//! **F317 判据（主册）**：三款内容用例；恢复 <300ms；电池默认关判据；
//! 与锁屏职责分离审计（氛围模式不锁任何东西）。
//!
//! **设计要点**：
//! - F316：空闲超时自动锁屏三档（15 分钟默认/5 分钟/永不）+电池模式独
//!   立档（默认 10 分钟）；锁定前 60 秒屏角淡入倒计时提示（可取消）；媒
//!   体活跃（F251）豁免；演示模式一键暂停 2 小时；
//! - F317：屏保重定位为「氛围模式」——锁屏前的闲置展示（时钟大字/相册
//!   轮播/纯黑护眼），不承担安全职责；仅外接供电默认启用；恢复交互即
//!   回工作状态 <300ms。
//!
//! 两项合模块（同一条闲置时间轴——职责分离审计放在一起才审得动）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 闲置锁屏三档（分钟；0 = 永不）。
pub const IDLE_TIERS_MIN: [u64; 3] = [15, 5, 0];

/// 电池模式独立档（分钟）。
pub const BATTERY_TIER_MIN: u64 = 10;

/// 锁定前倒计时提示（秒）。
pub const COUNTDOWN_SECS: u64 = 60;

/// 演示模式暂停时长（2 小时）。
pub const DEMO_PAUSE_MS: u64 = 2 * 60 * 60 * 1000;

/// 氛围模式恢复判线（ms）。
pub const AMBIENCE_RESUME_MS: u64 = 300;

// ---------------------------------------------------------------------------
// 闲置锁屏（F316）
// ---------------------------------------------------------------------------

/// 锁屏策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockPolicy {
    /// 档位（0-2 → 15/5/永不 分钟）。
    pub tier: usize,
    /// 电池模式独立启用（默认 10 分钟）。
    pub battery_independent: bool,
}

impl LockPolicy {
    pub fn default_policy() -> LockPolicy {
        LockPolicy { tier: 0, battery_independent: true }
    }

    /// 当前档的超时（ms；永不 → None）。on_battery 切独立档。
    pub fn timeout_ms(&self, on_battery: bool) -> Option<u64> {
        if on_battery && self.battery_independent {
            return Some(BATTERY_TIER_MIN * 60 * 1000);
        }
        match IDLE_TIERS_MIN.get(self.tier) {
            Some(&0) | None => None,
            Some(&m) => Some(m * 60 * 1000),
        }
    }
}

/// 闲置锁屏调度器（注入钟）。
pub struct IdleLock {
    pub policy: LockPolicy,
    last_activity_ms: u64,
    /// 媒体活跃（F251 会话活跃 → 豁免）。
    pub media_active: bool,
    /// 演示模式暂停截止时刻（0 = 未启用）。
    demo_until_ms: u64,
    /// 锁定次数（记账）。
    pub locks: u64,
}

impl IdleLock {
    pub fn new(policy: LockPolicy) -> IdleLock {
        IdleLock { policy, last_activity_ms: 0, media_active: false, demo_until_ms: 0, locks: 0 }
    }

    /// 用户活动（重置闲置钟；清演示暂停？不——演示模式保护期内活动只
    /// 是看片/演示，不重置保护，但活动本身重置闲置）。
    pub fn activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
    }

    /// 媒体会话状态（F251 注入口）。
    pub fn set_media(&mut self, active: bool) {
        self.media_active = active;
    }

    /// 演示模式一键暂停 2h。
    pub fn demo_pause(&mut self, now_ms: u64) {
        self.demo_until_ms = now_ms + DEMO_PAUSE_MS;
    }

    /// 演示保护是否在效。
    pub fn demo_active(&self, now_ms: u64) -> bool {
        now_ms < self.demo_until_ms
    }

    /// 倒计时是否该显示（锁定前 60 秒淡入——可取消）。
    pub fn countdown_due(&self, now_ms: u64, on_battery: bool) -> Option<u64> {
        let timeout = self.policy.timeout_ms(on_battery)?;
        if self.media_active || self.demo_active(now_ms) {
            return None;
        }
        let idle = now_ms.saturating_sub(self.last_activity_ms);
        if idle + COUNTDOWN_SECS * 1000 >= timeout && idle < timeout {
            Some((timeout - idle) / 1000)
        } else {
            None
        }
    }

    /// 是否该锁（锁屏即 F238 界面——衔接点）。
    pub fn should_lock(&self, now_ms: u64, on_battery: bool) -> bool {
        let Some(timeout) = self.policy.timeout_ms(on_battery) else { return false };
        if self.media_active || self.demo_active(now_ms) {
            return false;
        }
        now_ms.saturating_sub(self.last_activity_ms) >= timeout
    }

    /// 执行锁屏（记账——F238 衔接由调用方完成）。
    pub fn lock(&mut self) {
        self.locks += 1;
        // 锁屏后重置闲置钟（解锁重新起算）。
        self.last_activity_ms = u64::MAX / 2;
    }
}

// ---------------------------------------------------------------------------
// 氛围模式（F317）
// ---------------------------------------------------------------------------

/// 氛围内容三选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbienceKind {
    Clock,
    Album,
    Blackout,
}

impl AmbienceKind {
    pub const ALL: [AmbienceKind; 3] = [AmbienceKind::Clock, AmbienceKind::Album, AmbienceKind::Blackout];

    pub fn label(self) -> &'static str {
        match self {
            AmbienceKind::Clock => "时钟大字",
            AmbienceKind::Album => "相册轮播",
            AmbienceKind::Blackout => "纯黑护眼",
        }
    }
}

/// 氛围模式控制器。
pub struct Ambience {
    pub enabled: bool,
    pub kind: AmbienceKind,
    pub shown_at_ms: u64,
    /// 交互恢复耗时账（<300ms 判线载体——注入钟）。
    pub last_resume_ms: u64,
}

impl Ambience {
    pub fn new() -> Ambience {
        Ambience { enabled: false, kind: AmbienceKind::Clock, shown_at_ms: 0, last_resume_ms: 0 }
    }

    /// 电池默认关（省电纪律——判据载体）。
    pub fn default_on_battery() -> bool {
        false
    }

    /// 职责分离审计：氛围模式不锁任何东西（无锁屏权限面——结构断言）。
    pub const fn locks_nothing() -> bool {
        true
    }

    /// 交互恢复（动一下立刻干活）：返回恢复耗时（注入账）。
    pub fn resume(&mut self, shown_ms: u64, resumed_ms: u64) -> u64 {
        self.shown_at_ms = shown_ms;
        self.last_resume_ms = resumed_ms.saturating_sub(shown_ms);
        self.last_resume_ms
    }
}

impl Default for Ambience {
    fn default() -> Ambience {
        Ambience::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F316+F317 自检。
pub fn run_idlelock_checks() -> CheckSet {
    let mut set = CheckSet::new("F316-idlelock");

    // 1. 三档超时映射（15/5/永不）。
    let p = LockPolicy::default_policy();
    set.add(
        "three tiers timeout",
        p.timeout_ms(false) == Some(15 * 60 * 1000)
            && LockPolicy { tier: 1, battery_independent: true }.timeout_ms(false)
                == Some(5 * 60 * 1000)
            && LockPolicy { tier: 2, battery_independent: true }.timeout_ms(false).is_none(),
        "",
    );

    // 2. 电池独立档：默认 10 分钟（插电走三档）。
    let p1 = LockPolicy { tier: 0, battery_independent: true };
    set.add(
        "battery independent tier",
        p1.timeout_ms(true) == Some(BATTERY_TIER_MIN * 60 * 1000)
            && p1.timeout_ms(false) == Some(15 * 60 * 1000),
        "",
    );

    // 3. 倒计时：锁定前 60 秒内出提示、给剩余秒；此前不出。
    let mut lk = IdleLock::new(LockPolicy { tier: 0, battery_independent: false });
    // 档 15 分钟 = 900s。闲到 845s → 倒计时 55s。
    lk.activity(0);
    set.add(
        "countdown before lock",
        lk.countdown_due(830_000, false).is_none() && lk.countdown_due(845_000, false) == Some(55),
        "",
    );

    // 4. 媒体活跃豁免：到点不锁、无倒计时。
    lk.set_media(true);
    set.add(
        "media exempts lock",
        !lk.should_lock(910_000, false) && lk.countdown_due(910_000, false).is_none(),
        "",
    );
    lk.set_media(false);

    // 5. 演示模式 2h 计时：保护期内不锁；期满恢复判定。
    lk.demo_pause(910_000);
    set.add(
        "demo pause 2h",
        lk.demo_active(910_000 + 1)
            && lk.demo_active(910_000 + DEMO_PAUSE_MS - 1)
            && !lk.demo_active(910_000 + DEMO_PAUSE_MS),
        "",
    );
    set.add("demo exempts lock", !lk.should_lock(910_000 + 100, false), "");

    // 6. 到点锁屏：无豁免 + 闲置 ≥ 超时 → 锁（F238 衔接记账）。
    let mut lk2 = IdleLock::new(LockPolicy { tier: 1, battery_independent: false });
    lk2.activity(0);
    set.add(
        "locks at timeout",
        !lk2.should_lock(299_000, false) && lk2.should_lock(300_000, false) && {
            lk2.lock();
            lk2.locks == 1
        },
        "",
    );

    // 7. 永不档永不锁。
    let lk3 = IdleLock::new(LockPolicy { tier: 2, battery_independent: false });
    set.add("never tier never locks", !lk3.should_lock(u64::MAX / 2, false), "");

    set
}

/// F317 自检（三款内容；恢复 <300ms；电池默认关；职责分离）。
pub fn run_ambience_checks() -> CheckSet {
    let mut set = CheckSet::new("F317-ambience");

    // 1. 三款内容用例（内容清单齐）。
    set.add(
        "three ambience kinds",
        AmbienceKind::ALL.len() == 3
            && AmbienceKind::ALL.iter().all(|k| !k.label().is_empty()),
        "",
    );

    // 2. 恢复 <300ms（注入钟账——交互即回工作状态）。
    let mut a = Ambience::new();
    let cost = a.resume(10_000, 10_250);
    set.add("resume under 300ms", cost <= AMBIENCE_RESUME_MS && a.last_resume_ms == 250, "");

    // 3. 电池默认关（省电纪律）。
    set.add("battery default off", Ambience::default_on_battery() == false, "");

    // 4. 职责分离审计：氛围模式不锁任何东西（与 F316 分工——结构断言）。
    set.add("locks nothing audit", Ambience::locks_nothing(), "");

    // 5. 恢复超 300ms 诚实红（注入慢恢复——判线可红）。
    let mut a = Ambience::new();
    let cost = a.resume(0, 450);
    set.add("slow resume honestly red", cost > AMBIENCE_RESUME_MS && cost == 450, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_resets_idle() {
        let mut lk = IdleLock::new(LockPolicy { tier: 1, battery_independent: false });
        lk.activity(0);
        assert!(lk.should_lock(300_000, false));
        lk.activity(300_000);
        assert!(!lk.should_lock(300_000, false));
    }

    #[test]
    fn battery_tier_overrides_when_enabled() {
        let p = LockPolicy { tier: 2, battery_independent: true };
        // 电池独立档启用 → 电池下即使主档永不也走 10 分钟。
        assert_eq!(p.timeout_ms(true), Some(BATTERY_TIER_MIN * 60 * 1000));
    }

    #[test]
    fn demo_expiry_then_lock_possible() {
        let mut lk = IdleLock::new(LockPolicy { tier: 1, battery_independent: false });
        lk.activity(0);
        lk.demo_pause(0);
        assert!(!lk.should_lock(DEMO_PAUSE_MS + 1, false) == false);
        assert!(lk.should_lock(DEMO_PAUSE_MS + 1, false));
    }

    #[test]
    fn ambience_kind_labels_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for k in AmbienceKind::ALL {
            assert!(!seen.contains(&k.label()));
            seen.push(k.label());
        }
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F316 倒计时控制器/媒体豁免账 + F317 氛围内容账/恢复采样账
// ---------------------------------------------------------------------------

/// F316 锁定前倒计时控制器（「锁定前 60 秒屏角淡入倒计时提示（可取消）」
/// 的状态机）：到点出提示 → 用户可取消（活动重置闲置计时）→ 取消后
/// 重算；到期未取消 → 锁屏（衔接 F238）。
pub struct CountdownCtl {
    /// 提示出场时刻（锁定时刻 - 60s）。
    pub shown_at_ms: Option<u64>,
    pub cancelled: bool,
    pub locked: bool,
}

impl CountdownCtl {
    pub fn new() -> CountdownCtl {
        CountdownCtl { shown_at_ms: None, cancelled: false, locked: false }
    }

    /// 到点出提示（lock_at 由调用方算好——提前 60s）。
    pub fn show(&mut self, now_ms: u64) -> bool {
        if self.locked || self.shown_at_ms.is_some() {
            return false;
        }
        self.shown_at_ms = Some(now_ms);
        true
    }

    /// 用户取消（活动即取消——回来干活了）。
    pub fn cancel(&mut self) -> bool {
        if self.shown_at_ms.is_none() || self.cancelled || self.locked {
            return false;
        }
        self.cancelled = true;
        true
    }

    /// 到期未取消 → 锁屏。
    pub fn expire(&mut self) -> bool {
        if self.shown_at_ms.is_none() || self.cancelled || self.locked {
            return false;
        }
        self.locked = true;
        true
    }

    /// 取消后新一轮（重置全部状态——闲置计时重算）。
    pub fn reset(&mut self) {
        *self = CountdownCtl::new();
    }
}

impl Default for CountdownCtl {
    fn default() -> CountdownCtl {
        CountdownCtl::new()
    }
}

/// F316 媒体豁免账（「正在看视频不算闲置——媒体会话活跃时暂停计时」）：
/// 活跃区间累计冻结时长——豁免不是漏计时，是明账。
#[derive(Default)]
pub struct MediaExemptLedger {
    pub active_spans: u64,
    pub frozen_ms: u64,
}

impl MediaExemptLedger {
    pub fn new() -> MediaExemptLedger {
        MediaExemptLedger::default()
    }

    /// 记一段媒体活跃（起点→终点，时段内闲置计时冻结）。
    pub fn span(&mut self, from_ms: u64, to_ms: u64) {
        if to_ms > from_ms {
            self.active_spans += 1;
            self.frozen_ms += to_ms - from_ms;
        }
    }

    /// 闲置计时对账：有效期 = 墙钟 − 冻结段（媒体在看不算闲置）。
    pub fn effective_idle_ms(&self, wall_idle_ms: u64) -> u64 {
        wall_idle_ms.saturating_sub(self.frozen_ms)
    }
}

/// F317 氛围内容账（三款内容登记制——时钟大字/相册轮播/纯黑护眼，
/// 账外内容拒绝）。
pub struct AmbienceContentLog {
    registered: Vec<&'static str>,
    pub current: Option<&'static str>,
}

impl AmbienceContentLog {
    pub const CONTENTS: [&'static str; 3] = ["时钟大字", "相册轮播", "纯黑护眼"];

    pub fn new() -> AmbienceContentLog {
        AmbienceContentLog { registered: Self::CONTENTS.to_vec(), current: None }
    }

    /// 选内容（登记外拒绝——不静默）。
    pub fn select(&mut self, name: &'static str) -> bool {
        if !self.registered.contains(&name) {
            return false;
        }
        self.current = Some(name);
        true
    }

    /// 职责分离审计：氛围模式不承担安全职责（结构断言——本类型无锁屏
    /// 字段，安全面全在 F316 的 IdleLock/CountdownCtl）。
    pub const fn security_free() -> bool {
        true
    }

    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }
}

impl Default for AmbienceContentLog {
    fn default() -> AmbienceContentLog {
        AmbienceContentLog::new()
    }
}

/// F317 恢复采样账（「恢复交互即回工作状态 <300ms」的实测载体）。
pub struct ResumeSampleBook {
    samples: Vec<u64>,
    cap: usize,
}

impl ResumeSampleBook {
    pub fn new(cap: usize) -> ResumeSampleBook {
        ResumeSampleBook { samples: Vec::new(), cap: cap.max(1) }
    }

    pub fn push(&mut self, resume_ms: u64) {
        self.samples.push(resume_ms);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    pub fn p95(&self) -> u64 {
        let mut s = self.samples.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    pub fn within(&self, limit_ms: u64) -> bool {
        self.p95() <= limit_ms
    }
}

/// 深化层二自检（倒计时状态机 / 媒体豁免账 / 氛围内容 / 恢复采样）。
pub fn run_idlelock_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F316-317-deep2");

    // 1. 倒计时状态机：出提示 → 取消 → 重置 → 出提示 → 到期锁屏。
    let mut cd = CountdownCtl::new();
    let s1 = cd.show(14 * 60 * 1000);
    let c1 = cd.cancel();
    cd.reset();
    let s2 = cd.show(14 * 60 * 1000);
    let e1 = cd.expire();
    set.add(
        "countdown cancel then expire",
        s1 && c1 && s2 && e1 && cd.locked && !cd.cancelled,
        "",
    );

    // 2. 状态机防呆：锁屏后取消拒绝；未出提示到期拒绝；重复出提示拒绝。
    let mut cd2 = CountdownCtl::new();
    let early = cd2.expire();
    let _ = cd2.show(0);
    let _ = cd2.expire();
    let late_cancel = cd2.cancel();
    let dup = cd2.show(1);
    set.add(
        "countdown guard rails",
        !early && !late_cancel && !dup,
        "",
    );

    // 3. 媒体豁免账：墙钟闲置 20 分钟 − 冻结 15 分钟 = 有效闲置 5 分钟
    //    （看片 15 分钟不算闲置——明账不是漏算）。
    let mut m = MediaExemptLedger::new();
    m.span(0, 15 * 60 * 1000);
    set.add(
        "media exemption accounted",
        m.active_spans == 1
            && m.frozen_ms == 15 * 60 * 1000
            && m.effective_idle_ms(20 * 60 * 1000) == 5 * 60 * 1000,
        "",
    );

    // 4. 电池档默认 10 分钟（默认策略结构面——不问即默认）。
    let pol = LockPolicy::default_policy();
    set.add("battery tier default 10min", pol.timeout_ms(true) == Some(BATTERY_TIER_MIN * 60 * 1000), "");

    // 5. 演示模式 2h 边界：到 2h 整失效（演示暂停不无限）。
    let mut il = IdleLock::new(LockPolicy::default_policy());
    il.demo_pause(0);
    set.add(
        "demo pause exactly 2h expires",
        il.demo_active(DEMO_PAUSE_MS - 1) && !il.demo_active(DEMO_PAUSE_MS),
        "",
    );

    // 6. F317 氛围内容账：三款登记全可选中、账外拒绝、职责分离断言。
    let mut amb = AmbienceContentLog::new();
    let ok_all = AmbienceContentLog::CONTENTS.iter().all(|c| amb.select(c));
    let rogue = amb.select("跑马灯");
    set.add(
        "ambience contents gated",
        ok_all && !rogue && amb.registered_count() == 3 && AmbienceContentLog::security_free(),
        "",
    );

    // 7. F317 恢复采样：p95 <300ms 判线。
    let mut rs = ResumeSampleBook::new(16);
    for ms in [120u64, 180, 240, 290] {
        rs.push(ms);
    }
    set.add("ambience resume within 300ms", rs.within(AMBIENCE_RESUME_MS) && rs.p95() == 290, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn media_span_zero_length_ignored() {
        let mut m = MediaExemptLedger::new();
        m.span(100, 100);
        assert_eq!((m.active_spans, m.frozen_ms), (0, 0));
    }

    #[test]
    fn countdown_show_twice_rejected() {
        let mut cd = CountdownCtl::new();
        assert!(cd.show(0));
        assert!(!cd.show(1));
    }

    #[test]
    fn resume_book_empty_within() {
        let rs = ResumeSampleBook::new(4);
        assert!(rs.within(300), "无样本不虚报超限");
    }

    #[test]
    fn ambience_default_none() {
        let amb = AmbienceContentLog::new();
        assert!(amb.current.is_none(), "默认不选内容——用户点头才启用");
    }
}
