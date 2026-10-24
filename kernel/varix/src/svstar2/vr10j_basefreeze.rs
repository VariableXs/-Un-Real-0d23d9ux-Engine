//! VE-F3790 · R 域基准与预算基线冻结（R10 收口组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3790`
//!
//! **判据（锚点原文）**：六域五用、1500 条、20 维八用、非冻结阻断、ADR 留痕、
//! 判据。
//!
//! **本条是什么**：五件资产基线化（校准画像/基准不变/预算不变/心流不变/
//! 破基必 ADR）；三列冻结点表；>5% 偏差触发修正。非冻结即阻断后续工序。
//!
//! 诊断码段：独占 0xB5xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB5xx）
// ─────────────────────────────────────────────────────────────

/// F3790 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B5Code(pub u16);

impl B5Code {
    /// 偏差>5%——ADR。
    pub const DEVIATION: B5Code = B5Code(0xB501);
    /// 破基线无 ADR——P1。
    pub const BREAK_NO_ADR: B5Code = B5Code(0xB502);
    /// 画像失实——校准。
    pub const PROFILE_BAD: B5Code = B5Code(0xB503);
    /// 未冻结——阻断。
    pub const NOT_FROZEN: B5Code = B5Code(0xB504);
    /// 五件缺件。
    pub const ITEM_MISS: B5Code = B5Code(0xB505);
    /// 冻结点缺。
    pub const POINT_MISS: B5Code = B5Code(0xB506);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB501 => "基线偏差超 5%，触发 ADR。",
            0xB502 => "破基线未立 ADR，P1。",
            0xB503 => "校准画像失实，需重校。",
            0xB504 => "基线未冻结，后续工序阻断。",
            0xB505 => "五件基线缺件。",
            0xB506 => "冻结点表缺项。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 五件基线
// ─────────────────────────────────────────────────────────────

/// 基线件（五件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseItem {
    /// 校准画像。
    CalibProfile,
    /// 基准不变。
    BenchInvar,
    /// 预算不变。
    BudgetInvar,
    /// 心流不变。
    FlowInvar,
    /// 破基必 ADR。
    BreakAdr,
}

impl BaseItem {
    /// 全集。
    pub const ALL: [BaseItem; 5] = [
        BaseItem::CalibProfile,
        BaseItem::BenchInvar,
        BaseItem::BudgetInvar,
        BaseItem::FlowInvar,
        BaseItem::BreakAdr,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            BaseItem::CalibProfile => 0,
            BaseItem::BenchInvar => 1,
            BaseItem::BudgetInvar => 2,
            BaseItem::FlowInvar => 3,
            BaseItem::BreakAdr => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            BaseItem::CalibProfile => "校准画像",
            BaseItem::BenchInvar => "基准不变",
            BaseItem::BudgetInvar => "预算不变",
            BaseItem::FlowInvar => "心流不变",
            BaseItem::BreakAdr => "破基必ADR",
        }
    }
}

/// 基线册。
#[derive(Clone, Debug)]
pub struct BaseBook {
    set: [bool; 5],
    /// 冻结完成。
    pub frozen: bool,
}

impl BaseBook {
    /// 新建。
    pub const fn new() -> Self {
        BaseBook {
            set: [false; 5],
            frozen: false,
        }
    }

    /// 置件。
    pub fn place(&mut self, b: BaseItem) -> bool {
        let i = b.ordinal();
        if self.set[i] {
            false
        } else {
            self.set[i] = true;
            true
        }
    }

    /// 缺件数。
    pub fn missing(&self) -> usize {
        self.set.iter().filter(|s| !**s).count()
    }

    /// 五件齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 冻结（五件齐才可冻）。
    pub fn freeze(&mut self) -> bool {
        if self.complete() {
            self.frozen = true;
            true
        } else {
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 偏差 + ADR
// ─────────────────────────────────────────────────────────────

/// 基线偏差记录。
#[derive(Clone, Copy, Debug)]
pub struct Deviation {
    /// 偏差万分比。
    pub bp: u16,
    /// ADR 已立。
    pub adr: bool,
    /// 破基标志。
    pub broke: bool,
}

impl Deviation {
    /// 新建。
    pub const fn new(bp: u16) -> Self {
        Deviation {
            bp,
            adr: false,
            broke: false,
        }
    }

    /// 超 5%（500 基点）。
    pub fn over_5(&self) -> bool {
        self.bp > 500
    }

    /// 立 ADR。
    pub fn file_adr(&mut self) {
        self.adr = true;
        self.broke = true;
    }

    /// 合规（超偏必须有 ADR）。
    pub fn compliant(&self) -> bool {
        if self.over_5() {
            self.adr
        } else {
            true
        }
    }
}

/// 偏差账。
#[derive(Clone, Debug)]
pub struct DevLedger {
    recs: Vec<Deviation>,
}

impl DevLedger {
    /// 新建。
    pub fn new() -> Self {
        DevLedger { recs: Vec::new() }
    }

    /// 记一条。
    pub fn push(&mut self, d: Deviation) -> usize {
        self.recs.push(d);
        self.recs.len() - 1
    }

    /// 可变取。
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut Deviation> {
        self.recs.get_mut(idx)
    }

    /// 不合规数。
    pub fn non_compliant(&self) -> usize {
        self.recs.iter().filter(|d| !d.compliant()).count()
    }

    /// 破基数。
    pub fn broke_count(&self) -> usize {
        self.recs.iter().filter(|d| d.broke).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
}

impl Default for DevLedger {
    fn default() -> Self {
        DevLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 冻结点表（三列）
// ─────────────────────────────────────────────────────────────

/// 冻结点。
#[derive(Clone, Copy, Debug)]
pub struct FreezePoint {
    /// 指标 id。
    pub metric: u16,
    /// 基线值万分比。
    pub base_bp: u16,
    /// 三列齐（指标/基线/责任方）。
    pub cols: bool,
}

impl FreezePoint {
    /// 构造。
    pub const fn new(metric: u16, base_bp: u16) -> Self {
        FreezePoint {
            metric,
            base_bp,
            cols: false,
        }
    }

    /// 补齐责任方列。
    pub fn complete_cols(&mut self) {
        self.cols = true;
    }

    /// 当前值相对基线偏差万分比。
    pub fn dev_of(&self, now_bp: u16) -> u16 {
        let b = if self.base_bp == 0 {
            1
        } else {
            self.base_bp
        };
        let d = if now_bp > self.base_bp {
            now_bp - self.base_bp
        } else {
            self.base_bp - now_bp
        };
        ((d as u32) * 10_000 / (b as u32)) as u16
    }
}

/// 冻结点表。
#[derive(Clone, Debug)]
pub struct FreezeTable {
    pts: Vec<FreezePoint>,
}

impl FreezeTable {
    /// 新建。
    pub fn new() -> Self {
        FreezeTable { pts: Vec::new() }
    }

    /// 加点。
    pub fn push(&mut self, p: FreezePoint) -> usize {
        self.pts.push(p);
        self.pts.len() - 1
    }

    /// 三列不全数。
    pub fn col_missing(&self) -> usize {
        self.pts.iter().filter(|p| !p.cols).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.pts.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }
}

impl Default for FreezeTable {
    fn default() -> Self {
        FreezeTable::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3790 判据集。
pub fn run_vr10j_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3790");

    // 0xB505 五件基线
    {
        let mut b = BaseBook::new();
        set.add("0xB505-初始缺五", b.missing() == 5, "");
        for it in BaseItem::ALL.iter() {
            b.place(*it);
        }
        set.add("0xB505-五件齐", b.complete(), "");
        set.add("0xB505-重置拒", !b.place(BaseItem::CalibProfile), "");
    }

    // 0xB504 冻结门
    {
        let mut b = BaseBook::new();
        set.add("0xB504-缺件不冻", !b.freeze() && !b.frozen, "");
        for it in BaseItem::ALL.iter() {
            b.place(*it);
        }
        set.add("0xB504-齐件冻", b.freeze() && b.frozen, "");
    }

    // 0xB501 5% 偏差 ADR
    {
        let d = Deviation::new(600);
        set.add("0xB501-6%超偏", d.over_5(), "");
        set.add("0xB502-无ADR不合规", !d.compliant(), "");
        let mut d2 = Deviation::new(600);
        d2.file_adr();
        set.add("0xB501-立ADR合规", d2.compliant() && d2.broke, "");
        let d3 = Deviation::new(300);
        set.add("0xB501-3%不超", !d3.over_5() && d3.compliant(), "");
    }

    // 偏差账
    {
        let mut l = DevLedger::new();
        set.add("0xB501-空账", l.is_empty() && l.non_compliant() == 0, "");
        l.push(Deviation::new(700));
        l.push(Deviation::new(100));
        set.add("0xB502-一不合规", l.non_compliant() == 1, "");
        if let Some(d) = l.get_mut(0) {
            d.file_adr();
        }
        set.add("0xB502-补ADR后清", l.non_compliant() == 0, "");
        set.add("0xB502-破基计数", l.broke_count() == 1, "");
    }

    // 0xB506 冻结点表
    {
        let mut t = FreezeTable::new();
        set.add("0xB506-初始空", t.is_empty(), "");
        let mut p = FreezePoint::new(1, 1000);
        t.push(p);
        set.add("0xB506-缺列检出", t.col_missing() == 1, "");
        if let Some(x) = t.pts.get_mut(0) {
            x.complete_cols();
        }
        set.add("0xB506-补齐零缺", t.col_missing() == 0, "");
        set.add("0xB501-偏差算", p.dev_of(1100) == 1000, "");
        let _ = &mut p;
    }

    // 件序自洽
    {
        let mut ok = true;
        for (i, b) in BaseItem::ALL.iter().enumerate() {
            if b.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB505-件序自洽", ok, "");
    }

    set
}
