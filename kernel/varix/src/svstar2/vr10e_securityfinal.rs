//! VE-F3785 · R 域安全终审（R10 收口组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3785`
//!
//! **判据（锚点原文）**：十六面零、缝隙硬门、分级执行、三列终版、即时修、
//! 判据。
//!
//! **本条是什么**：F3369 模式 R 版十六面终审（十二标准面 + R 新面：
//! 创作侵权面/隐写面/经济面/文化面）——🔴零断言；攻击面三列终版
//! （面/风险/处置）；缝隙审计硬门；安全分级执行核验。🔴检出即阻断+即时修。
//!
//! 诊断码段：独占 0xB0xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB0xx）
// ─────────────────────────────────────────────────────────────

/// F3785 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B0Code(pub u16);

impl B0Code {
    /// 🔴检出——阻断+即时修。
    pub const RED_FOUND: B0Code = B0Code(0xB001);
    /// 证据断——重验。
    pub const EVIDENCE_BREAK: B0Code = B0Code(0xB002);
    /// 缝隙未补——阻断（硬门）。
    pub const GAP_OPEN: B0Code = B0Code(0xB003);
    /// 降级失当——P0。
    pub const DEGRADE_BAD: B0Code = B0Code(0xB004);
    /// 十六面缺面。
    pub const FACE_MISS: B0Code = B0Code(0xB005);
    /// 三列不全。
    pub const COL_MISS: B0Code = B0Code(0xB006);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB001 => "检出红色风险，阻断并即时修复。",
            0xB002 => "安全证据链断裂，需重验。",
            0xB003 => "缝隙审计未补，硬门阻断。",
            0xB004 => "安全降级失当，P0。",
            0xB005 => "十六面终审缺面。",
            0xB006 => "攻击面三列不全。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十六面终审
// ─────────────────────────────────────────────────────────────

/// 审查面（十六面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 身份。
    Auth,
    /// 权限。
    Perm,
    /// 数据。
    Data,
    /// 网络。
    Network,
    /// 输入。
    Input,
    /// 输出。
    Output,
    /// 供应链。
    Supply,
    /// 密码。
    Crypto,
    /// 日志。
    Logging,
    /// 配置。
    Config,
    /// 依赖。
    Deps,
    /// 运行时。
    Runtime,
    /// 创作侵权面（R 新）。
    Infringe,
    /// 隐写面（R 新）。
    Stego,
    /// 经济面（R 新）。
    Economy,
    /// 文化面（R 新）。
    Culture,
}

impl Face {
    /// 全集。
    pub const ALL: [Face; 16] = [
        Face::Auth,
        Face::Perm,
        Face::Data,
        Face::Network,
        Face::Input,
        Face::Output,
        Face::Supply,
        Face::Crypto,
        Face::Logging,
        Face::Config,
        Face::Deps,
        Face::Runtime,
        Face::Infringe,
        Face::Stego,
        Face::Economy,
        Face::Culture,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Face::Auth => 0,
            Face::Perm => 1,
            Face::Data => 2,
            Face::Network => 3,
            Face::Input => 4,
            Face::Output => 5,
            Face::Supply => 6,
            Face::Crypto => 7,
            Face::Logging => 8,
            Face::Config => 9,
            Face::Deps => 10,
            Face::Runtime => 11,
            Face::Infringe => 12,
            Face::Stego => 13,
            Face::Economy => 14,
            Face::Culture => 15,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Face::Auth => "身份",
            Face::Perm => "权限",
            Face::Data => "数据",
            Face::Network => "网络",
            Face::Input => "输入",
            Face::Output => "输出",
            Face::Supply => "供应链",
            Face::Crypto => "密码",
            Face::Logging => "日志",
            Face::Config => "配置",
            Face::Deps => "依赖",
            Face::Runtime => "运行时",
            Face::Infringe => "创作侵权",
            Face::Stego => "隐写",
            Face::Economy => "经济",
            Face::Culture => "文化",
        }
    }

    /// 是否 R 新面。
    pub fn is_r_new(self) -> bool {
        matches!(self, Face::Infringe | Face::Stego | Face::Economy | Face::Culture)
    }
}

/// 面结论（红/黄/绿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    /// 未审。
    Unreviewed,
    /// 绿。
    Green,
    /// 黄。
    Yellow,
    /// 红。
    Red,
}

/// 十六面终审表。
#[derive(Clone, Debug)]
pub struct FaceReview {
    colors: [Color; 16],
    /// 即时修记录数。
    pub fixed: u32,
}

impl FaceReview {
    /// 新建。
    pub const fn new() -> Self {
        FaceReview {
            colors: [Color::Unreviewed; 16],
            fixed: 0,
        }
    }

    /// 记一面。
    pub fn judge(&mut self, f: Face, c: Color) -> bool {
        self.colors[f.ordinal()] = c;
        if c == Color::Red {
            self.fixed += 1;
        }
        true
    }

    /// 🔴 数。
    pub fn red_count(&self) -> usize {
        self.colors.iter().filter(|c| **c == Color::Red).count()
    }

    /// 未审数。
    pub fn unreviewed(&self) -> usize {
        self.colors
            .iter()
            .filter(|c| **c == Color::Unreviewed)
            .count()
    }

    /// 全审。
    pub fn complete(&self) -> bool {
        self.unreviewed() == 0
    }

    /// 🔴零。
    pub fn red_zero(&self) -> bool {
        self.red_count() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 攻击面三列 + 缝隙硬门
// ─────────────────────────────────────────────────────────────

/// 攻击面条目（三列：面/风险/处置）。
#[derive(Clone, Copy, Debug)]
pub struct ThreatRow {
    /// 面序号。
    pub face: u8,
    /// 风险级 0..3。
    pub risk: u8,
    /// 处置完成。
    pub handled: bool,
    /// 三列齐（面+风险+处置说明都有）。
    pub cols_full: bool,
}

impl ThreatRow {
    /// 构造。
    pub const fn new(face: u8, risk: u8) -> Self {
        ThreatRow {
            face,
            risk,
            handled: false,
            cols_full: false,
        }
    }

    /// 补齐三列。
    pub fn complete_cols(&mut self) {
        self.cols_full = true;
    }
}

/// 攻击面清单。
#[derive(Clone, Debug)]
pub struct ThreatList {
    rows: Vec<ThreatRow>,
}

impl ThreatList {
    /// 新建。
    pub fn new() -> Self {
        ThreatList { rows: Vec::new() }
    }

    /// 收一行。
    pub fn push(&mut self, r: ThreatRow) -> usize {
        self.rows.push(r);
        self.rows.len() - 1
    }

    /// 三列不全数。
    pub fn col_missing(&self) -> usize {
        self.rows.iter().filter(|r| !r.cols_full).count()
    }

    /// 未处置高风险数（risk>=2 且未处置）。
    pub fn open_high(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.risk >= 2 && !r.handled)
            .count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl Default for ThreatList {
    fn default() -> Self {
        ThreatList::new()
    }
}

/// 缝隙审计（硬门：存在未补缝隙即阻断）。
#[derive(Clone, Debug)]
pub struct GapAudit {
    /// 缝隙总数。
    pub total: u32,
    /// 已补数。
    pub sealed: u32,
    /// 证据链完好。
    pub evidence_ok: bool,
}

impl GapAudit {
    /// 新建。
    pub const fn new() -> Self {
        GapAudit {
            total: 0,
            sealed: 0,
            evidence_ok: true,
        }
    }

    /// 发现缝隙。
    pub fn find(&mut self) {
        self.total += 1;
    }

    /// 补一条。
    pub fn seal(&mut self) -> bool {
        if self.sealed < self.total {
            self.sealed += 1;
            true
        } else {
            false
        }
    }

    /// 未补数。
    pub fn open(&self) -> u32 {
        self.total.saturating_sub(self.sealed)
    }

    /// 硬门通过。
    pub fn gate_pass(&self) -> bool {
        self.open() == 0 && self.evidence_ok
    }
}

// ─────────────────────────────────────────────────────────────
// 安全分级执行
// ─────────────────────────────────────────────────────────────

/// 降级处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Degrade {
    /// 拒绝。
    Reject,
    /// 降级放行。
    Soften,
    /// 失当放行（违规）。
    BadPass,
}

/// 分级执行器。
#[derive(Clone, Debug)]
pub struct GradeExec {
    /// P0 处置数。
    pub p0: u32,
    /// P1 处置数。
    pub p1: u32,
    /// 失当放行数。
    pub bad_pass: u32,
}

impl GradeExec {
    /// 新建。
    pub const fn new() -> Self {
        GradeExec {
            p0: 0,
            p1: 0,
            bad_pass: 0,
        }
    }

    /// 执行一次。
    pub fn exec(&mut self, severity: u8, d: Degrade) {
        match d {
            Degrade::Reject => {
                if severity == 0 {
                    self.p0 += 1;
                } else {
                    self.p1 += 1;
                }
            }
            Degrade::Soften => {
                self.p1 += 1;
            }
            Degrade::BadPass => {
                self.bad_pass += 1;
            }
        }
    }

    /// 降级合规（无失当）。
    pub fn compliant(&self) -> bool {
        self.bad_pass == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3785 判据集。
pub fn run_vr10e_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3785");

    // 0xB005 十六面覆盖
    {
        let mut r = FaceReview::new();
        set.add("0xB005-初始未审16", r.unreviewed() == 16, "");
        for f in Face::ALL.iter() {
            r.judge(*f, Color::Green);
        }
        set.add("0xB005-全审", r.complete(), "");
        set.add("0xB005-红零", r.red_zero() && r.red_count() == 0, "");
    }

    // 0xB001 🔴即时修
    {
        let mut r = FaceReview::new();
        for f in Face::ALL.iter() {
            r.judge(*f, Color::Green);
        }
        r.judge(Face::Stego, Color::Red);
        set.add("0xB001-红检出", r.red_count() == 1, "");
        set.add("0xB001-即时修计数", r.fixed == 1, "");
        set.add("0xB001-红未清不过", !r.red_zero(), "");
        r.judge(Face::Stego, Color::Green);
        set.add("0xB001-修后红零", r.red_zero(), "");
    }

    // R 新面标记
    {
        set.add("0xB005-侵权是新面", Face::Infringe.is_r_new(), "");
        set.add("0xB005-隐写是新面", Face::Stego.is_r_new(), "");
        set.add("0xB005-经济是新面", Face::Economy.is_r_new(), "");
        set.add("0xB005-文化是新面", Face::Culture.is_r_new(), "");
        set.add("0xB005-身份非新面", !Face::Auth.is_r_new(), "");
    }

    // 0xB006 三列
    {
        let mut t = ThreatList::new();
        let mut row = ThreatRow::new(0, 2);
        t.push(row);
        set.add("0xB006-缺列检出", t.col_missing() == 1, "");
        if let Some(r) = t.rows.get_mut(0) {
            r.complete_cols();
        }
        set.add("0xB006-补齐后零", t.col_missing() == 0, "");
        set.add("0xB006-高风险未处置", t.open_high() == 1, "");
        if let Some(r) = t.rows.get_mut(0) {
            r.handled = true;
        }
        set.add("0xB006-处置后清", t.open_high() == 0, "");
        let _ = row;
    }

    // 0xB003 缝隙硬门
    {
        let mut g = GapAudit::new();
        set.add("0xB003-空审计过门", g.gate_pass(), "");
        g.find();
        g.find();
        set.add("0xB003-未补不过门", !g.gate_pass() && g.open() == 2, "");
        g.seal();
        set.add("0xB003-补一剩一", g.open() == 1 && !g.gate_pass(), "");
        g.seal();
        set.add("0xB003-补完过门", g.gate_pass(), "");
        g.evidence_ok = false;
        set.add("0xB002-证据断不过", !g.gate_pass(), "");
        set.add("0xB003-空补拒", !g.seal(), "");
    }

    // 0xB004 降级合规
    {
        let mut e = GradeExec::new();
        e.exec(0, Degrade::Reject);
        e.exec(1, Degrade::Soften);
        set.add("0xB004-合规执行", e.compliant() && e.p0 == 1 && e.p1 == 1, "");
        e.exec(0, Degrade::BadPass);
        set.add("0xB004-失当检出", !e.compliant() && e.bad_pass == 1, "");
    }

    // 面序自洽
    {
        let mut ok = true;
        for (i, f) in Face::ALL.iter().enumerate() {
            if f.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB005-面序自洽", ok, "");
    }

    set
}
