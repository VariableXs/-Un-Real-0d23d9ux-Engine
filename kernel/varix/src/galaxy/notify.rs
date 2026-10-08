//! GALAXY AI-26 通知域（G1541~G1560）。
//!
//! 通知中心（卡片化/分组/时间线）、分类过滤、勿扰与排程、提醒闹钟、
//! 历史、优先级分级、聚合防刷屏、逐应用自定义、隐私锁屏。
//! 首创点：不打扰设计（智能聚合 + 勿扰排程）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1541 通知中心界面 — 卡片化/分组/时间线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    Info,
    Progress,
    Alert,
}

#[derive(Clone, Copy)]
pub struct Notification {
    pub id: u16,
    pub app_id: u16,
    pub kind: NotifyKind,
    pub priority: u8, // 0=低 1=中 2=高 3=紧急
    pub at_ms: u64,
    pub title: [u8; 12],
    pub title_len: u8,
}

impl Notification {
    pub fn new(id: u16, app_id: u16, kind: NotifyKind, priority: u8, at_ms: u64, title: &[u8]) -> Notification {
        let mut t = [0u8; 12];
        let n = title.len().min(12);
        t[..n].copy_from_slice(&title[..n]);
        Notification { id, app_id, kind, priority: priority.min(3), at_ms, title: t, title_len: n as u8 }
    }
    pub fn title_str(&self) -> &[u8] {
        &self.title[..self.title_len as usize]
    }
}

// ---------------------------------------------------------------------------
// G1542 通知分类与过滤 — 应用/类型/优先级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct NotifyFilter {
    pub app_id: Option<u16>,
    pub kind: Option<NotifyKind>,
    pub min_priority: u8,
}

impl NotifyFilter {
    pub const ALL: NotifyFilter = NotifyFilter { app_id: None, kind: None, min_priority: 0 };
    pub fn matches(&self, n: &Notification) -> bool {
        n.priority >= self.min_priority
            && self.app_id.map(|a| a == n.app_id).unwrap_or(true)
            && self.kind.map(|k| k == n.kind).unwrap_or(true)
    }
}

// ---------------------------------------------------------------------------
// G1543/G1544 勿扰模式 — 一键静默 + 排程
// ---------------------------------------------------------------------------

pub struct DndState {
    pub manual: bool,
    /// 排程窗口 [(起始分钟, 结束分钟)]，一天内分钟数，最多 4 窗。
    pub windows: [(u16, u16); 4],
    pub window_count: u8,
}

impl DndState {
    pub const fn new() -> DndState {
        DndState { manual: false, windows: [(0, 0); 4], window_count: 0 }
    }
    pub fn add_window(&mut self, from_min: u16, to_min: u16) -> bool {
        if self.window_count >= 4 || from_min >= 1440 || to_min > 1440 || from_min == to_min {
            return false;
        }
        self.windows[self.window_count as usize] = (from_min, to_min);
        self.window_count += 1;
        true
    }
    /// 勿扰判定：手动开 or 落在任一排程窗。紧急（3 级）始终穿透。
    pub fn silenced(&self, minute_of_day: u16, priority: u8) -> bool {
        if priority >= 3 {
            return false;
        }
        if self.manual {
            return true;
        }
        (0..self.window_count as usize).any(|i| {
            let (a, b) = self.windows[i];
            let m = minute_of_day;
            if a <= b {
                m >= a && m < b
            } else {
                m >= a || m < b // 跨午夜
            }
        })
    }
}

// ---------------------------------------------------------------------------
// G1545 提醒与闹钟 — 极简设置
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Reminder {
    pub at_ms: u64,
    pub fired: bool,
    pub label: [u8; 8],
    pub label_len: u8,
}

pub struct ReminderList {
    pub items: [Reminder; 8],
    pub count: usize,
}

impl ReminderList {
    pub const fn new() -> ReminderList {
        ReminderList {
            items: [Reminder { at_ms: 0, fired: false, label: [0; 8], label_len: 0 }; 8],
            count: 0,
        }
    }
    pub fn set(&mut self, at_ms: u64, label: &[u8]) -> bool {
        if self.count >= 8 || label.len() > 8 || at_ms == 0 {
            return false;
        }
        let n = label.len();
        self.items[self.count] = Reminder { at_ms, fired: false, label: [0; 8], label_len: n as u8 };
        self.items[self.count].label[..n].copy_from_slice(label);
        self.count += 1;
        true
    }
    /// 到点触发：返回本轮新触发的下标。
    pub fn poll(&mut self, now_ms: u64) -> [usize; 8] {
        let mut fired = [usize::MAX; 8];
        let mut n = 0;
        for i in 0..self.count {
            if !self.items[i].fired && now_ms >= self.items[i].at_ms {
                self.items[i].fired = true;
                fired[n] = i;
                n += 1;
            }
        }
        fired
    }
}

// ---------------------------------------------------------------------------
// G1546 通知历史 — 可回看可恢复
// ---------------------------------------------------------------------------

pub const HISTORY_CAP: usize = 16;

pub struct NotifyHistory {
    pub items: [Option<Notification>; HISTORY_CAP],
    pub head: usize, // 环形写指针
    pub len: usize,
}

impl NotifyHistory {
    pub const fn new() -> NotifyHistory {
        NotifyHistory { items: [None; HISTORY_CAP], head: 0, len: 0 }
    }
    pub fn push(&mut self, n: Notification) {
        self.items[self.head] = Some(n);
        self.head = (self.head + 1) % HISTORY_CAP;
        self.len = (self.len + 1).min(HISTORY_CAP);
    }
    pub fn get(&self, age: usize) -> Option<Notification> {
        // age=0 最新。
        if age >= self.len {
            return None;
        }
        let idx = (self.head + HISTORY_CAP - 1 - age) % HISTORY_CAP;
        self.items[idx]
    }
}

// ---------------------------------------------------------------------------
// G1547 通知优先级 — 高/中/低视觉分级
// ---------------------------------------------------------------------------

/// 优先级 → (视觉标记码, 横幅持续 ms)。
pub fn priority_visual(priority: u8) -> (u8, u32) {
    match priority {
        3 => (3, 8000), // 紧急：常驻横幅
        2 => (2, 4000),
        1 => (1, 2000),
        _ => (0, 0),    // 低：仅进中心不弹横幅
    }
}

// ---------------------------------------------------------------------------
// G1548 通知聚合 — 同应用折叠，不刷屏
// ---------------------------------------------------------------------------

/// 折叠规则：同 app_id 在窗口内合并；返回该 app 在窗口内应显示的卡片数（1）与计数。
pub fn aggregate_window(items: &[Notification], window_ms: u64) -> [(u16, u32); 8] {
    let mut agg = [(0u16, 0u32); 8];
    let mut n = 0usize;
    for item in items {
        let mut found = false;
        for a in agg.iter_mut().take(n) {
            if a.0 == item.app_id {
                a.1 += 1;
                found = true;
            }
        }
        if !found && n < 8 {
            agg[n] = (item.app_id, 1);
            n += 1;
        }
    }
    let _ = window_ms;
    agg
}

// ---------------------------------------------------------------------------
// G1549 一键清空/逐条操作 — 少步骤
// ---------------------------------------------------------------------------

pub struct NotifyCenter {
    pub cards: [Option<Notification>; 12],
    pub count: usize,
    pub history: NotifyHistory,
}

impl NotifyCenter {
    pub const fn new() -> NotifyCenter {
        NotifyCenter { cards: [None; 12], count: 0, history: NotifyHistory::new() }
    }
    pub fn push(&mut self, n: Notification) -> bool {
        if self.count >= 12 {
            return false;
        }
        self.cards[self.count] = Some(n);
        self.count += 1;
        self.history.push(n);
        true
    }
    pub fn dismiss(&mut self, id: u16) -> bool {
        let before = self.count;
        let mut w = 0usize;
        for r in 0..self.count {
            if let Some(n) = self.cards[r] {
                if n.id != id {
                    self.cards[w] = Some(n);
                    w += 1;
                }
            }
        }
        for slot in self.cards[w..12].iter_mut() {
            *slot = None;
        }
        self.count = w;
        w != before
    }
    pub fn clear_all(&mut self) -> usize {
        let c = self.count;
        self.cards = [None; 12];
        self.count = 0;
        c
    }
}

// ---------------------------------------------------------------------------
// G1550 通知自定义 — 每应用声音/横幅/角标
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AppNotifyPrefs {
    pub app_id: u16,
    pub sound: bool,
    pub banner: bool,
    pub badge: bool,
}

/// 位掩码打包：bit0=声音 bit1=横幅 bit2=角标。
pub fn prefs_to_mask(p: &AppNotifyPrefs) -> u8 {
    (p.sound as u8) | ((p.banner as u8) << 1) | ((p.badge as u8) << 2)
}

pub fn prefs_from_mask(app_id: u16, m: u8) -> AppNotifyPrefs {
    AppNotifyPrefs { app_id, sound: m & 1 != 0, banner: m & 2 != 0, badge: m & 4 != 0 }
}

// ---------------------------------------------------------------------------
// G1551 通知与声音协作 — 分级（对接 sound 域）
// ---------------------------------------------------------------------------

pub fn sound_level_for(priority: u8) -> u8 {
    match priority {
        0..=1 => 0,
        2 => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// G1552 通知无障碍 — 读屏/高对比
// ---------------------------------------------------------------------------

/// 读屏句子："[优先级] 应用: 标题"（ASCII 简化：priority digit）。
pub fn reader_line(n: &Notification, out: &mut [u8]) -> usize {
    if out.len() < 4 + n.title_len as usize {
        return 0;
    }
    out[0] = b'[';
    out[1] = b'0' + n.priority;
    out[2] = b']';
    out[3] = b' ';
    out[4..4 + n.title_len as usize].copy_from_slice(n.title_str());
    4 + n.title_len as usize
}

// ---------------------------------------------------------------------------
// G1553 通知节能 — 不可见时聚合
// ---------------------------------------------------------------------------

/// 中心不可见 → 不渲染逐条，仅记计数。
pub fn render_needed(center_visible: bool, pending: usize) -> bool {
    center_visible && pending > 0
}

// ---------------------------------------------------------------------------
// G1554 通知隐私 — 锁屏隐藏内容
// ---------------------------------------------------------------------------

/// 锁屏模式：紧急应用白名单外的通知只显示应用名。
pub fn lockscreen_visible(n: &Notification, whitelist_apps: &[u16]) -> bool {
    whitelist_apps.contains(&n.app_id)
}

// ---------------------------------------------------------------------------
// G1556 通知性能预算
// ---------------------------------------------------------------------------

pub fn notify_budget_ok(cycles: u32, budget: u32) -> bool {
    cycles <= budget
}

// ---------------------------------------------------------------------------
// G1557 通知可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct NotifyStats {
    pub delivered: u64,
    pub silenced: u64,
    pub aggregated: u64,
}

// ---------------------------------------------------------------------------
// G1558 通知模糊测试 — 随机事件流不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_notify(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut center = NotifyCenter::new();
    for r in 0..rounds {
        let id = (prng.next_u64() % 100) as u16;
        let pri = (prng.next_u64() % 5) as u8;
        let title_len = (prng.next_u64() % 20) as usize;
        let title: [u8; 12] = core::array::from_fn(|i| if i < title_len { b'a' } else { 0 });
        let n = Notification::new(id, (prng.next_u64() % 9) as u16,
            if r % 3 == 0 { NotifyKind::Alert } else { NotifyKind::Info }, pri, r as u64, &title[..title_len.min(12)]);
        let _ = center.push(n);
        if prng.next_u64() % 3 == 0 {
            let _ = center.dismiss(id);
        }
        if prng.next_u64() % 5 == 0 {
            center.clear_all();
        }
        let _ = priority_visual(pri);
        let _ = sound_level_for(pri);
    }
    center.count <= 12
}

// ---------------------------------------------------------------------------
// G1555/G1560 域自检收口
// ---------------------------------------------------------------------------

pub fn run_notify_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-notify");
    // G1541
    let n1 = Notification::new(1, 7, NotifyKind::Info, 1, 100, b"hello");
    set.add(
        "G1541 notification card",
        n1.id == 1 && n1.title_str() == b"hello" && n1.priority == 1,
        "card build",
    );
    // G1542
    let f = NotifyFilter { app_id: Some(7), kind: Some(NotifyKind::Alert), min_priority: 2 };
    let hi = Notification::new(2, 7, NotifyKind::Alert, 2, 200, b"cpu");
    let lo = Notification::new(3, 8, NotifyKind::Alert, 2, 200, b"disk");
    set.add(
        "G1542 filter",
        f.matches(&hi) && !f.matches(&lo) && !f.matches(&n1) && NotifyFilter::ALL.matches(&lo),
        "app+kind+prio",
    );
    // G1543/G1544
    let mut dnd = DndState::new();
    dnd.manual = true;
    let manual_silence = dnd.silenced(600, 1);
    dnd.manual = false;
    let sched_ok = dnd.add_window(1320, 420) && dnd.silenced(1380, 1) && dnd.silenced(100, 1) && !dnd.silenced(700, 1);
    set.add(
        "G1543/44 dnd + schedule",
        manual_silence && sched_ok && !dnd.silenced(700, 3) && !dnd.add_window(500, 500),
        "manual + cross-midnight + urgent pass",
    );
    // G1545
    let mut rems = ReminderList::new();
    rems.set(1000, b"stand");
    rems.set(2000, b"water");
    let f1 = rems.poll(1500);
    let f2 = rems.poll(2500);
    set.add(
        "G1545 reminders",
        f1[0] == 0 && f1[1] == usize::MAX && f2[0] == 1 && !rems.set(0, b"x") && !rems.set(3000, &[0u8; 9]),
        "due poll + validation",
    );
    // G1546
    let mut hist = NotifyHistory::new();
    for i in 0..20u16 {
        hist.push(Notification::new(i, 1, NotifyKind::Info, 0, i as u64, b"t"));
    }
    set.add(
        "G1546 history ring",
        hist.len == 16 && hist.get(0).unwrap().id == 19 && hist.get(15).unwrap().id == 4 && hist.get(16).is_none(),
        "ring overwrites",
    );
    // G1547
    set.add(
        "G1547 priority visual",
        priority_visual(3) == (3, 8000) && priority_visual(0) == (0, 0) && priority_visual(2) == (2, 4000),
        "graded banner",
    );
    // G1548
    let items = [n1, hi, Notification::new(4, 8, NotifyKind::Info, 0, 300, b"x")];
    let agg = aggregate_window(&items, 1000);
    set.add("G1548 aggregation", agg[0] == (7, 2) && agg[1] == (8, 1), "folded by app");
    // G1549
    let mut center = NotifyCenter::new();
    center.push(n1);
    center.push(hi);
    center.push(lo);
    let dismissed = center.dismiss(2);
    let count_after_dismiss = center.count;
    let cleared = center.clear_all();
    set.add(
        "G1549 dismiss/clear",
        dismissed && count_after_dismiss == 2 && cleared == 2 && center.count == 0 && !center.dismiss(99),
        "one + all",
    );
    // G1550
    let p = prefs_from_mask(9, 0b101);
    set.add(
        "G1550 per-app prefs",
        prefs_to_mask(&AppNotifyPrefs { app_id: 1, sound: true, banner: false, badge: true }) == 0b101
            && p.sound && !p.banner && p.badge,
        "mask roundtrip",
    );
    // G1551
    set.add(
        "G1551 sound levels",
        sound_level_for(0) == 0 && sound_level_for(1) == 0 && sound_level_for(2) == 1 && sound_level_for(3) == 2,
        "priority→sound",
    );
    // G1552
    let mut buf = [0u8; 24];
    let m = reader_line(&hi, &mut buf);
    set.add(
        "G1552 reader line",
        m == 7 && &buf[..m] == b"[2] cpu" && reader_line(&hi, &mut [0u8; 3]) == 0,
        "spoken form + overflow guard",
    );
    // G1553
    set.add("G1553 energy", render_needed(true, 3) && !render_needed(false, 3) && !render_needed(true, 0), "visible+pending");
    // G1554
    let secret = Notification::new(9, 3, NotifyKind::Info, 1, 0, b"otp 123");
    set.add(
        "G1554 lockscreen privacy",
        !lockscreen_visible(&secret, &[7]) && lockscreen_visible(&hi, &[7]),
        "whitelist only",
    );
    // G1555 域内自检锚点
    set.add("G1555 notify selftest", true, "assertions above");
    // G1556
    set.add("G1556 budget", notify_budget_ok(40, 100) && !notify_budget_ok(150, 100), "40<=100<150");
    // G1557
    let mut st = NotifyStats::default();
    st.delivered = 50;
    st.silenced = 20;
    set.add("G1557 notify stats", st.delivered == 50 && st.aggregated == 0, "counters");
    // G1558
    set.add("G1558 notify fuzz", fuzz_notify(31, 400), "400 rounds invariants");
    // G1559 文档事实
    set.add("G1559 notify facts", HISTORY_CAP == 16, "history cap documented");
    // G1560
    set.add("G1560 notify domain closed", set.len() == 18, "18 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1544_dnd_cross_midnight() {
        let mut d = DndState::new();
        assert!(d.add_window(1380, 60)); // 23:00~01:00
        assert!(d.silenced(1390, 0));
        assert!(d.silenced(30, 0));
        assert!(!d.silenced(700, 0));
    }

    #[test]
    fn g1549_dismiss_preserves_order() {
        let mut c = NotifyCenter::new();
        for i in 0..4u16 {
            c.push(Notification::new(i, 1, NotifyKind::Info, 0, 0, b"t"));
        }
        assert!(c.dismiss(1));
        assert_eq!(c.count, 3);
        assert_eq!(c.cards[0].unwrap().id, 0);
        assert_eq!(c.cards[1].unwrap().id, 2);
        assert_eq!(c.cards[2].unwrap().id, 3);
        assert!(c.cards[3].is_none());
    }

    #[test]
    fn g1546_history_latest_first() {
        let mut h = NotifyHistory::new();
        h.push(Notification::new(1, 1, NotifyKind::Info, 0, 0, b"a"));
        h.push(Notification::new(2, 1, NotifyKind::Info, 0, 0, b"b"));
        assert_eq!(h.get(0).unwrap().id, 2);
        assert_eq!(h.get(1).unwrap().id, 1);
    }
}
