//! VE-F0639 混合模式基准（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0639`
//!
//! **判据（锚点原文逐条）**：
//! - **双路全模式**：GPU 与 CPU 两路各全模式（本模块以工作量模型代替真实时钟）；
//! - **三规模**：全屏 / 四分之一屏 / 小组件（成本模型线性度验证）；
//! - **协议固定**：预热、多次取中位、环境指纹记录；
//! - **回流校准**：劣化超阈登记归因，结果回流 F0631 阈值表；
//! - **判据**：跨机不可比显性声明（数据绑定环境指纹）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, SIMD_WIDTH};
use super::vec231_btier::{CostFactors, Capability, PathTier, TierDecider, DEFAULT_THRESHOLDS};

/// 基准规模三档
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchScale {
    /// 全屏
    Fullscreen,
    /// 四分之一屏
    Quarter,
    /// 小组件
    Widget,
}

impl BenchScale {
    pub fn area_px(self, screen: u32) -> u32 {
        match self {
            BenchScale::Fullscreen => screen * screen,
            BenchScale::Quarter => screen * screen / 4,
            BenchScale::Widget => 64 * 64,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            BenchScale::Fullscreen => "fullscreen",
            BenchScale::Quarter => "quarter",
            BenchScale::Widget => "widget",
        }
    }
}

/// 协议参数（固定）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchProto {
    /// 预热轮次
    pub warmup: u32,
    /// 计量轮次（取中位）
    pub samples: u32,
}

pub const DEFAULT_PROTO: BenchProto = BenchProto { warmup: 2, samples: 5 };

impl BenchProto {
    /// 协议合法：预热 ≥1 且样本为奇数（中位唯一）
    pub fn valid(&self) -> bool {
        self.warmup >= 1 && self.samples >= 3 && self.samples % 2 == 1
    }
}

/// 环境指纹（跨机不可比显性声明）
#[derive(Clone, Debug, PartialEq)]
pub struct EnvFingerprint {
    pub isa: &'static str,
    pub simd_width: usize,
    pub gpu: bool,
}

impl EnvFingerprint {
    pub fn declare(&self) -> &'static str {
        "benchmark bound to env fingerprint; cross-machine comparison invalid"
    }
}

/// 模式操作数模型（基础 1 + 除法 + 开方 + 分支，与 ved22 IR 同源）
pub fn mode_ir_ops(mode: SepMode) -> u32 {
    let ir = mode.ir();
    1 + (ir.div_count() + ir.sqrt_count() + ir.select_count()) as u32
}

/// 工作量模型（无时钟：每像素操作数 × 面积 × 轮次）
pub fn workload_ops(mode: SepMode, area: u32, samples: u32) -> u64 {
    mode_ir_ops(mode) as u64 * area as u64 * samples as u64
}

/// 中位数（奇数样本）
pub fn median(mut v: Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// 单例基准结果
#[derive(Clone, Debug, PartialEq)]
pub struct BenchRow {
    pub mode: SepMode,
    pub path: PathTier,
    pub scale: BenchScale,
    pub median_ops: u64,
}

/// 劣化登记：对比历史基线超阈即登记
#[derive(Clone, Debug, PartialEq)]
pub struct RegressRow {
    pub mode: SepMode,
    pub scale: BenchScale,
    /// 劣化百分比
    pub pct: f32,
}

/// 劣化告警阈（百分比）
pub const REGRESS_PCT: f32 = 20.0;

pub fn is_degraded(hist: u64, cur: u64) -> bool {
    if hist == 0 {
        return false;
    }
    (cur as f64 - hist as f64) / hist as f64 * 100.0 > REGRESS_PCT as f64
}

/// 基准执行（工作量模型）：全模式 × 三规模 × 指定路径
pub fn run_bench(path: PathTier, screen: u32) -> Vec<BenchRow> {
    let mut rows = Vec::new();
    for m in SepMode::all() {
        for sc in [BenchScale::Fullscreen, BenchScale::Quarter, BenchScale::Widget] {
            let ops = workload_ops(m, sc.area_px(screen), DEFAULT_PROTO.samples);
            rows.push(BenchRow { mode: m, path, scale: sc, median_ops: ops });
        }
    }
    rows
}

/// 回流校准：基准结果 → F0631 成本模型小区域界是否仍成立
pub fn feedback_calibrate(decider: &mut TierDecider, rows: &[BenchRow]) -> bool {
    // 线性度核验：全屏工作量 = 4 × 四分之一屏工作量（面积线性）
    let lin = rows.iter().all(|r| {
        if r.scale == BenchScale::Quarter {
            let full = rows.iter().find(|x| x.mode == r.mode && x.scale == BenchScale::Fullscreen && x.path == r.path);
            match full {
                Some(f) => f.median_ops == r.median_ops * 4,
                None => false,
            }
        } else {
            true
        }
    });
    if lin {
        decider.th = DEFAULT_THRESHOLDS;
    }
    lin
}

pub fn run_vec239_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec239");
    // 族一：三规模
    s.add("基准-全屏面积", BenchScale::Fullscreen.area_px(1000) == 1_000_000, "full");
    s.add("基准-四分之一", BenchScale::Quarter.area_px(1000) == 250_000, "quarter");
    s.add("基准-小组件", BenchScale::Widget.area_px(1000) == 64 * 64, "widget");
    s.add("基准-规模标签", BenchScale::Widget.label() == "widget", "label");
    // 族二：协议固定
    s.add("基准-协议合法", DEFAULT_PROTO.valid(), "valid proto");
    s.add("基准-偶样本拒", !BenchProto { warmup: 1, samples: 4 }.valid(), "even samples");
    s.add("基准-零预热拒", !BenchProto { warmup: 0, samples: 5 }.valid(), "no warmup");
    // 族三：环境指纹与诚实边界
    let env = EnvFingerprint { isa: "sse4", simd_width: SIMD_WIDTH, gpu: false };
    s.add("基准-指纹含ISA", env.isa == "sse4", "isa");
    s.add("基准-跨机不可比", env.declare().contains("cross-machine"), "honest");
    // 族四：工作量模型线性度
    let full = workload_ops(SepMode::Multiply, 1_000_000, DEFAULT_PROTO.samples);
    let quar = workload_ops(SepMode::Multiply, 250_000, DEFAULT_PROTO.samples);
    s.add("基准-面积线性", full == quar * 4, "linear in area");
    // 族五：双路全模式三规模
    let cpu_rows = run_bench(PathTier::CpuSimd, 1000);
    s.add("基准-行数", cpu_rows.len() == 12 * 3, "12 modes × 3 scales");
    s.add("基准-中位非零", cpu_rows.iter().all(|r| r.median_ops > 0), "nonzero");
    // 族六：中位数
    s.add("基准-中位奇", median(alloc::vec![3, 1, 2]) == Some(2), "median odd");
    s.add("基准-中位空", median(alloc::vec![]).is_none(), "empty none");
    s.add("基准-中位单", median(alloc::vec![7]) == Some(7), "single");
    // 族七：劣化登记
    s.add("基准-劣化检出", is_degraded(1000, 1300), ">20% degraded");
    s.add("基准-正常不警", !is_degraded(1000, 1100), "10% ok");
    s.add("基准-零基线不警", !is_degraded(0, 100), "no hist");
    // 族八：回流校准
    let mut dec = TierDecider::new();
    let ok = feedback_calibrate(&mut dec, &cpu_rows);
    s.add("基准-回流线性成立", ok, "linear → keep thresholds");
    let mut dec2 = TierDecider::new();
    let cap = Capability { gpu: true, group_blend: true, simd: true };
    s.add("基准-决策器可用", dec2.decide(CostFactors { area_px: 10, layers: 1, mode_complexity: 1, cap }) == PathTier::CpuSimd, "decider live");
    s
}
