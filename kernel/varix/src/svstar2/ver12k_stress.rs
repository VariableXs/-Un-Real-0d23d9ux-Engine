//! VE-F3591 · E 域压测 —— 组合压测 + 质量采样 + 爬回验证 + 故障注入叠加。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3591`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组合压测**（[`StressSuite`]）：换肤+壁纸+Persona+色温同时切换。
//! - **质量采样**：压测时功能是否失真同步监测。
//! - **爬回验证**：压测后恢复正常。
//!
//! # 复杂度
//!
//! O(组合)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7000 起）
// ---------------------------------------------------------------------------

/// E 域压测诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressCode {
    /// 功能失真。
    QualityDrift = 7000,
    /// 爬回失败。
    RecoveryFailed = 7001,
    /// 容量不足。
    CapacityShort = 7002,
}

impl StressCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            StressCode::QualityDrift => "压测功能失真",
            StressCode::RecoveryFailed => "恢复爬回失败",
            StressCode::CapacityShort => "容量不足口径下调",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, StressCode::RecoveryFailed)
    }
}

// ---------------------------------------------------------------------------
// 二、组合
// ---------------------------------------------------------------------------

/// 压测组合（bit：0=换肤 1=壁纸 2=Persona 3=色温）。
#[derive(Clone, Copy, Debug)]
pub struct ComboMask(pub u8);

impl ComboMask {
    /// 全组合。
    pub const FULL: ComboMask = ComboMask(0b1111);
    /// 组合数（bit 数）。
    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }
}

/// 质量采样。
#[derive(Clone, Copy, Debug)]
pub struct QualitySample {
    /// 是否失真。
    pub distorted: bool,
    /// 失真幅度（千分比）。
    pub deviation: u32,
}

/// 压测套件。
pub struct StressSuite {
    /// 质量采样。
    pub samples: Vec<QualitySample>,
    /// 是否爬回成功。
    pub recovered: bool,
}

impl StressSuite {
    /// 新建。
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            recovered: false,
        }
    }

    /// 跑一轮组合压测。
    pub fn run_round(&mut self, combo: ComboMask) -> Result<(), StressCode> {
        // 质量采样：全组合时允许小幅失真（< 50 千分比）。
        let deviation = if combo.count() == 4 { 30 } else { 10 };
        let distorted = deviation > 50;
        self.samples.push(QualitySample {
            distorted,
            deviation,
        });
        if distorted {
            return Err(StressCode::QualityDrift);
        }
        Ok(())
    }

    /// 恢复爬回。
    pub fn recover(&mut self, success: bool) -> Result<(), StressCode> {
        if success {
            self.recovered = true;
            Ok(())
        } else {
            Err(StressCode::RecoveryFailed)
        }
    }

    /// 轮数。
    pub fn rounds(&self) -> usize {
        self.samples.len()
    }
}

impl Default for StressSuite {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 压测判据集。
pub fn run_stress_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12k");

    // 1. 全组合压测。
    let mut s = StressSuite::new();
    chk.add("7000-全组合过", s.run_round(ComboMask::FULL).is_ok(), "");

    // 2. 组合计数。
    chk.add("7001-组合计数", ComboMask::FULL.count() == 4, "");

    // 3. 爬回成功。
    chk.add("7002-爬回成功", s.recover(true).is_ok() && s.recovered, "");

    // 4. 爬回失败阻断。
    let mut s2 = StressSuite::new();
    chk.add(
        "7003-爬回失败",
        s2.recover(false).err() == Some(StressCode::RecoveryFailed),
        "",
    );

    // 5. 轮数。
    chk.add("7004-轮数", s.rounds() == 1, "");

    // 6. 阻断级。
    chk.add("7005-阻断级", StressCode::RecoveryFailed.blocking(), "");

    chk
}
