//! VE-F3764 · R07-R08 组自查核验
//!
//! 锚点：R07-R08 组自查核验——两组联合核验（R07 性能：中断硬预算/五段分账/数据
//! 保全第一列/总线 N² 红线/deadline 台账——20 项；R08 无障碍：双维分册/五关实测/
//! 产出豁免 P0/眼动 80%/同源保真——20 项；三段核验复刻）；特色专项（心流证据
//! 核验（中断率剧本线实测记录核验（心流红线联动复述；用户数据作恶核验（伪造
//! 用户数据 P0 的独立审计证据核验（复述独立审计红线。
//!
//! 数据结构：两组核验表（40 项）；心流证据核验；独立审计核验。
//! 跨批对接点：两组档案；F3734/F3755 对端；F3765 输入。
//!
//! 诊断码段：独占 0x9Bxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Bxx）
// ─────────────────────────────────────────────────────────────

/// F3764 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W9Code(pub u16);

impl W9Code {
    /// 心流伪造——作恶红线。
    pub const FLOW_FAKE: W9Code = W9Code(0x9B01);
    /// 独立审计缺——重审红线。
    pub const AUDIT_INDEP_MISS: W9Code = W9Code(0x9B02);
    /// 40 项缺——补核验。
    pub const ITEM_GAP: W9Code = W9Code(0x9B03);
    /// 三段缺——复刻。
    pub const PHASE_MISS: W9Code = W9Code(0x9B04);
    /// 数据保全 P0——保全红线。
    pub const DATA_LOSS_P0: W9Code = W9Code(0x9B05);
    /// 分歧未立案。
    pub const CONFLICT_OPEN: W9Code = W9Code(0x9B06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9B01 => "心流证据伪造，作恶红线复测不通过。",
            0x9B02 => "独立审计缺失，需重审。",
            0x9B03 => "两组 40 项核验有缺口。",
            0x9B04 => "三段核验缺段，复刻不完整。",
            0x9B05 => "数据保全 P0 触发，保全红线复测。",
            0x9B06 => "核验分歧未立案。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 两组核验（R07/R08 × 20 项）
// ─────────────────────────────────────────────────────────────

/// 组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R78Group {
    /// R07 性能。
    R07,
    /// R08 无障碍。
    R08,
}

impl R78Group {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            R78Group::R07 => 0,
            R78Group::R08 => 1,
        }
    }

    /// 全集。
    pub fn all() -> [R78Group; 2] {
        [R78Group::R07, R78Group::R08]
    }
}

/// 组核验表。
#[derive(Clone, Debug)]
pub struct G78Check {
    checked: [u32; 2],
    phases: [[bool; 3]; 2],
}

impl G78Check {
    /// 空表。
    pub fn new() -> G78Check {
        G78Check {
            checked: [0; 2],
            phases: [[false; 3]; 2],
        }
    }

    /// 记一项。
    pub fn check_item(&mut self, g: R78Group) -> u32 {
        let i = g.index() as usize;
        self.checked[i] += 1;
        self.checked[i]
    }

    /// 记一段。
    pub fn mark_phase(&mut self, g: R78Group, phase: u8) -> bool {
        if phase >= 3 {
            return false;
        }
        self.phases[g.index() as usize][phase as usize] = true;
        true
    }

    /// 40 项齐。
    pub fn all_complete(&self) -> bool {
        self.total_items() >= 40
    }

    /// 总项。
    pub fn total_items(&self) -> u32 {
        self.checked.iter().sum()
    }

    /// 三段齐。
    pub fn all_phases(&self) -> bool {
        for row in self.phases.iter() {
            if !row.iter().all(|b| *b) {
                return false;
            }
        }
        true
    }
}

impl Default for G78Check {
    fn default() -> G78Check {
        G78Check::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 心流证据核验
// ─────────────────────────────────────────────────────────────

/// 心流记录。
#[derive(Clone, Debug)]
pub struct FlowRecord {
    /// 中断率剧本线（万分制）。
    pub interrupt_bp: u16,
    /// 是否实测（非伪造）。
    pub hands_on: bool,
    /// 是否达标（≤剧本线）。
    pub meets: bool,
}

impl FlowRecord {
    /// 构造。
    pub fn new(interrupt_bp: u16, hands_on: bool) -> FlowRecord {
        FlowRecord {
            interrupt_bp,
            hands_on,
            meets: interrupt_bp <= 1000, // 剧本线 10%
        }
    }

    /// 合规：实测 + 达标。
    pub fn ok(&self) -> bool {
        self.hands_on && self.meets
    }
}

/// 心流核验器。
#[derive(Clone, Debug, Default)]
pub struct FlowAuditor {
    records: Vec<FlowRecord>,
}

impl FlowAuditor {
    /// 空。
    pub fn new() -> FlowAuditor {
        FlowAuditor {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, r: FlowRecord) {
        self.records.push(r);
    }

    /// 扫描伪造/不达标。
    pub fn scan_bad(&self) -> Vec<u16> {
        let mut bad = Vec::new();
        for r in self.records.iter() {
            if !r.ok() {
                bad.push(r.interrupt_bp);
            }
        }
        bad
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 独立审计核验（伪造用户数据 P0）
// ─────────────────────────────────────────────────────────────

/// 独立审计记录。
#[derive(Clone, Debug)]
pub struct IndepAudit {
    /// 是否独立审计（非自我声明）。
    pub independent: bool,
    /// 是否发现伪造用户数据。
    pub fake_found: bool,
    /// 是否闭环（若发现则有处置）。
    pub closure: bool,
}

impl IndepAudit {
    /// 构造。
    pub fn new(independent: bool) -> IndepAudit {
        IndepAudit {
            independent,
            fake_found: false,
            closure: true,
        }
    }

    /// 合规：独立 + （无发现 或 发现且闭环）。
    pub fn ok(&self) -> bool {
        self.independent && (!self.fake_found || self.closure)
    }
}

/// 独立审计核验器。
#[derive(Clone, Debug, Default)]
pub struct IndepAuditor {
    records: Vec<IndepAudit>,
}

impl IndepAuditor {
    /// 空。
    pub fn new() -> IndepAuditor {
        IndepAuditor {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, r: IndepAudit) {
        self.records.push(r);
    }

    /// 扫描不合规。
    pub fn scan_bad(&self) -> Vec<usize> {
        let mut bad = Vec::new();
        for (i, r) in self.records.iter().enumerate() {
            if !r.ok() {
                bad.push(i);
            }
        }
        bad
    }

    /// 有独立审计记录。
    pub fn has_independent(&self) -> bool {
        self.records.iter().any(|r| r.independent)
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3764 判据集。
pub fn run_vr09d_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3764");

    // 0x9B03 40 项
    {
        let mut g = G78Check::new();
        out.add("0x9B03-空表不全", !g.all_complete(), "");
        for _ in 0..20 {
            for grp in R78Group::all() {
                g.check_item(grp);
            }
        }
        out.add("0x9B03-40项齐", g.all_complete(), "");
    }

    // 0x9B04 三段
    {
        let mut g = G78Check::new();
        for grp in R78Group::all() {
            for ph in 0..3u8 {
                g.mark_phase(grp, ph);
            }
        }
        out.add("0x9B04-三段齐", g.all_phases(), "");
    }

    // 0x9B01 心流
    {
        let mut f = FlowAuditor::new();
        f.add(FlowRecord::new(800, true));
        out.add("0x9B01-达标实测过", f.scan_bad().is_empty(), "");
        f.add(FlowRecord::new(1500, true));
        out.add("0x9B01-超标检出", f.scan_bad().len() == 1, "");
    }

    // 0x9B02 独立审计
    {
        let mut a = IndepAuditor::new();
        let mut r = IndepAudit::new(true);
        r.fake_found = true;
        r.closure = false;
        a.add(r);
        out.add("0x9B02-无闭环检出", a.scan_bad().len() == 1, "");
        let good = IndepAudit::new(true);
        a.add(good);
        out.add("0x9B02-有独立", a.has_independent(), "");
    }

    // 0x9B05 数据保全
    {
        let mut f = FlowAuditor::new();
        f.add(FlowRecord::new(2000, false));
        out.add("0x9B05-伪造检出", f.scan_bad().len() == 1, "");
    }

    out
}
