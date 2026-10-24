//! VE-F3753 · 创作无障碍与 S/E 协同
//!
//! 锚点：创作无障碍跨域协同（F3154 模式创作版复刻：E 域协同（创作产出→E 域令牌
//! 的对比度联动验证（复用门（复述单源；S 域协同（创作产出的语义描述→S 域语义库
//! （双签复述；N 域协同（创作工具焦点/语义投影对账（复述矩阵；跨域等效
//! （创作工具的直达等效承诺跨域一致（复述交叉对拍。
//!
//! 数据结构：门复用；双签复述；对账矩阵；交叉对拍复用。
//! 错误路径与降级矩阵：双签缺→不生效（复述）；对拍分歧→立案（复述）；门分叉→
//! 归一（复述红线实测）；领地冲突→裁决（复用）。
//! 跨批对接点：F3154/F2895 单源复用声明；E/S/N 三对端；F3760 收口输入。
//!
//! 诊断码段：独占 0x90xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x90xx）
// ─────────────────────────────────────────────────────────────

/// F3753 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S7Code(pub u16);

impl S7Code {
    /// 双签缺——不生效。
    pub const DUAL_SIGN_MISS: S7Code = S7Code(0x9001);
    /// 对拍分歧——立案。
    pub const CROSS_DIVERGE: S7Code = S7Code(0x9002);
    /// 门分叉——归一红线。
    pub const GATE_FORK: S7Code = S7Code(0x9003);
    /// 领地冲突——裁决。
    pub const TERRITORY_CLASH: S7Code = S7Code(0x9004);
    /// 单源漂移——对账。
    pub const SOURCE_DRIFT: S7Code = S7Code(0x9005);
    /// 语义库未对账——S 域。
    pub const SEMLIB_UNSYNC: S7Code = S7Code(0x9006);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9001 => "双签缺失，跨域变更不生效。",
            0x9002 => "交叉对拍分歧，跨域等效承诺不一致。",
            0x9003 => "E 域门分叉，需归一。",
            0x9004 => "跨域领地冲突，需裁决。",
            0x9005 => "与单源声明漂移，需对账。",
            0x9006 => "S 域语义库未对账，双签复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// E 域协同（对比度联动门，复用）
// ─────────────────────────────────────────────────────────────

/// 联动门。
#[derive(Clone, Debug, Default)]
pub struct ContrastGate {
    /// 创作产出对比度分（万分制）。
    pub output_contrast_bp: u16,
    /// E 域令牌对比度分（万分制）。
    pub token_contrast_bp: u16,
    /// 门是否已归一。
    pub unified: bool,
}

impl ContrastGate {
    /// 构造。
    pub fn new() -> ContrastGate {
        ContrastGate {
            output_contrast_bp: 0,
            token_contrast_bp: 0,
            unified: true,
        }
    }

    /// 联动验证：双方对比度均 ≥ 4500（45%）且一致。
    pub fn linked_ok(&self) -> bool {
        self.output_contrast_bp >= 4500
            && self.token_contrast_bp >= 4500
            && self.output_contrast_bp == self.token_contrast_bp
    }

    /// 分叉检测。
    pub fn forked(&self) -> bool {
        !self.unified || self.output_contrast_bp != self.token_contrast_bp
    }

    /// 归一：以令牌为准回填。
    pub fn unify(&mut self) {
        self.output_contrast_bp = self.token_contrast_bp;
        self.unified = true;
    }

    /// 设值。
    pub fn set_both(&mut self, output_bp: u16, token_bp: u16) {
        self.output_contrast_bp = output_bp;
        self.token_contrast_bp = token_bp;
        self.unified = output_bp == token_bp;
    }
}

// ─────────────────────────────────────────────────────────────
// S 域协同（语义库双签）
// ─────────────────────────────────────────────────────────────

/// 双签记录。
#[derive(Clone, Debug, Default)]
pub struct DualSign {
    /// 创作侧签名。
    pub sign_create: bool,
    /// S 域侧签名。
    pub sign_sem: bool,
    /// 是否已生效。
    pub effective: bool,
}

impl DualSign {
    /// 构造。
    pub fn new() -> DualSign {
        DualSign {
            sign_create: false,
            sign_sem: false,
            effective: false,
        }
    }

    /// 创作侧签。
    pub fn sign_a(&mut self) {
        self.sign_create = true;
        self.refresh();
    }

    /// S 域侧签。
    pub fn sign_b(&mut self) {
        self.sign_sem = true;
        self.refresh();
    }

    /// 双签齐才生效。
    fn refresh(&mut self) {
        self.effective = self.sign_create && self.sign_sem;
    }

    /// 生效。
    pub fn is_effective(&self) -> bool {
        self.effective
    }
}

/// 语义库对账。
#[derive(Clone, Debug, Default)]
pub struct SemLibSync {
    /// 创作侧描述数。
    pub create_count: u32,
    /// S 域侧条目数。
    pub sem_count: u32,
}

impl SemLibSync {
    /// 构造。
    pub fn new() -> SemLibSync {
        SemLibSync {
            create_count: 0,
            sem_count: 0,
        }
    }

    /// 对账一致。
    pub fn in_sync(&self) -> bool {
        self.create_count == self.sem_count
    }

    /// 注入漂移。
    pub fn inject_drift(&mut self, create: u32, sem: u32) {
        self.create_count = create;
        self.sem_count = sem;
    }
}

// ─────────────────────────────────────────────────────────────
// N 域协同（焦点/语义投影对账矩阵）
// ─────────────────────────────────────────────────────────────

/// 对账格。
#[derive(Clone, Debug, Default)]
pub struct ProjCell {
    /// 焦点 id。
    pub focus_id: u32,
    /// 投影 id（0=缺失）。
    pub proj_id: u32,
}

impl ProjCell {
    /// 构造。
    pub fn new(focus_id: u32, proj_id: u32) -> ProjCell {
        ProjCell { focus_id, proj_id }
    }

    /// 一致。
    pub fn aligned(&self) -> bool {
        self.proj_id != 0 && self.focus_id == self.proj_id
    }
}

/// 投影对账矩阵。
#[derive(Clone, Debug, Default)]
pub struct ProjMatrix {
    cells: Vec<ProjCell>,
}

impl ProjMatrix {
    /// 空矩阵。
    pub fn new() -> ProjMatrix {
        ProjMatrix { cells: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, c: ProjCell) {
        self.cells.push(c);
    }

    /// 扫描未对齐。
    pub fn scan_misalign(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for c in self.cells.iter() {
            if !c.aligned() {
                bad.push(c.focus_id);
            }
        }
        bad
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 交叉对拍（跨域等效）
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct CrossPair {
    /// 工具侧直达等效 id。
    pub tool_equiv: u32,
    /// 跨域侧等效 id。
    pub domain_equiv: u32,
}

impl CrossPair {
    /// 构造。
    pub fn new(tool_equiv: u32, domain_equiv: u32) -> CrossPair {
        CrossPair {
            tool_equiv,
            domain_equiv,
        }
    }

    /// 等效一致。
    pub fn match_ok(&self) -> bool {
        self.tool_equiv != 0 && self.tool_equiv == self.domain_equiv
    }
}

/// 交叉对拍表。
#[derive(Clone, Debug, Default)]
pub struct CrossLedger {
    pairs: Vec<CrossPair>,
}

impl CrossLedger {
    /// 空表。
    pub fn new() -> CrossLedger {
        CrossLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, p: CrossPair) {
        self.pairs.push(p);
    }

    /// 扫描分歧。
    pub fn scan_diverge(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.pairs.iter() {
            if !p.match_ok() {
                bad.push(p.tool_equiv);
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
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 跨域协同审查器。
#[derive(Clone, Debug, Default)]
pub struct CrossDomainAuditor {
    gate: ContrastGate,
    dual: DualSign,
    sem: SemLibSync,
    proj: ProjMatrix,
    cross: CrossLedger,
}

impl CrossDomainAuditor {
    /// 构造。
    pub fn new() -> CrossDomainAuditor {
        CrossDomainAuditor {
            gate: ContrastGate::new(),
            dual: DualSign::new(),
            sem: SemLibSync::new(),
            proj: ProjMatrix::new(),
            cross: CrossLedger::new(),
        }
    }

    /// 取门（可变）。
    pub fn gate_mut(&mut self) -> &mut ContrastGate {
        &mut self.gate
    }

    /// 取双签（可变）。
    pub fn dual_mut(&mut self) -> &mut DualSign {
        &mut self.dual
    }

    /// 取语义库（可变）。
    pub fn sem_mut(&mut self) -> &mut SemLibSync {
        &mut self.sem
    }

    /// 取投影矩阵（可变）。
    pub fn proj_mut(&mut self) -> &mut ProjMatrix {
        &mut self.proj
    }

    /// 取对拍（可变）。
    pub fn cross_mut(&mut self) -> &mut CrossLedger {
        &mut self.cross
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<S7Code> {
        let mut bad = Vec::new();
        if !self.dual.is_effective() {
            bad.push(S7Code::DUAL_SIGN_MISS);
        }
        if !self.cross.scan_diverge().is_empty() {
            bad.push(S7Code::CROSS_DIVERGE);
        }
        if self.gate.forked() {
            bad.push(S7Code::GATE_FORK);
        }
        if !self.sem.in_sync() {
            bad.push(S7Code::SEMLIB_UNSYNC);
        }
        if !self.proj.scan_misalign().is_empty() {
            bad.push(S7Code::SOURCE_DRIFT);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3753 判据集。
pub fn run_vr08m_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3753");

    // 0x9003 门归一
    {
        let mut g = ContrastGate::new();
        g.set_both(5000, 5000);
        out.add("0x9003-一致过", !g.forked() && g.linked_ok(), "");
        g.set_both(5000, 6000);
        out.add("0x9003-分叉检出", g.forked(), "");
        g.unify();
        out.add("0x9003-归一后过", !g.forked(), "");
    }

    // 0x9001 双签
    {
        let mut d = DualSign::new();
        out.add("0x9001-缺签不生效", !d.is_effective(), "");
        d.sign_a();
        out.add("0x9001-单签不生效", !d.is_effective(), "");
        d.sign_b();
        out.add("0x9001-双签生效", d.is_effective(), "");
    }

    // 0x9006 语义对账
    {
        let mut s = SemLibSync::new();
        out.add("0x9006-初始一致", s.in_sync(), "");
        s.inject_drift(10, 9);
        out.add("0x9006-漂移检出", !s.in_sync(), "");
    }

    // 0x9002 对拍
    {
        let mut c = CrossLedger::new();
        c.push(CrossPair::new(1, 1));
        out.add("0x9002-一致过", c.scan_diverge().is_empty(), "");
        c.push(CrossPair::new(2, 3));
        out.add("0x9002-分歧检出", c.scan_diverge().len() == 1, "");
    }

    // 0x9005 投影对账
    {
        let mut m = ProjMatrix::new();
        m.push(ProjCell::new(5, 5));
        out.add("0x9005-对齐过", m.scan_misalign().is_empty(), "");
        m.push(ProjCell::new(6, 0));
        out.add("0x9005-缺投影检出", m.scan_misalign().len() == 1, "");
    }

    out
}
