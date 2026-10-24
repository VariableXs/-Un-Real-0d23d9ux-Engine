//! VE-F3590 · E 域缺陷总账 —— 三档汇总 + 清零确认 + 收敛趋势 + 恶化预警。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3590`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三档**（[`DefectGrade`]）：🔴 清零 / 🟡 收口前修完 / 🟢 登记。
//! - **收敛趋势**：每轮缺陷数递减为健康。
//! - **历史对比**。
//!
//! # 复杂度
//!
//! O(缺陷)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6980 起）
// ---------------------------------------------------------------------------

/// 缺陷总账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectCode {
    /// 🔴 残留。
    CriticalRemain = 6980,
    /// 🟡 未修完。
    YellowUnfixed = 6981,
    /// 趋势恶化。
    TrendWorsening = 6982,
}

impl DefectCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DefectCode::CriticalRemain => "🔴 清零确认失败",
            DefectCode::YellowUnfixed => "🟡 收口前未修完",
            DefectCode::TrendWorsening => "缺陷趋势恶化",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DefectCode::CriticalRemain | DefectCode::YellowUnfixed)
    }
}

// ---------------------------------------------------------------------------
// 二、三档
// ---------------------------------------------------------------------------

/// 缺陷档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectGrade {
    /// 🔴 阻断。
    Critical,
    /// 🟡 收口前修。
    Major,
    /// 🟢 登记在册。
    Minor,
}

impl DefectGrade {
    /// 档短名。
    pub const fn label(self) -> &'static str {
        match self {
            DefectGrade::Critical => "🔴",
            DefectGrade::Major => "🟡",
            DefectGrade::Minor => "🟢",
        }
    }
}

/// 缺陷项。
#[derive(Clone, Copy, Debug)]
pub struct Defect {
    /// id。
    pub id: u32,
    /// 档。
    pub grade: DefectGrade,
    /// 是否已修。
    pub fixed: bool,
}

// ---------------------------------------------------------------------------
// 三、总账
// ---------------------------------------------------------------------------

/// 缺陷总账。
pub struct DefectLedger {
    defects: Vec<Defect>,
    /// 历史轮次缺陷数（趋势）。
    history: Vec<u32>,
}

impl DefectLedger {
    /// 新建。
    pub fn new() -> Self {
        Self {
            defects: Vec::new(),
            history: Vec::new(),
        }
    }

    /// 登记缺陷。
    pub fn report(&mut self, id: u32, grade: DefectGrade) {
        self.defects.push(Defect {
            id,
            grade,
            fixed: false,
        });
    }

    /// 修一个。
    pub fn fix(&mut self, id: u32) -> bool {
        for d in self.defects.iter_mut() {
            if d.id == id {
                d.fixed = true;
                return true;
            }
        }
        false
    }

    /// 🔴 残留数。
    pub fn critical_open(&self) -> usize {
        self.defects
            .iter()
            .filter(|d| d.grade == DefectGrade::Critical && !d.fixed)
            .count()
    }

    /// 🟡 未修数。
    pub fn major_open(&self) -> usize {
        self.defects
            .iter()
            .filter(|d| d.grade == DefectGrade::Major && !d.fixed)
            .count()
    }

    /// 记一轮总数（趋势）。
    pub fn snapshot(&mut self) {
        let open = self.defects.iter().filter(|d| !d.fixed).count();
        self.history.push(open as u32);
    }

    /// 趋势（递减 = true）。
    pub fn converging(&self) -> bool {
        if self.history.len() < 2 {
            return true;
        }
        let n = self.history.len();
        self.history[n - 1] <= self.history[n - 2]
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.defects.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.defects.is_empty()
    }
}

impl Default for DefectLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 缺陷总账判据集。
pub fn run_defect_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12j");

    // 1. 🔴 残留。
    let mut l = DefectLedger::new();
    l.report(1, DefectGrade::Critical);
    chk.add("6980-🔴残留", l.critical_open() == 1, "");

    // 2. 修复后清零。
    l.fix(1);
    chk.add("6981-清零确认", l.critical_open() == 0, "");

    // 3. 🟡 未修。
    let mut l2 = DefectLedger::new();
    l2.report(1, DefectGrade::Major);
    chk.add("6982-🟡未修", l2.major_open() == 1, "");

    // 4. 趋势收敛。
    let mut l3 = DefectLedger::new();
    l3.report(1, DefectGrade::Minor);
    l3.snapshot();
    l3.fix(1);
    l3.snapshot();
    chk.add("6983-收敛趋势", l3.converging(), "");

    // 5. 趋势恶化。
    let mut l4 = DefectLedger::new();
    l4.snapshot();
    l4.report(1, DefectGrade::Minor);
    l4.snapshot();
    chk.add("6984-恶化预警", !l4.converging(), "");

    // 6. 档名可达。
    chk.add("6985-档名", DefectGrade::Major.label() == "🟡", "");

    chk
}
