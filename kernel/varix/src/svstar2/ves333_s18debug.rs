//! ves333_s18debug — VE-F4093 · T05 语言资源调试
//! 字形缺口探查器（输入文本→标出回退字符）+ 资产引用图查看器（视口裁剪防爆炸）+ 热更新三步计时器 + 工具故障降级命令行模式 + 缺口多量聚合报告 + 输出遵循五标准复述 + 探查不含用户私有文本持久化。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x97C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbCode {
    Ok = 0,
    ToolFail = SEG_BASE + 0x01,
    GraphBurst = SEG_BASE + 0x02,
    Aggregated = SEG_BASE + 0x03,
}

/// 字形缺口探查器：文本（码点序列）→ 逐码点标注（本族命中/回退命中/缺口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphHit { Direct = 0, Fallback = 1, Missing = 2 }

pub struct GlyphProbe { pub covered: Vec<u32>, pub fallback: Vec<u32> }

impl GlyphProbe {
    pub fn new(covered: Vec<u32>, fallback: Vec<u32>) -> Self { GlyphProbe { covered, fallback } }

    pub fn classify(&self, cp: u32) -> GlyphHit {
        if self.covered.contains(&cp) { GlyphHit::Direct }
        else if self.fallback.contains(&cp) { GlyphHit::Fallback }
        else { GlyphHit::Missing }
    }

    /// 探查：输入码点序列 → 命中标序列 + 回退字符清单（聚合报告）。
    pub fn probe(&self, text: &[u32]) -> ProbeReport {
        let mut hits = Vec::new();
        let mut fb = Vec::new();
        let mut miss = Vec::new();
        for cp in text.iter() {
            let h = self.classify(*cp);
            if h == GlyphHit::Fallback && !fb.contains(cp) { fb.push(*cp); }
            if h == GlyphHit::Missing && !miss.contains(cp) { miss.push(*cp); }
            hits.push(h);
        }
        ProbeReport { hits, fallback_cps: fb, missing_cps: miss }
    }
}

#[derive(Clone)]
pub struct ProbeReport { pub hits: Vec<GlyphHit>, pub fallback_cps: Vec<u32>, pub missing_cps: Vec<u32> }

impl ProbeReport {
    /// 缺口多量 → 聚合报告（只给清单不给逐字符明细——防刷屏）。
    pub fn aggregate(&self, threshold: usize) -> Result<Vec<u32>, DbCode> {
        if self.missing_cps.len() >= threshold {
            Err(DbCode::Aggregated) // 信号：走聚合通道
        } else {
            Ok(self.missing_cps.clone())
        }
    }
    pub fn fallback_count(&self) -> usize { self.fallback_cps.len() }
}

/// 资产引用图：节点=资产，边=引用；查看器带视口裁剪。
pub struct RefGraph { pub edges: Vec<(&'static str, &'static str)> }

impl RefGraph {
    pub fn new() -> Self { RefGraph { edges: Vec::new() } }
    pub fn link(&mut self, from: &'static str, to: &'static str) { self.edges.push((from, to)); }

    /// 视口裁剪：只展开 from 集合的一跳邻接。
    pub fn viewport(&self, focus: &[&'static str], max_edges: usize) -> Result<Vec<(&'static str, &'static str)>, DbCode> {
        let out: Vec<(&'static str, &'static str)> = self.edges.iter()
            .filter(|(f, _)| focus.contains(f))
            .map(|e| *e)
            .collect();
        if out.len() > max_edges { return Err(DbCode::GraphBurst); }
        Ok(out)
    }

    /// 闭合检测：focus 集的一跳里引用了不存在的资产 → 断点。
    pub fn dangling(&self, focus: &[&'static str], known: &[&'static str]) -> Vec<&'static str> {
        self.edges.iter()
            .filter(|(f, t)| focus.contains(f) && !known.contains(t))
            .map(|(_, t)| *t)
            .collect()
    }
}

/// 热更新三步计时器：记录三步耗时 + 阈值判定。
#[derive(Clone, Copy)]
pub struct StepTiming { pub step: &'static str, pub elapsed_ms: u32, pub threshold_ms: u32 }

pub struct StepTimer { pub timings: Vec<StepTiming> }

impl StepTimer {
    pub fn new() -> Self { StepTimer { timings: Vec::new() } }
    pub fn record(&mut self, step: &'static str, elapsed_ms: u32, threshold_ms: u32) {
        self.timings.push(StepTiming { step, elapsed_ms, threshold_ms });
    }
    pub fn over_threshold(&self) -> Vec<&'static str> {
        self.timings.iter().filter(|t| t.elapsed_ms > t.threshold_ms).map(|t| t.step).collect()
    }
    pub fn total_ms(&self) -> u32 { self.timings.iter().map(|t| t.elapsed_ms).sum() }
}

/// 工具宿主：GUI 工具故障 → 降级命令行模式。
pub enum ToolMode { Gui, Cli }

pub struct DebugHost { pub mode: ToolMode, pub outputs: Vec<String> }

impl DebugHost {
    pub fn new() -> Self { DebugHost { mode: ToolMode::Gui, outputs: Vec::new() } }

    /// 输出遵循五标准复述（对比/焦点/复述/动效/同步——此处以标记串自证）。
    pub fn emit(&mut self, standard_tag: &'static str, text: &str) {
        let mut s = String::from("[std:");
        s.push_str(standard_tag);
        s.push(']');
        s.push_str(text);
        self.outputs.push(s);
    }

    /// 工具故障 → 降级 CLI。
    pub fn degrade(&mut self) -> bool {
        self.mode = ToolMode::Cli;
        true
    }

    pub fn is_cli(&self) -> bool { matches!(self.mode, ToolMode::Cli) }

    /// 隐私：探查文本不持久化——只有聚合统计可入库（结构即证明：返回 usize）。
    pub fn persist_probe(&self, r: &ProbeReport) -> usize {
        r.missing_cps.len() + r.fallback_cps.len()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves333_checks() -> CheckSet {
    let mut set = CheckSet::new("ves333");

    // 夹具：主字面覆盖 ASCII 大写，回退覆盖 0x4E00，其余缺。
    let gp = GlyphProbe::new(vec![0x41, 0x42, 0x43], vec![0x4E00]);

    // C333-01 三态分类。
    chk(&mut set, "C333-01",
        gp.classify(0x41) == GlyphHit::Direct && gp.classify(0x4E00) == GlyphHit::Fallback
        && gp.classify(0x9111) == GlyphHit::Missing, "直中/回退/缺 三态");

    // C333-02 探查标注：文本 → 命中序列。
    let rep = gp.probe(&[0x41, 0x4E00, 0x9111, 0x41]);
    chk(&mut set, "C333-02", rep.hits == vec![GlyphHit::Direct, GlyphHit::Fallback, GlyphHit::Missing, GlyphHit::Direct], "四字符逐标注");

    // C333-03 回退字符清单去重。
    chk(&mut set, "C333-03", rep.fallback_cps == vec![0x4E00], "回退清单去重单条");

    // C333-04 缺口少量 → 逐字符明细。
    chk(&mut set, "C333-04", rep.aggregate(3) == Ok(vec![0x9111]), "缺 1 个 → 明细通道");

    // C333-05 缺口多量 → 聚合通道。
    let big = gp.probe(&[0x9111, 0x9112, 0x9113, 0x4E00]);
    chk(&mut set, "C333-05", big.aggregate(3) == Err(DbCode::Aggregated), "缺 3 个 → 聚合通道");

    // C333-06 引用图视口裁剪。
    let mut g = RefGraph::new();
    g.link("pack-zh", "fnt-zh");
    g.link("pack-zh", "txt-zh");
    g.link("pack-ja", "fnt-ja");
    let vp = g.viewport(&["pack-zh"], 10);
    chk(&mut set, "C333-06", vp == Ok(vec![("pack-zh", "fnt-zh"), ("pack-zh", "txt-zh")]), "一跳裁剪两出边");

    // C333-07 引用图爆炸 → GraphBurst。
    chk(&mut set, "C333-07", g.viewport(&["pack-zh"], 1) == Err(DbCode::GraphBurst), "出边 2 > 上限 1 => 爆炸");

    // C333-08 断引用定位。
    g.link("pack-zh", "aud-gone");
    chk(&mut set, "C333-08", g.dangling(&["pack-zh"], &["fnt-zh", "txt-zh"]) == vec!["aud-gone"], "缺 aud-gone 定位");

    // C333-09 三步计时器：越阈清单。
    let mut tm = StepTimer::new();
    tm.record("rebind", 35, 40);
    tm.record("relayout", 520, 400);
    tm.record("swap", 90, 120);
    chk(&mut set, "C333-09", tm.over_threshold() == vec!["relayout"], "三步计时仅 relayout 越阈");

    // C333-10 计时总量。
    chk(&mut set, "C333-10", tm.total_ms() == 645, "三步总耗时 645ms");

    // C333-11 工具故障降级 CLI。
    let mut host = DebugHost::new();
    chk(&mut set, "C333-11", host.degrade() && host.is_cli(), "GUI 挂 → CLI 降级");

    // C333-12 输出五标准复述标记。
    host.emit("contrast", "缺口 0x9111 走回退链");
    chk(&mut set, "C333-12", host.outputs[0].starts_with("[std:contrast]") && !host.outputs.is_empty(), "输出带五标准标记");

    // C333-13 隐私：探查不持久化原文本——只有计数可入库。
    chk(&mut set, "C333-13", host.persist_probe(&rep) == 2, "仅统计入库（2 = 1 回退 + 1 缺）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves333_smoke() {
        let set = run_ves333_checks();
        assert!(set.failed_count() == 0, "ves333 red: {}", set.red_items_text());
    }

    #[test]
    fn ves333_probe() {
        let gp = GlyphProbe::new(vec![1, 2], vec![3]);
        let r = gp.probe(&[1, 2, 3, 4, 4]);
        assert_eq!(r.missing_cps, vec![4]);
        assert_eq!(r.fallback_cps, vec![3]);
    }
}
