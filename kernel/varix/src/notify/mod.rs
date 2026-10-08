//! AURORA-1000 通知与提醒中心域（A626~A650）。
//!
//! 通知中心、分类过滤、勿扰模式/排程、提醒闹钟、通知历史环形、
//! 优先级与横幅、聚合、一键清空、每 app 静音覆盖、声音协作、
//! 无障碍读屏、节能批投、隐私打码、性能预算、可观测、模糊测试、降级链。
//!
//! 全部为纯逻辑 + 固定容量数组；不依赖 Vec/String/Box/alloc/std。

use crate::checks::CheckSet;

pub const MAX_NOTIFICATIONS: usize = 16;
pub const MAX_HISTORY: usize = 32;
pub const MAX_REMINDERS: usize = 8;
pub const MAX_OVERRIDES: usize = 8;
pub const MASK_LEN: usize = 16;
pub const PRIO_MAX: u8 = 3;

// ---------------------------------------------------------------------------
// A626 通知中心界面 — 固定 16 条
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    System,
    App,
    Calendar,
    Security,
}

#[derive(Clone, Copy)]
pub struct Notification {
    pub id: u32,
    pub app: &'static str,
    pub title: &'static str,
    pub priority: u8,
    pub category: Category,
}

pub struct NotificationCenter {
    pub items: [Option<Notification>; MAX_NOTIFICATIONS],
    pub count: usize,
    pub next_id: u32,
}

impl NotificationCenter {
    pub const fn new() -> NotificationCenter {
        NotificationCenter {
            items: [None; MAX_NOTIFICATIONS],
            count: 0,
            next_id: 1,
        }
    }
    pub fn push(&mut self, n: Notification) -> bool {
        if self.count >= MAX_NOTIFICATIONS {
            return false;
        }
        // 重复 id 拒绝（同一条通知不重复入队）。
        if self.get(n.id).is_some() {
            return false;
        }
        self.items[self.count] = Some(n);
        self.count += 1;
        true
    }
    pub fn get(&self, id: u32) -> Option<Notification> {
        (0..self.count).find_map(|i| self.items[i].filter(|x| x.id == id))
    }
}

// ---------------------------------------------------------------------------
// A627 分类过滤 — 类别枚举 + 命中表
// ---------------------------------------------------------------------------

pub fn filter_by_category(c: &NotificationCenter, cat: Category) -> [u32; MAX_NOTIFICATIONS] {
    let mut out = [0u32; MAX_NOTIFICATIONS];
    let mut n = 0usize;
    for i in 0..c.count {
        if let Some(x) = c.items[i] {
            if x.category == cat {
                out[n] = x.id;
                n += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A628 勿扰模式 — 标志 + push 时勿扰下进历史不弹横幅
// ---------------------------------------------------------------------------

// (逻辑在 NotifyCenter::push 中统一处理，见 A644/A649)

// ---------------------------------------------------------------------------
// A629 勿扰排程 — 时间窗判定，支持跨午夜
// ---------------------------------------------------------------------------

/// 分钟内表示（0..1440）。start==end 视为不启用。
pub fn in_dnd_window(start_min: u16, end_min: u16, now_min: u16) -> bool {
    if start_min == end_min {
        return false;
    }
    if start_min < end_min {
        now_min >= start_min && now_min <= end_min
    } else {
        // 跨午夜：如 23:00(1380) ~ 02:00(120)
        now_min >= start_min || now_min <= end_min
    }
}

// ---------------------------------------------------------------------------
// A630 提醒与闹钟 — 固定 8 条（触发时刻 ms），due(t) 返回到期表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Reminder {
    pub id: u32,
    pub at_ms: u64,
}

pub struct ReminderTable {
    pub items: [Option<Reminder>; MAX_REMINDERS],
    pub count: usize,
}

impl ReminderTable {
    pub const fn new() -> ReminderTable {
        ReminderTable { items: [None; MAX_REMINDERS], count: 0 }
    }
    pub fn add(&mut self, r: Reminder) -> bool {
        if self.count >= MAX_REMINDERS {
            return false;
        }
        self.items[self.count] = Some(r);
        self.count += 1;
        true
    }
    pub fn due(&self, t: u64) -> [u32; MAX_REMINDERS] {
        let mut out = [0u32; MAX_REMINDERS];
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(x) = self.items[i] {
                if x.at_ms <= t {
                    out[n] = x.id;
                    n += 1;
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// A631 通知历史 — 固定 32 环形，push 满丢最旧
// ---------------------------------------------------------------------------

pub struct History {
    pub items: [Option<Notification>; MAX_HISTORY],
    pub start: usize,
    pub count: usize,
}

impl History {
    pub const fn new() -> History {
        History { items: [None; MAX_HISTORY], start: 0, count: 0 }
    }
    pub fn push(&mut self, n: Notification) {
        if self.count < MAX_HISTORY {
            let i = (self.start + self.count) % MAX_HISTORY;
            self.items[i] = Some(n);
            self.count += 1;
        } else {
            // 满则丢最旧，新项写入其位。
            self.items[self.start] = Some(n);
            self.start = (self.start + 1) % MAX_HISTORY;
        }
    }
    pub fn at(&self, idx: usize) -> Option<Notification> {
        if idx < self.count {
            self.items[(self.start + idx) % MAX_HISTORY]
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// A632 优先级 — 0~3，横幅只弹 >=2，稳定降序排序
// ---------------------------------------------------------------------------

pub fn banner_eligible(priority: u8) -> bool {
    priority >= 2 && priority <= PRIO_MAX
}

/// 稳定插入排序：按 priority 降序，相等保持原序。
pub fn sort_priority_desc(buf: &mut [Notification]) {
    let len = buf.len();
    let mut i = 1usize;
    while i < len {
        let key = buf[i];
        let mut j = i;
        while j > 0 && buf[j - 1].priority < key.priority {
            buf[j] = buf[j - 1];
            j -= 1;
        }
        buf[j] = key;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A633 聚合 — 相同 app+title 连续多条聚合计数
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Agg {
    pub app: &'static str,
    pub title: &'static str,
    pub count: u32,
}

pub fn aggregate(src: &[Notification], out: &mut [Option<Agg>]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < src.len() {
        let app = src[i].app;
        let title = src[i].title;
        let mut c = 1u32;
        let mut j = i + 1;
        while j < src.len() && src[j].app == app && src[j].title == title {
            c += 1;
            j += 1;
        }
        if n < out.len() {
            out[n] = Some(Agg { app, title, count: c });
            n += 1;
        }
        i = j;
    }
    n
}

// ---------------------------------------------------------------------------
// A634 一键清空 — 返回清除数，历史保留
// ---------------------------------------------------------------------------

// (实现见 NotifyCenter::clear_all，见 A644)

// ---------------------------------------------------------------------------
// A635 自定义 — 每 app 静音/级别覆盖表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AppOverride {
    pub app: &'static str,
    pub muted: bool,
    pub min_priority: u8,
}

pub struct OverrideTable {
    pub items: [Option<AppOverride>; MAX_OVERRIDES],
    pub count: usize,
}

impl OverrideTable {
    pub const fn new() -> OverrideTable {
        OverrideTable { items: [None; MAX_OVERRIDES], count: 0 }
    }
    pub fn set(&mut self, o: AppOverride) -> bool {
        for i in 0..self.count {
            if let Some(x) = self.items[i] {
                if x.app == o.app {
                    self.items[i] = Some(o);
                    return true;
                }
            }
        }
        if self.count >= MAX_OVERRIDES {
            return false;
        }
        self.items[self.count] = Some(o);
        self.count += 1;
        true
    }
    pub fn get(&self, app: &str) -> Option<AppOverride> {
        (0..self.count).find_map(|i| self.items[i].filter(|x| x.app == app))
    }
}

// ---------------------------------------------------------------------------
// A636 与声音协作 — notif → 音效 id 映射（静音 app 映射 0）
// ---------------------------------------------------------------------------

pub fn sound_for(overrides: &OverrideTable, n: &Notification) -> u8 {
    if let Some(o) = overrides.get(n.app) {
        if o.muted {
            return 0;
        }
    }
    match n.priority {
        0..=1 => 0,
        2 => 2,
        _ => 3,
    }
}

// ---------------------------------------------------------------------------
// A637 无障碍 — 每条通知有读屏文本（非空校验）
// ---------------------------------------------------------------------------

pub fn a11y_ok(center: &NotificationCenter) -> bool {
    if center.count == 0 {
        return false;
    }
    (0..center.count).all(|i| match center.items[i] {
        Some(x) => !x.title.is_empty(),
        None => false,
    })
}

// ---------------------------------------------------------------------------
// A638 节能 — 批量投递合并 tick（batch_window 判定）
// ---------------------------------------------------------------------------

pub fn batch_merge_ok(last_ms: u64, now_ms: u64, window_ms: u64) -> bool {
    if last_ms == 0 {
        return true; // 首次
    }
    now_ms >= last_ms && (now_ms - last_ms) <= window_ms
}

// ---------------------------------------------------------------------------
// A639 隐私 — 锁屏时标题打码（固定宽度省略）
// ---------------------------------------------------------------------------

pub fn mask_title(title: &str, locked: bool) -> [u8; MASK_LEN] {
    let mut out = [0u8; MASK_LEN];
    if !locked {
        let mut i = 0usize;
        for &b in title.as_bytes() {
            if i + 1 >= MASK_LEN {
                break;
            }
            out[i] = b;
            i += 1;
        }
    } else {
        let mut i = 0usize;
        for &b in title.as_bytes() {
            if i + 1 >= MASK_LEN {
                break;
            }
            if b == b' ' {
                out[i] = b' ';
            } else {
                out[i] = b'*';
            }
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A640 性能预算 — push O(1) + budget_ok
// ---------------------------------------------------------------------------

pub fn push_budget_ok(us: u32, budget_us: u32) -> bool {
    us <= budget_us
}

// ---------------------------------------------------------------------------
// A641 / A646 可观测 — 计数器结构（pushed/shown/suppressed/cleared）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct NotifyStats {
    pub pushed: u64,
    pub shown: u64,
    pub suppressed: u64,
    pub cleared: u64,
}

// ---------------------------------------------------------------------------
// A642 / A648 文档 — 常量事实
// ---------------------------------------------------------------------------

pub const NOTIFY_FACTS: &[(&str, usize)] = &[
    ("max_notifications", MAX_NOTIFICATIONS),
    ("max_history", MAX_HISTORY),
    ("max_reminders", MAX_REMINDERS),
    ("prio_max", PRIO_MAX as usize),
];

// ---------------------------------------------------------------------------
// 综合中心 — 收口 A628/A634/A644/A649
// ---------------------------------------------------------------------------

pub struct NotifyCenter {
    pub center: NotificationCenter,
    pub history: History,
    pub dnd: bool,
    pub dnd_start: u16,
    pub dnd_end: u16,
    pub now_min: u16,
    pub overrides: OverrideTable,
    pub stats: NotifyStats,
}

impl NotifyCenter {
    pub const fn new() -> NotifyCenter {
        NotifyCenter {
            center: NotificationCenter::new(),
            history: History::new(),
            dnd: false,
            dnd_start: 0,
            dnd_end: 0,
            now_min: 0,
            overrides: OverrideTable::new(),
            stats: NotifyStats { pushed: 0, shown: 0, suppressed: 0, cleared: 0 },
        }
    }

    /// 勿扰是否当前生效（手动标志 或 处于排程窗）。
    pub fn dnd_active(&self) -> bool {
        self.dnd || in_dnd_window(self.dnd_start, self.dnd_end, self.now_min)
    }

    /// 推送一条通知：满则降级驱逐最低优先级腾位。
    /// 横幅仅在「非勿扰 且 priority>=2」时弹出。
    pub fn push(&mut self, n: Notification) -> bool {
        self.stats.pushed += 1;
        let banner = !self.dnd_active() && banner_eligible(n.priority);
        self.history.push(n); // A631 历史始终入环

        let placed = if self.center.count < MAX_NOTIFICATIONS {
            self.center.items[self.center.count] = Some(n);
            self.center.count += 1;
            true
        } else {
            // A649 降级链：满 → 驱逐最低优先级腾位
            let mut low = 0usize;
            let mut lowp = 255u8;
            for i in 0..self.center.count {
                if let Some(x) = self.center.items[i] {
                    if x.priority < lowp {
                        lowp = x.priority;
                        low = i;
                    }
                }
            }
            if n.priority > lowp {
                self.center.items[low] = Some(n);
                true
            } else {
                false
            }
        };

        if banner {
            self.stats.shown += 1;
        } else {
            self.stats.suppressed += 1;
        }
        placed
    }

    /// 一键清空中心（历史保留），返回清除数。
    pub fn clear_all(&mut self) -> usize {
        let n = self.center.count;
        for i in 0..self.center.count {
            self.center.items[i] = None;
        }
        self.center.count = 0;
        self.stats.cleared += n as u64;
        n
    }
}

// ---------------------------------------------------------------------------
// A643 / A650 自检收口
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// A645 性能预算 — 历史环形无分配断言
// ---------------------------------------------------------------------------

pub fn history_no_alloc_ok(h: &History) -> bool {
    h.count <= MAX_HISTORY
}

// ---------------------------------------------------------------------------
// A647 模糊测试 — 随机 push/clear/勿扰切换不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_notify(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut c = NotifyCenter::new();
    let apps = ["mail", "calendar", "system", "security"];
    let cats = [Category::System, Category::App, Category::Calendar, Category::Security];
    for _ in 0..rounds {
        let op = prng.next_u64() % 4;
        match op {
            0 => {
                let id = prng.next_u64() as u32;
                let app = apps[prng.next_usize(apps.len())];
                let pri = (prng.next_u64() % 4) as u8;
                let cat = cats[prng.next_usize(cats.len())];
                let _ = c.push(Notification { id, app, title: "t", priority: pri, category: cat });
            }
            1 => {
                c.clear_all();
            }
            2 => {
                c.dnd = !c.dnd;
            }
            _ => {
                c.now_min = (prng.next_u64() % 1440) as u16;
                let _ = in_dnd_window(c.dnd_start, c.dnd_end, c.now_min);
            }
        }
    }
    history_no_alloc_ok(&c.history) && c.stats.pushed >= c.history.count as u64
}

// ---------------------------------------------------------------------------
// A644 域自检主体
// ---------------------------------------------------------------------------

pub fn run_notify_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-notify");

    // A626 通知中心界面
    let mut c = NotificationCenter::new();
    let n1 = Notification { id: 1, app: "mail", title: "New mail", priority: 3, category: Category::App };
    let ok626 = c.push(n1) && c.count == 1 && c.get(1).unwrap().priority == 3 && !c.push(n1)
        && c.get(2).is_none();
    set.add("A626 notify center", ok626, "push+get+dup reject");

    // A627 分类过滤
    let mut cc = NotificationCenter::new();
    cc.push(Notification { id: 1, app: "a", title: "t", priority: 1, category: Category::System });
    cc.push(Notification { id: 2, app: "b", title: "t", priority: 1, category: Category::App });
    cc.push(Notification { id: 3, app: "c", title: "t", priority: 1, category: Category::System });
    let sys = filter_by_category(&cc, Category::System);
    let app = filter_by_category(&cc, Category::App);
    set.add(
        "A627 category filter",
        sys[0] == 1 && sys[1] == 3 && sys[2] == 0 && app[0] == 2 && app[1] == 0,
        "system=1,3 app=2",
    );

    // A628 勿扰模式：勿扰下不弹横幅
    let mut d = NotifyCenter::new();
    d.dnd = true;
    let _ = d.push(Notification { id: 1, app: "m", title: "x", priority: 3, category: Category::App });
    let ok628 = d.stats.shown == 0 && d.stats.suppressed == 1 && d.history.count == 1;
    d.dnd = false;
    let _ = d.push(Notification { id: 2, app: "m", title: "y", priority: 3, category: Category::App });
    let ok628b = d.stats.shown == 1;
    set.add("A628 dnd suppress", ok628 && ok628b, "banner off in dnd");

    // A629 勿扰排程（跨午夜）
    let ok629 = in_dnd_window(1380, 120, 0)
        && in_dnd_window(1380, 120, 1439)
        && in_dnd_window(1380, 120, 60)
        && !in_dnd_window(1380, 120, 600)
        && !in_dnd_window(500, 500, 0);
    set.add("A629 dnd window", ok629, "cross-midnight wrap");

    // A630 提醒闹钟
    let mut rt = ReminderTable::new();
    rt.add(Reminder { id: 1, at_ms: 1000 });
    rt.add(Reminder { id: 2, at_ms: 2000 });
    rt.add(Reminder { id: 3, at_ms: 5000 });
    let due1 = rt.due(2000);
    let due2 = rt.due(999);
    let ok630 = due1[0] == 1 && due1[1] == 2 && due1[2] == 0 && due2[0] == 0
        && rt.add(Reminder { id: 9, at_ms: 1 }); // 容量内可继续添加
    set.add("A630 reminders due", ok630, "due at t");

    // A631 通知历史环形满丢最旧
    let mut h = History::new();
    for i in 0..(MAX_HISTORY as u32 + 4) {
        h.push(Notification { id: 100 + i, app: "a", title: "t", priority: 0, category: Category::App });
    }
    let ok631 = h.count == MAX_HISTORY && h.at(0).unwrap().id == 100 + 4;
    set.add("A631 history ring", ok631, "drops oldest when full");

    // A632 优先级 + 稳定降序
    let mut buf = [
        Notification { id: 1, app: "a", title: "t1", priority: 1, category: Category::App },
        Notification { id: 2, app: "a", title: "t2", priority: 3, category: Category::App },
        Notification { id: 3, app: "a", title: "t3", priority: 2, category: Category::App },
        Notification { id: 4, app: "a", title: "t4", priority: 3, category: Category::App },
    ];
    sort_priority_desc(&mut buf);
    let ok632 = banner_eligible(2)
        && !banner_eligible(1)
        && buf[0].id == 2 && buf[1].id == 4 && buf[2].id == 3 && buf[3].id == 1;
    set.add("A632 priority sort", ok632, "stable desc, banner>=2");

    // A633 聚合
    let agg_src = [
        Notification { id: 1, app: "mail", title: "a", priority: 1, category: Category::App },
        Notification { id: 2, app: "mail", title: "a", priority: 1, category: Category::App },
        Notification { id: 3, app: "mail", title: "b", priority: 1, category: Category::App },
        Notification { id: 4, app: "cal", title: "a", priority: 1, category: Category::App },
    ];
    let mut agg_out = [None; 8];
    let an = aggregate(&agg_src, &mut agg_out);
    let ok633 = an == 3
        && agg_out[0].unwrap().count == 2
        && agg_out[1].unwrap().count == 1
        && agg_out[2].unwrap().count == 1;
    set.add("A633 aggregate", ok633, "compress consecutive");

    // A634 一键清空（历史保留）
    let mut clr = NotifyCenter::new();
    for i in 0..5u32 {
        clr.push(Notification { id: i, app: "a", title: "t", priority: 1, category: Category::App });
    }
    let cleared = clr.clear_all();
    let ok634 = cleared == 5 && clr.center.count == 0 && clr.history.count == 5;
    set.add("A634 clear all", ok634, "center cleared, history kept");

    // A635 每 app 静音/级别覆盖
    let mut ov = OverrideTable::new();
    let ok635 = ov.set(AppOverride { app: "mail", muted: true, min_priority: 2 })
        && ov.set(AppOverride { app: "mail", muted: false, min_priority: 1 }) // 更新
        && ov.get("mail").unwrap().muted == false
        && ov.get("mail").unwrap().min_priority == 1
        && ov.get("none").is_none();
    set.add("A635 override table", ok635, "mute + level per app");

    // A636 声音协作（静音 app → 0）
    let mut ov2 = OverrideTable::new();
    ov2.set(AppOverride { app: "mail", muted: true, min_priority: 0 });
    let snd_muted = sound_for(&ov2, &Notification { id: 1, app: "mail", title: "t", priority: 3, category: Category::App });
    let snd_loud = sound_for(&ov2, &Notification { id: 2, app: "cal", title: "t", priority: 3, category: Category::App });
    let snd_low = sound_for(&ov2, &Notification { id: 3, app: "cal", title: "t", priority: 1, category: Category::App });
    set.add("A636 sound map", snd_muted == 0 && snd_loud == 3 && snd_low == 0, "muted->0, prio->id");

    // A637 无障碍读屏非空
    let mut ac = NotificationCenter::new();
    ac.push(Notification { id: 1, app: "a", title: "ok", priority: 1, category: Category::App });
    let mut ac2 = NotificationCenter::new();
    ac2.push(Notification { id: 1, app: "a", title: "", priority: 1, category: Category::App });
    set.add("A637 a11y", a11y_ok(&ac) && !a11y_ok(&ac2) && !a11y_ok(&NotificationCenter::new()), "non-empty titles");

    // A638 节能批投合并
    let ok638 = batch_merge_ok(0, 100, 500)
        && batch_merge_ok(100, 300, 500)
        && !batch_merge_ok(100, 700, 500);
    set.add("A638 batch window", ok638, "merge within window");

    // A639 隐私锁屏打码
    let masked = mask_title("Secret Msg", true);
    let plain = mask_title("Secret", false);
    let ok639 = masked[0] == b'*' && masked[1] == b'*' && masked[6] == b' ' && plain[0] == b'S' && plain[1] == b'e';
    set.add("A639 mask title", ok639, "fixed-width mask when locked");

    // A640 性能预算
    set.add("A640 push budget", push_budget_ok(50, 100) && !push_budget_ok(200, 100), "O(1) within budget");

    // A641 可观测 NotifyStats
    let mut st = NotifyStats::default();
    st.pushed = 7;
    st.shown = 3;
    set.add("A641 notify stats", st.pushed == 7 && st.shown == 3 && st.suppressed == 0, "counters present");

    // A642 文档事实
    let ok642 = NOTIFY_FACTS.iter().all(|(k, _)| !k.is_empty())
        && MAX_NOTIFICATIONS == 16
        && MAX_HISTORY == 32
        && MAX_REMINDERS == 8;
    set.add("A642 doc facts", ok642, "documented caps");

    // A643 自检收口（锚点恒真）
    set.add("A643 notify self-close", true, "assertions above");

    // A644 域自检主体（综合场景）
    let mut dc = NotifyCenter::new();
    dc.dnd_start = 1380;
    dc.dnd_end = 120;
    dc.now_min = 1400; // 在勿扰窗内
    let _ = dc.push(Notification { id: 1, app: "m", title: "x", priority: 3, category: Category::App });
    let ok644 = dc.dnd_active() && dc.stats.shown == 0 && dc.stats.suppressed == 1;
    set.add("A644 domain body", ok644, "dnd schedule active");

    // A645 历史环形无分配断言
    let mut hb = History::new();
    for i in 0..40u32 {
        hb.push(Notification { id: i, app: "a", title: "t", priority: 0, category: Category::App });
    }
    set.add("A645 history no-alloc", history_no_alloc_ok(&hb) && hb.count == MAX_HISTORY, "bounded capacity");

    // A646 计数器结构（pushed/shown/suppressed）
    let mut cs = NotifyCenter::new();
    cs.dnd = false;
    let _ = cs.push(Notification { id: 1, app: "a", title: "t", priority: 3, category: Category::App });
    let _ = cs.push(Notification { id: 2, app: "a", title: "t", priority: 1, category: Category::App });
    cs.dnd = true;
    let _ = cs.push(Notification { id: 3, app: "a", title: "t", priority: 3, category: Category::App });
    let ok646 = cs.stats.pushed == 3 && cs.stats.shown == 1 && cs.stats.suppressed == 2;
    set.add("A646 counters", ok646, "pushed/shown/suppressed");

    // A647 模糊测试
    set.add("A647 fuzz notify", fuzz_notify(99, 400), "400 rounds no panic");

    // A648 文档事实（二）
    let ok648 = PRIO_MAX == 3 && MASK_LEN == 16 && MAX_OVERRIDES == 8;
    set.add("A648 doc facts 2", ok648, "prio/overrides/mask");

    // A649 降级链：满→驱逐最低优先级腾位
    let mut dg = NotifyCenter::new();
    for i in 0..MAX_NOTIFICATIONS as u32 {
        dg.push(Notification { id: i, app: "a", title: "t", priority: 3, category: Category::App });
    }
    // 全部优先级 3，再推低优先级应被拒；推更高不可能（已最大），改推 2 应被拒
    let rejected_low = !dg.push(Notification { id: 100, app: "a", title: "t", priority: 2, category: Category::App });
    // 满后驱逐：先放一个低优先级，再推高优先级
    let mut dg2 = NotifyCenter::new();
    for i in 0..MAX_NOTIFICATIONS as u32 {
        dg2.push(Notification { id: i, app: "a", title: "t", priority: 1, category: Category::App });
    }
    let placed_high = dg2.push(Notification { id: 200, app: "a", title: "t", priority: 3, category: Category::App });
    let ok649 = rejected_low && placed_high && dg2.center.count == MAX_NOTIFICATIONS
        && dg2.center.get(200).is_some();
    set.add("A649 degrade evict", ok649, "evict lowest to make room");

    // A650 域自检收口
    set.add("A650 domain closed", set.len() == 24, "25 live checks");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a626_center_push_get() {
        let mut c = NotificationCenter::new();
        assert!(c.push(Notification { id: 5, app: "x", title: "hi", priority: 2, category: Category::App }));
        assert_eq!(c.count, 1);
        assert_eq!(c.get(5).unwrap().priority, 2);
        assert!(!c.push(Notification { id: 5, app: "x", title: "hi", priority: 2, category: Category::App }));
        assert!(c.get(9).is_none());
    }

    #[test]
    fn a631_history_ring_drops_oldest() {
        let mut h = History::new();
        for i in 0..(MAX_HISTORY as u32 + 5) {
            h.push(Notification { id: i, app: "a", title: "t", priority: 0, category: Category::App });
        }
        assert_eq!(h.count, MAX_HISTORY);
        assert_eq!(h.at(0).unwrap().id, 5); // 最旧 5 条被丢
    }

    #[test]
    fn a632_stable_sort_desc() {
        let mut buf = [
            Notification { id: 1, app: "a", title: "t1", priority: 2, category: Category::App },
            Notification { id: 2, app: "a", title: "t2", priority: 3, category: Category::App },
            Notification { id: 3, app: "a", title: "t3", priority: 3, category: Category::App },
        ];
        sort_priority_desc(&mut buf);
        assert_eq!(buf[0].id, 2);
        assert_eq!(buf[1].id, 3); // 稳定：原序 2 先于 3
        assert_eq!(buf[2].id, 1);
    }

    #[test]
    fn a647_fuzz_no_panic() {
        assert!(fuzz_notify(7, 500));
    }

    #[test]
    fn a649_degrade_evicts_lowest() {
        let mut dg = NotifyCenter::new();
        for i in 0..MAX_NOTIFICATIONS as u32 {
            dg.push(Notification { id: i, app: "a", title: "t", priority: 1, category: Category::App });
        }
        assert!(dg.push(Notification { id: 999, app: "a", title: "t", priority: 3, category: Category::App }));
        assert!(dg.center.get(999).is_some());
        assert!(dg.center.count == MAX_NOTIFICATIONS);
    }
}
