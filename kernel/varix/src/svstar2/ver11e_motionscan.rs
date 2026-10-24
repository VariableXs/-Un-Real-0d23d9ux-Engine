//! VE-F3565 · 动效一致性扫描 —— 参数扫描 + 单源核验 + 越界登记 + 私写标记。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3565`
//!
//! # 职责（锚点原文拆解）
//!
//! - **参数扫描**（[`MotionScanner`]）：时长 / 缓动 / 幅度与
//!   Z05 规范逐项对照。
//! - **单源核验**（[`EasingSingleSource`]）：缓动函数唯一实现
//!   核验——别处不许私写缓动（Z05 单源延续）。
//! - **越界登记 + 私写标记**：违规登记，私写缓动标记替换。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 私写缓动 | 标记替换 |
//! | 时长越界 | 登记 |
//! | 扫描失效 | 自检 |
//!
//! # 复杂度
//!
//! O(动效)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6480 起）
// ---------------------------------------------------------------------------

/// 动效扫描诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotionCode {
    /// 时长越界。
    DurationOutOfRange = 6480,
    /// 私写缓动。
    PrivateEasing = 6481,
    /// 幅度越界。
    AmplitudeOutOfRange = 6482,
}

impl MotionCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MotionCode::DurationOutOfRange => "动效时长越界",
            MotionCode::PrivateEasing => "私写缓动已标记",
            MotionCode::AmplitudeOutOfRange => "动效幅度越界",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, MotionCode::PrivateEasing)
    }
}

// ---------------------------------------------------------------------------
// 二、规范约束
// ---------------------------------------------------------------------------

/// 时长上限（毫秒，Z05 规范）。
pub const DURATION_MAX_MS: u32 = 1000;
/// 时长下限。
pub const DURATION_MIN_MS: u32 = 50;
/// 幅度上限（千分比）。
pub const AMPLITUDE_MAX: u32 = 500;

/// 标准缓动 id（单源）。
pub const STANDARD_EASING_ID: u32 = 1;

// ---------------------------------------------------------------------------
// 三、单源核验
// ---------------------------------------------------------------------------

/// 缓动单源核验器。
pub struct EasingSingleSource {
    /// 私写缓动计数。
    pub private_writes: u32,
}

impl EasingSingleSource {
    /// 新建。
    pub fn new() -> Self {
        Self {
            private_writes: 0,
        }
    }

    /// 核验一处缓动实现来源。
    pub fn verify(&mut self, easing_id: u32) -> Result<(), MotionCode> {
        if easing_id != STANDARD_EASING_ID {
            self.private_writes = self.private_writes.saturating_add(1);
            return Err(MotionCode::PrivateEasing);
        }
        Ok(())
    }
}

impl Default for EasingSingleSource {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、扫描器
// ---------------------------------------------------------------------------

/// 动效参数。
#[derive(Clone, Copy, Debug)]
pub struct MotionParams {
    /// 时长（毫秒）。
    pub duration_ms: u32,
    /// 缓动 id。
    pub easing_id: u32,
    /// 幅度（千分比）。
    pub amplitude: u32,
}

/// 扫描结果。
#[derive(Clone, Copy, Debug)]
pub struct ScanHit {
    /// 违规码。
    pub code: MotionCode,
}

/// 动效扫描器。
pub struct MotionScanner {
    easing: EasingSingleSource,
    /// 越界登记数。
    pub violations: u32,
}

impl MotionScanner {
    /// 新建。
    pub fn new() -> Self {
        Self {
            easing: EasingSingleSource::new(),
            violations: 0,
        }
    }

    /// 扫描一组动效参数。
    pub fn scan(&mut self, p: &MotionParams) -> Result<(), MotionCode> {
        // 缓动单源。
        self.easing.verify(p.easing_id)?;
        // 时长边界。
        if p.duration_ms > DURATION_MAX_MS || p.duration_ms < DURATION_MIN_MS {
            self.violations = self.violations.saturating_add(1);
            return Err(MotionCode::DurationOutOfRange);
        }
        // 幅度边界。
        if p.amplitude > AMPLITUDE_MAX {
            self.violations = self.violations.saturating_add(1);
            return Err(MotionCode::AmplitudeOutOfRange);
        }
        Ok(())
    }

    /// 私写计数。
    pub fn private_count(&self) -> u32 {
        self.easing.private_writes
    }
}

impl Default for MotionScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 动效扫描判据集。
pub fn run_motion_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11e");

    let good = MotionParams {
        duration_ms: 300,
        easing_id: STANDARD_EASING_ID,
        amplitude: 200,
    };

    // 1. 合规参数过。
    let mut s = MotionScanner::new();
    chk.add("6480-合规通过", s.scan(&good).is_ok(), "");

    // 2. 私写缓动拦截。
    let priv_ease = MotionParams {
        easing_id: 99,
        ..good
    };
    let mut s2 = MotionScanner::new();
    let hit = s2.scan(&priv_ease).err() == Some(MotionCode::PrivateEasing);
    chk.add("6481-私写标记", hit && s2.private_count() == 1, "");

    // 3. 时长越界。
    let too_long = MotionParams {
        duration_ms: 5000,
        ..good
    };
    let mut s3 = MotionScanner::new();
    chk.add(
        "6482-时长越界",
        s3.scan(&too_long).err() == Some(MotionCode::DurationOutOfRange),
        "",
    );

    // 4. 幅度越界。
    let too_much = MotionParams {
        amplitude: 999,
        ..good
    };
    let mut s4 = MotionScanner::new();
    chk.add(
        "6483-幅度越界",
        s4.scan(&too_much).err() == Some(MotionCode::AmplitudeOutOfRange),
        "",
    );

    // 5. 常量正确。
    chk.add(
        "6484-规范常量",
        DURATION_MAX_MS == 1000 && DURATION_MIN_MS == 50 && AMPLITUDE_MAX == 500,
        "",
    );

    chk
}
