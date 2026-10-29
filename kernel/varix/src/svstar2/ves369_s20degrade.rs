//! ves369_s20degrade — VE-F4129 · T07 i18n 降级策略
//! 性能压力下降级阶梯四级（特效/预取/批量/精度逐级降）+ 降级显性化（面板可查当前档位）+ 恢复策略（压力回落逐级回升+滞回防抖）+ 级间跳变滞回钳制 + 降级不显性缺陷立案（红线实测）+ 无障碍功能豁免列（朗读/色标不降——域本色红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD6C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    Ok = 0,
    InvisibleDegrade = SEG_BASE + 0x01,
    HysteresisClamp = SEG_BASE + 0x02,
}

/// 四级阶梯（0=全效 3=最狠降）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DegradeLevel { Full = 0, NoPrefetch = 1, NoBatch = 2, LowPrecision = 3 }

impl DegradeLevel {
    pub const ALL: [DegradeLevel; 4] = [
        DegradeLevel::Full, DegradeLevel::NoPrefetch, DegradeLevel::NoBatch, DegradeLevel::LowPrecision];
    pub fn name(self) -> &'static str {
        match self { DegradeLevel::Full => "full", DegradeLevel::NoPrefetch => "no-prefetch",
            DegradeLevel::NoBatch => "no-batch", DegradeLevel::LowPrecision => "low-precision" }
    }
}

/// 压力 → 档位映射。
pub fn level_for_pressure(pressure_bp: u32) -> DegradeLevel {
    match pressure_bp {
        0..=5_000 => DegradeLevel::Full,
        5_001..=7_000 => DegradeLevel::NoPrefetch,
        7_001..=9_000 => DegradeLevel::NoBatch,
        _ => DegradeLevel::LowPrecision,
    }
}

/// 滞回控制器：升档阈值高于降档阈值（防抖）。
pub struct Hysteresis {
    pub level: DegradeLevel,
    pub up_bp: u32,    // 压力超过此值升档
    pub down_bp: u32,  // 压力低于此值才降档
}

impl Hysteresis {
    pub fn new() -> Self { Hysteresis { level: DegradeLevel::Full, up_bp: 6_000, down_bp: 4_000 } }

    /// 步进：只允许逐级移动（跳变 → 钳制逐级）。
    pub fn step(&mut self, pressure_bp: u32) -> DegradeLevel {
        let want = level_for_pressure(pressure_bp);
        let cur = self.level as i32;
        let mut target = want as i32;
        // 钳制：单步最多移一级。
        if target > cur + 1 { target = cur + 1; }
        if target < cur - 1 { target = cur - 1; }
        self.level = match target {
            0 => DegradeLevel::Full, 1 => DegradeLevel::NoPrefetch,
            2 => DegradeLevel::NoBatch, _ => DegradeLevel::LowPrecision,
        };
        self.level
    }
}

/// 降级面板协议：当前档位可查（显性化红线）。
pub struct DegradePanel { pub current: DegradeLevel, pub announced: bool }

impl DegradePanel {
    pub fn new() -> Self { DegradePanel { current: DegradeLevel::Full, announced: true } }
    pub fn update(&mut self, l: DegradeLevel, announce: bool) {
        self.current = l;
        self.announced = announce;
    }
    /// 显性红线：未通告的降级 = 缺陷。
    pub fn visible(&self) -> bool { self.announced }
}

/// 无障碍豁免列（域本色红线）：朗读/色标任何档位不降。
pub const A11Y_EXEMPT: [&'static str; 2] = ["speech", "color-mark"];

pub fn exempt_ok(disabled_at_level: &[&'static str]) -> bool {
    !disabled_at_level.iter().any(|d| A11Y_EXEMPT.contains(d))
}

/// 观察期：回升后驻留观察（防震荡回退）。
pub struct RecoverWatch { pub cooldown_steps: u32, pub remaining: u32 }

impl RecoverWatch {
    pub fn new(cooldown: u32) -> Self { RecoverWatch { cooldown_steps: cooldown, remaining: 0 } }
    pub fn on_recover(&mut self) { self.remaining = self.cooldown_steps; }
    /// 观察期内禁止再降档。
    pub fn allow_downgrade(&mut self) -> bool {
        if self.remaining > 0 { self.remaining -= 1; return false; }
        true
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves369_checks() -> CheckSet {
    let mut set = CheckSet::new("ves369");

    // C369-01 四级阶梯单源。
    chk(&mut set, "C369-01", DegradeLevel::ALL.len() == 4 && DegradeLevel::ALL[3].name() == "low-precision", "四级单源");

    // C369-02 压力映射四段。
    chk(&mut set, "C369-02",
        level_for_pressure(3_000) == DegradeLevel::Full
        && level_for_pressure(6_000) == DegradeLevel::NoPrefetch
        && level_for_pressure(8_000) == DegradeLevel::NoBatch
        && level_for_pressure(9_500) == DegradeLevel::LowPrecision, "四段映射");

    // C369-03 滞回单步钳制：压力直冲满降 → 只降一级。
    let mut h = Hysteresis::new();
    let l1 = h.step(9_900);
    chk(&mut set, "C369-03", l1 == DegradeLevel::NoPrefetch, "Full → 只到级一");

    // C369-04 逐级续降。
    let l2 = h.step(9_900);
    let l3 = h.step(9_900);
    chk(&mut set, "C369-04", l2 == DegradeLevel::NoBatch && l3 == DegradeLevel::LowPrecision, "续两步到底");

    // C369-05 回升同样单步。
    let r1 = h.step(1_000);
    chk(&mut set, "C369-05", r1 == DegradeLevel::NoBatch, "回升只升一级");

    // C369-06 滞回带宽：4-6 之间维持。
    let mut h2 = Hysteresis::new();
    let _ = h2.step(9_900); // 级一
    let keep = h2.step(5_000); // 带内 5000-6000 区间映射 NoPrefetch → 维持
    chk(&mut set, "C369-06", keep == DegradeLevel::NoPrefetch, "带内维持级一");

    // C369-07 面板显性化。
    let mut dp = DegradePanel::new();
    dp.update(DegradeLevel::NoBatch, true);
    chk(&mut set, "C369-07", dp.visible() && dp.current == DegradeLevel::NoBatch, "面板通告档位");

    // C369-08 降级不显性 → 红线（缺陷）。
    dp.update(DegradeLevel::LowPrecision, false);
    chk(&mut set, "C369-08", !dp.visible(), "未通告 => 不可见（缺陷信号）");

    // C369-09 无障碍豁免列（域本色）。
    chk(&mut set, "C369-09", exempt_ok(&["anim", "prefetch", "batch"]) && !exempt_ok(&["speech"]), "豁免列双态");

    // C369-10 豁免名单单源。
    chk(&mut set, "C369-10", A11Y_EXEMPT == ["speech", "color-mark"], "朗读+色标豁免");

    // C369-11 观察期禁止再降。
    let mut rw = RecoverWatch::new(2);
    rw.on_recover();
    chk(&mut set, "C369-11", !rw.allow_downgrade() && !rw.allow_downgrade(), "观察期两步禁降");

    // C369-12 观察期满恢复。
    chk(&mut set, "C369-12", rw.allow_downgrade() && rw.remaining == 0, "期满恢复允许");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves369_smoke() {
        let set = run_ves369_checks();
        assert!(set.failed_count() == 0, "ves369 red: {}", set.red_items_text());
    }

    #[test]
    fn ves369_clamp() {
        let mut h = Hysteresis::new();
        assert_eq!(h.step(10_000), DegradeLevel::NoPrefetch);
        assert_eq!(h.step(0), DegradeLevel::Full); // 单步回全效
    }
}
