//! VE-F3765 · 组间两两对账
//!
//! 锚点：组间两两对账——对账（28 对×三类=84 核查点复述第七用；R 域枢纽边界
//! （R01×R02 流水/R02×R03 画布资产/R04×全偏好注入/R06×全质量门/R07×全预算/
//! R08×全双维——六枢纽优先（放射风险复述）；正向核验（F3003 令牌/F3202 图/
//! F3605 预览隔离/F3610 预算四单源双方声明一致断言（复述。
//!
//! 数据结构：对账矩阵（84 点）；枢纽排序复述；正向核验记录。
//! 跨批对接点：八组档案；F3766 清点输入；F2970 模式复用。
//!
//! 诊断码段：独占 0x9Cxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Cxx）
// ─────────────────────────────────────────────────────────────

/// F3765 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P9Code(pub u16);

impl P9Code {
    /// 对账分歧——裁决。
    pub const PAIR_DIVERGE: P9Code = P9Code(0x9C01);
    /// 单边缺失——补登。
    pub const UNILATERAL: P9Code = P9Code(0x9C02);
    /// 枢纽未优先——放射风险。
    pub const HUB_UNPRIO: P9Code = P9Code(0x9C03);
    /// 84 点不全——补对账。
    pub const POINTS_INCOMPLETE: P9Code = P9Code(0x9C04);
    /// 正向核验缺——四单源。
    pub const FORWARD_MISS: P9Code = P9Code(0x9C05);
    /// 裁决未记录——立案。
    pub const ARBITRATION_MISS: P9Code = P9Code(0x9C06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9C01 => "对账分歧，需裁决。",
            0x9C02 => "单边声明缺失，需补登。",
            0x9C03 => "枢纽边界未优先对账，放射风险。",
            0x9C04 => "84 核查点不全。",
            0x9C05 => "四单源正向核验缺失。",
            0x9C06 => "裁决未记录，立案不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 枢纽边界（六枢纽）
// ─────────────────────────────────────────────────────────────

/// 枢纽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hub {
    /// R01×R02 流水。
    Pipeline,
    /// R02×R03 画布资产。
    CanvasAsset,
    /// R04×全偏好注入。
    PrefInject,
    /// R06×全质量门。
    QualGate,
    /// R07×全预算。
    Budget,
    /// R08×全双维。
    DualDim,
}

impl Hub {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Hub::Pipeline => 0,
            Hub::CanvasAsset => 1,
            Hub::PrefInject => 2,
            Hub::QualGate => 3,
            Hub::Budget => 4,
            Hub::DualDim => 5,
        }
    }

    /// 全集（六枢纽）。
    pub fn all() -> [Hub; 6] {
        [
            Hub::Pipeline,
            Hub::CanvasAsset,
            Hub::PrefInject,
            Hub::QualGate,
            Hub::Budget,
            Hub::DualDim,
        ]
    }
}

/// 枢纽对账。
#[derive(Clone, Debug)]
pub struct HubCheck {
    /// 枢纽。
    pub hub: Hub,
    /// 是否已优先对账。
    pub prioritized: bool,
    /// 是否一致。
    pub consistent: bool,
}

impl HubCheck {
    /// 构造。
    pub fn new(hub: Hub) -> HubCheck {
        HubCheck {
            hub,
            prioritized: false,
            consistent: false,
        }
    }

    /// 合规：已优先 + 一致。
    pub fn ok(&self) -> bool {
        self.prioritized && self.consistent
    }
}

/// 枢纽台账。
#[derive(Clone, Debug, Default)]
pub struct HubLedger {
    checks: Vec<HubCheck>,
}

impl HubLedger {
    /// 空。
    pub fn new() -> HubLedger {
        HubLedger {
            checks: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, c: HubCheck) {
        self.checks.push(c);
    }

    /// 扫描不合规。
    pub fn scan_bad(&self) -> Vec<Hub> {
        let mut bad = Vec::new();
        for c in self.checks.iter() {
            if !c.ok() {
                bad.push(c.hub);
            }
        }
        bad
    }

    /// 六枢纽齐。
    pub fn all_registered(&self) -> bool {
        for h in Hub::all() {
            if !self.checks.iter().any(|c| c.hub == h) {
                return false;
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────
// 84 核查点（28 对 × 三类）
// ─────────────────────────────────────────────────────────────

/// 对账格。
#[derive(Clone, Debug, Default)]
pub struct PairCell {
    /// 是否已查。
    pub checked: bool,
    /// 是否一致。
    pub consistent: bool,
}

impl PairCell {
    /// 构造。
    pub fn new() -> PairCell {
        PairCell {
            checked: false,
            consistent: false,
        }
    }

    /// 查通过。
    pub fn mark_pass(&mut self) {
        self.checked = true;
        self.consistent = true;
    }

    /// 查失败。
    pub fn mark_fail(&mut self) {
        self.checked = true;
        self.consistent = false;
    }
}

/// 84 点矩阵（28 对 × 3 类 = 84）。
#[derive(Clone, Debug)]
pub struct PairMatrix {
    cells: Vec<PairCell>,
}

impl PairMatrix {
    /// 空矩阵（84 格）。
    pub fn new() -> PairMatrix {
        let mut cells = Vec::new();
        for _ in 0..84 {
            cells.push(PairCell::new());
        }
        PairMatrix { cells }
    }

    /// 跑通一点。
    pub fn pass(&mut self, idx: usize) -> bool {
        if idx >= 84 {
            return false;
        }
        self.cells[idx].mark_pass();
        true
    }

    /// 跑失败一点。
    pub fn fail(&mut self, idx: usize) -> bool {
        if idx >= 84 {
            return false;
        }
        self.cells[idx].mark_fail();
        true
    }

    /// 未查数。
    pub fn unchecked(&self) -> usize {
        self.cells.iter().filter(|c| !c.checked).count()
    }

    /// 不一致数。
    pub fn inconsistent(&self) -> usize {
        self.cells.iter().filter(|c| c.checked && !c.consistent).count()
    }

    /// 全覆盖。
    pub fn complete(&self) -> bool {
        self.unchecked() == 0
    }

    /// 总点数。
    pub fn total(&self) -> usize {
        84
    }
}

impl Default for PairMatrix {
    fn default() -> PairMatrix {
        PairMatrix::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 正向核验（四单源双方声明一致）
// ─────────────────────────────────────────────────────────────

/// 正向核验对。
#[derive(Clone, Debug)]
pub struct ForwardPair {
    /// 单源 id。
    pub source_id: u32,
    /// 甲方声明 id。
    pub decl_a: u32,
    /// 乙方声明 id。
    pub decl_b: u32,
}

impl ForwardPair {
    /// 构造。
    pub fn new(source_id: u32, decl_a: u32, decl_b: u32) -> ForwardPair {
        ForwardPair {
            source_id,
            decl_a,
            decl_b,
        }
    }

    /// 一致。
    pub fn match_ok(&self) -> bool {
        self.decl_a != 0 && self.decl_a == self.decl_b
    }
}

/// 正向核验表。
#[derive(Clone, Debug, Default)]
pub struct ForwardLedger {
    pairs: Vec<ForwardPair>,
}

impl ForwardLedger {
    /// 空。
    pub fn new() -> ForwardLedger {
        ForwardLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: ForwardPair) {
        self.pairs.push(p);
    }

    /// 扫描分歧。
    pub fn scan_diverge(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.pairs.iter() {
            if !p.match_ok() {
                bad.push(p.source_id);
            }
        }
        bad
    }

    /// 对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3765 判据集。
pub fn run_vr09e_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3765");

    // 0x9C03 枢纽优先
    {
        let mut l = HubLedger::new();
        let mut c = HubCheck::new(Hub::Pipeline);
        c.prioritized = true;
        c.consistent = true;
        l.add(c);
        out.add("0x9C03-枢纽过", l.scan_bad().is_empty(), "");
        l.add(HubCheck::new(Hub::Budget));
        out.add("0x9C03-未优先检出", l.scan_bad().len() == 1, "");
    }

    // 0x9C04 84 点
    {
        let mut m = PairMatrix::new();
        out.add("0x9C04-初始全未查", m.unchecked() == 84, "");
        for i in 0..84 {
            m.pass(i);
        }
        out.add("0x9C04-全覆盖", m.complete() && m.inconsistent() == 0, "");
        out.add("0x9C04-越界拒", !m.pass(84), "");
    }

    // 0x9C05 正向核验
    {
        let mut f = ForwardLedger::new();
        f.add(ForwardPair::new(1, 100, 100));
        out.add("0x9C05-一致过", f.scan_diverge().is_empty(), "");
        f.add(ForwardPair::new(2, 200, 201));
        out.add("0x9C05-分歧检出", f.scan_diverge().len() == 1, "");
    }

    // 0x9C01/0x9C02 分歧
    {
        let mut m = PairMatrix::new();
        m.fail(0);
        out.add("0x9C01-不一致检出", m.inconsistent() == 1, "");
    }

    out
}
