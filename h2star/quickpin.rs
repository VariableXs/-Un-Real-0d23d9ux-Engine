//! F253 快速访问固定 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：固定/取消/拖排序用例；自动推荐与固定项共存显示
//! 逻辑；持久化（重启验证）；拖拽固定动画与落点指示。
//!
//! **设计要点（主册）**：资源管理器左侧栏「快速访问」：任意文件夹可
//! 「固定到快速访问」（右键/拖到侧栏均可），取消固定同样一步；未固定时
//! 侧栏自动显示最近使用的 5 个文件夹（F072 引擎供数）+ 最近打开的 5 个
//! 文件；固定项排序可拖、固定状态持久化。
//!
//! 实装：固定表（有序，拖排序=摘除再插位）+ 推荐区（F072 供给的最近
//! 使用流里取「最近 5 文件夹 + 最近 5 文件」，已固定的文件夹自动从推荐
//! 区隐藏——共存显示逻辑）；快照序列化做持久化往返；重复固定幂等。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 自动推荐配额：最近 5 文件夹 + 最近 5 文件（主册定值）。
pub const RECENT_FOLDERS: usize = 5;
pub const RECENT_FILES: usize = 5;

/// 推荐条目来源（F072 引擎供数的两类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedKind {
    Folder,
    File,
}

/// 快速访问模型。
pub struct QuickAccess {
    /// 固定文件夹（有序——拖排序直接改这个序）。
    pinned: Vec<String>,
    /// F072 最近使用流（注入：(路径, 类型, 最近使用分钟戳)）。
    feed: Vec<(String, FeedKind, u64)>,
}

impl QuickAccess {
    pub fn new() -> QuickAccess {
        QuickAccess { pinned: Vec::new(), feed: Vec::new() }
    }

    /// 固定（右键/拖入同口）；已固定幂等返回 false。
    pub fn pin(&mut self, path: &str) -> bool {
        if self.pinned.iter().any(|p| p == path) {
            return false;
        }
        self.pinned.push(String::from(path));
        true
    }

    /// 取消固定；未固定返回 false。
    pub fn unpin(&mut self, path: &str) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|p| p != path);
        self.pinned.len() != before
    }

    /// 拖排序：把 `path` 从当前位置移到 `to`（0 基）。
    /// 未固定或越界返回 false（不静默钳制——落点指示层自己负责画对）。
    pub fn reorder(&mut self, path: &str, to: usize) -> bool {
        let from = match self.pinned.iter().position(|p| p == path) {
            Some(i) => i,
            None => return false,
        };
        if to >= self.pinned.len() {
            return false;
        }
        let item = self.pinned.remove(from);
        self.pinned.insert(to, item);
        true
    }

    /// F072 最近使用流注入（去重——同路径保留最新一次）。
    pub fn feed_recent(&mut self, path: &str, kind: FeedKind, used_min: u64) {
        self.feed.retain(|(p, _, _)| p != path);
        self.feed.push((String::from(path), kind, used_min));
    }

    /// 侧栏渲染序列：固定区（原序）→ 推荐文件夹（最近优先、滤掉已固定）
    /// → 推荐文件（最近优先）。三类分区一次给全（共存显示逻辑）。
    pub fn render(&self) -> Vec<(String, FeedKind, bool)> {
        let mut out: Vec<(String, FeedKind, bool)> = self
            .pinned
            .iter()
            .map(|p| (p.clone(), FeedKind::Folder, true))
            .collect();
        let mut sorted: Vec<&(String, FeedKind, u64)> =
            self.feed.iter().collect();
        sorted.sort_by(|a, b| b.2.cmp(&a.2));
        let mut rec_f: usize = 0;
        let mut rec_d: usize = 0;
        for (p, k, _) in sorted {
            match k {
                FeedKind::Folder => {
                    if rec_f >= RECENT_FOLDERS || self.pinned.iter().any(|x| x == p) {
                        continue;
                    }
                    rec_f += 1;
                    out.push((p.clone(), FeedKind::Folder, false));
                }
                FeedKind::File => {
                    if rec_d >= RECENT_FILES {
                        continue;
                    }
                    rec_d += 1;
                    out.push((p.clone(), FeedKind::File, false));
                }
            }
        }
        out
    }

    /// 持久化快照（固定表 + 推荐流，重启后恢复）。
    pub fn snapshot(&self) -> (Vec<String>, Vec<(String, FeedKind, u64)>) {
        (self.pinned.clone(), self.feed.clone())
    }

    pub fn restore(&mut self, pinned: Vec<String>, feed: Vec<(String, FeedKind, u64)>) {
        self.pinned = pinned;
        self.feed = feed;
    }

    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }
}

pub fn run_quickpin_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F253");
    let mut qa = QuickAccess::new();
    // 固定/取消/重复固定幂等。
    set.add("F253 pin", qa.pin("C:\\报告") && !qa.pin("C:\\报告"), "pin idempotent");
    // 拖排序：三个固定项 0/1/2 → 把 0 拖到 2。
    let _ = qa.pin("C:\\图纸");
    let _ = qa.pin("C:\\素材");
    set.add("F253 reorder", qa.reorder("C:\\报告", 2) && qa.pinned()[2] == "C:\\报告", "drag sort");
    set.add("F253 reorder reject", !qa.reorder("C:\\无", 0) && !qa.reorder("C:\\图纸", 9), "honest fail");
    // 自动推荐：注入 8 文件夹 + 8 文件流，推荐只出 5+5。
    for i in 0..8 {
        qa.feed_recent(&alloc::format!("C:\\目录{}\\", i), FeedKind::Folder, 100 - i);
    }
    for i in 0..8 {
        qa.feed_recent(&alloc::format!("C:\\目录{}\\文件{}.txt", i, i), FeedKind::File, 200 - i);
    }
    let view = qa.render();
    let auto_f = view.iter().filter(|(_, k, p)| *k == FeedKind::Folder && !*p).count();
    let auto_d = view.iter().filter(|(_, k, p)| *k == FeedKind::File && !*p).count();
    set.add("F253 quota 5+5", auto_f == RECENT_FOLDERS && auto_d == RECENT_FILES, "5+5");
    // 共存显示：已固定的文件夹从推荐区隐藏。
    let _ = qa.pin("C:\\目录0\\");
    let view2 = qa.render();
    let dup = view2
        .iter()
        .filter(|(p, k, _)| *k == FeedKind::Folder && p == "C:\\目录0\\")
        .count();
    set.add("F253 coexist hide", dup == 1 && view2[3].0 == "C:\\目录0\\", "pinned wins");
    // 取消固定一步回退 + 持久化往返。
    set.add("F253 unpin", qa.unpin("C:\\目录0\\") && !qa.unpin("C:\\目录0\\"), "one step");
    let snap = qa.snapshot();
    let mut qa2 = QuickAccess::new();
    qa2.restore(snap.0, snap.1);
    set.add(
        "F253 persistence",
        qa2.pinned() == qa.pinned() && qa2.render() == qa.render(),
        "round-trip",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f253_pin_flow_green() {
        let set = run_quickpin_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F253 自检红 {f}/{p}");
    }

    #[test]
    fn recommendation_order_is_recency() {
        let mut qa = QuickAccess::new();
        qa.feed_recent("C:\\旧\\", FeedKind::Folder, 10);
        qa.feed_recent("C:\\新\\", FeedKind::Folder, 20);
        let auto: alloc::vec::Vec<(String, FeedKind, bool)> =
            qa.render().into_iter().filter(|(_, _, p)| !*p).collect();
        assert_eq!(auto[0].0, "C:\\新\\", "最近优先");
    }
}
