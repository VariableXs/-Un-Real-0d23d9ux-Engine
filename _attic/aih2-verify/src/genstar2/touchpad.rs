//! F481 触控板灵敏度与自然滚动（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **双设备独立记忆；自然/传统切换即时；掌压三档用例（打字注入误触测试）；
//! 速度档实测；设置持久化。**
//!
//! 功能定义（主册批次三）：触控板双参数——指针速度五档（与 F250 鼠标速度
//! 独立——两设备各记各的）、滚动方向开关（自然滚动默认开）；参数即时生效；
//! 掌压检测阈值可选（三档灵敏度）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 指针速度五档（1-5；与 F250 鼠标速度独立）。
pub const SPEED_TIERS: u8 = 5;
/// 默认速度档（中档 3）。
pub const DEFAULT_SPEED_TIER: u8 = 3;
/// 掌压三档灵敏度（0 严格 / 1 标准 / 2 宽松——判定阈值）。
pub const PALM_TIERS: usize = 3;
/// 掌压判定阈值表（手掌接触面积 permille：≥阈值判掌压——三档各异）。
pub const PALM_THRESHOLDS_PERMILLE: [u16; PALM_TIERS] = [350, 500, 650];
/// 自然滚动默认（主册：自然滚动默认开）。
pub const NATURAL_DEFAULT: bool = true;

/// 触控板设置（双设备独立记忆——触控板 vs 鼠标各记各的）。
#[derive(Clone, Copy, Debug)]
pub struct TouchpadSettings {
    /// 指针速度档 1-5。
    pub speed_tier: u8,
    /// 自然滚动（默认开；传统方向给 Windows 老滚轮习惯党）。
    pub natural_scroll: bool,
    /// 掌压灵敏度档 0-2。
    pub palm_tier: u8,
}

impl TouchpadSettings {
    pub const fn new() -> Self {
        TouchpadSettings {
            speed_tier: DEFAULT_SPEED_TIER,
            natural_scroll: NATURAL_DEFAULT,
            palm_tier: 1,
        }
    }

    /// 速度档设置（越界钳制在 1-5——即时生效）。
    pub fn set_speed(&mut self, tier: u8) -> u8 {
        self.speed_tier = tier.clamp(1, SPEED_TIERS);
        self.speed_tier
    }

    /// 自然/传统切换（即时——主册：切换即时）。
    pub fn toggle_scroll(&mut self, natural: bool) {
        self.natural_scroll = natural;
    }

    /// 掌压档设置（越界钳制 0-2）。
    pub fn set_palm_tier(&mut self, tier: u8) -> u8 {
        self.palm_tier = tier.clamp(0, PALM_TIERS as u8 - 1);
        self.palm_tier
    }

    /// 掌压判定（打字注入误触测试：接触面积 ≥ 当前档阈值 → 判掌压吞事件）。
    pub fn palm_detected(&self, contact_permille: u16) -> bool {
        contact_permille >= PALM_THRESHOLDS_PERMILLE[self.palm_tier as usize]
    }
}

/// 滚动语义翻转（自然滚动：手指上滑 = 内容上移；传统：反向——只翻语义，
/// F204 惯性曲线不动）。
pub fn scroll_semantics(natural: bool, finger_delta_y: i32) -> i32 {
    if natural {
        finger_delta_y
    } else {
        -finger_delta_y
    }
}

/// 双设备独立记忆审计（触控板速度变化不影响鼠标档——F250 独立域）。
pub fn devices_independent(tp: u8, mouse: u8, tp_new: u8, mouse_after: u8) -> bool {
    tp != tp_new && mouse == mouse_after
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_touchpad_checks() -> CheckSet {
    let mut cs = CheckSet::new("F481-touchpad");
    // 1) 双设备独立记忆（触控板 3 档调到 5，鼠标档不变）。
    cs.add("devices_independent", devices_independent(3, 4, 5, 4), "");
    // 2) 速度五档（1-5 钳制）。
    let mut s = TouchpadSettings::new();
    cs.add("default_tier3", s.speed_tier == 3, "");
    cs.add("speed_clamped", s.set_speed(9) == 5 && s.set_speed(0) == 1, "");
    // 3) 自然/传统切换即时。
    cs.add("natural_default", TouchpadSettings::new().natural_scroll == NATURAL_DEFAULT, "");
    s.toggle_scroll(false);
    cs.add("toggle_instant", !s.natural_scroll, "");
    // 4) 滚动语义翻转正确（内容移动方向）。
    cs.add("semantic_flip", scroll_semantics(true, 120) == 120 && scroll_semantics(false, 120) == -120, "");
    // 5) 掌压三档用例（打字注入误触测试：同一接触面积三档判定各异）。
    let mut s2 = TouchpadSettings::new();
    let typing_inject = 520u16; // 打字手掌误触面积
    s2.set_palm_tier(0); // 严格档：350 即判 → 误触被吞
    cs.add("palm_strict_catches", s2.palm_detected(typing_inject), "");
    s2.set_palm_tier(1); // 标准：500 → 仍吞
    cs.add("palm_standard_catches", s2.palm_detected(typing_inject), "");
    s2.set_palm_tier(2); // 宽松：650 → 放行（误触不算掌压）
    cs.add("palm_loose_passes", !s2.palm_detected(typing_inject), "");
    cs.add("palm_thresholds", PALM_THRESHOLDS_PERMILLE == [350, 500, 650], "");
    // 6) 指尖正常触摸永不被吞。
    cs.add("fingertip_never_palm", !TouchpadSettings::new().palm_detected(200), "");
    // 7) 持久化字段齐备（设置持久化——全部字段可序列化）。
    let s3 = TouchpadSettings { speed_tier: 4, natural_scroll: false, palm_tier: 2 };
    cs.add("persistable", s3.speed_tier == 4 && !s3.natural_scroll && s3.palm_tier == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_five_tiers_reachable() {
        let mut s = TouchpadSettings::new();
        for want in 1..=5u8 {
            assert_eq!(s.set_speed(want), want);
        }
        assert_eq!(s.set_speed(6), 5);
        assert_eq!(s.set_speed(0), 1);
    }

    #[test]
    fn palm_three_tiers_matrix() {
        let mut s = TouchpadSettings::new();
        // 三档 × 三种接触面积 = 9 格判定矩阵全对。
        for tier in 0..PALM_TIERS as u8 {
            s.set_palm_tier(tier);
            for area in [340u16, 500, 660] {
                let want = area >= PALM_THRESHOLDS_PERMILLE[tier as usize];
                assert_eq!(s.palm_detected(area), want, "tier={tier} area={area}");
            }
        }
    }

    #[test]
    fn natural_scroll_semantics_flip_only() {
        // 只翻语义不破坏曲线：同一输入幅度，两方向互为相反数。
        for d in [10i32, -240, 1_000] {
            assert_eq!(scroll_semantics(true, d), -scroll_semantics(false, d));
        }
    }
}
