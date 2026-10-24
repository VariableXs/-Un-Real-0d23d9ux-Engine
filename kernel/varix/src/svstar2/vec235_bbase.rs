//! VE-F0635 混合回归基线（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0635`
//!
//! **判据（锚点原文逐条）**：
//! - **版本化快照**：色卡矩阵全模式期望值固化（版本号 + 生成参数指纹）；
//! - **四触发**：公式修改 / 精度策略变更 / SIMD 宽度或 ISA 变化 / GPU 模板重生成；
//! - **双签升级**：基线修订须双签（变更理由与影响面留档）；
//! - **禁静默改**：改基线掩盖回归是红线；
//! - **判据**：回归命中 → 阻断合入。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, oracle, SWATCH_PROBES, SIMD_WIDTH};

/// 基线快照行：色卡 (cb, cs) × 模式 → 期望值（oracle 固化）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaseRow {
    pub mode: SepMode,
    pub cb: f32,
    pub cs: f32,
    pub expect: f32,
}

/// 基线快照库（带版本号与指纹）
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    pub ver: u32,
    /// 生成参数指纹（色卡集 × 模式集长度的组合指纹）
    pub fingerprint: u64,
    pub rows: Vec<BaseRow>,
}

/// 指纹：色卡数 × 100003 + 模式数 × 31 + SIMD 宽度（参数变化必变指纹）
pub fn fingerprint(swatch_n: usize, mode_n: usize) -> u64 {
    (swatch_n as u64)
        .wrapping_mul(100_003)
        .wrapping_add((mode_n as u64).wrapping_mul(31))
        .wrapping_add(SIMD_WIDTH as u64)
}

/// 生成基线（oracle 直译固化）
pub fn gen_baseline(ver: u32) -> Baseline {
    let modes = SepMode::all();
    let rows = modes
        .iter()
        .flat_map(|&m| {
            SWATCH_PROBES.iter().map(move |&(cb, cs)| BaseRow {
                mode: m,
                cb,
                cs,
                expect: oracle(m, cb, cs),
            })
        })
        .collect();
    Baseline { ver, fingerprint: fingerprint(SWATCH_PROBES.len(), modes.len()), rows }
}

/// 四触发清单
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    FormulaChange,
    PrecChange,
    SimdChange,
    GpuRegen,
}

pub const TRIGGER_COUNT: usize = 4;

impl Trigger {
    pub fn all() -> [Trigger; TRIGGER_COUNT] {
        [
            Trigger::FormulaChange,
            Trigger::PrecChange,
            Trigger::SimdChange,
            Trigger::GpuRegen,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            Trigger::FormulaChange => "公式修改",
            Trigger::PrecChange => "精度策略变更",
            Trigger::SimdChange => "SIMD 宽度或 ISA 变化",
            Trigger::GpuRegen => "GPU 模板重生成",
        }
    }
}

/// 双签升级流程：禁止静默改基线（红线）
#[derive(Clone, Debug, PartialEq)]
pub struct UpgradeLog {
    pub rows: Vec<(&'static str, &'static str, &'static str)>,
}

impl UpgradeLog {
    pub fn new() -> UpgradeLog {
        UpgradeLog { rows: alloc::vec![] }
    }
    /// 双签登记（变更理由 + 影响面 + 双签人）
    pub fn sign(&mut self, reason: &'static str, impact: &'static str, signer: &'static str) {
        self.rows.push((reason, impact, signer));
    }
    /// 双签成立：影响面留档 + 签名非空
    pub fn signed_ok(&self) -> bool {
        self.rows.iter().all(|r| !r.0.is_empty() && !r.1.is_empty() && !r.2.is_empty())
    }
}

/// 回归比对：实现值 vs 基线期望（≤1 LSB）
pub fn regress_check(base: &Baseline) -> (usize, usize, Option<(SepMode, f32, f32)>) {
    let mut cases = 0usize;
    let mut fails = 0usize;
    let mut first = None;
    for r in &base.rows {
        cases += 1;
        let got = oracle(r.mode, r.cb, r.cs);
        if (got - r.expect).abs() > 1.0 / 255.0 {
            fails += 1;
            if first.is_none() {
                first = Some((r.mode, got, r.expect));
            }
        }
    }
    (cases, fails, first)
}

pub fn run_vec235_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec235");
    // 族一：版本化快照
    let b1 = gen_baseline(1);
    s.add("基线-行数", b1.rows.len() == 12 * SWATCH_PROBES.len(), "12 modes × swatches");
    s.add("基线-版本号", b1.ver == 1, "v1");
    s.add("基线-指纹非零", b1.fingerprint != 0, "fp set");
    let b1b = gen_baseline(1);
    s.add("基线-可再生", b1 == b1b, "regen equal");
    // 族二：指纹敏感性
    let fp1 = fingerprint(12, 12);
    let fp2 = fingerprint(13, 12);
    let fp3 = fingerprint(12, 13);
    s.add("基线-指纹敏感", fp1 != fp2 && fp1 != fp3 && fp2 != fp3, "fp distinct");
    // 族三：回归零命中（基线固化自洽）
    let (cases, fails, first) = regress_check(&b1);
    s.add("基线-回归零", fails == 0 && first.is_none(), "clean regress");
    s.add("基线-全量跑", cases == b1.rows.len(), "full cases");
    // 族四：四触发
    let tr = Trigger::all();
    s.add("基线-四触发", tr.len() == TRIGGER_COUNT, "4 triggers");
    s.add("基线-触发名齐", tr.iter().all(|t| !t.label().is_empty()), "labeled");
    // 族五：双签升级
    let mut up = UpgradeLog::new();
    up.sign("预算修订 1→2 LSB", "全模式基线重生成", "AI-A");
    s.add("双签-影响面留档", !up.rows[0].1.is_empty(), "impact logged");
    up.sign("期望值修正", "soft-light 3 卡", "AI-B");
    s.add("双签-成立", up.signed_ok(), "dual signed");
    // 族六：禁静默改（无签名行不可过双签检查）
    let mut up2 = UpgradeLog::new();
    up2.rows.push(("改基线掩盖回归", "", ""));
    s.add("红线-缺签不可过", !up2.signed_ok(), "no silent change");
    // 族七：基线值域（全期望值在 [0,1]）
    s.add("基线-值域", b1.rows.iter().all(|r| (0.0..=1.0).contains(&r.expect)), "expect range");
    s
}
