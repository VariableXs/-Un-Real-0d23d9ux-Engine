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
