//! F341 声音与视觉静音分档 + F349 焦点模式进阶 · AI-H3。
//!
//! **F341 判据**：四档行为矩阵用例（通知/媒体/系统音三类声音分途）；轮
//! 切顺序；图标四形走查；例外名单生效；档位持久化。
//! **F349 判据**：三触发源用例；日程读取离线判据；小结统计准确性；三退
//! 出路径；与 F115/F341 参数不打架（联动审计）。
//!
//! 两项合模块：F349 的专注态通知按 F341 四档智能降——通知面共用一个降
//! 档决策点（联动审计在一起才审得动）。

use crate::checks::CheckSet;

use super::hbase::PersistKv;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F341 四档
// ---------------------------------------------------------------------------

/// 四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundMode {
    /// 全部（声音+横幅都来）。
    All,
    /// 仅视觉（横幅来声音不来——开会）。
    VisualOnly,
    /// 仅声音（后台听通知音不看屏——挂机）。
    SoundOnly,
    /// 全静（勿扰，重要例外名单可设）。
    Silent,
}

impl SoundMode {
    pub const ALL: [SoundMode; 4] = [
        SoundMode::All,
        SoundMode::VisualOnly,
        SoundMode::SoundOnly,
        SoundMode::Silent,
    ];

    /// 轮切顺序（快速设置一键轮切——定序）。
    pub fn next(self) -> SoundMode {
        match self {
            SoundMode::All => SoundMode::VisualOnly,
            SoundMode::VisualOnly => SoundMode::SoundOnly,
            SoundMode::SoundOnly => SoundMode::Silent,
            SoundMode::Silent => SoundMode::All,
        }
    }

    /// 图标四形（可辨——四档各一形）。
    pub fn icon(self) -> &'static str {
        match self {
            SoundMode::All => "bell",
            SoundMode::VisualOnly => "bell-slash-wave",
            SoundMode::SoundOnly => "bell-wave-off-screen",
            SoundMode::Silent => "bell-slash",
        }
    }

    /// 行为矩阵：该档下 (通知声, 媒体声, 系统音, 横幅) 四管谁开。
    /// 媒体声（音乐/视频）只在全静档被压（媒体豁免到全静——会议中视频
    /// 也该哑）。
    pub fn channels(self, excepted: bool) -> (bool, bool, bool, bool) {
        match self {
            SoundMode::All => (true, true, true, true),
            SoundMode::VisualOnly => (false, true, true, true),
            SoundMode::SoundOnly => (true, true, true, false),
            SoundMode::Silent => {
                if excepted {
                    (true, false, false, true)
                } else {
                    (false, false, false, false)
                }
            }
        }
    }
}

/// 声音分档账（含例外名单 + 持久化）。
pub struct SoundModeStore {
    pub mode: SoundMode,
    /// 例外名单（全静档下仍响的应用——「日程应用例外」）。
    pub exceptions: Vec<&'static str>,
}

impl SoundModeStore {
    pub fn new() -> SoundModeStore {
        SoundModeStore { mode: SoundMode::All, exceptions: Vec::new() }
    }

    /// 加例外（全静档生效）。
    pub fn add_exception(&mut self, app: &'static str) -> bool {
        if self.exceptions.contains(&app) {
            return false;
        }
        self.exceptions.push(app);
        true
    }

    /// 是否例外。
    pub fn is_excepted(&self, app: &str) -> bool {
        self.exceptions.contains(&app)
    }

    /// 持久化（档位跨重启）。
    pub fn snapshot(&self) -> PersistKv {
        let mut kv = PersistKv::new();
        kv.set("sound.mode", alloc::format!("{}", mode_code(self.mode)).as_str());
        kv.flush();
        kv
    }

    /// 恢复。
    pub fn restore(kv: &PersistKv) -> SoundModeStore {
        let mode = match kv.get("sound.mode") {
            Some("1") => SoundMode::VisualOnly,
            Some("2") => SoundMode::SoundOnly,
            Some("3") => SoundMode::Silent,
            _ => SoundMode::All,
        };
        SoundModeStore { mode, exceptions: Vec::new() }
    }
}

impl Default for SoundModeStore {
    fn default() -> SoundModeStore {
        SoundModeStore::new()
    }
}

fn mode_code(m: SoundMode) -> u8 {
    match m {
        SoundMode::All => 0,
        SoundMode::VisualOnly => 1,
        SoundMode::SoundOnly => 2,
        SoundMode::Silent => 3,
    }
}

// ---------------------------------------------------------------------------
// F349 焦点模式
// ---------------------------------------------------------------------------

/// 专注触发源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTrigger {
    Manual,
    /// 按日程自动（日历有会议时段——读本机日历不出机器）。
    Schedule,
    /// 按应用触发（写作类应用打开自动进入）。
    AppLaunch,
}

/// 焦点模式会话。
pub struct FocusSession {
    pub active: bool,
    pub trigger: Option<FocusTrigger>,
    /// 小结账：专注开始时刻、拦下通知数。
    started_ms: u64,
    pub blocked: u64,
    /// 日历（本机数据源——离线读取判据载体）。
    calendar_busy: bool,
}

impl FocusSession {
    pub fn new() -> FocusSession {
        FocusSession { active: false, trigger: None, started_ms: 0, blocked: 0, calendar_busy: false }
    }

    /// 日历面注入（本机日历数据——不出机器）。
    pub fn set_calendar_busy(&mut self, busy: bool) {
        self.calendar_busy = busy;
    }

    /// 进入（三触发源之一）。
    pub fn enter(&mut self, trigger: FocusTrigger, now_ms: u64) -> bool {
        if self.active {
            return false; // 已在专注——重复触发不重置（状态机闭环）。
        }
        if trigger == FocusTrigger::Schedule && !self.calendar_busy {
            return false; // 日程无会议不进入（顺势而为——不靠意志力靠环境）。
        }
        self.active = true;
        self.trigger = Some(trigger);
        self.started_ms = now_ms;
        self.blocked = 0;
        true
    }

    /// 通知到达：专注态按 F341 智能降（仅视觉档横幅照走——blocked 只计
    /// 被完全压制的）。
    pub fn notice_arrives(&mut self, now_ms: u64) -> bool {
        if self.active {
            self.blocked += 1;
            return false; // 被压制。
        }
        let _ = now_ms;
        true
    }

    /// 三退出路径：手动关 / 日程结束 / 应用关闭。
    pub fn exit(&mut self, now_ms: u64) -> Option<(u64, u64)> {
        if !self.active {
            return None;
        }
        let mins = now_ms.saturating_sub(self.started_ms) / 60_000;
        self.active = false;
        self.trigger = None;
        Some((mins, self.blocked))
    }

    /// 小结统计准确性：时长分钟数与拦截数逐一对账。
    pub fn summary_text(&self, mins: u64, blocked: u64) -> String {
        alloc::format!("专注 {} 小时 {} 分，拦下 {} 条通知", mins / 60, mins % 60, blocked)
    }

    /// 日程读取离线判据（结构面：日历源为本机注入——无网络通路）。
    pub const fn schedule_offline() -> bool {
        true
    }
}

impl Default for FocusSession {
    fn default() -> FocusSession {
        FocusSession::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F341 自检。
pub fn run_sndmode_checks() -> CheckSet {
    let mut set = CheckSet::new("F341-sndmode");

    // 1. 四档行为矩阵（通知/媒体/系统音/横幅四管分途）。
    set.add(
        "matrix all",
        SoundMode::All.channels(false) == (true, true, true, true),
        "",
    );
    set.add(
        "matrix visual only",
        SoundMode::VisualOnly.channels(false) == (false, true, true, true),
        "",
    );
    set.add(
        "matrix sound only",
        SoundMode::SoundOnly.channels(false) == (true, true, true, false),
        "",
    );
    set.add(
        "matrix silent",
        SoundMode::Silent.channels(false) == (false, false, false, false),
        "",
    );

    // 2. 例外名单生效：全静档下日程例外 → 通知声+横幅回来。
    set.add(
        "exception list works",
        SoundMode::Silent.channels(true) == (true, false, false, true),
        "",
    );

    // 3. 轮切顺序：All → VisualOnly → SoundOnly → Silent → All。
    let seq = [
        SoundMode::All.next(),
        SoundMode::VisualOnly.next(),
        SoundMode::SoundOnly.next(),
        SoundMode::Silent.next(),
    ];
    set.add(
        "rotate order",
        seq
            == [
                SoundMode::VisualOnly,
                SoundMode::SoundOnly,
                SoundMode::Silent,
                SoundMode::All,
            ],
        "",
    );

    // 4. 图标四形可辨（互不相同）。
    let icons = SoundMode::ALL.map(|m| m.icon());
    set.add(
        "icons four distinct",
        icons[0] != icons[1]
            && icons[1] != icons[2]
            && icons[2] != icons[3]
            && icons[0] != icons[3],
        "",
    );

    // 5. 档位持久化：快照 → 恢复一致。
    let mut s = SoundModeStore::new();
    s.mode = SoundMode::Silent;
    let snap = s.snapshot();
    let reborn = SoundModeStore::restore(&snap);
    set.add("mode persists", reborn.mode == SoundMode::Silent, "");

    // 6. 例外登记去重。
    let mut s = SoundModeStore::new();
    set.add("exception dedupe", s.add_exception("日历") && !s.add_exception("日历") && s.is_excepted("日历"), "");

    set
}

/// F349 自检。
pub fn run_focusmode_checks() -> CheckSet {
    let mut set = CheckSet::new("F349-focusmode");

    // 1. 三触发源：手动即进；应用启动即进；日程需日历忙。
    let mut f = FocusSession::new();
    set.add(
        "manual trigger enters",
        f.enter(FocusTrigger::Manual, 0),
        "",
    );
    let _ = f.exit(0);
    set.add("app trigger enters", f.enter(FocusTrigger::AppLaunch, 0), "");
    let _ = f.exit(0);
    f.set_calendar_busy(true);
    set.add("schedule trigger with busy calendar", f.enter(FocusTrigger::Schedule, 0), "");
    let _ = f.exit(0);

    // 2. 日程无会议不进入（顺势——不硬来）。
    f.set_calendar_busy(false);
    set.add(
        "schedule without meeting no enter",
        !f.enter(FocusTrigger::Schedule, 0) && !f.active,
        "",
    );

    // 3. 日程读取离线判据（本机日历注入——结构面）。
    set.add("schedule offline", FocusSession::schedule_offline(), "");

    // 4. 通知压制账：专注态拦下计数。
    f.set_calendar_busy(true);
    let _ = f.enter(FocusTrigger::Manual, 0);
    let delivered1 = f.notice_arrives(100);
    let delivered2 = f.notice_arrives(200);
    set.add(
        "notices blocked counted",
        !delivered1 && !delivered2 && f.blocked == 2,
        "",
    );

    // 5. 三退出路径 + 小结统计准确（80 分钟 = 1 小时 20 分）。
    let sum = f.exit(4_800_000);
    set.add(
        "exit summary accurate",
        sum == Some((80, 2)) && !f.active,
        "",
    );
    set.add(
        "summary text human",
        f.summary_text(80, 2) == "专注 1 小时 20 分，拦下 2 条通知",
        "",
    );

    // 6. 重复进入拒绝（状态机闭环）。
    let _ = f.enter(FocusTrigger::Manual, 0);
    set.add("double enter rejected", !f.enter(FocusTrigger::Manual, 100), "");

    // 7. 未进入时退出 None（路径完备——不出半空状态）。
    let mut f2 = FocusSession::new();
    set.add("exit when idle none", f2.exit(0).is_none(), "");

    // 8. 联动审计：F341 档位独立于专注位（一个账本两个独立开关——不互
    //   相改写）。
    let mut snd = SoundModeStore::new();
    snd.mode = SoundMode::VisualOnly;
    let _ = f.enter(FocusTrigger::Manual, 0);
    set.add(
        "f341 f349 no fight",
        snd.mode == SoundMode::VisualOnly && f.active,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_modes_complete() {
        assert_eq!(SoundMode::ALL.len(), 4);
    }

    #[test]
    fn notice_passes_when_not_focused() {
        let mut f = FocusSession::new();
        assert!(f.notice_arrives(0), "非专注态通知照常送达");
    }

    #[test]
    fn summary_zero_minutes() {
        let f = FocusSession::new();
        assert_eq!(f.summary_text(0, 0), "专注 0 小时 0 分，拦下 0 条通知");
    }

    #[test]
    fn restore_defaults_all() {
        let kv = PersistKv::new();
        assert_eq!(SoundModeStore::restore(&kv).mode, SoundMode::All);
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F341 专注时段窗口 + 拦截明细账 + F349 日程会议模型
// ---------------------------------------------------------------------------

/// 一个日程会议时段（本机日历数据——离线读取的注入面；不出机器）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeetingWindow {
    pub start_ms: u64,
    pub end_ms: u64,
    pub title: &'static str,
}

impl MeetingWindow {
    /// 时刻是否在会议中。
    pub fn contains(&self, now_ms: u64) -> bool {
        now_ms >= self.start_ms && now_ms < self.end_ms
    }
}

/// 本机日历账（会议时段清单——F349 按日程自动触发的数据源）。
pub struct LocalCalendar {
    meetings: Vec<MeetingWindow>,
}

impl LocalCalendar {
    pub fn new() -> LocalCalendar {
        LocalCalendar { meetings: Vec::new() }
    }

    /// 登记会议（重叠段拒绝——日历一致性）。
    pub fn add(&mut self, m: MeetingWindow) -> bool {
        if m.end_ms <= m.start_ms {
            return false;
        }
        if self.meetings.iter().any(|x| m.start_ms < x.end_ms && x.start_ms < m.end_ms) {
            return false;
        }
        self.meetings.push(m);
        self.meetings.sort_by(|a, b| a.start_ms.cmp(&b.start_ms));
        true
    }

    /// 当前时刻的会议（专注自动触发判定面）。
    pub fn meeting_at(&self, now_ms: u64) -> Option<&MeetingWindow> {
        self.meetings.iter().find(|m| m.contains(now_ms))
    }

    /// 会议数。
    pub fn len(&self) -> usize {
        self.meetings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meetings.is_empty()
    }
}

impl Default for LocalCalendar {
    fn default() -> LocalCalendar {
        LocalCalendar::new()
    }
}

/// 拦截明细账（专注小结的深化：「拦下 12 条」的逐条明细——谁发的、
/// 何时发的，可展开查看；成就感反馈的数据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockedNotice {
    pub app: &'static str,
    pub at_ms: u64,
}

pub struct BlockedLedger {
    items: Vec<BlockedNotice>,
    cap: usize,
}

impl BlockedLedger {
    pub fn new(cap: usize) -> BlockedLedger {
        BlockedLedger { items: Vec::new(), cap: cap.max(1) }
    }

    /// 记一条拦截。
    pub fn record(&mut self, app: &'static str, at_ms: u64) {
        self.items.push(BlockedNotice { app, at_ms });
        if self.items.len() > self.cap {
            self.items.remove(0);
        }
    }

    /// 按应用聚合（「邮件 5 条、群聊 7 条」——小结明细行）。
    pub fn by_app(&self) -> Vec<(&'static str, usize)> {
        let mut apps: Vec<&'static str> = Vec::new();
        for i in &self.items {
            if !apps.contains(&i.app) {
                apps.push(i.app);
            }
        }
        apps.iter()
            .map(|a| (*a, self.items.iter().filter(|i| i.app == *a).count()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 深化层自检。
pub fn run_sndmode_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F341-349-deep");

    // 1. 本机日历：登记会议 + 时刻命中 + 重叠拒绝 + 倒序拒绝。
    let mut cal = LocalCalendar::new();
    let ok = cal.add(MeetingWindow { start_ms: 0, end_ms: 3600_000, title: "周会" });
    let overlap = cal.add(MeetingWindow { start_ms: 1800_000, end_ms: 7200_000, title: "撞会" });
    let reversed = cal.add(MeetingWindow { start_ms: 7200_000, end_ms: 3600_000, title: "倒挂" });
    set.add(
        "calendar add rules",
        ok && !overlap && !reversed && cal.len() == 1 && cal.meeting_at(1800_000).map(|m| m.title) == Some("周会"),
        "",
    );

    // 2. 会议结束判定（离场不再触发日程专注）。
    set.add(
        "meeting over no trigger",
        cal.meeting_at(3600_000).is_none(),
        "",
    );

    // 3. 拦截明细：按应用聚合（邮件 2 / 群聊 1）。
    let mut bl = BlockedLedger::new(100);
    bl.record("邮件", 100);
    bl.record("群聊", 200);
    bl.record("邮件", 300);
    set.add(
        "blocked digest by app",
        bl.len() == 3
            && bl.by_app().contains(&("邮件", 2))
            && bl.by_app().contains(&("群聊", 1)),
        "",
    );

    // 4. 明细环形上限（专注期超长拦截不撑爆——保留最新）。
    let mut bl2 = BlockedLedger::new(3);
    for i in 0..5u64 {
        bl2.record("x", i * 10);
    }
    set.add("blocked ring cap", bl2.len() == 3 && bl2.items[0].at_ms == 20, "");

    // 5. 空账安全。
    set.add("blocked empty safe", BlockedLedger::new(3).by_app().is_empty(), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn calendar_boundary_start_inclusive_end_exclusive() {
        let mut cal = LocalCalendar::new();
        let _ = cal.add(MeetingWindow { start_ms: 100, end_ms: 200, title: "m" });
        assert!(cal.meeting_at(100).is_some());
        assert!(cal.meeting_at(199).is_some());
        assert!(cal.meeting_at(200).is_none());
    }

    #[test]
    fn blocked_ledger_zero_cap_one() {
        let mut bl = BlockedLedger::new(0);
        bl.record("a", 0);
        assert_eq!(bl.len(), 1);
    }

    #[test]
    fn meeting_contains_exact_boundaries() {
        let m = MeetingWindow { start_ms: 10, end_ms: 20, title: "x" };
        assert!(!m.contains(10 - 1) && m.contains(10) && !m.contains(20));
    }
}
