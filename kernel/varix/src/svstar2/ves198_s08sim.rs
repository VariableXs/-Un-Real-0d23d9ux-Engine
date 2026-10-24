#![allow(dead_code)]
//! VE-F3944 · 读屏仿真器
//!
//! 读屏输出仿真引擎：语义树 → 模拟播报序列（复用 F3851 模式全域化——
//! 单源复用声明）。仿真精度：仿真与真机读屏对拍（对拍校准复述）。仿真
//! 覆盖：三读屏器仿真档（矩阵联动）。前置红线全域化：仿真通过不豁免
//! 真机。
//! 错误路径：仿真失真→校准立案；仿真替代真机→P0；器覆盖缺→补；语义
//! 错播→修正。
//! 性能：仿真 O(序列)；对拍 O(抽样)；覆盖 O(三器)；前置 O(制度)。
//! 跨批对接：F3851 复用单源声明；F3858 真机矩阵对端；N08 语义对端。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0B80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0B80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8DCode(pub u16);

impl S8DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const SIM_DISTORT: Self = Self(SEG_BASE + 1);
    pub const SIM_SUBSTITUTES: Self = Self(SEG_BASE + 2);
    pub const READER_GAP: Self = Self(SEG_BASE + 3);
    pub const SEMANTIC_MISSPEAK: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SIM_DISTORT => "仿真失真→校准立案",
            Self::SIM_SUBSTITUTES => "仿真替代真机→P0",
            Self::READER_GAP => "器覆盖缺→补",
            Self::SEMANTIC_MISSPEAK => "语义错播→修正",
            _ => "未知",
        }
    }
}

// ── 语义树与播报序列 ─────────────────────────────────────────────────

/// 语义树节点（N08 语义对端最小面）。
#[derive(Debug, Clone)]
pub struct SemNode {
    /// 语义角色（按钮/标题/列表项…）。
    pub role: &'static str,
    /// 朗读文本。
    pub text: &'static str,
    /// 子节点。
    pub children: Vec<SemNode>,
}

impl SemNode {
    pub fn leaf(role: &'static str, text: &'static str) -> Self {
        Self { role, text, children: Vec::new() }
    }

    pub fn branch(role: &'static str, children: Vec<SemNode>) -> Self {
        Self { role, text: "", children }
    }

    /// 深度优先播报序（F3851 单源模式）。
    pub fn announce_order(&self, out: &mut Vec<&'static str>) {
        if !self.text.is_empty() {
            out.push(self.text);
        }
        for c in self.children.iter() {
            c.announce_order(out);
        }
    }

    pub fn count(&self) -> usize {
        1 + self.children.iter().map(|c| c.count()).sum::<usize>()
    }
}

/// 单源复用声明：仿真引擎播报序 = F3851 模式（深度优先）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimSource {
    /// F3851 读屏模拟器（单源）。
    F3851,
}

/// 三读屏器仿真档（矩阵联动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderKind {
    /// 读屏器一。
    R1,
    /// 读屏器二。
    R2,
    /// 读屏器三。
    R3,
}

pub const READERS: [ReaderKind; 3] = [ReaderKind::R1, ReaderKind::R2, ReaderKind::R3];

// ── 仿真引擎 ─────────────────────────────────────────────────────────

/// 仿真输出：某读屏器对语义树的模拟播报序列。
#[derive(Debug, Clone)]
pub struct SimRun {
    pub reader: ReaderKind,
    pub spoken: Vec<&'static str>,
}

/// 读屏仿真器：语义树 → 模拟播报。
#[derive(Debug, Clone)]
pub struct Simulator {
    pub source: SimSource,
}

impl Simulator {
    pub fn new() -> Self {
        Self { source: SimSource::F3851 }
    }

    /// 仿真 O(序列)：深度优先播报（单源复用）。
    pub fn simulate(&self, tree: &SemNode, reader: ReaderKind) -> SimRun {
        let mut spoken = Vec::new();
        tree.announce_order(&mut spoken);
        SimRun { reader, spoken }
    }
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}

// ── 对拍校准（仿真 vs 真机）──────────────────────────────────────────

/// 真机实测样本（开发后回填）。
#[derive(Debug, Clone)]
pub struct DeviceSample {
    pub reader: ReaderKind,
    pub spoken: Vec<&'static str>,
}

/// 对拍结果：逐位比对，首分歧位定位。
#[derive(Debug, Clone)]
pub struct CalibReport {
    pub reader: ReaderKind,
    /// 失真度 = 分歧位占比（万分比）。
    pub dist_ppm: u32,
    /// 首分歧位（无分歧 = None）。
    pub first_diverge: Option<usize>,
}

/// 对拍校准器：仿真序列 vs 真机样本。
pub struct Calibrator;

impl Calibrator {
    /// 对拍 O(抽样)：逐位比对 + 失真度计算。
    pub fn compare(sim: &SimRun, dev: &DeviceSample) -> CalibReport {
        let n = sim.spoken.len().max(dev.spoken.len());
        let mut diff = 0usize;
        let mut first = None;
        for i in 0..n {
            let a = sim.spoken.get(i);
            let b = dev.spoken.get(i);
            if a != b {
                diff += 1;
                if first.is_none() {
                    first = Some(i);
                }
            }
        }
        let ppm = if n == 0 { 0 } else { (diff * 10_000 / n) as u32 };
        CalibReport { reader: sim.reader, dist_ppm: ppm, first_diverge: first }
    }

    /// 校准立案阈值：失真 > 0 即立案（仿真必须与真机一致）。
    pub fn needs_case(r: &CalibReport) -> Option<S8DCode> {
        if r.dist_ppm > 0 {
            Some(S8DCode::SIM_DISTORT)
        } else {
            None
        }
    }
}

// ── 前置制度（仿真通过不豁免真机）────────────────────────────────────

/// 真机核销册：仿真通过的项仍需真机核销。
#[derive(Debug, Clone)]
pub struct DeviceGate {
    /// 项名 → 仿真是否通过。
    pub sim_pass: Vec<&'static str>,
    /// 项名 → 真机是否核销。
    pub device_confirmed: Vec<&'static str>,
}

impl DeviceGate {
    pub fn new() -> Self {
        Self { sim_pass: Vec::new(), device_confirmed: Vec::new() }
    }

    pub fn sim_accept(&mut self, name: &'static str) {
        self.sim_pass.push(name);
    }

    /// 前置红线实测：仿真通过但真机未核销 → P0 名单。
    pub fn pending_p0(&self) -> Vec<&'static str> {
        self.sim_pass
            .iter()
            .filter(|n| !self.device_confirmed.iter().any(|d| d == *n))
            .map(|n| *n)
            .collect()
    }

    /// 真机核销。
    pub fn device_confirm(&mut self, name: &'static str) -> S8DCode {
        if self.sim_pass.iter().any(|n| *n == name) {
            self.device_confirmed.push(name);
            S8DCode::OK
        } else {
            S8DCode::SIM_SUBSTITUTES
        }
    }

    pub fn released(&self) -> bool {
        self.pending_p0().is_empty()
    }
}

impl Default for DeviceGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── 器覆盖（三器矩阵联动）────────────────────────────────────────────

/// 三器覆盖账。
#[derive(Debug, Clone)]
pub struct ReaderCover {
    pub covered: [bool; 3],
}

impl ReaderCover {
    pub fn new() -> Self {
        Self { covered: [false; 3] }
    }

    pub fn mark(&mut self, r: ReaderKind) {
        self.covered[r as usize] = true;
    }

    /// 器覆盖缺→补：返回未覆盖器清单。
    pub fn gaps(&self) -> Vec<ReaderKind> {
        READERS.iter().filter(|r| !self.covered[**r as usize]).map(|r| *r).collect()
    }

    /// 补齐动作。
    pub fn fill(&mut self, r: ReaderKind) {
        self.mark(r);
    }

    pub fn full(&self) -> bool {
        self.covered.iter().all(|c| *c)
    }
}

impl Default for ReaderCover {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves198_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves198_s08sim");

    fn demo_tree() -> SemNode {
        SemNode::branch("dialog", vec![
            SemNode::leaf("title", "设置"),
            SemNode::branch("list", vec![
                SemNode::leaf("item", "对比度"),
                SemNode::leaf("item", "焦点序"),
            ]),
            SemNode::leaf("button", "确定"),
        ])
    }

    // 01 仿真引擎单源复用声明
    {
        let s = Simulator::new();
        chk(&mut set, "C3944-01", s.source == SimSource::F3851, "err");
    }
    // 02 深度优先播报序正确
    {
        let sim = Simulator::new();
        let run = sim.simulate(&demo_tree(), ReaderKind::R1);
        chk(&mut set, "C3944-02", run.spoken == vec!["设置", "对比度", "焦点序", "确定"], "err");
    }
    // 03 三器仿真覆盖联动
    {
        let sim = Simulator::new();
        let mut cover = ReaderCover::new();
        for r in READERS.iter() {
            let run = sim.simulate(&demo_tree(), *r);
            if run.spoken.len() == 4 {
                cover.mark(*r);
            }
        }
        chk(&mut set, "C3944-03", cover.full() && cover.gaps().is_empty(), "err");
    }
    // 04 器覆盖缺→补实测
    {
        let mut cover = ReaderCover::new();
        cover.mark(ReaderKind::R1);
        chk(&mut set, "C3944-04", cover.gaps().len() == 2 && { cover.fill(ReaderKind::R2); cover.fill(ReaderKind::R3); cover.full() }, "err");
    }
    // 05 对拍一致零失真
    {
        let sim = Simulator::new();
        let run = sim.simulate(&demo_tree(), ReaderKind::R2);
        let dev = DeviceSample { reader: ReaderKind::R2, spoken: vec!["设置", "对比度", "焦点序", "确定"] };
        let rep = Calibrator::compare(&run, &dev);
        chk(&mut set, "C3944-05", rep.dist_ppm == 0 && rep.first_diverge.is_none() && Calibrator::needs_case(&rep).is_none(), "err");
    }
    // 06 仿真失真→首分歧位定位
    {
        let sim = Simulator::new();
        let run = sim.simulate(&demo_tree(), ReaderKind::R3);
        let dev = DeviceSample { reader: ReaderKind::R3, spoken: vec!["设置", "对比度", "错误项", "确定"] };
        let rep = Calibrator::compare(&run, &dev);
        chk(&mut set, "C3944-06", rep.first_diverge == Some(2) && rep.dist_ppm == 2500 && Calibrator::needs_case(&rep) == Some(S8DCode::SIM_DISTORT), "err");
    }
    // 07 长度失配也计失真
    {
        let sim = Simulator::new();
        let run = sim.simulate(&demo_tree(), ReaderKind::R1);
        let dev = DeviceSample { reader: ReaderKind::R1, spoken: vec!["设置", "对比度"] };
        let rep = Calibrator::compare(&run, &dev);
        chk(&mut set, "C3944-07", rep.first_diverge == Some(2) && rep.dist_ppm == 5000, "err");
    }
    // 08 前置红线：仿真通过未核销→P0 名单
    {
        let mut g = DeviceGate::new();
        g.sim_accept("焦点序");
        g.sim_accept("对比度");
        g.device_confirm("对比度");
        chk(&mut set, "C3944-08", g.pending_p0() == vec!["焦点序"] && !g.released(), "err");
    }
    // 09 真机核销闭环
    {
        let mut g = DeviceGate::new();
        g.sim_accept("焦点序");
        chk(&mut set, "C3944-09", g.device_confirm("焦点序") == S8DCode::OK && g.released(), "err");
    }
    // 10 未仿真项拒绝核销（防仿真替代真机反向漏洞）
    {
        let mut g = DeviceGate::new();
        chk(&mut set, "C3944-10", g.device_confirm("幽灵项") == S8DCode::SIM_SUBSTITUTES, "err");
    }
    // 11 语义树节点计数守恒
    {
        let t = demo_tree();
        chk(&mut set, "C3944-11", t.count() == 6, "err");
    }
    // 12 语义错播→修正（失真清零后立案消除）
    {
        let sim = Simulator::new();
        let run = sim.simulate(&demo_tree(), ReaderKind::R2);
        let mut dev = DeviceSample { reader: ReaderKind::R2, spoken: vec!["设置", "对比度", "错播", "确定"] };
        let r1 = Calibrator::compare(&run, &dev);
        dev.spoken = vec!["设置", "对比度", "焦点序", "确定"];
        let r2 = Calibrator::compare(&run, &dev);
        chk(&mut set, "C3944-12", Calibrator::needs_case(&r1).is_some() && Calibrator::needs_case(&r2).is_none(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3944-13", S8DCode::SEMANTIC_MISSPEAK.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8DCode::OK, S8DCode::SIM_DISTORT, S8DCode::SIM_SUBSTITUTES, S8DCode::READER_GAP, S8DCode::SEMANTIC_MISSPEAK];
        chk(&mut set, "C3944-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_sim_all_green() {
        let s = run_ves198_checks();
        assert!(s.all_passed(), "ves198 red: {}", s.red_items_text());
    }

    #[test]
    fn test_deep_first_order() {
        let tree = SemNode::branch("root", vec![
            SemNode::branch("grp", vec![SemNode::leaf("a", "甲"), SemNode::leaf("b", "乙")]),
            SemNode::leaf("c", "丙"),
        ]);
        let mut out = Vec::new();
        tree.announce_order(&mut out);
        assert_eq!(out, vec!["甲", "乙", "丙"]);
    }

    #[test]
    fn test_gate_full_cycle() {
        let mut g = DeviceGate::new();
        g.sim_accept("x");
        assert!(!g.released());
        g.device_confirm("x");
        assert!(g.released());
    }
}
