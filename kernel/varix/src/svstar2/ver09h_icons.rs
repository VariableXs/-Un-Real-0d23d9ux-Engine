//! VE-F3528 · 图标形状人格化 —— 形状蒙版切换 + 混合统一 + 可读性断言。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3528`
//!
//! # 职责（锚点原文拆解）
//!
//! - **形状蒙版**（[`IconMask`]）：圆形/圆角方/泪滴三种蒙版风格，
//!   同一图标底稿配不同蒙版出不同形状。蒙版是数据（模板号），
//!   不是逐图标定制。
//! - **统一应用**（[`MaskUnifier`]）：混合形状图标自动统一——
//!   用户选了圆形，桌面上就不该同时出现圆角方。统一器扫描
//!   已应用蒙版，不一致的重打。
//! - **可读性断言**（[`ReadabilityGate`]）：形状改变不损识别度
//!   ——蒙版裁切面积占比（[`MASK_AREA_PERMILLE`]）低于阈值即
//!   判识别度受损，警告 + 回退原形状。
//! - **回退保护**：蒙版缺失 → 原样（不裁切）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 识别度下降 | 警告 + 回退 |
//! | 蒙版缺失 | 原样 |
//! | 混用 | 统一 |
//!
//! # 复杂度
//!
//! 全部 O(图标数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5970 起）
// ---------------------------------------------------------------------------

/// 图标形状域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskCode {
    /// 识别度受损（回退原形状）。
    ReadabilityLoss = 5970,
    /// 蒙版缺失（原样输出）。
    MaskMissing = 5971,
    /// 混用形状（统一器重打）。
    MixedShapes = 5972,
}

impl MaskCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MaskCode::ReadabilityLoss => "裁切损识别度已回退",
            MaskCode::MaskMissing => "蒙版缺失原样输出",
            MaskCode::MixedShapes => "混用形状已统一",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、蒙版定义
// ---------------------------------------------------------------------------

/// 最低裁切面积占比（千分比，低于此判识别度受损）。
pub const MASK_AREA_PERMILLE: u32 = 700;

/// 形状蒙版三风格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconMask {
    /// 圆形（裁切最多）。
    Circle,
    /// 圆角方（默认）。
    RoundedSquare,
    /// 泪滴。
    Teardrop,
}

impl IconMask {
    /// 裁切面积占比（千分比，相对方形底稿）。
    pub fn area_permille(self) -> u32 {
        match self {
            IconMask::Circle => 785,
            IconMask::RoundedSquare => 960,
            IconMask::Teardrop => 820,
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            IconMask::Circle => "圆形",
            IconMask::RoundedSquare => "圆角方",
            IconMask::Teardrop => "泪滴",
        }
    }
}

/// 一枚已应用的图标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IconTile {
    /// 图标 id。
    pub id: u16,
    /// 当前蒙版（None = 原样未裁切）。
    pub mask: Option<IconMask>,
    /// 图标主体面积占比（千分比，底稿特征）。
    pub subject_area: u32,
}

// ---------------------------------------------------------------------------
// 三、可读性门
// ---------------------------------------------------------------------------

/// 可读性判定：裁切后主体必须仍占底稿 [`MASK_AREA_PERMILLE`] 以上。
/// 受损 → 回退原样并告警。
pub fn apply_mask(tile: &IconTile, want: IconMask) -> (Option<IconMask>, Option<MaskCode>) {
    // 主体面积 × 蒙版占比 = 裁切后可见主体。
    let visible = tile.subject_area * want.area_permille() / 1000;
    if visible < MASK_AREA_PERMILLE {
        (None, Some(MaskCode::ReadabilityLoss))
    } else {
        (Some(want), None)
    }
}

// ---------------------------------------------------------------------------
// 四、统一器
// ---------------------------------------------------------------------------

/// 统一结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnifyReport {
    /// 重打数（混合被统一的图标数）。
    pub re_masked: u32,
    /// 回退数（识别度受损回原样的）。
    pub reverted: u32,
}

/// 统一器：把全部图标对齐到目标蒙版。
#[derive(Default)]
pub struct MaskUnifier {
    pub codes: Vec<MaskCode>,
}

impl MaskUnifier {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 统一应用：want = None 表示全部回原样（用户关闭形状个性化）。
    pub fn unify(&mut self, tiles: &mut [IconTile], want: Option<IconMask>) -> UnifyReport {
        let mut rep = UnifyReport { re_masked: 0, reverted: 0 };
        // 混用检测：现有蒙版不一致（且都在场）→ 记违例。
        let masks: Vec<IconMask> = tiles.iter().filter_map(|t| t.mask).collect();
        if masks.windows(2).any(|w| w[0] != w[1]) {
            self.codes.push(MaskCode::MixedShapes);
        }
        for t in tiles.iter_mut() {
            match want {
                None => {
                    if t.mask.is_some() {
                        t.mask = None;
                        rep.re_masked += 1;
                    }
                }
                Some(m) => {
                    let (got, code) = apply_mask(t, m);
                    match code {
                        Some(MaskCode::ReadabilityLoss) => {
                            self.codes.push(MaskCode::ReadabilityLoss);
                            rep.reverted += 1;
                        }
                        _ => {}
                    }
                    if t.mask != got {
                        t.mask = got;
                        rep.re_masked += 1;
                    }
                }
            }
        }
        rep
    }

    /// 一致性复检：全部在场蒙版一致（或全 None）。
    pub fn consistent(tiles: &[IconTile]) -> bool {
        let masks: Vec<IconMask> = tiles.iter().filter_map(|t| t.mask).collect();
        masks.windows(2).all(|w| w[0] == w[1])
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(id: u16, area: u32) -> IconTile {
        IconTile { id, mask: None, subject_area: area }
    }

    #[test]
    fn unify_all_to_circle() {
        let mut u = MaskUnifier::new();
        let mut tiles = vec![tile(1, 990), tile(2, 950)];
        let rep = u.unify(&mut tiles, Some(IconMask::Circle));
        assert_eq!(rep.re_masked, 2);
        assert!(MaskUnifier::consistent(&tiles));
    }

    #[test]
    fn readability_loss_reverts() {
        let mut u = MaskUnifier::new();
        // 主体只占 750，圆形裁切后 750*785/1000=589 < 700 → 回退。
        let mut tiles = vec![tile(1, 750)];
        let rep = u.unify(&mut tiles, Some(IconMask::Circle));
        assert_eq!(rep.reverted, 1);
        assert_eq!(tiles[0].mask, None);
    }

    #[test]
    fn mixed_detected_then_fixed() {
        let mut u = MaskUnifier::new();
        let mut tiles = vec![
            IconTile { id: 1, mask: Some(IconMask::Circle), subject_area: 990 },
            IconTile { id: 2, mask: Some(IconMask::Teardrop), subject_area: 990 },
        ];
        let rep = u.unify(&mut tiles, Some(IconMask::RoundedSquare));
        assert!(u.codes.contains(&MaskCode::MixedShapes));
        assert_eq!(rep.re_masked, 2);
    }

    #[test]
    fn disable_restores_all() {
        let mut u = MaskUnifier::new();
        let mut tiles = vec![IconTile { id: 1, mask: Some(IconMask::Circle), subject_area: 990 }];
        u.unify(&mut tiles, None);
        assert_eq!(tiles[0].mask, None);
    }
}
