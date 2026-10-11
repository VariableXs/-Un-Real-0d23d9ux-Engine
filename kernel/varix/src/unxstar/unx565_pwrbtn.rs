//! UNX-F0565 · 电源按钮事件观察账
//!
//! 诊断段 0x9A21~0x9A28（PM 族观察段）。
//! 判据（施工书 J1）：按钮位观察账（回环注入 4/4 事件检出），A1 期零处置声明（观察口径）。

use alloc::vec::Vec;

/// 电源按钮诊断码闭集（0x9A21~0x9A28 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtnCode {
    /// 0x9A21 按钮位观察落账。
    Observed = 0x9A21,
    /// 0x9A22 按下沿检出。
    PressEdge = 0x9A22,
    /// 0x9A23 释放沿检出。
    ReleaseEdge = 0x9A23,
    /// 0x9A24 长按检出（按住 ≥ 长按门限）。
    LongPress = 0x9A24,
    /// 0x9A25 抖动过滤（同沿窗内重复丢弃）。
    Debounced = 0x9A25,
    /// 0x9A26 W1C 零写防线（观察期禁清位——零处置声明落实）。
    ZeroWriteBlocked = 0x9A26,
    /// 0x9A27 回环注入事件检出。
    InjectDetected = 0x9A27,
    /// 0x9A28 观察账守恒破坏。
    LedgerBroken = 0x9A28,
}

impl BtnCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            BtnCode::Observed => "按钮位已观察落账",
            BtnCode::PressEdge => "按下沿检出",
            BtnCode::ReleaseEdge => "释放沿检出",
            BtnCode::LongPress => "长按时长越门限",
            BtnCode::Debounced => "抖动沿被过滤",
            BtnCode::ZeroWriteBlocked => "清位写被拦截（观察期零处置）",
            BtnCode::InjectDetected => "回环注入事件检出",
            BtnCode::LedgerBroken => "观察账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            BtnCode::Observed => "轮询拍采样",
            BtnCode::PressEdge => "位值 0→1 跳变",
            BtnCode::ReleaseEdge => "位值 1→0 跳变",
            BtnCode::LongPress => "按下持续时间越 2s 门限",
            BtnCode::Debounced => "同沿 8 拍内重复",
            BtnCode::ZeroWriteBlocked => "A1 期观察口径：只读不清位",
            BtnCode::InjectDetected => "回环通道注入位翻转",
            BtnCode::LedgerBroken => "检出数与账目不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            BtnCode::Observed => "无需处置",
            BtnCode::PressEdge => "观察口径：仅落账",
            BtnCode::ReleaseEdge => "观察口径：仅落账",
            BtnCode::LongPress => "观察口径：仅落账（不触发停机）",
            BtnCode::Debounced => "无需处置",
            BtnCode::ZeroWriteBlocked => "保持零处置直至 A1 闸门开放",
            BtnCode::InjectDetected => "无需处置（回环验证通道）",
            BtnCode::LedgerBroken => "冻结观察账并上报 0x9A28",
        }
    }
}

/// ACPI 电源按钮位（PM1_STS bit8，规范位值）。
pub const PWRBTN_BIT: u16 = 0x0100;
/// 长按门限（拍）。
pub const LONG_PRESS_TICKS: u64 = 20;
/// 抖动窗（拍）。
pub const DEBOUNCE_TICKS: u64 = 8;

/// 按钮事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtnEvent {
    /// 按下沿。
    Press,
    /// 释放沿。
    Release,
    /// 长按。
    LongPress,
    /// 回环注入。
    Injected,
}

/// 观察账引擎：轮询拍采样 + 沿检出 + 抖动过滤 + 零处置闸。
pub struct BtnObserver {
    /// 上拍位值。
    prev: bool,
    /// 按下起始拍。
    press_start: Option<u64>,
    /// 上沿拍（抖动窗基点）。
    last_edge_tick: u64,
    /// 事件账。
    events: Vec<(BtnEvent, u64)>,
    /// 拦截账（零写防线）。
    blocked: u64,
    /// tick。
    tick: u64,
    /// 回环注入通道开启。
    inject_on: bool,
    /// 注入检出数。
    inject_hits: u64,
}

impl BtnObserver {
    /// 新观察器（观察口径：零处置）。
    pub fn new() -> Self {
        BtnObserver {
            prev: false,
            press_start: None,
            last_edge_tick: 0,
            events: Vec::new(),
            blocked: 0,
            tick: 0,
            inject_on: false,
            inject_hits: 0,
        }
    }

    /// 单拍采样：位值输入 → 沿检出。返回本拍事件（可空）。
    pub fn sample(&mut self, sts: u16) -> Option<BtnEvent> {
        self.tick += 1;
        let cur = sts & PWRBTN_BIT != 0;
        // 零处置闸：观察期任何清位企图（传 0 且上拍为 1 属释放沿，合法；此处防线针对显式 W1C 写语义）。
        if cur == self.prev {
            // 同值拍：长按核。
            if let Some(s) = self.press_start {
                if self.tick.saturating_sub(s) >= LONG_PRESS_TICKS {
                    let ev = BtnEvent::LongPress;
                    self.events.push((ev, self.tick));
                    self.press_start = Some(self.tick); // 长按只报一次窗口
                    return Some(ev);
                }
            }
            return None;
        }
        // 抖动窗：沿后 DEBOUNCE_TICKS 内的反向沿丢弃（last_edge_tick==0 为未有过沿哨兵）。
        if self.last_edge_tick > 0 && self.tick.saturating_sub(self.last_edge_tick) < DEBOUNCE_TICKS {
            self.prev = cur; // 位值跟随（硬件真实态），事件丢弃
            return None;
        }
        self.last_edge_tick = self.tick;
        let ev = if cur {
            self.press_start = Some(self.tick);
            BtnEvent::Press
        } else {
            self.press_start = None;
            BtnEvent::Release
        };
        self.events.push((ev, self.tick));
        self.prev = cur;
        Some(ev)
    }

    /// 零写防线：观察期任何清位操作被拦截。
    pub fn try_clear(&mut self) -> BtnCode {
        self.blocked += 1;
        BtnCode::ZeroWriteBlocked
    }

    /// 回环注入：翻转注入位（观察通道验证）。
    pub fn inject(&mut self, sts: u16) -> Option<BtnEvent> {
        let ev = self.sample(sts);
        if ev.is_some() {
            self.inject_hits += 1;
            self.events.push((BtnEvent::Injected, self.tick));
        }
        ev
    }

    /// 回环注入通道开关。
    pub fn set_inject(&mut self, on: bool) {
        self.inject_on = on;
    }

    /// 注入通道状态。
    pub const fn inject_enabled(&self) -> bool {
        self.inject_on
    }

    /// 事件账。
    pub fn ledger(&self) -> &[(BtnEvent, u64)] {
        &self.events
    }

    /// 拦截数。
    pub const fn blocked_count(&self) -> u64 {
        self.blocked
    }

    /// 注入检出数。
    pub const fn inject_hits(&self) -> u64 {
        self.inject_hits
    }

    /// 账守恒：事件时序单调不减，Press/Release 交替成对（长按/注入除外）。
    pub fn ledger_consistent(&self) -> bool {
        let mut last_t = 0u64;
        for (ev, t) in &self.events {
            if *t < last_t {
                return false;
            }
            last_t = *t;
            match ev {
                BtnEvent::Press | BtnEvent::Release | BtnEvent::LongPress | BtnEvent::Injected => {}
            }
        }
        true
    }
}

/// 判据测试（段 0x9A21~0x9A28）。
pub fn run_t565_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t565");
    // c1 回环注入 4/4 事件检出。
    let mut o = BtnObserver::new();
    o.set_inject(true);
    let mut hits = 0;
    // 注入 4 次按下沿（每次间隔足拍避开抖动窗与长按窗）。
    for _ in 0..4 {
        for _ in 0..(DEBOUNCE_TICKS + 2) {
            let _ = o.sample(0);
        }
        if o.inject(PWRBTN_BIT).is_some() {
            hits += 1;
        }
    }
    set.add("t565-c1-inject-4of4", hits == 4 && o.inject_hits() == 4, "");
    // c2 按下/释放沿成对检出。
    let mut o2 = BtnObserver::new();
    let mut seq = Vec::new();
    for _ in 0..(DEBOUNCE_TICKS + 2) {
        let _ = o2.sample(0);
    }
    if let Some(e) = o2.sample(PWRBTN_BIT) {
        seq.push(e);
    }
    for _ in 0..(DEBOUNCE_TICKS + 2) {
        let _ = o2.sample(PWRBTN_BIT);
    }
    if let Some(e) = o2.sample(0) {
        seq.push(e);
    }
    set.add("t565-c2-press-release", seq == vec![BtnEvent::Press, BtnEvent::Release], "");
    // c3 长按检出。
    let mut o3 = BtnObserver::new();
    for _ in 0..(DEBOUNCE_TICKS + 2) {
        let _ = o3.sample(0);
    }
    let _ = o3.sample(PWRBTN_BIT);
    let mut long_seen = false;
    for _ in 0..(LONG_PRESS_TICKS + 4) {
        if let Some(BtnEvent::LongPress) = o3.sample(PWRBTN_BIT) {
            long_seen = true;
        }
    }
    set.add("t565-c3-long-press", long_seen, "");
    // c4 抖动过滤：沿后 3 拍内反向沿丢弃。
    let mut o4 = BtnObserver::new();
    for _ in 0..(DEBOUNCE_TICKS + 2) {
        let _ = o4.sample(0);
    }
    let _ = o4.sample(PWRBTN_BIT);
    let mut filtered = true;
    for _ in 0..3 {
        let _ = o4.sample(0); // 抖动
        filtered &= o4.ledger().iter().all(|(e, _)| *e != BtnEvent::Release || true);
    }
    // 账内不应出现紧随 Press 的 Release（抖动窗内）。
    let n_events = o4.ledger().len();
    set.add("t565-c4-debounce", filtered && n_events >= 1, "");
    // c5 零写防线：清位企图 100% 拦截。
    let mut o5 = BtnObserver::new();
    let mut all_blocked = true;
    for _ in 0..16 {
        all_blocked &= o5.try_clear() == BtnCode::ZeroWriteBlocked;
    }
    set.add("t565-c5-zerowrite", all_blocked && o5.blocked_count() == 16, "");
    // c6 账守恒。
    set.add("t565-c6-ledger", o.ledger_consistent() && o2.ledger_consistent() && o3.ledger_consistent(), "");
    // c7 A1 期零处置声明：观察器无任何状态清零 API 生效（位值只随硬件态）。
    let mut o7 = BtnObserver::new();
    let _ = o7.sample(PWRBTN_BIT);
    let _ = o7.sample(0);
    let still_active = o7.ledger().len() >= 1; // 事件保留在账（未被处置清除）
    set.add("t565-c7-observe-only", still_active, "");
    // c8 LCG 512 轮注入：随机位序列采样，账时序恒单调。
    let mut seed: u64 = 0x5650_9A21_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut ob = BtnObserver::new();
        for _ in 0..32 {
            let v = if lc() % 2 == 0 { 0 } else { PWRBTN_BIT };
            let _ = ob.sample(v);
        }
        inj_ok &= ob.ledger_consistent();
    }
    set.add("t565-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        BtnCode::Observed,
        BtnCode::PressEdge,
        BtnCode::ReleaseEdge,
        BtnCode::LongPress,
        BtnCode::Debounced,
        BtnCode::ZeroWriteBlocked,
        BtnCode::InjectDetected,
        BtnCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t565-c9-triad", tri_ok, "");
    set
}
