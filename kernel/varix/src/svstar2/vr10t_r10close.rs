//! VE-F3800 · R 域收官宣告（R10 收口组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3800`
//!
//! **判据（锚点原文）**：四卷终版、双维五维、20 维八用、三纪律零违例、
//! 清账 200/200、收官宣告、判据。
//!
//! **本条是什么**：R 域 200 项收官宣告——四卷终版（总账卷/分账卷/汇总卷/
//! 例外卷）；双维五维清账（工具/产出双维 + 五资产）；20 维验收；
//! 三纪律零违例（不虚标/不弃用/不隐瞒）；清账 200/200 六账相符；
//! 家族第十五个域收官宣告。
//!
//! 诊断码段：独占 0xBFxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBFxx）
// ─────────────────────────────────────────────────────────────

/// F3800 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BFCode(pub u16);

impl BFCode {
    /// 卷缺终版——补。
    pub const VOL_MISS: BFCode = BFCode(0xBF01);
    /// 纪律违例——阻断。
    pub const DISCIPLINE: BFCode = BFCode(0xBF02);
    /// 清账不符——对账。
    pub const CLEAR_MISMATCH: BFCode = BFCode(0xBF03);
    /// 宣告条件未满——不可宣告。
    pub const DECL_BLOCK: BFCode = BFCode(0xBF04);
    /// 双维五维缺。
    pub const DUAL_MISS: BFCode = BFCode(0xBF05);
    /// 20 维不齐。
    pub const DIM_MISS: BFCode = BFCode(0xBF06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBF01 => "四卷缺终版，需补齐。",
            0xBF02 => "三纪律违例，阻断收官。",
            0xBF03 => "清账不符，需对账。",
            0xBF04 => "宣告条件未满，不可宣告。",
            0xBF05 => "双维五维缺。",
            0xBF06 => "20 维验收不齐。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 四卷终版
// ─────────────────────────────────────────────────────────────

/// 卷（四卷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Volume {
    /// 总账卷。
    Master,
    /// 分账卷。
    Split,
    /// 汇总卷。
    Summary,
    /// 例外卷。
    Exception,
}

impl Volume {
    /// 全集。
    pub const ALL: [Volume; 4] = [
        Volume::Master,
        Volume::Split,
        Volume::Summary,
        Volume::Exception,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Volume::Master => 0,
            Volume::Split => 1,
            Volume::Summary => 2,
            Volume::Exception => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Volume::Master => "总账卷",
            Volume::Split => "分账卷",
            Volume::Summary => "汇总卷",
            Volume::Exception => "例外卷",
        }
    }
}

/// 卷册。
#[derive(Clone, Debug)]
pub struct VolumeSet {
    final_ver: [bool; 4],
}

impl VolumeSet {
    /// 新建。
    pub const fn new() -> Self {
        VolumeSet {
            final_ver: [false; 4],
        }
    }

    /// 置终版。
    pub fn finalize(&mut self, v: Volume) -> bool {
        let i = v.ordinal();
        if self.final_ver[i] {
            false
        } else {
            self.final_ver[i] = true;
            true
        }
    }

    /// 终版数。
    pub fn count(&self) -> usize {
        self.final_ver.iter().filter(|f| **f).count()
    }

    /// 四卷全终。
    pub fn complete(&self) -> bool {
        self.count() == 4
    }

    /// 缺卷名。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for v in Volume::ALL.iter() {
            if !self.final_ver[v.ordinal()] {
                out.push(v.name());
            }
        }
        out
    }
}

// ─────────────────────────────────────────────────────────────
// 三纪律零违例
// ─────────────────────────────────────────────────────────────

/// 纪律（三条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Discipline {
    /// 不虚标。
    NoFalseMark,
    /// 不弃用。
    NoAbandon,
    /// 不隐瞒。
    NoConceal,
}

impl Discipline {
    /// 全集。
    pub const ALL: [Discipline; 3] = [
        Discipline::NoFalseMark,
        Discipline::NoAbandon,
        Discipline::NoConceal,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Discipline::NoFalseMark => 0,
            Discipline::NoAbandon => 1,
            Discipline::NoConceal => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Discipline::NoFalseMark => "不虚标",
            Discipline::NoAbandon => "不弃用",
            Discipline::NoConceal => "不隐瞒",
        }
    }
}

/// 纪律账。
#[derive(Clone, Debug)]
pub struct DiscLedger {
    violations: [u32; 3],
}

impl DiscLedger {
    /// 新建。
    pub const fn new() -> Self {
        DiscLedger {
            violations: [0; 3],
        }
    }

    /// 记违例。
    pub fn bump(&mut self, d: Discipline) {
        self.violations[d.ordinal()] += 1;
    }

    /// 总违例。
    pub fn total(&self) -> u32 {
        self.violations.iter().sum()
    }

    /// 零违例。
    pub fn zero(&self) -> bool {
        self.total() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 双维五维清账 + 20 维 + 宣告器
// ─────────────────────────────────────────────────────────────

/// 清账状态。
#[derive(Clone, Copy, Debug)]
pub struct ClearState {
    /// 已清项数（须 200）。
    pub cleared: u16,
    /// 目标项数。
    pub target: u16,
    /// 六账相符。
    pub six_sync: bool,
    /// 双维在册（工具+产出）。
    pub tool_dim: bool,
    pub output_dim: bool,
    /// 五维在册（五资产）。
    pub assets: u8,
    /// 20 维齐。
    pub dims_ok: bool,
}

impl ClearState {
    /// 新建。
    pub const fn new() -> Self {
        ClearState {
            cleared: 0,
            target: 200,
            six_sync: false,
            tool_dim: false,
            output_dim: false,
            assets: 0,
            dims_ok: false,
        }
    }

    /// 清账达标（200/200 且六账相符）。
    pub fn clear_ok(&self) -> bool {
        self.cleared == self.target && self.six_sync
    }

    /// 双维齐。
    pub fn dual_ok(&self) -> bool {
        self.tool_dim && self.output_dim
    }

    /// 五维齐（五资产）。
    pub fn five_ok(&self) -> bool {
        self.assets == 5
    }
}

/// 收官宣告器。
#[derive(Clone, Debug)]
pub struct Decl {
    /// 四卷终。
    pub vols_ok: bool,
    /// 三纪律零违例。
    pub disc_zero: bool,
    /// 清账达标。
    pub clear_ok: bool,
    /// 双维五维齐。
    pub dual_five_ok: bool,
    /// 20 维齐。
    pub dims_ok: bool,
    /// 已宣告。
    declared: bool,
}

impl Decl {
    /// 新建。
    pub const fn new() -> Self {
        Decl {
            vols_ok: false,
            disc_zero: false,
            clear_ok: false,
            dual_five_ok: false,
            dims_ok: false,
            declared: false,
        }
    }

    /// 条件齐备数。
    pub fn cond_count(&self) -> u32 {
        let mut n = 0;
        if self.vols_ok {
            n += 1;
        }
        if self.disc_zero {
            n += 1;
        }
        if self.clear_ok {
            n += 1;
        }
        if self.dual_five_ok {
            n += 1;
        }
        if self.dims_ok {
            n += 1;
        }
        n
    }

    /// 可宣告。
    pub fn ready(&self) -> bool {
        self.cond_count() == 5
    }

    /// 执行宣告。
    pub fn declare(&mut self) -> bool {
        if self.ready() {
            self.declared = true;
            true
        } else {
            false
        }
    }

    /// 已宣告。
    pub fn declared(&self) -> bool {
        self.declared
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3800 判据集。
pub fn run_vr10t_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3800");

    // 0xBF01 四卷终版
    {
        let mut v = VolumeSet::new();
        set.add("0xBF01-初始缺四", v.count() == 0, "");
        for vol in Volume::ALL.iter() {
            v.finalize(*vol);
        }
        set.add("0xBF01-四卷全终", v.complete() && v.missing().is_empty(), "");
        set.add("0xBF01-重终拒", !v.finalize(Volume::Master), "");
    }

    // 0xBF02 三纪律
    {
        let mut d = DiscLedger::new();
        set.add("0xBF02-初始零违", d.zero() && d.total() == 0, "");
        d.bump(Discipline::NoFalseMark);
        set.add("0xBF02-违例检出", !d.zero() && d.total() == 1, "");
        for disc in Discipline::ALL.iter() {
            d.bump(*disc);
        }
        set.add("0xBF02-计数累进", d.total() == 4, "");
    }

    // 0xBF03 清账
    {
        let mut c = ClearState::new();
        set.add("0xBF03-初始不符", !c.clear_ok(), "");
        c.cleared = 200;
        set.add("0xBF03-仅数量不符", !c.clear_ok(), "");
        c.six_sync = true;
        set.add("0xBF03-200/200相符", c.clear_ok(), "");
    }

    // 0xBF05 双维五维
    {
        let mut c = ClearState::new();
        set.add("0xBF05-双维缺", !c.dual_ok(), "");
        c.tool_dim = true;
        set.add("0xBF05-单维不齐", !c.dual_ok(), "");
        c.output_dim = true;
        set.add("0xBF05-双维齐", c.dual_ok(), "");
        set.add("0xBF05-五维缺", !c.five_ok(), "");
        c.assets = 5;
        set.add("0xBF05-五维齐", c.five_ok(), "");
    }

    // 0xBF04 宣告器
    {
        let mut d = Decl::new();
        set.add("0xBF04-初始零条件", d.cond_count() == 0, "");
        set.add("0xBF04-未满不宣告", !d.declare() && !d.declared(), "");
        d.vols_ok = true;
        d.disc_zero = true;
        d.clear_ok = true;
        d.dual_five_ok = true;
        d.dims_ok = true;
        set.add("0xBF04-五条件满", d.ready() && d.cond_count() == 5, "");
        set.add("0xBF04-宣告进", d.declare() && d.declared(), "");
    }

    // 卷序自洽
    {
        let mut ok = true;
        for (i, v) in Volume::ALL.iter().enumerate() {
            if v.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBF01-卷序自洽", ok, "");
        let mut ok_d = true;
        for (i, disc) in Discipline::ALL.iter().enumerate() {
            if disc.ordinal() != i {
                ok_d = false;
            }
        }
        set.add("0xBF02-纪律序自洽", ok_d, "");
    }

    set
}
