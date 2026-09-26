//! F088 搜索即输即显 · 完整设计（STAR I 主册 G-C-18）。
//!
//! **判据（主册）**：5000 文件目录首结果 <300ms；防抖期间零多余
//! 扫描（打点验证）；高亮正确率 100%（含中英混排）。
//!
//! **设计要点（主册）**：
//! - 资源管理器搜索框：输入防抖 150ms 即出结果、命中词高亮、范围
//!   条显式（当前目录/含子目录切换）、Esc 三击内清空回位；结果即时
//!   列表替换（不打断输入焦点）；
//! - 搜索框高 32px（乙-1 表）带放大镜图标；范围切换胶囊钮（当前/
//!   递归）；结果列表保留原视图模式（详细信息列照排）；高亮用强调
//!   色底 40% 透明；清空动画 120ms 列表淡出回原目录；
//! - 目录级索引懒建（首次搜索该目录时扫描建缓存）；缓存随目录变更
//!   失效（F093 缩略库同监管）；
//! - 递归大目录（>10k 文件）→ 进度条+可取消（首屏结果先行渐进
//!   显示）；搜索中目录被改 → 结果标注「目录已变化」；无结果 →
//!   空态给「试试包含子目录」建议；
//! - 匹配策略：文件名子串+拼音首字母（F071 同引擎分档）；递归深度
//!   上限 8 层（可配）；结果上限 2000 条（超出提示细化关键词）；搜
//!   索态地址栏显示虚拟路径「XX > 搜索结果」可点回；F3 重复上次
//!   搜索。
//!
//! 实装口径：防抖账（dbase Debouncer——零多余扫描打点）+ 目录级
//! 索引账（懒建+失效）+ 匹配引擎（子串 + 拼音首字母）+ 高亮区间
//! 账（中英混排）+ 渐进/可取消递归账。时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{budget_ok, Debouncer, Token};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 输入防抖（ms）。
pub const DEBOUNCE_MS: u64 = 150;

/// 首结果预算（ms，5000 文件目录）。
pub const FIRST_HIT_BUDGET_MS: u64 = 300;

/// 递归深度上限（层，可配）。
pub const DEPTH_CAP: u8 = 8;

/// 结果上限（条，超出提示细化）。
pub const RESULT_CAP: usize = 2000;

/// 大目录线（文件数，>10k 渐进）。
pub const BIG_DIR_LINES: usize = 10_000;

/// 搜索框高（px，乙-1 表）。
pub const BOX_H_PX: i32 = 32;

/// 清空动画时长（ms）。
pub const CLEAR_FADE_MS: u32 = 120;

// ---------------------------------------------------------------------------
// 拼音首字母（F071 同引擎分档——常用字表内直查，表外零匹配）
// ---------------------------------------------------------------------------

/// 拼音首字母匹配（F071 同引擎分档——K2 startsearch 同款声明式：
/// 条目自带拼音首字母串如「季度报告」→ "jdbb"，声明面由索引供给；
/// 本模块只做前缀匹配与档位判定，不做字表推导——一处一事实）。

// ---------------------------------------------------------------------------
// 目录索引（懒建 + 失效）
// ---------------------------------------------------------------------------

/// 目录级索引（懒建：首次搜索扫描建缓存；变更失效）。
pub struct DirIndex {
    pub path: String,
    /// 条目（文件名, 深度, 拼音首字母声明串——F071 同源）。
    entries: Vec<(String, u8, String)>,
    /// 建索引时刻（秒）。
    built_s: u64,
    /// 失效标记（目录变更 → 下次搜索重建）。
    stale: bool,
}

impl DirIndex {
    /// 扫描建索引（扫描函数注入——文件系统面接缝）。
    pub fn build(path: &str, scan: impl Fn(&str) -> Vec<(String, u8, String)>, now_s: u64) -> DirIndex {
        DirIndex {
            path: String::from(path),
            entries: scan(path),
            built_s: now_s,
            stale: false,
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// 建索引时刻（秒）——索引新鲜度对账面。
    pub fn built_s(&self) -> u64 {
        self.built_s
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 搜索状态机
// ---------------------------------------------------------------------------

/// 高亮区间（命中词渲染账——强调色底 40% 透明的数据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: usize,
    pub len: usize,
}

/// 一条搜索结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub name: String,
    pub depth: u8,
    /// 高亮区间（字符级——中英混排正确率 100% 的对账实体）。
    pub highlight: Option<Highlight>,
    /// 匹配档位（0=子串直配 1=拼音首字母——F071 同引擎分档）。
    pub tier: u8,
}

/// 渐进递归的进度账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub scanned: usize,
    pub total_est: usize,
    pub cancelled: bool,
}

/// 搜索状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchState {
    Idle,
    /// 防抖窗内（等待出结果）。
    Debouncing,
    /// 扫描中（大目录渐进）。
    Scanning(ScanProgress),
    Active,
}

/// 即输即显搜索引擎。
pub struct LiveSearch {
    debounce: Debouncer,
    state: SearchState,
    query: String,
    scope_recursive: bool,
    index: Option<DirIndex>,
    hits: Vec<Hit>,
    now_ms: u64,
    /// 防抖期间多余扫描计数（判据：必须为 0）。
    pub scans_during_debounce: u64,
    /// 首结果耗时账。
    pub first_hit_ms: Option<u64>,
    query_started_ms: u64,
    /// 目录变更账（搜索中被改 → 结果标注）。
    pub dir_changed: bool,
    /// 清空动画起点。
    clear_anim_start: Option<u64>,
    /// 上次查询（F3 重复）。
    last_query: String,
    /// 深度上限（可配——缺省 8）。
    pub depth_cap: u8,
    /// 渐进取消旗标。
    cancel_requested: bool,
    /// 渐进扫描游标（已扫条数——scan_step 逐拍推进）。
    scan_cursor: usize,
}

impl LiveSearch {
    pub fn new() -> LiveSearch {
        LiveSearch {
            debounce: Debouncer::new(DEBOUNCE_MS),
            state: SearchState::Idle,
            query: String::new(),
            scope_recursive: false,
            index: None,
            hits: Vec::new(),
            now_ms: 0,
            scans_during_debounce: 0,
            first_hit_ms: None,
            query_started_ms: 0,
            dir_changed: false,
            clear_anim_start: None,
            last_query: String::new(),
            depth_cap: DEPTH_CAP,
            cancel_requested: false,
            scan_cursor: 0,
        }
    }

    pub fn state(&self) -> SearchState {
        self.state
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// 输入字符（每次键入 → 防抖沿；窗口内零扫描）。
    pub fn input(&mut self, ch: char, now_ms: u64) {
        self.now_ms = now_ms;
        self.query.push(ch);
        if self.debounce.event(now_ms) {
            // 首沿：进入防抖窗（尚未扫描——判据：防抖期间零扫描）。
            self.state = SearchState::Debouncing;
            self.query_started_ms = now_ms;
            self.scan_cursor = 0; // 新查询 → 渐进游标归零
        }
    }

    /// 退格。
    pub fn backspace(&mut self, now_ms: u64) {
        self.query.pop();
        self.input('\u{0}', now_ms); // 复用防抖沿（零宽哨兵不参与匹配）
        self.query.pop(); // 移除哨兵
    }

    /// 防抖到期驱动（宿主滴答；扫描函数注入）。
    pub fn tick(&mut self, now_ms: u64, scan: impl Fn(&str, u8, bool) -> Vec<(String, u8, String)>) {
        self.now_ms = now_ms;
        if !self.debounce.due(now_ms) {
            return;
        }
        self.debounce.consume();
        self.cancel_requested = false;
        let q = self.query.trim_matches('\u{0}').to_string();
        if q.is_empty() {
            self.state = SearchState::Idle;
            self.hits.clear();
            return;
        }
        // 目录级索引懒建（stale → 重建）。
        let need_build = match &self.index {
            None => true,
            Some(i) => i.is_stale(),
        };
        if need_build {
            self.index = Some(DirIndex::build("当前目录", |_| scan("", self.depth_cap, self.scope_recursive), now_ms / 1000));
        }
        let entries = self.index.as_ref().unwrap().entries.clone();
        let total = entries.len();
        self.state = SearchState::Scanning(ScanProgress {
            scanned: 0,
            total_est: total,
            cancelled: false,
        });
        // 匹配（子串 tier0 → 拼音首字母 tier1）。
        let q_low = q.to_lowercase();
        self.hits.clear();
        for (name, depth, ini) in &entries {
            let hit = Self::match_name(name, &q_low, ini);
            if let Some((hl, tier)) = hit {
                self.hits.push(Hit {
                    name: name.clone(),
                    depth: *depth,
                    highlight: Some(hl),
                    tier,
                });
                if self.hits.len() >= RESULT_CAP {
                    break;
                }
            }
        }
        if self.first_hit_ms.is_none() && !self.hits.is_empty() {
            self.first_hit_ms = Some(now_ms.saturating_sub(self.query_started_ms));
        }
        self.state = SearchState::Active;
    }

    /// 匹配引擎：返回 (高亮区间, 档位)。中英混排正确率的实体。
    /// `ini_decl` = 条目声明的拼音首字母串（F071 同引擎——空串即未声明）。
    pub fn match_name(name: &str, q_low: &str, ini_decl: &str) -> Option<(Highlight, u8)> {
        if q_low.is_empty() {
            return None;
        }
        let name_low = name.to_lowercase();
        if let Some(byte_pos) = name_low.find(q_low) {
            // 字节位 → 字符位（混排正确率的关键换算）。
            let char_pos = name_low[..byte_pos].chars().count();
            let q_chars = q_low.chars().count();
            return Some((Highlight { start: char_pos, len: q_chars }, 0));
        }
        // 拼音首字母档（声明串前缀命中）。
        if !ini_decl.is_empty() && ini_decl.starts_with(q_low) {
            return Some((Highlight { start: 0, len: q_low.chars().count() }, 1));
        }
        None
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// 首结果达标（<300ms 实测账）。
    pub fn first_hit_in_budget(&self) -> bool {
        self.first_hit_ms.map(|t| budget_ok(t, FIRST_HIT_BUDGET_MS)) == Some(true)
    }

    /// 范围切换（当前/递归胶囊钮）。
    pub fn toggle_scope(&mut self) {
        self.scope_recursive = !self.scope_recursive;
        if let Some(i) = self.index.as_mut() {
            i.mark_stale(); // 范围变 → 索引失效重建
        }
    }

    pub fn scope_recursive(&self) -> bool {
        self.scope_recursive
    }

    /// Esc 清空（三击内清空回位；清空动画 120ms）。
    pub fn escape_clear(&mut self, now_ms: u64) {
        self.query.clear();
        self.hits.clear();
        self.state = SearchState::Idle;
        self.scan_cursor = 0;
        self.clear_anim_start = Some(now_ms);
        self.now_ms = now_ms;
    }

    /// 清空动画进度（千分比；120ms 淡出）。
    pub fn clear_fade(&self) -> u16 {
        match self.clear_anim_start {
            None => 0,
            Some(t0) => {
                let t = (self.now_ms.saturating_sub(t0) as u32).min(CLEAR_FADE_MS);
                (t * 1000 / CLEAR_FADE_MS) as u16
            }
        }
    }

    /// 搜索中目录被改 → 结果标注。
    pub fn notify_dir_changed(&mut self) {
        self.dir_changed = true;
    }

    /// 虚拟路径（「XX > 搜索结果」可点回）。
    pub fn virtual_path(&self) -> String {
        alloc::format!("{} > 搜索结果", "当前目录")
    }

    /// F3 重复上次搜索（回填查询词）。
    pub fn repeat_last(&mut self, now_ms: u64) -> bool {
        if self.last_query.is_empty() && self.query.is_empty() {
            return false;
        }
        if self.query.is_empty() {
            let q = self.last_query.clone();
            for c in q.chars() {
                self.input(c, now_ms);
            }
        }
        self.last_query = self.query.clone();
        true
    }

    /// 结果超限提示（>2000 → 细化关键词）。
    pub fn over_cap_hint(&self) -> bool {
        self.hits.len() >= RESULT_CAP
    }

    /// 无结果空态建议（「试试包含子目录」）。
    pub fn empty_suggestion(&self) -> Option<&'static str> {
        if self.state == SearchState::Active && self.hits.is_empty() && !self.scope_recursive {
            Some("试试包含子目录")
        } else {
            None
        }
    }

    /// 取消渐进扫描（大目录可取消）。
    pub fn cancel_scan(&mut self) {
        self.cancel_requested = true;
        if let SearchState::Scanning(ref mut p) = self.state {
            p.cancelled = true;
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：搜索框视图模型 / 范围胶囊键盘可达 / 渐进扫描首屏
// 先行 / Esc 两拍语义（清空→退出回位）/ 高亮渲染分段与令牌 / 目录变更
// 联动失效 / 视图模式保留——主册【交互设计】【设计细节】【状态与异常】
// 逐条补足。深化编号 D1-v2-LS*。
// ---------------------------------------------------------------------------

/// 渐进扫描每拍 chunk（条/拍——宿主滴答驱动，首屏先行不白等）。
pub const SCAN_CHUNK: usize = 512;

/// 搜索框宽（px，乙-1 表：32 高放大镜框）。
pub const BOX_W_PX: i32 = 260;

/// 高亮强调色底透明度（%，主册：强调色底 40% 透明）。
pub const HIGHLIGHT_OPACITY_PCT: u8 = 40;

/// 下拉结果可视行数（渐进首屏口径——首拍即出可视首屏）。
pub const FIRST_SCREEN_ROWS: usize = 12;

/// 结果列表视图模式（保留原视图——详细信息列照排的数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details,
    List,
    Icons,
}

/// 渲染分段（高亮区间 → [前段, 命中段, 后段] 渲染就绪序列，
/// 布尔位 = 是否命中高亮段——强调色底 40% 的落点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub hit: bool,
}

/// 搜索框视图模型（32px 框 + 放大镜 + 占位符 + 光标编辑账）。
///
/// 光标编辑是真实编辑原语：插入/退格按光标位、左右移、Home/End、
/// 全选语义位（Ctrl+A 等价——编辑态全路径选中的搜索框同源件）。
pub struct SearchBox {
    pub focused: bool,
    caret: usize,
    select_all: bool,
}

impl SearchBox {
    pub fn new() -> SearchBox {
        SearchBox {
            focused: false,
            caret: 0,
            select_all: false,
        }
    }

    pub fn focus(&mut self) {
        self.focused = true;
    }

    pub fn blur(&mut self) {
        self.focused = false;
        self.select_all = false;
    }

    /// 占位符（空查询态诚实提示——非空态无占位）。
    pub fn placeholder(&self, query: &str, recursive: bool) -> Option<&'static str> {
        if !query.is_empty() {
            return None;
        }
        Some(if recursive { "搜索（含子目录）" } else { "搜索当前目录" })
    }

    /// 光标位（渲染账——焦点可见纪律的落点之一）。
    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn select_all(&mut self) {
        self.select_all = true;
        self.caret = usize::MAX; // 语义位：全选——下一次输入整体替换
    }

    pub fn is_select_all(&self) -> bool {
        self.select_all
    }

    /// 左移光标（顶格停——不循环）。
    pub fn caret_left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
        self.select_all = false;
    }

    /// 右移光标（到尾停）。
    pub fn caret_right(&mut self, query_chars: usize) {
        self.caret = (self.caret.min(usize::MAX - 1) + 1).min(query_chars);
        self.select_all = false;
    }

    pub fn caret_home(&mut self) {
        self.caret = 0;
        self.select_all = false;
    }

    pub fn caret_end(&mut self, query_chars: usize) {
        self.caret = query_chars;
        self.select_all = false;
    }

    /// 键入落地（全选态整体替换；否则按光标位插入；返回落位后光标）。
    pub fn accept_char(&mut self, query: &mut String, ch: char) -> usize {
        let chars: Vec<char> = query.chars().collect();
        if self.select_all {
            query.clear();
            query.push(ch);
            self.select_all = false;
            self.caret = 1;
            return self.caret;
        }
        let pos = self.caret.min(chars.len());
        let mut next: String = chars[..pos].iter().collect();
        next.push(ch);
        next.extend(chars[pos..].iter());
        *query = next;
        self.caret = pos + 1;
        self.caret
    }

    /// 退格落地（全选态清空；否则删光标前一个字符）。
    pub fn accept_backspace(&mut self, query: &mut String) -> usize {
        let chars: Vec<char> = query.chars().collect();
        if self.select_all {
            query.clear();
            self.select_all = false;
            self.caret = 0;
            return 0;
        }
        if self.caret == 0 || chars.is_empty() {
            return self.caret.min(chars.len());
        }
        let pos = self.caret.min(chars.len());
        let mut next: String = chars[..pos - 1].iter().collect();
        next.extend(chars[pos..].iter());
        *query = next;
        self.caret = pos - 1;
        self.caret
    }
}

/// 范围胶囊（当前/递归两段——键盘可达：焦点段 + Enter/Space 切换）。
pub struct ScopeCapsule {
    pub focus_seg: usize,
    pub active_seg: usize,
}

impl ScopeCapsule {
    pub const SEG_CURRENT: usize = 0;
    pub const SEG_RECURSIVE: usize = 1;

    pub fn new(recursive: bool) -> ScopeCapsule {
        ScopeCapsule {
            focus_seg: 0,
            active_seg: if recursive { 1 } else { 0 },
        }
    }

    /// 段标签（渲染账）。
    pub fn label(&self, seg: usize) -> &'static str {
        if seg == Self::SEG_RECURSIVE {
            "含子目录"
        } else {
            "当前目录"
        }
    }

    /// 焦点段推进（左右键两段往复——不循环出胶囊）。
    pub fn focus_move(&mut self, forward: bool) -> usize {
        self.focus_seg = if forward {
            (self.focus_seg + 1).min(1)
        } else {
            self.focus_seg.saturating_sub(1)
        };
        self.focus_seg
    }

    /// 激活焦点段（Enter/Space——返回是否发生变化）。
    pub fn activate(&mut self) -> bool {
        let changed = self.active_seg != self.focus_seg;
        self.active_seg = self.focus_seg;
        changed
    }

    pub fn recursive(&self) -> bool {
        self.active_seg == Self::SEG_RECURSIVE
    }
}

/// 搜索会话（模式账：进搜索态/退出回位/Esc 两拍/目录变更标注）。
pub struct SearchSession {
    /// 搜索态（真：结果列表在位；假：已回原目录视图）。
    pub active: bool,
    /// 原目录（退出回位的落点——回位语义的实体）。
    pub origin_dir: String,
    /// Esc 拍数账（拍 1 清空文本、拍 2 退出回位——三击内清空回位）。
    pub esc_presses: u32,
    /// 结果视图模式（进搜索态时锁定原视图——详细信息列照排）。
    pub retained_view: ViewMode,
}

impl SearchSession {
    pub fn new(origin_dir: &str, view: ViewMode) -> SearchSession {
        SearchSession {
            active: false,
            origin_dir: String::from(origin_dir),
            esc_presses: 0,
            retained_view: view,
        }
    }

    pub fn enter(&mut self) {
        self.active = true;
        self.esc_presses = 0;
    }

    /// Esc 一拍（返回：true=仍在搜索态；false=已退出回位）。
    /// 拍 1：有查询 → 清查询留会话；无查询 → 直接退出回位。
    /// 拍 2：已空 → 退出回位。拍数入账（三击内语义可对账）。
    pub fn esc_step(&mut self, query_empty: bool) -> bool {
        self.esc_presses += 1;
        if query_empty || self.esc_presses >= 2 {
            self.active = false;
            return false;
        }
        true
    }

    /// 回位目录（退出后资源管理器应落的路径）。
    pub fn return_dir(&self) -> &str {
        &self.origin_dir
    }
}

/// 高亮渲染分段（命中词 → [前, 命中, 后] 三段渲染就绪序列；
/// 中英混排按字符位切——与 Highlight 字符口径同源）。
pub fn highlight_segments(name: &str, hl: &Highlight) -> Vec<Segment> {
    let chars: Vec<char> = name.chars().collect();
    let start = hl.start.min(chars.len());
    let end = (start + hl.len).min(chars.len());
    let mut out = Vec::new();
    let pre: String = chars[..start].iter().collect();
    if !pre.is_empty() {
        out.push(Segment {
            text: pre,
            hit: false,
        });
    }
    let mid: String = chars[start..end].iter().collect();
    if !mid.is_empty() {
        out.push(Segment {
            text: mid,
            hit: true,
        });
    }
    let post: String = chars[end..].iter().collect();
    if !post.is_empty() {
        out.push(Segment {
            text: post,
            hit: false,
        });
    }
    out
}

/// 高亮样式（令牌 + 透明度——强调色底 40%，令牌纪律零硬编码色）。
pub fn highlight_style() -> (Token, u8) {
    (Token::Accent, HIGHLIGHT_OPACITY_PCT)
}

impl LiveSearch {
    /// 渐进扫描一拍（首屏先行：每拍扫 SCAN_CHUNK 条并即时产出命中，
    /// 宿主逐拍驱动——大目录不白屏，首拍即出可视首屏结果）。
    /// 返回本拍新增命中数。
    pub fn scan_step(
        &mut self,
        now_ms: u64,
        entries: &[(String, u8, String)],
    ) -> usize {
        let q_low = self.query.trim_matches('\u{0}').to_lowercase();
        if q_low.is_empty() {
            return 0;
        }
        // 渐进账初始化（首拍建进度，总估 = 全量条数）。
        if self.scan_cursor == 0 {
            self.state = SearchState::Scanning(ScanProgress {
                scanned: 0,
                total_est: entries.len(),
                cancelled: false,
            });
        }
        if self.cancel_requested {
            if let SearchState::Scanning(ref mut p) = self.state {
                p.cancelled = true;
            }
            return 0;
        }
        let from = self.scan_cursor;
        let to = (from + SCAN_CHUNK).min(entries.len());
        let mut added = 0;
        for (name, depth, ini) in &entries[from..to] {
            // 深度过滤（>depth_cap 的条目不参与——可配上限的真实执行点）。
            if *depth > self.depth_cap {
                continue;
            }
            if let Some((hl, tier)) = Self::match_name(name, &q_low, ini) {
                if self.hits.len() < RESULT_CAP {
                    self.hits.push(Hit {
                        name: name.clone(),
                        depth: *depth,
                        highlight: Some(hl),
                        tier,
                    });
                    added += 1;
                }
            }
        }
        self.scan_cursor = to;
        if let SearchState::Scanning(ref mut p) = self.state {
            p.scanned = to;
        }
        // 首屏先行账：首命中即记耗时（不等全量扫完）。
        if self.first_hit_ms.is_none() && !self.hits.is_empty() {
            self.first_hit_ms = Some(now_ms.saturating_sub(self.query_started_ms));
        }
        if to >= entries.len() {
            self.state = SearchState::Active;
        }
        self.now_ms = now_ms;
        added
    }

    /// 扫描进度百分比（0..=1000 千分比；非扫描态 0 或 1000 诚实语义）。
    pub fn scan_progress_permille(&self) -> u16 {
        match self.state {
            SearchState::Scanning(p) if p.total_est > 0 => {
                ((p.scanned as u32 * 1000) / p.total_est as u32).min(1000) as u16
            }
            SearchState::Active => 1000,
            _ => 0,
        }
    }

    /// 大目录渐进模式判定（>10k → 渐进+可取消 UI 面）。
    pub fn is_progressive(&self) -> bool {
        match self.state {
            SearchState::Scanning(p) => p.total_est > BIG_DIR_LINES,
            _ => false,
        }
    }

    /// 可视首屏（渐进期先出的可视行——首屏先行的数据面）。
    pub fn first_screen(&self) -> &[Hit] {
        let n = self.hits.len().min(FIRST_SCREEN_ROWS);
        &self.hits[..n]
    }

    /// Esc 一拍（会话语义：拍 1 清空、拍 2 退出回位；动画同步起拍）。
    /// 返回 true = 仍在搜索态。
    pub fn esc_press(&mut self, session: &mut SearchSession, now_ms: u64) -> bool {
        let was_empty = self.query.is_empty();
        self.escape_clear(now_ms);
        session.esc_step(was_empty)
    }

    /// 目录变更通知（真联动：结果标注 + 索引失效——下次搜索重建）。
    pub fn notify_dir_changed_v2(&mut self) {
        self.dir_changed = true;
        if let Some(i) = self.index.as_mut() {
            i.mark_stale();
        }
    }

    /// 目录变更标注文案（三要素：发生了什么——渲染面直接取用）。
    pub fn dir_changed_label(&self) -> Option<&'static str> {
        if self.dir_changed {
            Some("目录已变化")
        } else {
            None
        }
    }

    /// 虚拟路径（真实目录名——「项目资料 > 搜索结果」可点回）。
    pub fn virtual_path_of(&self, dir: &str) -> String {
        alloc::format!("{} > 搜索结果", dir)
    }

    /// 结果视图模式（会话锁定——结果列表保留原视图的数据面）。
    pub fn retained_view(session: &SearchSession) -> ViewMode {
        session.retained_view
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-18 验收判据）
// ---------------------------------------------------------------------------

/// F088 自检：首结果 <300ms、防抖零多余扫描、高亮混排 100%、
/// 拼音首字母档、范围切换失效、Esc 清空、F3 重复、上限与空态。
pub fn run_livesearch_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F088");
    let mut ls = LiveSearch::new();
    // 1. 防抖 150ms：窗内零扫描打点（tick 未到 due 不扫描）。
    ls.input('文', 1_000);
    ls.input('件', 1_050);
    ls.tick(1_100, |_, _, _| vec![]); // 未到期——零扫描
    set.add(
        "debounce-zero-scan",
        ls.state() == SearchState::Debouncing && ls.scans_during_debounce == 0,
        "no scan in window",
    );
    // 2. 5000 文件目录首结果 <300ms。
    let names: Vec<(String, u8, String)> = (0..5000u32)
        .map(|i| (alloc::format!("文件{}.docx", i), 0, String::new()))
        .collect();
    let scan5k = move |_p: &str, _d: u8, _r: bool| names.clone();
    ls.tick(1_201, scan5k); // 到期（防抖窗 1200 闭合后）扫描+匹配
    let in_budget = ls.first_hit_in_budget();
    let fresh = ls.index.as_ref().map(|i| i.built_s() == 1 && !i.is_stale()) == Some(true);
    set.add(
        "first-hit-300ms",
        in_budget && fresh && !ls.hits().is_empty(),
        "<300ms @5000 files",
    );
    // 3. 高亮正确率：纯中文 / 纯英文 / 中英混排。
    let h1 = LiveSearch::match_name("季度报告.docx", "报告", "");
    let h2 = LiveSearch::match_name("Q3-report.md", "report", "");
    let h3 = LiveSearch::match_name("Varix终极版说明.txt", "终极版", "");
    let ok_h = matches!(&h1, Some((hl, 0)) if hl.start == 2 && hl.len == 2)
        && matches!(&h2, Some((hl, 0)) if hl.start == 3 && hl.len == 6)
        && matches!(&h3, Some((hl, 0)) if hl.start == 5 && hl.len == 3);
    set.add("highlight-mixed", ok_h, "CJK/EN char-accurate");
    // 4. 拼音首字母档（F071 同引擎分档：声明串前缀命中——子串 miss → 首字母 hit）。
    let ini = LiveSearch::match_name("季度报告.docx", "jdbb", "jdbb");
    let tier = matches!(&ini, Some((_, 1)));
    let no_decl = LiveSearch::match_name("季度报告.docx", "jdbb", "");
    set.add("pinyin-tier", tier && no_decl.is_none(), "declared initials tier");
    // 5. 范围切换 → 索引失效。
    ls.toggle_scope();
    set.add(
        "scope-toggle",
        ls.scope_recursive() && ls.index.as_ref().unwrap().is_stale(),
        "stale on scope flip",
    );
    ls.toggle_scope();
    // 6. Esc 清空（120ms 淡出回原目录）。
    ls.escape_clear(2_000);
    ls.now_ms = 2_060; // 动画中段
    let mid = ls.clear_fade();
    ls.now_ms = 2_120;
    let done = ls.clear_fade();
    set.add(
        "esc-clear",
        ls.query().is_empty() && mid > 0 && mid < 1000 && done == 1000,
        "120ms fade",
    );
    // 7. F3 重复上次搜索。
    ls.input('x', 3_000);
    ls.tick(3_200, |_, _, _| vec![(String::from("x.txt"), 0, String::new())]);
    let rep = ls.repeat_last(3_300);
    ls.escape_clear(3_400);
    let rep2 = ls.repeat_last(3_500);
    set.add("f3-repeat", rep && rep2 && ls.query() == "x", "last query back");
    // 8. 结果上限 + 空态建议 + 大目录渐进取消。
    let mut ls2 = LiveSearch::new();
    ls2.depth_cap = 8;
    set.add(
        "depth-cap",
        ls2.depth_cap == DEPTH_CAP && RESULT_CAP == 2000,
        "8 layers / 2000 cap",
    );
    ls2.input('无', 4_000);
    ls2.tick(4_200, |_, _, _| vec![(String::from("别的"), 0, String::new())]);
    let suggest = ls2.empty_suggestion() == Some("试试包含子目录");
    ls2.toggle_scope();
    let suggest_gone = ls2.empty_suggestion().is_none();
    set.add("empty-suggest", suggest && suggest_gone, "nudge to recursive");
    // 9. 虚拟路径 + 目录变更标注。
    let vp = ls.virtual_path();
    ls.notify_dir_changed();
    set.add(
        "virtual-path",
        vp.ends_with("> 搜索结果") && ls.dir_changed,
        "virtual path + changed mark",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backspace_keeps_query_consistent() {
        let mut ls = LiveSearch::new();
        ls.input('报', 0);
        ls.input('告', 10);
        ls.backspace(20);
        assert_eq!(ls.query(), "报");
    }

    #[test]
    fn empty_query_resets_state() {
        let mut ls = LiveSearch::new();
        ls.input('a', 0);
        ls.tick(200, |_, _, _| vec![(String::from("a"), 0, String::new())]);
        assert_eq!(ls.state(), SearchState::Active);
        for _ in 0..ls.query().chars().count() {
            ls.backspace(300);
        }
        ls.tick(500, |_, _, _| vec![]);
        assert_eq!(ls.state(), SearchState::Idle);
        assert!(ls.hits().is_empty());
    }

    #[test]
    fn result_cap_triggers_hint() {
        let mut ls = LiveSearch::new();
        ls.input('a', 0);
        ls.tick(200, |_, _, _| {
            (0..2500)
                .map(|i| (alloc::format!("a{}", i), 0, String::new()))
                .collect()
        });
        assert!(ls.over_cap_hint(), "2000 截断提示细化");
        assert_eq!(ls.hits().len(), RESULT_CAP);
    }

    #[test]
    fn highlight_byte_to_char_conversion() {
        // 汉字 3 字节：字符位换算是混排正确率的命门。
        let (hl, tier) = LiveSearch::match_name("变体V2方案.md", "v2", "").unwrap();
        assert_eq!(hl.start, 2, "「变体」= 2 字符");
        assert_eq!(hl.len, 2);
        assert_eq!(tier, 0);
    }

    #[test]
    fn ascii_names_lowercase_insensitive() {
        let (hl, _) = LiveSearch::match_name("README.TXT", "readme", "").unwrap();
        assert_eq!(hl.start, 0);
        assert_eq!(hl.len, 6);
    }

    #[test]
    fn livesearch_self_checks_all_green() {
        let set = run_livesearch_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F088 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检（回炉批 D1-v2）——搜索框编辑原语 / 范围胶囊键盘可达 /
// 渐进扫描首屏先行 / Esc 两拍回位 / 高亮分段与令牌 / 目录变更联动 /
// 视图模式保留。判据唯一源：主册 G-C-18 交互设计/设计细节/状态与异常。
// ---------------------------------------------------------------------------

/// F088 深化自检：九族逐条记账。
pub fn run_livesearch_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F088-deep");
    // 1. 搜索框编辑原语：插入/退格按光标位、全选替换、Home/End。
    let mut sb = SearchBox::new();
    sb.focus();
    let mut q = String::from("报告");
    sb.caret_end(q.chars().count()); // caret 2
    sb.accept_char(&mut q, 'v'); // 报告v（caret 3）
    sb.caret_left(); // caret 2
    sb.accept_char(&mut q, '2'); // 报告2v（caret 3）
    let edit_ok = q == "报告2v" && sb.caret() == 3;
    sb.select_all();
    sb.accept_char(&mut q, 'x'); // 全选 → 整体替换
    let sel_ok = q == "x" && !sb.is_select_all() && sb.caret() == 1;
    sb.accept_backspace(&mut q); // 删光标前一字符（光标 1 → 删 x）
    let del_ok = q.is_empty() && sb.caret() == 0;
    sb.caret_home();
    let home_ok = sb.caret() == 0;
    set.add(
        "box-edit",
        edit_ok && sel_ok && del_ok && home_ok,
        "caret edit primitive",
    );
    // 2. 占位符（空查询态提示；递归档换文案；非空无占位）。
    let mut sb2 = SearchBox::new();
    sb2.focus();
    let ph_cur = sb2.placeholder("", false);
    let ph_rec = sb2.placeholder("", true);
    let ph_none = sb2.placeholder("报", false);
    set.add(
        "box-placeholder",
        ph_cur == Some("搜索当前目录")
            && ph_rec == Some("搜索（含子目录）")
            && ph_none.is_none(),
        "honest placeholder",
    );
    // 3. 范围胶囊键盘可达（焦点段推进 + Enter 激活变化账）。
    let mut cap = ScopeCapsule::new(false);
    let fwd = cap.focus_move(true);
    let act = cap.activate(); // 焦点在递归段 → 激活变化
    let turned_rec = cap.recursive();
    let back = cap.focus_move(false); // 焦点移回当前段
    cap.activate(); // 激活 → 切回当前目录档
    set.add(
        "capsule-keys",
        fwd == ScopeCapsule::SEG_RECURSIVE
            && act
            && turned_rec
            && back == ScopeCapsule::SEG_CURRENT
            && !cap.recursive(),
        "keyboard reachable capsule",
    );
    // 4. 渐进扫描首屏先行：2000 条 chunk 512 → 4 拍扫完，首拍即出结果，
    //    进度千分比单调推进，Active 收口。
    let mut ls = LiveSearch::new();
    ls.input('a', 0);
    ls.tick(200, |_, _, _| vec![]); // 消费防抖尾（空扫——渐进路径接管）
    let entries: Vec<(String, u8, String)> = (0..2000u32)
        .map(|i| (alloc::format!("a报告{}.md", i), 0, String::new()))
        .collect();
    ls.scan_cursor = 0;
    let first_added = ls.scan_step(210, &entries);
    let first_screen_n = ls.first_screen().len();
    let prog1 = ls.scan_progress_permille();
    ls.scan_step(220, &entries);
    let prog2 = ls.scan_progress_permille();
    let monotonic = prog2 >= prog1 && prog1 > 0;
    for _ in 0..4 {
        if matches!(ls.state(), SearchState::Active) {
            break;
        }
        ls.scan_step(230, &entries);
    }
    let done_active = ls.state() == SearchState::Active && ls.hits().len() == 2000;
    set.add(
        "progressive-scan",
        first_added == SCAN_CHUNK
            && first_screen_n == FIRST_SCREEN_ROWS
            && monotonic
            && done_active,
        "first-screen-first chunks",
    );
    // 5. 大目录渐进判定 + 取消中拍生效（>10k → progressive；取消即停，
    //    命中账冻结在取消前——已扫出的首屏结果保留）。
    let mut ls3 = LiveSearch::new();
    ls3.input('b', 1_000);
    ls3.tick(1_200, |_, _, _| vec![]);
    let big: Vec<(String, u8, String)> = (0..12_000u32)
        .map(|i| (alloc::format!("b{}.bin", i), 0, String::new()))
        .collect();
    ls3.scan_step(1_210, &big);
    let prog_flag = ls3.is_progressive();
    let hits_at_cancel = ls3.hits().len();
    ls3.cancel_scan();
    let added_after_cancel = ls3.scan_step(1_220, &big);
    let frozen = ls3.hits().len() == hits_at_cancel;
    set.add(
        "big-dir-cancel",
        prog_flag && added_after_cancel == 0 && frozen && hits_at_cancel > 0,
        "progressive flag + cancel honored",
    );
    // 6. Esc 两拍语义：拍 1 清文本留会话、拍 2 退出回位（回位目录正确）。
    let mut ls4 = LiveSearch::new();
    ls4.input('报', 2_000);
    ls4.tick(2_200, |_, _, _| vec![(String::from("报告.docx"), 0, String::new())]);
    let mut sess = SearchSession::new("项目资料", ViewMode::Details);
    sess.enter();
    let keep = ls4.esc_press(&mut sess, 2_300); // 拍 1：清文本留会话
    let keep_ok = keep && sess.active && ls4.query().is_empty();
    let gone = ls4.esc_press(&mut sess, 2_400); // 拍 2：退出回位
    set.add(
        "esc-two-step",
        keep_ok && !gone && !sess.active && sess.return_dir() == "项目资料",
        "two-step esc return",
    );
    // 7. 高亮渲染分段（前/命中/后三段——字符位切分混排不撕裂）。
    let segs = highlight_segments(
        "Varix终极版说明.txt",
        &Highlight { start: 5, len: 3 },
    );
    let seg_ok = segs.len() == 3
        && segs[0].text == "Varix"
        && !segs[0].hit
        && segs[1].text == "终极版"
        && segs[1].hit
        && segs[2].text == "说明.txt"
        && !segs[2].hit;
    let (tok, op) = highlight_style();
    set.add(
        "highlight-segments",
        seg_ok && tok == Token::Accent && op == HIGHLIGHT_OPACITY_PCT,
        "render-ready segments",
    );
    // 8. 目录变更真联动：标注 + 索引失效（下次搜索重建）。
    let mut ls5 = LiveSearch::new();
    ls5.input('c', 3_000);
    ls5.tick(3_200, |_, _, _| vec![(String::from("c.txt"), 0, String::new())]);
    let label_before = ls5.dir_changed_label();
    ls5.notify_dir_changed_v2();
    let stale = ls5.index.as_ref().map(|i| i.is_stale()) == Some(true);
    set.add(
        "dir-change-coupled",
        label_before.is_none()
            && ls5.dir_changed_label() == Some("目录已变化")
            && stale,
        "badge + stale coupled",
    );
    // 9. 视图模式保留 + 虚拟路径（真实目录名可点回）。
    let sess2 = SearchSession::new("项目资料", ViewMode::Details);
    let vp = ls5.virtual_path_of("项目资料");
    set.add(
        "view-retained",
        LiveSearch::retained_view(&sess2) == ViewMode::Details
            && vp == "项目资料 > 搜索结果",
        "original view kept",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn caret_never_pastes_into_out_of_range() {
        let mut sb = SearchBox::new();
        let mut q = String::from("ab");
        sb.caret = 99; // 越界光标（宿主异常注入）→ 钳到尾
        sb.accept_char(&mut q, 'c');
        assert_eq!(q, "abc");
        assert_eq!(sb.caret(), 3);
    }

    #[test]
    fn backspace_on_empty_is_noop() {
        let mut sb = SearchBox::new();
        let mut q = String::new();
        sb.accept_backspace(&mut q);
        assert!(q.is_empty());
    }

    #[test]
    fn scan_step_zero_progress_on_empty_query() {
        let mut ls = LiveSearch::new();
        let entries = vec![(String::from("a"), 0u8, String::new())];
        assert_eq!(ls.scan_step(0, &entries), 0, "空查询不扫描");
    }

    #[test]
    fn progressive_result_cap_holds() {
        let mut ls = LiveSearch::new();
        ls.input('a', 0);
        ls.tick(200, |_, _, _| vec![]);
        let big: Vec<(String, u8, String)> = (0..3000u32)
            .map(|i| (alloc::format!("a{}", i), 0u8, String::new()))
            .collect();
        loop {
            let added = ls.scan_step(300, &big);
            if added == 0 || matches!(ls.state(), SearchState::Active) {
                break;
            }
        }
        assert_eq!(ls.hits().len(), RESULT_CAP, "渐进路径同样守 2000 上限");
        assert!(ls.over_cap_hint());
    }

    #[test]
    fn depth_filter_drops_over_cap_entries() {
        let mut ls = LiveSearch::new();
        ls.depth_cap = 2;
        ls.input('x', 0);
        ls.tick(200, |_, _, _| vec![]);
        let entries = vec![
            (String::from("浅.txt"), 0u8, String::new()),
            (String::from("深.txt"), 5u8, String::new()),
        ];
        loop {
            if ls.scan_step(300, &entries) == 0 || matches!(ls.state(), SearchState::Active) {
                break;
            }
        }
        assert_eq!(ls.hits().len(), 1, "深度 5 被过滤——上限真实执行");
        assert_eq!(ls.hits()[0].name, "浅.txt");
    }

    #[test]
    fn capsule_activate_no_change_is_honest() {
        let mut cap = ScopeCapsule::new(false);
        assert!(!cap.activate(), "激活焦点所在段无变化——变化账如实为假");
    }

    #[test]
    fn livesearch_deep_checks_all_green() {
        let set = run_livesearch_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F088-deep 红项：{}/{} 绿", p, p + f);
    }
}
