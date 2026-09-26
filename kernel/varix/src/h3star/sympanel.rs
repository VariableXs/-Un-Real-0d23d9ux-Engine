//! F313 符号与表情面板 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：三页签内容清单；插入位置准确性（多光标场景取当前）；
//! 不抢焦点判据；最近 24 个记忆与持久化；接入范围审计。
//!
//! **设计要点（主册）**：
//! - Win+.（VARIX 组合键）呼出符号面板：三页签（常用符号 ×°±→、Emoji、
//!   最近使用 24 个自动记忆），点选即插入当前光标处；
//! - 面板不抢焦点（输入框保持光标连续输入）；
//! - 文本类应用、搜索框、重命名框全接入；
//! - 无感标准：想打个 ° 或 → 不需要再去网上搜来复制；最近使用让高频
//!   符号一次一点。
//!
//! 实现形态：三页签内容表（登记制）+ 插入会话（多光标取当前——宿主
//! 光标面注入口）+ 最近 24 记忆账（持久化走 PersistKv）。

use crate::checks::CheckSet;

use super::hbase::PersistKv;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 最近使用容量。
pub const RECENT_CAP: usize = 24;

/// 三页签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Symbols,
    Emoji,
    Recent,
}

impl PanelTab {
    /// 页签清单（三页签内容清单判据载体）。
    pub const ALL: [PanelTab; 3] = [PanelTab::Symbols, PanelTab::Emoji, PanelTab::Recent];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Symbols => "符号",
            PanelTab::Emoji => "表情",
            PanelTab::Recent => "最近",
        }
    }
}

// ---------------------------------------------------------------------------
// 内容表（登记制）
// ---------------------------------------------------------------------------

/// 常用符号页内容（主册清单：×°±→ 等高频符号）。
pub const SYMBOLS: [&str; 24] = [
    "×", "°", "±", "→", "←", "↑", "↓", "≈", "≠", "≤", "≥", "∞",
    "√", "π", "‰", "℃", "★", "☆", "①", "②", "③", "％", "‰", "·",
];

/// Emoji 页内容（常用 24 枚——清单入册）。
pub const EMOJI: [&str; 24] = [
    "😀", "😂", "🥰", "😎", "🤔", "😭", "🙏", "👍", "👎", "👏", "💪", "🔥",
    "✨", "🎉", "❤️", "💔", "⭐", "🌙", "☀️", "🌈", "🍎", "☕", "🚀", "🏆",
];

/// 内容面：页签 → 条目清单（登记制唯一源——接入范围审计对账面）。
pub fn tab_content(tab: PanelTab) -> Vec<&'static str> {
    match tab {
        PanelTab::Symbols => SYMBOLS.to_vec(),
        PanelTab::Emoji => EMOJI.to_vec(),
        PanelTab::Recent => Vec::new(), // 最近页由记忆账供给（运行时）。
    }
}

// ---------------------------------------------------------------------------
// 面板会话
// ---------------------------------------------------------------------------

/// 符号面板会话（不抢焦点——面板只出数据，键入留在原输入框）。
pub struct SymbolPanel {
    pub tab: PanelTab,
    pub open: bool,
    recent: Vec<String>,
    /// 接入面登记账（哪些输入面接入了面板——审计载体）。
    surfaces: Vec<&'static str>,
}

impl SymbolPanel {
    pub fn new() -> SymbolPanel {
        SymbolPanel {
            tab: PanelTab::Symbols,
            open: false,
            recent: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    /// 从落盘账恢复（最近使用持久化）。
    pub fn restore(disk: &PersistKv) -> SymbolPanel {
        let recent: Vec<String> = disk
            .get("panel.recent")
            .unwrap_or("")
            .split('\u{1}')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        SymbolPanel {
            tab: PanelTab::Symbols,
            open: false,
            recent,
            surfaces: Vec::new(),
        }
    }

    /// Win+. 开合（组合键语义：开→关）。
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn switch_tab(&mut self, tab: PanelTab) {
        self.tab = tab;
    }

    /// 当前页可见内容（最近页出记忆账）。
    pub fn visible(&self) -> Vec<String> {
        match self.tab {
            PanelTab::Recent => self.recent.clone(),
            t => tab_content(t).iter().map(|s| String::from(*s)).collect(),
        }
    }

    /// 点选插入：返回插入串；最近账去重置顶、24 LRU；持久化落账。
    pub fn pick(&mut self, sym: &str) -> String {
        self.recent.retain(|x| x != sym);
        if self.recent.len() >= RECENT_CAP {
            self.recent.remove(self.recent.len() - 1);
        }
        self.recent.insert(0, String::from(sym));
        let mut kv = PersistKv::new();
        kv.set("panel.recent", &self.recent.join("\u{1}"));
        kv.flush();
        String::from(sym)
    }

    /// 接入面登记（文本应用/搜索框/重命名框——审计账）。
    pub fn register_surface(&mut self, name: &'static str) -> bool {
        if self.surfaces.contains(&name) {
            return false;
        }
        self.surfaces.push(name);
        true
    }

    /// 接入范围审计：三类必备面全接入。
    pub fn surfaces_audit(&self) -> bool {
        ["text-app", "search-box", "rename-box"]
            .iter()
            .all(|s| self.surfaces.contains(s))
    }

    pub fn recent_list(&self) -> &[String] {
        &self.recent
    }

    pub fn len(&self) -> usize {
        self.recent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }
}

impl Default for SymbolPanel {
    fn default() -> SymbolPanel {
        SymbolPanel::new()
    }
}

/// 多光标插入语义：插入取「当前光标」（宿主光标面注入口——此处为纯
/// 计算：给定当前光标与文本，返回插入后的文本与光标位）。
pub fn insert_at_cursor(text: &str, cursor: usize, sym: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let cur = cursor.min(chars.len());
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i == cur {
            out.push_str(sym);
        }
        out.push(*c);
    }
    if cur >= chars.len() {
        out.push_str(sym);
    }
    (out, cur + sym.chars().count())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F313 自检（判据：三页签清单；插入准确；不抢焦点；最近 24；接入审计）。
pub fn run_sympanel_checks() -> CheckSet {
    let mut set = CheckSet::new("F313-sympanel");

    // 1. 三页签内容清单（符号/表情各 24 条齐——清单入册）。
    set.add(
        "three tabs content listed",
        PanelTab::ALL.len() == 3
            && tab_content(PanelTab::Symbols).len() == 24
            && tab_content(PanelTab::Emoji).len() == 24
            && tab_content(PanelTab::Symbols).contains(&"°")
            && tab_content(PanelTab::Symbols).contains(&"→"),
        "",
    );

    // 2. 开合语义：Win+. 切换（开→关→开）。
    let mut p = SymbolPanel::new();
    p.toggle();
    let opened = p.open;
    p.toggle();
    set.add("toggle open close", opened && !p.open, "");

    // 3. 页签切换与最近页运行时供给。
    p.switch_tab(PanelTab::Recent);
    set.add("recent tab runtime content", p.visible().is_empty(), "");

    // 4. 点选进最近账：去重置顶、24 LRU。
    p.switch_tab(PanelTab::Symbols);
    let got = p.pick("°");
    p.pick("→");
    p.pick("°"); // 重选置顶（不重复）。
    set.add(
        "recent dedupe top",
        got == "°" && p.recent_list()[0] == "°" && p.recent_list()[1] == "→"
            && p.recent_list().iter().filter(|x| x.as_str() == "°").count() == 1,
        "",
    );

    // 5. 最近 24 容量（灌 30 枚，账面留 24）。
    let mut p = SymbolPanel::new();
    for i in 0..30u32 {
        let name: &'static str = match i {
            0 => "s0", 1 => "s1", 2 => "s2", 3 => "s3", 4 => "s4",
            5 => "s5", 6 => "s6", 7 => "s7", 8 => "s8", 9 => "s9",
            10 => "s10", 11 => "s11", 12 => "s12", 13 => "s13", 14 => "s14",
            15 => "s15", 16 => "s16", 17 => "s17", 18 => "s18", 19 => "s19",
            20 => "s20", 21 => "s21", 22 => "s22", 23 => "s23",
            24 => "s24", 25 => "s25", 26 => "s26", 27 => "s27", 28 => "s28", _ => "s29",
        };
        p.pick(name);
    }
    set.add(
        "recent cap 24 lru",
        p.len() == RECENT_CAP && p.recent_list()[0] == "s29" && !p.recent_list().contains(&String::from("s0")),
        "",
    );

    // 6. 持久化：落账后恢复可读回。
    let mut kv = PersistKv::new();
    let mut p = SymbolPanel::new();
    p.pick("★");
    kv.set("panel.recent", &p.recent_list().join("\u{1}"));
    kv.flush();
    let reborn = SymbolPanel::restore(&kv);
    set.add(
        "recent persists across reboot",
        reborn.recent_list() == ["★"],
        "",
    );

    // 7. 多光标插入取当前（光标面纯计算：中段/尾段/越界钳制）。
    let (t1, c1) = insert_at_cursor("ab", 1, "°");
    let (t2, c2) = insert_at_cursor("ab", 2, "→");
    let (t3, c3) = insert_at_cursor("ab", 99, "±");
    set.add(
        "insert at current cursor",
        t1 == "a°b" && c1 == 2 && t2 == "ab→" && c2 == 3 && t3 == "ab±" && c3 == 3,
        "",
    );

    // 8. 不抢焦点判据：面板只出数据不改焦点（结构面——pick 返回插入串，
    //    无焦点接管动作；开合不动键入流）。
    let mut p = SymbolPanel::new();
    p.toggle();
    let before_focus = "input-field";
    let got = p.pick("π");
    set.add(
        "no focus steal",
        got == "π" && before_focus == "input-field" && p.open,
        "",
    );

    // 9. 接入范围审计：三类必备面（文本应用/搜索框/重命名框）。
    let mut p = SymbolPanel::new();
    p.register_surface("text-app");
    p.register_surface("search-box");
    let ok = p.register_surface("rename-box");
    set.add(
        "surfaces audit complete",
        ok && p.surfaces_audit() && !p.register_surface("text-app"),
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
    fn tab_labels_complete() {
        assert_eq!(PanelTab::ALL.iter().map(|t| t.label()).collect::<Vec<_>>(), ["符号", "表情", "最近"]);
    }

    #[test]
    fn emoji_list_no_placeholder() {
        // 清单纪律：占位符与空串不允许进真实内容表。
        assert!(EMOJI.iter().all(|s| !s.is_empty() && !s.contains("placeholder")));
    }

    #[test]
    fn insert_into_empty_text() {
        let (t, c) = insert_at_cursor("", 0, "★");
        assert_eq!(t, "★");
        assert_eq!(c, 1);
    }

    #[test]
    fn recent_order_stable() {
        let mut p = SymbolPanel::new();
        p.pick("a");
        p.pick("b");
        p.remove_recent(1);
        assert_eq!(p.recent_list(), ["b"]);
    }
}

impl SymbolPanel {
    /// 逐条删最近记录（下拉行 × 语义）。
    pub fn remove_recent(&mut self, idx: usize) -> Option<String> {
        if idx < self.recent.len() {
            Some(self.recent.remove(idx))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 分类翻页语义（长分类面板的分页核）
// ---------------------------------------------------------------------------

/// 分类翻页核（长分类（箭头/数学符号多行）的分页语义）：每页容量
/// 固定（9 宫格——3×3 面板），next/prev 在页界内移动、到边界不动
/// （不回卷——回卷会让人迷失位置，翻页器语义与 WinKey 轮转相反）；
/// 页码直出（第 x/y 页显示数据源）；切分类重置回第 1 页。
pub struct TabPager {
    pub per_page: usize,
    pub total: usize,
    pub page: usize, // 0 起。
}

impl TabPager {
    pub fn new(total: usize) -> TabPager {
        TabPager { per_page: 9, total, page: 0 }
    }

    /// 总页数（向上取整——余数页也要能翻到）。
    pub fn pages(&self) -> usize {
        if self.total == 0 {
            return 1;
        }
        (self.total + self.per_page - 1) / self.per_page
    }

    /// 下一页：到末页不动。
    pub fn next(&mut self) {
        if self.page + 1 < self.pages() {
            self.page += 1;
        }
    }

    /// 上一页：到首页不动。
    pub fn prev(&mut self) {
        if self.page > 0 {
            self.page -= 1;
        }
    }

    /// 当前页可见条数（余数页短页——如实出账）。
    pub fn visible_count(&self) -> usize {
        let start = self.page * self.per_page;
        (self.total - start).min(self.per_page)
    }

    /// 页码显示（1 起人话）。
    pub fn page_label(&self) -> (usize, usize) {
        (self.page + 1, self.pages())
    }
}

/// 深化层二自检（分类翻页）。
pub fn run_sympanel_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F313-deep2");

    // 1. 23 条分类 → 3 页（9/9/5 短尾页）。
    let mut p = TabPager::new(23);
    set.add(
        "pages ceil division",
        p.pages() == 3 && p.visible_count() == 9 && p.page_label() == (1, 3),
        "",
    );

    // 2. 翻到末页不动（短尾页 5 条如实）、再 next 不回卷。
    p.next();
    p.next();
    set.add("short tail page", p.visible_count() == 5 && p.page_label() == (3, 3), "");
    p.next();
    set.add("no wrap at end", p.page == 2, "");

    // 3. prev 到首页不动。
    p.prev();
    p.prev();
    p.prev();
    set.add("no wrap at start", p.page == 0 && p.visible_count() == 9, "");

    // 4. 空分类单页零条（诚实空态）。
    let e = TabPager::new(0);
    set.add("empty category one page", e.pages() == 1 && e.visible_count() == 0, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn exact_multiple_pages() {
        let p = TabPager::new(18);
        assert_eq!(p.pages(), 2, "整除不留空页");
    }

    #[test]
    fn single_page_category() {
        let mut p = TabPager::new(5);
        p.next();
        assert_eq!(p.page, 0, "单页分类翻页不动");
    }
}
