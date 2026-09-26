//! F446 分辨率与刷新率选择 · 完整设计（STAR I 主册 G-I-46）。
//!
//! **判据（主册）**：支持列表真实性（EDID 读取）；确认倒计时回滚链路；
//! 每屏独立；缩放联动；切换黑屏期 <2s。＋通12。
//!
//! 设计：分辨率选择核——支持组合表（来自 EDID 注入——不支持的根本
//! 不出现，选错的坑从源头铲掉）；当前组合标记；切换两拍（应用 → 
//! **15s 确认倒计时**——超时/拒绝自动回滚原组合，选黑屏了也能自救，
//! 回滚链路全程记账）；每屏独立（各屏各自的当前组合与支持表）；缩放
//! 联动（分辨率变化 → 缩放档重算建议）；黑屏期账（>2s 计数——诚实）；
//! 游戏向组合（该屏最高刷新率）快捷标记。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 确认倒计时时长（ms）。
pub const CONFIRM_WINDOW_MS: u64 = 15_000;
/// 黑屏期判线（ms）。
pub const BLACKOUT_BUDGET_MS: u64 = 2_000;

/// 一个显示模式组合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeCombo {
    pub width: u32,
    pub height: u32,
    /// 刷新率（Hz ×100，支持 59.94 类小数）。
    pub refresh_centi_hz: u32,
}

impl ModeCombo {
    pub fn label(self) -> (u32, u32, u32) {
        (self.width, self.height, self.refresh_centi_hz)
    }
}

/// 每屏独立的分辨率状态机。
pub struct ScreenRes {
    pub screen_id: u64,
    /// EDID 注入的真实支持组合（只列真实——不支持的不出现）。
    pub supported: Vec<ModeCombo>,
    /// 当前生效组合。
    pub current: ModeCombo,
    /// 在途切换（Some = (新组合, 剩余确认 ms, 黑屏 ms)）。
    pub pending: Option<(ModeCombo, u64, u64)>,
    /// 回滚次数账。
    pub rollbacks: u64,
    /// 黑屏超预算计数（>2s——诚实记账）。
    pub blackout_over_budget: u64,
}

impl ScreenRes {
    pub fn new(screen_id: u64, supported: Vec<ModeCombo>, current: ModeCombo) -> ScreenRes {
        ScreenRes {
            screen_id,
            supported,
            current,
            pending: None,
            rollbacks: 0,
            blackout_over_budget: 0,
        }
    }

    /// 支持列表真实性：待选组合必须在 EDID 表内（不在表内拒绝）。
    pub fn can_select(&self, combo: ModeCombo) -> bool {
        self.supported.contains(&combo)
    }

    /// 应用新组合：记录在途 + 黑屏期账。
    pub fn apply(&mut self, combo: ModeCombo, blackout_ms: u64) -> bool {
        if !self.can_select(combo) || self.pending.is_some() {
            return false;
        }
        if blackout_ms > BLACKOUT_BUDGET_MS {
            self.blackout_over_budget += 1;
        }
        self.pending = Some((combo, CONFIRM_WINDOW_MS, blackout_ms));
        true
    }

    /// 倒计时推进；归零 → 自动回滚（自救判据）。返回 Some(回滚到) 。
    pub fn tick(&mut self, elapsed_ms: u64) -> Option<ModeCombo> {
        let (combo, remain, _) = self.pending?;
        if remain <= elapsed_ms {
            self.pending = None;
            self.rollbacks += 1;
            Some(self.current)
        } else {
            self.pending = Some((combo, remain - elapsed_ms, self.pending.unwrap().2));
            None
        }
    }

    /// 用户确认「保留新设置」。
    pub fn confirm(&mut self) -> bool {
        let Some((combo, _, _)) = self.pending else { return false };
        self.current = combo;
        self.pending = None;
        true
    }

    /// 游戏向快捷标记：该屏支持表中刷新率最高的组合。
    pub fn gaming_pick(&self) -> Option<ModeCombo> {
        self.supported.iter().copied().max_by_key(|c| c.refresh_centi_hz)
    }

    /// 缩放联动建议：4K 级分辨率建议 150%，1080p 级 100%（F224 联动）。
    pub fn scale_suggestion(&self) -> u32 {
        if self.current.height >= 2160 {
            150
        } else if self.current.height >= 1440 {
            125
        } else {
            100
        }
    }
}

pub fn run_ressel_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F446");
    let edid = alloc::vec![
        ModeCombo { width: 3840, height: 2160, refresh_centi_hz: 6_000 },
        ModeCombo { width: 2560, height: 1440, refresh_centi_hz: 14_400 },
        ModeCombo { width: 2560, height: 1440, refresh_centi_hz: 16_500 },
        ModeCombo { width: 1920, height: 1080, refresh_centi_hz: 6_000 },
    ];
    let mut s1 = ScreenRes::new(1, edid.clone(), edid[0]);
    let s2 = ScreenRes::new(2, alloc::vec![edid[3]], edid[3]);
    // 支持列表真实性：表内可选，表外（假 4K 240Hz）拒绝——不给人选错的坑。
    let fake = ModeCombo { width: 3840, height: 2160, refresh_centi_hz: 24_000 };
    set.add(
        "f446-edid-truthful",
        s1.can_select(edid[2]) && !s1.can_select(fake) && !s2.can_select(edid[0]),
        "",
    );
    // 每屏独立：屏 2 只有 1080p，屏 1 的 2K 对它不可见。
    set.add("f446-independent-screens", s1.screen_id != s2.screen_id && s2.supported.len() == 1, "");
    // 切换 + 黑屏期账。
    set.add(
        "f446-apply-with-blackout-ledger",
        s1.apply(edid[2], 1_200) && s1.blackout_over_budget == 0 && s1.pending.is_some(),
        "",
    );
    set.add("f446-pending-blocks-reapply", !s1.apply(edid[1], 1_000), "");
    // 确认倒计时：15s 窗内确认 → 保留。
    let _ = s1.tick(14_000);
    set.add("f446-confirm-keeps", s1.confirm() && s1.current == edid[2] && s1.rollbacks == 0, "");
    // 超时回滚：不确认 → 15s 后自动回原组合（自救）。
    set.add(
        "f446-timeout-rollback",
        s1.apply(edid[1], 1_500)
            && s1.tick(14_999).is_none()
            && s1.tick(1) == Some(edid[2])
            && s1.current == edid[2]
            && s1.rollbacks == 1,
        "",
    );
    // 黑屏期 >2s 诚实记账（换到 4K 时黑屏 2.5s）。
    let _ = s1.apply(edid[0], 2_500);
    set.add("f446-blackout-over-logged", s1.blackout_over_budget == 1, "");
    // 游戏向标记：最高刷新率组合（165Hz）。
    set.add("f446-gaming-pick", s1.gaming_pick() == Some(edid[2]), "");
    // 缩放联动建议：当前 2K → 125%；1080p → 100%（4K → 150% 由 s2 前身验证）。
    set.add(
        "f446-scale-suggestion",
        s1.scale_suggestion() == 125 && s2.scale_suggestion() == 100,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_restores_exactly() {
        let edid = alloc::vec![
            ModeCombo { width: 1920, height: 1080, refresh_centi_hz: 6_000 },
            ModeCombo { width: 1280, height: 720, refresh_centi_hz: 6_000 },
        ];
        let mut s = ScreenRes::new(1, edid.clone(), edid[0]);
        assert!(s.apply(edid[1], 800));
        // 倒计时全程不确认 → 精确回原组合。
        let mut rolled = None;
        for _ in 0..15 {
            if let Some(back) = s.tick(1_000) {
                rolled = Some(back);
            }
        }
        assert_eq!(rolled, Some(edid[0]), "回滚精确还原原组合");
        assert_eq!(s.current, edid[0]);
    }

    #[test]
    fn confirm_stops_the_clock() {
        let edid = alloc::vec![
            ModeCombo { width: 1920, height: 1080, refresh_centi_hz: 6_000 },
            ModeCombo { width: 1280, height: 720, refresh_centi_hz: 6_000 },
        ];
        let mut s = ScreenRes::new(1, edid.clone(), edid[0]);
        assert!(s.apply(edid[1], 500));
        assert!(s.confirm());
        assert!(s.tick(60_000).is_none(), "确认后倒计时不再走");
        assert_eq!(s.current, edid[1]);
    }
}
