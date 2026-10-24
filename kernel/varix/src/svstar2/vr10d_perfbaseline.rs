//! VE-F3784 · R 域性能总基准（R10 收口组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3784`
//!
//! **判据（锚点原文）**：五资产、十段瀑布、心流入册、20% ADR、四用复刻、
//! 判据。
//!
//! **本条是什么**：五资产总册化（基准/分账/水位/心流/功耗）；端到端
//! 十段瀑布双列；功耗矩阵；与 F2468 对账（偏差 >20% 触发 ADR）；
//! 心流数据缺 → 补（红线联动）。
//!
//! 诊断码段：独占 0xAFxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xAFxx）
// ─────────────────────────────────────────────────────────────

/// F3784 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AFCode(pub u16);

impl AFCode {
    /// 偏差 >20%——ADR。
    pub const DEVIATION: AFCode = AFCode(0xAF01);
    /// 跨段异常——移交。
    pub const SEG_ANOMALY: AFCode = AFCode(0xAF02);
    /// 心流数据缺——补。
    pub const FLOW_MISS: AFCode = AFCode(0xAF03);
    /// 画像失实——校准。
    pub const PROFILE_BAD: AFCode = AFCode(0xAF04);
    /// 五资产缺资产。
    pub const ASSET_MISS: AFCode = AFCode(0xAF05);
    /// 瀑布缺段。
    pub const FALL_MISS: AFCode = AFCode(0xAF06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xAF01 => "对账偏差超 20%，触发 ADR。",
            0xAF02 => "跨段性能异常，移交归因。",
            0xAF03 => "心流数据缺失，红线联动补齐。",
            0xAF04 => "画像失实，需校准。",
            0xAF05 => "五资产总册缺资产。",
            0xAF06 => "十段瀑布缺段。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 五资产
// ─────────────────────────────────────────────────────────────

/// 资产（五件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asset {
    /// 基准。
    Bench,
    /// 分账。
    Split,
    /// 水位。
    Watermark,
    /// 心流。
    Flow,
    /// 功耗。
    Power,
}

impl Asset {
    /// 全集。
    pub const ALL: [Asset; 5] = [
        Asset::Bench,
        Asset::Split,
        Asset::Watermark,
        Asset::Flow,
        Asset::Power,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Asset::Bench => 0,
            Asset::Split => 1,
            Asset::Watermark => 2,
            Asset::Flow => 3,
            Asset::Power => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Asset::Bench => "基准",
            Asset::Split => "分账",
            Asset::Watermark => "水位",
            Asset::Flow => "心流",
            Asset::Power => "功耗",
        }
    }
}

/// 总册。
#[derive(Clone, Debug)]
pub struct MasterBook {
    present: [bool; 5],
    /// 心流数据在册（红线联动）。
    pub flow_data: bool,
}

impl MasterBook {
    /// 新建。
    pub const fn new() -> Self {
        MasterBook {
            present: [false; 5],
            flow_data: false,
        }
    }

    /// 收资产。
    pub fn intake(&mut self, a: Asset) -> bool {
        let i = a.ordinal();
        if self.present[i] {
            false
        } else {
            self.present[i] = true;
            if a == Asset::Flow {
                self.flow_data = true;
            }
            true
        }
    }

    /// 缺资产名。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for a in Asset::ALL.iter() {
            if !self.present[a.ordinal()] {
                out.push(a.name());
            }
        }
        out
    }

    /// 五件齐。
    pub fn complete(&self) -> bool {
        self.present.iter().filter(|p| **p).count() == 5
    }

    /// 心流在册。
    pub fn flow_ok(&self) -> bool {
        self.flow_data
    }
}

impl Default for MasterBook {
    fn default() -> Self {
        MasterBook::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 十段瀑布（双列）
// ─────────────────────────────────────────────────────────────

/// 瀑布行（段耗时双列：基线/实测，单位微秒）。
#[derive(Clone, Copy, Debug)]
pub struct FallRow {
    /// 段序号 0..10。
    pub seg: u8,
    /// 基线微秒。
    pub base_us: u32,
    /// 实测微秒。
    pub actual_us: u32,
    /// 跨段异常标记。
    pub anomaly: bool,
}

impl FallRow {
    /// 构造。
    pub const fn new(seg: u8, base_us: u32, actual_us: u32) -> Self {
        FallRow {
            seg,
            base_us,
            actual_us,
            anomaly: false,
        }
    }

    /// 偏差万分比（|a-b|*10000/max(b,1)）。
    pub fn dev_bp(&self) -> u32 {
        let b = if self.base_us == 0 { 1 } else { self.base_us };
        let d = if self.actual_us > self.base_us {
            self.actual_us - self.base_us
        } else {
            self.base_us - self.actual_us
        };
        d * 10_000 / b
    }

    /// 超 20%（2000 基点）。
    pub fn over_20(&self) -> bool {
        self.dev_bp() > 2_000
    }
}

/// 瀑布表。
#[derive(Clone, Debug)]
pub struct FallTable {
    rows: Vec<FallRow>,
}

impl FallTable {
    /// 新建。
    pub fn new() -> Self {
        FallTable { rows: Vec::new() }
    }

    /// 记一段。
    pub fn push(&mut self, r: FallRow) -> usize {
        self.rows.push(r);
        self.rows.len() - 1
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 超偏段数。
    pub fn over_count(&self) -> usize {
        self.rows.iter().filter(|r| r.over_20()).count()
    }

    /// 异常段数。
    pub fn anomaly_count(&self) -> usize {
        self.rows.iter().filter(|r| r.anomaly).count()
    }

    /// 十段齐。
    pub fn complete(&self) -> bool {
        self.rows.len() >= 10
    }
}

impl Default for FallTable {
    fn default() -> Self {
        FallTable::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 对账 + ADR + 功耗矩阵
// ─────────────────────────────────────────────────────────────

/// 对账记录。
#[derive(Clone, Copy, Debug)]
pub struct ReconcileRec {
    /// 对账基线 id。
    pub baseline: u16,
    /// 最大偏差基点。
    pub max_dev_bp: u32,
    /// ADR 已立。
    pub adr: bool,
}

impl ReconcileRec {
    /// 新建。
    pub const fn new(baseline: u16) -> Self {
        ReconcileRec {
            baseline,
            max_dev_bp: 0,
            adr: false,
        }
    }

    /// 记偏差并判 ADR。
    pub fn record(&mut self, dev_bp: u32) {
        if dev_bp > self.max_dev_bp {
            self.max_dev_bp = dev_bp;
        }
        if dev_bp > 2_000 {
            self.adr = true;
        }
    }

    /// 需 ADR。
    pub fn needs_adr(&self) -> bool {
        self.max_dev_bp > 2_000
    }
}

/// 功耗矩阵（三档 × 五资产）。
#[derive(Clone, Debug)]
pub struct PowerMatrix {
    cells: [[u16; 5]; 3],
}

impl PowerMatrix {
    /// 新建。
    pub const fn new() -> Self {
        PowerMatrix {
            cells: [[0; 5]; 3],
        }
    }

    /// 落值（档 0..3）。
    pub fn set(&mut self, tier: usize, asset: Asset, mw: u16) -> bool {
        if tier < 3 {
            self.cells[tier][asset.ordinal()] = mw;
            true
        } else {
            false
        }
    }

    /// 取值。
    pub fn get(&self, tier: usize, asset: Asset) -> u16 {
        if tier < 3 {
            self.cells[tier][asset.ordinal()]
        } else {
            0
        }
    }

    /// 非零格数。
    pub fn filled(&self) -> usize {
        let mut n = 0;
        for row in self.cells.iter() {
            for v in row.iter() {
                if *v > 0 {
                    n += 1;
                }
            }
        }
        n
    }

    /// 越档拒验证。
    pub fn set_bad(&mut self) -> bool {
        !self.set(3, Asset::Bench, 1)
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3784 判据集。
pub fn run_vr10d_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3784");

    // 0xAF05 五资产
    {
        let mut b = MasterBook::new();
        set.add("0xAF05-初始缺五", b.missing().len() == 5, "");
        for a in Asset::ALL.iter() {
            b.intake(*a);
        }
        set.add("0xAF05-五件齐", b.complete(), "");
        set.add("0xAF05-重收拒", !b.intake(Asset::Bench), "");
        set.add("0xAF03-心流在册", b.flow_ok(), "");
    }

    // 0xAF03 心流缺红线
    {
        let mut b = MasterBook::new();
        b.intake(Asset::Bench);
        b.intake(Asset::Split);
        b.intake(Asset::Watermark);
        b.intake(Asset::Power);
        set.add("0xAF03-心流缺检出", !b.flow_ok(), "");
        set.add("0xAF03-缺一心流", b.missing().len() == 1, "");
    }

    // 0xAF06 瀑布
    {
        let mut f = FallTable::new();
        set.add("0xAF06-初始空", f.is_empty(), "");
        for i in 0..10u8 {
            f.push(FallRow::new(i, 1000, 1000));
        }
        set.add("0xAF06-十段齐", f.complete() && f.len() == 10, "");
    }

    // 0xAF01 20% ADR
    {
        let r = FallRow::new(0, 1000, 1300);
        set.add("0xAF01-30%偏差", r.dev_bp() == 3000 && r.over_20(), "");
        let r2 = FallRow::new(1, 1000, 1100);
        set.add("0xAF01-10%不超", !r2.over_20(), "");
        let mut rec = ReconcileRec::new(1);
        set.add("0xAF01-初始无需", !rec.needs_adr(), "");
        rec.record(2500);
        set.add("0xAF01-超偏立ADR", rec.needs_adr() && rec.adr, "");
    }

    // 0xAF02 异常移交
    {
        let mut f = FallTable::new();
        let mut r = FallRow::new(0, 100, 100);
        r.anomaly = true;
        f.push(r);
        set.add("0xAF02-异常计数", f.anomaly_count() == 1, "");
        set.add("0xAF02-超偏零", f.over_count() == 0, "");
    }

    // 功耗矩阵
    {
        let mut m = PowerMatrix::new();
        set.add("0xAF04-初始零填", m.filled() == 0, "");
        m.set(0, Asset::Bench, 50);
        m.set(1, Asset::Flow, 30);
        set.add("0xAF04-两格填", m.filled() == 2, "");
        set.add("0xAF04-取值", m.get(0, Asset::Bench) == 50, "");
        set.add("0xAF04-越档拒", m.set_bad(), "");
    }

    // 资产序自洽
    {
        let mut ok = true;
        for (i, a) in Asset::ALL.iter().enumerate() {
            if a.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAF05-资产序自洽", ok, "");
    }

    set
}
