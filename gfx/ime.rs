//! TRINITY-500 · AI-06 · F137 IME 候选窗（内核版） / F138 中文拼音输入
//!
//! 诚实边界：内核版 IME 是**字面音节映射表**，无语言模型、无云、无联网。
//! 表是什么就出什么，不做「智能联想」的暗示。

pub const CANDIDATE_LEN: usize = 12;
pub const MAX_CANDIDATES: usize = 27;
pub const PAGE_SIZE: usize = 9;

// ---------------------------------------------------------------------------
// F137 IME 候选窗（内核版）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    buf: [u8; CANDIDATE_LEN],
    len: usize,
}

impl Candidate {
    pub fn new(text: &str) -> Option<Candidate> {
        let b = text.as_bytes();
        if b.is_empty() || b.len() > CANDIDATE_LEN {
            return None;
        }
        let mut buf = [0u8; CANDIDATE_LEN];
        buf[..b.len()].copy_from_slice(b);
        Some(Candidate { buf, len: b.len() })
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

pub struct CandidateWindow {
    items: [Option<Candidate>; MAX_CANDIDATES],
    count: usize,
    pub page: usize,
    pub selected: usize,
}

impl CandidateWindow {
    pub const fn new() -> CandidateWindow {
        CandidateWindow { items: [None; MAX_CANDIDATES], count: 0, page: 0, selected: 0 }
    }

    pub fn page_size(&self) -> usize {
        PAGE_SIZE
    }

    pub fn set(&mut self, texts: &[&str]) {
        self.count = 0;
        self.page = 0;
        self.selected = 0;
        for t in texts.iter() {
            if self.count >= MAX_CANDIDATES {
                break;
            }
            if let Some(c) = Candidate::new(t) {
                self.items[self.count] = Some(c);
                self.count += 1;
            }
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn page_count(&self) -> usize {
        self.count / PAGE_SIZE + usize::from(self.count % PAGE_SIZE != 0)
    }

    pub fn visible(&self) -> bool {
        self.count > 0
    }

    /// 当前页的条目数。
    pub fn page_items(&self, out: &mut [Candidate]) -> usize {
        let start = self.page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.count);
        let mut n = 0usize;
        for i in start..end {
            if let Some(c) = self.items[i] {
                if n < out.len() {
                    out[n] = c;
                    n += 1;
                }
            }
        }
        n
    }

    /// 移动选中项（在本页内）。
    pub fn move_selection(&mut self, delta: i32) {
        if self.count == 0 {
            return;
        }
        let page_items = self.page_items_len();
        let cur = self.selected as i32 + delta;
        if cur < 0 {
            self.selected = page_items - 1;
        } else if cur >= page_items as i32 {
            self.selected = 0;
        } else {
            self.selected = cur as usize;
        }
    }

    fn page_items_len(&self) -> usize {
        let start = self.page * PAGE_SIZE;
        PAGE_SIZE.min(self.count.saturating_sub(start))
    }

    /// 翻页；返回是否真的翻了。
    pub fn flip_page(&mut self, forward: bool) -> bool {
        let pages = self.page_count();
        if pages <= 1 {
            return false;
        }
        self.page = if forward {
            (self.page + 1) % pages
        } else if self.page == 0 {
            pages - 1
        } else {
            self.page - 1
        };
        self.selected = 0;
        true
    }

    /// 取当前选中候选（全局下标）。
    pub fn current(&self) -> Option<Candidate> {
        let idx = self.page * PAGE_SIZE + self.selected;
        if idx < self.count {
            self.items[idx]
        } else {
            None
        }
    }
}

impl Default for CandidateWindow {
    fn default() -> Self {
        CandidateWindow::new()
    }
}

// ---------------------------------------------------------------------------
// F138 中文拼音输入
// ---------------------------------------------------------------------------

/// 音节 → 候选串（空格分隔）。表即能力边界。
pub const PINYIN_TABLE: [(&str, &str); 26] = [
    ("a", "啊 阿 吖"),
    ("ai", "爱 哀 碍 癌"),
    ("an", "安 案 暗 岸"),
    ("ba", "把 八 巴 爸 拔"),
    ("bu", "不 部 步 布 补"),
    ("cao", "草 操 曹 槽"),
    ("chi", "吃 迟 持 池 尺"),
    ("dao", "到 道 倒 岛 导"),
    ("de", "的 德 得 地"),
    ("ge", "个 歌 格 哥 隔"),
    ("hao", "好 号 毫 豪"),
    ("he", "和 何 河 合 盒"),
    ("ji", "机 及 几 记 级"),
    ("jian", "件 见 间 简 建"),
    ("kan", "看 砍 刊 堪"),
    ("le", "了 乐 勒 了"),
    ("ma", "吗 马 嘛 妈 码"),
    ("ni", "你 尼 泥 拟 逆"),
    ("shi", "是 时 事 十 使"),
    ("wo", "我 握 沃 窝"),
    ("xi", "西 系 希 息 洗"),
    ("yi", "一 以 已 意 医"),
    ("you", "有 又 右 优 游"),
    ("zhong", "中 重 种 众 终"),
    ("zi", "子 字 自 资 紫"),
    ("zuo", "做 作 坐 左 座"),
];

pub const MAX_COMPOSING: usize = 8;
pub const MAX_COMMITTED: usize = 64;

pub struct PinyinIme {
    composing: [u8; MAX_COMPOSING],
    composing_len: usize,
    committed: [u8; MAX_COMMITTED],
    committed_len: usize,
    pub window: CandidateWindow,
}

impl PinyinIme {
    pub const fn new() -> PinyinIme {
        PinyinIme {
            composing: [0u8; MAX_COMPOSING],
            composing_len: 0,
            committed: [0u8; MAX_COMMITTED],
            committed_len: 0,
            window: CandidateWindow::new(),
        }
    }

    pub fn composing(&self) -> &str {
        core::str::from_utf8(&self.composing[..self.composing_len]).unwrap_or("")
    }

    pub fn committed(&self) -> &str {
        core::str::from_utf8(&self.committed[..self.committed_len]).unwrap_or("")
    }

    pub fn candidate_count(&self) -> usize {
        self.window.count()
    }

    /// 追加拼音字母；只接受 a-z，长度受限。返回组合串长度。
    pub fn feed(&mut self, letters: &str) -> usize {
        for b in letters.bytes() {
            if !b.is_ascii_lowercase() {
                continue;
            }
            if self.composing_len >= MAX_COMPOSING {
                break;
            }
            self.composing[self.composing_len] = b;
            self.composing_len += 1;
        }
        self.update_candidates();
        self.composing_len
    }

    pub fn backspace(&mut self) -> bool {
        if self.composing_len == 0 {
            return false;
        }
        self.composing_len -= 1;
        self.update_candidates();
        true
    }

    fn update_candidates(&mut self) {
        let key = self.composing();
        if key.is_empty() {
            self.window.set(&[]);
            return;
        }
        let mut list: [&str; 8] = [""; 8];
        let mut n = 0usize;
        if let Some((_, cands)) = PINYIN_TABLE.iter().find(|(k, _)| *k == key) {
            for part in cands.split(' ') {
                if !part.is_empty() && n < list.len() {
                    list[n] = part;
                    n += 1;
                }
            }
        } else {
            // 未命中：试试「已输入串是某个音节的前缀」，给出提示性候选为空。
            let prefix_hit = PINYIN_TABLE.iter().any(|(k, _)| k.starts_with(key));
            if !prefix_hit {
                self.window.set(&[]);
                return;
            }
        }
        self.window.set(&list[..n]);
    }

    /// 选中第 `i` 个候选并上屏。
    pub fn select(&mut self, i: usize) -> bool {
        let target = if i < PAGE_SIZE {
            let idx = self.window.page * PAGE_SIZE + i;
            if idx < self.window.count() {
                self.window.items[idx]
            } else {
                None
            }
        } else {
            None
        };
        match target {
            Some(c) => {
                let t = c.text();
                let bytes = t.as_bytes();
                if self.committed_len + bytes.len() > MAX_COMMITTED {
                    return false;
                }
                self.committed[self.committed_len..self.committed_len + bytes.len()].copy_from_slice(bytes);
                self.committed_len += bytes.len();
                self.reset_composing();
                true
            }
            None => false,
        }
    }

    /// 直接把当前拼音以字母形式上屏（用户按了回车但没选候选）。
    pub fn commit_raw(&mut self) -> bool {
        if self.composing_len == 0 {
            return false;
        }
        if self.committed_len + self.composing_len > MAX_COMMITTED {
            return false;
        }
        let len = self.composing_len;
        self.committed[self.committed_len..self.committed_len + len]
            .copy_from_slice(&self.composing[..len]);
        self.committed_len += len;
        self.reset_composing();
        true
    }

    fn reset_composing(&mut self) {
        self.composing = [0u8; MAX_COMPOSING];
        self.composing_len = 0;
        self.window.set(&[]);
    }

    pub fn reset(&mut self) {
        self.reset_composing();
        self.committed = [0u8; MAX_COMMITTED];
        self.committed_len = 0;
    }
}

impl Default for PinyinIme {
    fn default() -> Self {
        PinyinIme::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f137_window_pages() {
        let mut w = CandidateWindow::new();
        assert_eq!(w.page_size(), 9);
        assert!(!w.visible());
        let items = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
        w.set(&items);
        assert_eq!(w.count(), 11);
        assert_eq!(w.page_count(), 2);
        let mut page = [Candidate::new("x").unwrap(); 9];
        assert_eq!(w.page_items(&mut page), 9);
        assert_eq!(page[0].text(), "1");
        assert!(w.flip_page(true));
        let mut page2 = [Candidate::new("x").unwrap(); 9];
        assert_eq!(w.page_items(&mut page2), 2);
        assert_eq!(page2[0].text(), "10");
    }

    #[test]
    fn f137_selection_wraps() {
        let mut w = CandidateWindow::new();
        w.set(&["a", "b", "c"]);
        w.move_selection(-1);
        assert_eq!(w.current().unwrap().text(), "c");
        w.move_selection(1);
        assert_eq!(w.current().unwrap().text(), "a");
    }

    #[test]
    fn f138_pinyin_lookup() {
        let mut ime = PinyinIme::new();
        assert_eq!(ime.feed("ni"), 2);
        assert_eq!(ime.composing(), "ni");
        assert!(ime.candidate_count() >= 4);
        assert!(ime.select(0));
        assert_eq!(ime.committed(), "你");
        assert_eq!(ime.composing(), "");
    }

    #[test]
    fn f138_unknown_syllable_has_no_candidates() {
        let mut ime = PinyinIme::new();
        ime.feed("qqq");
        assert_eq!(ime.candidate_count(), 0);
        assert!(!ime.select(0));
        assert!(ime.commit_raw());
        assert_eq!(ime.committed(), "qqq");
    }

    #[test]
    fn f138_backspace_updates() {
        let mut ime = PinyinIme::new();
        ime.feed("zh");
        assert!(ime.candidate_count() == 0, "prefix shows nothing yet is a valid prefix");
        ime.backspace();
        assert_eq!(ime.composing(), "z");
    }

    #[test]
    fn f138_commit_limit() {
        let mut ime = PinyinIme::new();
        for _ in 0..40 {
            ime.feed("ni");
            ime.select(0);
        }
        assert!(ime.committed().chars().count() <= MAX_COMMITTED / 3 + 1);
    }
}
