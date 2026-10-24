//! VE-F3568 · 无障碍全域扫描 —— 四类扫描 + 逐 UI 遍历 + CI 门禁 + 兜底遍历。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3568`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四类扫描**（[`A11yCheck`]）：对比度 / 焦点环 / 触达尺寸 /
//!   读屏标注四类逐 UI 遍历。
//! - **CI 门禁**：不达标的 UI 进不了主干（[`CiGate`]）。
//! - **兜底遍历**：没扫到的 UI 列清单（覆盖率断言）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 扫描漏 UI | 兜底遍历 |
//! | CI 失败 | 告警 |
//! | 误报 | 调阈留痕 |
//!
//! # 复杂度
//!
//! O(UI 数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6540 起）
// ---------------------------------------------------------------------------

/// 无障碍扫描诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yCode {
    /// 对比度不达标。
    ContrastFail = 6540,
    /// 焦点环缺失。
    FocusRingMissing = 6541,
    /// 触达尺寸不足。
    TouchTargetTooSmall = 6542,
    /// 读屏标注缺失。
    LabelMissing = 6543,
}

impl A11yCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            A11yCode::ContrastFail => "对比度不达标",
            A11yCode::FocusRingMissing => "焦点环缺失",
            A11yCode::TouchTargetTooSmall => "触达尺寸不足",
            A11yCode::LabelMissing => "读屏标注缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            A11yCode::ContrastFail | A11yCode::FocusRingMissing | A11yCode::LabelMissing
        )
    }
}

// ---------------------------------------------------------------------------
// 二、四类检查
// ---------------------------------------------------------------------------

/// 四类检查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yCheck {
    /// 对比度。
    Contrast,
    /// 焦点环。
    FocusRing,
    /// 触达尺寸。
    TouchTarget,
    /// 读屏标注。
    Label,
}

impl A11yCheck {
    /// 全部四类。
    pub const ALL: [A11yCheck; 4] = [
        A11yCheck::Contrast,
        A11yCheck::FocusRing,
        A11yCheck::TouchTarget,
        A11yCheck::Label,
    ];

    /// 类短名。
    pub const fn label(self) -> &'static str {
        match self {
            A11yCheck::Contrast => "对比度",
            A11yCheck::FocusRing => "焦点环",
            A11yCheck::TouchTarget => "触达尺寸",
            A11yCheck::Label => "读屏标注",
        }
    }
}

/// UI 项的四维达标状态（bit 掩码：0b1111=全过）。
#[derive(Clone, Copy, Debug)]
pub struct UiA11yStatus {
    /// UI id。
    pub ui_id: u32,
    /// 达标掩码。
    pub pass_mask: u8,
}

// ---------------------------------------------------------------------------
// 三、扫描器与 CI 门禁
// ---------------------------------------------------------------------------

/// 全域扫描器。
pub struct A11yScanner {
    scanned: Vec<u32>,
    /// 兜底补扫数。
    pub fallback_scans: u32,
}

impl A11yScanner {
    /// 新建。
    pub fn new() -> Self {
        Self {
            scanned: Vec::new(),
            fallback_scans: 0,
        }
    }

    /// 扫描一个 UI（四类判定）。
    pub fn scan_ui(&mut self, status: UiA11yStatus) -> Result<(), A11yCode> {
        self.scanned.push(status.ui_id);
        if status.pass_mask & 1 == 0 {
            return Err(A11yCode::ContrastFail);
        }
        if status.pass_mask & 2 == 0 {
            return Err(A11yCode::FocusRingMissing);
        }
        if status.pass_mask & 4 == 0 {
            return Err(A11yCode::TouchTargetTooSmall);
        }
        if status.pass_mask & 8 == 0 {
            return Err(A11yCode::LabelMissing);
        }
        Ok(())
    }

    /// 兜底遍历：对照期望 UI 全集，列出未扫到的。
    pub fn fallback_coverage(&mut self, expected: &[u32]) -> Vec<u32> {
        let missing: Vec<u32> = expected
            .iter()
            .filter(|id| !self.scanned.contains(id))
            .copied()
            .collect();
        if !missing.is_empty() {
            self.fallback_scans = self.fallback_scans.saturating_add(1);
        }
        missing
    }

    /// 已扫数。
    pub fn scanned_count(&self) -> usize {
        self.scanned.len()
    }
}

impl Default for A11yScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// CI 门禁。
pub struct CiGate;

impl CiGate {
    /// 全达标才过门。
    pub fn evaluate(statuses: &[UiA11yStatus]) -> bool {
        statuses.iter().all(|s| s.pass_mask == 0b1111)
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 无障碍扫描判据集。
pub fn run_a11y_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11h");

    // 1. 四类全过。
    let mut s = A11yScanner::new();
    let all_pass = UiA11yStatus {
        ui_id: 1,
        pass_mask: 0b1111,
    };
    chk.add("6540-四类全过", s.scan_ui(all_pass).is_ok(), "");

    // 2. 对比度缺失拦。
    let no_contrast = UiA11yStatus {
        ui_id: 2,
        pass_mask: 0b1110,
    };
    chk.add(
        "6541-对比度拦",
        s.scan_ui(no_contrast).err() == Some(A11yCode::ContrastFail),
        "",
    );

    // 3. 读屏标注缺失拦。
    let no_label = UiA11yStatus {
        ui_id: 3,
        pass_mask: 0b0111,
    };
    chk.add(
        "6542-标注拦",
        s.scan_ui(no_label).err() == Some(A11yCode::LabelMissing),
        "",
    );

    // 4. 兜底遍历列出遗漏。
    let missing = s.fallback_coverage(&[1, 2, 3, 99]);
    chk.add("6543-兜底遍历", missing.len() == 1 && missing[0] == 99, "");

    // 5. CI 门禁。
    let mixed = [
        all_pass,
        UiA11yStatus {
            ui_id: 2,
            pass_mask: 0b1111,
        },
    ];
    let bad = [all_pass, no_label];
    chk.add("6544-CI门禁", CiGate::evaluate(&mixed) && !CiGate::evaluate(&bad), "");

    // 6. 类名可达。
    chk.add(
        "6545-类名可达",
        A11yCheck::TouchTarget.label() == "触达尺寸",
        "",
    );

    chk
}
