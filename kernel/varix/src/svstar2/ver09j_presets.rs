//! VE-F3530 · Persona 预设包 —— 三官方预设 + 一键应用（原子事务复用）+ 微调共存。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3530`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三官方预设**（[`Preset::ALL`]）：深空玻璃/经典拟物/极简高对比，
//!   每个预设是一份完整五域档案（F3529 [`PersonaProfile`] 语义）。
//! - **一键应用**（[`PresetBook::apply`]）：原子事务复用（E04 语义）
//!   ——预演 → 提交 → 失败回滚。预设损坏 → 默认档案。
//! - **微调共存**（[`PresetBook::tweak`]）：预设为底、微调覆盖
//!   （覆盖层仲裁：微调值压预设值，域级仲裁）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 应用失败 | 回滚 |
//! | 微调冲突 | 覆盖层仲裁 |
//! | 预设损坏 | 默认 |
//!
//! # 复杂度
//!
//! 全部 O(1)（五域定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5990 起）
// ---------------------------------------------------------------------------

/// 预设域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresetCode {
    /// 应用下发失败（已回滚）。
    ApplyFailed = 5990,
    /// 预设指纹损坏（回默认档案）。
    PresetCorrupt = 5991,
    /// 未预演即提交。
    NotPreviewed = 5992,
}

impl PresetCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PresetCode::ApplyFailed => "预设应用失败已回滚",
            PresetCode::PresetCorrupt => "预设指纹损坏回默认",
            PresetCode::NotPreviewed => "未预演即提交",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PresetCode::NotPreviewed)
    }
}

// ---------------------------------------------------------------------------
// 二、三官方预设
// ---------------------------------------------------------------------------

/// 预设标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    /// 深空玻璃。
    DeepGlass,
    /// 经典拟物。
    ClassicSkeuo,
    /// 极简高对比。
    MinHighContrast,
}

impl Preset {
    /// 全集。
    pub const ALL: [Preset; 3] = [
        Preset::DeepGlass,
        Preset::ClassicSkeuo,
        Preset::MinHighContrast,
    ];

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Preset::DeepGlass => "深空玻璃",
            Preset::ClassicSkeuo => "经典拟物",
            Preset::MinHighContrast => "极简高对比",
        }
    }

    /// 预设档案（域值经过合法性检查——构造期保证）。
    pub fn profile(self) -> crate::svstar2::ver09i_profile::PersonaProfile {
        use crate::svstar2::ver09i_profile::*;
        let mut p = PersonaProfile::default_profile();
        match self {
            Preset::DeepGlass => {
                p.domains.color.accent = 0x3584E4;
                p.domains.color.dark = true;
                p.domains.motion.enabled = true;
                p.domains.motion.tier = 2;
                p.domains.icon.mask = 1;
            }
            Preset::ClassicSkeuo => {
                p.domains.color.accent = 0x8A5A2B;
                p.domains.color.dark = false;
                p.domains.motion.tier = 1;
                p.domains.icon.mask = 3;
            }
            Preset::MinHighContrast => {
                p.domains.color.accent = 0xFFFFFF;
                p.domains.color.dark = true;
                p.domains.motion.enabled = false; // 高对比+无障碍：动效默认关
                p.domains.icon.mask = 2;
            }
        }
        let _ = p.sanitize();
        p
    }
}

// ---------------------------------------------------------------------------
// 三、应用事务
// ---------------------------------------------------------------------------

/// 应用事务：预演 → 提交 → 失败回滚（E04 复用）。
pub struct PresetTx {
    snapshot: Option<crate::svstar2::ver09i_profile::PersonaProfile>,
    stage: TxStage,
}

/// 阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxStage {
    /// 未开始。
    Idle,
    /// 预演过。
    Prepared,
    /// 已提交。
    Committed,
    /// 已回滚。
    RolledBack,
}

impl PresetTx {
    /// 新事务。
    pub fn new() -> Self {
        PresetTx { snapshot: None, stage: TxStage::Idle }
    }

    /// 阶段。
    pub fn stage(&self) -> TxStage {
        self.stage
    }

    /// 预演：抓快照 + 预设指纹校验（损坏 → 回默认档案）。
    pub fn preview(
        &mut self,
        current: &crate::svstar2::ver09i_profile::PersonaProfile,
        want: Preset,
    ) -> crate::svstar2::ver09i_profile::PersonaProfile {
        self.snapshot = Some(*current);
        self.stage = TxStage::Prepared;
        // 指纹校验：预设档案消毒后无 Sanitized 码视为完好。
        let mut target = want.profile();
        let codes = target.sanitize();
        let corrupt = codes.contains(&crate::svstar2::ver09i_profile::PersonaCode::Sanitized);
        if corrupt {
            target = crate::svstar2::ver09i_profile::PersonaProfile::default_profile();
        }
        target
    }

    /// 提交（只有预演过才能提交）。`apply` 下发回调 false 即回滚。
    pub fn commit(
        &mut self,
        target: &crate::svstar2::ver09i_profile::PersonaProfile,
        mut apply: impl FnMut(&crate::svstar2::ver09i_profile::PersonaProfile) -> bool,
    ) -> Result<(), PresetCode> {
        if self.stage != TxStage::Prepared {
            return Err(PresetCode::NotPreviewed);
        }
        if apply(target) {
            self.stage = TxStage::Committed;
            Ok(())
        } else {
            let snap = self.snapshot.take();
            if let Some(s) = snap {
                let _ = apply(&s);
            }
            self.stage = TxStage::RolledBack;
            Err(PresetCode::ApplyFailed)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、微调覆盖层
// ---------------------------------------------------------------------------

/// 一条微调。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tweak {
    /// 改主音量。
    Volume(u32),
    /// 改基础字号。
    BasePx(u32),
    /// 改动效档。
    MotionTier(u8),
}

/// 把微调压到预设底稿上（微调覆盖纪律）。
pub fn apply_tweaks(
    base: &mut crate::svstar2::ver09i_profile::PersonaProfile,
    tweaks: &[Tweak],
) -> usize {
    let mut n = 0;
    for t in tweaks {
        match *t {
            Tweak::Volume(v) => {
                base.domains.sound.volume = v.min(1000);
                n += 1;
            }
            Tweak::BasePx(px) => {
                base.domains.font.base_px = px.max(crate::svstar2::ver09g_fonts::MIN_READABLE_PX);
                n += 1;
            }
            Tweak::MotionTier(tier) => {
                base.domains.motion.tier = tier.min(2);
                n += 1;
            }
        }
    }
    let _ = base.sanitize();
    n
}

// ---------------------------------------------------------------------------
// 五、预设账
// ---------------------------------------------------------------------------

/// 预设账。
pub struct PresetBook {
    /// 当前生效档案。
    pub current: crate::svstar2::ver09i_profile::PersonaProfile,
    /// 当前预设底（None = 纯微调/默认）。
    pub base: Option<Preset>,
    pub codes: Vec<PresetCode>,
}

impl PresetBook {
    /// 默认起步。
    pub fn new() -> Self {
        PresetBook {
            current: crate::svstar2::ver09i_profile::PersonaProfile::default_profile(),
            base: None,
            codes: Vec::new(),
        }
    }

    /// 一键应用。
    pub fn apply(&mut self, want: Preset, mut apply: impl FnMut(&crate::svstar2::ver09i_profile::PersonaProfile) -> bool) -> Result<(), PresetCode> {
        let mut tx = PresetTx::new();
        let target = tx.preview(&self.current, want);
        match tx.commit(&target, apply) {
            Ok(()) => {
                self.current = target;
                self.base = Some(want);
                Ok(())
            }
            Err(c) => {
                self.codes.push(c);
                Err(c)
            }
        }
    }

    /// 微调（预设为底覆盖）。
    pub fn tweak(&mut self, tweaks: &[Tweak]) -> usize {
        let n = apply_tweaks(&mut self.current, tweaks);
        n
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_presets_valid() {
        for p in Preset::ALL {
            let prof = p.profile();
            assert_eq!(prof.version, crate::svstar2::ver09i_profile::PERSONA_VERSION);
            assert!(prof.domains.sound.volume <= 1000);
        }
    }

    #[test]
    fn apply_rollback_on_failure() {
        let mut b = PresetBook::new();
        let r = b.apply(Preset::DeepGlass, |_| false);
        assert_eq!(r, Err(PresetCode::ApplyFailed));
        // 回滚：档案不变。
        assert_eq!(b.current, crate::svstar2::ver09i_profile::PersonaProfile::default_profile());
    }

    #[test]
    fn tweak_overrides_preset() {
        let mut b = PresetBook::new();
        b.apply(Preset::DeepGlass, |_| true).unwrap();
        b.tweak(&[Tweak::Volume(300)]);
        assert_eq!(b.current.domains.sound.volume, 300);
        assert_eq!(b.base, Some(Preset::DeepGlass)); // 底仍是预设
    }

    #[test]
    fn floor_enforced_in_tweak() {
        let mut b = PresetBook::new();
        b.tweak(&[Tweak::BasePx(4)]);
        assert_eq!(b.current.domains.font.base_px, crate::svstar2::ver09g_fonts::MIN_READABLE_PX);
    }
}
