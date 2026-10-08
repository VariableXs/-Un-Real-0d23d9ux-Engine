//! 深化层五 · F144 「Crafted for VARIX」徽标（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：徽标状态 → 应用详情页角标数据（F352 缩略图/
//! Alt+Tab 卡片三处一致的数据源）、tooltip 行、争议入口深链。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 角标数据：徽标状态 → 三处共用渲染数据（标题栏/缩略图/任务切换卡）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipState {
    Active,
    Grace,
    None,
}

pub struct BadgeChip {
    pub app: &'static str,
    pub state: ChipState,
    /// 角标最小渲染尺寸（F144 usage 线 24px 的一致性下放）。
    pub min_px: u32,
}

pub fn chip_for(state: u8, app: &'static str) -> BadgeChip {
    BadgeChip {
        app,
        state: match state {
            0 => ChipState::Active,
            1 => ChipState::Grace,
            _ => ChipState::None,
        },
        min_px: 24,
    }
}

/// 三处一致性：同一 chip 数据喂三个位置（数据源唯一——十号纪律）。
pub fn chip_surfaces(chip: &BadgeChip) -> [(&'static str, bool); 3] {
    let show = chip.state != ChipState::None;
    [("titlebar", show), ("thumbnail", show), ("alttab", show)]
}

// ---------------------------------------------------------------------------
// tooltip 行：状态 → 说明（含点击去向）
// ---------------------------------------------------------------------------

pub fn chip_tooltip(state: ChipState) -> &'static str {
    match state {
        ChipState::Active => "Crafted for VARIX：通过四查门禁（点此查看详情）",
        ChipState::Grace => "复检宽限期内：下季复检通过即恢复（点此查看详情）",
        ChipState::None => "",
    }
}

// ---------------------------------------------------------------------------
// 争议入口深链：不服判定 → F148 治理立案参数
// ---------------------------------------------------------------------------

pub fn dispute_link(app: &str, badge_seq: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    let prefix = b"gov://dispute/";
    out[..14].copy_from_slice(prefix);
    let s = alloc::format!("{app}/{badge_seq}");
    let b = s.as_bytes();
    let take = b.len().min(32 - 14);
    out[14..14 + take].copy_from_slice(&b[..take]);
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144H_TAG: &str = "stareco-F144-deep5";

pub fn run_f144_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F144H_TAG);

    // 角标数据
    let active = chip_for(0, "files-app");
    let grace = chip_for(1, "editor");
    let none = chip_for(2, "stranger");
    set.add(
        "f144h chip states",
        active.state == ChipState::Active
            && grace.state == ChipState::Grace
            && none.state == ChipState::None,
        "三态映射",
    );
    set.add("f144h min px", active.min_px == 24, "24px 一致性下放");

    // 三处一致
    let surfaces = chip_surfaces(&active);
    set.add(
        "f144h surfaces consistent",
        surfaces.iter().all(|(_, show)| *show),
        "三处同显（数据源唯一）",
    );
    let hidden = chip_surfaces(&none);
    set.add(
        "f144h surfaces hidden",
        hidden.iter().all(|(_, show)| !*show),
        "无徽标三处同隐",
    );

    // tooltip
    set.add(
        "f144h tooltips",
        chip_tooltip(ChipState::Active).contains("四查")
            && chip_tooltip(ChipState::Grace).contains("复检")
            && chip_tooltip(ChipState::None).is_empty(),
        "三态文案",
    );

    // 争议深链
    let link = dispute_link("files-app", 3);
    set.add(
        "f144h dispute link",
        &link[..14] == b"gov://dispute/" && link[14..18] == *b"file",
        "深链前缀与参数",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn grace_state_visible() {
        // 宽限态仍显示徽标（降级不消失——用户知情权）。
        let c = chip_for(1, "x");
        assert!(chip_surfaces(&c).iter().all(|(_, show)| *show));
        assert!(chip_tooltip(ChipState::Grace).contains("宽限"));
    }

    #[test]
    fn dispute_link_truncates() {
        let link = dispute_link("a-very-long-application-name-exceeding-buffer", 99);
        assert!(link.len() == 32);
    }
}
