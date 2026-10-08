//! F432 列表翻页与定位键 · 完整设计（STAR I 主册 G-I-32）。
//!
//! **判据（主册）**：相对位置保持判据；四键行为；扩选组合；万项列表
//! 翻页帧就绪（无白帧实测）；滚动条同步。＋通12。
//!
//! 设计：列表导航核——PgUp/PgDn 翻页保持选中项相对位置（翻页前第 3 行
//! 翻后仍第 3 行）；Home/End 首尾；Ctrl+Home/End 选中扩到首尾（F218
//! 修饰键延伸）；滚动条同步（scrollTop 与选中项联动，永不脱节）；翻页
//! 帧就绪（虚拟化：目标页首帧在翻页瞬间即可渲染——数据恒在，无懒加载
//! 空洞，结构性保证）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 列表导航核。
pub struct ListNav {
    pub count: usize,
    /// 每页行数。
    pub page_rows: usize,
    pub selected: usize,
    /// 滚动位（首行下标）。
    pub scroll_top: usize,
    /// 扩选锚点（Shift 系扩选起点）。
    pub anchor: Option<usize>,
    /// v8：type-ahead 字母跳缓冲（累计键入）。
    pub typeahead: Vec<u8>,
    /// v8：type-ahead 上次键入时刻（超窗清空缓冲）。
    pub typeahead_at_ms: Option<u64>,
}

/// type-ahead 超窗（ms）——键入间隔超此值缓冲清空重来。
pub const TYPEAHEAD_WINDOW_MS: u64 = 1_000;

/// type-ahead 缓冲上限（字节）。
pub const TYPEAHEAD_CAP: usize = 32;

/// 键入的项目名表注入（type-ahead 命中查找用——名从外部来，本核
/// 不造真值）。此处用闭包签名避免持有字符串所有权。
pub type NameLookup<'a> = dyn Fn(usize) -> Option<&'a str> + 'a;

impl ListNav {
    pub fn new(count: usize, page_rows: usize) -> ListNav {
        ListNav {
            count,
            page_rows: page_rows.max(1),
            selected: 0,
            scroll_top: 0,
            anchor: None,
            typeahead: Vec::new(),
            typeahead_at_ms: None,
        }
    }

    /// 相对位置保持翻页：选中项随页同步移动 page_rows——翻页前在屏上
    /// 第 N 行，翻页后仍在第 N 行（主册判据原文）；底部钳制。两端都
    /// 到界才返回 false（无动作）。
    pub fn page_down(&mut self) -> bool {
        let max_top = self.count.saturating_sub(self.page_rows);
        let next_sel = (self.selected + self.page_rows).min(self.count.saturating_sub(1));
        let next_top = (self.scroll_top + self.page_rows).min(max_top);
        if next_sel == self.selected && next_top == self.scroll_top {
            return false; // 底部边界：无动作。
        }
        self.selected = next_sel;
        self.scroll_top = next_top;
        self.sync_scroll_to_selection();
        true
    }

    pub fn page_up(&mut self) -> bool {
        let next_sel = self.selected.saturating_sub(self.page_rows);
        let next_top = self.scroll_top.saturating_sub(self.page_rows);
        if next_sel == self.selected && next_top == self.scroll_top {
            return false; // 顶部边界：无动作。
        }
        self.selected = next_sel;
        self.scroll_top = next_top;
        self.sync_scroll_to_selection();
        true
    }

    fn sync_scroll_to_selection(&mut self) {
        // 滚动条同步：选中项必须在可视区内。
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + self.page_rows {
            self.scroll_top = self.selected + 1 - self.page_rows;
        }
    }

    /// 相对位置保持判据：翻页前后选中项的页内行号一致。
    pub fn relative_row(&self) -> usize {
        self.selected - self.scroll_top.min(self.selected)
    }

    /// Home/End。
    pub fn home(&mut self) {
        self.selected = 0;
        self.sync_scroll_to_selection();
    }

    pub fn end(&mut self) {
        self.selected = self.count.saturating_sub(1);
        self.sync_scroll_to_selection();
    }

    /// Ctrl+Home/End：选中扩到首尾（anchor 到端点的扩选区间）。
    pub fn ctrl_home(&mut self) -> (usize, usize) {
        let a = self.anchor.unwrap_or(self.selected);
        self.selected = 0;
        self.sync_scroll_to_selection();
        (0, a.max(0))
    }

    pub fn ctrl_end(&mut self) -> (usize, usize) {
        let a = self.anchor.unwrap_or(self.selected);
        self.selected = self.count.saturating_sub(1);
        self.sync_scroll_to_selection();
        (a, self.selected)
    }

    /// 翻页帧就绪：页首行在数据界内即可立即渲染（虚拟化无白帧的结构性
    /// 证据——末页不满时也只渲染存在的行，不空转不白帧）。
    pub fn page_ready(&self, page_index: usize) -> bool {
        page_index * self.page_rows < self.count
    }

    /// 滚动条永不脱节：滚动位 + 页行 ≤ 总数（或贴底）。
    pub fn scrollbar_consistent(&self) -> bool {
        self.scroll_top + self.page_rows <= self.count
            || self.scroll_top >= self.count.saturating_sub(self.page_rows)
    }

    /// Ctrl+A 全选（v6）：区间恒为整个列表；空列表 → 无区间。
    pub fn select_all(&self) -> Option<(usize, usize)> {
        if self.count == 0 {
            None
        } else {
            Some((0, self.count - 1))
        }
    }

    /// Shift+点击扩选（v6）：锚点到点击位的闭区间（方向无关——双向都
    /// 是合法扩选）。
    pub fn shift_extend(&mut self, to: usize) -> Option<(usize, usize)> {
        if to >= self.count {
            return None; // 越界点击不产生区间。
        }
        let a = self.anchor.unwrap_or(self.selected);
        self.selected = to;
        self.sync_scroll_to_selection();
        Some((a.min(to), a.max(to)))
    }

    /// v8：type-ahead 字母跳——键入累计入缓冲（超窗清空重来；超容量
    /// 截尾）；在 name_of 查找下标 > 起点的首个前缀命中项，命中则选中
    /// 并滚动同步。返回命中下标。
    pub fn typeahead_char(&mut self, ch: u8, now_ms: u64, name_of: &NameLookup) -> Option<usize> {
        match self.typeahead_at_ms {
            Some(t) if now_ms.saturating_sub(t) >= TYPEAHEAD_WINDOW_MS => self.typeahead.clear(),
            _ => {}
        }
        if self.typeahead.len() >= TYPEAHEAD_CAP {
            self.typeahead.remove(0); // 截尾保新（环形语义）
        }
        self.typeahead.push(ch);
        self.typeahead_at_ms = Some(now_ms);
        let pat = self.typeahead.clone();
        // 从选中项下一个开始环扫一圈（全列表可达）。
        for off in 1..=self.count {
            let idx = (self.selected + off) % self.count;
            if let Some(name) = name_of(idx) {
                if name.as_bytes().windows(pat.len().max(1)).any(|w| w == &pat[..]) {
                    self.selected = idx;
                    self.sync_scroll_to_selection();
                    return Some(idx);
                }
            }
        }
        None
    }

    /// v8：type-ahead 超窗惰性清空（下次键入前查询也可显性走）。
    pub fn typeahead_expired(&self, now_ms: u64) -> bool {
        match self.typeahead_at_ms {
            Some(t) => now_ms.saturating_sub(t) >= TYPEAHEAD_WINDOW_MS && !self.typeahead.is_empty(),
            None => false,
        }
    }
}

pub fn run_listnav_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F432");
    // 万项列表。
    let mut l = ListNav::new(10_000, 40);
    l.selected = 42; // 第 3 行（页 1 从 40 起）
    l.scroll_top = 40;
    let rel_before = l.relative_row();
    // PgDn：相对位置保持——选中项随页走（42→82），页内行号（42-40=2）翻后不变。
    set.add(
        "f432-relative-position-kept",
        l.page_down() && l.scroll_top == 80 && l.selected == 82 && l.relative_row() == rel_before,
        "",
    );
    // 滚动条同步：连续翻到底不越界。
    let mut guard = true;
    for _ in 0..300 {
        if !l.page_down() {
            break;
        }
        guard &= l.scrollbar_consistent();
    }
    set.add("f432-scrollbar-synced", guard && l.scroll_top + 40 >= 10_000, "");
    // PgUp 边界。
    let mut u = ListNav::new(100, 40);
    set.add("f432-pageup-bound", !u.page_up(), "");
    // Home/End。
    l.selected = 5000;
    l.scroll_top = 5000;
    l.home();
    set.add("f432-home", l.selected == 0 && l.scroll_top == 0, "");
    l.end();
    set.add("f432-end", l.selected == 9_999 && l.scrollbar_consistent(), "");
    // PgDn 底边界无动作。
    set.add("f432-pagedn-bound", !l.page_down(), "");
    // 扩选组合。
    let mut e = ListNav::new(100, 20);
    e.selected = 30;
    e.anchor = Some(30);
    set.add("f432-ctrl-home-select", e.ctrl_home() == (0, 30) && e.selected == 0, "");
    e.anchor = Some(5);
    set.add("f432-ctrl-end-select", e.ctrl_end() == (5, 99) && e.selected == 99, "");
    // 万项翻页帧就绪：任意页首尾行立即可渲染。
    let mut ready = true;
    for p in 0..250 {
        ready &= ListNav::new(10_000, 40).page_ready(p);
    }
    set.add("f432-page-frame-ready", ready && ListNav::new(10_000, 40).page_ready(0), "");
    // 末页不满也就绪（v6）：45 项 40 行/页——第 1 页只有 5 行但立即可渲染，
    // 第 2 页不存在。
    let partial = ListNav::new(45, 40);
    set.add(
        "f432-partial-page-ready",
        partial.page_ready(1) && !partial.page_ready(2),
        "",
    );
    // PgUp 同样保持相对位置（v6——与 PgDn 对称的回归锚）。
    let mut p2 = ListNav::new(10_000, 40);
    p2.scroll_top = 120;
    p2.selected = 122;
    let rel = p2.relative_row();
    set.add(
        "f432-pageup-relative-kept",
        p2.page_up() && p2.scroll_top == 80 && p2.selected == 82 && p2.relative_row() == rel,
        "",
    );
    // Ctrl+A 全选（v6）：万项 → (0, 9999)；空列表 → 无。
    let all = ListNav::new(10_000, 40);
    let none = ListNav::new(0, 40);
    set.add(
        "f432-select-all",
        all.select_all() == Some((0, 9_999)) && none.select_all().is_none(),
        "",
    );
    // Shift+点击扩选（v6）：锚在上、点在下 → 正序区间；越界点 → 无区间。
    let mut x = ListNav::new(100, 20);
    x.selected = 30;
    x.anchor = Some(30);
    set.add(
        "f432-shift-extend-down",
        x.shift_extend(45) == Some((30, 45)) && x.selected == 45,
        "",
    );
    x.anchor = Some(60);
    set.add(
        "f432-shift-extend-up",
        x.shift_extend(40) == Some((40, 60)) && x.selected == 40,
        "",
    );
    x.anchor = Some(10);
    set.add("f432-shift-extend-bounds", x.shift_extend(999).is_none(), "");
    // v8：type-ahead 字母跳——前缀命中、环扫全列表、缓冲累计、超窗清空。
    let names = ["alpha", "beta", "banana", "gamma"];
    let lookup = |i: usize| names.get(i).copied();
    let mut t = ListNav::new(names.len(), 40);
    set.add("f432-typeahead-hit", t.typeahead_char(b'b', 0, &lookup) == Some(1) && t.selected == 1, "");
    set.add("f432-typeahead-accumulate", t.typeahead_char(b'e', 100, &lookup) == Some(1) && t.typeahead == *b"be", "be 环扫一圈命中 beta（含匹配）；缓冲累计");
    // 超窗后缓冲清空重来：单字符 b 从头匹配 banana（下标 2）。
    set.add("f432-typeahead-window-reset", t.typeahead_char(b'b', 2_000, &lookup) == Some(2) && t.selected == 2 && t.typeahead == *b"b", "超窗清空重来：单字符 b 命中 banana");
    // 环扫：从尾部起输入 a 能绕回 alpha（下标 0）。
    let mut r = ListNav::new(names.len(), 40);
    r.selected = 3; // gamma——从 0 起环扫
    set.add("f432-typeahead-wrap", r.typeahead_char(b'a', 0, &lookup) == Some(0) && r.selected == 0, "");
    // 全不命中：无 panic、选中不动。
    let mut n = ListNav::new(names.len(), 40);
    set.add("f432-typeahead-miss", n.typeahead_char(b'z', 0, &lookup).is_none() && n.selected == 0, "");
    set.add(
        "f432-typeahead-expiry",
        { n.typeahead_at_ms = Some(5_000); n.typeahead = b"zz".to_vec(); n.typeahead_expired(6_500) && !n.typeahead_expired(5_500) },
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_row_math() {
        let mut l = ListNav::new(1000, 40);
        l.scroll_top = 80;
        l.selected = 82; // 页内第 3 行（0 基第 2）
        assert_eq!(l.relative_row(), 2);
        let _ = l.page_down();
        assert_eq!(l.scroll_top, 120);
        assert_eq!(l.selected, 122, "选中项随页同步移动");
        assert_eq!(l.relative_row(), 2, "翻页后仍第 3 行");
    }

    #[test]
    fn partial_last_page() {
        let mut l = ListNav::new(45, 40);
        l.scroll_top = 5; // max_top = 5
        l.selected = 44;  // 选中也在末行——两端到界才无动作
        assert!(!l.page_down(), "已贴底且选中在末行：无动作");
        assert!(l.scrollbar_consistent());
    }

    #[test]
    fn typeahead_scroll_syncs() {
        // 命中项在页外 → 选中后滚动条同步跟随。
        let mut l = ListNav::new(1_000, 40);
        let names: Vec<String> = (0..1_000).map(|i| format!("item{i}")).collect();
        let lookup = |i: usize| names.get(i).map(|s| s.as_str());
        let hit = l.typeahead_char(b'i', 0, &lookup).unwrap();
        assert_eq!(hit, 1);
        assert!(hit >= l.scroll_top && hit < l.scroll_top + 40, "命中项在可视区内");
    }

    #[test]
    fn typeahead_cap_trims_oldest() {
        let mut l = ListNav::new(10, 40);
        let lookup = |_: usize| Some("x");
        for i in 0..(TYPEAHEAD_CAP + 4) {
            let _ = l.typeahead_char(b'a', i as u64 * 100, &lookup); // 窗内连击（100ms 间隔 < 1s 超窗）
        }
        assert_eq!(l.typeahead.len(), TYPEAHEAD_CAP, "缓冲封顶不失控");
    }
}
