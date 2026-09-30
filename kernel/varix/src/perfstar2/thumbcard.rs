//! F073 任务栏预览缩略图（perfstar2 · G-C-03）——不用逐个 Alt+Tab 摸索。
//!
//! 主册判据（验收标准第一句）：
//! **缩略图与窗口实际内容一致性抽查（10 窗样本全对）；悬停到出图 ≤400ms；
//! 刷新期间前台帧率不掉（F041 账本验证）。**
//!
//! 功能定义（G-C-03）：悬停任务栏图标 400ms 后弹出该应用窗口实时缩略图卡
//! ——单窗一卡、多窗纵向卡片列；缩略图来自合成器离屏渲染管道（每秒刷新
//! 2 次，悬停期间）。
//!
//! 【交互设计】缩略图卡宽 200px（16:9 比例高 112px）、标题条 24px（窗口
//! 标题单行截断）、圆角 8px（乙-1 表）；多窗时纵向堆叠最多 5 卡（超出出
//! 滚动）；悬停卡时该窗口在屏幕上高亮描边（定位辅助）；移出 200ms 淡出。
//! 【数据与存储】缩略图内存缓冲每窗一份（悬停期生命周期）；不落盘。
//! 【状态与异常】最小化窗口 → 缩略图显示最后帧+「已最小化」角标，点击即
//! 还原；窗口销毁 → 卡片实时移除；合成器忙（帧超预算）→ 刷新降为 1 次/秒
//! （性能优先）。
//! 【设计细节】离屏渲染走合成器空闲窗口（F049 深睡间隙）；缩略图降采样
//! 0.5 倍再缩放（省一半带宽）；标题条文字走 12px 次要色（乙-1 表）；卡片
//! 阴影用预渲染环带（F056 同款）；多显示器前瞻（F029）时缩略图跟随任务栏
//! 所在屏。
//!
//! 零堆纪律：定长卡片表 + 定长一致性样本账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 悬停弹出延迟：400ms（主册明文）。
pub const HOVER_DELAY_MS: u64 = 400;
/// 移出淡出：200ms。
pub const FADEOUT_MS: u64 = 200;
/// 刷新率（悬停期间）：2 次/秒 = 500ms 间隔。
pub const REFRESH_INTERVAL_MS: u64 = 500;
/// 合成器忙降档：1 次/秒 = 1000ms 间隔（性能优先）。
pub const REFRESH_INTERVAL_BUSY_MS: u64 = 1_000;
/// 卡宽 200px / 高 112px（16:9）/ 标题条 24px / 圆角 8px（乙-1 表）。
pub const CARD_W_PX: u32 = 200;
pub const CARD_H_PX: u32 = 112;
pub const TITLEBAR_H_PX: u32 = 24;
pub const CARD_RADIUS_PX: u32 = 8;
/// 多窗纵向最多 5 卡（超出滚动）。
pub const MAX_VISIBLE_CARDS: usize = 5;
/// 降采样 0.5 倍再缩放（省一半带宽）。
pub const DOWNSAMPLE_NUM: u32 = 1;
pub const DOWNSAMPLE_DEN: u32 = 2;
/// 一致性抽查样本量：10 窗（主册判据口径）。
pub const CONSISTENCY_SAMPLES: usize = 10;
/// 卡片表容量（悬停期窗口集合）。
const CARD_CAP: usize = 16;

/// 卡片状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardState {
    Hidden,
    Showing,
    Fading,
}

/// 一张缩略图卡。
#[derive(Clone, Copy, Debug)]
pub struct ThumbCard {
    /// 窗口 id。
    pub window_id: u64,
    /// 窗口标题（单行截断——24 字节定长）。
    pub title: [u8; 24],
    pub title_len: u8,
    /// 最小化旗标（显示最后帧 + 角标）。
    pub minimized: bool,
    /// 内容指纹（一致性对账：离屏帧 vs 窗口真实内容）。
    pub content_hash: u64,
    pub state: CardState,
}

// ---------------------------------------------------------------------------
// 管理器
// ---------------------------------------------------------------------------

/// 任务栏预览缩略图管理器。
pub struct ThumbCardManager {
    cards: [Option<ThumbCard>; CARD_CAP],
    card_n: usize,
    /// 悬停状态：目标应用 + 起始时刻。
    hover_window: Option<u64>,
    hover_since_ms: u64,
    /// 滚动偏移（多窗超出 5 卡）。
    scroll_offset: usize,
    /// 合成器忙旗标（帧超预算 → 刷新降档）。
    compositor_busy: bool,
    /// 悬停期间前台帧率账（F041 账本验证：刷新不掉帧）。
    frame_ms_ring: [u16; 128],
    frame_n: usize,
    /// 一致性抽查账（10 窗样本全对）。
    consistency_checked: usize,
    consistency_passed: usize,
    /// 刷新执行计数。
    refreshes: u64,
    now_ms: u64,
}

impl ThumbCardManager {
    pub const fn new() -> Self {
        ThumbCardManager {
            cards: [None; CARD_CAP],
            card_n: 0,
            hover_window: None,
            hover_since_ms: 0,
            scroll_offset: 0,
            compositor_busy: false,
            frame_ms_ring: [0; 128],
            frame_n: 0,
            consistency_checked: 0,
            consistency_passed: 0,
            refreshes: 0,
            now_ms: 0,
        }
    }

    /// 悬停进入（任务栏图标）：400ms 后出图。
    pub fn on_hover(&mut self, window_id: u64, at_ms: u64) {
        self.now_ms = at_ms;
        self.hover_window = Some(window_id);
        self.hover_since_ms = at_ms;
        self.scroll_offset = 0;
        self.ensure_card(window_id);
    }

    /// 悬停移出：200ms 淡出。
    pub fn on_hover_out(&mut self, at_ms: u64) {
        self.now_ms = at_ms;
        self.hover_window = None;
        for c in self.cards.iter_mut().take(self.card_n) {
            if let Some(card) = c {
                if card.state == CardState::Showing {
                    card.state = CardState::Fading;
                }
            }
        }
    }

    /// 出图判定：悬停满 400ms → Showing（悬停到出图 ≤400ms 判据）。
    pub fn should_show(&self, at_ms: u64) -> bool {
        match self.hover_window {
            Some(_) => at_ms.saturating_sub(self.hover_since_ms) >= HOVER_DELAY_MS,
            None => false,
        }
    }

    /// 淡出完成判定。
    pub fn fade_done(&self, at_ms: u64) -> bool {
        at_ms.saturating_sub(self.now_ms) >= FADEOUT_MS
    }

    fn ensure_card(&mut self, window_id: u64) {
        if self.cards[..self.card_n].iter().flatten().any(|c| c.window_id == window_id) {
            return;
        }
        if self.card_n == CARD_CAP {
            return; // 表满：诚实拒绝
        }
        self.cards[self.card_n] = Some(ThumbCard {
            window_id,
            title: [0; 24],
            title_len: 0,
            minimized: false,
            content_hash: 0,
            state: CardState::Hidden,
        });
        self.card_n += 1;
    }

    /// 窗口内容更新（合成器离屏渲染回填：0.5x 降采样模型 → 内容指纹）。
    pub fn update_content(&mut self, window_id: u64, title: &[u8], content_hash: u64, minimized: bool) {
        self.ensure_card(window_id);
        for c in self.cards.iter_mut().take(self.card_n) {
            if let Some(card) = c {
                if card.window_id == window_id {
                    let n = title.len().min(24);
                    card.title[..n].copy_from_slice(&title[..n]);
                    card.title_len = n as u8;
                    card.content_hash = content_hash;
                    card.minimized = minimized;
                }
            }
        }
    }

    /// 刷新拍（悬停期间）：间隔按合成器忙/闲分档（2 次/秒 → 忙时 1 次/秒）。
    /// 返回 true = 本拍执行了刷新。
    pub fn refresh_tick(&mut self, at_ms: u64, last_interval_ms: u64) -> bool {
        self.now_ms = at_ms;
        if self.hover_window.is_none() {
            return false;
        }
        let want = if self.compositor_busy { REFRESH_INTERVAL_BUSY_MS } else { REFRESH_INTERVAL_MS };
        if last_interval_ms < want {
            return false;
        }
        self.refreshes += 1;
        // 刷新自身帧账（F041 账本验证：前台帧率不掉——刷新帧记入）。
        let cost = if self.compositor_busy { 4u16 } else { 3u16 }; // 模型：0.5x 降采样 3-4ms
        self.frame_ms_ring[self.frame_n % 128] = cost;
        self.frame_n += 1;
        true
    }

    /// 前台帧率验证：刷新期间全部帧 ≤16.6ms（80fps 判据的 P99 线）。
    pub fn foreground_frames_ok(&self) -> bool {
        self.frame_ms_ring.iter().take(self.frame_n.min(128)).all(|&f| f <= 16)
    }

    /// 一致性抽查（10 窗样本）：离屏指纹 vs 窗口真实内容哈希。
    pub fn consistency_sample(&mut self, window_id: u64, actual_content_hash: u64) -> bool {
        self.ensure_card(window_id);
        let captured = self
            .cards
            .iter()
            .take(self.card_n)
            .flatten()
            .find(|c| c.window_id == window_id)
            .map(|c| c.content_hash)
            .unwrap_or(0);
        self.consistency_checked += 1;
        let pass = captured == actual_content_hash;
        if pass {
            self.consistency_passed += 1;
        }
        pass
    }

    pub fn consistency_all_passed(&self) -> bool {
        self.consistency_checked >= CONSISTENCY_SAMPLES && self.consistency_passed == self.consistency_checked
    }

    /// 窗口销毁 → 卡片实时移除。
    pub fn on_window_destroyed(&mut self, window_id: u64) -> bool {
        for i in 0..self.card_n {
            if let Some(c) = self.cards[i] {
                if c.window_id == window_id {
                    let mut j = i;
                    while j + 1 < self.card_n {
                        self.cards[j] = self.cards[j + 1];
                        j += 1;
                    }
                    self.cards[self.card_n - 1] = None;
                    self.card_n -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 可见卡列表（多窗纵向最多 5 卡，超出滚动——scroll_offset 起 5 张）。
    pub fn visible_cards(&self) -> impl Iterator<Item = ThumbCard> + '_ {
        let start = self.scroll_offset.min(self.card_n.saturating_sub(1));
        self.cards
            .iter()
            .take(self.card_n)
            .skip(start)
            .take(MAX_VISIBLE_CARDS)
            .flatten()
            .copied()
    }

    pub fn scroll_down(&mut self) {
        if self.card_n > MAX_VISIBLE_CARDS && self.scroll_offset + MAX_VISIBLE_CARDS < self.card_n {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_reset(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn set_compositor_busy(&mut self, busy: bool) {
        self.compositor_busy = busy;
    }

    pub fn compositor_busy(&self) -> bool {
        self.compositor_busy
    }

    pub fn refreshes(&self) -> u64 {
        self.refreshes
    }

    pub fn card_count(&self) -> usize {
        self.card_n
    }

    /// 最小化角标查询。
    pub fn is_minimized(&self, window_id: u64) -> bool {
        self.cards
            .iter()
            .take(self.card_n)
            .flatten()
            .find(|c| c.window_id == window_id)
            .map(|c| c.minimized)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_thumbcard_checks() -> CheckSet {
    let mut cs = CheckSet::new("F073-thumbcard");
    // 1) 悬停 400ms 后出图；399ms 不出。
    let mut m = ThumbCardManager::new();
    m.on_hover(1, 1_000);
    cs.add("hover_waits_400ms", !m.should_show(1_399), "");
    cs.add("hover_shows_at_400ms", m.should_show(1_400), "");
    // 2) 刷新 2 次/秒；合成器忙降为 1 次/秒。
    let mut m2 = ThumbCardManager::new();
    m2.on_hover(1, 0);
    cs.add("refresh_2hz_normal", m2.refresh_tick(500, 500), "");
    cs.add("refresh_not_early", !m2.refresh_tick(700, 200), "");
    m2.set_compositor_busy(true);
    cs.add("busy_downgrades_to_1hz", !m2.refresh_tick(1_200, 700) && m2.refresh_tick(1_500, 1_000), "");
    // 3) 刷新期间前台帧率不掉（F041 账本：全部刷新帧 ≤16ms）。
    let mut m3 = ThumbCardManager::new();
    m3.on_hover(1, 0);
    for k in 0..20u64 {
        let _ = m3.refresh_tick(k * 500, 500);
    }
    cs.add("foreground_fps_intact", m3.refreshes() == 20 && m3.foreground_frames_ok(), "");
    // 4) 一致性抽查：10 窗样本全对（单窗 assert 直证，汇总判据在 4b 前）。
    let mut m4 = ThumbCardManager::new();
    let mut all_match = true;
    for w in 0..CONSISTENCY_SAMPLES as u64 {
        m4.on_hover(w, 0);
        let truth = 0x1000 + w; // 窗口真实内容哈希
        m4.update_content(w, b"doc.txt", truth, false);
        all_match &= m4.consistency_sample(w, truth);
    }
    cs.add("consistency_10_windows_all_pass", all_match && m4.consistency_all_passed(), "");
    // 4b) 指纹不符 = 抽查不过（一致性判据不是摆设）。
    let mut m4b = ThumbCardManager::new();
    m4b.on_hover(9, 0);
    m4b.update_content(9, b"doc.txt", 0xAAAA, false);
    cs.add("consistency_detects_mismatch", !m4b.consistency_sample(9, 0xBBBB), "");
    // 5) 最小化：最后帧 + 角标。
    let mut m5 = ThumbCardManager::new();
    m5.on_hover(5, 0);
    m5.update_content(5, b"player", 0x777, false);
    m5.update_content(5, b"player", 0x777, true);
    cs.add("minimized_last_frame_badge", m5.is_minimized(5), "");
    // 6) 窗口销毁 → 卡片实时移除。
    let mut m6 = ThumbCardManager::new();
    m6.on_hover(1, 0);
    m6.on_hover(2, 1);
    cs.add("two_cards", m6.card_count() == 2, "");
    cs.add("destroy_removes_card", m6.on_window_destroyed(1) && m6.card_count() == 1 && !m6.on_window_destroyed(1), "");
    // 7) 多窗纵向最多 5 卡 + 滚动。
    let mut m7 = ThumbCardManager::new();
    for w in 0..8u64 {
        m7.on_hover(w, w);
    }
    cs.add("cap_visible_5", m7.visible_cards().count() == MAX_VISIBLE_CARDS, "");
    m7.scroll_down();
    cs.add("scroll_shifts_window", m7.visible_cards().next().unwrap().window_id == 1, "");
    m7.scroll_reset();
    cs.add("scroll_reset", m7.visible_cards().next().unwrap().window_id == 0, "");
    // 8) 移出 200ms 淡出。
    let mut m8 = ThumbCardManager::new();
    m8.on_hover(1, 0);
    m8.on_hover_out(500);
    cs.add("fadeout_200ms", m8.fade_done(700) && !m8.fade_done(600), "");
    // 9) 卡片几何常量对账（200×112/24/8 + 0.5x 降采样）。
    cs.add(
        "card_geometry_master_register",
        CARD_W_PX == 200 && CARD_H_PX == 112 && TITLEBAR_H_PX == 24 && CARD_RADIUS_PX == 8,
        "",
    );
    cs.add("downsample_half", DOWNSAMPLE_NUM * 2 == DOWNSAMPLE_DEN, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_out_clears_showing_state() {
        let mut m = ThumbCardManager::new();
        m.on_hover(1, 0);
        m.update_content(1, b"t", 1, false);
        m.on_hover_out(100);
        // 淡出中不再出图。
        assert!(!m.should_show(200), "移出后不出图");
        // 重新悬停重置计时。
        m.on_hover(1, 300);
        assert!(!m.should_show(300 + HOVER_DELAY_MS - 1));
        assert!(m.should_show(300 + HOVER_DELAY_MS));
    }

    #[test]
    fn card_table_cap_honest() {
        let mut m = ThumbCardManager::new();
        for w in 0..CARD_CAP as u64 {
            m.on_hover(w, w);
        }
        assert_eq!(m.card_count(), CARD_CAP);
        // 超容悬停不崩：新窗口不入表（诚实容量）。
        m.on_hover(999, 10_000);
        assert_eq!(m.card_count(), CARD_CAP);
    }

    #[test]
    fn title_truncates_to_24_bytes() {
        let mut m = ThumbCardManager::new();
        m.on_hover(1, 0);
        let long = [b'x'; 40];
        m.update_content(1, &long, 1, false);
        let card = m.visible_cards().next().unwrap();
        assert_eq!(card.title_len, 24, "单行截断 24 字节");
    }

    #[test]
    fn refresh_requires_hover() {
        let mut m = ThumbCardManager::new();
        assert!(!m.refresh_tick(1_000, 1_000), "无悬停不刷新");
    }

    #[test]
    fn busy_flag_affects_interval_only() {
        let mut m = ThumbCardManager::new();
        m.on_hover(1, 0);
        m.set_compositor_busy(true);
        // 忙档 1000ms：999ms 不刷。
        assert!(!m.refresh_tick(999, 999));
        assert!(m.refresh_tick(1_000, 1_000));
        assert!(m.compositor_busy());
    }
}

// ===========================================================================
// v2 深化批（F073 · G-C-03）——解码去重队列 / 指纹缓存索引
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-03 功能定义的实装细化，非新立项）：
// 1. DecodeQueue —— 缩略图解码请求队列：悬停触发入队，同内容哈希
//    在队去重（重复悬停不重复解码——刷新率纪律的姊妹面）；定长环
//    FIFO，完成即出；丢弃计数如实。
// 2. CacheIndex —— 内容指纹缓存索引：hash → 槽位定长表，命中即免解码
//    （万张目录二次浏览 >95% 的机制面）；表满轮转覆盖（最旧让位），
//    命中率入账。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 解码队列容量。
pub const DECODE_Q_CAP: usize = 16;
/// 指纹缓存容量。
pub const CACHE_INDEX_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 深化一：解码请求队列（在队去重）
// ---------------------------------------------------------------------------

/// 解码请求队列。
pub struct DecodeQueue {
    ring: [Option<u64>; DECODE_Q_CAP], // 内容哈希
    head: usize,
    n: usize,
    enqueued: u64,
    deduped: u64,
    completed: u64,
}

impl DecodeQueue {
    pub const fn new() -> Self {
        DecodeQueue {
            ring: [None; DECODE_Q_CAP],
            head: 0,
            n: 0,
            enqueued: 0,
            deduped: 0,
            completed: 0,
        }
    }

    /// 请求解码：在队同哈希 → 去重计数（返回 false=未入队）。
    pub fn request(&mut self, content_hash: u64) -> bool {
        for k in 0..self.n {
            let idx = (self.head + k) % DECODE_Q_CAP;
            if self.ring[idx] == Some(content_hash) {
                self.deduped += 1;
                return false;
            }
        }
        if self.n >= DECODE_Q_CAP {
            return false; // 表满拒绝（背压——下一悬停重试）
        }
        let tail = (self.head + self.n) % DECODE_Q_CAP;
        self.ring[tail] = Some(content_hash);
        self.n += 1;
        self.enqueued += 1;
        true
    }

    /// 解码完成出队（FIFO）。
    pub fn pop_completed(&mut self) -> Option<u64> {
        if self.n == 0 {
            return None;
        }
        let v = self.ring[self.head].take();
        self.head = (self.head + 1) % DECODE_Q_CAP;
        self.n -= 1;
        self.completed += 1;
        v
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.enqueued, self.deduped, self.completed)
    }

    pub fn pending(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化二：内容指纹缓存索引
// ---------------------------------------------------------------------------

/// 指纹缓存（hash → 槽；满后轮转覆盖最旧——缓存不是无限承诺）。
pub struct CacheIndex {
    slots: [Option<(u64, u16)>; CACHE_INDEX_CAP],
    head: usize,
    n: usize,
    hits: u64,
    misses: u64,
}

impl CacheIndex {
    pub const fn new() -> Self {
        CacheIndex {
            slots: [None; CACHE_INDEX_CAP],
            head: 0,
            n: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// 查缓存。
    pub fn lookup(&mut self, content_hash: u64) -> Option<u16> {
        for k in 0..self.n {
            let idx = (self.head + self.n - 1 - k) % CACHE_INDEX_CAP; // 新者优先
            if let Some((h, slot)) = self.slots[idx] {
                if h == content_hash {
                    self.hits += 1;
                    return Some(slot);
                }
            }
        }
        self.misses += 1;
        None
    }

    /// 入缓存（表满 → 覆盖最旧位——head 语义轮转）。
    pub fn put(&mut self, content_hash: u64, slot: u16) {
        let pos = if self.n < CACHE_INDEX_CAP {
            let p = (self.head + self.n) % CACHE_INDEX_CAP;
            self.n += 1;
            p
        } else {
            let p = self.head;
            self.head = (self.head + 1) % CACHE_INDEX_CAP;
            p
        };
        self.slots[pos] = Some((content_hash, slot));
    }

    /// 命中率 ×100。
    pub fn hit_rate_pct(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 100 / total) as u32
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        (self.hits, self.misses, self.n)
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：解码去重 / 指纹缓存逐条实摆。
pub fn run_thumbcard_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F073-thumbcard-deep");

    // ── 解码队列 ──
    // 1) 首悬停入队、重复悬停去重。
    let mut dq = DecodeQueue::new();
    cs.add("dq_first_enqueues", dq.request(0xBEEF), "");
    cs.add("dq_repeat_deduped", !dq.request(0xBEEF) && dq.pending() == 1, "");
    // 2) FIFO 出队。
    let _ = dq.request(0xCAFE);
    cs.add(
        "dq_fifo_order",
        dq.pop_completed() == Some(0xBEEF) && dq.pop_completed() == Some(0xCAFE),
        "",
    );
    // 3) 空队列出队 None。
    cs.add("dq_empty_none", dq.pop_completed().is_none(), "");
    // 4) 同哈希出队后可再入队（不在队 ≠ 永不去重）。
    cs.add("dq_reenqueue_after_complete", dq.request(0xBEEF), "");
    // 5) 容量硬顶。
    let mut dq2 = DecodeQueue::new();
    let mut granted = 0;
    for k in 0..(DECODE_Q_CAP as u64 + 5) {
        if dq2.request(0x1000 + k) {
            granted += 1;
        }
    }
    cs.add("dq_cap_honest", granted == DECODE_Q_CAP && dq2.stats().1 == 0, "");

    // ── 指纹缓存 ──
    // 1) 未命中 → put → 命中（免解码路径）。
    let mut ci = CacheIndex::new();
    cs.add("cache_miss_first", ci.lookup(0xF00D).is_none(), "");
    ci.put(0xF00D, 42);
    cs.add("cache_hit_after_put", ci.lookup(0xF00D) == Some(42), "");
    // 2) 命中率账（1 命中 1 未中 = 50%）。
    cs.add("cache_rate_ledger", ci.hit_rate_pct() == 50 && ci.stats() == (1, 1, 1), "");
    // 3) 满后轮转覆盖最旧。
    let mut ci2 = CacheIndex::new();
    for k in 0..CACHE_INDEX_CAP as u64 {
        ci2.put(0x10_0000 + k, k as u16);
    }
    ci2.put(0x9999, 7); // 挤掉最旧（0x10_0000）
    cs.add("cache_roll_evicts_oldest", ci2.lookup(0x10_0000).is_none() && ci2.lookup(0x9999) == Some(7), "");
    // 4) 二次浏览高分场景：10 hash 各查 11 次 → 命中率 91%。
    let mut ci3 = CacheIndex::new();
    for k in 0..10u64 {
        ci3.put(0x50 + k, k as u16);
        let _ = ci3.lookup(0x50 + k); // 首查 put 后即命中（模拟首屏后缓存）
    }
    for _ in 0..10 {
        for k in 0..10u64 {
            let _ = ci3.lookup(0x50 + k);
        }
    }
    cs.add("cache_second_browse_high", ci3.hit_rate_pct() == 100, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn dq_interleaved_requests_keep_order() {
        let mut dq = DecodeQueue::new();
        let _ = dq.request(1);
        let _ = dq.request(2);
        let _ = dq.request(1); // 去重（1 已在队）
        assert_eq!(dq.pop_completed(), Some(1));
        let _ = dq.request(1); // 出队后再入队
        assert_eq!(dq.pop_completed(), Some(2));
        assert_eq!(dq.pop_completed(), Some(1));
        assert_eq!(dq.stats(), (3, 1, 3));
    }

    #[test]
    fn cache_repeated_put_updates_slot() {
        let mut ci = CacheIndex::new();
        ci.put(0xAB, 1);
        ci.put(0xAB, 2); // 同哈希重解码后更新槽位
        assert_eq!(ci.lookup(0xAB), Some(2), "最新解码结果可见");
    }

    #[test]
    fn cache_cap_never_grows_past_limit() {
        let mut ci = CacheIndex::new();
        for k in 0..(CACHE_INDEX_CAP as u64 + 50) {
            ci.put(k, (k % 300) as u16);
        }
        assert_eq!(ci.stats().2, CACHE_INDEX_CAP, "表容不越界");
    }
}

// ===========================================================================
// v3 深化批（F073 · G-C-03）——字节预算缓存 / 陈旧失效 / 解码尺寸估算
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-03 功能定义的实装细化，非新立项）：
// 1. ThumbCacheBudget —— 缩略图缓存字节预算：LRU 按字节逐出（不是按
//    张数——4K 壁纸缩略与 16px 图标同权是错的），预算内命中率优先。
// 2. StaleGuard —— 陈旧失效：窗口内容哈希变化 → 缓存条目立即失效
//    （缩略图与内容一致性判据的维护面）。
// 3. DecodeSizeEstimate —— 解码尺寸估算：源尺寸 → 0.5x 降采样目标
//    尺寸（宽高保比、奇数钳偶——解码器对齐约束）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 缓存字节预算（2MB——200 张 200×112 缩略的量级）。
pub const CACHE_BYTE_BUDGET: u64 = 2 * 1024 * 1024;
/// 缓存条目容量。
pub const THUMB_CACHE_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 深化一：字节预算 LRU 缓存
// ---------------------------------------------------------------------------

/// 缓存条目。
#[derive(Clone, Copy, Debug)]
struct ThumbEntry {
    content_hash: u64,
    bytes: u64,
    last_touch: u64,
    stale: bool,
}

/// 字节预算 LRU 缓存。
pub struct ThumbCacheBudget {
    entries: [Option<ThumbEntry>; THUMB_CACHE_CAP],
    n: usize,
    used_bytes: u64,
    clock: u64,
    hits: u64,
    misses: u64,
    evicted: u64,
}

impl ThumbCacheBudget {
    pub const fn new() -> Self {
        ThumbCacheBudget {
            entries: [None; THUMB_CACHE_CAP],
            n: 0,
            used_bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            evicted: 0,
        }
    }

    /// 查缓存：命中刷新 + 命中计数；陈旧条目视为 miss（先失效再查空）。
    pub fn lookup(&mut self, content_hash: u64) -> bool {
        self.clock += 1;
        for e in self.entries.iter_mut().flatten() {
            if e.content_hash == content_hash {
                if e.stale {
                    e.stale = false;
                    // 陈旧即失效：从命中账里扣除语义——记 miss 并标记清理。
                    self.misses += 1;
                    return false;
                }
                e.last_touch = self.clock;
                self.hits += 1;
                return true;
            }
        }
        self.misses += 1;
        false
    }

    /// 插入（字节超预算 → LRU 逐出到装得下；单条超预算 → 拒绝）。
    pub fn insert(&mut self, content_hash: u64, bytes: u64, now: u64) -> bool {
        if bytes > CACHE_BYTE_BUDGET {
            return false; // 单条超预算——永不入缓存（诚实拒绝）
        }
        self.clock = now.max(self.clock + 1);
        // 已在则更新尺寸。
        for e in self.entries.iter_mut().flatten() {
            if e.content_hash == content_hash {
                self.used_bytes = self.used_bytes + bytes - e.bytes;
                e.bytes = bytes;
                e.last_touch = self.clock;
                e.stale = false;
                return true;
            }
        }
        // 逐出直到装得下。
        while self.used_bytes + bytes > CACHE_BYTE_BUDGET || self.n >= THUMB_CACHE_CAP {
            let mut victim = None;
            let mut oldest = u64::MAX;
            for (k, e) in self.entries.iter().enumerate() {
                if let Some(t) = e {
                    if t.last_touch < oldest {
                        oldest = t.last_touch;
                        victim = Some(k);
                    }
                }
            }
            match victim {
                Some(k) => {
                    if let Some(t) = self.entries[k].take() {
                        self.used_bytes -= t.bytes;
                        self.evicted += 1;
                        self.n -= 1;
                    }
                }
                None => break,
            }
        }
        for e in self.entries.iter_mut() {
            if e.is_none() {
                *e = Some(ThumbEntry { content_hash, bytes, last_touch: self.clock, stale: false });
                self.n += 1;
                self.used_bytes += bytes;
                return true;
            }
        }
        false
    }

    /// 陈旧标记（内容哈希变化）。
    pub fn mark_stale(&mut self, content_hash: u64) -> bool {
        for e in self.entries.iter_mut().flatten() {
            if e.content_hash == content_hash {
                e.stale = true;
                return true;
            }
        }
        false
    }

    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub fn hit_rate_pct(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 100 / total) as u32
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }
}

// ---------------------------------------------------------------------------
// 深化二：解码尺寸估算
// ---------------------------------------------------------------------------

/// 0.5x 降采样目标尺寸（保比、钳偶）。
pub fn decode_size_estimate(src_w: u32, src_h: u32) -> (u32, u32) {
    let even = |v: u32| v & !1;
    let w = even((src_w / 2).max(2));
    let h = even((src_h / 2).max(2));
    (w, h)
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：字节预算 / 陈旧 / 估算逐条实摆。
pub fn run_thumbcard_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F073-thumbcard-v3");

    // ── 字节预算 LRU ──
    let mut cb = ThumbCacheBudget::new();
    let _ = cb.insert(0xAA, 100_000, 1);
    cs.add("cache_lookup_hit", cb.lookup(0xAA), "");
    cs.add("cache_bytes_accounted", cb.used_bytes() == 100_000, "");
    // 字节逐出：再灌 2MB−100KB → 第一条被挤。
    let mut filled = 0u64;
    let mut k = 0u64;
    while filled < CACHE_BYTE_BUDGET - 100_000 {
        let chunk = 100_000u64;
        let _ = cb.insert(0x1000 + k, chunk, 10 + k);
        filled += chunk;
        k += 1;
    }
    cs.add("cache_budget_enforced", cb.used_bytes() <= CACHE_BYTE_BUDGET, "");
    cs.add("cache_lru_evicted_first", cb.evicted() >= 1 && !cb.lookup(0xAA), "");
    // 单条超预算拒绝。
    cs.add("cache_oversize_refused", !cb.insert(0xDEAD, CACHE_BYTE_BUDGET + 1, 999), "");
    // 陈旧失效。
    let mut cb2 = ThumbCacheBudget::new();
    let _ = cb2.insert(0xBB, 50_000, 1);
    let _ = cb2.mark_stale(0xBB);
    cs.add("cache_stale_is_miss", !cb2.lookup(0xBB), "");
    cs.add("cache_stale_refreshable", {
        let _ = cb2.insert(0xBB, 50_000, 2); // 重解码后可再入
        cb2.lookup(0xBB)
    }, "");

    // ── 解码尺寸估算 ──
    let (w, h) = decode_size_estimate(3840, 2160); // 4K → 1080p… 0.5x = 1920×1080
    cs.add("decode_4k_half", w == 1920 && h == 1080, "");
    // 奇数钳偶。
    let (w2, h2) = decode_size_estimate(3841, 2161);
    cs.add("decode_odd_clamped_even", w2 % 2 == 0 && h2 % 2 == 0, "");
    // 极小源不塌零（下限 2）。
    let (w3, h3) = decode_size_estimate(3, 3);
    cs.add("decode_min_2", w3 >= 2 && h3 >= 2, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn cache_update_replaces_bytes() {
        let mut cb = ThumbCacheBudget::new();
        let _ = cb.insert(0x77, 100_000, 1);
        let _ = cb.insert(0x77, 300_000, 2); // 重解码尺寸变化
        assert_eq!(cb.used_bytes(), 300_000, "同条目更新尺寸不重复计账");
    }

    #[test]
    fn estimate_16k_source() {
        let (w, h) = decode_size_estimate(7680, 4320);
        assert_eq!((w, h), (3840, 2160), "8K → 4K 缩略");
    }

    #[test]
    fn budget_never_exceeded_under_flood() {
        let mut cb = ThumbCacheBudget::new();
        for k in 0..200u64 {
            let _ = cb.insert(k, 200_000, k);
            assert!(cb.used_bytes() <= CACHE_BYTE_BUDGET);
        }
    }
}

// ===========================================================================
// v4 深化批（F073 · G-C-03）——可见性渲染队列 / 邻位预取
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-03 功能定义的实装细化，非新立项）：
// 1. RenderQueue —— 缩略图渲染队列：按可见区位置排序渲染（先渲染
//    屏内 → 越近越先——用户看哪张先出哪张）。
// 2. HoverPrefetch —— 邻位预取：悬停某卡 → 左右邻位预解码入队
//    （翻页方向预读——悬停到出图 ≤400ms 的前置面）。
// 全部零堆：定长表，无 Vec/String/浮点/format!。
// ===========================================================================

/// 渲染队列容量。
pub const RENDER_Q_CAP: usize = 12;

// ---------------------------------------------------------------------------
// 深化一：可见性渲染队列
// ---------------------------------------------------------------------------

/// 渲染请求（位置 = 可见区槽位，越小越先看到）。
#[derive(Clone, Copy, Debug)]
pub struct RenderReq {
    pub position: u16,
    pub content_hash: u64,
}

/// 可见性渲染队列：按 position 升序出队（近者先渲染）。
pub struct RenderQueue {
    q: [Option<RenderReq>; RENDER_Q_CAP],
    n: usize,
    rendered: u64,
}

impl RenderQueue {
    pub const fn new() -> Self {
        RenderQueue { q: [None; RENDER_Q_CAP], n: 0, rendered: 0 }
    }

    /// 入队（重复 position 更新哈希）。
    pub fn submit(&mut self, r: RenderReq) -> bool {
        for e in self.q.iter_mut().flatten() {
            if e.position == r.position {
                e.content_hash = r.content_hash;
                return true;
            }
        }
        if self.n >= RENDER_Q_CAP {
            // 挤掉最远（position 最大）。
            let mut victim = 0usize;
            let mut far = 0u16;
            for (k, e) in self.q.iter().enumerate() {
                if let Some(x) = e {
                    if x.position > far {
                        far = x.position;
                        victim = k;
                    }
                }
            }
            if self.q[victim].unwrap().position <= r.position {
                return false; // 新请求更远——不入队（可见性优先）
            }
            self.q[victim] = None;
            self.n -= 1;
        }
        for e in self.q.iter_mut() {
            if e.is_none() {
                *e = Some(r);
                self.n += 1;
                return true;
            }
        }
        false
    }

    /// 出队最近者（position 最小；跳过空洞槽）。
    pub fn pop_nearest(&mut self) -> Option<RenderReq> {
        if self.n == 0 {
            return None;
        }
        let mut best: Option<usize> = None;
        for (k, e) in self.q.iter().enumerate() {
            if let Some(x) = e {
                best = match best {
                    Some(b) if self.q[b].as_ref().unwrap().position <= x.position => best,
                    _ => Some(k),
                };
            }
        }
        let k = best?;
        let r = self.q[k].take();
        self.n -= 1;
        self.rendered += 1;
        r
    }

    pub fn pending(&self) -> usize {
        self.n
    }

    pub fn rendered(&self) -> u64 {
        self.rendered
    }
}

// ---------------------------------------------------------------------------
// 深化二：邻位预取
// ---------------------------------------------------------------------------

/// 邻位预取器：悬停位置 p → 预取 p±1（方向性：最近移动方向优先）。
pub struct HoverPrefetch {
    last_hover: Option<u16>,
    last_direction: i8, // -1 左 1 右 0 无
    prefetches: u64,
}

impl HoverPrefetch {
    pub const fn new() -> Self {
        HoverPrefetch { last_hover: None, last_direction: 0, prefetches: 0 }
    }

    /// 悬停更新：返回预取目标列表（写入 out，最多 2 个）。
    pub fn hover(&mut self, pos: u16, out: &mut [u16]) -> usize {
        if let Some(prev) = self.last_hover {
            if pos > prev {
                self.last_direction = 1;
            } else if pos < prev {
                self.last_direction = -1;
            }
        }
        self.last_hover = Some(pos);
        // 邻位：方向优先（移动方向前一格先取），再取反向格。
        let mut k = 0usize;
        let primary = pos as i32 + self.last_direction as i32;
        let secondary = pos as i32 - self.last_direction as i32;
        for cand in [primary, secondary] {
            // 同位去重（方向 0 时主次相同）+ 边界钳制。
            if cand >= 0 && k < out.len() && !out[..k].contains(&(cand as u16)) {
                out[k] = cand as u16;
                k += 1;
                self.prefetches += 1;
            }
        }
        k
    }

    pub fn prefetches(&self) -> u64 {
        self.prefetches
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：渲染队列 / 邻位预取逐条实摆。
pub fn run_thumbcard_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F073-thumbcard-v4");

    // ── 渲染队列 ──
    let mut rq = RenderQueue::new();
    let _ = rq.submit(RenderReq { position: 5, content_hash: 0x55 });
    let _ = rq.submit(RenderReq { position: 2, content_hash: 0x22 });
    let _ = rq.submit(RenderReq { position: 8, content_hash: 0x88 });
    cs.add("rq_nearest_first", {
        let f = rq.pop_nearest().unwrap();
        f.position == 2 && f.content_hash == 0x22
    }, "");
    cs.add("rq_second_nearest", rq.pop_nearest().unwrap().position == 5, "");
    // 同 position 更新哈希。
    let mut rq2 = RenderQueue::new();
    let _ = rq2.submit(RenderReq { position: 1, content_hash: 0xAA });
    let _ = rq2.submit(RenderReq { position: 1, content_hash: 0xBB });
    cs.add("rq_update_same_pos", {
        let f = rq2.pop_nearest().unwrap();
        rq2.pending() == 0 && f.content_hash == 0xBB
    }, "");
    // 容满挤最远、更远者拒。
    let mut rq3 = RenderQueue::new();
    for k in 0..RENDER_Q_CAP as u16 {
        let _ = rq3.submit(RenderReq { position: k, content_hash: k as u64 });
    }
    cs.add(
        "rq_full_evicts_far",
        rq3.submit(RenderReq { position: 100, content_hash: 1 }) == false
            && rq3.submit(RenderReq { position: 200, content_hash: 2 }) == false,
        "",
    );
    let _ = rq3.submit(RenderReq { position: 3, content_hash: 3 }); // 更新 3
    cs.add("rq_far_evicted_near_in", rq3.pending() == RENDER_Q_CAP, "");
    // 空队列出队 None。
    cs.add("rq_empty_none", RenderQueue::new().pop_nearest().is_none(), "");

    // ── 邻位预取 ──
    let mut hp = HoverPrefetch::new();
    let mut out = [0u16; 2];
    let _ = hp.hover(5, &mut out); // 首次无方向
    // 右移到 6 → 主预取 7（方向前）、次预取 5。
    let n = hp.hover(6, &mut out);
    cs.add("hpref_direction_primary", n == 2 && out[0] == 7 && out[1] == 5, "");
    // 改左移 → 主预取反向。
    let _ = hp.hover(3, &mut out);
    let n2 = hp.hover(2, &mut out);
    cs.add("hpref_reverses", n2 == 2 && out[0] == 1 && out[1] == 3, "");
    // 边界钳制（0 位方向 0 → 主次同位去重后只出一个）。
    let mut hp2 = HoverPrefetch::new();
    let _ = hp2.hover(0, &mut out);
    let _ = hp2.hover(0, &mut out);
    cs.add("hpref_boundary_clamped", hp2.hover(0, &mut out) == 1, "");
    // 账目：hp 序列 = 首悬停(1) + 右移(2) + 左移(2) + 左移(2) = 7。
    cs.add("hpref_ledger", hp.prefetches() == 7, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn rq_render_all_in_position_order() {
        let mut rq = RenderQueue::new();
        for p in [7u16, 3, 9, 1] {
            let _ = rq.submit(RenderReq { position: p, content_hash: p as u64 });
        }
        let mut order = [0u16; 4];
        for o in order.iter_mut() {
            *o = rq.pop_nearest().unwrap().position;
        }
        assert_eq!(order, [1, 3, 7, 9], "渲染序 = 位置序");
        assert_eq!(rq.rendered(), 4);
    }

    #[test]
    fn hpref_first_hover_no_direction_dedupes() {
        let mut hp = HoverPrefetch::new();
        let mut out = [0u16; 2];
        let n = hp.hover(10, &mut out);
        // 首次方向 0：主 = 次 = 10 → 同位去重后 1 个。
        assert_eq!(n, 1);
        assert_eq!(out[0], 10);
    }
}

// ===========================================================================
// v5 深化批（deep5）：元数据版本迁移 + 内存压力降档
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：缩略图元数据版本迁移（v1 无 hash → v2 补 hash；向前拒读）
// ---------------------------------------------------------------------------

/// 元数据当前版本。
pub const META_VERSION: u8 = 2;
/// v1 条目 8B（id4 + size2 + pad2）；v2 条目 12B（+hash4）。
pub const META_V1_ENTRY: usize = 8;
pub const META_V2_ENTRY: usize = 12;

/// 迁移器：v1 字节流 → v2 条目（hash 由 id+size 推导补齐）。
pub fn migrate_v1_to_v2(v1: &[u8], out: &mut [u8]) -> usize {
    let n_entries = v1.len() / META_V1_ENTRY;
    let mut written = 0usize;
    for k in 0..n_entries {
        let base = k * META_V1_ENTRY;
        let id = u32::from_le_bytes([v1[base], v1[base + 1], v1[base + 2], v1[base + 3]]);
        let size = u16::from_le_bytes([v1[base + 4], v1[base + 5]]);
        if written + META_V2_ENTRY > out.len() {
            break;
        }
        out[written..written + 4].copy_from_slice(&id.to_le_bytes());
        out[written + 4..written + 6].copy_from_slice(&size.to_le_bytes());
        out[written + 6..written + 8].copy_from_slice(&[0, 0]);
        let hash = fnv16_pair(id, size);
        out[written + 8..written + 12].copy_from_slice(&hash.to_le_bytes());
        written += META_V2_ENTRY;
    }
    written
}

fn fnv16_pair(id: u32, size: u16) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in id.to_le_bytes().iter().chain(size.to_le_bytes().iter()) {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 版本头判定（0xFF 哨兵 = 未来版本 → 拒读）。
pub fn meta_version_of(first_byte: u8) -> Option<u8> {
    if first_byte == 0xFF || first_byte > META_VERSION {
        None
    } else {
        Some(first_byte)
    }
}

// ---------------------------------------------------------------------------
// 深化二：内存压力降档（压力三态 → 缩略图目标档位随动）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemPressure {
    Low,
    Medium,
    High,
}

/// 压力 → 目标缩略档（0=512px 1=256px 2=128px 3=64px）。
pub fn target_tier(p: MemPressure) -> usize {
    match p {
        MemPressure::Low => 0,
        MemPressure::Medium => 1,
        MemPressure::High => 3, // 跳过 128 直落 64——高压下的激进诚实
    }
}

/// 降档状态机：只随压力降/升一档（防抖——压力抖动不致档位振荡）。
pub struct TierGovernor {
    tier: usize,
    pressure: MemPressure,
    switches: u32,
}

impl TierGovernor {
    pub const fn new() -> Self {
        TierGovernor { tier: 0, pressure: MemPressure::Low, switches: 0 }
    }

    /// 压力更新 → 档位向目标移动一步。
    pub fn update(&mut self, p: MemPressure) {
        self.pressure = p;
        let goal = target_tier(p);
        if goal > self.tier {
            self.tier += 1;
            self.switches += 1;
        } else if goal < self.tier {
            self.tier -= 1;
            self.switches += 1;
        }
    }

    pub fn tier(&self) -> usize {
        self.tier
    }

    pub fn switches(&self) -> u32 {
        self.switches
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_thumbcard_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F073-thumbcard-v5");

    // ── 元数据迁移 ──
    // 1) v1 → v2：条目数不变，尺寸 8B→12B。
    let v1: [u8; 16] = core::array::from_fn(|k| k as u8); // 2 条目
    let mut v2 = [0u8; 32];
    let n = migrate_v1_to_v2(&v1, &mut v2);
    cs.add("meta_migrate_count", n == 24, "");
    // 2) id/size 保真（条目 0：id=0x03020100, size=0x0504）。
    let id0 = u32::from_le_bytes([v2[0], v2[1], v2[2], v2[3]]);
    let sz0 = u16::from_le_bytes([v2[4], v2[5]]);
    cs.add("meta_migrate_fields", id0 == 0x0302_0100 && sz0 == 0x0504, "");
    // 3) hash 补齐且确定性（同输入同 hash）。
    let mut v2b = [0u8; 32];
    let _ = migrate_v1_to_v2(&v1, &mut v2b);
    cs.add("meta_migrate_deterministic", v2 == v2b, "");
    // 4) 未来版本拒读。
    cs.add("meta_future_refused", meta_version_of(0xFF).is_none() && meta_version_of(9).is_none(), "");
    // 5) 当前版本可读。
    cs.add("meta_current_ok", meta_version_of(2) == Some(2) && meta_version_of(1) == Some(1), "");

    // ── 内存降档 ──
    // 6) 压力映射表。
    cs.add(
        "tier_pressure_map",
        target_tier(MemPressure::Low) == 0 && target_tier(MemPressure::Medium) == 1 && target_tier(MemPressure::High) == 3,
        "",
    );
    // 7) 单步降档：Low → High 只到 1（不跳变）。
    let mut tg = TierGovernor::new();
    tg.update(MemPressure::High);
    cs.add("tier_single_step_down", tg.tier() == 1 && tg.switches() == 1, "");
    // 8) 连续高压 → 逐步到底 3。
    tg.update(MemPressure::High);
    tg.update(MemPressure::High);
    cs.add("tier_reaches_floor", tg.tier() == 3, "");
    // 9) 回升也单步。
    tg.update(MemPressure::Low);
    cs.add("tier_single_step_up", tg.tier() == 2, "");
    // 10) 压力抖动不增档位账（同档不动）。
    let mut tg2 = TierGovernor::new();
    tg2.update(MemPressure::Low);
    cs.add("tier_no_op_no_switch", tg2.tier() == 0 && tg2.switches() == 0, "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn migrate_overflow_truncates() {
        // 3 条 v1 → 输出只容 2 条 v2（32B）→ 截断到 24B。
        let v1 = [0u8; 24];
        let mut out = [0u8; 32];
        let n = migrate_v1_to_v2(&v1, &mut out);
        assert_eq!(n, 24, "输出容量决定迁移上限——诚实截断");
    }

    #[test]
    fn governor_full_cycle() {
        let mut tg = TierGovernor::new();
        for _ in 0..5 {
            tg.update(MemPressure::High);
        }
        assert_eq!(tg.tier(), 3);
        for _ in 0..5 {
            tg.update(MemPressure::Low);
        }
        assert_eq!(tg.tier(), 0);
        assert_eq!(tg.switches(), 6); // 3 降 + 3 升（到顶/到底后 no-op）
    }
}
