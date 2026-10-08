//! F427 Ctrl+X/C/V 语义表 · 完整设计（STAR I 主册 G-I-27）。
//!
//! **判据（主册）**：三域×三键矩阵用例；文件剪切延迟执行与反悔（剪切
//! 后源文件还在直到粘贴）；粘贴冲突走 F087；禁用静默判据。＋通12。
//!
//! 设计：剪贴板三键核——文件/文本/图片三域 × X/C/V 矩阵；文件剪切 =
//! 暂存移动意图（源文件保留，粘贴时才动——可反悔）；粘贴冲突钩子
//! （F087 面板）；禁用场景（只读）快捷键静默无效（不弹错误）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 剪贴板三域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipDomain {
    File,
    Text,
    Image,
}

/// 剪贴板载荷。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipPayload {
    pub domain: ClipDomain,
    /// 文件域：id 清单（剪切 = 移动意图）；文本域：长度；图片域：字节数。
    pub file_ids: Vec<u64>,
    pub size: u64,
    /// cut 意图标记（文件域剪切；粘贴后清除）。
    pub is_cut: bool,
}

/// 剪贴板历史容量（ring——最近 8 笔，粘贴回找）。
pub const HISTORY_CAP: usize = 8;

/// 剪贴板核。
pub struct ClipKeys {
    pub payload: Option<ClipPayload>,
    /// 源文件账（剪切暂存——粘贴成功才清）。
    pub cut_stash: Vec<u64>,
    pub paste_conflicts: u64,
    pub disabled_silent: u64,
    /// 剪贴板历史 ring（v6：最近 HISTORY_CAP 笔，F060 历史面板同源）。
    pub history: Vec<ClipPayload>,
    /// 冲突裁决账（v6：F087 面板裁决后替换/跳过分记）。
    pub conflicts_resolved: u64,
    pub conflicts_skipped: u64,
}

impl ClipKeys {
    pub fn new() -> ClipKeys {
        ClipKeys {
            payload: None,
            cut_stash: Vec::new(),
            paste_conflicts: 0,
            disabled_silent: 0,
            history: Vec::new(),
            conflicts_resolved: 0,
            conflicts_skipped: 0,
        }
    }

    fn push_history(&mut self, p: ClipPayload) {
        self.history.push(p);
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
    }

    /// Ctrl+C 复制（三域通用；清除剪切意图）。
    pub fn copy(&mut self, p: ClipPayload) {
        let mut p = p;
        p.is_cut = false;
        self.cut_stash.clear();
        self.push_history(p.clone());
        self.payload = Some(p);
    }

    /// Ctrl+X 剪切：文件域 = 暂存移动意图（源文件还在）；文本/图片同传统。
    pub fn cut(&mut self, p: ClipPayload) -> bool {
        if p.domain == ClipDomain::File {
            if p.file_ids.is_empty() {
                return false;
            }
            self.cut_stash = p.file_ids.clone();
            let mut p = p;
            p.is_cut = true;
            self.push_history(p.clone());
            self.payload = Some(p);
            true
        } else {
            // 文本/图片域：剪切同传统语义（立即生效——copy 即可）。
            self.copy(p);
            true
        }
    }

    /// Ctrl+V 粘贴：剪切意图在此刻才执行（目标占用 → F087 冲突面板钩子）。
    /// 返回：Ok(执行动作数)；Err(冲突数)。
    pub fn paste(&mut self, occupied: &[u64]) -> Result<usize, usize> {
        let Some(p) = self.payload.take() else { return Err(0) };
        match p.domain {
            ClipDomain::File => {
                let conflicts = p.file_ids.iter().filter(|id| occupied.contains(id)).count();
                if conflicts > 0 {
                    self.paste_conflicts += conflicts as u64;
                    // 冲突 → 载荷退回（等 F087 面板裁决后重试）。
                    self.payload = Some(p);
                    return Err(conflicts);
                }
                if p.is_cut {
                    // 延迟执行此刻发生：源暂存清空。
                    self.cut_stash.clear();
                }
                Ok(p.file_ids.len())
            }
            _ => Ok(1),
        }
    }

    /// 冲突裁决粘贴（v6）：F087 面板逐项裁决（replace=true 替换 /
    /// false 跳过）——裁决后执行移动，跳过项不静默消失（账分记）。
    /// 移动/跳过总数 != 载荷数即暴露（调用方对账）。
    pub fn paste_with_verdicts(&mut self, verdicts: &[(u64, bool)]) -> (usize, usize) {
        let Some(p) = self.payload.take() else { return (0, 0) };
        if p.domain != ClipDomain::File {
            self.payload = Some(p);
            return (0, 0);
        }
        let mut moved = 0usize;
        let mut skipped = 0usize;
        for id in &p.file_ids {
            match verdicts.iter().find(|(i, _)| i == id) {
                Some((_, true)) => {
                    moved += 1;
                    self.conflicts_resolved += 1;
                }
                Some((_, false)) => {
                    skipped += 1;
                    self.conflicts_skipped += 1;
                }
                None => {
                    skipped += 1;
                    self.conflicts_skipped += 1;
                }
            }
        }
        // 裁决完成即移动完成（跳过项保留原位——「不动」也是明确结局）。
        if p.is_cut {
            self.cut_stash.clear();
        }
        (moved, skipped)
    }

    /// 剪切反悔：粘贴前重新复制 → 意图清除、源文件从未动过（结构性：
    /// cut_stash 清空即反悔成立）。
    pub fn undo_cut(&mut self) -> bool {
        if self.cut_stash.is_empty() {
            return false;
        }
        self.cut_stash.clear();
        if let Some(p) = self.payload.as_mut() {
            p.is_cut = false;
        }
        true
    }

    /// 禁用场景（只读）：快捷键静默无效（无错误、无载荷变化）。
    pub fn disabled_press(&mut self) {
        self.disabled_silent += 1;
    }

    /// 源文件仍在判据：剪切后、粘贴前，暂存非空。
    pub fn source_intact(&self) -> bool {
        !self.cut_stash.is_empty()
    }
}

pub fn run_clipkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F427");
    let mut k = ClipKeys::new();
    // 三域×三键矩阵：文件域。
    let f = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![1, 2], size: 0, is_cut: false };
    k.copy(f.clone());
    set.add("f427-file-copy", k.payload.as_ref().map(|p| !p.is_cut).unwrap_or(false), "");
    set.add(
        "f427-file-cut-stash",
        k.cut(f.clone()) && k.source_intact() && k.payload.as_ref().map(|p| p.is_cut).unwrap_or(false),
        "",
    );
    // 剪切后源文件还在（直到粘贴）。
    set.add("f427-source-intact-before-paste", k.cut_stash == alloc::vec![1, 2], "");
    // 反悔：重新复制清意图。
    set.add("f427-undo-cut", k.undo_cut() && !k.source_intact(), "");
    // 粘贴执行延迟语义：剪切 → 粘贴到空目标 → 暂存清空（此刻才动文件）。
    let _ = k.cut(f.clone());
    set.add(
        "f427-paste-executes-now",
        k.paste(&[]) == Ok(2) && !k.source_intact(),
        "",
    );
    // 粘贴冲突走 F087（计数 + 载荷退回）。
    let _ = k.cut(f.clone());
    set.add(
        "f427-paste-conflict-f087",
        k.paste(&[1]) == Err(1) && k.paste_conflicts == 1 && k.payload.is_some(),
        "",
    );
    // 文本/图片域。
    let t = ClipPayload { domain: ClipDomain::Text, file_ids: alloc::vec![], size: 42, is_cut: false };
    set.add("f427-text-copy", { k.copy(t.clone()); k.payload.as_ref().map(|p| p.domain == ClipDomain::Text && p.size == 42).unwrap_or(false) }, "");
    set.add(
        "f427-text-cut-paste",
        { k.cut(t.clone()); k.paste(&[]) == Ok(1) },
        "",
    );
    let img = ClipPayload { domain: ClipDomain::Image, file_ids: alloc::vec![], size: 9_600, is_cut: false };
    set.add(
        "f427-image-copy",
        { k.copy(img); k.payload.as_ref().map(|p| p.domain == ClipDomain::Image).unwrap_or(false) },
        "",
    );
    // 禁用静默。
    k.disabled_press();
    set.add("f427-disabled-silent", k.disabled_silent == 1 && k.payload.is_some(), "");
    // 空剪切拒绝。
    let empty = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![], size: 0, is_cut: false };
    set.add("f427-empty-cut-rejected", !k.cut(empty), "");
    // 剪贴板历史 ring（v6）：每笔入账、容量淘汰最老、粘贴不消历史。
    let mut h = ClipKeys::new();
    for i in 0..10u64 {
        let p = ClipPayload { domain: ClipDomain::Text, file_ids: alloc::vec![], size: i, is_cut: false };
        h.copy(p);
    }
    set.add(
        "f427-history-cap-evict",
        h.history.len() == HISTORY_CAP
            && h.history.first().map(|p| p.size).unwrap_or(0) == 2
            && h.history.last().map(|p| p.size).unwrap_or(0) == 9,
        "",
    );
    let _ = h.paste(&[]);
    set.add(
        "f427-history-survives-paste",
        h.history.len() == HISTORY_CAP && h.payload.is_none(),
        "",
    );
    // 复制清剪切意图（反悔的第二条路：不必专门 undo）。
    let mut c2 = ClipKeys::new();
    let f2 = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![5], size: 0, is_cut: false };
    let _ = c2.cut(f2.clone());
    c2.copy(f2);
    set.add("f427-copy-cancels-cut", !c2.source_intact() && c2.payload.as_ref().map(|p| !p.is_cut).unwrap_or(false), "");
    // 冲突裁决粘贴（v6）：F087 逐项裁决——替换/跳过/缺席分记。
    let mut v = ClipKeys::new();
    let fv = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![1, 2, 3], size: 0, is_cut: true };
    let _ = v.cut(fv);
    let (moved, skipped) = v.paste_with_verdicts(&[(1, true), (2, false)]);
    set.add(
        "f427-verdict-partial",
        moved == 1 && skipped == 2 && v.conflicts_resolved == 1 && v.conflicts_skipped == 2,
        "",
    );
    // 裁决完成即移动完成：cut_stash 清、载荷消（跳过项留原位是明确结局）。
    set.add("f427-verdict-completes-cut", !v.source_intact() && v.payload.is_none(), "");
    // 全跳过裁决：移动 0 但仍是完成（不挂起）。
    let mut w = ClipKeys::new();
    let fw = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![7], size: 0, is_cut: true };
    let _ = w.cut(fw);
    let (m2, s2) = w.paste_with_verdicts(&[(7, false)]);
    set.add(
        "f427-verdict-skip-all",
        m2 == 0 && s2 == 1 && !w.source_intact() && w.conflicts_skipped == 1,
        "",
    );
    // 文本域裁决粘贴：不适用（返回 0/0、载荷保留）。
    let mut t2 = ClipKeys::new();
    let tv = ClipPayload { domain: ClipDomain::Text, file_ids: alloc::vec![], size: 5, is_cut: false };
    t2.copy(tv);
    set.add(
        "f427-verdict-text-not-applicable",
        t2.paste_with_verdicts(&[(1, true)]) == (0, 0) && t2.payload.is_some(),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_conflict_counts_all() {
        let mut k = ClipKeys::new();
        let f = ClipPayload { domain: ClipDomain::File, file_ids: alloc::vec![1, 2, 3], size: 0, is_cut: false };
        let _ = k.cut(f);
        assert_eq!(k.paste(&[1, 3]), Err(2));
        assert_eq!(k.paste_conflicts, 2);
        assert!(k.payload.is_some(), "载荷退回待 F087 裁决");
    }

    #[test]
    fn paste_without_payload_errs_zero() {
        let mut k = ClipKeys::new();
        assert_eq!(k.paste(&[]), Err(0));
    }
}
