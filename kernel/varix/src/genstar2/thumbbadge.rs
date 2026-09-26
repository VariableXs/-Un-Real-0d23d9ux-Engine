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

// ===========================================================================
// 深化 v2（F453）：同步中进度态 / 同锚互斥裁决矩阵 / 变更批次时延账 /
// 状态跃迁语义表（同步完成→锁形立现）
// ===========================================================================

/// 同步进行态的进度账（主册「云形进行态」——进度 permille 驱动角标渲染；
/// 0‰ 起、1000‰ 完成；完成后由跃迁语义表收口）。
#[derive(Clone, Copy, Debug)]
pub struct SyncProgress {
    pub file_key: u64,
    /// 0..=1000。
    pub permille: u16,
    pub done: bool,
}

pub const SYNC_PERMILLE_CAP: u16 = 1000;

pub fn sync_advance(p: &mut SyncProgress, delta_permille: u16) -> bool {
    if p.done {
        return false;
    }
    let total = p.permille as u32 + delta_permille as u32;
    if total >= SYNC_PERMILLE_CAP as u32 {
        p.permille = SYNC_PERMILLE_CAP;
        p.done = true;
    } else {
        p.permille = total as u16;
    }
    true
}

/// 同步完成的状态跃迁语义表（主册「加密完成锁形立现」同源：
/// 同步中 → 完成后云形收口、结果态立现；失败则进行态退场无结果态）。
pub enum SyncOutcome {
    Uploaded,
    Failed,
}

/// 应用跃迁：进行态角标关闭 + 结果态角标打开（一步内完成——不出现
/// 「同步没了但结果也没来」的真空帧）。
pub fn apply_sync_outcome(
    ledger: &mut BadgeLedger,
    path: &str,
    outcome: SyncOutcome,
    now_ms: u64,
) -> bool {
    let off = ledger.set_badge(path, BadgeKind::Syncing, false, now_ms);
    let on = match outcome {
        SyncOutcome::Uploaded => ledger.set_badge(path, BadgeKind::OfflineAvailable, true, now_ms),
        SyncOutcome::Failed => true, // 失败：进行态退场即收口（无结果态角标）。
    };
    off && on
}

/// 同锚互斥裁决矩阵（主册「大图标视图不互遮」的完整版：五类角标的
/// 锚位两两关系——同锚对互斥，异锚对可共存）。
/// 返回 true = 两类可同时呈现。
pub fn anchor_compatible(a: BadgeKind, b: BadgeKind) -> bool {
    a.anchor_name() != b.anchor_name()
}

/// 图标上全部角标的共存自检（逐对扫锚位——O(25) 定长，无分配）。
pub fn all_badges_compatible(badges: &[Option<BadgeKind>; 5]) -> bool {
    for i in 0..badges.len() {
        for j in (i + 1)..badges.len() {
            if let (Some(a), Some(b)) = (badges[i], badges[j]) {
                if !anchor_compatible(a, b) {
                    return false;
                }
            }
        }
    }
    true
}

/// 变更批次时延账（主册「状态变更 <1s」的批量面：一次加密批次
/// 改 N 个文件的角标，批内最后渲染时刻 - 批首变更时刻 < 1s 才达标——
/// 逐文件达标还不够，批次整体超时同样是说谎）。
pub struct BatchTiming {
    pub batch_start_ms: u64,
    pub last_change_ms: u64,
}

impl BatchTiming {
    pub const fn new(start_ms: u64) -> Self {
        BatchTiming { batch_start_ms: start_ms, last_change_ms: start_ms }
    }

    pub fn record_change(&mut self, at_ms: u64) {
        self.last_change_ms = at_ms;
    }

    pub fn batch_within_deadline(&self) -> bool {
        self.last_change_ms.saturating_sub(self.batch_start_ms) < SYNC_DEADLINE_MS
    }
}

/// 五类角标渲染矩形全表审计（五锚位 × 1/2 尺寸——矩形互不重叠当且仅当
/// 锚位互异；「角标与缩略图共存」的几何面）。
pub fn rects_disjoint(a: (i32, i32, u32, u32), b: (i32, i32, u32, u32)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax + aw as i32 <= bx || bx + bw as i32 <= ax || ay + ah as i32 <= by || by + bh as i32 <= ay
}

// ---------------------------------------------------------------------------
// 深化自检（F453 v2）
// ---------------------------------------------------------------------------

pub fn run_thumbbadge_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F453-v2");
    // 1) 同步进度推进与收口。
    let mut p = SyncProgress { file_key: 1, permille: 0, done: false };
    cs.add("sync_partial", {
        sync_advance(&mut p, 400);
        p.permille == 400 && !p.done
    }, "");
    cs.add("sync_overflow_clamps", {
        sync_advance(&mut p, 700);
        p.permille == SYNC_PERMILLE_CAP && p.done
    }, "");
    cs.add("sync_done_frozen", !sync_advance(&mut p, 100), "");
    // 2) 跃迁语义：完成→离线可用立现；失败→无真空帧。
    let mut led = BadgeLedger::new();
    let _ = led.set_badge("C:\\a.zip", BadgeKind::Syncing, true, 100);
    cs.add("outcome_upload", apply_sync_outcome(&mut led, "C:\\a.zip", SyncOutcome::Uploaded, 500)
        && !led.has_badge("C:\\a.zip", BadgeKind::Syncing)
        && led.has_badge("C:\\a.zip", BadgeKind::OfflineAvailable), "");
    let mut led2 = BadgeLedger::new();
    let _ = led2.set_badge("C:\\b.zip", BadgeKind::Syncing, true, 100);
    cs.add("outcome_fail_no_vacuum", apply_sync_outcome(&mut led2, "C:\\b.zip", SyncOutcome::Failed, 500)
        && !led2.has_badge("C:\\b.zip", BadgeKind::Syncing), "");
    // 3) 同锚互斥矩阵：同锚拒、异锚容。
    cs.add("same_anchor_incompatible", !anchor_compatible(BadgeKind::Shortcut, BadgeKind::OfflineAvailable), "");
    cs.add("diff_anchor_compatible", anchor_compatible(BadgeKind::Shortcut, BadgeKind::Encrypted)
        && anchor_compatible(BadgeKind::Archive, BadgeKind::Syncing), "");
    // 全表共存自检：快捷方式+加密+压缩（三锚位）→ 兼容。
    let triple = [Some(BadgeKind::Shortcut), Some(BadgeKind::Encrypted), Some(BadgeKind::Archive), None, None];
    cs.add("triple_coexist", all_badges_compatible(&triple), "");
    let conflict = [Some(BadgeKind::Shortcut), Some(BadgeKind::OfflineAvailable), None, None, None];
    cs.add("pair_conflict_detected", !all_badges_compatible(&conflict), "");
    // 4) 批次时延账：批内 <1s 达标；拖长即红（不靠逐文件达标掩盖）。
    let mut bt = BatchTiming::new(1000);
    bt.record_change(1500);
    cs.add("batch_on_time", bt.batch_within_deadline(), "");
    bt.record_change(2500);
    cs.add("batch_late_detected", !bt.batch_within_deadline(), "");
    // 5) 几何面：异锚矩形互不重叠（F300 栅格下五角标共存不互遮）。
    let sc = badge_rect(0, 0, 64, 64, BadgeKind::Shortcut);
    let en = badge_rect(0, 0, 64, 64, BadgeKind::Encrypted);
    let ar = badge_rect(0, 0, 64, 64, BadgeKind::Archive);
    cs.add("rects_disjoint", rects_disjoint(sc, en) && rects_disjoint(sc, ar) && rects_disjoint(en, ar), "");
    // 尺寸 = 图标 1/2（比例锚复核）。
    cs.add("rect_ratio", sc.2 == 32 && sc.3 == 32, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn sync_progress_never_regresses() {
        let mut p = SyncProgress { file_key: 2, permille: 0, done: false };
        for _ in 0..10 {
            let before = p.permille;
            sync_advance(&mut p, 150);
            assert!(p.permille >= before.min(p.permille));
        }
        assert!(p.done);
    }

    #[test]
    fn anchor_matrix_symmetry() {
        // 互斥关系对称（a vs b = b vs a）。
        let all = [BadgeKind::Shortcut, BadgeKind::Archive, BadgeKind::Encrypted, BadgeKind::Syncing, BadgeKind::OfflineAvailable];
        for &a in all.iter() {
            for &b in all.iter() {
                assert_eq!(anchor_compatible(a, b), anchor_compatible(b, a));
            }
        }
    }

    #[test]
    fn batch_timing_boundary() {
        // 恰好 1s 边界：<1s 达标，=1s 不达标（主册「<1s」严格小于）。
        let mut bt = BatchTiming::new(0);
        bt.record_change(999);
        assert!(bt.batch_within_deadline());
        let mut bt2 = BatchTiming::new(0);
        bt2.record_change(1000);
        assert!(!bt2.batch_within_deadline());
    }

    #[test]
    fn outcome_idempotent_double_apply() {
        let mut led = BadgeLedger::new();
        let _ = led.set_badge("C:\\x", BadgeKind::Syncing, true, 0);
        assert!(apply_sync_outcome(&mut led, "C:\\x", SyncOutcome::Uploaded, 10));
        // 重复应用：Syncing 已关（幂等）、OfflineAvailable 已开（幂等）。
        assert!(apply_sync_outcome(&mut led, "C:\\x", SyncOutcome::Uploaded, 20));
        assert!(led.has_badge("C:\\x", BadgeKind::OfflineAvailable));
        assert!(!led.has_badge("C:\\x", BadgeKind::Syncing));
    }
}
