//! F077 通知中心 · 完整设计（STAR I 主册 G-C-07）。
//!
//! **判据（主册）**：toast→历史→点击直达全链录屏；风暴合并实测；
//! 免打扰时段自动生效抽查。
//!
//! **设计要点（主册）**：
//! - 右侧滑出通知面板：toast 停留 5s 后沉入历史；历史按应用分组
//!   （组头=应用名+图标+「全部清除」）；顶部免打扰总开关；逐应用
//!   通知权限管理入口；
//! - 面板宽 360px、右侧全高滑入（250ms F124 曲线）；单条通知卡：
//!   应用图标+标题+正文两行截断+时间；卡片操作钮（应用自定义最多
//!   2 枚）；组折叠/展开；清空走确认（防手滑，可撤销 5s）；
//! - 历史保留 50 条/应用上限 20 条，落盘配置层（重启不丢）；免打扰
//!   计划（时段）可设；
//! - 应用通知风暴（>10 条/分钟）→ 自动折叠该组+提示；toast 期间
//!   同组新到 → 合并为计数角标（不叠罗汉）；免打扰期间 → 静默入
//!   历史（不弹）；通知点击直达应用对应内容（深链协议，应用注册）；
//! - 优先级三档（应用声明）：高=免打扰也弹（闹钟类）/正常/低=只进
//!   历史；时间标注相对格式与 F072 同函数（一处一事实：直用
//!   `crate::star::recenteng::relative_time`）；通知卡操作按钮走应用
//!   回执协议（点击带 action id 回应用）；深链失败 → 降级打开应用
//!   主界面。
//!
//! 实装口径：通知总线状态机 + 风暴合并账 + 免打扰时段机 + 撤销窗
//! 暂存账。落盘由配置层接手（本模块给持久化标志位与序列化边界）。

use crate::checks::CheckSet;

use crate::deskstar::dbase::FloatLayer;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册功能定义/交互设计/数据与存储）
// ---------------------------------------------------------------------------

/// toast 停留时长（ms，后沉入历史）。
pub const TOAST_DWELL_MS: u64 = 5_000;

/// 通知面板宽（px，右侧全高滑入）。
pub const PANEL_W_PX: i32 = 360;

/// 滑入动画时长（ms，F124 曲线）。
pub const SLIDE_IN_MS: u32 = 250;

/// 历史总上限（条）。
pub const HISTORY_CAP: usize = 50;

/// 单应用历史上限（条）。
pub const PER_APP_CAP: usize = 20;

/// 清空撤销窗（ms，可撤销 5s——真清空在撤销窗后）。
pub const UNDO_CLEAR_MS: u64 = 5_000;

/// 风暴判定线（条/分钟）。
pub const STORM_PER_MIN: u32 = 10;

/// 卡片操作钮上限（应用自定义）。
pub const MAX_ACTIONS: usize = 2;

/// 正文截断宽度（两行截断的字符账——截断以「行」为语义单位，
/// 本账以字符数近似行容量，渲染层换行后本值即两行总容）。
pub const BODY_CLIP_CHARS: usize = 120;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 优先级三档（应用声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// 高：免打扰也弹（闹钟类）。
    High,
    /// 正常。
    Normal,
    /// 低：只进历史。
    Low,
}

/// 通知动作钮（应用回执协议：点击带 action id 回应用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifAction {
    pub label: &'static str,
    pub action_id: u32,
}

/// 一条通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notif {
    pub id: u64,
    pub app_id: u64,
    pub title: String,
    pub body: String,
    pub ts_s: u64,
    pub priority: Priority,
    pub actions: Vec<NotifAction>,
    /// 深链目标（应用注册；None 或失效时降级打开应用主界面）。
    pub deep_link: Option<String>,
}

impl Notif {
    /// 正文两行截断（超容加省略号——诚实截断）。
    pub fn body_clipped(&self) -> String {
        if self.body.chars().count() <= BODY_CLIP_CHARS {
            return self.body.clone();
        }
        let mut out: String = self.body.chars().take(BODY_CLIP_CHARS).collect();
        out.push('…');
        out
    }

    /// 时间标注（F072 同函数——一处一事实）。
    pub fn time_label(&self, now_s: u64) -> String {
        crate::star::recenteng::relative_time(now_s, self.ts_s)
    }

    pub fn action_count(&self) -> usize {
        self.actions.len().min(MAX_ACTIONS)
    }
}

/// toast 位：展示中的通知 + 同组合并计数角标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub notif: Notif,
    pub shown_at_ms: u64,
    /// toast 期间同组新到合并计数（不叠罗汉）。
    pub merged: u32,
}

/// toast 沉没原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastExit {
    /// 停留到期沉历史。
    Dwell,
    /// 用户点掉。
    Dismissed,
    /// 免打扰生效改判（静默入历史）。
    DndSilenced,
}

/// 应用注册面（组头+权限管理入口的数据源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub id: u64,
    pub name: String,
    pub icon_token: u16,
    /// 通知权限（逐应用管理入口的状态位）。
    pub allowed: bool,
    /// 深链注册（点击直达协议；未注册 = 无深链）。
    pub deep_links: Vec<String>,
}

/// 组渲染态（历史按应用分组的折叠账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupView {
    pub app_id: u64,
    pub collapsed: bool,
    /// 风暴自动折叠标记（组头提示「通知较多已折叠」）。
    storm_folded: bool,
}

/// 免打扰时段（分钟粒度，支持跨零点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DndSpan {
    /// 起止分钟（0..1440）。
    pub from_min: u16,
    pub to_min: u16,
}

impl DndSpan {
    pub fn contains(&self, day_min: u16) -> bool {
        if self.from_min <= self.to_min {
            day_min >= self.from_min && day_min < self.to_min
        } else {
            // 跨零点：23:00-07:00。
            day_min >= self.from_min || day_min < self.to_min
        }
    }
}

// ---------------------------------------------------------------------------
// 通知中心状态机
// ---------------------------------------------------------------------------

/// 通知中心。
pub struct NotifCenter {
    layer: FloatLayer,
    apps: Vec<AppEntry>,
    groups: Vec<GroupView>,
    history: Vec<Notif>,
    toasts: Vec<Toast>,
    /// 免打扰总开关（手动优先于时段——主册 F153 手动优先逻辑同族）。
    dnd_manual: bool,
    dnd_spans: Vec<DndSpan>,
    now_ms: u64,
    next_id: u64,
    /// 每应用每分钟到达计数（风暴判定账：(app_id, minute_stamp, count)）。
    burst: Vec<(u64, u64, u32)>,
    /// 撤销窗暂存（真清空在撤销窗后）：被清历史 + 入暂存时刻。
    pending_clear: Option<(Vec<Notif>, u64)>,
    /// 风暴提示队列。
    storm_hints: Vec<u64>,
    /// 深链直达/降级账（全链判据的对账面）。
    pub deep_link_hits: u64,
    pub deep_link_fallbacks: u64,
    /// toast 沉没账（逐原因计数——全链录屏的数据底账）。
    toast_exits: [u32; 3],
}

impl NotifCenter {
    pub fn new() -> NotifCenter {
        NotifCenter {
            layer: FloatLayer::new(),
            apps: Vec::new(),
            groups: Vec::new(),
            history: Vec::new(),
            toasts: Vec::new(),
            dnd_manual: false,
            dnd_spans: Vec::new(),
            now_ms: 0,
            next_id: 1,
            burst: Vec::new(),
            pending_clear: None,
            storm_hints: Vec::new(),
            deep_link_hits: 0,
            deep_link_fallbacks: 0,
            toast_exits: [0; 3],
        }
    }

    pub fn is_open(&self) -> bool {
        self.layer.is_open()
    }

    pub fn open(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        self.layer.open(now_ms);
    }

    pub fn close(&mut self, now_ms: u64) {
        self.layer
            .close(crate::deskstar::dbase::CloseCause::Escape, now_ms, true);
    }

    // -- 应用注册 ----------------------------------------------------------

    /// 应用注册（组头数据源 + 权限位 + 深链注册表）。
    pub fn register_app(&mut self, id: u64, name: &str, icon_token: u16) {
        if self.apps.iter().any(|a| a.id == id) {
            return;
        }
        self.apps.push(AppEntry {
            id,
            name: String::from(name),
            icon_token,
            allowed: true,
            deep_links: Vec::new(),
        });
        self.groups.push(GroupView {
            app_id: id,
            collapsed: false,
            storm_folded: false,
        });
    }

    /// 深链注册（点击直达协议的应用侧声明）。
    pub fn register_deep_link(&mut self, app_id: u64, link: &str) {
        if let Some(a) = self.apps.iter_mut().find(|a| a.id == app_id) {
            a.deep_links.push(String::from(link));
        }
    }

    pub fn app_allowed(&self, app_id: u64) -> bool {
        self.apps.iter().find(|a| a.id == app_id).map(|a| a.allowed) == Some(true)
    }

    /// 逐应用权限切换（管理入口的状态位）。
    pub fn set_app_allowed(&mut self, app_id: u64, allowed: bool) {
        if let Some(a) = self.apps.iter_mut().find(|a| a.id == app_id) {
            a.allowed = allowed;
        }
    }

    // -- 免打扰 ------------------------------------------------------------

    pub fn set_dnd_manual(&mut self, on: bool) {
        self.dnd_manual = on;
    }

    pub fn dnd_manual(&self) -> bool {
        self.dnd_manual
    }

    pub fn add_dnd_span(&mut self, span: DndSpan) {
        self.dnd_spans.push(span);
    }

    /// 免打扰是否生效（手动总开 或 时段命中——时段自动生效判据）。
    pub fn dnd_active(&self, day_min: u16) -> bool {
        self.dnd_manual || self.dnd_spans.iter().any(|s| s.contains(day_min))
    }

    // -- 通知到达（总线入口）----------------------------------------------

    /// 通知到达。返回是否弹 toast（判据数据流：免打扰静默入历史 /
    /// 低优先级只进历史 / 正常与高弹 toast——高在免打扰也弹）。
    pub fn post(
        &mut self,
        app_id: u64,
        title: &str,
        body: &str,
        ts_s: u64,
        priority: Priority,
        now_ms: u64,
    ) -> bool {
        self.now_ms = now_ms;
        // 权限关：整条不入（权限管理是硬闸）。
        if !self.app_allowed(app_id) {
            return false;
        }
        // 风暴记账（>10 条/分钟 → 自动折叠该组+提示）。
        if self.record_burst(app_id, now_ms) {
            if let Some(g) = self.groups.iter_mut().find(|g| g.app_id == app_id) {
                g.collapsed = true;
                g.storm_folded = true;
            }
            self.storm_hints.push(app_id);
        }
        let notif = Notif {
            id: self.next_id,
            app_id,
            title: String::from(title),
            body: String::from(body),
            ts_s,
            priority,
            actions: Vec::new(),
            deep_link: None,
        };
        self.next_id += 1;
        // toast 合并：同组 toast 展示中 → 计数角标（不叠罗汉）。
        if priority != Priority::Low && self.toast_alive(now_ms) {
            if let Some(t) = self
                .toasts
                .iter_mut()
                .find(|t| t.notif.app_id == app_id)
            {
                t.merged += 1;
                // 合并入历史（合并的实体也留档）。
                self.push_history(notif);
                return false;
            }
        }
        // 免打扰：高优先级照弹，其余静默入历史（不弹）。
        let dnd = self.dnd_active((now_ms / 60_000 % 1440) as u16);
        if (dnd && priority != Priority::High) || priority == Priority::Low {
            self.push_history(notif);
            if dnd && priority == Priority::Normal {
                self.toast_exits[2] += 1; // DndSilenced
            }
            return false;
        }
        self.toasts.push(Toast {
            notif,
            shown_at_ms: now_ms,
            merged: 0,
        });
        true
    }

    /// 给 toast 补动作钮与深链（应用回执协议装配口）。
    pub fn decorate(&mut self, notif_id: u64, actions: Vec<NotifAction>, deep_link: Option<&str>) {
        if let Some(t) = self.toasts.iter_mut().find(|t| t.notif.id == notif_id) {
            t.notif.actions = actions.into_iter().take(MAX_ACTIONS).collect();
            t.notif.deep_link = deep_link.map(String::from);
        }
    }

    /// 风暴记账：同应用同分钟 ≥ STORM_PER_MIN 返回真（只沿越过沿报一次）。
    fn record_burst(&mut self, app_id: u64, now_ms: u64) -> bool {
        let min = now_ms / 60_000;
        // 60 分钟前的风暴桶清账（窗口账本）。
        self.burst.retain(|(_, m, _)| min.saturating_sub(*m) < 60);
        match self.burst.iter_mut().find(|(a, m, _)| *a == app_id && *m == min) {
            Some(entry) => {
                entry.2 += 1;
                entry.2 == STORM_PER_MIN + 1
            }
            None => {
                self.burst.push((app_id, min, 1));
                false
            }
        }
    }

    /// toast 是否存活（面板开且未过停留期）。
    fn toast_alive(&self, now_ms: u64) -> bool {
        self.toasts
            .iter()
            .any(|t| now_ms.saturating_sub(t.shown_at_ms) < TOAST_DWELL_MS)
    }

    /// 驱动到点沉没（宿主时钟滴答调用；到期 toast → 历史）。
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        let mut sunk = Vec::new();
        self.toasts.retain(|t| {
            if now_ms.saturating_sub(t.shown_at_ms) >= TOAST_DWELL_MS {
                sunk.push(t.notif.clone());
                false
            } else {
                true
            }
        });
        for n in sunk {
            self.toast_exits[0] += 1; // Dwell
            self.push_history(n);
        }
        // 撤销窗到期：真清空落账。
        if let Some((_, at)) = self.pending_clear {
            if now_ms.saturating_sub(at) >= UNDO_CLEAR_MS {
                self.pending_clear = None;
            }
        }
    }

    /// 用户点掉 toast。
    pub fn dismiss_toast(&mut self, notif_id: u64, now_ms: u64) {
        if let Some(pos) = self.toasts.iter().position(|t| t.notif.id == notif_id) {
            let t = self.toasts.remove(pos);
            self.toast_exits[1] += 1; // Dismissed
            self.push_history(t.notif);
            self.now_ms = now_ms;
        }
    }

    /// 沉历史（新在前；单应用上限 20 挤最旧、总上限 50 挤最旧——新挤旧）。
    fn push_history(&mut self, n: Notif) {
        self.history.insert(0, n);
        // 单应用计数。
        let mut count_by_app: Vec<(u64, usize)> = Vec::new();
        for h in &self.history {
            match count_by_app.iter_mut().find(|(a, _)| *a == h.app_id) {
                Some((_, c)) => *c += 1,
                None => count_by_app.push((h.app_id, 1)),
            }
        }
        // 标记待删位：单应用超额挤最旧（新在前 → 从尾部标）。
        let mut to_remove = vec![false; self.history.len()];
        for &(app_id, cnt) in &count_by_app {
            let mut excess = cnt.saturating_sub(PER_APP_CAP);
            for i in (0..self.history.len()).rev() {
                if excess == 0 {
                    break;
                }
                if self.history[i].app_id == app_id {
                    to_remove[i] = true;
                    excess -= 1;
                }
            }
        }
        // 总上限：尾部（最旧）超额整段标删。
        if self.history.len() > HISTORY_CAP {
            for i in HISTORY_CAP..self.history.len() {
                to_remove[i] = true;
            }
        }
        let mut idx = 0usize;
        self.history.retain(|_| {
            let keep = !to_remove[idx];
            idx += 1;
            keep
        });
    }

    // -- 点击直达与操作回执 ------------------------------------------------

    /// 通知点击：深链注册且有效 → 直达（返回 Ok(链接)）；
    /// 深链失败/未注册 → 降级打开应用主界面（Err(app_id)）。
    pub fn click(&mut self, notif_id: u64) -> Result<String, u64> {
        let n = match self
            .toasts
            .iter()
            .find(|t| t.notif.id == notif_id)
            .map(|t| t.notif.clone())
            .or_else(|| self.history.iter().find(|n| n.id == notif_id).cloned())
        {
            Some(n) => n,
            None => return Err(0),
        };
        if let Some(link) = &n.deep_link {
            let valid = self
                .apps
                .iter()
                .find(|a| a.id == n.app_id)
                .map(|a| a.deep_links.contains(link))
                == Some(true);
            if valid {
                self.deep_link_hits += 1;
                return Ok(link.clone());
            }
        }
        self.deep_link_fallbacks += 1;
        Err(n.app_id)
    }

    /// 卡片操作钮点击（action id 回应用——回执协议出口）。
    pub fn click_action(&self, notif_id: u64, idx: usize) -> Option<u32> {
        let n = self
            .toasts
            .iter()
            .find(|t| t.notif.id == notif_id)
            .map(|t| &t.notif)
            .or_else(|| self.history.iter().find(|n| n.id == notif_id))?;
        n.actions.get(idx).map(|a| a.action_id)
    }

    // -- 历史管理 ----------------------------------------------------------

    /// 组头「全部清除」（走确认 + 可撤销 5s——真清空在撤销窗后）。
    pub fn clear_group(&mut self, app_id: u64, now_ms: u64) {
        let taken: Vec<Notif> = self
            .history
            .iter()
            .filter(|n| n.app_id == app_id)
            .cloned()
            .collect();
        self.history.retain(|n| n.app_id != app_id);
        self.pending_clear = Some((taken, now_ms));
    }

    /// 全部清空（同撤销语义）。
    pub fn clear_all(&mut self, now_ms: u64) {
        let taken = core::mem::take(&mut self.history);
        self.pending_clear = Some((taken, now_ms));
    }

    /// 撤销清空（5s 窗内；窗后不可撤——诚实）。
    pub fn undo_clear(&mut self, now_ms: u64) -> bool {
        match self.pending_clear.take() {
            Some((items, at)) if now_ms.saturating_sub(at) < UNDO_CLEAR_MS => {
                self.history.splice(0..0, items);
                true
            }
            Some((items, _)) => {
                // 窗后到达的撤销请求：暂存已真清空，如实回绝。
                let _ = items;
                false
            }
            None => false,
        }
    }

    pub fn undo_pending(&self) -> bool {
        self.pending_clear.is_some()
    }

    /// 组折叠/展开。
    pub fn toggle_group(&mut self, app_id: u64) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.app_id == app_id) {
            g.collapsed = !g.collapsed;
            g.storm_folded = false; // 手动展开即解除风暴折叠态
        }
    }

    pub fn group_collapsed(&self, app_id: u64) -> bool {
        self.groups
            .iter()
            .find(|g| g.app_id == app_id)
            .map(|g| g.collapsed)
            == Some(true)
    }

    pub fn storm_folded(&self, app_id: u64) -> bool {
        self.groups
            .iter()
            .find(|g| g.app_id == app_id)
            .map(|g| g.storm_folded)
            .unwrap_or(false)
    }

    // -- 视图 --------------------------------------------------------------

    /// toast 视图（渲染序：后到在上）。
    pub fn toast_view(&self) -> Vec<&Toast> {
        self.toasts.iter().rev().collect()
    }

    /// 历史视图（渲染序：新在前；折叠组不展开条目）。
    pub fn history_view(&self) -> Vec<&Notif> {
        self.history
            .iter()
            .filter(|n| {
                !self
                    .groups
                    .iter()
                    .find(|g| g.app_id == n.app_id)
                    .map(|g| g.collapsed)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// 面板滑入进度（千分比；250ms F124 曲线，锚点=开启时刻）。
    pub fn slide_progress(&self) -> u16 {
        if !self.layer.is_open() {
            return 0;
        }
        let t = self.now_ms.saturating_sub(self.layer.opened_at_ms()) as u32;
        (t.min(SLIDE_IN_MS) * 1000 / SLIDE_IN_MS) as u16
    }

    /// 风暴提示队列消费。
    pub fn pop_storm_hint(&mut self) -> Option<u64> {
        if self.storm_hints.is_empty() {
            None
        } else {
            Some(self.storm_hints.remove(0))
        }
    }

    /// toast 沉没账（Dwell/Dismissed/DndSilenced）。
    pub fn exit_counts(&self) -> [u32; 3] {
        self.toast_exits
    }

    /// 持久化标志（重启不丢——配置层接手本账序列化）。
    pub fn persist_ready(&self) -> bool {
        self.history.len() <= HISTORY_CAP
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-07 验收判据）
// ---------------------------------------------------------------------------

/// F077 自检：toast→历史→点击直达全链、风暴合并、免打扰时段生效、
/// 优先级三档、组上限裁剪、撤销窗、权限硬闸、相对时间同源。
pub fn run_notifctr_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F077");
    let mut c = NotifCenter::new();
    c.register_app(1, "星邮", 7);
    c.register_app(2, "时钟", 3);
    // 1. toast→历史→点击直达全链。
    c.open(0);
    let popped = c.post(1, "新邮件", "来自 Variable", 100, Priority::Normal, 0);
    c.register_deep_link(1, "mail://inbox/1");
    c.decorate(1, vec![NotifAction { label: "已读", action_id: 9 }], Some("mail://inbox/1"));
    c.tick(TOAST_DWELL_MS); // 到期沉历史
    let ex = c.exit_counts();
    let in_hist = c.history_view().len() == 1;
    let link = c.click(1);
    ok_if(
        &mut set,
        "full-chain",
        popped && ex[0] == 1 && in_hist && matches!(&link, Ok(s) if s == "mail://inbox/1"),
        "toast→history→deep link",
    );
    // 2. 深链失败降级主界面。
    c.register_deep_link(2, "clock://alarms");
    let popped2 = c.post(2, "闹钟", "06:30", 200, Priority::Normal, TOAST_DWELL_MS + 1);
    c.tick(TOAST_DWELL_MS * 2);
    let degraded = c.click(2);
    ok_if(
        &mut set,
        "deep-link-fallback",
        popped2 && matches!(degraded, Err(2)),
        "fallback to app main",
    );
    // 3. 风暴合并：12 条/分钟 → 组自动折叠 + toast 合并角标。
    for i in 0..12u64 {
        c.post(1, "风暴", "批量", 1000 + i, Priority::Normal, 60_000 + i * 100);
    }
    let folded = c.group_collapsed(1) && c.storm_folded(1);
    ok_if(
        &mut set,
        "storm-fold",
        folded && c.pop_storm_hint() == Some(1),
        ">10/min folds group",
    );
    // 风暴 toast 显式沉底（否则它会活到第 7 检的 tick 里，把刚清空的
    // 历史又填回去——测试时间线纪律：每检自理自己的时序遗留）。
    c.tick(61_100 + TOAST_DWELL_MS);
    // 4. 免打扰时段自动生效：23:00-07:00 跨零点。
    c.add_dnd_span(DndSpan { from_min: 23 * 60, to_min: 7 * 60 });
    // 06:00（360 分）在段内 → 静默入历史不弹；高优先级照弹。
    let silent = c.post(2, "普通", "夜到", 2000, Priority::Normal, 360 * 60_000);
    let highp = c.post(2, "闹钟", "高优", 2001, Priority::High, 360 * 60_000 + 1);
    ok_if(
        &mut set,
        "dnd-span",
        !silent && highp && c.exit_counts()[2] >= 1,
        "span dnd + high bypass",
    );
    // 5. 低优先级只进历史（免打扰外也不弹）。
    let low = c.post(2, "低优", "只入历史", 3000, Priority::Low, 30 * 60_000);
    ok_if(&mut set, "low-priority", !low, "low never toasts");
    // 6. 单应用上限 20：第 21 条挤最旧。
    for i in 0..25u64 {
        c.post(1, "灌", "n", 4000 + i, Priority::Low, 31 * 60_000 + i);
    }
    let app1 = c.history.iter().filter(|n| n.app_id == 1).count();
    ok_if(&mut set, "per-app-cap", app1 <= PER_APP_CAP, "cap 20");
    // 7. 清空撤销窗：窗内可撤、窗后不可撤。
    c.clear_all(40 * 60_000);
    let undone = c.undo_clear(40 * 60_000 + 1_000);
    c.clear_all(41 * 60_000);
    c.tick(41 * 60_000 + UNDO_CLEAR_MS);
    let after_window = c.undo_clear(41 * 60_000 + UNDO_CLEAR_MS + 1);
    ok_if(
        &mut set,
        "undo-window",
        undone && !after_window && c.history.is_empty(),
        "5s undo",
    );
    // 8. 权限硬闸：关权限后整条不入。
    c.set_app_allowed(2, false);
    let blocked = c.post(2, "拒", "无权限", 5000, Priority::High, 50 * 60_000);
    ok_if(&mut set, "perm-gate", !blocked && !c.app_allowed(2), "hard gate");
    // 9. 相对时间同源（F072 函数直调冒烟）。
    let label = crate::star::recenteng::relative_time(10_000, 10_000 - 30);
    ok_if(&mut set, "time-same-source", label == "刚刚", "F072 same fn");
    set
}

fn ok_if(set: &mut CheckSet, group: &'static str, ok: bool, detail: &'static str) {
    set.add(group, ok, detail);
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_merges_same_group_badge() {
        let mut c = NotifCenter::new();
        c.register_app(1, "邮", 1);
        c.open(0);
        assert!(c.post(1, "第一条", "b", 10, Priority::Normal, 100));
        // 展示中同组新到 → 合并角标不弹。
        assert!(!c.post(1, "第二条", "b", 11, Priority::Normal, 200));
        let toasts = c.toast_view();
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].merged, 1, "计数角标合并不叠罗汉");
    }

    #[test]
    fn toast_dwell_5s_exact_boundary() {
        let mut c = NotifCenter::new();
        c.register_app(1, "邮", 1);
        c.open(0);
        c.post(1, "t", "b", 0, Priority::Normal, 1_000);
        c.tick(1_000 + TOAST_DWELL_MS - 1);
        assert_eq!(c.history_view().len(), 0, "5s 未到不沉");
        c.tick(1_000 + TOAST_DWELL_MS);
        assert_eq!(c.history_view().len(), 1, "5s 到点即沉");
    }

    #[test]
    fn body_clip_two_lines() {
        let mut c = NotifCenter::new();
        c.register_app(1, "邮", 1);
        let long = "长".repeat(BODY_CLIP_CHARS + 10);
        c.post(1, "t", &long, 0, Priority::Low, 0);
        let n = &c.history[0];
        let clipped = n.body_clipped();
        assert!(clipped.ends_with('…'));
        assert_eq!(clipped.chars().count(), BODY_CLIP_CHARS + 1);
    }

    #[test]
    fn action_receipt_protocol() {
        let mut c = NotifCenter::new();
        c.register_app(1, "邮", 1);
        c.open(0);
        c.post(1, "t", "b", 0, Priority::Normal, 0);
        c.decorate(
            1,
            vec![
                NotifAction { label: "已读", action_id: 1 },
                NotifAction { label: "删除", action_id: 2 },
                NotifAction { label: "多余", action_id: 3 }, // 超上限截断
            ],
            None,
        );
        assert_eq!(c.click_action(1, 0), Some(1));
        assert_eq!(c.click_action(1, 1), Some(2));
        assert_eq!(c.click_action(1, 2), None, "最多 2 枚");
    }

    #[test]
    fn history_total_cap_50() {
        let mut c = NotifCenter::new();
        for a in 1..=6u64 {
            c.register_app(a, "应用", 1);
        }
        for i in 0..60u64 {
            let app = 1 + i % 6;
            c.post(app, "n", "b", i, Priority::Low, i);
        }
        assert!(c.history.len() <= HISTORY_CAP);
        assert!(c.persist_ready());
    }

    #[test]
    fn group_clear_only_touches_own_group() {
        let mut c = NotifCenter::new();
        c.register_app(1, "甲", 1);
        c.register_app(2, "乙", 2);
        c.post(1, "a", "b", 1, Priority::Low, 0);
        c.post(2, "c", "d", 2, Priority::Low, 0);
        c.clear_group(1, 10_000);
        assert_eq!(c.history.len(), 1);
        assert!(c.undo_pending());
        assert!(c.undo_clear(11_000));
        assert_eq!(c.history.len(), 2, "撤销还原整组");
    }

    #[test]
    fn dnd_manual_override_and_toggle_group() {
        let mut c = NotifCenter::new();
        c.register_app(1, "甲", 1);
        c.set_dnd_manual(true);
        let popped = c.post(1, "n", "b", 0, Priority::Normal, 0);
        assert!(!popped, "手动免打扰压正常优先级");
        c.set_dnd_manual(false);
        let popped2 = c.post(1, "n", "b", 1, Priority::Normal, 1_000);
        assert!(popped2);
        c.tick(1_000 + TOAST_DWELL_MS); // toast 到期沉历史
        c.toggle_group(1);
        assert!(c.group_collapsed(1));
        assert!(c.history_view().is_empty(), "折叠组不展开条目");
        c.toggle_group(1);
        assert_eq!(c.history_view().len(), 2);
    }

    #[test]
    fn notifctr_self_checks_all_green() {
        let set = run_notifctr_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F077 自检红项：{}/{} 绿", p, p + f);
    }
}
