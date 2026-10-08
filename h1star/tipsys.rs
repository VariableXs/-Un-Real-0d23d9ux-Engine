//! F205 Tooltip 悬停提示系统 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F205「Tooltip 悬停提示系统」。
//!
//! **验收标准（主册第一句）**：出现/消失延迟实测 500ms/200ms±20ms；
//! 屏幕四角锚点翻转正确（4 角用例）；全系统审计无一处自绘 tooltip。
//!
//! **设计要点**：
//! - [`TipScheduler`] 状态机：Idle → Armed（悬停计时）→ Shown（500ms
//!   到点点亮）→ Closing（离场 200ms 收）→ Idle；离场期内重新悬停
//!   取消关闭（200ms 窗是防抖窗口不是关断）；
//! - 锚点跟随：提示只锚定元素矩形，不跟随鼠标——调度器不持有鼠标
//!   状态，位置唯一由 [`place`]（anchor, tip 尺寸, 屏幕）决定；
//! - [`place`] 定位器：下方 8px 优先 → 越底翻上方 → 上下都放不下
//!   翻右侧 → 再越界翻左侧 → 全败钳入屏幕（四角用例全枚举）；
//! - 内容解析 [`parse_tip`]：「复制 Ctrl+C」格式——label 与快捷键
//!   以两个空格分隔，快捷键段只收大写字母/数字/+/分隔符；
//! - 审计注册制 [`TipAudit`]：一切 tip 必须走本系统登记（256 位位图），
//!   审计函数揪出未登记的自绘面并回填清单；
//! - [`TipBook`] 记忆表（容量 128、逐出最旧）：每 tip 的展示计数与
//!   最近展示时刻入账。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::h1star::h1base::{Rect, Rgb8}`（几何/浮层主题色）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Rect, Rgb8};

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 出现延迟——主册 F205：「悬停 500ms 出现」。
pub const SHOW_DELAY_MS: u64 = 500;

/// 消失延迟——主册 F205：「消失延迟实测 200ms±20ms」。
pub const HIDE_DELAY_MS: u64 = 200;

/// 下方优先间距——主册 F205：「位置优先在下方 8px」。
pub const GAP_PX: i32 = 8;

/// 字号档位——主册 F205：「字号第三档」。
pub const FONT_TIER: u8 = 3;

/// 圆角——主册 F205：「圆角 8px」。
pub const RADIUS_PX: i32 = 8;

/// 浮层浅色底（F151 浮层色令牌，实装定值）。
pub const POPUP_BG_LIGHT: Rgb8 = Rgb8::new(0xF5, 0xF5, 0xF5);
/// 浮层深色底。
pub const POPUP_BG_DARK: Rgb8 = Rgb8::new(0x2B, 0x2B, 0x2B);

/// 审计注册位图容量（256 个 tip 槽位）。
pub const REG_CAP: usize = 256;

/// 记忆表容量（128 条，超出逐出最旧）。
pub const BOOK_CAP: usize = 128;

// ---------------------------------------------------------------------------
// 展示调度状态机
// ---------------------------------------------------------------------------

/// 调度相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipPhase {
    Idle,
    /// 悬停计时中（未到 500ms）。
    Armed,
    /// 展示中。
    Shown,
    /// 离场收尾（200ms 窗）。
    Closing,
}

/// Tooltip 展示调度器：500ms 出、200ms 收、离场重入取消关闭。
pub struct TipScheduler {
    phase: TipPhase,
    hover_since: u64,
    show_at: u64,
    hide_at: u64,
    /// 展示总次数（出现时机入账）。
    pub shown_count: u32,
    /// 离场期内重入取消的关闭次数。
    pub cancelled_hides: u32,
    /// 悬停未满 500ms 就离开的次数（从未展示）。
    pub short_leaves: u32,
}

impl TipScheduler {
    pub fn new() -> TipScheduler {
        TipScheduler {
            phase: TipPhase::Idle,
            hover_since: 0,
            show_at: 0,
            hide_at: 0,
            shown_count: 0,
            cancelled_hides: 0,
            short_leaves: 0,
        }
    }

    pub fn phase(&self) -> TipPhase {
        self.phase
    }

    pub fn is_shown(&self) -> bool {
        self.phase == TipPhase::Shown
    }

    /// 实际点亮时刻（验收「出现延迟实测」的打点）。
    pub fn shown_at(&self) -> Option<u64> {
        if self.phase == TipPhase::Shown {
            Some(self.show_at)
        } else {
            None
        }
    }

    /// 悬停开始（Closing 期内 = 防抖窗口重入 → 取消关闭）。
    pub fn on_hover_start(&mut self, now: u64) {
        if self.phase == TipPhase::Closing {
            self.phase = TipPhase::Shown;
            self.cancelled_hides += 1;
            return;
        }
        self.phase = TipPhase::Armed;
        self.hover_since = now;
    }

    /// 悬停结束：Shown → 200ms 收尾；Armed（未及展示）→ 直接回 Idle。
    pub fn on_hover_end(&mut self, now: u64) {
        match self.phase {
            TipPhase::Shown => {
                self.phase = TipPhase::Closing;
                self.hide_at = now + HIDE_DELAY_MS;
            }
            TipPhase::Armed => {
                self.phase = TipPhase::Idle;
                self.short_leaves += 1;
            }
            _ => {}
        }
    }

    /// 每帧步进：Armed 计满 500ms → Shown；Closing 计满 200ms → Idle。
    pub fn tick(&mut self, now: u64) {
        match self.phase {
            TipPhase::Armed if now >= self.hover_since + SHOW_DELAY_MS => {
                self.phase = TipPhase::Shown;
                self.show_at = now;
                self.shown_count += 1;
            }
            TipPhase::Closing if now >= self.hide_at => {
                self.phase = TipPhase::Idle;
            }
            _ => {}
        }
    }

    /// 立即收起（点击别处/开始滚动等打断场景；跳过 200ms 收尾窗）。
    /// 缺陷账本：现象=Armed 期打断后相位停在 Armed、计时到点仍会点亮
    /// （单元测试「Armed 期打断 → 直接回 Idle」红）；根因=dismiss_now
    /// 只处理 Shown/Closing，漏掉 Armed——与 on_hover_end 的「Armed 未及
    /// 展示 → 直接回 Idle」语义不一致，属实现漏分支；修法=Armed 也归
    /// Idle（打断即悬停终止，陈旧计时不得再点亮）。
    pub fn dismiss_now(&mut self) {
        if self.phase != TipPhase::Idle {
            self.phase = TipPhase::Idle;
        }
    }

    /// 距点亮还差多少 ms（Armed 期诊断面；其余相位回 0）。
    pub fn ms_until_show(&self, now: u64) -> u64 {
        match self.phase {
            TipPhase::Armed => SHOW_DELAY_MS.saturating_sub(now.saturating_sub(self.hover_since)),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 单点仲裁（全系统同时只亮一个 tip）
// ---------------------------------------------------------------------------

/// tip 单点仲裁器：任意时刻至多一个 tip 在亮——「全系统唯一」判据的
/// 实现面。后到申请被拒并入账（`denied`）。
pub struct TipArbiter {
    shown: Option<u32>,
    pub denied: u32,
}

impl TipArbiter {
    pub fn new() -> TipArbiter {
        TipArbiter { shown: None, denied: 0 }
    }

    /// 申请展示：空位即占；已有在亮 → 拒绝并计数。
    pub fn claim(&mut self, uid: u32) -> bool {
        if self.shown.is_some() {
            self.denied += 1;
            return false;
        }
        self.shown = Some(uid);
        true
    }

    /// 释放（收起方调用；uid 不符不动——防误释放他人）。
    pub fn release(&mut self, uid: u32) {
        if self.shown == Some(uid) {
            self.shown = None;
        }
    }

    pub fn current(&self) -> Option<u32> {
        self.shown
    }
}

impl Default for TipArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TipScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 锚点定位器
// ---------------------------------------------------------------------------

/// 提示停靠边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipSide {
    Below,
    Above,
    Left,
    Right,
}

fn clamp_x(x: i32, w: i32, screen: &Rect) -> i32 {
    Rect::clamp_i32(x, screen.x, screen.right() - w)
}

fn clamp_y(y: i32, h: i32, screen: &Rect) -> i32 {
    Rect::clamp_i32(y, screen.y, screen.bottom() - h)
}

/// 锚点定位：下方 8px 优先；越底翻上方；上下都放不下按可用侧翻右/左；
/// 全败钳入屏幕。翻转判定只看纵向容量，横向恒钳入（贴边锚点不因
/// 水平越界而误翻边）；tip 尺寸超屏时先钳到屏内。
/// 只依赖锚点矩形——「跟随锚点不跟随鼠标」的几何落点。
pub fn place(anchor: Rect, tip_w: i32, tip_h: i32, screen: Rect) -> (Rect, TipSide) {
    let tip_w = tip_w.clamp(1, screen.w);
    let tip_h = tip_h.clamp(1, screen.h);
    // 下方 8px（纵向容量判定）。
    let below_y = anchor.bottom() + GAP_PX;
    if below_y + tip_h <= screen.bottom() {
        return (Rect::new(clamp_x(anchor.x, tip_w, &screen), below_y, tip_w, tip_h), TipSide::Below);
    }
    // 上方 8px。
    let above_y = anchor.y - GAP_PX - tip_h;
    if above_y >= screen.y {
        return (Rect::new(clamp_x(anchor.x, tip_w, &screen), above_y, tip_w, tip_h), TipSide::Above);
    }
    // 右侧 8px（纵向放不下时的横向翻转）。
    let right_x = anchor.right() + GAP_PX;
    if right_x + tip_w <= screen.right() {
        return (
            Rect::new(right_x, clamp_y(anchor.y, tip_h, &screen), tip_w, tip_h),
            TipSide::Right,
        );
    }
    // 左侧 8px。
    let left_x = anchor.x - GAP_PX - tip_w;
    if left_x >= screen.x {
        return (
            Rect::new(left_x, clamp_y(anchor.y, tip_h, &screen), tip_w, tip_h),
            TipSide::Left,
        );
    }
    (Rect::new(anchor.x, below_y, tip_w, tip_h).clamped_into(&screen), TipSide::Below)
}

// ---------------------------------------------------------------------------
// 内容解析（快捷键内联）
// ---------------------------------------------------------------------------

/// 解析结果：label 长度与快捷键段起点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TipContent {
    /// label 字节数（不含分隔空格）。
    pub label_len: usize,
    /// 快捷键段在原串中的字节起点（无快捷键为 None）。
    pub shortcut_start: Option<usize>,
}

/// 快捷键段合法性：字符域为 ASCII 字母/数字/「+」「-」，且至少含一个
/// 大写字母（键名惯例大写——Ctrl+C、Shift+F1）。
/// 缺陷账本：现象=「复制  Ctrl+C」解析不出快捷键段；根因=原实现只收
/// 大写字母，与主册 F205 判据原文的格式样张「复制 Ctrl+C」（含小写
/// t/r/l）自相矛盾——实现违反判据；修法=字符域放宽到字母并叠加
/// 「至少一个大写」约束，既收 Ctrl+C 类键名、又把小写正文（abc）挡在
/// 快捷键域外（维持「小写正文视为纯文本提示」语义）。
pub fn is_shortcut_token(b: &[u8]) -> bool {
    !b.is_empty()
        && b.iter().all(|&c| {
            c.is_ascii_alphanumeric() || c == b'+' || c == b'-'
        })
        && b.iter().any(|&c| c.is_ascii_uppercase())
}

/// 解析「复制 Ctrl+C」格式：label 与快捷键以两个空格分隔。
/// 快捷键段不合法（如小写正文）视为纯文本提示。
pub fn parse_tip(text: &str) -> TipContent {
    let b = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b' ' && b[i + 1] == b' ' {
            let sc = i + 2;
            if sc < b.len() && is_shortcut_token(&b[sc..]) {
                return TipContent { label_len: i, shortcut_start: Some(sc) };
            }
        }
        i += 1;
    }
    TipContent { label_len: b.len(), shortcut_start: None }
}

// ---------------------------------------------------------------------------
// 审计注册制（自绘检测）
// ---------------------------------------------------------------------------

/// tip 审计注册表：一切 tooltip 必须登记；审计函数揪出自绘面。
pub struct TipAudit {
    bits: [u64; REG_CAP / 64],
    /// 历史累计查出的自绘违规数。
    pub violations_total: u32,
}

impl TipAudit {
    pub fn new() -> TipAudit {
        TipAudit { bits: [0u64; REG_CAP / 64], violations_total: 0 }
    }

    /// 登记（uid < 256；越界 uid 显性忽略——登记面容量即 256）。
    pub fn register(&mut self, uid: u32) {
        if (uid as usize) < REG_CAP {
            self.bits[uid as usize / 64] |= 1u64 << (uid % 64);
        }
    }

    pub fn registered(&self, uid: u32) -> bool {
        (uid as usize) < REG_CAP && (self.bits[uid as usize / 64] >> (uid % 64)) & 1 == 1
    }

    /// 审计一批实际绘制了 tooltip 的面 id：返回未登记数，违规 id 回填
    /// `out`（容量钳制）。登记齐全 → 0 违规（主册验收「无一处自绘」）。
    pub fn audit(&mut self, drawn: &[u32], out: &mut [u32]) -> usize {
        let mut n = 0usize;
        for &id in drawn {
            if !self.registered(id) {
                self.violations_total += 1;
                if n < out.len() {
                    out[n] = id;
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for TipAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 记忆表（展示计数 / 最近时刻，定容逐出）
// ---------------------------------------------------------------------------

/// 单条记忆。
#[derive(Clone, Copy, Debug)]
pub struct TipBookRec {
    pub uid: u32,
    pub shows: u32,
    pub last_ms: u64,
}

/// tip 记忆表：容量 128，满则逐出最旧一条（展示统计的持久化面）。
pub struct TipBook {
    recs: alloc::vec::Vec<TipBookRec>,
    pub evicted: u32,
}

impl TipBook {
    pub fn new() -> TipBook {
        TipBook { recs: alloc::vec::Vec::new(), evicted: 0 }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn get(&self, uid: u32) -> Option<&TipBookRec> {
        self.recs.iter().find(|r| r.uid == uid)
    }

    /// 记一次展示（upsert；满则逐出 last_ms 最旧者）。
    pub fn note_shown(&mut self, uid: u32, now: u64) {
        if let Some(r) = self.recs.iter_mut().find(|r| r.uid == uid) {
            r.shows += 1;
            r.last_ms = now;
            return;
        }
        if self.recs.len() >= BOOK_CAP {
            let mut oldest = 0usize;
            for (i, r) in self.recs.iter().enumerate() {
                if r.last_ms < self.recs[oldest].last_ms {
                    oldest = i;
                }
            }
            self.recs.remove(oldest);
            self.evicted += 1;
        }
        self.recs.push(TipBookRec { uid, shows: 1, last_ms: now });
    }
}

impl Default for TipBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 浮层底色（主题联动：深浅两套令牌）。
pub fn popup_bg(dark: bool) -> Rgb8 {
    if dark {
        POPUP_BG_DARK
    } else {
        POPUP_BG_LIGHT
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F205 自检（判据面：500/200ms 实测 + 四角翻转 + 注册制审计零自绘）。
pub fn run_tipsys_checks() -> CheckSet {
    let mut set = CheckSet::new("F205-tipsys");
    let screen = Rect::new(0, 0, 800, 600);

    // 1. 悬停 500ms 出现：+499ms 未亮、+500ms 恰亮（实测打点=悬停起点+500）。
    let mut sch = TipScheduler::new();
    sch.on_hover_start(1_000);
    sch.tick(1_499);
    let early = !sch.is_shown();
    sch.tick(1_500);
    set.add(
        "show at exactly 500ms hover",
        early && sch.is_shown() && sch.shown_at() == Some(1_500) && sch.shown_count == 1,
        "",
    );

    // 2. 离场 200ms 消失：+199ms 仍在收尾窗、+200ms 回 Idle（±20ms 窗内）。
    let still = sch.is_shown();
    sch.on_hover_end(2_000);
    sch.tick(2_199);
    let closing_at199 = sch.phase() == TipPhase::Closing;
    sch.tick(2_200);
    set.add(
        "hide at exactly 200ms after leave",
        still && closing_at199 && sch.phase() == TipPhase::Idle,
        "",
    );

    // 3. 离场 200ms 窗内重入 → 取消关闭（防抖不是关断）。
    let mut sch2 = TipScheduler::new();
    sch2.on_hover_start(0);
    sch2.tick(500);
    sch2.on_hover_end(600);
    sch2.on_hover_start(650); // 离场未满 200ms
    set.add(
        "re-enter cancels closing",
        sch2.is_shown() && sch2.cancelled_hides == 1,
        "",
    );

    // 4. 未满 500ms 就离开 → 从未出现（不闪 tip）。
    let mut sch3 = TipScheduler::new();
    sch3.on_hover_start(0);
    sch3.on_hover_end(300);
    set.add(
        "leave before 500ms never shows",
        sch3.phase() == TipPhase::Idle && sch3.shown_count == 0 && sch3.short_leaves == 1,
        "",
    );

    // 5. 跟随锚点不跟随鼠标：同锚点两次定位恒同矩形（调度器无鼠标态）。
    let anchor = Rect::new(100, 100, 80, 24);
    let (r1, s1) = place(anchor, 200, 40, screen);
    let (r2, s2) = place(anchor, 200, 40, screen);
    set.add(
        "anchor not mouse: placement deterministic",
        r1 == r2 && s1 == s2 && r1.y == anchor.bottom() + GAP_PX,
        "",
    );

    // 6. 默认下方 8px、水平钳入屏幕。
    let edge_anchor = Rect::new(700, 100, 80, 24);
    let (r3, s3) = place(edge_anchor, 200, 40, screen);
    set.add(
        "below 8px default, x clamped",
        s3 == TipSide::Below && r3.y == edge_anchor.bottom() + 8 && r3.right() <= screen.right(),
        "",
    );

    // 7. 四角用例：左下/右下翻上方、左上/右上保下方（右侧越界钳入）。
    let bl = place(Rect::new(0, 590, 60, 10), 200, 40, screen);
    let br = place(Rect::new(740, 590, 60, 10), 200, 40, screen);
    let tl = place(Rect::new(0, 0, 60, 10), 200, 40, screen);
    let tr = place(Rect::new(740, 0, 60, 10), 200, 40, screen);
    set.add(
        "four corners flip correctly",
        bl.1 == TipSide::Above
            && br.1 == TipSide::Above
            && tl.1 == TipSide::Below
            && tr.1 == TipSide::Below
            && tr.0.right() <= screen.right()
            && br.0.right() <= screen.right(),
        "",
    );

    // 8. 上下都放不下 → 左右翻转（高锚点场景）。
    let tall_left = place(Rect::new(0, 10, 60, 580), 200, 40, screen);
    let tall_right = place(Rect::new(740, 10, 60, 580), 200, 40, screen);
    set.add(
        "vertical overflow flips to side",
        tall_left.1 == TipSide::Right && tall_right.1 == TipSide::Left,
        "",
    );

    // 9. 巨型 tip 全向放不下 → 钳入屏幕（不越界渲染）。
    let (r4, _) = place(Rect::new(0, 0, 60, 10), 2000, 1000, screen);
    set.add(
        "oversized tip clamped into screen",
        r4.x >= screen.x && r4.y >= screen.y && r4.right() <= screen.right() && r4.bottom() <= screen.bottom(),
        "",
    );

    // 10. 内容解析：「复制  Ctrl+C」内联快捷键（双空格分隔）；纯文本与非法快捷键段。
    let c1 = parse_tip("复制  Ctrl+C");
    let c2 = parse_tip("纯文本提示");
    let c3 = parse_tip("粘贴 abc");
    set.add(
        "inline shortcut parsing",
        c1.label_len == "复制".len()
            && c1.shortcut_start == Some("复制 ".len() + 1)
            && c2.shortcut_start.is_none()
            && c3.shortcut_start.is_none(),
        "",
    );

    // 11. 审计注册制：未登记自绘面被揪出入清单；登记面零违规（无一处自绘）。
    let mut audit = TipAudit::new();
    audit.register(7);
    audit.register(9);
    let drawn = [7u32, 8, 9, 10];
    let mut viol = [0u32; 4];
    let n = audit.audit(&drawn, &mut viol);
    let mut viol2 = [0u32; 4];
    let n2 = audit.audit(&[7, 9], &mut viol2);
    set.add(
        "audit registry: catches self-drawn, registered clean",
        n == 2 && viol[0] == 8 && viol[1] == 10 && audit.violations_total == 2 && n2 == 0,
        "",
    );

    // 12. 主题联动：浮层深浅两色 + 字号第三档 + 圆角 8px。
    set.add(
        "theme popup tokens / font tier / radius",
        popup_bg(false) == POPUP_BG_LIGHT
            && popup_bg(true) == POPUP_BG_DARK
            && popup_bg(false) != popup_bg(true)
            && FONT_TIER == 3
            && RADIUS_PX == 8
            && GAP_PX == 8,
        "",
    );

    // 13. 记忆表：upsert 计数、容量 128、满则逐出最旧。
    let mut book = TipBook::new();
    for k in 0..(BOOK_CAP as u32 + 2) {
        book.note_shown(k, k as u64 * 10);
    }
    book.note_shown(50, 99_999); // 已存在 → upsert
    let r50 = book.get(50).unwrap();
    set.add(
        "tip book upsert + evict oldest",
        book.len() == BOOK_CAP && book.evicted == 2 && r50.shows == 2 && book.get(0).is_none(),
        "",
    );

    // 14. fuzz（xorshift32 范式）：随机事件流 3000 轮——不变量：
    //     Shown 必然处于悬停中；曾从 Shown 离场且未满 200ms 时不得回 Idle。
    let mut x: u32 = 0x5E3779B9;
    let mut schf = TipScheduler::new();
    let mut now = 0u64;
    let mut hovering = false;
    let mut left_at = 0u64;
    let mut left_from_shown = false;
    let mut fuzz_ok = true;
    for _ in 0..3000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let dt = (x % 120) as u64 + 1;
        now += dt;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match x % 3 {
            0 => {
                hovering = true;
                schf.on_hover_start(now);
            }
            1 => {
                if hovering {
                    left_at = now;
                    left_from_shown = schf.phase() == TipPhase::Shown;
                    hovering = false;
                }
                schf.on_hover_end(now);
            }
            _ => {
                schf.tick(now);
                if schf.is_shown() && !hovering {
                    fuzz_ok = false;
                }
                if left_from_shown
                    && !hovering
                    && now < left_at + HIDE_DELAY_MS
                    && schf.phase() == TipPhase::Idle
                {
                    // 展示后离场不足 200ms 就 Idle → 消失时机非法。
                    fuzz_ok = false;
                }
            }
        }
    }
    set.add("scheduler fuzz 3000 events invariants", fuzz_ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_full_lifecycle() {
        let mut s = TipScheduler::new();
        assert_eq!(s.phase(), TipPhase::Idle);
        s.on_hover_start(0);
        assert_eq!(s.phase(), TipPhase::Armed);
        s.tick(250);
        assert!(!s.is_shown());
        s.tick(500);
        assert_eq!(s.phase(), TipPhase::Shown);
        s.on_hover_end(1_000);
        assert_eq!(s.phase(), TipPhase::Closing);
        s.tick(1_199);
        assert!(s.phase() == TipPhase::Closing);
        s.tick(1_200);
        assert_eq!(s.phase(), TipPhase::Idle);
        assert_eq!(s.shown_count, 1);
    }

    #[test]
    fn four_corners_exact_rects() {
        let screen = Rect::new(0, 0, 800, 600);
        // 左下：翻上方，x 钳 0。
        let (r, side) = place(Rect::new(0, 590, 60, 10), 200, 40, screen);
        assert_eq!(side, TipSide::Above);
        assert_eq!(r.y, 590 - 8 - 40);
        assert_eq!(r.x, 0);
        // 右上：保下方，x 钳到 800-200。
        let (r2, side2) = place(Rect::new(740, 0, 60, 10), 200, 40, screen);
        assert_eq!(side2, TipSide::Below);
        assert_eq!(r2.x, 600);
        assert_eq!(r2.y, 10 + 8);
    }

    #[test]
    fn shortcut_token_rules() {
        assert!(is_shortcut_token(b"Ctrl+C"));
        assert!(is_shortcut_token(b"Shift+F1"));
        assert!(is_shortcut_token(b"Ctrl+Shift+X"));
        assert!(!is_shortcut_token(b""));
        assert!(!is_shortcut_token(b"abc"));
        assert!(!is_shortcut_token("复制".as_bytes()));
    }

    #[test]
    fn audit_capacity_and_unknown_uid() {
        let mut a = TipAudit::new();
        a.register(REG_CAP as u32 + 5); // 越界 uid 显性忽略
        assert!(!a.registered(REG_CAP as u32 + 5));
        a.register(255);
        assert!(a.registered(255));
        let mut out = [0u32; 2];
        assert_eq!(a.audit(&[255, 256], &mut out), 1, "越界 uid 恒未登记");
    }

    #[test]
    fn book_evicts_oldest_by_last_ms() {
        let mut b = TipBook::new();
        for k in 0..BOOK_CAP {
            b.note_shown(k as u32, (k as u64 + 1) * 100);
        }
        // 最旧 = uid 0（last_ms=100）。再来一条 → 逐出 uid 0。
        b.note_shown(999, 50);
        assert!(b.get(0).is_none());
        assert!(b.get(999).is_some());
        assert_eq!(b.evicted, 1);
    }

    #[test]
    fn arbiter_allows_single_tip() {
        let mut a = TipArbiter::new();
        assert!(a.claim(7));
        assert!(!a.claim(8), "已有在亮 → 拒绝");
        assert_eq!(a.denied, 1);
        a.release(8); // 误释放他人 → 不动
        assert_eq!(a.current(), Some(7));
        a.release(7);
        assert_eq!(a.current(), None);
        assert!(a.claim(9), "释放后可再占");
    }

    #[test]
    fn dismiss_now_and_countdown() {
        let mut s = TipScheduler::new();
        s.on_hover_start(1_000);
        assert_eq!(s.ms_until_show(1_200), 300);
        s.dismiss_now();
        assert_eq!(s.phase(), TipPhase::Idle, "Armed 期打断 → 直接回 Idle");
        s.on_hover_start(2_000);
        s.tick(2_500);
        assert!(s.is_shown());
        assert_eq!(s.ms_until_show(2_500), 0, "展示期无倒计时");
        s.dismiss_now();
        assert_eq!(s.phase(), TipPhase::Idle, "展示期即时收起跳过 200ms 窗");
    }

    #[test]
    fn tipsys_selfcheck_all_green() {
        let set = run_tipsys_checks();
        assert!(set.all_passed(), "F205 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（相位编码非法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：tooltip 状态机快照 ----

/// 相位 ↔ 字节编码（0..=3 对应 Idle/Armed/Shown/Closing）。
fn phase_enc(p: TipPhase) -> u8 {
    match p {
        TipPhase::Idle => 0,
        TipPhase::Armed => 1,
        TipPhase::Shown => 2,
        TipPhase::Closing => 3,
    }
}

fn phase_dec(v: u8) -> Option<TipPhase> {
    match v {
        0 => Some(TipPhase::Idle),
        1 => Some(TipPhase::Armed),
        2 => Some(TipPhase::Shown),
        3 => Some(TipPhase::Closing),
        _ => None,
    }
}

/// 记录长：magic4 + ver1 + phase1 + since8 + shown4 + cancelled4 + sum4。
pub const TIPSTATE_REC_LEN: usize = 4 + 1 + 1 + 8 + 4 + 4 + 4;

/// tooltip 状态机快照：相位 + 悬停基准时刻 + 出现/取消计数——
/// 调度器可整态落盘回放（时间字段语义由调用方持有，本模块不持时钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TipStateRec {
    pub phase: TipPhase,
    /// 悬停/展示基准时刻（ms，语义随相位：Armed=悬停起点）。
    pub since_ms: u64,
    pub shown_count: u32,
    pub cancelled_hides: u32,
}

impl TipStateRec {
    pub fn to_bytes(&self) -> [u8; TIPSTATE_REC_LEN] {
        let mut out = [0u8; TIPSTATE_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5] = phase_enc(self.phase);
        out[6..14].copy_from_slice(&self.since_ms.to_le_bytes());
        out[14..18].copy_from_slice(&self.shown_count.to_le_bytes());
        out[18..22].copy_from_slice(&self.cancelled_hides.to_le_bytes());
        let sum = fnv1a(&out[..22]).to_le_bytes();
        out[22..26].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<TipStateRec, V2CodecErr> {
        if b.len() != TIPSTATE_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[22..26]);
        if fnv1a(&b[..22]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let phase = match phase_dec(b[5]) {
            Some(p) => p,
            None => return Err(V2CodecErr::BadField),
        };
        let mut since = [0u8; 8];
        since.copy_from_slice(&b[6..14]);
        let mut shown = [0u8; 4];
        shown.copy_from_slice(&b[14..18]);
        let mut cancel = [0u8; 4];
        cancel.copy_from_slice(&b[18..22]);
        Ok(TipStateRec {
            phase,
            since_ms: u64::from_le_bytes(since),
            shown_count: u32::from_le_bytes(shown),
            cancelled_hides: u32::from_le_bytes(cancel),
        })
    }
}

// ---- UI 壳接线面：四角翻转后的绘制清单 ----

/// 锚点凸块边长（实装定值；壳层锚点指示）。
pub const STEM_PX: i32 = 8;

/// 绘制图元：几何 + 颜色索引（0 = 浮层底色令牌，1 = 锚点强调令牌）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: Rect,
    pub color_idx: u8,
}

/// 四角翻转后的绘制清单（定长 2 图元）：0 = 面板，1 = 锚点凸块——
/// 凸块贴在翻转边一侧（Below 在上缘、Above 在下缘、Right 左缘、
/// Left 右缘）。几何纯函数，位置由 `place` 的翻转结果决定——
/// 主册 F205「屏幕四角锚点翻转正确」的壳层落点。
pub fn tip_draw_items(tip: Rect, side: TipSide) -> [V2Prim; 2] {
    let stem = match side {
        TipSide::Below => Rect::new(tip.x + 8, tip.y - STEM_PX, STEM_PX, STEM_PX),
        TipSide::Above => Rect::new(tip.x + 8, tip.bottom(), STEM_PX, STEM_PX),
        TipSide::Right => Rect::new(tip.x - STEM_PX, tip.y + 8, STEM_PX, STEM_PX),
        TipSide::Left => Rect::new(tip.right(), tip.y + 8, STEM_PX, STEM_PX),
    };
    [V2Prim { rect: tip, color_idx: 0 }, V2Prim { rect: stem, color_idx: 1 }]
}

/// F205 v2 自检（首条恒为持久化 round-trip）。
pub fn run_tipsys_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F205-tipsys-v2");

    // 1. 持久化 round-trip：Shown 态快照编码→解码逐字段还原。
    let rec = TipStateRec {
        phase: TipPhase::Shown,
        since_ms: 1_500,
        shown_count: 3,
        cancelled_hides: 1,
    };
    let bytes = rec.to_bytes();
    set.add("v2 persist roundtrip tip state", TipStateRec::from_bytes(&bytes) == Ok(rec), "");

    // 2. 损坏拒绝四类 + 字段越界（相位 7 非法）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[6] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[5] = 7; // 相位 7 非法（重算 sum 使只坏字段）
    let s4 = fnv1a(&bad4[..22]);
    bad4[22..26].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt tip states",
        TipStateRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && TipStateRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && TipStateRec::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && TipStateRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && TipStateRec::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 相位编码全枚举无损（4 相位 0..=3 往返恒等）。
    let phases = [TipPhase::Idle, TipPhase::Armed, TipPhase::Shown, TipPhase::Closing];
    set.add(
        "v2 phase codec total",
        phases.iter().all(|&p| phase_dec(phase_enc(p)) == Some(p)),
        "",
    );

    // 4. 四角翻转凸块：下方翻转凸块在面板上缘、上方翻转到下缘
    //    （验主册 F205「屏幕四角锚点翻转正确」的壳层几何）。
    let below = tip_draw_items(Rect::new(100, 200, 120, 36), TipSide::Below);
    let above = tip_draw_items(Rect::new(100, 200, 120, 36), TipSide::Above);
    set.add(
        "v2 tip stem follows flip side",
        below[1].rect == Rect::new(108, 192, 8, 8) && above[1].rect == Rect::new(108, 236, 8, 8),
        "",
    );

    // 5. 左右翻转凸块贴缘（上下都放不下翻侧面的壳层几何）。
    let right = tip_draw_items(Rect::new(100, 200, 120, 36), TipSide::Right);
    let left = tip_draw_items(Rect::new(100, 200, 120, 36), TipSide::Left);
    set.add(
        "v2 tip stem on side flip",
        right[1].rect == Rect::new(92, 208, 8, 8) && left[1].rect == Rect::new(220, 208, 8, 8),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn tip_state_idle_roundtrip() {
        let rec = TipStateRec { phase: TipPhase::Idle, since_ms: 0, shown_count: 0, cancelled_hides: 0 };
        assert_eq!(TipStateRec::from_bytes(&rec.to_bytes()).unwrap(), rec);
    }

    #[test]
    fn decoded_shown_state_is_reusable() {
        // 解码还原的 Shown 态喂回调度语义：壳层可直接按相位续渲染。
        let rec = TipStateRec { phase: TipPhase::Shown, since_ms: 500, shown_count: 1, cancelled_hides: 0 };
        let back = TipStateRec::from_bytes(&rec.to_bytes()).unwrap();
        assert!(back.phase == TipPhase::Shown && back.since_ms == 500);
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_tipsys_v2_checks();
        assert!(set.all_passed(), "F205 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
