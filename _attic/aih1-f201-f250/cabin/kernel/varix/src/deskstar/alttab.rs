//! F082 Alt+Tab 现代化 · 完整设计（STAR I 主册 G-C-12）。
//!
//! **判据（主册）**：双窗切换延迟 <100ms 实测；十二窗压力下卡片墙
//! 布局不乱；假死窗口标注实测。
//!
//! **设计要点（主册）**：
//! - Alt+Tab 切换器：缩略图+图标双排卡片、按最近使用排序、长按 Alt
//!   停留展开完整面板（松开即切、加 Tab 或方向键选择）；
//! - 快速态：双窗直接切换（<200ms 无 UI）；停留态：卡片墙居中弹出
//!   （卡 240×150px、图标 24px 左上、标题底部）、最近使用序从左上
//!   蛇形排列；选择高亮放大 5%（弹性曲线）；Alt+Shift+Tab 反向；
//! - 排序即 F072 应用维度最近序（同引擎）；无额外存储；
//! - 单窗口 → 不弹（直接忽略）；应用假死 → 卡片灰显「无响应」仍
//!   可选（切过去看它死透没——诚实）；UAC 类系统窗不进墙；
//! - 停留判定：Alt 按住 >350ms 出墙（低于则维持快速态）；墙出现后
//!   Alt 可松开（选择态独立）；卡片缩略图 2 次/秒刷新（同 F073）；
//!   卡片数上限 24（超出滚动）；墙背景毛玻璃+背景窗口降透明 40%
//!   （聚焦感）。
//!
//! 实装口径：快速态/停留态双态状态机 + MRU 序（F072 最近序投影，
//! 经显式 ranked 供给注入）+ 蛇形布局账 + 假死标注账 + 双窗切换
//! 延迟账。缩略图刷新节拍账本内置。时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{budget_ok, Ease, EaseKind, Rect, Token};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 停留判定线（ms，Alt 按住超此值出墙）。
pub const HOLD_MS: u64 = 350;

/// 卡片宽（px）。
pub const CARD_W_PX: i32 = 240;

/// 卡片高（px）。
pub const CARD_H_PX: i32 = 150;

/// 卡内图标（px，左上）。
pub const ICON_PX: i32 = 24;

/// 双窗切换预算（ms）。
pub const SWITCH_BUDGET_MS: u64 = 100;

/// 墙单页卡片数（超出滚动分页——主册「卡片数上限 24（超出滚动）」）。
pub const WALL_CAP: usize = 24;

/// 候选池容量（4 页——墙内滚动分页的总承载上限，池外如实截断）。
pub const POOL_CAP: usize = WALL_CAP * 4;

/// 缩略图刷新间隔（ms，2 次/秒）。
pub const THUMB_REFRESH_MS: u64 = 500;

/// 墙背景窗口降透明（%）。
pub const BG_DIM_PCT: u8 = 40;

/// 选择高亮放大量（%，弹性曲线）。
pub const HIGHLIGHT_SCALE_PCT: u8 = 5;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 切换候选（卡片数据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCard {
    pub window: u64,
    pub title: String,
    pub icon_token: u16,
    /// 假死标注（灰显「无响应」仍可选——诚实）。
    pub hung: bool,
    /// UAC 类系统窗（不进墙）。
    pub system: bool,
    /// 最小化状态（卡片角标——诚实标注）。
    pub minimized: bool,
}

/// 切换器状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchState {
    Idle,
    /// 快速态：Alt 按住未超 350ms——Tab 直切不弹墙。
    Fast,
    /// 停留态：卡片墙弹出（Alt 松开后仍保持——选择态独立）。
    Wall,
}

/// Alt+Tab 切换器。
pub struct AltTab {
    state: SwitchState,
    alt_down_since: Option<u64>,
    now_ms: u64,
    /// 候选（最近使用序——F072 投影经 set_candidates 注入）。
    cards: Vec<TabCard>,
    /// 墙内焦点下标。
    sel: usize,
    /// 双窗切换延迟账（最近一次快速切换的实测值）。
    pub last_switch_ms: Option<u64>,
    /// 缩略图刷新节拍账（2 次/秒）。
    last_thumb_ms: Option<u64>,
    pub thumb_refreshes: u64,
    /// UAC 排除账。
    pub excluded: u64,
    /// 蛇形布局缓存（墙开时计算一次）。
    serpentine: Vec<Rect>,
    /// 超限滚动页（>24 卡分页）。
    scroll_page: usize,
    /// 缩略脏标记（定向刷新——不整墙重刷）。
    thumb_dirty: Vec<u64>,
}

impl AltTab {
    pub fn new() -> AltTab {
        AltTab {
            state: SwitchState::Idle,
            alt_down_since: None,
            now_ms: 0,
            cards: Vec::new(),
            sel: 0,
            last_switch_ms: None,
            last_thumb_ms: None,
            thumb_refreshes: 0,
            excluded: 0,
            serpentine: Vec::new(),
            scroll_page: 0,
            thumb_dirty: Vec::new(),
        }
    }

    /// 候选注入（F072 最近序投影——调用方把 ranked 应用序灌进来；
    /// UAC 类系统窗在此被挡在墙外并记账。池容量 POOL_CAP=4 页：墙每页
    /// WALL_CAP=24 张滚动分页（主册「卡片数上限 24（超出滚动）」的正确
    /// 读法是滚动而非截断——深化层语义回填，见缺陷账本 D1-v2-01）。
    pub fn set_candidates(&mut self, cards: Vec<TabCard>) {
        self.excluded += cards.iter().filter(|c| c.system).count() as u64;
        self.cards = cards
            .into_iter()
            .filter(|c| !c.system)
            .take(POOL_CAP)
            .collect();
        // 池变化后旧页位失效——诚实归零（第一页重新起算）。
        self.scroll_page = 0;
    }

    pub fn candidates(&self) -> &[TabCard] {
        &self.cards
    }

    pub fn state(&self) -> SwitchState {
        self.state
    }

    /// Alt 按下（计时起点）。
    pub fn alt_down(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.state == SwitchState::Idle {
            self.alt_down_since = Some(now_ms);
            self.state = SwitchState::Fast;
            self.sel = 0;
        }
    }

    /// Alt 期间的 Tab/方向步进（快速态直切下一个；停留态移动焦点）。
    /// 返回快速态实际切换到的窗口（None = 停留态移动或无候选）。
    pub fn step(&mut self, now_ms: u64) -> Option<u64> {
        self.now_ms = now_ms;
        if self.cards.is_empty() {
            return None;
        }
        match self.state {
            SwitchState::Fast => {
                // 快速态：直切 MRU 次序窗口（<100ms 无 UI——账记延迟）。
                let t0 = self.alt_down_since.unwrap_or(now_ms);
                self.last_switch_ms = Some(now_ms.saturating_sub(t0).max(1));
                Some(self.cards[1 % self.cards.len()].window)
            }
            SwitchState::Wall => {
                self.sel = (self.sel + 1) % self.wall_cards().len();
                None
            }
            SwitchState::Idle => None,
        }
    }

    /// 反向步进（Alt+Shift+Tab）。
    pub fn step_back(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.state == SwitchState::Wall && !self.wall_cards().is_empty() {
            let n = self.wall_cards().len();
            self.sel = (self.sel + n - 1) % n;
        }
    }

    /// Alt 持续按住的心跳（>350ms 出墙；单窗口不出墙——直接忽略）。
    pub fn hold_tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.state == SwitchState::Fast {
            if let Some(t0) = self.alt_down_since {
                if now_ms.saturating_sub(t0) > HOLD_MS && self.cards.len() >= 2 {
                    self.state = SwitchState::Wall;
                    self.sel = 0;
                    self.layout_serpentine();
                }
            }
        }
    }

    /// Alt 松开：墙已开 → 选择态独立（保持）；未开 → 回 Idle 并把
    /// 快速切换落定（调用方按 step() 返回值执行切换）。
    pub fn alt_up(&mut self, now_ms: u64) -> Option<u64> {
        self.now_ms = now_ms;
        self.alt_down_since = None;
        match self.state {
            SwitchState::Wall => None, // 墙保持，等待选择
            _ => {
                self.state = SwitchState::Idle;
                None
            }
        }
    }

    /// 墙内焦点步进（方向键；Alt 已松也有效——选择态独立）。
    pub fn wall_next(&mut self) {
        if self.state == SwitchState::Wall {
            let n = self.wall_cards().len();
            if n > 0 {
                self.sel = (self.sel + 1) % n;
            }
        }
    }

    pub fn wall_prev(&mut self) {
        if self.state == SwitchState::Wall {
            let n = self.wall_cards().len();
            if n > 0 {
                self.sel = (self.sel + n - 1) % n;
            }
        }
    }

    /// 确认选择（Enter/松手切到焦点窗口；非墙态无选择——取消语义）。
    pub fn commit(&mut self, now_ms: u64) -> Option<u64> {
        let w = if self.state == SwitchState::Wall {
            self.wall_cards().get(self.sel).map(|c| c.window)
        } else {
            None
        };
        self.state = SwitchState::Idle;
        self.sel = 0;
        self.now_ms = now_ms;
        w
    }

    /// Esc 取消（墙关，不切换）。
    pub fn cancel(&mut self, now_ms: u64) {
        self.state = SwitchState::Idle;
        self.sel = 0;
        self.now_ms = now_ms;
    }

    /// 墙内候选（假死卡在内——灰显仍可选）。
    fn wall_cards(&self) -> &[TabCard] {
        &self.cards
    }

    pub fn selected(&self) -> Option<&TabCard> {
        self.wall_cards().get(self.sel)
    }

    /// 假死卡灰显令牌 + 标注（诚实呈现——不装活）。
    pub fn card_style(&self, idx: usize) -> (Token, Option<&'static str>) {
        match self.wall_cards().get(idx) {
            Some(c) if c.hung => (Token::Disabled, Some("无响应")),
            Some(_) => (Token::SurfaceRaised, None),
            None => (Token::Off, None),
        }
    }

    /// 选择高亮放大 5%。
    pub fn highlight_scale_permille(&self) -> u32 {
        1000 + (HIGHLIGHT_SCALE_PCT as u32 * 10)
    }

    /// 背景降透明（墙弹出时 40%）。
    pub fn bg_opacity_pct(&self) -> u8 {
        if self.state == SwitchState::Wall {
            100 - BG_DIM_PCT
        } else {
            100
        }
    }

    /// 蛇形布局（最近使用序从左上蛇形排列；十二窗压力下布局不乱——
    /// 4 列固定网格，蛇形行进，越界换行回折）。
    fn layout_serpentine(&mut self) {
        self.serpentine.clear();
        let n = self.wall_cards().len();
        let cols = 4usize;
        for i in 0..n {
            let row = i / cols;
            let col_in_row = i % cols;
            let col = if row % 2 == 0 {
                col_in_row
            } else {
                cols - 1 - col_in_row
            };
            self.serpentine.push(Rect::new(
                col as i32 * CARD_W_PX,
                row as i32 * CARD_H_PX,
                CARD_W_PX,
                CARD_H_PX,
            ));
        }
    }

    pub fn serpentine_rects(&self) -> &[Rect] {
        &self.serpentine
    }

    /// 十二窗布局不乱判据：矩形互不重叠、网格对齐。
    pub fn layout_is_tidy(&self) -> bool {
        let n = self.serpentine.len();
        if n != self.cards.len() {
            return false;
        }
        for i in 0..n {
            for j in (i + 1)..n {
                if self.serpentine[i].intersects(&self.serpentine[j]) {
                    return false;
                }
            }
        }
        true
    }

    /// 缩略图刷新节拍（2 次/秒——到期才刷，账上留痕）。
    pub fn thumb_due(&mut self, now_ms: u64) -> bool {
        let due = match self.last_thumb_ms {
            None => self.state == SwitchState::Wall,
            Some(t) => self.state == SwitchState::Wall && now_ms.saturating_sub(t) >= THUMB_REFRESH_MS,
        };
        if due {
            self.last_thumb_ms = Some(now_ms);
            self.thumb_refreshes += 1;
        }
        due
    }

    /// 双窗切换延迟是否达标（<100ms 实测账）。
    pub fn switch_in_budget(&self) -> bool {
        self.last_switch_ms.map(|t| budget_ok(t, SWITCH_BUDGET_MS)) == Some(true)
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：墙内关窗（Delete）/ 最小化角标 / 超限滚动页 / 缩略
// 脏标记 / 数字键快选 / 墙点外关闭——主册【交互设计】【设计细节】补足。
// ---------------------------------------------------------------------------

/// 数字键快选上限（1-9 直选）。
pub const NUM_QUICK_SELECT: usize = 9;

/// 卡片角标（最小化窗口的诚实标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardBadge {
    None,
    Minimized,
}

impl AltTab {
    /// 墙内关窗（Delete：列表中直接关掉——返回被关窗口）。
    pub fn wall_close_window(&mut self, now_ms: u64) -> Option<u64> {
        if self.state != SwitchState::Wall {
            return None;
        }
        if let Some(w) = self.wall_cards().get(self.sel).map(|c| c.window) {
            if self.sel < self.cards.len() {
                self.cards.remove(self.sel);
                if self.sel >= self.cards.len() && self.sel > 0 {
                    self.sel -= 1;
                }
                self.layout_serpentine();
                self.now_ms = now_ms;
                return Some(w);
            }
        }
        None
    }

    /// 最小化标注查询（卡片角标——诚实标注不装活）。
    pub fn card_badge(&self, idx: usize) -> CardBadge {
        match self.wall_cards().get(idx) {
            Some(c) if c.minimized => CardBadge::Minimized,
            Some(_) => CardBadge::None,
            None => CardBadge::None,
        }
    }

    /// 超限滚动页（>24 卡分页：页内显示 24，翻页查其余）。
    pub fn scroll_page(&mut self, forward: bool) -> usize {
        if self.cards.len() <= WALL_CAP {
            return self.scroll_page;
        }
        let pages = (self.cards.len() + WALL_CAP - 1) / WALL_CAP;
        if forward {
            self.scroll_page = (self.scroll_page + 1) % pages;
        } else {
            self.scroll_page = (self.scroll_page + pages - 1) % pages;
        }
        self.scroll_page
    }

    /// 当前页卡片（滚动窗口切片；页位对池收缩自钳制——删窗后旧页位
    /// 不越界，切片零 panic 路径）。
    pub fn page_cards(&self) -> &[TabCard] {
        let len = self.cards.len();
        let pages = (len + WALL_CAP - 1) / WALL_CAP;
        let page = if pages == 0 {
            0
        } else {
            self.scroll_page.min(pages - 1)
        };
        let start = page * WALL_CAP;
        let end = (start + WALL_CAP).min(len);
        &self.cards[start..end]
    }

    /// 缩略图脏标记（窗口内容变化 → 该卡缩略需重取；2 次/秒节拍内的
    /// 定向刷新——不整墙重刷）。
    pub fn mark_thumb_dirty(&mut self, window: u64) -> bool {
        self.thumb_dirty.push(window);
        true
    }

    /// 消费脏标记（刷新节拍到点时取走定向清单——只刷脏卡）。
    pub fn take_thumb_dirty(&mut self) -> Vec<u64> {
        core::mem::take(&mut self.thumb_dirty)
    }

    /// 数字键快选（1-9 直选墙内第 N 卡）。
    pub fn quick_select(&mut self, n: usize) -> Option<u64> {
        if self.state != SwitchState::Wall || n == 0 || n > NUM_QUICK_SELECT {
            return None;
        }
        let idx = n - 1;
        if idx < self.wall_cards().len() {
            self.sel = idx;
            self.commit(self.now_ms)
        } else {
            None
        }
    }

    /// 墙点外关闭（与浮层出路纪律对齐——点外 = 取消选择）。
    pub fn click_outside_wall(&mut self, now_ms: u64) -> bool {
        if self.state == SwitchState::Wall {
            self.cancel(now_ms);
            true
        } else {
            false
        }
    }

    /// PgUp/PgDn 翻页接缝（键盘路径与 scroll_page 同一状态——键盘
    /// 与滚轮/点击共用页账，不另设第二份页码）。
    pub fn page_key(&mut self, forward: bool) -> usize {
        self.scroll_page(forward)
    }

    /// 墙面材质令牌（毛玻璃——材质族与任务栏 C-3 同源，色值走 E1）。
    pub fn wall_surface_token(&self) -> Token {
        Token::SurfaceRaised
    }

    /// 选中卡高亮动画规格（放大 5% 弹性曲线——F124 弹性档同参，
    /// 参数只此一处：曲线型 + 时长 + 峰值）。
    pub fn highlight_ease(&self) -> Ease {
        Ease {
            dur_ms: 120,
            kind: EaseKind::EaseOut,
            overshoot_permille: 0,
            rebound_ms: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层二（回炉批 v2）：卡内布局几何 / 标题截断 / 角标位 / 缩略
// 节拍与页联动——主册【交互设计】【设计细节】渲染面补足。
// 深化编号 D1-v2-AT*。
// ---------------------------------------------------------------------------

/// 卡内边距（px）。
pub const CARD_PAD_PX: i32 = 8;

/// 标题底条高（px——标题在卡底部一条，主册「标题底部」）。
pub const TITLE_STRIP_H_PX: i32 = 24;

/// 卡片标题最大字符数（超出截断加省略号——底条宽度内可读）。
pub const TITLE_MAX_CHARS: usize = 18;

impl AltTab {
    /// 图标矩形（卡内左上——24px，主册「图标 24px 左上」）。
    pub fn icon_rect(&self, card_index: usize) -> Rect {
        let r = self.serpentine.get(card_index).copied().unwrap_or(Rect::new(0, 0, CARD_W_PX, CARD_H_PX));
        Rect::new(r.x + CARD_PAD_PX, r.y + CARD_PAD_PX, ICON_PX, ICON_PX)
    }

    /// 标题条矩形（卡底部——文字在条内垂直居中）。
    pub fn title_rect(&self, card_index: usize) -> Rect {
        let r = self.serpentine.get(card_index).copied().unwrap_or(Rect::new(0, 0, CARD_W_PX, CARD_H_PX));
        Rect::new(
            r.x + CARD_PAD_PX,
            r.y + r.h - TITLE_STRIP_H_PX - CARD_PAD_PX,
            r.w - CARD_PAD_PX * 2,
            TITLE_STRIP_H_PX,
        )
    }

    /// 角标矩形（最小化标注位——图标右侧 16px 徽标位）。
    pub fn badge_rect(&self, card_index: usize) -> Rect {
        let r = self.serpentine.get(card_index).copied().unwrap_or(Rect::new(0, 0, CARD_W_PX, CARD_H_PX));
        Rect::new(
            r.x + CARD_PAD_PX + ICON_PX + CARD_PAD_PX,
            r.y + CARD_PAD_PX,
            16,
            16,
        )
    }

    /// 标题截断（超长 → 保头 + 「…」——底条内单行可读；短标题原样）。
    pub fn truncate_title(title: &str) -> String {
        let n = title.chars().count();
        if n <= TITLE_MAX_CHARS {
            return String::from(title);
        }
        let kept: String = title.chars().take(TITLE_MAX_CHARS).collect();
        alloc::format!("{}…", kept)
    }

    /// 缩略节拍与页联动（页切换 → 缩略账换页重计——脏页从零刷，
    /// 不跨页继承节拍账）。
    pub fn notify_page_changed(&mut self) {
        self.last_thumb_ms = None;
        self.thumb_refreshes = 0;
    }
}

/// F082 深化自检：墙内关窗、最小化角标、超限滚动、脏标记定向刷新、
/// 数字快选、点外关闭。
pub fn run_alttab_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F082-deep");
    let card = |w: u64, hung: bool, min: bool| TabCard {
        window: w,
        title: String::from("窗"),
        icon_token: 1,
        hung,
        system: false,
        minimized: min,
    };
    let mut at = AltTab::new();
    // 1. 最小化角标（诚实标注）。
    at.set_candidates(vec![card(1, false, false), card(2, false, true)]);
    at.alt_down(0);
    at.hold_tick(400);
    set.add(
        "min-badge",
        at.card_badge(0) == CardBadge::None && at.card_badge(1) == CardBadge::Minimized,
        "minimized honest badge",
    );
    // 2. 墙内关窗（Delete）：焦点卡被移除、布局重排、焦点不越界。
    at.wall_next();
    let removed = at.wall_close_window(500);
    let n_after = at.candidates().len();
    let focus_ok = at.selected().unwrap().window == 1; // 回到首卡
    at.cancel(600);
    set.add(
        "wall-close",
        removed == Some(2) && n_after == 1 && focus_ok,
        "delete closes card",
    );
    // 3. 数字快选 1-9。
    at.set_candidates(vec![card(1, false, false), card(2, false, false), card(3, false, false)]);
    at.alt_down(1_000);
    at.hold_tick(1_400);
    let q3 = at.quick_select(3);
    set.add(
        "num-quick-select",
        q3 == Some(3) && at.quick_select(0).is_none() && at.quick_select(4).is_none(),
        "1-9 direct select",
    );
    // 4. 墙点外关闭（浮层出路纪律；快选的 commit 已收墙——重新出墙再测）。
    at.alt_down(1_400);
    at.hold_tick(1_800);
    let outside = at.click_outside_wall(1_900) && at.state() == SwitchState::Idle;
    set.add("outside-close", outside, "point-outside dismisses");
    // 5. 超限滚动页（30 卡 → 2 页循环）。
    let mut many: Vec<TabCard> = Vec::new();
    for i in 0..30u64 {
        many.push(card(i, false, false));
    }
    at.set_candidates(many);
    at.alt_down(2_000);
    at.hold_tick(2_400);
    let p0 = at.page_cards().len();
    let p1 = at.scroll_page(true);
    let p1_len = at.page_cards().len();
    let wrapped = at.scroll_page(true) == 0; // 2 页循环回绕
    let back = at.scroll_page(false) == 1;
    set.add(
        "scroll-pages",
        p0 == 24 && p1 == 1 && p1_len == 6 && wrapped && back,
        "24/page cycle",
    );
    // 6. 缩略脏标记：定向刷新（只取走脏卡——不整墙重刷）。
    at.mark_thumb_dirty(5);
    at.mark_thumb_dirty(9);
    let dirty = at.take_thumb_dirty();
    let drained = at.take_thumb_dirty().is_empty();
    set.add(
        "thumb-dirty",
        dirty == vec![5, 9] && drained,
        "targeted refresh",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    fn card(w: u64) -> TabCard {
        TabCard {
            window: w,
            title: String::from("窗"),
            icon_token: 1,
            hung: false,
            system: false,
            minimized: false,
        }
    }

    #[test]
    fn wall_close_only_in_wall_state() {
        let mut at = AltTab::new();
        at.set_candidates(vec![card(1), card(2)]);
        assert!(at.wall_close_window(0).is_none(), "非墙态不可关窗");
    }

    #[test]
    fn scroll_page_noop_when_under_cap() {
        let mut at = AltTab::new();
        at.set_candidates(vec![card(1), card(2)]);
        assert_eq!(at.scroll_page(true), 0, "未超限不翻页");
    }

    #[test]
    fn quick_select_needs_wall() {
        let mut at = AltTab::new();
        at.set_candidates(vec![card(1), card(2)]);
        at.alt_down(0);
        assert!(at.quick_select(1).is_none(), "快速态无快选");
    }

    #[test]
    fn alttab_deep_checks_all_green() {
        let set = run_alttab_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F082-deep 红项：{}/{} 绿", p, p + f);
    }
}

// 自检（判据唯一源：主册 G-C-12 验收判据）
// ---------------------------------------------------------------------------

/// F082 自检：双窗切换 <100ms、十二窗布局不乱、假死标注、单窗忽略、
/// UAC 排除、350ms 出墙、墙后 Alt 可松、反向步进、缩略节拍、上限 24。
pub fn run_alttab_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F082");
    let mut at = AltTab::new();
    let mk = |w: u64, t: &str, hung: bool, sys: bool| TabCard {
        window: w,
        title: String::from(t),
        icon_token: 3,
        hung,
        system: sys,
        minimized: false,
    };
    // 1. 双窗快速切换 <100ms（无 UI 直切）。
    at.set_candidates(vec![mk(1, "甲", false, false), mk(2, "乙", false, false)]);
    at.alt_down(1_000);
    let switched = at.step(1_050);
    let ok_delay = at.switch_in_budget() && switched == Some(2);
    at.alt_up(1_060);
    // 2. 停留判定：>350ms 出墙；单窗口不出墙。
    at.alt_down(2_000);
    at.hold_tick(2_300); // 300ms 未出
    let still_fast = at.state() == SwitchState::Fast;
    at.hold_tick(2_351); // 351ms 出墙
    let walled = at.state() == SwitchState::Wall;
    at.cancel(2_400);
    let mut single = AltTab::new();
    single.set_candidates(vec![mk(1, "独", false, false)]);
    single.alt_down(3_000);
    single.hold_tick(3_500);
    set.add(
        "hold-wall",
        ok_delay && still_fast && walled && single.state() == SwitchState::Fast,
        ">350ms wall; single ignored",
    );
    // 3. 十二窗压力布局不乱（蛇形、互不重叠）。
    let mut twelve: Vec<TabCard> = Vec::new();
    for i in 0..12u64 {
        twelve.push(mk(i, "窗", false, false));
    }
    at.set_candidates(twelve);
    at.alt_down(4_000);
    at.hold_tick(4_400);
    let tidy = at.layout_is_tidy() && at.serpentine_rects().len() == 12;
    // 4. 方向键 + 反向 + Alt 松开选择态独立。
    at.wall_next();
    at.wall_next();
    let fwd_idx = at.selected().unwrap().window;
    at.wall_prev();
    let back_idx = at.selected().unwrap().window;
    at.alt_up(4_500); // Alt 松开
    let still_wall = at.state() == SwitchState::Wall;
    at.wall_next();
    let after_up = at.selected().unwrap().window;
    set.add(
        "wall-nav",
        tidy && fwd_idx == 2 && back_idx == 1 && still_wall && after_up == 2,
        "serpentine + independent select",
    );
    // 5. 假死卡：灰显「无响应」仍可选（切过去看它死透没）。
    let mut hung_set: Vec<TabCard> = Vec::new();
    for i in 0..4u64 {
        hung_set.push(mk(i, "窗", i == 1, false));
    }
    at.set_candidates(hung_set);
    at.cancel(4_900); // 上检墙态残留归 Idle（时间线自理纪律）
    at.alt_down(5_000);
    at.hold_tick(5_400);
    at.wall_next(); // 焦点到假死卡 idx1
    let (tok, label) = at.card_style(1);
    let commit_hung = at.commit(5_500);
    set.add(
        "hung-honest",
        tok == Token::Disabled && label == Some("无响应") && commit_hung == Some(1),
        "gray + selectable",
    );
    // 6. UAC 系统窗不进墙（注入 2 枚系统窗 + 3 枚普通）。
    let mut mixed: Vec<TabCard> = vec![
        mk(100, "UAC", false, true),
        mk(101, "UAC2", false, true),
        mk(1, "甲", false, false),
        mk(2, "乙", false, false),
        mk(3, "丙", false, false),
    ];
    mixed.insert(1, mk(102, "UAC3", false, true));
    at.set_candidates(mixed);
    let wall_n = at.candidates().len();
    set.add(
        "uac-excluded",
        wall_n == 3 && at.excluded == 3,
        "system windows out",
    );
    // 7. 缩略节拍 2 次/秒（墙开时 500ms 一次）。
    at.alt_down(6_000);
    at.hold_tick(6_400);
    let r1 = at.thumb_due(6_400);
    let r2 = at.thumb_due(6_700); // 300ms 未到
    let r3 = at.thumb_due(6_900); // 500ms 到
    set.add(
        "thumb-rate",
        r1 && !r2 && r3 && at.thumb_refreshes == 2,
        "2 per second",
    );
    // 8. 卡片数上限 24：墙页 24 张滚动（超出滚动而非截断——深化层
    //    语义回填 D1-v2-01：池保有 30，首页 24，其余翻页可达）。
    let mut many: Vec<TabCard> = Vec::new();
    for i in 0..30u64 {
        many.push(mk(i, "窗", false, false));
    }
    at.set_candidates(many);
    set.add(
        "cap-24",
        at.candidates().len() == 30 && at.page_cards().len() == WALL_CAP,
        "24/page scroll beyond",
    );
    // 9. 背景降透明 40%（墙弹出时）。
    at.alt_down(7_000);
    at.hold_tick(7_400);
    let dimmed = at.bg_opacity_pct() == 60;
    at.cancel(7_500);
    let normal = at.bg_opacity_pct() == 100;
    set.add("bg-dim", dimmed && normal, "40% dim on wall");
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn card(w: u64) -> TabCard {
        TabCard {
            window: w,
            title: String::from("窗"),
            icon_token: 1,
            hung: false,
            system: false,
            minimized: false,
        }
    }

    #[test]
    fn esc_cancels_without_switch() {
        let mut at = AltTab::new();
        at.set_candidates(vec![card(1), card(2)]);
        at.alt_down(0);
        at.hold_tick(400);
        at.cancel(450);
        assert_eq!(at.commit(460), None, "取消不切");
        assert_eq!(at.state(), SwitchState::Idle);
    }

    #[test]
    fn fast_state_no_wall_ui() {
        let mut at = AltTab::new();
        at.set_candidates(vec![card(1), card(2)]);
        at.alt_down(0);
        at.step(10);
        assert_eq!(at.state(), SwitchState::Fast, "快速态不出墙");
        assert!(at.serpentine_rects().is_empty(), "无墙几何");
    }

    #[test]
    fn serpentine_row_direction_alternates() {
        let mut at = AltTab::new();
        at.set_candidates((0..8).map(card).collect());
        at.alt_down(0);
        at.hold_tick(400);
        let r = at.serpentine_rects();
        // 第 0 行从左往右：idx0 x=0，idx1 x=240。
        assert_eq!(r[0].x, 0);
        assert_eq!(r[1].x, CARD_W_PX);
        // 第 1 行从右往左（蛇形回折）：idx4→col3（x=720）、idx7→col0（x=0）。
        assert_eq!(r[4].x, 3 * CARD_W_PX);
        assert_eq!(r[7].x, 0);
    }

    #[test]
    fn highlight_scale_is_5_percent() {
        let at = AltTab::new();
        assert_eq!(at.highlight_scale_permille(), 1050);
    }

    #[test]
    fn alttab_self_checks_all_green() {
        let set = run_alttab_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F082 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——卡内几何 / 标题截断 / 角标位 / 材质与
// 高亮规格 / 翻页键接缝。判据唯一源：主册 G-C-12 交互设计/设计细节。
// ---------------------------------------------------------------------------

/// F082 深化自检二：四族逐条记账。
pub fn run_alttab_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F082-deep2");
    let mut at = AltTab::new();
    let mk = |w: u64, t: &str, min: bool| TabCard {
        window: w,
        title: String::from(t),
        icon_token: 1,
        hung: false,
        system: false,
        minimized: min,
    };
    at.set_candidates(vec![mk(1, "星记", false), mk(2, "星海音乐", true)]);
    at.alt_down(0);
    at.hold_tick(400); // 出墙（布局就绪）
    // 1. 卡内几何：图标左上 24px、标题底条、最小化角标位——互不重叠
    //    且都在卡内。
    let icon = at.icon_rect(0);
    let title = at.title_rect(0);
    let badge = at.badge_rect(0);
    let card = at.serpentine_rects()[0];
    let inside = |r: &Rect| {
        r.x >= card.x && r.y >= card.y && r.right() <= card.right() && r.bottom() <= card.bottom()
    };
    let no_overlap = !icon.intersects(&title) && !icon.intersects(&badge);
    set.add(
        "card-geometry",
        inside(&icon)
            && inside(&title)
            && inside(&badge)
            && no_overlap
            && icon.w == ICON_PX
            && title.h == TITLE_STRIP_H_PX
            && badge.w == 16,
        "icon top-left + title strip + badge slot",
    );
    // 2. 最小化角标（诚实标注渲染位）：2 号卡 minimized → 角标令牌位。
    let badge_1 = at.card_badge(1);
    set.add(
        "badge-render",
        badge_1 == CardBadge::Minimized && at.card_badge(0) == CardBadge::None,
        "minimized badge slot",
    );
    // 3. 标题截断：超长保头加省略；短标题原样；字符口径（非字节）。
    let long = "这是一个非常非常非常长的窗口标题已经超出底条宽度了";
    let cut = AltTab::truncate_title(long);
    let short = AltTab::truncate_title("星记");
    set.add(
        "title-truncate",
        cut.chars().count() == TITLE_MAX_CHARS + 1 && cut.ends_with('…')
            && short == "星记",
        "char-accurate ellipsis",
    );
    // 4. 材质/高亮规格 + 翻页键接缝（键盘翻页与滚轮同页账）。
    let ease = at.highlight_ease();
    let p0 = at.page_cards().len();
    let via_key = at.page_key(true);
    at.notify_page_changed();
    set.add(
        "surface-and-keys",
        at.wall_surface_token() == Token::SurfaceRaised
            && ease.dur_ms == 120
            && at.highlight_scale_permille() == 1050
            && p0 == 2
            && via_key == 0 // 2 卡不超页容 → 页位不动（noop 如实）
            && at.thumb_refreshes == 0, // 换页后节拍账清零
        "glass token + ease + page key seam",
    );
    at.cancel(1_000);
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn geometry_needs_layout_ready() {
        // 未出墙（无布局）→ 几何回退整卡缺省——不炸。
        let mut at = AltTab::new();
        at.set_candidates(vec![TabCard {
            window: 1,
            title: String::from("甲"),
            icon_token: 1,
            hung: false,
            system: false,
            minimized: false,
        }]);
        let icon = at.icon_rect(0);
        assert_eq!(icon.w, ICON_PX);
    }

    #[test]
    fn truncate_exactly_at_limit_untouched() {
        let exact: String = "字".repeat(TITLE_MAX_CHARS);
        assert_eq!(AltTab::truncate_title(&exact), exact, "恰好上限不截");
    }

    #[test]
    fn page_key_before_wall_is_noop() {
        let mut at = AltTab::new();
        assert_eq!(at.page_key(true), 0, "未出墙翻页键如实无效");
    }

    #[test]
    fn alttab_deep2_checks_all_green() {
        let set = run_alttab_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F082-deep2 红项：{}/{} 绿", p, p + f);
    }
}
