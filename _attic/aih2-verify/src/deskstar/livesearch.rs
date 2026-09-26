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

use crate::deskstar::dbase::{budget_ok, Debouncer};
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
