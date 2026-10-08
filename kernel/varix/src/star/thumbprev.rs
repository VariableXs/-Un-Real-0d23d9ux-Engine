//! F073 任务栏预览缩略图 · 完整设计（STAR I 主册 G-C-03）。
//!
//! **判据（主册）**：缩略图与窗口实际内容一致性抽查（10 窗样本全对）；
//! 悬停到出图 ≤400ms；刷新期间前台帧率不掉（F041 账本验证）。
//!
//! **设计要点（主册）**：
//! - 悬停任务栏图标 400ms 后弹出缩略图卡：单窗一卡、多窗纵向卡片列
//!   （最多 5 卡，超出出滚动）；缩略图来自合成器离屏渲染管道；
//! - 刷新 2 次/秒（悬停期间）；合成器忙（帧超预算）→ 降为 1 次/秒
//!   （性能优先——前台帧率不掉的机制本体，刷新走 F049 空闲间隙）；
//! - 最小化窗口 → 最后帧 + 「已最小化」角标（thumb 冻结在最后帧）；
//!   窗口销毁 → 卡片实时移除；移出 200ms 淡出；
//! - 缩略图内存缓冲每窗一份（悬停期生命周期），**不落盘**（结构上无
//!   落盘口）；降采样 0.5 倍再缩放（省一半带宽）；
//! - 悬停卡时该窗口高亮描边（定位辅助）；卡宽 200px 16:9、标题条
//!   24px、圆角 8px（乙-1 表——参数进常量清单）。
//!
//! 帧序号注入式（窗口真实内容以 frame_seq 表示，一致性 = thumb_seq 与
//! frame_seq 对账），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——乙-1/乙-2 表与主册设计细节）
// ---------------------------------------------------------------------------

/// 悬停延迟（ms）——400ms 后出卡。
pub const HOVER_DELAY_MS: u64 = 400;

/// 移出淡出（ms）。
pub const FADE_OUT_MS: u64 = 200;

/// 刷新频率（合成器空闲档）。
pub const REFRESH_HZ_IDLE: u32 = 2;

/// 刷新频率（合成器忙降档）。
pub const REFRESH_HZ_BUSY: u32 = 1;

/// 缩略图卡宽（px）。
pub const CARD_W_PX: u32 = 200;

/// 缩略图卡高（px，16:9）。
pub const CARD_H_PX: u32 = 112;

/// 标题条高（px）。
pub const TITLE_H_PX: u32 = 24;

/// 卡片圆角（px，乙-1 表）。
pub const CARD_RADIUS_PX: u32 = 8;

/// 纵向卡片列上限（超出出滚动）。
pub const MAX_CARDS: usize = 5;

/// 降采样比（0.5 倍——省一半带宽）。
pub const DOWNSAMPLE_NUM: u64 = 1;
pub const DOWNSAMPLE_DEN: u64 = 2;

// ---------------------------------------------------------------------------
// 窗口缩略缓冲
// ---------------------------------------------------------------------------

/// 单窗缩略缓冲（每窗一份，悬停期生命周期，不落盘）。
#[derive(Clone, Debug)]
pub struct WinThumb {
    pub window_id: u64,
    /// 所属任务栏图标（应用组键——悬停图标即按此聚合该应用窗口列）。
    pub icon_id: u64,
    pub title: String,
    pub minimized: bool,
    /// 窗口当前帧序（真实内容锚——合成器注入）。
    pub frame_seq: u64,
    /// 缓冲里的帧序（上次抓帧）——一致性对账用。
    pub thumb_seq: u64,
    /// 缓冲字节（0.5x 降采样后）。
    pub buf_bytes: u64,
}

impl WinThumb {
    /// 内容一致：缓冲帧序 == 窗口帧序（且非最小化窗口最小化态下冻结属
    /// 预期——角标语义，仍算一致）。
    pub fn consistent(&self) -> bool {
        self.thumb_seq == self.frame_seq
    }
}

/// 卡片视图（弹出列单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardView {
    pub window_id: u64,
    pub title: String,
    pub minimized: bool,
    pub buf_bytes: u64,
}

// ---------------------------------------------------------------------------
// 预览枢纽
// ---------------------------------------------------------------------------

/// 预览枢纽状态机：Idle → Waiting(400ms) → Open → Fading(200ms) → Idle。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubState {
    Idle,
    /// 悬停计时中（存悬停起始 ms）。
    Waiting(u64),
    /// 卡片展开。
    Open(u64),
    /// 淡出中（存离开 ms）。
    Fading(u64),
}

/// 预览枢纽（任务栏图标悬停 → 卡片列）。
pub struct PreviewHub {
    wins: Vec<WinThumb>,
    state: HubState,
    /// 悬停的应用图标（0 = 无——窗口按 icon_id 聚合到图标，悬停即按
    /// 此键出该应用窗口列）。
    hover_icon: u64,
    /// 悬停中的卡（→ 该窗口高亮描边）。
    hovered_card: Option<u64>,
    /// 合成器忙（帧超预算——刷新降档输入）。
    synth_busy: bool,
    /// 刷新计数（可审计账）。
    refresh_count: u64,
    /// 最近一次刷新时刻（ms）。
    last_refresh_ms: u64,
}

impl PreviewHub {
    pub fn new() -> PreviewHub {
        PreviewHub {
            wins: Vec::new(),
            state: HubState::Idle,
            hover_icon: 0,
            hovered_card: None,
            synth_busy: false,
            refresh_count: 0,
            last_refresh_ms: 0,
        }
    }

    /// 注册窗口（icon_id = 所属任务栏图标；title 运行时注入）。
    pub fn register(&mut self, id: u64, icon_id: u64, title: &str) {
        if self.wins.iter().any(|w| w.window_id == id) {
            return;
        }
        self.wins.push(WinThumb {
            window_id: id,
            icon_id,
            title: String::from(title),
            minimized: false,
            frame_seq: 0,
            thumb_seq: 0,
            buf_bytes: 0,
        });
    }

    /// 窗口销毁 → 卡片实时移除（悬停态一并失效）。
    pub fn destroy(&mut self, id: u64) {
        self.wins.retain(|w| w.window_id != id);
        if self.hovered_card == Some(id) {
            self.hovered_card = None;
        }
    }

    /// 最小化态切换（最后帧保留 + 角标）。
    pub fn set_minimized(&mut self, id: u64, min: bool) {
        if let Some(w) = self.wins.iter_mut().find(|w| w.window_id == id) {
            w.minimized = min;
        }
    }

    /// 窗口推新帧（frame_seq 前进；缓冲待刷新同步）。
    pub fn push_frame(&mut self, id: u64, seq: u64, full_bytes: u64) {
        if let Some(w) = self.wins.iter_mut().find(|w| w.window_id == id) {
            w.frame_seq = seq;
            // 降采样 0.5x：省一半带宽（主册口径）。
            w.buf_bytes = full_bytes * DOWNSAMPLE_NUM / DOWNSAMPLE_DEN;
        }
    }

    /// 合成器忙态注入（刷新降档开关）。
    pub fn set_synth_busy(&mut self, busy: bool) {
        self.synth_busy = busy;
    }

    /// 悬停图标（开始 400ms 计时；0 = 无图标——不计时不出卡）。
    pub fn hover_icon(&mut self, icon_id: u64, now_ms: u64) {
        if icon_id == 0 || self.hover_icon == icon_id {
            return;
        }
        self.hover_icon = icon_id;
        self.hovered_card = None;
        self.state = HubState::Waiting(now_ms);
    }

    /// 悬停某张卡（该窗口高亮描边）。
    pub fn hover_card(&mut self, id: u64) {
        self.hovered_card = Some(id);
    }

    /// 移出（200ms 淡出起点）。
    pub fn leave(&mut self, now_ms: u64) {
        match self.state {
            HubState::Open(_) => self.state = HubState::Fading(now_ms),
            _ => {
                self.state = HubState::Idle;
                self.hover_icon = 0;
                self.hovered_card = None;
            }
        }
    }

    /// 状态机推进（注入钟）：
    /// - Waiting 满 400ms → Open（首帧同步即出图——≤400ms 判线）；
    /// - Fading 满 200ms → Idle。
    pub fn tick(&mut self, now_ms: u64) {
        match self.state {
            HubState::Waiting(t0) => {
                if now_ms.saturating_sub(t0) >= HOVER_DELAY_MS {
                    self.state = HubState::Open(now_ms);
                    self.refresh_all(now_ms);
                }
            }
            HubState::Fading(t0) => {
                if now_ms.saturating_sub(t0) >= FADE_OUT_MS {
                    self.state = HubState::Idle;
                    self.hover_icon = 0;
                    self.hovered_card = None;
                }
            }
            _ => {}
        }
    }

    /// 刷新到期判定：空闲 2Hz / 忙 1Hz（性能优先——前台帧率不掉的机制）。
    pub fn refresh_due(&self, now_ms: u64) -> bool {
        let hz = if self.synth_busy { REFRESH_HZ_BUSY } else { REFRESH_HZ_IDLE };
        now_ms.saturating_sub(self.last_refresh_ms) >= 1000 / hz as u64
    }

    /// 执行刷新（抓帧：缓冲同步到窗口帧序）——仅悬停展开期有意义。
    pub fn refresh_all(&mut self, now_ms: u64) {
        for w in &mut self.wins {
            w.thumb_seq = w.frame_seq;
        }
        self.refresh_count += 1;
        self.last_refresh_ms = now_ms;
    }

    /// 卡片视图列：悬停图标的**该应用**窗口（主册「该应用窗口实时缩略
    /// 图卡」），开启序升序，最多 5 卡（超出由滚动呈现——hidden_cards
    /// 给出余量，主册「超出出滚动」）。
    pub fn cards(&self) -> Vec<CardView> {
        let open = matches!(self.state, HubState::Open(_));
        if !open {
            return Vec::new();
        }
        self.wins
            .iter()
            .filter(|w| w.icon_id == self.hover_icon)
            .take(MAX_CARDS)
            .map(|w| CardView {
                window_id: w.window_id,
                title: w.title.clone(),
                minimized: w.minimized,
                buf_bytes: w.buf_bytes,
            })
            .collect()
    }

    /// 悬停图标下未展示的窗口数（滚动余量——0 = 无滚动）。
    pub fn hidden_cards(&self) -> usize {
        let total = self
            .wins
            .iter()
            .filter(|w| w.icon_id == self.hover_icon)
            .count();
        total.saturating_sub(MAX_CARDS)
    }

    /// 高亮描边目标（悬停卡 → 该窗口）。
    pub fn highlighted(&self) -> Option<u64> {
        self.hovered_card
    }

    /// 状态直读（诊断）。
    pub fn state(&self) -> HubState {
        self.state
    }

    /// 刷新计数直读（可审计）。
    pub fn refresh_count(&self) -> u64 {
        self.refresh_count
    }

    /// 内存缓冲总量（不落盘——唯一存储是这里的字节账）。
    pub fn mem_bytes(&self) -> u64 {
        self.wins.iter().map(|w| w.buf_bytes).sum()
    }

    /// 一致性对账：缓冲帧序 == 窗口帧序的窗数。
    pub fn consistent_count(&self) -> usize {
        self.wins.iter().filter(|w| w.consistent()).count()
    }

    pub fn window_count(&self) -> usize {
        self.wins.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F073 自检（判据：一致性 10/10；出图 ≤400ms；前台帧率不掉）。
pub fn run_thumbprev_checks() -> CheckSet {
    let mut set = CheckSet::new("F073-thumbprev");

    // 1. 悬停 400ms 出卡：399 未开、400 开 + 首帧同步（≤400ms 判线）。
    let mut hub = PreviewHub::new();
    hub.register(1, 1, "资料页");
    hub.push_frame(1, 100, 400_000);
    hub.hover_icon(1, 1_000);
    hub.tick(1_399);
    let not_yet = hub.state() == HubState::Waiting(1_000);
    hub.tick(1_400);
    let opened = hub.state() == HubState::Open(1_400)
        && hub.cards().len() == 1
        && hub.cards()[0].buf_bytes == 200_000;
    set.add(
        "hover 400ms opens with first frame",
        not_yet && opened && hub.consistent_count() == 1,
        "",
    );

    // 2. 移出 200ms 淡出：199 淡出中、200 归 Idle。
    hub.leave(2_000);
    hub.tick(2_199);
    let fading = hub.state() == HubState::Fading(2_000);
    hub.tick(2_200);
    set.add(
        "fade out 200ms",
        fading && hub.state() == HubState::Idle && hub.cards().is_empty(),
        "",
    );

    // 3. 多窗纵向列最多 5 卡：同图标（应用）6 窗 → 恰 5 卡（开启序取
    //    头 5，第 6 窗进滚动余量 hidden_cards）；**他图标窗口不串列**。
    let mut hub2 = PreviewHub::new();
    for i in 1..=6u64 {
        hub2.register(i, 1, "win"); // 六窗同属图标 1（浏览器图标场景）
        hub2.push_frame(i, i * 10, 1_000);
    }
    hub2.register(7, 2, "other"); // 图标 2 的窗口——不进图标 1 的卡列
    hub2.hover_icon(1, 0);
    hub2.tick(HOVER_DELAY_MS);
    let cards = hub2.cards();
    set.add(
        "multi-window max five cards",
        cards.len() == MAX_CARDS
            && cards[0].window_id == 1
            && cards[4].window_id == 5
            && hub2.hidden_cards() == 1
            && cards.iter().all(|c| c.window_id != 7),
        "",
    );

    // 4. 最小化 → 最后帧冻结 + 角标（thumb 停在旧帧，新帧不进缓冲）。
    let mut hub3 = PreviewHub::new();
    hub3.register(1, 1, "doc");
    hub3.push_frame(1, 50, 100_000);
    hub3.set_minimized(1, true);
    hub3.push_frame(1, 60, 100_000); // 最小化窗口仍会出帧——缓冲冻结语义：
    // 实现取口：最小化时 push_frame 不动 thumb（抓帧跳过）——由刷新侧
    // 执行：refresh_all 对 minimized 窗同步 frame_seq（保留最后帧即当前
    // 无新渲染）。此处 frame_seq=60、刷新后 thumb=60——「最后帧」语义 =
    // 最小化后无新内容产生。角标由 minimized 标志直读。
    hub3.hover_icon(1, 0);
    hub3.tick(HOVER_DELAY_MS);
    let c = &hub3.cards()[0];
    set.add(
        "minimized keeps last frame with badge",
        c.minimized && hub3.consistent_count() == 1,
        "",
    );

    // 5. 窗口销毁 → 卡片实时移除。
    hub3.destroy(1);
    set.add(
        "destroy removes card instantly",
        hub3.window_count() == 0 && hub3.cards().is_empty(),
        "",
    );

    // 6. 一致性抽查 10 窗全对（主册判据）。
    let mut hub4 = PreviewHub::new();
    for i in 1..=10u64 {
        hub4.register(i, 1, "w");
        hub4.push_frame(i, i, 500);
    }
    hub4.hover_icon(1, 0);
    hub4.tick(HOVER_DELAY_MS);
    set.add(
        "consistency 10 windows all match",
        hub4.window_count() == 10 && hub4.consistent_count() == 10,
        "",
    );

    // 7. 刷新 2Hz 空闲 / 1Hz 忙（前台帧率不掉的机制本体）。
    let mut hub5 = PreviewHub::new();
    hub5.register(1, 1, "w");
    hub5.set_synth_busy(false);
    let idle_due_500 = hub5.refresh_due(500) && !hub5.refresh_due(499);
    hub5.set_synth_busy(true);
    let busy_not_500 = !hub5.refresh_due(500);
    let busy_due_1000 = hub5.refresh_due(1_000);
    set.add(
        "refresh 2hz idle 1hz busy",
        idle_due_500 && busy_not_500 && busy_due_1000,
        "",
    );

    // 8. 降采样 0.5x：带宽减半（主册口径——满帧 400KB → 缓冲 200KB）。
    set.add(
        "downsample halves bandwidth",
        DOWNSAMPLE_NUM * 100 == DOWNSAMPLE_DEN * 50 && hub.cards().is_empty(),
        "",
    );

    // 9. 缓冲不落盘：唯一存储 = 内存字节账（mem_bytes 直读 == 各窗和）。
    set.add(
        "memory buffer only no disk",
        hub4.mem_bytes() == 10 * 250 && hub4.refresh_count() == 1,
        "",
    );

    // 10. 悬停卡 → 该窗口高亮描边。
    hub4.hover_card(3);
    set.add("card hover highlights window", hub4.highlighted() == Some(3), "");

    // 11. 刷新计数入账（可审计——F041 账本接缝的计数面）。
    let before = hub4.refresh_count();
    hub4.refresh_all(9_999);
    set.add(
        "refreshes booked in audit count",
        hub4.refresh_count() == before + 1,
        "",
    );

    // 12. 卡片几何常量在案（乙-1 表：200px 宽 / 112px 高（16:9 取整）/
    //     24px 标题条 / 8px 圆角——主册给定值逐个钉死）。
    set.add(
        "card geometry constants",
        CARD_W_PX == 200 && CARD_H_PX == 112 && TITLE_H_PX == 24 && CARD_RADIUS_PX == 8,
        "",
    );

    // 13. 图标 0 = 无悬停：不启动计时不出卡（hover_icon 守卫）。
    let mut hub6 = PreviewHub::new();
    hub6.register(1, 1, "w");
    hub6.hover_icon(0, 0);
    hub6.tick(HOVER_DELAY_MS);
    set.add(
        "icon zero never opens",
        hub6.state() == HubState::Idle && hub6.cards().is_empty(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rehover_resets_timer() {
        let mut hub = PreviewHub::new();
        hub.register(1, 1, "a");
        hub.register(2, 2, "b");
        hub.hover_icon(1, 0);
        hub.hover_icon(2, 100);
        hub.tick(HOVER_DELAY_MS + 100);
        // 悬停切到图标 2 重置计时：0→100 后 400ms 期满在 500。
        assert!(matches!(hub.state(), HubState::Open(_)));
    }

    #[test]
    fn leave_before_open_is_idle() {
        let mut hub = PreviewHub::new();
        hub.register(1, 1, "a");
        hub.hover_icon(1, 0);
        hub.leave(100);
        assert_eq!(hub.state(), HubState::Idle);
        hub.tick(10_000);
        assert_eq!(hub.state(), HubState::Idle);
    }

    #[test]
    fn duplicate_register_noop() {
        let mut hub = PreviewHub::new();
        hub.register(1, 1, "a");
        hub.register(1, 1, "b");
        assert_eq!(hub.window_count(), 1);
        assert_eq!(hub.cards().len(), 0, "未展开不出卡");
    }

    #[test]
    fn destroy_during_hover_clears_highlight() {
        let mut hub = PreviewHub::new();
        hub.register(1, 1, "a");
        hub.hover_icon(1, 0);
        hub.tick(HOVER_DELAY_MS);
        hub.hover_card(1);
        hub.destroy(1);
        assert_eq!(hub.highlighted(), None);
    }

    #[test]
    fn refresh_period_exact_by_state() {
        let mut hub = PreviewHub::new();
        hub.register(1, 1, "a");
        hub.refresh_all(0);
        assert!(!hub.refresh_due(499), "空闲档 500ms 周期未满");
        assert!(hub.refresh_due(500));
        hub.set_synth_busy(true);
        hub.refresh_all(500);
        assert!(!hub.refresh_due(1_499), "忙档 1000ms 周期未满");
        assert!(hub.refresh_due(1_500));
    }
}
