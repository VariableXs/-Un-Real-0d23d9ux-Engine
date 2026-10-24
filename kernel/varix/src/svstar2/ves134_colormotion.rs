//! VE-F3834 · 色彩适配与 M 域联动
//!
//! 高对比切换过渡动效：F2889 原子事务复用。过渡帧对比度保障（全帧扫描断言 P1）。
//! reduce 协同正交。动效替代协同复用。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB6xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB6Code(pub u16);

impl CB6Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const TRANSITION_BELOW_GATE: Self = Self(SEG_BASE + 1);
    pub const SWITCH_GLITCH: Self = Self(SEG_BASE + 2);
    pub const ORTHO_MIXED: Self = Self(SEG_BASE + 3);
    pub const REPLACEMENT_LOST: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::TRANSITION_BELOW_GATE => "过渡破门（P1 红线实测）：过渡帧对比度不足",
            Self::SWITCH_GLITCH => "切换闪断，事务复用红线",
            Self::ORTHO_MIXED => "正交混，需修正",
            Self::REPLACEMENT_LOST => "替代失联，需单源",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 过渡帧
#[derive(Debug, Clone, Copy)]
pub struct TransitionFrame {
    pub index: u16,
    pub contrast: u16,
    pub threshold: u16,
}

/// 切换事务
#[derive(Debug, Clone, Copy)]
pub struct SwitchTransaction {
    pub atomic: bool,
    pub committed: bool,
}

/// 联动配置
#[derive(Debug, Clone)]
pub struct MLinkage {
    pub reduce_orthogonal: bool,
    pub replacement_active: bool,
    pub frames: Vec<TransitionFrame>,
    pub transaction: SwitchTransaction,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct ColorMotionLink {
    config: MLinkage,
}

impl ColorMotionLink {
    pub fn new(config: MLinkage) -> Self {
        Self { config }
    }

    /// 过渡全帧断言 P1
    pub fn assert_transition_frames(&self) -> Result<(), CB6Code> {
        for f in &self.config.frames {
            if f.contrast < f.threshold {
                return Err(CB6Code::TRANSITION_BELOW_GATE);
            }
        }
        Ok(())
    }

    /// 事务断言（原子切换）
    pub fn assert_transaction(&self) -> Result<(), CB6Code> {
        if self.config.transaction.atomic && self.config.transaction.committed {
            Ok(())
        } else {
            Err(CB6Code::SWITCH_GLITCH)
        }
    }

    /// 正交断言
    pub fn assert_orthogonal(&self) -> Result<(), CB6Code> {
        if self.config.reduce_orthogonal {
            Ok(())
        } else {
            Err(CB6Code::ORTHO_MIXED)
        }
    }

    /// 替代单源断言
    pub fn assert_replacement(&self) -> Result<(), CB6Code> {
        if self.config.replacement_active {
            Ok(())
        } else {
            Err(CB6Code::REPLACEMENT_LOST)
        }
    }

    /// 全帧扫描统计
    pub fn scan_frames(&self) -> (usize, usize) {
        let total = self.config.frames.len();
        let ok = self.config.frames.iter().filter(|f| f.contrast >= f.threshold).count();
        (total, ok)
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves134_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves134_colormotion");

    fn good_config() -> MLinkage {
        MLinkage {
            reduce_orthogonal: true,
            replacement_active: true,
            frames: alloc::vec![
                TransitionFrame { index: 0, contrast: 500, threshold: 450 },
                TransitionFrame { index: 1, contrast: 480, threshold: 450 },
            ],
            transaction: SwitchTransaction { atomic: true, committed: true },
        }
    }

    // 01 过渡破门 P1
    {
        let mut cfg = good_config();
        cfg.frames.push(TransitionFrame { index: 2, contrast: 300, threshold: 450 });
        let m = ColorMotionLink::new(cfg);
        chk(&mut set, "C3834-01", m.assert_transition_frames() == Err(CB6Code::TRANSITION_BELOW_GATE), "err");
    }
    // 02 过渡全帧过
    {
        let m = ColorMotionLink::new(good_config());
        chk(&mut set, "C3834-02", m.assert_transition_frames().is_ok(), "err");
    }
    // 03 事务闪断
    {
        let mut cfg = good_config();
        cfg.transaction = SwitchTransaction { atomic: false, committed: true };
        let m = ColorMotionLink::new(cfg);
        chk(&mut set, "C3834-03", m.assert_transaction() == Err(CB6Code::SWITCH_GLITCH), "err");
    }
    // 04 事务正常
    {
        let m = ColorMotionLink::new(good_config());
        chk(&mut set, "C3834-04", m.assert_transaction().is_ok(), "err");
    }
    // 05 正交混
    {
        let mut cfg = good_config();
        cfg.reduce_orthogonal = false;
        let m = ColorMotionLink::new(cfg);
        chk(&mut set, "C3834-05", m.assert_orthogonal() == Err(CB6Code::ORTHO_MIXED), "err");
    }
    // 06 正交正常
    {
        let m = ColorMotionLink::new(good_config());
        chk(&mut set, "C3834-06", m.assert_orthogonal().is_ok(), "err");
    }
    // 07 替代失联
    {
        let mut cfg = good_config();
        cfg.replacement_active = false;
        let m = ColorMotionLink::new(cfg);
        chk(&mut set, "C3834-07", m.assert_replacement() == Err(CB6Code::REPLACEMENT_LOST), "err");
    }
    // 08 替代正常
    {
        let m = ColorMotionLink::new(good_config());
        chk(&mut set, "C3834-08", m.assert_replacement().is_ok(), "err");
    }
    // 09 帧扫描
    {
        let m = ColorMotionLink::new(good_config());
        let (total, ok) = m.scan_frames();
        chk(&mut set, "C3834-09", total == 2 && ok == 2, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3834-10", !CB6Code::TRANSITION_BELOW_GATE.explain().is_empty(), "err");
    }

    set
}
