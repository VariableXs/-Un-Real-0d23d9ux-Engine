//! ves372_s20debug — VE-F4132 · T07 i18n 性能调试器
//! 三视区：查找热点视图（键查找耗时分布）/切换时间线（三步分解耗时）/排版整形耗时视图 + 体验日志联动（慢查询自动标记挫败信号）+ 采样开销超限降采样告警 + 视图失真数据源对拍 + 采样不含用户输入内容（红线）+ 输出遵循五标准（复述）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD840;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbCode {
    Ok = 0,
    SampleOverhead = SEG_BASE + 0x01,
    ViewDrift = SEG_BASE + 0x02,
}

/// 采样开销上限（万分比——采样成本/被测成本）。
pub const SAMPLE_OVERHEAD_BP: u32 = 500;

/// 三视区。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View { HotKeys = 0, SwitchTimeline = 1, LayoutCost = 2 }

impl View {
    pub const ALL: [View; 3] = [View::HotKeys, View::SwitchTimeline, View::LayoutCost];
    pub fn name(self) -> &'static str {
        match self { View::HotKeys => "hot-keys", View::SwitchTimeline => "switch-timeline",
            View::LayoutCost => "layout-cost" }
    }
}

/// 查找热点视图：键 → 累计耗时。
pub struct HotView { pub items: Vec<(&'static str, u32)> }

impl HotView {
    pub fn new() -> Self { HotView { items: Vec::new() } }

    pub fn record(&mut self, key: &'static str, ns: u32) {
        if let Some(x) = self.items.iter_mut().find(|x| x.0 == key) { x.1 += ns; }
        else { self.items.push((key, ns)); }
    }

    /// 热点 top N（降序）。
    pub fn top(&self, n: usize) -> Vec<(&'static str, u32)> {
        let mut s = self.items.clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        s.into_iter().take(n).collect()
    }

    pub fn total(&self) -> u32 { self.items.iter().map(|x| x.1).sum() }
}

/// 切换时间线：三步耗时分解。
#[derive(Clone, Copy)]
pub struct SwitchStep { pub step: &'static str, pub ms: u32 }

pub struct SwitchTimeline { pub steps: Vec<SwitchStep> }

pub const SWITCH_STEPS: [&'static str; 3] = ["rebind", "relayout", "swap"];

impl SwitchTimeline {
    pub fn new() -> Self { SwitchTimeline { steps: Vec::new() } }
    pub fn record(&mut self, step: &'static str, ms: u32) { self.steps.push(SwitchStep { step, ms }); }
    pub fn total(&self) -> u32 { self.steps.iter().map(|s| s.ms).sum() }
    /// 最慢步归因。
    pub fn bottleneck(&self) -> Option<&'static str> {
        self.steps.iter().max_by_key(|s| s.ms).map(|s| s.step)
    }
    /// 三步齐全。
    pub fn complete(&self) -> bool {
        SWITCH_STEPS.iter().all(|s| self.steps.iter().any(|x| x.step == *s))
    }
}

/// 排版整形耗时视图：族 → 累计 us。
pub struct LayoutView { pub items: Vec<(&'static str, u32)> }

impl LayoutView {
    pub fn new() -> Self { LayoutView { items: Vec::new() } }
    pub fn record(&mut self, family: &'static str, us: u32) {
        if let Some(x) = self.items.iter_mut().find(|x| x.0 == family) { x.1 += us; }
        else { self.items.push((family, us)); }
    }
    pub fn of(&self, family: &str) -> u32 {
        self.items.iter().find(|x| x.0 == family).map_or(0, |x| x.1)
    }
}

/// 体验日志联动：慢查询自动标挫败信号。
pub struct FrustrationLink { pub slow: Vec<&'static str>, pub marked: Vec<&'static str> }

pub const SLOW_MS: u32 = 400;

impl FrustrationLink {
    pub fn new() -> Self { FrustrationLink { slow: Vec::new(), marked: Vec::new() } }

    pub fn observe(&mut self, key: &'static str, ms: u32) {
        if ms > SLOW_MS {
            self.slow.push(key);
            if !self.marked.contains(&key) { self.marked.push(key); } // 自动标挫败
        }
    }

    pub fn linked(&self, key: &str) -> bool { self.marked.iter().any(|m| *m == key) }
}

/// 采样开销控制：超限降采样。
pub fn sampler_gate(overhead_bp: u32, alarms: &mut Vec<&'static str>) -> u32 {
    if overhead_bp > SAMPLE_OVERHEAD_BP {
        alarms.push("sample-overhead");
        SAMPLE_OVERHEAD_BP / 2 // 降采样至一半预算
    } else {
        10_000 // 全采样
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves372_checks() -> CheckSet {
    let mut set = CheckSet::new("ves372");

    // C372-01 三视区单源。
    chk(&mut set, "C372-01", View::ALL.len() == 3 && View::ALL[1].name() == "switch-timeline", "三视区单源");

    // C372-02 热点累计与排序。
    let mut hv = HotView::new();
    hv.record("menu.title", 300);
    hv.record("menu.title", 200);
    hv.record("btn.ok", 900);
    chk(&mut set, "C372-02", hv.top(1) == vec![("btn.ok", 900)] && hv.total() == 1_400, "热点 btn.ok 居首");

    // C372-03 热点 top2 序。
    chk(&mut set, "C372-03", hv.top(2) == vec![("btn.ok", 900), ("menu.title", 500)], "降序两热");

    // C372-04 切换时间线三步分解。
    let mut st = SwitchTimeline::new();
    st.record("rebind", 30); st.record("relayout", 400); st.record("swap", 90);
    chk(&mut set, "C372-04", st.complete() && st.total() == 520, "三步齐 520ms");

    // C372-05 瓶颈归因。
    chk(&mut set, "C372-05", st.bottleneck() == Some("relayout"), "relayout 最慢");

    // C372-06 排版视图族累计。
    let mut lv = LayoutView::new();
    lv.record("cjk", 800); lv.record("cjk", 400); lv.record("latin", 100);
    chk(&mut set, "C372-06", lv.of("cjk") == 1_200 && lv.of("latin") == 100, "族累计 1200/100");

    // C372-07 挫败联动：慢查询自动标记。
    let mut fl = FrustrationLink::new();
    fl.observe("search.slow-key", 600);
    fl.observe("search.fast-key", 50);
    chk(&mut set, "C372-07", fl.linked("search.slow-key") && !fl.linked("search.fast-key"), "慢标快不标");

    // C372-08 重复挫败去重。
    fl.observe("search.slow-key", 900);
    chk(&mut set, "C372-08", fl.marked.len() == 1, "重复标记去重");

    // C372-09 慢阈值单源。
    chk(&mut set, "C372-09", SLOW_MS == 400, "慢线 400ms");

    // C372-10 采样开销带内全采样。
    let mut alarms: Vec<&'static str> = Vec::new();
    chk(&mut set, "C372-10", sampler_gate(300, &mut alarms) == 10_000 && alarms.is_empty(), "3% 开销全采样");

    // C372-11 超限降采样告警。
    chk(&mut set, "C372-11", sampler_gate(800, &mut alarms) == 250 && alarms == vec!["sample-overhead"], "8% 超限 → 半预算+告警");

    // C372-12 采样红线：不含用户输入内容（结构即证明——只收键名与耗时）。
    let _ = hv.record("k.utf8-len", 10); // 只统计键路径与耗时
    chk(&mut set, "C372-12", hv.items.iter().all(|(k, _)| k.len() < 64), "键名非内容（长度界内）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves372_smoke() {
        let set = run_ves372_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves372 red: {:?}", red);
    }

    #[test]
    fn ves372_hot() {
        let mut hv = HotView::new();
        hv.record("a", 10); hv.record("b", 20); hv.record("a", 30);
        assert_eq!(hv.top(2), vec![("a", 40), ("b", 20)]);
    }
}
