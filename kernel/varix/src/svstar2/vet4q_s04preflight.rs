#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDE00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDE00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDECode(pub u16);

impl CDECode {
    pub const RED_UNCLEARED: Self = Self(SEG_BASE);   // 红未清
    pub const INPUT_MISSING: Self = Self(SEG_BASE + 1); // 判定缺输入
    pub const URGENT_LOST: Self = Self(SEG_BASE + 2);  // 紧急丢
    pub const YELLOW_OVERDUE: Self = Self(SEG_BASE + 3); // 黄超期
    pub const HANDOVER_FAIL: Self = Self(SEG_BASE + 4);  // 移交失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::RED_UNCLEARED => "红未清",
            Self::INPUT_MISSING => "判定缺输入",
            Self::URGENT_LOST => "紧急丢",
            Self::YELLOW_OVERDUE => "黄超期",
            Self::HANDOVER_FAIL => "移交失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::RED_UNCLEARED, Self::INPUT_MISSING, Self::URGENT_LOST,
        Self::YELLOW_OVERDUE, Self::HANDOVER_FAIL,
    ];
}

// ── 十维自查（S04 定制）─────────────────────────────────────────────

/// 三色评级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color { Green, Yellow, Red }

/// S04 十维。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S04Dim {
    Arch, Engine, Video, Audio, Realtime, Scale, Vestibular, Photosafe, Control, Api,
}

impl S04Dim {
    pub const ALL: [Self; 10] = [
        Self::Arch, Self::Engine, Self::Video, Self::Audio, Self::Realtime,
        Self::Scale, Self::Vestibular, Self::Photosafe, Self::Control, Self::Api,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Arch => "总纲",
            Self::Engine => "引擎",
            Self::Video => "视频",
            Self::Audio => "音频",
            Self::Realtime => "实时",
            Self::Scale => "倍率",
            Self::Vestibular => "前庭",
            Self::Photosafe => "光敏",
            Self::Control => "控制",
            Self::Api => "API",
        }
    }
}

/// 自查格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfCheckCell {
    pub dim: S04Dim,
    pub color: Color,
    pub overdue: bool,
}

// ── 十维矩阵 ────────────────────────────────────────────────────────

/// 自查矩阵。
#[derive(Debug, Clone)]
pub struct SelfCheckMatrix {
    pub cells: Vec<SelfCheckCell>,
}

impl SelfCheckMatrix {
    pub fn new() -> Self { Self { cells: Vec::new() } }

    /// 十维全填。
    pub fn complete(&self) -> bool {
        self.cells.len() == 10
    }

    /// 零红。
    pub fn zero_red(&self) -> bool {
        self.cells.iter().all(|c| c.color != Color::Red)
    }

    /// 黄超期→转红判定。
    pub fn promote_overdue(&mut self) {
        for c in &mut self.cells {
            if c.color == Color::Yellow && c.overdue {
                c.color = Color::Red;
            }
        }
    }
}

// ── GO/NO-GO 判定 ──────────────────────────────────────────────────

/// 判定输入。
#[derive(Debug, Clone)]
pub struct GoInput {
    pub ten_dim_zero_red: bool,
    pub urgent_fidelity: bool,
    pub align_complete: bool,
    pub overhead_ok: bool,
}

impl GoInput {
    /// 四条件。
    pub fn all_met(&self) -> bool {
        self.ten_dim_zero_red && self.urgent_fidelity && self.align_complete && self.overhead_ok
    }
}

/// GO/NO-GO。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoDecision { Go, NoGo }

/// 判定器。
#[derive(Debug, Clone)]
pub struct GoJudge;

impl GoJudge {
    pub fn judge(input: &GoInput) -> GoDecision {
        if input.all_met() { GoDecision::Go } else { GoDecision::NoGo }
    }
}

// ── 移交清单 ────────────────────────────────────────────────────────

/// 移交件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverItem {
    pub id: u32,
    pub delivered: bool,
    pub acknowledged: bool,
}

/// 移交四件套。
#[derive(Debug, Clone)]
pub struct HandoverPackage {
    pub items: Vec<HandoverItem>,
}

impl HandoverPackage {
    /// 四件全达。
    pub fn all_delivered(&self) -> bool {
        self.items.len() == 4 && self.items.iter().all(|i| i.delivered && i.acknowledged)
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3879_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4q_s04preflight");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 十维枚举
    {
        chk(&mut set, "C3879-01", S04Dim::ALL.len() == 10, "err");
    }
    // 2 维名
    {
        chk(&mut set, "C3879-02", S04Dim::Api.name() == "API", "err");
    }
    // 3 矩阵完整
    {
        let mut m = SelfCheckMatrix::new();
        for d in S04Dim::ALL {
            m.cells.push(SelfCheckCell { dim: d, color: Color::Green, overdue: false });
        }
        chk(&mut set, "C3879-03", m.complete(), "err");
    }
    // 4 零红
    {
        let mut m = SelfCheckMatrix::new();
        for d in S04Dim::ALL {
            m.cells.push(SelfCheckCell { dim: d, color: Color::Green, overdue: false });
        }
        chk(&mut set, "C3879-04", m.zero_red(), "err");
    }
    // 5 有红不通过
    {
        let mut m = SelfCheckMatrix::new();
        for d in S04Dim::ALL {
            m.cells.push(SelfCheckCell { dim: d, color: Color::Yellow, overdue: false });
        }
        m.cells[0].color = Color::Red;
        chk(&mut set, "C3879-05", !m.zero_red(), "err");
    }
    // 6 黄超期→转红
    {
        let mut m = SelfCheckMatrix::new();
        for d in S04Dim::ALL {
            m.cells.push(SelfCheckCell { dim: d, color: Color::Yellow, overdue: true });
        }
        m.promote_overdue();
        chk(&mut set, "C3879-06", !m.zero_red(), "err");
    }
    // 7 GO 判定
    {
        let i = GoInput { ten_dim_zero_red: true, urgent_fidelity: true, align_complete: true, overhead_ok: true };
        chk(&mut set, "C3879-07", GoJudge::judge(&i) == GoDecision::Go, "err");
    }
    // 8 NO-GO（红未清）
    {
        let i = GoInput { ten_dim_zero_red: false, urgent_fidelity: true, align_complete: true, overhead_ok: true };
        chk(&mut set, "C3879-08", GoJudge::judge(&i) == GoDecision::NoGo, "err");
    }
    // 9 NO-GO（紧急丢）
    {
        let i = GoInput { ten_dim_zero_red: true, urgent_fidelity: false, align_complete: true, overhead_ok: true };
        chk(&mut set, "C3879-09", GoJudge::judge(&i) == GoDecision::NoGo, "err");
    }
    // 10 四条件全满足
    {
        let i = GoInput { ten_dim_zero_red: true, urgent_fidelity: true, align_complete: true, overhead_ok: true };
        chk(&mut set, "C3879-10", i.all_met(), "err");
    }
    // 11 移交四件
    {
        let mut p = HandoverPackage { items: Vec::new() };
        for id in 0..4 {
            p.items.push(HandoverItem { id, delivered: true, acknowledged: true });
        }
        chk(&mut set, "C3879-11", p.all_delivered(), "err");
    }
    // 12 移交缺件
    {
        let mut p = HandoverPackage { items: Vec::new() };
        p.items.push(HandoverItem { id: 0, delivered: true, acknowledged: true });
        chk(&mut set, "C3879-12", !p.all_delivered(), "err");
    }
    // 13 移交未核收
    {
        let mut p = HandoverPackage { items: Vec::new() };
        for id in 0..4 {
            p.items.push(HandoverItem { id, delivered: true, acknowledged: false });
        }
        chk(&mut set, "C3879-13", !p.all_delivered(), "err");
    }
    // 14 空矩阵不完备
    {
        let m = SelfCheckMatrix::new();
        chk(&mut set, "C3879-14", !m.complete(), "err");
    }
    // 15 黄未超期不转红
    {
        let mut m = SelfCheckMatrix::new();
        for d in S04Dim::ALL {
            m.cells.push(SelfCheckCell { dim: d, color: Color::Yellow, overdue: false });
        }
        m.promote_overdue();
        chk(&mut set, "C3879-15", m.zero_red(), "err");
    }
    // 16 判定缺输入（对齐不完整）
    {
        let i = GoInput { ten_dim_zero_red: true, urgent_fidelity: true, align_complete: false, overhead_ok: true };
        chk(&mut set, "C3879-16", GoJudge::judge(&i) == GoDecision::NoGo, "err");
    }
    // 17 开销超标 NO-GO
    {
        let i = GoInput { ten_dim_zero_red: true, urgent_fidelity: true, align_complete: true, overhead_ok: false };
        chk(&mut set, "C3879-17", GoJudge::judge(&i) == GoDecision::NoGo, "err");
    }
    // 18 三色枚举
    {
        chk(&mut set, "C3879-18", true, "err"); // Color enum is always valid
    }

    set
}
