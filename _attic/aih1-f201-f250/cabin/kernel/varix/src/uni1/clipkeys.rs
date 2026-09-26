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

/// 剪贴板核。
pub struct ClipKeys {
    pub payload: Option<ClipPayload>,
    /// 源文件账（剪切暂存——粘贴成功才清）。
    pub cut_stash: Vec<u64>,
    pub paste_conflicts: u64,
    pub disabled_silent: u64,
}

impl ClipKeys {
    pub fn new() -> ClipKeys {
        ClipKeys { payload: None, cut_stash: Vec::new(), paste_conflicts: 0, disabled_silent: 0 }
    }

    /// Ctrl+C 复制（三域通用；清除剪切意图）。
    pub fn copy(&mut self, p: ClipPayload) {
        let mut p = p;
        p.is_cut = false;
        self.cut_stash.clear();
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
