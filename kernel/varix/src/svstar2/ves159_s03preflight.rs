//! VE-F3859 S03 预备自查（SEG 0xCAxx）。
//!
//! 十维自查（闭环/跟随/标注/噪音/公告/导航/虚拟化/表面/性能/矩阵）；
//! GO 四条件（十维红零+断流零+真机矩阵绿+覆盖 100%）；黄超期转红；移交四件套。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// 诊断码（SEG 0xCAxx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CACode {
    /// 红未清
    RedOpen = 0xCA00,
    /// 判定缺输入
    NoInput = 0xCA01,
    /// 断流检出
    StreamBreak = 0xCA02,
    /// 黄超期转红
    YellowExpire = 0xCA03,
    /// 移交缺件
    HandoffMiss = 0xCA04,
    /// 维度越界
    DimOver = 0xCA05,
    /// 真机未绿
    DeviceNotGreen = 0xCA06,
    /// 覆盖不足
    CoverShort = 0xCA07,
    /// 三色非法
    ColorInvalid = 0xCA08,
    /// 风险册缺失
    RiskMiss = 0xCA09,
    /// 核收失败
    AcceptFail = 0xCA0A,
    /// 输入伪造
    InputFake = 0xCA0B,
}

impl CACode {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn explain(self) -> &'static str {
        match self {
            CACode::RedOpen => "红未清阻断",
            CACode::NoInput => "判定缺输入",
            CACode::StreamBreak => "断流检出",
            CACode::YellowExpire => "黄超期转红",
            CACode::HandoffMiss => "移交缺件",
            CACode::DimOver => "维度越界",
            CACode::DeviceNotGreen => "真机矩阵未绿",
            CACode::CoverShort => "覆盖不足",
            CACode::ColorInvalid => "三色非法",
            CACode::RiskMiss => "风险册缺失",
            CACode::AcceptFail => "核收失败",
            CACode::InputFake => "输入伪造",
        }
    }
}

/// 三色灯。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Green,
    Yellow,
    Red,
}

impl Light {
    pub fn as_str(self) -> &'static str {
        match self {
            Light::Green => "绿",
            Light::Yellow => "黄",
            Light::Red => "红",
        }
    }
}

/// S03 十维。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum S03Dim {
    /// 闭环
    Loop,
    /// 跟随
    Follow,
    /// 标注
    Annot,
    /// 噪音
    Noise,
    /// 公告
    Announce,
    /// 导航
    Nav,
    /// 虚拟化
    Virt,
    /// 表面
    Surface,
    /// 性能
    Perf,
    /// 矩阵
    Matrix,
}

impl S03Dim {
    pub const ALL: [S03Dim; 10] = [
        S03Dim::Loop,
        S03Dim::Follow,
        S03Dim::Annot,
        S03Dim::Noise,
        S03Dim::Announce,
        S03Dim::Nav,
        S03Dim::Virt,
        S03Dim::Surface,
        S03Dim::Perf,
        S03Dim::Matrix,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            S03Dim::Loop => "闭环",
            S03Dim::Follow => "跟随",
            S03Dim::Annot => "标注",
            S03Dim::Noise => "噪音",
            S03Dim::Announce => "公告",
            S03Dim::Nav => "导航",
            S03Dim::Virt => "虚拟化",
            S03Dim::Surface => "表面",
            S03Dim::Perf => "性能",
            S03Dim::Matrix => "矩阵",
        }
    }
}

/// 维度状态。
#[derive(Clone, Debug)]
pub struct DimState {
    pub dim: S03Dim,
    pub light: Light,
    /// 黄灯已持续天数
    pub yellow_days: u32,
    /// 断流发生
    pub broke: bool,
}

/// 移交四件套。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffItem {
    /// 自查表
    Checklist,
    /// 风险册
    RiskBook,
    /// 证据包
    Evidence,
    /// 移交函
    Letter,
}

impl HandoffItem {
    pub const ALL: [HandoffItem; 4] = [
        HandoffItem::Checklist,
        HandoffItem::RiskBook,
        HandoffItem::Evidence,
        HandoffItem::Letter,
    ];
}

/// 预备自查器。
pub struct S03Preflight {
    dims: Vec<DimState>,
    handoff: Vec<HandoffItem>,
    /// 覆盖率
    coverage: u32,
    /// 真机矩阵是否绿
    device_green: bool,
}

impl S03Preflight {
    pub fn new() -> Self {
        let dims = S03Dim::ALL
            .iter()
            .map(|d| DimState {
                dim: *d,
                light: Light::Green,
                yellow_days: 0,
                broke: false,
            })
            .collect();
        S03Preflight {
            dims,
            handoff: Vec::new(),
            coverage: 0,
            device_green: false,
        }
    }

    fn idx(&self, d: S03Dim) -> Option<usize> {
        self.dims.iter().position(|x| x.dim == d)
    }

    pub fn set_light(&mut self, d: S03Dim, light: Light) -> Result<(), CACode> {
        let i = self.idx(d).ok_or(CACode::DimOver)?;
        self.dims[i].light = light;
        if light != Light::Yellow {
            self.dims[i].yellow_days = 0;
        }
        Ok(())
    }

    pub fn tick_yellow(&mut self, d: S03Dim) -> Result<(), CACode> {
        let i = self.idx(d).ok_or(CACode::DimOver)?;
        if self.dims[i].light == Light::Yellow {
            self.dims[i].yellow_days += 1;
            if self.dims[i].yellow_days > 3 {
                self.dims[i].light = Light::Red;
                return Err(CACode::YellowExpire);
            }
        }
        Ok(())
    }

    pub fn mark_broke(&mut self, d: S03Dim) -> Result<(), CACode> {
        let i = self.idx(d).ok_or(CACode::DimOver)?;
        self.dims[i].broke = true;
        self.dims[i].light = Light::Red;
        Err(CACode::StreamBreak)
    }

    pub fn red_count(&self) -> usize {
        self.dims.iter().filter(|x| x.light == Light::Red).count()
    }

    pub fn break_count(&self) -> usize {
        self.dims.iter().filter(|x| x.broke).count()
    }

    pub fn set_coverage(&mut self, c: u32) {
        self.coverage = c;
    }

    pub fn set_device_green(&mut self, v: bool) {
        self.device_green = v;
    }

    /// GO 四条件。
    pub fn go(&self) -> bool {
        self.red_count() == 0
            && self.break_count() == 0
            && self.device_green
            && self.coverage >= 100
    }

    pub fn add_handoff(&mut self, item: HandoffItem) {
        if !self.handoff.contains(&item) {
            self.handoff.push(item);
        }
    }

    pub fn handoff_complete(&self) -> bool {
        HandoffItem::ALL.iter().all(|i| self.handoff.contains(i))
    }

    pub fn yellow_count(&self) -> usize {
        self.dims.iter().filter(|x| x.light == Light::Yellow).count()
    }
}

pub fn run_ves159_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves159_s03preflight");
    {
        let p = S03Preflight::new();
        chk(&mut set, "C3859-01", p.dims.len() == 10, "十维矩阵");
    }
    {
        let mut p = S03Preflight::new();
        let r = p.set_light(S03Dim::Follow, Light::Yellow);
        chk(&mut set, "C3859-02", r.is_ok() && p.yellow_count() == 1, "黄灯登记");
    }
    {
        let mut p = S03Preflight::new();
        for _ in 0..4 {
            let _ = p.tick_yellow(S03Dim::Noise);
        }
        let r = p.tick_yellow(S03Dim::Noise);
        chk(
            &mut set,
            "C3859-03",
            r == Err(CACode::YellowExpire) || p.red_count() > 0,
            "黄超期转红",
        );
    }
    {
        let mut p = S03Preflight::new();
        let r = p.mark_broke(S03Dim::Announce);
        chk(
            &mut set,
            "C3859-04",
            r == Err(CACode::StreamBreak),
            "断流即红",
        );
    }
    {
        let p = S03Preflight::new();
        chk(&mut set, "C3859-05", p.red_count() == 0, "初始无红");
    }
    {
        let mut p = S03Preflight::new();
        chk(&mut set, "C3859-06", !p.go(), "缺覆盖不可GO");
    }
    {
        let mut p = S03Preflight::new();
        p.set_coverage(100);
        p.set_device_green(true);
        chk(&mut set, "C3859-07", p.go(), "四条件满足可GO");
    }
    {
        let mut p = S03Preflight::new();
        p.set_coverage(100);
        p.set_device_green(false);
        chk(&mut set, "C3859-08", !p.go(), "真机未绿不可GO");
    }
    {
        let mut p = S03Preflight::new();
        p.add_handoff(HandoffItem::Checklist);
        chk(&mut set, "C3859-09", !p.handoff_complete(), "移交未齐");
    }
    {
        let mut p = S03Preflight::new();
        for i in HandoffItem::ALL {
            p.add_handoff(i);
        }
        chk(&mut set, "C3859-10", p.handoff_complete(), "四件套齐备");
    }
    {
        let mut p = S03Preflight::new();
        let r = p.set_light(S03Dim::Surface, Light::Red);
        chk(&mut set, "C3859-11", r.is_ok() && p.red_count() == 1, "红灯登记");
        chk(&mut set, "C3859-12", !p.go(), "红未清阻断GO");
    }
    set
}
