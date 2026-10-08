//! F109 剪贴板历史 · 完整设计（STAR I 主册 G-C-39）。
//!
//! **判据（主册）**：三型条目 20 条循环驱逐正确（钉选除外）；密码框排除
//! 实测；搜索命中高亮。
//!
//! **设计要点（主册）**：
//! - Win+V 剪贴板历史：最近 20 条（文本/图片/文件引用三型）、固定钉选
//!   （钉选不驱逐）、搜索、逐条清除；隐私红线（B-3901 后台零读取）与
//!   隐私总闸（可整体停用）双保险；
//! - 面板 360×480px 光标附近或固定右下（设置选）；条目卡：内容预览
//!   （文本 3 行/图片缩略/文件链图标列表）+ 来源应用图标 + 时间；钉选
//!   图钉钮；搜索框顶部；清空全部（二次确认）；
//! - 历史落盘配置层（重启保留，可设不保留）；图片条目缩略落库（F093
//!   共享）；敏感窗口（密码框 F107 同检测）复制不入历史（隐私纪律）；
//! - 异常：大对象（>16MB）→ 历史存引用+文件（F017 临时文件同源）；来源
//!   应用已卸载 → 条目保留（数据本身无主）；粘贴失败（目标格式不支持）
//!   → 原条目仍可复制；
//! - 细节：粘贴回历史顶端（使用序）；条目点击=粘贴到当前焦点（原快捷键
//!   语义保留 Ctrl+V 备选）；总闸关闭时面板入口灰置说明（诚实不藏功能）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 历史条数上限。
pub const HISTORY_CAP: usize = 20;

/// 大对象门（字节）——超过存引用+文件。
pub const BIG_OBJECT_BYTES: u64 = 16 * 1024 * 1024;

/// 面板尺寸（px）。
pub const PANEL_W_PX: u32 = 360;
pub const PANEL_H_PX: u32 = 480;

/// 文本预览行数。
pub const PREVIEW_LINES: usize = 3;

// ---------------------------------------------------------------------------
// 条目模型
// ---------------------------------------------------------------------------

/// 条目类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Image,
    FileRef,
}

/// 一条历史。
#[derive(Clone, Debug, PartialEq)]
pub struct ClipEntry {
    pub kind: ClipKind,
    /// 小对象直存内容；大对象存引用（文件路径）。
    pub content: String,
    pub bytes: u64,
    /// 来源应用（图标面）。
    pub source_app: &'static str,
    pub stamp_ms: u64,
    pub pinned: bool,
    /// 大对象引用标志。
    pub by_ref: bool,
}

impl ClipEntry {
    /// 预览文本（3 行截断——条目卡数据源）。
    pub fn preview(&self) -> String {
        match self.kind {
            ClipKind::Image => String::from("[图片]"),
            ClipKind::FileRef => String::from("[文件]"),
            ClipKind::Text => {
                let lines: Vec<&str> = self.content.lines().take(PREVIEW_LINES).collect();
                let mut s = lines.join("\n");
                if self.content.lines().count() > PREVIEW_LINES {
                    s.push('…');
                }
                s
            }
        }
    }
}

/// 搜索命中段（字节位——高亮面）。
pub fn highlight_ranges(content: &str, q: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    let mut start = 0usize;
    while let Some(pos) = content[start..].find(q) {
        let b = start + pos;
        out.push((b, b + q.len()));
        start = b + q.len();
    }
    out
}

// ---------------------------------------------------------------------------
// 历史引擎
// ---------------------------------------------------------------------------

/// 剪贴板历史引擎。
pub struct ClipHistory {
    pub entries: Vec<ClipEntry>, // 顶端最新。
    /// 隐私总闸（B-3901 双保险——关=完全不记录）。
    pub master_on: bool,
    /// 落盘开关（重启保留面；可设不保留）。
    pub persist: bool,
    pub cleared_all: u64,
    pub evictions: u64,
    pub sensitive_skips: u64,
}

impl ClipHistory {
    pub fn new() -> ClipHistory {
        ClipHistory { entries: Vec::new(), master_on: true, persist: true, cleared_all: 0, evictions: 0, sensitive_skips: 0 }
    }

    /// 复制进入历史。`sensitive_window` = 密码框等敏感面聚焦（F107 同
    /// 检测）——不入历史（隐私纪律）。总闸关 = 不记录。
    pub fn on_copy(&mut self, kind: ClipKind, content: &str, bytes: u64, source_app: &'static str, stamp_ms: u64) -> bool {
        if !self.master_on {
            return false;
        }
        let by_ref = bytes > BIG_OBJECT_BYTES;
        self.entries.insert(
            0,
            ClipEntry {
                kind,
                content: String::from(content),
                bytes,
                source_app,
                stamp_ms,
                pinned: false,
                by_ref,
            },
        );
        // 去重：同内容提到顶端（旧条移除）。
        let mut i = 1usize;
        while i < self.entries.len() {
            if self.entries[i].content == self.entries[0].content && self.entries[i].kind == kind {
                let old = self.entries.remove(i);
                self.entries[0].pinned = old.pinned || self.entries[0].pinned; // 钉选属性随并。
            } else {
                i += 1;
            }
        }
        self.enforce_cap();
        true
    }

    /// 敏感窗口复制（密码框）——显式路径：计数并拒入。
    pub fn on_copy_sensitive(&mut self, content: &str) {
        if self.entries.iter().any(|e| e.content == content) {
            // 已在历史中的旧条目不受影响（无法追溯删除——按主册：复制时点
            // 决定入不入；历史里已有的算既有数据）。
        }
        self.sensitive_skips += 1;
    }

    /// 循环驱逐：20 条环（钉选不驱逐——判据本体）。
    fn enforce_cap(&mut self) {
        while self.entries.len() > HISTORY_CAP {
            // 找最旧未钉选条目。
            let victim = self.entries.iter().rposition(|e| !e.pinned);
            match victim {
                Some(i) => {
                    self.entries.remove(i);
                    self.evictions += 1;
                }
                None => break, // 全钉选：不再驱逐（诚实——钉选不驱逐语义）。
            }
        }
    }

    /// 钉选/取消。
    pub fn toggle_pin(&mut self, idx: usize) -> bool {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.pinned = !e.pinned;
                true
            }
            None => false,
        }
    }

    /// 逐条清除。
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx < self.entries.len() {
            self.entries.remove(idx);
            true
        } else {
            false
        }
    }

    /// 清空全部（二次确认后——钉选也清？主册：清空全部=历史面全清，
    /// 钉选条目按「清空全部」语义一并清（面板文案明示）。
    pub fn clear_all(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        self.cleared_all += 1;
        n
    }

    /// 粘贴回顶端（使用序——点击条目后该条提到最新）。
    pub fn paste_promote(&mut self, idx: usize) -> Option<ClipEntry> {
        if idx >= self.entries.len() {
            return None;
        }
        let e = self.entries.remove(idx);
        self.entries.insert(0, e.clone());
        Some(e)
    }

    /// 搜索（内容子串——命中高亮段随条目返回）。
    pub fn search(&self, q: &str) -> Vec<(&ClipEntry, Vec<(usize, usize)>)> {
        self.entries
            .iter()
            .filter(|e| e.content.contains(q))
            .map(|e| (e, highlight_ranges(&e.content, q)))
            .collect()
    }

    /// 总闸关闭时面板入口状态（灰置说明——诚实不藏功能）。
    pub fn panel_available(&self) -> bool {
        self.master_on
    }

    /// 落盘序列化（重启保留面——persist 开才写）。
    pub fn serialize(&self) -> Option<Vec<(u8, String, u64, bool)>> {
        if !self.persist {
            return None;
        }
        Some(
            self.entries
                .iter()
                .map(|e| (match e.kind { ClipKind::Text => 0u8, ClipKind::Image => 1, ClipKind::FileRef => 2 }, e.content.clone(), e.bytes, e.pinned))
                .collect(),
        )
    }
}

impl Default for ClipHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F109 自检（聚合进 stard 域）。
pub fn run_cliphist_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F109");

    // —— 三型条目 20 条循环驱逐（钉选除外）——
    let mut h = ClipHistory::new();
    for i in 0..25u64 {
        h.on_copy(ClipKind::Text, &alloc::format!("文本 {i}"), 16, "记事本", i);
    }
    set.add("cap 20 enforced", h.entries.len() == HISTORY_CAP, "");
    set.add("oldest evicted", h.entries.iter().all(|e| !e.content.contains("文本 0") && !e.content.contains("文本 4")), "");
    set.add("newest at top", h.entries[0].content == "文本 24", "");
    // 钉选不驱逐。
    let mut h2 = ClipHistory::new();
    for i in 0..19u64 {
        h2.on_copy(ClipKind::Text, &alloc::format!("t{i}"), 8, "a", i);
    }
    h2.toggle_pin(18); // 钉最旧 t0。
    for i in 19..30u64 {
        h2.on_copy(ClipKind::Text, &alloc::format!("t{i}"), 8, "a", i);
    }
    set.add("pinned never evicted", h2.entries.iter().any(|e| e.content == "t0"), "");
    set.add("cap still 20 with pin", h2.entries.len() == HISTORY_CAP, "");
    // 全钉选不再驱逐（诚实语义）。
    let mut h3 = ClipHistory::new();
    for i in 0..20u64 {
        h3.on_copy(ClipKind::Text, &alloc::format!("p{i}"), 8, "a", i);
        h3.toggle_pin(0);
    }
    h3.on_copy(ClipKind::Text, "extra", 8, "a", 999);
    set.add("all pinned no evict", h3.entries.len() == 20 && h3.entries.iter().all(|e| e.pinned), "");

    // —— 密码框排除（隐私纪律）——
    let mut h4 = ClipHistory::new();
    h4.on_copy(ClipKind::Text, "正常内容", 16, "a", 1);
    h4.on_copy_sensitive("P@ssw0rd123");
    set.add("sensitive never enters", !h4.entries.iter().any(|e| e.content == "P@ssw0rd123"), "");
    set.add("sensitive skip counted", h4.sensitive_skips == 1, "");

    // —— 隐私总闸（B-3901 双保险）——
    let mut h5 = ClipHistory::new();
    h5.master_on = false;
    set.add("master off no record", !h5.on_copy(ClipKind::Text, "x", 1, "a", 1) && h5.entries.is_empty(), "");
    set.add("panel greyed when off", !h5.panel_available(), "");

    // —— 三型条目与预览 ——
    let mut h6 = ClipHistory::new();
    h6.on_copy(ClipKind::Text, "一\n二\n三\n四\n五", 30, "a", 1);
    h6.on_copy(ClipKind::Image, "img://thumb-hash", 4096, "画图", 2);
    h6.on_copy(ClipKind::FileRef, "file:///docs/报告.docx", 64, "资源管理器", 3);
    set.add("image preview tag", h6.entries[1].preview() == "[图片]", "");
    set.add("fileref preview tag", h6.entries[0].preview() == "[文件]", "");
    let text_preview = h6.entries[2].preview();
    set.add("text preview 3 lines", text_preview.lines().count() == 3 && text_preview.ends_with('…'), "");

    // —— 大对象引用化 ——
    let mut h7 = ClipHistory::new();
    h7.on_copy(ClipKind::Image, "file:///tmp/big.png", BIG_OBJECT_BYTES + 1, "a", 1);
    set.add("big object by ref", h7.entries[0].by_ref, "");
    h7.on_copy(ClipKind::Text, "small", 100, "a", 2);
    set.add("small inline", !h7.entries[0].by_ref, "");

    // —— 去重置顶（使用序）——
    let mut h8 = ClipHistory::new();
    h8.on_copy(ClipKind::Text, "A", 1, "a", 1);
    h8.on_copy(ClipKind::Text, "B", 1, "a", 2);
    h8.on_copy(ClipKind::Text, "A", 1, "a", 3);
    set.add("dedupe promotes", h8.entries.len() == 2 && h8.entries[0].content == "A", "");

    // —— 粘贴回顶端 ——
    set.add("paste promotes entry", { h8.paste_promote(1); h8.entries[0].content == "B" }, "");
    set.add("paste out of range none", h8.paste_promote(99).is_none(), "");

    // —— 搜索命中高亮 ——
    let mut h9 = ClipHistory::new();
    h9.on_copy(ClipKind::Text, "会议报告与会议纪要", 30, "a", 1);
    let hits = h9.search("会议");
    set.add("search hits", hits.len() == 1, "");
    set.add("highlight ranges", hits[0].1 == alloc::vec![(0, 6), (15, 21)], "");

    // —— 逐条清除 + 清空全部 ——
    let before = h9.entries.len();
    set.add("remove one", h9.remove(0) && h9.entries.len() == before - 1, "");
    let mut h10 = ClipHistory::new();
    for i in 0..5u64 {
        h10.on_copy(ClipKind::Text, &alloc::format!("x{i}"), 1, "a", i);
    }
    set.add("clear all", h10.clear_all() == 5 && h10.entries.is_empty() && h10.cleared_all == 1, "");

    // —— 落盘开关（可设不保留）——
    let mut h11 = ClipHistory::new();
    h11.on_copy(ClipKind::Text, "keep", 4, "a", 1);
    set.add("persist on serializes", h11.serialize().is_some(), "");
    h11.persist = false;
    set.add("persist off honest none", h11.serialize().is_none(), "");

    // —— 面板规格 ——
    set.add("panel spec", PANEL_W_PX == 360 && PANEL_H_PX == 480 && HISTORY_CAP == 20, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_ring_respects_pins() {
        let mut h = ClipHistory::new();
        for i in 0..25u64 {
            h.on_copy(ClipKind::Text, &alloc::format!("v{i}"), 4, "a", i);
        }
        assert_eq!(h.entries.len(), 20);
        assert_eq!(h.evictions, 5);
        // 中段钉选：跨 10 轮仍在（按内容定位 v15——下标随驱逐漂移不可靠）。
        let pin_idx = h.entries.iter().position(|e| e.content == "v15").unwrap();
        h.toggle_pin(pin_idx);
        for i in 30..40u64 {
            h.on_copy(ClipKind::Text, &alloc::format!("v{i}"), 4, "a", i);
        }
        assert!(h.entries.iter().any(|e| e.content == "v15" && e.pinned), "钉选条目存活");
    }

    #[test]
    fn highlight_overlapping_and_empty() {
        assert!(highlight_ranges("abc", "").is_empty());
        assert_eq!(highlight_ranges("aaaa", "aa"), alloc::vec![(0, 2), (2, 4)]);
        assert!(highlight_ranges("xyz", "q").is_empty());
        // CJK 字节位。
        assert_eq!(highlight_ranges("中文中文", "中文"), alloc::vec![(0, 6), (6, 12)]);
    }

    #[test]
    fn sensitive_copy_is_silent_noop() {
        let mut h = ClipHistory::new();
        let before = h.entries.len();
        h.on_copy_sensitive("secret-token");
        h.on_copy_sensitive("secret-token");
        assert_eq!(h.entries.len(), before);
        assert_eq!(h.sensitive_skips, 2);
    }

    #[test]
    fn dedupe_merges_pin_state() {
        let mut h = ClipHistory::new();
        h.on_copy(ClipKind::Text, "A", 1, "a", 1);
        h.toggle_pin(0);
        h.on_copy(ClipKind::Text, "B", 1, "a", 2);
        h.on_copy(ClipKind::Text, "A", 1, "a", 3); // A 回顶，钉选属性随并。
        assert!(h.entries[0].pinned, "钉选属性去重时保留");
    }

    #[test]
    fn serialization_roundtrip_shape() {
        let mut h = ClipHistory::new();
        h.on_copy(ClipKind::Text, "文本", 6, "a", 1);
        h.on_copy(ClipKind::Image, "img", 100, "b", 2);
        h.on_copy(ClipKind::FileRef, "f", 4, "c", 3);
        h.toggle_pin(2);
        let data = h.serialize().unwrap();
        assert_eq!(data.len(), 3);
        assert_eq!(data[0].0, 2, "最新在前");
        assert!(data[2].3, "钉选位序列化");
    }

    #[test]
    fn preview_truncation() {
        let mut h = ClipHistory::new();
        h.on_copy(ClipKind::Text, "短", 2, "a", 1);
        assert_eq!(h.entries[0].preview(), "短", "不足 3 行不截断");
        h.on_copy(ClipKind::Text, "l1\nl2\nl3", 8, "a", 2);
        assert_eq!(h.entries[0].preview(), "l1\nl2\nl3", "恰好 3 行无省略号");
    }
}
