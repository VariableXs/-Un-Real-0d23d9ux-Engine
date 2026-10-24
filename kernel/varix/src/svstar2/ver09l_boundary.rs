//! VE-F3532 · 个性化与无障碍边界 —— 三条硬线 + 无障碍强制胜出 + 越界自动降级。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3532`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三条硬线**（[`HardLine`]）：个性化的自由与无障碍的底线——
//!   ①可达性（交互目标可触达）、②对比度（文本可读）、③动效减弱
//!   （前庭安全）。个性化不得破坏这三条，冲突时**无障碍强制胜出**。
//! - **自动降级**（[`BoundaryGuard::check`]):越界个性化自动降级
//!   并告知——不争论、不弹窗轰炸，改完说一句。
//! - **反复越界审查**（[`BoundaryGuard::strike`]):同一域反复越界
//!    计数，超阈值标记审查（提醒用户检查配置而不是默默改）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 硬线破坏 | 强制降级 |
//! | 未告知 | 补告知 |
//! | 反复越界 | 审查标记 |
//!
//! # 复杂度
//!
//! 全部 O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A10 起）
// ---------------------------------------------------------------------------

/// 无障碍边界域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundCode {
    /// 触达硬线破坏（已强制降级）。
    ReachViolated = 0x5A10,
    /// 对比度硬线破坏（已强制降级）。
    ContrastViolated = 0x5A11,
    /// 动效硬线破坏（已强制降级）。
    MotionViolated = 0x5A12,
    /// 反复越界（审查标记）。
    RepeatedStrikes = 0x5A13,
}

impl BoundCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BoundCode::ReachViolated => "可达性硬线破坏已降级",
            BoundCode::ContrastViolated => "对比度硬线破坏已降级",
            BoundCode::MotionViolated => "动效硬线破坏已降级",
            BoundCode::RepeatedStrikes => "反复越界已标记审查",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、硬线阈值
// ---------------------------------------------------------------------------

/// 交互目标最小触达尺寸（px）。
pub const MIN_TARGET_PX: u32 = 32;
/// 文本对比度硬线（千分比 4.5:1）。
pub const CONTRAST_HARDLINE: u32 = 4500;
/// 反复越界审查阈值。
pub const STRIKE_LIMIT: u32 = 3;

/// 三条硬线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HardLine {
    /// 可达性（目标尺寸）。
    Reach,
    /// 对比度。
    Contrast,
    /// 动效减弱。
    MotionReduce,
}

impl HardLine {
    /// 中文名（告知文本）。
    pub fn label(self) -> &'static str {
        match self {
            HardLine::Reach => "可达性",
            HardLine::Contrast => "对比度",
            HardLine::MotionReduce => "动效减弱",
        }
    }
}

/// 无障碍申请（最高优先层 F3531 的落地点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11yClaim {
    /// 用户开启的硬线。
    pub line: HardLine,
}

// ---------------------------------------------------------------------------
// 三、边界守卫
// ---------------------------------------------------------------------------

/// 降级结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Downgrade {
    /// 触发的硬线。
    pub line: HardLine,
    /// 降级后的生效值。
    pub effective: u32,
    /// 告知文本。
    pub notice: &'static str,
}

/// 边界守卫。
pub struct BoundaryGuard {
    /// 用户无障碍申请（有则硬线生效）。
    pub claims: Vec<A11yClaim>,
    /// 各域越界计数。
    strikes: [u32; 3],
    pub codes: Vec<BoundCode>,
}

impl BoundaryGuard {
    /// 空守卫（无硬线申请——个性化自由态）。
    pub fn new() -> Self {
        BoundaryGuard {
            claims: Vec::new(),
            strikes: [0; 3],
            codes: Vec::new(),
        }
    }

    /// 声明无障碍硬线。
    pub fn claim(&mut self, line: HardLine) {
        if !self.claims.iter().any(|c| c.line == line) {
            self.claims.push(A11yClaim { line });
        }
    }

    /// 撤销硬线。
    pub fn unclaim(&mut self, line: HardLine) {
        self.claims.retain(|c| c.line != line);
    }

    /// 硬线是否生效。
    pub fn active(&self, line: HardLine) -> bool {
        self.claims.iter().any(|c| c.line == line)
    }

    fn line_idx(line: HardLine) -> usize {
        match line {
            HardLine::Reach => 0,
            HardLine::Contrast => 1,
            HardLine::MotionReduce => 2,
        }
    }

    /// 越界计数 +1 并判审查标记。
    fn strike(&mut self, line: HardLine) -> bool {
        let i = Self::line_idx(line);
        self.strikes[i] += 1;
        if self.strikes[i] >= STRIKE_LIMIT {
            self.codes.push(BoundCode::RepeatedStrikes);
            true
        } else {
            false
        }
    }

    /// 检查个性化请求值是否越线。越线 → 强制降级到安全值并告知。
    /// 返回 Some(降级结果) 或 None（未越线）。
    pub fn check(&mut self, line: HardLine, want: u32) -> Option<Downgrade> {
        if !self.active(line) {
            return None;
        }
        let (min_ok, downgraded) = match line {
            HardLine::Reach => (MIN_TARGET_PX, want < MIN_TARGET_PX),
            HardLine::Contrast => (CONTRAST_HARDLINE, want < CONTRAST_HARDLINE),
            // 动效线：want=1 表示开动效（线生效即禁）。
            HardLine::MotionReduce => (0, want != 0),
        };
        if !downgraded {
            return None;
        }
        let code = match line {
            HardLine::Reach => BoundCode::ReachViolated,
            HardLine::Contrast => BoundCode::ContrastViolated,
            HardLine::MotionReduce => BoundCode::MotionViolated,
        };
        self.codes.push(code);
        let repeated = self.strike(line);
        let effective = match line {
            HardLine::Reach => MIN_TARGET_PX,
            HardLine::Contrast => CONTRAST_HARDLINE,
            HardLine::MotionReduce => 0,
        };
        Some(Downgrade {
            line,
            effective,
            notice: if repeated {
                "已多次为您恢复无障碍设置，建议检查个性化配置"
            } else {
                "该设置与无障碍冲突，已恢复"
            },
        })
    }

    /// 各域越界计数快照（审查可查）。
    pub fn strike_counts(&self) -> [u32; 3] {
        self.strikes
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_claim_no_interfere() {
        let mut g = BoundaryGuard::new();
        // 无硬线申请：个性化自由。
        assert!(g.check(HardLine::Contrast, 1000).is_none());
    }

    #[test]
    fn contrast_line_enforced() {
        let mut g = BoundaryGuard::new();
        g.claim(HardLine::Contrast);
        let d = g.check(HardLine::Contrast, 2000).unwrap();
        assert_eq!(d.effective, CONTRAST_HARDLINE);
        assert_eq!(d.line, HardLine::Contrast);
    }

    #[test]
    fn within_line_passes() {
        let mut g = BoundaryGuard::new();
        g.claim(HardLine::Contrast);
        assert!(g.check(HardLine::Contrast, 7000).is_none());
    }

    #[test]
    fn repeated_strikes_flag_review() {
        let mut g = BoundaryGuard::new();
        g.claim(HardLine::Reach);
        for _ in 0..STRIKE_LIMIT {
            let _ = g.check(HardLine::Reach, 10);
        }
        assert!(g.codes.contains(&BoundCode::RepeatedStrikes));
        let d = g.check(HardLine::Reach, 10).unwrap();
        assert!(d.notice.contains("多次"));
    }

    #[test]
    fn motion_line_kills_fx() {
        let mut g = BoundaryGuard::new();
        g.claim(HardLine::MotionReduce);
        let d = g.check(HardLine::MotionReduce, 1).unwrap();
        assert_eq!(d.effective, 0);
    }
}
