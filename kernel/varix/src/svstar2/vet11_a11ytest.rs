//! VE-F3812 · 无障碍渲染测试（VE-T 域 · 无障碍渲染 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3812`
//!
//! **判据（锚点原文）**：八类断言、组合矩阵、SSIM 单源、真机复述、断链 P0、判据。
//!
//! **本条是什么**：无障碍渲染测试体系——八类断言生成器（断链/策略透明/
//! 1ms 预算/焦点 1 帧/截断/替代对齐/图形 3:1/突变）；四态×场景组合矩阵；
//! SSIM 视觉对拍单源复用；辅具真机联调（模拟代真 = 无效）。
//!
//! 诊断码段：独占 0xC5xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC5xx）
// ─────────────────────────────────────────────────────────────

/// F3812 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C5Code(pub u16);

impl C5Code {
    /// 断链漏检——P0。
    pub const BREAK_MISS: C5Code = C5Code(0xC501);
    /// 矩阵缺格——补跑。
    pub const MATRIX_GAP: C5Code = C5Code(0xC502);
    /// 生成器漂移——修复。
    pub const GEN_DRIFT: C5Code = C5Code(0xC503);
    /// 模拟代真——无效。
    pub const SIM_SUB: C5Code = C5Code(0xC504);
    /// 八类缺类。
    pub const CLASS_MISS: C5Code = C5Code(0xC505);
    /// SSIM 单源断。
    pub const SSIM_BREAK: C5Code = C5Code(0xC506);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC501 => "断链断言漏检，P0。",
            0xC502 => "组合矩阵缺格，需补跑。",
            0xC503 => "断言生成器漂移，需修复。",
            0xC504 => "模拟代真机，测试无效。",
            0xC505 => "八类断言缺类。",
            0xC506 => "SSIM 单源断链。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 八类断言
// ─────────────────────────────────────────────────────────────

/// 断言类（八类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertClass {
    /// 断链。
    ChainBreak,
    /// 策略透明。
    PolicyTransparent,
    /// 1ms 预算。
    Budget1ms,
    /// 焦点 1 帧。
    Focus1Frame,
    /// 截断。
    Truncation,
    /// 替代对齐。
    AltAlign,
    /// 图形 3:1。
    Graphic3to1,
    /// 突变。
    Mutation,
}

impl AssertClass {
    /// 全集。
    pub const ALL: [AssertClass; 8] = [
        AssertClass::ChainBreak,
        AssertClass::PolicyTransparent,
        AssertClass::Budget1ms,
        AssertClass::Focus1Frame,
        AssertClass::Truncation,
        AssertClass::AltAlign,
        AssertClass::Graphic3to1,
        AssertClass::Mutation,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            AssertClass::ChainBreak => 0,
            AssertClass::PolicyTransparent => 1,
            AssertClass::Budget1ms => 2,
            AssertClass::Focus1Frame => 3,
            AssertClass::Truncation => 4,
            AssertClass::AltAlign => 5,
            AssertClass::Graphic3to1 => 6,
            AssertClass::Mutation => 7,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            AssertClass::ChainBreak => "断链",
            AssertClass::PolicyTransparent => "策略透明",
            AssertClass::Budget1ms => "1ms预算",
            AssertClass::Focus1Frame => "焦点1帧",
            AssertClass::Truncation => "截断",
            AssertClass::AltAlign => "替代对齐",
            AssertClass::Graphic3to1 => "图形3:1",
            AssertClass::Mutation => "突变",
        }
    }
}

/// 生成器。
#[derive(Clone, Debug)]
pub struct AssertGen {
    /// 每类生成数。
    counts: [u16; 8],
    /// 漂移标志。
    pub drifted: bool,
}

impl AssertGen {
    /// 新建。
    pub const fn new() -> Self {
        AssertGen {
            counts: [0; 8],
            drifted: false,
        }
    }

    /// 生成一例。
    pub fn generate(&mut self, c: AssertClass) -> u16 {
        let i = c.ordinal();
        self.counts[i] = self.counts[i].saturating_add(1);
        self.counts[i]
    }

    /// 缺类数。
    pub fn missing(&self) -> usize {
        self.counts.iter().filter(|c| **c == 0).count()
    }

    /// 八类齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 总生成数。
    pub fn total(&self) -> u32 {
        self.counts.iter().map(|c| *c as u32).sum()
    }
}

// ─────────────────────────────────────────────────────────────
// 组合矩阵（四态×场景）
// ─────────────────────────────────────────────────────────────

/// 四态（辅具/reduce/高对比/大字号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderState {
    /// 辅具。
    Assistive,
    /// reduce。
    Reduce,
    /// 高对比。
    HighContrast,
    /// 大字号。
    LargeFont,
}

impl RenderState {
    /// 全集。
    pub const ALL: [RenderState; 4] = [
        RenderState::Assistive,
        RenderState::Reduce,
        RenderState::HighContrast,
        RenderState::LargeFont,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            RenderState::Assistive => 0,
            RenderState::Reduce => 1,
            RenderState::HighContrast => 2,
            RenderState::LargeFont => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            RenderState::Assistive => "辅具",
            RenderState::Reduce => "reduce",
            RenderState::HighContrast => "高对比",
            RenderState::LargeFont => "大字号",
        }
    }
}

/// 组合矩阵（4 态 × 6 场景 = 24 格）。
#[derive(Clone, Debug)]
pub struct ComboMatrix {
    ran: [[bool; 6]; 4],
}

impl ComboMatrix {
    /// 新建。
    pub const fn new() -> Self {
        ComboMatrix {
            ran: [[false; 6]; 4],
        }
    }

    /// 跑一格。
    pub fn run(&mut self, st: RenderState, scene: usize) -> bool {
        if scene < 6 {
            self.ran[st.ordinal()][scene] = true;
            true
        } else {
            false
        }
    }

    /// 已跑格数。
    pub fn ran_count(&self) -> usize {
        let mut n = 0;
        for row in self.ran.iter() {
            for v in row.iter() {
                if *v {
                    n += 1;
                }
            }
        }
        n
    }

    /// 缺格数。
    pub fn gaps(&self) -> usize {
        24 - self.ran_count()
    }

    /// 全跑。
    pub fn complete(&self) -> bool {
        self.gaps() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// SSIM 单源 + 真机红线
// ─────────────────────────────────────────────────────────────

/// SSIM 对拍（单源声明）。
#[derive(Clone, Copy, Debug)]
pub struct SsimCheck {
    /// 相似度万分比。
    pub sim_bp: u16,
    /// 单源声明在册。
    pub single_source: bool,
}

impl SsimCheck {
    /// 构造。
    pub const fn new(sim_bp: u16) -> Self {
        SsimCheck {
            sim_bp,
            single_source: true,
        }
    }

    /// 过门（≥9500）。
    pub fn pass(self) -> bool {
        self.sim_bp >= 9_500 && self.single_source
    }
}

/// 真机联调记录。
#[derive(Clone, Copy, Debug)]
pub struct RealDeviceRun {
    /// 真机标志（false=模拟代真，无效）。
    pub real: bool,
    /// 读屏接入。
    pub screenreader: bool,
}

impl RealDeviceRun {
    /// 有效。
    pub fn valid(self) -> bool {
        self.real && self.screenreader
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3812 判据集。
pub fn run_f3812_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3812");

    // 0xC505 八类
    {
        let mut g = AssertGen::new();
        set.add("0xC505-初始缺八", g.missing() == 8, "");
        for c in AssertClass::ALL.iter() {
            g.generate(*c);
        }
        set.add("0xC505-八类齐", g.complete() && g.total() == 8, "");
        set.add("0xC503-零漂移", !g.drifted, "");
    }

    // 0xC502 组合矩阵
    {
        let mut m = ComboMatrix::new();
        set.add("0xC502-初始缺24", m.gaps() == 24, "");
        for st in RenderState::ALL.iter() {
            for sc in 0..6 {
                m.run(*st, sc);
            }
        }
        set.add("0xC502-全跑", m.complete() && m.ran_count() == 24, "");
        set.add("0xC502-越界拒", !m.run(RenderState::Reduce, 6), "");
    }

    // 0xC506 SSIM
    {
        let s = SsimCheck::new(9600);
        set.add("0xC506-9600过", s.pass(), "");
        let s2 = SsimCheck::new(9400);
        set.add("0xC506-9400不过", !s2.pass(), "");
    }

    // 0xC504 真机红线
    {
        let r = RealDeviceRun {
            real: false,
            screenreader: true,
        };
        set.add("0xC504-模拟无效", !r.valid(), "");
        let r2 = RealDeviceRun {
            real: true,
            screenreader: true,
        };
        set.add("0xC504-真机有效", r2.valid(), "");
        let r3 = RealDeviceRun {
            real: true,
            screenreader: false,
        };
        set.add("0xC504-无读屏无效", !r3.valid(), "");
    }

    // 类序自洽
    {
        let mut ok = true;
        for (i, c) in AssertClass::ALL.iter().enumerate() {
            if c.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC505-类序自洽", ok, "");
        let mut ok_s = true;
        for (i, s) in RenderState::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok_s = false;
            }
        }
        set.add("0xC502-态序自洽", ok_s, "");
    }

    set
}
