//! VE-F3566 · 声音一致性审查 —— 三线审查（音量归一/触发时机/风格族）+ 违规登记。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3566`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三线审查**（[`SoundAuditor`]）：音量归一 / 触发时机 / 风格族
//!   三线对照 AB 域规范。
//! - **违规登记**：三线任一不过登记违规，不静默。
//! - **AB 域对拍**：审查与音效注册表对拍（一致性基线）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 违规 | 登记 |
//! | 规范漂移 | 版本锁定 |
//! | 审查失效 | 自检 |
//!
//! # 复杂度
//!
//! O(音效)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6500 起）
// ---------------------------------------------------------------------------

/// 声音审查诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundCode {
    /// 音量未归一。
    VolumeNotNormalized = 6500,
    /// 触发时机违例。
    TimingViolation = 6501,
    /// 风格族混用。
    StyleFamilyMix = 6502,
    /// 规范版本漂移。
    SpecDrift = 6503,
}

impl SoundCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SoundCode::VolumeNotNormalized => "音量未归一",
            SoundCode::TimingViolation => "触发时机违例",
            SoundCode::StyleFamilyMix => "风格族混用",
            SoundCode::SpecDrift => "声音规范版本漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、规范
// ---------------------------------------------------------------------------

/// 声音规范版本。
pub const SOUND_SPEC_VERSION: u32 = 1;

/// 音量归一目标（千分比）。
pub const VOLUME_TARGET: u32 = 500;

/// 音量容差（千分比）。
pub const VOLUME_TOLERANCE: u32 = 50;

// ---------------------------------------------------------------------------
// 三、音效项与审查器
// ---------------------------------------------------------------------------

/// 音效项。
#[derive(Clone, Copy, Debug)]
pub struct SoundEntry {
    /// 音效 id。
    pub id: u32,
    /// 音量（千分比）。
    pub volume: u32,
    /// 触发时机 id（0=加载 1=切换 2=错误 3=通知）。
    pub trigger: u8,
    /// 风格族 id。
    pub family: u32,
}

/// 审查器。
pub struct SoundAuditor {
    version: u32,
    /// 违规计数。
    pub violations: u32,
}

impl SoundAuditor {
    /// 新建（版本锁定）。
    pub fn new(version: u32) -> Result<Self, SoundCode> {
        if version != SOUND_SPEC_VERSION {
            return Err(SoundCode::SpecDrift);
        }
        Ok(Self {
            version,
            violations: 0,
        })
    }

    /// 三线审查。
    pub fn audit(&mut self, e: &SoundEntry) -> Result<(), SoundCode> {
        // 线一：音量归一。
        let d = if e.volume > VOLUME_TARGET {
            e.volume - VOLUME_TARGET
        } else {
            VOLUME_TARGET - e.volume
        };
        if d > VOLUME_TOLERANCE {
            self.violations = self.violations.saturating_add(1);
            return Err(SoundCode::VolumeNotNormalized);
        }
        // 线二：触发时机合法（0..=3）。
        if e.trigger > 3 {
            self.violations = self.violations.saturating_add(1);
            return Err(SoundCode::TimingViolation);
        }
        // 线三：风格族一致（全注册表同族——此处单条检查族 id 合法）。
        if e.family == 0 {
            self.violations = self.violations.saturating_add(1);
            return Err(SoundCode::StyleFamilyMix);
        }
        Ok(())
    }

    /// 版本可查。
    pub fn version(&self) -> u32 {
        self.version
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 声音审查判据集。
pub fn run_sound_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11f");

    // 1. 版本锁定。
    chk.add("6500-版本锁定", SoundAuditor::new(SOUND_SPEC_VERSION).is_ok() && SoundAuditor::new(9).is_err(), "");

    let good = SoundEntry {
        id: 1,
        volume: 500,
        trigger: 1,
        family: 1,
    };

    // 2. 合规过审。
    let mut a = SoundAuditor::new(SOUND_SPEC_VERSION).ok().unwrap_or(SoundAuditor {
        version: SOUND_SPEC_VERSION,
        violations: 0,
    });
    chk.add("6501-合规过", a.audit(&good).is_ok(), "");

    // 3. 音量越界。
    let loud = SoundEntry {
        volume: 900,
        ..good
    };
    chk.add(
        "6502-音量违例",
        a.audit(&loud).err() == Some(SoundCode::VolumeNotNormalized),
        "",
    );

    // 4. 时机违例。
    let bad_trigger = SoundEntry {
        trigger: 9,
        ..good
    };
    chk.add(
        "6503-时机违例",
        a.audit(&bad_trigger).err() == Some(SoundCode::TimingViolation),
        "",
    );

    // 5. 风格族。
    let no_family = SoundEntry {
        family: 0,
        ..good
    };
    chk.add(
        "6504-风格族",
        a.audit(&no_family).err() == Some(SoundCode::StyleFamilyMix),
        "",
    );

    // 6. 违规计数。
    chk.add("6505-违规计数", a.violations == 3, "");

    chk
}
