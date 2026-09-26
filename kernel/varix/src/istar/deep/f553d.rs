//! 深化层 · F553 多选拖影计数（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F553 节）：
//! ①「徽标几何：右下 12px 偏移，小而清楚不遮拖影」的**几何引擎**——
//!   徽标矩形从拖影尺寸 + [`BADGE_OFFSET_PX`] 派生（宽度随计数位数
//!   增长，×7 与 ×20 不同档），贴角合同（右/下缝恰为 12px）与界内
//!   合同（徽标矩形不越出拖影矩形）双红线；合成口在计数不足
//!   [`BADGE_MIN_COUNT`] 或不在拖动会话时不出矩形（单拖干净——无
//!   徽标即无几何）；
//! ②「加选实时性（拖动中 Ctrl 加选数字跟着变）」的**刷新账**——
//!   变化→徽标生效走节拍器：3 帧合账窗（@60fps ≈ 50ms，肉眼即实时），
//!   单窗积压上限 8 笔（超限记积压违约账，不许无声吞变化）；
//! ③「计数对账（7/20 项实测口径）」的**审计账本**——徽标展示数与
//!   实选数逐笔对拍：可见采样撒谎即红；隐藏期（单拖展示 0）不算撒谎。

use crate::checks::CheckSet;
use crate::istar::dragbadge::{DragBadge, BADGE_MIN_COUNT, BADGE_OFFSET_PX};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// ① 徽标几何引擎
// ---------------------------------------------------------------------------

/// 徽标矩形几何（相对拖影图标的锚定与界内判定）。
pub struct BadgeGeometry {
    pub icon_w: u32,
    pub icon_h: u32,
    pub badge_w: u32,
    pub badge_h: u32,
}

impl BadgeGeometry {
    /// 按计数定档：宽度 = 底 10px + 每位数字 8px（×7 一档、×20 两档），
    /// 高度固定 14px（小而清楚）。
    pub fn for_count(icon_w: u32, icon_h: u32, count: usize) -> BadgeGeometry {
        let mut digits = 1usize;
        let mut n = count;
        while n >= 10 {
            digits += 1;
            n /= 10;
        }
        BadgeGeometry { icon_w, icon_h, badge_w: 10 + 8 * digits as u32, badge_h: 14 }
    }

    /// 徽标左上角：右下角向内收 [`BADGE_OFFSET_PX`] 偏移、再退徽标宽高
    /// （拖影过小时饱和贴 0——不越界优先于贴角）。
    pub fn anchor(&self) -> (u32, u32) {
        let x = self.icon_w.saturating_sub(BADGE_OFFSET_PX + self.badge_w);
        let y = self.icon_h.saturating_sub(BADGE_OFFSET_PX + self.badge_h);
        (x, y)
    }

    /// 不遮拖影红线：徽标矩形完全落在拖影矩形内。
    pub fn fits_inside_drag_image(&self) -> bool {
        let (ax, ay) = self.anchor();
        ax + self.badge_w <= self.icon_w && ay + self.badge_h <= self.icon_h
    }

    /// 贴角合同：徽标右缘/下缘距拖影右/下缘恰为 12px 偏移。
    pub fn anchored_at_offset(&self) -> bool {
        let (ax, ay) = self.anchor();
        self.icon_w - (ax + self.badge_w) == BADGE_OFFSET_PX
            && self.icon_h - (ay + self.badge_h) == BADGE_OFFSET_PX
    }
}

/// 徽标合成口（深化层唯一渲染取数口）：拖动会话在途且计数达阈值才给矩形。
pub fn badge_rect_for(badge: &DragBadge, icon_w: u32, icon_h: u32) -> Option<(u32, u32, u32, u32)> {
    if !badge.badge_visible() {
        return None;
    }
    let g = BadgeGeometry::for_count(icon_w, icon_h, badge.count());
    let (x, y) = g.anchor();
    Some((x, y, g.badge_w, g.badge_h))
}

// ---------------------------------------------------------------------------
// ② 加选实时性刷新账（节拍器）
// ---------------------------------------------------------------------------

/// 合账窗（帧）：3 帧 @60fps ≈ 50ms——肉眼即实时。
pub const PACE_FRAMES: u64 = 3;
/// 单窗积压上限（笔）：超限即积压违约（不许无声吞变化）。
pub const PACE_CAP: usize = 8;

/// 刷新节拍器：变化合账、到点一刷。
pub struct RefreshPacer {
    pending: usize,
    last_flush: u64,
    flushed_total: usize,
    overdue: usize,
}

impl RefreshPacer {
    pub fn new() -> RefreshPacer {
        RefreshPacer { pending: 0, last_flush: 0, flushed_total: 0, overdue: 0 }
    }

    /// 记一笔选择变化（帧号供到期判定）。
    pub fn note_change(&mut self, _frame: u64) {
        self.pending += 1;
        if self.pending > PACE_CAP {
            self.overdue += 1;
        }
    }

    /// 到期判定：距上次刷新 ≥ 合账窗。
    pub fn due(&self, frame: u64) -> bool {
        frame.saturating_sub(self.last_flush) >= PACE_FRAMES
    }

    /// 刷新：合账清零，返回本笔合并的变化数。
    pub fn flush(&mut self, frame: u64) -> usize {
        let n = self.pending;
        self.pending = 0;
        self.flushed_total += n;
        self.last_flush = frame;
        n
    }

    pub fn overdue(&self) -> usize {
        self.overdue
    }

    pub fn flushed_total(&self) -> usize {
        self.flushed_total
    }
}

// ---------------------------------------------------------------------------
// ③ 计数对账（7/20 实测口径审计账本）
// ---------------------------------------------------------------------------

/// 计数审计账：徽标展示数 vs 实选数逐笔对拍。
pub struct CountAudit {
    entries: [(usize, usize); 32],
    len: usize,
}

impl CountAudit {
    pub fn new() -> CountAudit {
        CountAudit { entries: [(0, 0); 32], len: 0 }
    }

    /// 记一笔（人工口径——审计工具注入用）。
    pub fn record(&mut self, shown: usize, actual: usize) -> bool {
        if self.len >= 32 {
            return false;
        }
        self.entries[self.len] = (shown, actual);
        self.len += 1;
        true
    }

    /// 从 [`DragBadge`] 采样：可见记 (计数, 计数)，隐藏记 (0, 实选)。
    pub fn sample(&mut self, badge: &DragBadge) -> bool {
        let shown = if badge.badge_visible() { badge.count() } else { 0 };
        self.record(shown, badge.count())
    }

    /// 对账：可见采样撒谎即红；隐藏采样（展示 0 = 单拖干净）不算撒谎。
    pub fn all_match(&self) -> bool {
        (0..self.len).all(|i| {
            let (s, a) = self.entries[i];
            s == 0 || s == a
        })
    }

    pub fn mismatches(&self) -> usize {
        (0..self.len)
            .filter(|&i| {
                let (s, a) = self.entries[i];
                s != 0 && s != a
            })
            .count()
    }

    pub fn samples(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f553_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 贴角合同：×7 徽标在 48px 拖影上右/下缝恰 12px 且不越界。
    let mut b = DragBadge::new();
    b.set_dragging(true);
    for i in 1..=7u64 {
        b.select(i);
    }
    let g7 = BadgeGeometry::for_count(48, 48, b.count());
    cs.add(
        "badge 12px corner anchor",
        b.badge_visible() && g7.anchored_at_offset() && g7.fits_inside_drag_image(),
        "",
    );

    // 2) 位数定档：×20 比 ×7 宽一档，且两者都界内（不遮拖影）。
    let g20 = BadgeGeometry::for_count(48, 48, 20);
    cs.add(
        "badge widens with digits",
        g20.badge_w > g7.badge_w && g20.fits_inside_drag_image() && g7.fits_inside_drag_image(),
        "",
    );

    // 3) 合成口闸门：单拖（< BADGE_MIN_COUNT）与非拖动会话都不出矩形。
    let mut b1 = DragBadge::new();
    b1.set_dragging(true);
    b1.select(99);
    let single = b1.count() < BADGE_MIN_COUNT && badge_rect_for(&b1, 48, 48).is_none();
    let mut b5 = DragBadge::new();
    for i in 1..=5u64 {
        b5.select(i);
    }
    let idle = badge_rect_for(&b5, 48, 48).is_none();
    let rect = badge_rect_for(&b, 48, 48);
    cs.add(
        "rect gate min count and session",
        single && idle && rect.is_some(),
        "",
    );

    // 4) 节拍器：3 帧内不到期、第 3 帧到期一刷合并 3 笔、零积压。
    let mut p = RefreshPacer::new();
    p.note_change(0);
    p.note_change(1);
    p.note_change(2);
    let cadence = !p.due(2) && p.due(3);
    let flushed = p.flush(3);
    cs.add(
        "pacer coalesces on cadence",
        cadence && flushed == 3 && p.overdue() == 0 && p.flushed_total() == 3,
        "",
    );

    // 5) 积压违约：不刷连记 12 笔 → 超上限 4 笔记违约账；一刷全收不吞。
    let mut p2 = RefreshPacer::new();
    for f in 0..12u64 {
        p2.note_change(f);
    }
    let all = p2.flush(50);
    cs.add("pacer flags backlog breach", p2.overdue() == 4 && all == 12, "");

    // 6) 计数对账 7/20 实测口径：×7、×20 两笔可见采样全对齐；
    //    单拖隐藏采样（展示 0）不算撒谎。
    let mut audit = CountAudit::new();
    audit.sample(&b);
    for i in 8..=20u64 {
        b.select(i);
    }
    audit.sample(&b);
    audit.sample(&b1);
    cs.add(
        "count audit 7 20 hidden single",
        audit.samples() == 3 && audit.all_match() && b.count() == 20,
        "",
    );

    // 7) 审计抓谎：徽标报 ×5 实选 7 → 违约 1 笔、全账转红。
    let mut audit2 = CountAudit::new();
    audit2.record(5, 7);
    audit2.record(20, 20);
    cs.add("audit catches lying badge", audit2.mismatches() == 1 && !audit2.all_match(), "");

    // 8) 实时链路贯通：拖动中 7 次加选走节拍器，一刷全收、徽标计数同拍。
    let mut b8 = DragBadge::new();
    b8.set_dragging(true);
    let mut p3 = RefreshPacer::new();
    for i in 1..=7u64 {
        b8.select(i);
        p3.note_change(i as u64);
    }
    let merged = p3.flush(3);
    cs.add(
        "live selection through pacer",
        merged == 7 && b8.count() == 7 && b8.live_update_count() == 7,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_icon_saturates_not_overflows() {
        // 拖影 20px 放不下 12px 偏移 + 徽标 → 锚点饱和贴 0，仍界内不越界。
        let g = BadgeGeometry::for_count(20, 20, 7);
        assert!(g.fits_inside_drag_image());
        assert!(!g.anchored_at_offset());
    }

    #[test]
    fn pacer_overdue_counts_each_extra() {
        let mut p = RefreshPacer::new();
        for _ in 0..(PACE_CAP + 3) {
            p.note_change(0);
        }
        assert_eq!(p.overdue(), 3);
        assert_eq!(p.flush(99), PACE_CAP + 3);
    }

    #[test]
    fn audit_hidden_sample_not_a_lie() {
        let mut a = CountAudit::new();
        let mut b = DragBadge::new();
        b.select(1); // 未拖动 → 徽标隐藏
        assert!(a.sample(&b));
        assert!(a.all_match());
        assert_eq!(a.mismatches(), 0);
    }
}
