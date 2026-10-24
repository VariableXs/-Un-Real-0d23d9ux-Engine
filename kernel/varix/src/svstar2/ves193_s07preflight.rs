//! VE-F3939 · S07 预备自查
//!
//! 组级预备：十维自查（预算/模型/基准/门禁/规模/内存/功耗/降档/长稳/资产
//! ——十维×三色复述制度 S07 定制维）+ 风险册 + GO 预判（四条件 S 版：
//! 十维红零+断链零+下限全绿+模型校准）+ 移交复述四件套。
//! 错误路径：红未清→阻断、判定缺输入→NO-GO、断链检出→P0、黄超期→转红。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD300）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDGCode(pub u16);

impl CDGCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const RED_OPEN: Self = Self(SEG_BASE + 1);
    pub const NO_INPUT: Self = Self(SEG_BASE + 2);
    pub const BREAK_P0: Self = Self(SEG_BASE + 3);
    pub const YELLOW_EXPIRE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RED_OPEN => "红未清→阻断",
            Self::NO_INPUT => "判定缺输入→NO-GO",
            Self::BREAK_P0 => "断链检出→P0",
            Self::YELLOW_EXPIRE => "黄超期→转红",
            _ => "未知",
        }
    }
}

// ── 十维自查（S07 定制）─────────────────────────────────────────────

/// 十维。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S07Dim {
    Budget,
    Model,
    Baseline,
    Gate,
    Scale,
    Memory,
    Power,
    TierDown,
    LongRun,
    Asset,
}

impl S07Dim {
    pub const ALL: [S07Dim; 10] = [
        S07Dim::Budget,
        S07Dim::Model,
        S07Dim::Baseline,
        S07Dim::Gate,
        S07Dim::Scale,
        S07Dim::Memory,
        S07Dim::Power,
        S07Dim::TierDown,
        S07Dim::LongRun,
        S07Dim::Asset,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Budget => "预算",
            Self::Model => "模型",
            Self::Baseline => "基准",
            Self::Gate => "门禁",
            Self::Scale => "规模",
            Self::Memory => "内存",
            Self::Power => "功耗",
            Self::TierDown => "降档",
            Self::LongRun => "长稳",
            Self::Asset => "资产",
        }
    }
}

/// 三色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Green,
    Yellow,
    Red,
}

/// 单维自查读数。
#[derive(Debug, Clone, Copy)]
pub struct DimRead {
    pub dim: S07Dim,
    pub color: Color,
    /// 黄态挂账天数。
    pub yellow_days: u32,
}

/// 黄超期红线（14 天）。
pub const YELLOW_LIMIT_DAYS: u32 = 14;

impl DimRead {
    /// 黄超期→转红。
    pub fn effective(&self) -> Color {
        if self.color == Color::Yellow && self.yellow_days > YELLOW_LIMIT_DAYS {
            Color::Red
        } else {
            self.color
        }
    }
}

// ── 风险册 ──────────────────────────────────────────────────────────

/// 风险条目。
#[derive(Debug, Clone, Copy)]
pub struct RiskEntry {
    pub dim: S07Dim,
    pub desc: &'static str,
    /// 已消解。
    pub cleared: bool,
}

/// 风险册。
#[derive(Debug, Clone)]
pub struct RiskBook {
    pub items: Vec<RiskEntry>,
}

impl RiskBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn log(&mut self, dim: S07Dim, desc: &'static str) {
        self.items.push(RiskEntry {
            dim,
            desc,
            cleared: false,
        });
    }

    /// 某维未消解风险数。
    pub fn open_of(&self, dim: S07Dim) -> usize {
        self.items
            .iter()
            .filter(|r| r.dim == dim && !r.cleared)
            .count()
    }

    /// 消解。
    pub fn clear(&mut self, idx: usize) -> bool {
        if let Some(r) = self.items.get_mut(idx) {
            r.cleared = true;
            true
        } else {
            false
        }
    }
}

impl Default for RiskBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── GO 预判（四条件 S 版）───────────────────────────────────────────

/// 四条件输入。
#[derive(Debug, Clone, Copy)]
pub struct GoInput {
    /// 十维读数。
    pub dims: [DimRead; 10],
    /// 断链数。
    pub breaks: u32,
    /// 下限实测（µs）。
    pub floor_us: u32,
    /// 模型校准完成。
    pub model_calibrated: bool,
}

impl GoInput {
    /// 条件一：十维红零（按黄转红后的有效色）。
    pub fn dims_red_zero(&self) -> bool {
        self.dims.iter().all(|d| d.effective() != Color::Red)
    }

    /// 条件二：断链零。
    pub fn break_zero(&self) -> bool {
        self.breaks == 0
    }

    /// 条件三：下限全绿（≥300µs 预算底线）。
    pub fn floor_green(&self) -> bool {
        self.floor_us >= 300
    }

    /// 条件四：模型校准。
    pub fn model_ok(&self) -> bool {
        self.model_calibrated
    }

    /// GO 判定：四条件全成立。
    pub fn go(&self) -> bool {
        self.dims_red_zero() && self.break_zero() && self.floor_green() && self.model_ok()
    }
}

/// 判定输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    NoGo,
}

/// 判定器：缺输入→NO-GO；四条件→GO。
pub fn decide(input: Option<&GoInput>) -> Verdict {
    match input {
        None => Verdict::NoGo,
        Some(g) if g.go() => Verdict::Go,
        _ => Verdict::NoGo,
    }
}

// ── 移交四件套 ──────────────────────────────────────────────────────

/// 移交件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handover {
    /// 自查册。
    SelfCheckBook,
    /// 风险册。
    RiskBook,
    /// 判定记录。
    VerdictLog,
    /// 收口输入包。
    ClosePack,
}

impl Handover {
    pub const ALL: [Handover; 4] = [
        Handover::SelfCheckBook,
        Handover::RiskBook,
        Handover::VerdictLog,
        Handover::ClosePack,
    ];
}

/// 移交核收：四件齐且各有落点。
pub fn handover_ok(delivered: &[Handover]) -> bool {
    Handover::ALL.iter().all(|h| delivered.contains(h))
}

// ── 自查会话（总控）─────────────────────────────────────────────────

/// S07 预备自查会话。
#[derive(Debug, Clone)]
pub struct S07Session {
    pub dims: [DimRead; 10],
    pub risks: RiskBook,
    pub go_input: Option<GoInput>,
    pub delivered: Vec<Handover>,
    /// 断链检出数。
    pub breaks: u32,
}

impl S07Session {
    /// 干净基线：十维全绿。
    pub fn clean() -> Self {
        Self {
            dims: S07Dim::ALL
                .iter()
                .map(|d| DimRead {
                    dim: *d,
                    color: Color::Green,
                    yellow_days: 0,
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            risks: RiskBook::new(),
            go_input: None,
            delivered: Vec::new(),
            breaks: 0,
        }
    }

    /// 转红扫描：黄超期条目转红计数。
    pub fn expire_scan(&self) -> usize {
        self.dims.iter().filter(|d| d.effective() != d.color).count()
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CDGCode> {
        if self.breaks > 0 {
            return Err(CDGCode::BREAK_P0);
        }
        if self.dims.iter().any(|d| d.effective() == Color::Red) {
            return Err(CDGCode::RED_OPEN);
        }
        match self.go_input {
            None => Err(CDGCode::NO_INPUT),
            Some(ref g) if !g.go() => Err(CDGCode::RED_OPEN),
            _ => Ok(()),
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves193_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves193_s07preflight");

    fn read(dim: S07Dim, color: Color, days: u32) -> DimRead {
        DimRead {
            dim,
            color,
            yellow_days: days,
        }
    }

    // 01 十维齐备定名
    {
        let ok = S07Dim::ALL.len() == 10
            && S07Dim::ALL.iter().all(|d| !d.name().is_empty());
        chk(&mut set, "C3939-01", ok, "err");
    }
    // 02 黄未超期仍黄
    {
        let d = read(S07Dim::Budget, Color::Yellow, 10);
        chk(&mut set, "C3939-02", d.effective() == Color::Yellow, "err");
    }
    // 03 黄超期→转红
    {
        let d = read(S07Dim::Budget, Color::Yellow, 20);
        chk(&mut set, "C3939-03", d.effective() == Color::Red, "err");
    }
    // 04 转红扫描计数
    {
        let mut s = S07Session::clean();
        s.dims[3] = read(S07Dim::Gate, Color::Yellow, 99);
        s.dims[7] = read(S07Dim::TierDown, Color::Yellow, 5);
        chk(&mut set, "C3939-04", s.expire_scan() == 1, "err");
    }
    // 05 四条件：全绿+断链零+下限+校准→GO
    {
        let s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 0,
            floor_us: 320,
            model_calibrated: true,
        };
        chk(&mut set, "C3939-05", g.go(), "err");
    }
    // 06 四条件：断链非零→NO-GO
    {
        let s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 1,
            floor_us: 320,
            model_calibrated: true,
        };
        chk(
            &mut set,
            "C3939-06",
            !g.go() && !g.break_zero(),
            "err",
        );
    }
    // 07 四条件：下限破→NO-GO
    {
        let s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 0,
            floor_us: 299,
            model_calibrated: true,
        };
        chk(&mut set, "C3939-07", !g.floor_green() && !g.go(), "err");
    }
    // 08 四条件：未校准→NO-GO
    {
        let s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 0,
            floor_us: 320,
            model_calibrated: false,
        };
        chk(&mut set, "C3939-08", !g.model_ok() && !g.go(), "err");
    }
    // 09 判定器：缺输入→NO-GO
    {
        chk(
            &mut set,
            "C3939-09",
            decide(None) == Verdict::NoGo,
            "err",
        );
    }
    // 10 判定器：有输入且四条件→GO
    {
        let s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 0,
            floor_us: 320,
            model_calibrated: true,
        };
        chk(
            &mut set,
            "C3939-10",
            decide(Some(&g)) == Verdict::Go,
            "err",
        );
    }
    // 11 风险册登记与消解
    {
        let mut r = RiskBook::new();
        r.log(S07Dim::Memory, "水位波动");
        r.log(S07Dim::Memory, "碎片峰");
        chk(&mut set, "C3939-11", r.open_of(S07Dim::Memory) == 2, "err");
    }
    // 12 消解后归零
    {
        let mut r = RiskBook::new();
        r.log(S07Dim::Memory, "水位波动");
        let _ = r.clear(0);
        chk(&mut set, "C3939-12", r.open_of(S07Dim::Memory) == 0, "err");
    }
    // 13 移交四件套齐
    {
        chk(
            &mut set,
            "C3939-13",
            handover_ok(&Handover::ALL),
            "err",
        );
    }
    // 14 移交缺件即拒
    {
        let partial = [Handover::SelfCheckBook, Handover::RiskBook];
        chk(&mut set, "C3939-14", !handover_ok(&partial), "err");
    }
    // 15 会话总检：红未清→阻断
    {
        let mut s = S07Session::clean();
        s.dims[0] = read(S07Dim::Budget, Color::Red, 0);
        chk(
            &mut set,
            "C3939-15",
            s.full_check() == Err(CDGCode::RED_OPEN),
            "err",
        );
    }
    // 16 会话总检：断链→P0
    {
        let mut s = S07Session::clean();
        s.breaks = 1;
        chk(
            &mut set,
            "C3939-16",
            s.full_check() == Err(CDGCode::BREAK_P0),
            "err",
        );
    }
    // 17 会话总检：缺判定输入→NO-GO
    {
        let s = S07Session::clean();
        chk(
            &mut set,
            "C3939-17",
            s.full_check() == Err(CDGCode::NO_INPUT),
            "err",
        );
    }
    // 18 会话总检全绿
    {
        let mut s = S07Session::clean();
        let g = GoInput {
            dims: s.dims,
            breaks: 0,
            floor_us: 320,
            model_calibrated: true,
        };
        s.go_input = Some(g);
        chk(&mut set, "C3939-18", s.full_check() == Ok(()), "err");
    }
    // 19 风险清空越界防御
    {
        let mut r = RiskBook::new();
        chk(&mut set, "C3939-19", !r.clear(9), "err");
    }
    // 20 诊断码互异
    {
        let codes = [
            CDGCode::RED_OPEN,
            CDGCode::NO_INPUT,
            CDGCode::BREAK_P0,
            CDGCode::YELLOW_EXPIRE,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3939-20", distinct, "err");
    }

    set
}
