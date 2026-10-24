//! VE-F0586 · 着色器性能热分析
//!
//! 编辑态静态性能指标估算——指令数估算（按后端指令族计价表）、寄存器压力预估
//! （复用 F0487 模型在编辑态的轻量版）、采样与内存访问计数；编辑期呈现：热点
//! 构造高亮与指标面板；诚实边界声明：静态估算非实测，实机测量登记随闸门补测
//! （与 F0577 同纪律）；估算口径与 F0491 优化基准、F0499 性能总账一致（同一
//! 计价表），避免两套口径漂移。
//!
//! 跨批对接：上游 F0487 压力模型、F0491 计价基准；下游 F0589 遥测、F0597 三账。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 指令族（计价表键）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstFamily {
    Alu,
    Texture,
    Memory,
    ControlFlow,
}

impl InstFamily {
    /// 计价表（与 F0491/F0499 同一真源）：每条指令的抽象周期
    pub fn price(self) -> u32 {
        match self {
            InstFamily::Alu => 1,
            InstFamily::Texture => 8,
            InstFamily::Memory => 4,
            InstFamily::ControlFlow => 2,
        }
    }
}

/// 静态估算器
#[derive(Clone, Debug, Default)]
pub struct Estimator {
    /// 各指令族计数
    pub counts: Vec<(InstFamily, u32)>,
}

impl Estimator {
    pub fn count(&mut self, f: InstFamily, n: u32) {
        match self.counts.iter_mut().find(|(k, _)| *k == f) {
            Some(e) => e.1 += n,
            None => self.counts.push((f, n)),
        }
    }

    /// 指令总数
    pub fn total_insts(&self) -> u32 {
        self.counts.iter().map(|(_, n)| n).sum()
    }

    /// 周期估算（同一计价表，不另设口径）
    pub fn est_cycles(&self) -> u32 {
        self.counts.iter().map(|(f, n)| f.price() * n).sum()
    }

    /// 计价表缺失 → 条目级 N/A 显性
    pub fn priced_all(&self) -> bool {
        self.counts.iter().all(|(f, _)| f.price() > 0)
    }
}

/// 轻量寄存器压力模型（F0487 编辑态版）
#[derive(Clone, Debug)]
pub struct LightPressure {
    /// 活跃值峰值
    pub live_peak: u32,
    /// 寄存器预算
    pub budget: u32,
}

impl LightPressure {
    pub fn new(budget: u32) -> Self {
        LightPressure { live_peak: 0, budget }
    }

    pub fn observe(&mut self, live: u32) {
        if live > self.live_peak {
            self.live_peak = live;
        }
    }

    pub fn over(&self) -> bool {
        self.live_peak > self.budget
    }

    /// 压力比（千分位）
    pub fn ratio(&self) -> u32 {
        if self.budget == 0 {
            return 0;
        }
        self.live_peak * 1000 / self.budget
    }
}

/// 采样与内存访问计数
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemStat {
    pub samples: u32,
    pub loads: u32,
    pub stores: u32,
}

impl MemStat {
    pub fn total_access(&self) -> u32 {
        self.samples + self.loads + self.stores
    }
}

/// 热点构造高亮
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hotspot {
    pub source_line: u32,
    /// 该构造周期占比（千分位）
    pub share: u32,
}

impl Hotspot {
    pub fn highlightable(&self) -> bool {
        self.share >= 200
    }
}

/// 热点识别：按行号聚合周期估算
pub fn find_hotspots(lines: &[(u32, u32)], threshold: u32) -> Vec<Hotspot> {
    // lines: (行号, 周期估算)
    let total: u64 = lines.iter().map(|(_, c)| *c as u64).sum();
    if total == 0 {
        return Vec::new();
    }
    let mut hs: Vec<Hotspot> = Vec::new();
    for (line, cyc) in lines {
        let share = (*cyc as u64 * 1000 / total) as u32;
        if share >= threshold {
            hs.push(Hotspot { source_line: *line, share });
        }
    }
    hs
}

/// 指标面板（编辑期呈现）
#[derive(Clone, Debug, Default)]
pub struct Panel {
    pub cycles: u32,
    pub insts: u32,
    pub pressure: u32,
    pub mem: MemStat,
}

impl Panel {
    pub fn renderable(&self) -> bool {
        self.insts > 0
    }
}

/// 口径对账：本估算计价表与 F0491 基准一致
pub fn price_table_consistent(local: &[(InstFamily, u32)], baseline: &[(InstFamily, u32)]) -> bool {
    local.iter().all(|(f, p)| baseline.iter().find(|(bf, _)| bf == f).map(|(_, bp)| bp == p).unwrap_or(false))
}

/// 诚实边界：静态估算非实测，实机登记随闸门
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HonestBoundary {
    pub is_estimate: bool,
    pub device_gate_pending: bool,
}

impl HonestBoundary {
    pub fn declared(&self) -> bool {
        self.is_estimate && self.device_gate_pending
    }
}

pub fn run_vec186_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec186");
    // 族一：计价表（同源）
    s.add("计价-ALU", InstFamily::Alu.price() == 1, "alu");
    s.add("计价-纹理", InstFamily::Texture.price() == 8, "tex");
    s.add("计价-内存", InstFamily::Memory.price() == 4, "mem");
    s.add("计价-控制流", InstFamily::ControlFlow.price() == 2, "cf");
    // 族二：静态估算
    let mut e = Estimator::default();
    e.count(InstFamily::Alu, 10);
    e.count(InstFamily::Texture, 3);
    e.count(InstFamily::Alu, 5);
    s.add("估算-计数聚合", e.total_insts() == 18, "n18");
    s.add("估算-周期同口径", e.est_cycles() == 15 * 1 + 3 * 8, "cyc");
    s.add("估算-计价全备", e.priced_all(), "priced");
    // 族三：轻量压力模型
    let mut p = LightPressure::new(64);
    p.observe(30);
    p.observe(50);
    s.add("压力-峰值取大", p.live_peak == 50 && !p.over(), "peak");
    s.add("压力-千分比", p.ratio() == 781, "ratio");
    p.observe(70);
    s.add("压力-超限检出", p.over() && p.ratio() == 1093, "over");
    // 族四：内存访问计数
    let m = MemStat { samples: 4, loads: 10, stores: 2 };
    s.add("内存-访问总数", m.total_access() == 16, "n16");
    // 族五：热点识别
    let lines = vec![(1u32, 60u32), (2u32, 30u32), (3u32, 10u32)];
    let hs = find_hotspots(&lines, 200);
    s.add("热点-行1命中", hs.iter().any(|h| h.source_line == 1 && h.share == 600), "h1");
    s.add("热点-行3不命中", !hs.iter().any(|h| h.source_line == 3), "h3");
    s.add("热点-可高亮判定", hs[0].highlightable(), "hl");
    s.add("热点-空输入空表", find_hotspots(&[], 100).is_empty(), "empty");
    // 族六：指标面板
    let panel = Panel { cycles: 39, insts: 18, pressure: 781, mem: m };
    s.add("面板-可呈现", panel.renderable(), "render");
    s.add("面板-空不可呈现", !Panel::default().renderable(), "blank");
    // 族七：口径一致
    let baseline = [(InstFamily::Alu, 1u32), (InstFamily::Texture, 8u32), (InstFamily::Memory, 4u32)];
    s.add("对账-一致", price_table_consistent(&[(InstFamily::Alu, 1), (InstFamily::Texture, 8), (InstFamily::Memory, 4)], &baseline), "same");
    s.add("对账-漂移检出", !price_table_consistent(&[(InstFamily::Alu, 2)], &baseline), "drift");
    // 族八：诚实边界
    let hb = HonestBoundary { is_estimate: true, device_gate_pending: true };
    s.add("边界-显性声明", hb.declared(), "honest");
    s
}
