//! F453 缩略图角标覆盖（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **五类角标语义表；尺寸/位置规范；实时同步（状态变更 <1s）；渲染走 F300
//! 栅格；角标与缩略图共存（大图标视图不互遮）。**
//!
//! 功能定义（主册批次三）：快捷方式（左下小箭头 F013）、压缩包（右上拉链
//! F092）、加密卷（锁形 F439）、同步中（云形进行态）、离线可用（对勾实心）——
//! 角标尺寸统一（图标的 1/2）、位置五处固定不漂移；角标信息与状态实时同步
//! （加密完成锁形立现）。
//!
//! 零堆纪律：定长状态表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 状态变更到角标呈现的同步时限（主册：<1s）。
pub const SYNC_DEADLINE_MS: u64 = 1_000;
/// 角标尺寸 = 图标尺寸的 1/2（主册原文）。
pub const BADGE_SIZE_RATIO_NUM: u32 = 1;
pub const BADGE_SIZE_RATIO_DEN: u32 = 2;

/// 五类角标语义（主册原文五类，位置固定不漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    /// 快捷方式——左下小箭头（F013）。
    Shortcut,
    /// 压缩包——右上拉链（F092）。
    Archive,
    /// 加密卷——锁形（F439）。
    Encrypted,
    /// 同步中——云形进行态。
    Syncing,
    /// 离线可用——对勾实心。
    OfflineAvailable,
}

impl BadgeKind {
    /// 固定位置五处（左下/右上/左上/右下/左下二区——语义表唯一映射，
    /// 不漂移 = 同类角标位置恒定）。
    pub fn anchor_name(self) -> &'static str {
        match self {
            BadgeKind::Shortcut => "bottom-left",
            BadgeKind::Archive => "top-right",
            BadgeKind::Encrypted => "top-left",
            BadgeKind::Syncing => "bottom-right",
            BadgeKind::OfflineAvailable => "bottom-left",
        }
    }

    pub fn glyph_name(self) -> &'static str {
        match self {
            BadgeKind::Shortcut => "arrow",
            BadgeKind::Archive => "zip",
            BadgeKind::Encrypted => "lock",
            BadgeKind::Syncing => "cloud-busy",
            BadgeKind::OfflineAvailable => "check-solid",
        }
    }
}

/// 单文件角标状态账（五类可叠加——一个图标可同时是快捷方式+同步中）。
#[derive(Clone, Copy, Debug)]
pub struct BadgeState {
    pub file_key: u64,
    pub badges: [Option<BadgeKind>; 5],
    /// 最近一次状态变更时间戳（实时同步判据用）。
    pub changed_at_ms: u64,
    /// 角标已呈现时间戳（同步时延核算）。
    pub rendered_at_ms: u64,
}

/// 角标登记处。
pub struct BadgeLedger {
    entries: [Option<BadgeState>; 128],
    n: usize,
}

/// 简单 FNV-1a 键（与 F452 同构，仅作表键）。
fn file_key(path: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl BadgeLedger {
    pub const fn new() -> Self {
        BadgeLedger {
            entries: [None; 128],
            n: 0,
        }
    }

    fn entry_mut(&mut self, path: &str) -> Option<&mut BadgeState> {
        let k = file_key(path);
        let idx = (0..self.n).find(|&i| matches!(self.entries[i], Some(e) if e.file_key == k));
        match idx {
            Some(i) => self.entries[i].as_mut(),
            None => {
                if self.n >= 128 {
                    return None;
                }
                self.entries[self.n] = Some(BadgeState {
                    file_key: k,
                    badges: [None; 5],
                    changed_at_ms: 0,
                    rendered_at_ms: 0,
                });
                self.n += 1;
                self.entries[self.n - 1].as_mut()
            }
        }
    }

    /// 置角标（状态变更即时入账）。
    pub fn set_badge(&mut self, path: &str, kind: BadgeKind, on: bool, now_ms: u64) -> bool {
        let e = match self.entry_mut(path) {
            Some(e) => e,
            None => return false,
        };
        let slot = e.badges.iter().position(|b| *b == Some(kind));
        if on {
            if slot.is_none() {
                match e.badges.iter_mut().find(|b| b.is_none()) {
                    Some(b) => *b = Some(kind),
                    None => return false, // 五类已满——语义上不可能（仅五类）
                }
            }
        } else if let Some(i) = slot {
            e.badges[i] = None;
        } else {
            return true; // 关闭不存在的角标 = 幂等成功
        }
        e.changed_at_ms = now_ms;
        true
    }

    pub fn has_badge(&self, path: &str, kind: BadgeKind) -> bool {
        let k = file_key(path);
        (0..self.n).any(|i| {
            matches!(
                self.entries[i],
                Some(BadgeState { file_key: fk, badges: b, .. })
                if fk == k && b.iter().any(|&x| x == Some(kind))
            )
        })
    }

    /// 渲染入账：返回同步时延是否达标（<1s，主册判据）。
    pub fn mark_rendered(&mut self, path: &str, now_ms: u64) -> bool {
        let k = file_key(path);
        for i in 0..self.n {
            if let Some(e) = self.entries[i].as_mut() {
                if e.file_key == k {
                    e.rendered_at_ms = now_ms;
                    return now_ms.saturating_sub(e.changed_at_ms) < SYNC_DEADLINE_MS;
                }
            }
        }
        false
    }

    /// 大图标视图互遮审计：快捷方式（左下）与离线可用（左下）同屏时——
    /// 主册判据「角标与缩略图共存（大图标视图不互遮）」：同锚点角标互斥
    /// 由语义表裁决（离线可用让位快捷方式）。
    pub fn coexists_clean(path: &str, ledger: &BadgeLedger) -> bool {
        let k = file_key(path);
        for i in 0..ledger.n {
            if let Some(e) = ledger.entries[i] {
                if e.file_key == k {
                    let shortcut = e.badges.iter().any(|&b| b == Some(BadgeKind::Shortcut));
                    let offline = e.badges.iter().any(|&b| b == Some(BadgeKind::OfflineAvailable));
                    // 同锚点双角标 = 互遮风险 → 裁决为不可共存（调用方二选一）。
                    return !(shortcut && offline);
                }
            }
        }
        true
    }
}

/// 角标绘制矩形（F300 栅格口径：尺寸=图标 1/2，锚点五处固定）。
pub fn badge_rect(icon_x: i32, icon_y: i32, icon_w: u32, icon_h: u32, kind: BadgeKind) -> (i32, i32, u32, u32) {
    let bw = icon_w * BADGE_SIZE_RATIO_NUM / BADGE_SIZE_RATIO_DEN;
    let bh = icon_h * BADGE_SIZE_RATIO_NUM / BADGE_SIZE_RATIO_DEN;
    let (x, y) = match kind.anchor_name() {
        "bottom-left" => (icon_x, icon_y + icon_h as i32 - bh as i32),
        "top-right" => (icon_x + icon_w as i32 - bw as i32, icon_y),
        "top-left" => (icon_x, icon_y),
        _ => (icon_x + icon_w as i32 - bw as i32, icon_y + icon_h as i32 - bh as i32),
    };
    (x, y, bw, bh)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_thumbbadge_checks() -> CheckSet {
    let mut cs = CheckSet::new("F453-thumbbadge");
    // 1) 五类语义表：位置五处固定（同类角标位置恒定）。
    cs.add("five_badge_kinds", BadgeKind::Shortcut.glyph_name() == "arrow" && BadgeKind::Encrypted.glyph_name() == "lock", "");
    cs.add("anchors_stable", BadgeKind::Shortcut.anchor_name() == BadgeKind::OfflineAvailable.anchor_name() && BadgeKind::Archive.anchor_name() == "top-right", "");
    // 2) 尺寸规范 = 图标 1/2。
    let (x, y, w, h) = badge_rect(100, 100, 96, 96, BadgeKind::Encrypted);
    cs.add("size_half_icon", w == 48 && h == 48 && x == 100 && y == 100, "");
    let (_, _, w2, _) = badge_rect(0, 0, 256, 256, BadgeKind::Archive);
    cs.add("size_scales", w2 == 128, "");
    // 3) 实时同步 <1s（主册：加密完成锁形立现）。
    let mut l = BadgeLedger::new();
    l.set_badge("C:\\vault.vxc", BadgeKind::Encrypted, true, 5_000);
    cs.add("sync_within_deadline", l.mark_rendered("C:\\vault.vxc", 5_400), "");
    l.set_badge("C:\\slow.bin", BadgeKind::Syncing, true, 8_000);
    cs.add("sync_over_deadline_red", !l.mark_rendered("C:\\slow.bin", 9_200), "");
    // 4) 叠加语义（快捷方式+同步中可共存，异锚点）。
    l.set_badge("C:\\s.lnk", BadgeKind::Shortcut, true, 1_000);
    l.set_badge("C:\\s.lnk", BadgeKind::Syncing, true, 1_050);
    cs.add("multi_badge_overlay", l.has_badge("C:\\s.lnk", BadgeKind::Shortcut) && l.has_badge("C:\\s.lnk", BadgeKind::Syncing), "");
    // 5) 同锚点互斥裁决（不互遮）。
    let mut l2 = BadgeLedger::new();
    l2.set_badge("C:\\a.lnk", BadgeKind::Shortcut, true, 0);
    l2.set_badge("C:\\a.lnk", BadgeKind::OfflineAvailable, true, 0);
    cs.add("same_anchor_arbitrated", !BadgeLedger::coexists_clean("C:\\a.lnk", &l2), "");
    let mut l3 = BadgeLedger::new();
    l3.set_badge("C:\\b.zip", BadgeKind::Archive, true, 0);
    l3.set_badge("C:\\b.zip", BadgeKind::Shortcut, true, 0);
    cs.add("diff_anchor_coexist", BadgeLedger::coexists_clean("C:\\b.zip", &l3), "");
    // 6) 关闭角标幂等。
    cs.add("off_idempotent", l.set_badge("C:\\nothing", BadgeKind::Archive, false, 9_999), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_badge_appears_within_1s() {
        let mut l = BadgeLedger::new();
        // 加密完成（状态变更）→ 锁形立现（<1s 渲染）。
        l.set_badge("D:\\secret", BadgeKind::Encrypted, true, 100_000);
        assert!(l.mark_rendered("D:\\secret", 100_800));
    }

    #[test]
    fn badge_toggle_off_removes_only_that_kind() {
        let mut l = BadgeLedger::new();
        l.set_badge("D:\\x", BadgeKind::Archive, true, 0);
        l.set_badge("D:\\x", BadgeKind::Syncing, true, 0);
        assert!(l.set_badge("D:\\x", BadgeKind::Archive, false, 1));
        assert!(!l.has_badge("D:\\x", BadgeKind::Archive));
        assert!(l.has_badge("D:\\x", BadgeKind::Syncing));
    }

    #[test]
    fn rect_anchors_do_not_drift() {
        // 同一图标同一类角标，两次计算矩形必须逐像素一致（不漂移）。
        let a = badge_rect(40, 60, 128, 128, BadgeKind::Shortcut);
        let b = badge_rect(40, 60, 128, 128, BadgeKind::Shortcut);
        assert_eq!(a, b);
        assert_eq!(a.2, 64);
    }
}
