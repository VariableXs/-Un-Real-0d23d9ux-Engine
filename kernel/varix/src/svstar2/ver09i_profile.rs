//! VE-F3529 · Persona 档案数据模型 —— 五域档案统一模型 + 版本化 + 开放导出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3529`
//!
//! # 职责（锚点原文拆解）
//!
//! - **五域模型**（[`PersonaDomains`]）：个性化配置族统一数据模型
//!   ——色彩/声音/字体/图标/动效五域，每域一条档案。
//! - **版本化**（[`PersonaVersion`]）：档案带版本号；低版本档案
//!   迁移（[`migrate`]），未来版本拒绝（不猜）。
//! - **开放导出**（[`PersonaProfile::export`]):导出为开放格式
//!   （稳定文本序列化）——不锁用户（开放纪律），导入对称。
//! - **损坏修复**（[`PersonaProfile::sanitize`]）：解析出的档案
//!   逐域消毒——越界值钳制、缺域补默认，消毒记录可审计。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 模型损坏 | 修复流程（消毒） |
//! | 版本漂移 | 迁移 |
//! | 导出失败 | 重试（纯函数不会失败——失败只在 IO 层） |
//!
//! # 复杂度
//!
//! 全部 O(域数)（五域定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5980 起）
// ---------------------------------------------------------------------------

/// Persona 档案域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonaCode {
    /// 版本高于本运行时（不猜未来格式）。
    VersionTooNew = 5980,
    /// 版本低于迁移下限（不可迁移）。
    VersionTooOld = 5981,
    /// 档案损坏（已消毒）。
    Sanitized = 5982,
    /// 域值越界（已钳制）。
    DomainClamped = 5983,
}

impl PersonaCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PersonaCode::VersionTooNew => "档案版本高于运行时",
            PersonaCode::VersionTooOld => "档案版本过旧不可迁移",
            PersonaCode::Sanitized => "档案已消毒",
            PersonaCode::DomainClamped => "域值越界已钳制",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PersonaCode::VersionTooNew | PersonaCode::VersionTooOld)
    }
}

// ---------------------------------------------------------------------------
// 二、版本
// ---------------------------------------------------------------------------

/// 当前档案版本。
pub const PERSONA_VERSION: u32 = 3;
/// 迁移下限（低于此版本不可迁移）。
pub const MIGRATABLE_FLOOR: u32 = 1;

/// 版本检查与迁移。
pub fn migrate(v: u32) -> Result<u32, PersonaCode> {
    if v > PERSONA_VERSION {
        return Err(PersonaCode::VersionTooNew);
    }
    if v < MIGRATABLE_FLOOR {
        return Err(PersonaCode::VersionTooOld);
    }
    // 逐级迁移：v → v+1 → … → PERSONA_VERSION。
    Ok(PERSONA_VERSION)
}

// ---------------------------------------------------------------------------
// 三、五域档案
// ---------------------------------------------------------------------------

/// 色彩域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorDomain {
    /// 强调色（RGB 打包）。
    pub accent: u32,
    /// 是否暗色模式。
    pub dark: bool,
}

impl Default for ColorDomain {
    fn default() -> Self {
        ColorDomain { accent: 0x3584E4, dark: false }
    }
}

/// 声音域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoundDomain {
    /// 主音量（千分比）。
    pub volume: u32,
    /// 音效包 id（0 = 默认）。
    pub pack: u16,
}

impl Default for SoundDomain {
    fn default() -> Self {
        SoundDomain { volume: 800, pack: 0 }
    }
}

/// 字体域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontDomain {
    /// 基础字号（px）。
    pub base_px: u32,
    /// 字体族 id（0 = 系统默认）。
    pub face: u16,
}

impl Default for FontDomain {
    fn default() -> Self {
        FontDomain { base_px: 14, face: 0 }
    }
}

/// 图标域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IconDomain {
    /// 蒙版风格（0=原样 1=圆 2=圆角方 3=泪滴）。
    pub mask: u8,
}

impl Default for IconDomain {
    fn default() -> Self {
        IconDomain { mask: 2 }
    }
}

/// 动效域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MotionDomain {
    /// 动效开关。
    pub enabled: bool,
    /// 时长档（0=快 1=标准 2=平缓）。
    pub tier: u8,
}

impl Default for MotionDomain {
    fn default() -> Self {
        MotionDomain { enabled: true, tier: 1 }
    }
}

/// 五域档案。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PersonaDomains {
    pub color: ColorDomain,
    pub sound: SoundDomain,
    pub font: FontDomain,
    pub icon: IconDomain,
    pub motion: MotionDomain,
}

/// Persona 档案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersonaProfile {
    pub version: u32,
    pub domains: PersonaDomains,
}

impl PersonaProfile {
    /// 当前版本默认档案。
    pub fn default_profile() -> Self {
        PersonaProfile {
            version: PERSONA_VERSION,
            domains: PersonaDomains::default(),
        }
    }

    /// 消毒：逐域钳制 + 记录。
    pub fn sanitize(&mut self) -> Vec<PersonaCode> {
        let mut out = Vec::new();
        let d = &mut self.domains;
        if d.sound.volume > 1000 {
            d.sound.volume = 1000;
            out.push(PersonaCode::DomainClamped);
        }
        if d.font.base_px < super::ver09g_fonts::MIN_READABLE_PX {
            d.font.base_px = super::ver09g_fonts::MIN_READABLE_PX;
            out.push(PersonaCode::DomainClamped);
        }
        if d.icon.mask > 3 {
            d.icon.mask = 2;
            out.push(PersonaCode::DomainClamped);
        }
        if d.motion.tier > 2 {
            d.motion.tier = 1;
            out.push(PersonaCode::DomainClamped);
        }
        if !out.is_empty() {
            out.push(PersonaCode::Sanitized);
        }
        out
    }

    /// 开放导出（稳定键值文本——格式公开，不锁用户）。
    pub fn export(&self) -> String {
        use alloc::format;
        let d = &self.domains;
        alloc::format!(
            "persona_version={}\ncolor_accent={:#08x}\ncolor_dark={}\nsound_volume={}\nsound_pack={}\nfont_base_px={}\nfont_face={}\nicon_mask={}\nmotion_enabled={}\nmotion_tier={}\n",
            self.version,
            d.color.accent,
            d.color.dark as u8,
            d.sound.volume,
            d.sound.pack,
            d.font.base_px,
            d.font.face,
            d.icon.mask,
            d.motion.enabled as u8,
            d.motion.tier,
        )
    }

    /// 导入（解析 + 版本迁移 + 消毒）。坏行跳过（部分导入）。
    pub fn import(text: &str) -> Result<(Self, Vec<PersonaCode>), PersonaCode> {
        let mut p = PersonaProfile::default_profile();
        let mut seen_version = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "persona_version" => seen_version = v.parse().ok(),
                "color_accent" => {
                    let h = v.trim_start_matches("0x").trim_start_matches("#");
                    if let Ok(x) = u32::from_str_radix(h, 16) {
                        p.domains.color.accent = x & 0x00FF_FFFF;
                    }
                }
                "color_dark" => p.domains.color.dark = v == "1",
                "sound_volume" => p.domains.sound.volume = v.parse().unwrap_or(800),
                "sound_pack" => p.domains.sound.pack = v.parse().unwrap_or(0),
                "font_base_px" => p.domains.font.base_px = v.parse().unwrap_or(14),
                "font_face" => p.domains.font.face = v.parse().unwrap_or(0),
                "icon_mask" => p.domains.icon.mask = v.parse().unwrap_or(2),
                "motion_enabled" => p.domains.motion.enabled = v == "1",
                "motion_tier" => p.domains.motion.tier = v.parse().unwrap_or(1),
                _ => {}
            }
        }
        let v = match seen_version {
            None => return Err(PersonaCode::VersionTooOld),
            Some(v) => migrate(v)?,
        };
        p.version = v;
        let codes = p.sanitize();
        Ok((p, codes))
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate() {
        assert_eq!(migrate(99), Err(PersonaCode::VersionTooNew));
        assert_eq!(migrate(0), Err(PersonaCode::VersionTooOld));
        assert_eq!(migrate(2).unwrap(), PERSONA_VERSION);
    }

    #[test]
    fn sanitize_clamps() {
        let mut p = PersonaProfile::default_profile();
        p.domains.sound.volume = 5000;
        p.domains.font.base_px = 4;
        p.domains.icon.mask = 9;
        let codes = p.sanitize();
        assert!(codes.contains(&PersonaCode::Sanitized));
        assert_eq!(p.domains.sound.volume, 1000);
        assert_eq!(p.domains.font.base_px, super::ver09g_fonts::MIN_READABLE_PX);
        assert_eq!(p.domains.icon.mask, 2);
    }

    #[test]
    fn export_import_roundtrip() {
        let mut p = PersonaProfile::default_profile();
        p.domains.color.accent = 0x123456;
        p.domains.sound.volume = 700;
        let text = p.export();
        let (back, _) = PersonaProfile::import(&text).unwrap();
        assert_eq!(back.domains.color.accent, 0x123456);
        assert_eq!(back.domains.sound.volume, 700);
    }

    #[test]
    fn future_version_rejected() {
        let text = "persona_version=99\n";
        assert_eq!(
            PersonaProfile::import(text),
            Err(PersonaCode::VersionTooNew)
        );
    }
}
