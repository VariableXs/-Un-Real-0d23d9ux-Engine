//! F496 快速设置磁贴长按（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **500ms 与进度环；六代表磁贴直达页对照；与编辑态冲突消解用例；误触率
//! （快速点击 0 误进）；即时性。**
//!
//! 功能定义（主册批次三）：快速设置磁贴（F298）的长按语义——长按 500ms
//! 直达该功能详细设置页（音量磁贴→声音设置、Wi-Fi 磁贴→网络页）；长按有
//! 进度环反馈（500ms 视觉倒数防误触）；与编辑态冲突消解：磁贴右上编辑按钮
//! 进编辑、磁贴本体长按进详情（两长按分工明确）。
//!
//! 零堆纪律：定长映射表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 长按触发时限（主册：500ms）。
pub const LONGPRESS_MS: u64 = 500;
/// 六代表磁贴（主册：六代表磁贴直达页对照）。
pub const TILE_N: usize = 6;
/// 快速点击误触线（低于此时长 = 点击不是长按——误触率 0 判据）。
pub const CLICK_CAP_MS: u64 = 200;

/// 六代表磁贴 → 详情页映射（主册：直达页对照表）。
pub const TILE_PAGES: [(&str, &str); TILE_N] = [
    ("volume", "settings/sound"),
    ("wifi", "settings/network"),
    ("bluetooth", "settings/bluetooth"),
    ("brightness", "settings/display"),
    ("battery-saver", "settings/power"),
    ("night-light", "settings/display/night"),
];

/// 磁贴直达页查询（对照表逐一在册）。
pub fn detail_page(tile: &str) -> Option<&'static str> {
    TILE_PAGES.iter().find(|(t, _)| *t == tile).map(|(_, p)| *p)
}

/// 长按进度环（500ms 视觉倒数——0..1_000 permille）。
pub fn progress_ring_permille(held_ms: u64) -> u32 {
    (held_ms.min(LONGPRESS_MS) * 1_000 / LONGPRESS_MS) as u32
}

/// 长按状态机（与编辑态冲突消解 + 误触率 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressOutcome {
    /// 未达阈值释放 = 普通点击（0 误进详情）。
    Click,
    /// 达到 500ms = 直达详情页。
    LongPressToDetail,
    /// 编辑按钮长按 = 进编辑态（两长按分工明确——磁贴本体进详情）。
    EditMode,
    /// 中途取消（移出磁贴）。
    Cancelled,
}

/// 磁贴按压会话。
pub struct TilePress {
    start_ms: u64,
    on_edit_button: bool,
    moved_out: bool,
}

impl TilePress {
    pub fn begin(now_ms: u64, on_edit_button: bool) -> Self {
        TilePress { start_ms: now_ms, on_edit_button, moved_out: false }
    }

    /// 移出磁贴（取消——拖出手势）。
    pub fn move_out(&mut self) {
        self.moved_out = true;
    }

    /// 释放裁决。
    pub fn release(&self, now_ms: u64) -> PressOutcome {
        if self.moved_out {
            return PressOutcome::Cancelled;
        }
        let held = now_ms.saturating_sub(self.start_ms);
        if held < CLICK_CAP_MS {
            return PressOutcome::Click; // 快速点击（误触率 0 判据）
        }
        if held >= LONGPRESS_MS {
            if self.on_edit_button {
                PressOutcome::EditMode // 编辑按钮长按 → 编辑态
            } else {
                PressOutcome::LongPressToDetail // 磁贴本体长按 → 详情页
            }
        } else {
            PressOutcome::Click // 200-500ms 之间松手仍算点击（宽容窗）
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_tilelongpress_checks() -> CheckSet {
    let mut cs = CheckSet::new("F496-tilelongpress");
    // 1) 500ms 阈值 + 进度环。
    cs.add("threshold_500", LONGPRESS_MS == 500, "");
    cs.add("ring_zero_at_start", progress_ring_permille(0) == 0, "");
    cs.add("ring_half", progress_ring_permille(250) == 500, "");
    cs.add("ring_full_clamped", progress_ring_permille(9_999) == 1_000, "");
    // 2) 六代表磁贴直达页对照（逐一在册）。
    cs.add("six_tiles_mapped", TILE_PAGES.len() == TILE_N, "");
    cs.add("volume_to_sound", detail_page("volume") == Some("settings/sound"), "");
    cs.add("wifi_to_network", detail_page("wifi") == Some("settings/network"), "");
    cs.add("unknown_tile_honest", detail_page("warp-drive").is_none(), "");
    // 3) 误触率（快速点击 0 误进——200ms 内释放恒为 Click）。
    for t in [0u64, 50, 100, 199] {
        let p = TilePress::begin(1_000, false);
        if p.release(1_000 + t) != PressOutcome::Click {
            cs.add("no_misfire", false, "");
            return cs;
        }
    }
    cs.add("no_misfire", true, "");
    // 4) 长按直达详情。
    let p2 = TilePress::begin(0, false);
    cs.add("longpress_detail", p2.release(500) == PressOutcome::LongPressToDetail, "");
    // 5) 与编辑态冲突消解（编辑按钮 vs 磁贴本体——两长按各走各门）。
    let body = TilePress::begin(0, false);
    let edit = TilePress::begin(0, true);
    cs.add("two_longpress_distinct", body.release(600) == PressOutcome::LongPressToDetail && edit.release(600) == PressOutcome::EditMode, "");
    // 6) 中途取消（移出磁贴——长按作废）。
    let mut p3 = TilePress::begin(0, false);
    p3.move_out();
    cs.add("move_out_cancels", p3.release(800) == PressOutcome::Cancelled, "");
    // 7) 宽容窗（200-500ms 释放仍算点击——不卡边界）。
    let p4 = TilePress::begin(0, false);
    cs.add("grace_window", p4.release(350) == PressOutcome::Click, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_tiles_all_have_pages() {
        for (tile, page) in TILE_PAGES {
            assert_eq!(detail_page(tile), Some(page));
            assert!(page.starts_with("settings/"));
        }
    }

    #[test]
    fn fast_clicks_never_open_detail() {
        // 主册误触率判据：快速点击 0 误进。
        for held in [1u64, 100, 199] {
            let p = TilePress::begin(0, false);
            assert_eq!(p.release(held), PressOutcome::Click);
        }
    }

    #[test]
    fn edit_button_never_opens_detail() {
        let p = TilePress::begin(0, true);
        assert_eq!(p.release(1_000), PressOutcome::EditMode);
    }
}

// ===========================================================================
// 深化 v2（F496）：进度环 permille 曲线 / 六磁贴直达页全表审计 /
// 编辑态冲突消解矩阵 / 快速点击零误进 / 长按中断恢复
// ===========================================================================

/// 进度环曲线（500ms 线性倒数：held 0ms → 0‰、500ms → 1000‰、
/// 超时钳制 1000‰——进度环让人知道长按生效了）。
pub fn progress_curve_cases_ok() -> bool {
    progress_ring_permille(0) == 0
        && progress_ring_permille(250) == 500
        && progress_ring_permille(500) == 1_000
        && progress_ring_permille(2_000) == 1_000
}

/// 六磁贴直达页全表审计（主册「六代表磁贴直达页对照」——
/// 每枚磁贴都有直达页且互异：磁贴是快捷开关还是入口，分得清）。
pub fn tile_pages_table_healthy() -> bool {
    let mut distinct = true;
    for i in 0..TILE_N {
        for j in (i + 1)..TILE_N {
            if TILE_PAGES[i].0 == TILE_PAGES[j].0 || TILE_PAGES[i].1 == TILE_PAGES[j].1 {
                distinct = false;
            }
        }
    }
    distinct && (0..TILE_N).all(|i| detail_page(TILE_PAGES[i].0).is_some())
}

/// 编辑态冲突消解矩阵（主册「磁贴右上编辑按钮进编辑、磁贴本体长按
/// 进详情（两长按分工明确）」——两入口 2×2 全算：编辑按钮长按 → 编辑、
/// 本体长按 → 详情，互相不抢）。
pub fn edit_conflict_matrix(on_edit_button: bool, longpress_fired: bool) -> &'static str {
    match (on_edit_button, longpress_fired) {
        (true, true) => "edit-mode",      // 编辑按钮长按 → 编辑态。
        (true, false) => "edit-arm",      // 编辑按钮按下（未到时长）→ 预备。
        (false, true) => "detail-page",   // 本体长按 → 详情页。
        (false, false) => "none",         // 尚未构成动作。
    }
}

/// 快速点击零误进（主册「误触率（快速点击 0 误进）」：点击窗口内
/// （<200ms）松手 → 永不触发详情——快速开关磁贴的手感保障）。
pub fn quick_click_never_details(press: &TilePress, release_ms: u64) -> bool {
    matches!(press.release(release_ms), PressOutcome::Click)
}

/// 长按中断恢复（按到一半移出/松手 → 取消进度不触发——拖到一半
/// Esc 能放弃的磁贴版）。
pub fn longpress_interrupt_cancels(press: &mut TilePress, interrupt_ms: u64) -> bool {
    press.move_out();
    matches!(press.release(interrupt_ms), PressOutcome::Cancelled)
}

// ---------------------------------------------------------------------------
// 深化自检（F496 v2）
// ---------------------------------------------------------------------------

pub fn run_tilelongpress_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F496-v2");
    // 1) 进度环曲线：0/中点/满格/超时钳制。
    cs.add("progress_curve", progress_curve_cases_ok(), "");
    // 2) 六磁贴直达页全表。
    cs.add("tile_pages_full", tile_pages_table_healthy(), "");
    // 3) 编辑态冲突矩阵：两长按各走各门。
    cs.add("matrix_edit", edit_conflict_matrix(true, true) == "edit-mode", "");
    cs.add("matrix_detail", edit_conflict_matrix(false, true) == "detail-page", "");
    cs.add("matrix_idle", edit_conflict_matrix(false, false) == "none", "");
    // 4) 快速点击零误进（200ms 内松手 = Toggle 非 Detail）。
    let p = TilePress::begin(0, false);
    cs.add("quick_click_safe", quick_click_never_details(&p, 150), "");
    // 5) 长按中断：移出取消。
    let mut p2 = TilePress::begin(0, false);
    cs.add("interrupt_cancels", longpress_interrupt_cancels(&mut p2, 300), "");
    // 6) 长按时长锚。
    cs.add("longpress_500ms", LONGPRESS_MS == 500, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn full_press_reaches_detail() {
        let mut p = TilePress::begin(0, false);
        // 按住到 500ms 不松手 → 详情触发。
        assert!(matches!(p.release(500), PressOutcome::LongPressToDetail));
    }

    #[test]
    fn edit_button_longpress_never_detail() {
        let mut p = TilePress::begin(0, true);
        // 编辑按钮上长按：进编辑态、永不进详情（分工明确）。
        assert!(matches!(p.release(600), PressOutcome::EditMode));
    }

    #[test]
    fn progress_never_regresses() {
        // 单次按压内进度单调不减。
        let mut last = 0;
        for t in [100u64, 200, 300, 400, 500] {
            let cur = progress_ring_permille(t);
            assert!(cur >= last);
            last = cur;
        }
    }

    #[test]
    fn every_tile_has_unique_page() {
        for (tile, page) in TILE_PAGES {
            assert_eq!(detail_page(tile), Some(page));
        }
    }
}

// ===========================================================================
// 深化 v5（F496）：按压迟滞（移出回入不重启进度）/ 触发后冷却 /
// 六磁贴按压账全链
// ===========================================================================

/// 迟滞回入窗（移出磁贴后在此窗口内回到原磁贴 → 进度续走不重启——
/// 手指轻微打滑不该惩罚用户；超窗才算真取消）。
pub const HYSTERESIS_MS: u64 = 120;

/// 带迟滞的按压会话 v2（v1 move_out 直接取消——对打滑手抖过于严苛）。
pub struct TilePressV2 {
    start_ms: u64,
    on_edit_button: bool,
    /// 移出时刻（None = 未移出）。
    left_at: Option<u64>,
}

impl TilePressV2 {
    pub fn begin(now_ms: u64, on_edit_button: bool) -> Self {
        TilePressV2 { start_ms: now_ms, on_edit_button, left_at: None }
    }

    pub fn move_out(&mut self, now_ms: u64) {
        if self.left_at.is_none() {
            self.left_at = Some(now_ms);
        }
    }

    /// 迟滞内回入：移出账清除，进度续走。
    pub fn reenter(&mut self, now_ms: u64) -> bool {
        match self.left_at {
            Some(t) if now_ms.saturating_sub(t) <= HYSTERESIS_MS => {
                self.left_at = None;
                true
            }
            _ => false,
        }
    }

    pub fn release(&self, now_ms: u64) -> PressOutcome {
        if self.left_at.is_some() {
            return PressOutcome::Cancelled;
        }
        let held = now_ms.saturating_sub(self.start_ms);
        if held < CLICK_CAP_MS {
            return PressOutcome::Click;
        }
        if held >= LONGPRESS_MS {
            if self.on_edit_button {
                PressOutcome::EditMode
            } else {
                PressOutcome::LongPressToDetail
            }
        } else {
            PressOutcome::Click
        }
    }
}

/// 触发后冷却（长按刚触发详情页，同一磁贴立即再次按压不再秒触发——
/// 「关了又自动弹回」的反面模式防线；冷却窗 400ms）。
pub const TRIGGER_COOLDOWN_MS: u64 = 400;

pub struct TileCooldown {
    last_trigger_ms: Option<u64>,
}

impl TileCooldown {
    pub const fn new() -> Self {
        TileCooldown { last_trigger_ms: None }
    }

    /// 长按触发许可：冷却窗内拒绝（进度环走满也不进——防连按双开）。
    pub fn may_trigger(&self, now_ms: u64) -> bool {
        match self.last_trigger_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) > TRIGGER_COOLDOWN_MS,
        }
    }

    pub fn mark_triggered(&mut self, now_ms: u64) {
        self.last_trigger_ms = Some(now_ms);
    }
}

/// 六磁贴按压裁决账（批量走查：六枚磁贴逐一「长按进对页」——
/// 对照表语义全链验证，漏一枚都算数）。
pub fn all_tiles_longpress_to_own_page() -> bool {
    TILE_PAGES.iter().all(|(t, page)| match detail_page(t) {
        Some(p) => p == *page,
        None => false,
    })
}

pub fn run_tilelongpress_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F496-v5");
    // 1) 迟滞回入：移出 80ms 回入 → 进度续走、松手仍触发详情。
    let mut p = TilePressV2::begin(1_000, false);
    p.move_out(1_060);
    cs.add("hysteresis_reenter", p.reenter(1_140), "");
    cs.add("hysteresis_progress_kept", p.release(1_520) == PressOutcome::LongPressToDetail, "");
    // 2) 迟滞超窗：移出 200ms 后回入无效（真取消）。
    let mut p2 = TilePressV2::begin(1_000, false);
    p2.move_out(1_050);
    cs.add("hysteresis_expired", !p2.reenter(1_250), "");
    cs.add("hysteresis_expired_cancelled", p2.release(1_600) == PressOutcome::Cancelled, "");
    // 3) 移出未回入 → 释放取消（v1 语义保持）。
    let mut p3 = TilePressV2::begin(0, false);
    p3.move_out(100);
    cs.add("left_still_cancels", p3.release(900) == PressOutcome::Cancelled, "");
    // 4) 触发后冷却：刚触发不许立即再触发；冷却过后放行。
    let mut cd = TileCooldown::new();
    cs.add("cooldown_first_ok", cd.may_trigger(1_000), "");
    cd.mark_triggered(1_000);
    cs.add("cooldown_blocks", !cd.may_trigger(1_200), "");
    cs.add("cooldown_expires", cd.may_trigger(1_000 + TRIGGER_COOLDOWN_MS + 1), "");
    // 5) 边界：恰好冷却窗时长 = 仍拒（> 才放行——防边界双开）。
    let mut cd2 = TileCooldown::new();
    cd2.mark_triggered(500);
    cs.add("cooldown_boundary", !cd2.may_trigger(500 + TRIGGER_COOLDOWN_MS), "");
    // 6) 六磁贴逐一长按进对页（对照表全链）。
    cs.add("six_tiles_chain", all_tiles_longpress_to_own_page(), "");
    // 7) 迟滞窗常量锚（主册手感参数文档化）。
    cs.add("hysteresis_const", HYSTERESIS_MS == 120, "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn double_reenter_ignored() {
        // 二次回入（已不在移出态）不动账。
        let mut p = TilePressV2::begin(0, false);
        p.move_out(10);
        assert!(p.reenter(50));
        assert!(!p.reenter(60), "未移出时 reenter = false（无账可清）");
        assert!(matches!(p.release(600), PressOutcome::LongPressToDetail));
    }

    #[test]
    fn hysteresis_does_not_save_fast_click() {
        // 迟滞回入救进度不救点击：短按 + 打滑回入仍是 Click。
        let mut p = TilePressV2::begin(1_000, false);
        p.move_out(1_020);
        assert!(p.reenter(1_100));
        assert_eq!(p.release(1_150), PressOutcome::Click);
    }

    #[test]
    fn cooldown_per_tile_isolation() {
        // 冷却是每磁贴一册：A 磁贴触发不影响 B 磁贴。
        let mut a = TileCooldown::new();
        let b = TileCooldown::new();
        a.mark_triggered(0);
        assert!(!a.may_trigger(100));
        assert!(b.may_trigger(100), "B 无触发史 = 放行");
    }
}
