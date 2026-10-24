//! VE-F3809 · 光敏渲染安全（VE-T 域 · 无障碍渲染 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3809`
//!
//! **判据（锚点原文）**：闪烁红线、正交检测、降闪烁处置、自适应阈值、判据。
//!
//! **本条是什么**：光敏安全渲染——闪烁频率红线（>3Hz 即 P0）；
//! 正交检测（频率×幅度×面积三维独立判）；降闪烁自动处置；
//! 自适应阈值随用户档位调整。
//!
//! 诊断码段：独占 0xC2xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC2xx）
// ─────────────────────────────────────────────────────────────

/// F3809 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C2Code(pub u16);

impl C2Code {
    /// 闪烁超频——P0。
    pub const FLICKER_P0: C2Code = C2Code(0xC201);
    /// 幅度超限。
    pub const AMP_OVER: C2Code = C2Code(0xC202);
    /// 面积超限。
    pub const AREA_OVER: C2Code = C2Code(0xC203);
    /// 处置缺失——补。
    pub const TREAT_MISS: C2Code = C2Code(0xC204);
    /// 阈值漂移——校准。
    pub const THRESH_DRIFT: C2Code = C2Code(0xC205);
    /// 检测项缺。
    pub const CHECK_MISS: C2Code = C2Code(0xC206);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC201 => "闪烁频率超红线，P0——光敏风险。",
            0xC202 => "闪烁幅度过限。",
            0xC203 => "闪烁面积超限。",
            0xC204 => "降闪烁处置缺失。",
            0xC205 => "阈值漂移，需校准。",
            0xC206 => "正交检测项缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 正交三维检测（频率×幅度×面积）
// ─────────────────────────────────────────────────────────────

/// 闪烁样本。
#[derive(Clone, Copy, Debug)]
pub struct FlickerSample {
    /// 频率（厘赫兹，300 = 3Hz）。
    pub freq_centi_hz: u16,
    /// 幅度百分比（0..100）。
    pub amp_pct: u8,
    /// 面积占比（0..100）。
    pub area_pct: u8,
}

impl FlickerSample {
    /// 构造。
    pub const fn new(freq_centi_hz: u16, amp_pct: u8, area_pct: u8) -> Self {
        FlickerSample {
            freq_centi_hz,
            amp_pct,
            area_pct,
        }
    }

    /// 频率越红线（>3Hz）。
    pub fn freq_over(self) -> bool {
        self.freq_centi_hz > 300
    }

    /// 幅度越限（>25%）。
    pub fn amp_over(self) -> bool {
        self.amp_pct > 25
    }

    /// 面积越限（>25%）。
    pub fn area_over(self) -> bool {
        self.area_pct > 25
    }

    /// 风险分级（任一越限即风险；频率越限 = P0）。
    pub fn risk_level(self) -> u8 {
        if self.freq_over() {
            2
        } else if self.amp_over() || self.area_over() {
            1
        } else {
            0
        }
    }
}

/// 正交检测器。
#[derive(Clone, Copy, Debug)]
pub struct OrthoDetect {
    /// 三维各自通过。
    pub freq_ok: bool,
    pub amp_ok: bool,
    pub area_ok: bool,
}

impl OrthoDetect {
    /// 从样本推导。
    pub fn from(s: FlickerSample) -> Self {
        OrthoDetect {
            freq_ok: !s.freq_over(),
            amp_ok: !s.amp_over(),
            area_ok: !s.area_over(),
        }
    }

    /// 全过。
    pub fn all_pass(self) -> bool {
        self.freq_ok && self.amp_ok && self.area_ok
    }

    /// 失败维数。
    pub fn fail_count(self) -> u32 {
        let mut n = 0;
        if !self.freq_ok {
            n += 1;
        }
        if !self.amp_ok {
            n += 1;
        }
        if !self.area_ok {
            n += 1;
        }
        n
    }
}

// ─────────────────────────────────────────────────────────────
// 降闪烁处置 + 自适应阈值
// ─────────────────────────────────────────────────────────────

/// 处置方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Treat {
    /// 无。
    None,
    /// 降频。
    ReduceFreq,
    /// 降幅。
    ReduceAmp,
    /// 缩面积。
    ShrinkArea,
    /// 静态化。
    Freeze,
}

impl Treat {
    /// 全集。
    pub const ALL: [Treat; 5] = [
        Treat::None,
        Treat::ReduceFreq,
        Treat::ReduceAmp,
        Treat::ShrinkArea,
        Treat::Freeze,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Treat::None => 0,
            Treat::ReduceFreq => 1,
            Treat::ReduceAmp => 2,
            Treat::ShrinkArea => 3,
            Treat::Freeze => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Treat::None => "无",
            Treat::ReduceFreq => "降频",
            Treat::ReduceAmp => "降幅",
            Treat::ShrinkArea => "缩面积",
            Treat::Freeze => "静态化",
        }
    }
}

/// 处置器。
#[derive(Clone, Debug)]
pub struct TreatEngine {
    /// 已处置数。
    pub treated: u32,
    /// 处置缺失数。
    pub missing: u32,
}

impl TreatEngine {
    /// 新建。
    pub const fn new() -> Self {
        TreatEngine {
            treated: 0,
            missing: 0,
        }
    }

    /// 按风险处置。
    pub fn treat(&mut self, risk: u8) -> Treat {
        match risk {
            2 => {
                self.treated += 1;
                Treat::Freeze
            }
            1 => {
                self.treated += 1;
                Treat::ReduceAmp
            }
            _ => Treat::None,
        }
    }

    /// 记处置缺失。
    pub fn miss(&mut self) {
        self.missing += 1;
    }
}

/// 自适应阈值（随用户档位）。
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveThresh {
    /// 当前频率阈（厘赫兹）。
    pub freq_limit: u16,
    /// 用户档（0=标准 1=敏感 2=高度敏感）。
    pub tier: u8,
}

impl AdaptiveThresh {
    /// 新建（按档）。
    pub const fn new(tier: u8) -> Self {
        let limit = match tier {
            0 => 300,
            1 => 200,
            2 => 100,
            _ => 300,
        };
        AdaptiveThresh {
            freq_limit: limit,
            tier,
        }
    }

    /// 判越限。
    pub fn over(&self, freq_centi_hz: u16) -> bool {
        freq_centi_hz > self.freq_limit
    }

    /// 校准（换档重算）。
    pub fn calibrate(&mut self, tier: u8) {
        self.tier = tier;
        self.freq_limit = match tier {
            0 => 300,
            1 => 200,
            2 => 100,
            _ => 300,
        };
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3809 判据集。
pub fn run_f3809_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3809");

    // 0xC201 频率红线
    {
        let s = FlickerSample::new(301, 10, 10);
        set.add("0xC201-301超频", s.freq_over() && s.risk_level() == 2, "");
        let s2 = FlickerSample::new(300, 10, 10);
        set.add("0xC201-300不超", !s2.freq_over() && s2.risk_level() == 0, "");
    }

    // 0xC202/0xC203 正交三维
    {
        let s = FlickerSample::new(100, 30, 30);
        let d = OrthoDetect::from(s);
        set.add("0xC202-幅度越限", !d.amp_ok, "");
        set.add("0xC203-面积越限", !d.area_ok, "");
        set.add("0xC206-失败两维", d.fail_count() == 2 && !d.all_pass(), "");
        let s2 = FlickerSample::new(100, 10, 10);
        set.add("0xC206-全过", OrthoDetect::from(s2).all_pass(), "");
    }

    // 0xC204 处置
    {
        let mut e = TreatEngine::new();
        set.add("0xC204-P0静态化", e.treat(2) == Treat::Freeze, "");
        set.add("0xC204-P1降幅", e.treat(1) == Treat::ReduceAmp, "");
        set.add("0xC204-零风险无", e.treat(0) == Treat::None, "");
        set.add("0xC204-处置计数", e.treated == 2, "");
        e.miss();
        set.add("0xC204-缺失计数", e.missing == 1, "");
    }

    // 0xC205 自适应阈值
    {
        let t = AdaptiveThresh::new(0);
        set.add("0xC205-标准300", t.freq_limit == 300 && !t.over(250), "");
        let t2 = AdaptiveThresh::new(2);
        set.add("0xC205-高度敏感100", t2.freq_limit == 100 && t2.over(150), "");
        let mut t3 = AdaptiveThresh::new(0);
        t3.calibrate(1);
        set.add("0xC205-校准换档", t3.freq_limit == 200 && t3.tier == 1, "");
    }

    // 风险分级
    {
        let s = FlickerSample::new(200, 30, 5);
        set.add("0xC202-P1级", s.risk_level() == 1, "");
    }

    set
}
