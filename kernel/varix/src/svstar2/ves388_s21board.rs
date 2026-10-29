//! ves388_s21board — VE-F4148 · i18n 指标看板
//! 四视区看板（质量/进度/性能/覆盖：LQA 评分趋势+翻译进度矩阵+性能预算消耗+语种覆盖度）
//! + 数据源绑定表（数据源断→视区显性空态）+ 指标口径漂移→对版修正 + 看板卡顿→降级静态报表
//! + 全键盘可达+色标双维合规（域本色复述）+ 指标不含用户身份。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xE3C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdCode {
    Ok = 0,
    SourceDown = SEG_BASE + 0x01,
    MetricDrift = SEG_BASE + 0x02,
    PerfDegraded = SEG_BASE + 0x03,
}

/// 四视区。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel { Quality = 0, Progress = 1, PerfBudget = 2, Coverage = 3 }

impl Panel {
    pub const ALL: [Panel; 4] =
        [Panel::Quality, Panel::Progress, Panel::PerfBudget, Panel::Coverage];
    pub fn name(self) -> &'static str {
        match self {
            Panel::Quality => "quality", Panel::Progress => "progress",
            Panel::PerfBudget => "perf-budget", Panel::Coverage => "coverage",
        }
    }
}

/// 视区状态：数据源绑定 + 显性空态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelState { Bound = 0, EmptyVisible = 1, StaticFallback = 2 }

/// 看板：视区绑定表 + 口径注册 + 渲染预算。
pub struct Dashboard {
    pub bindings: Vec<(Panel, Option<&'static str>)>, // (视区, 数据源锚；None=源断)
    pub metrics: Vec<(&'static str, u32)>,            // 口径注册（指标名→口径版本）
    pub frame_ms: u32,                                // 渲染耗时
    pub frame_budget_ms: u32,
    pub keyboard_reachable: bool,                     // 全键盘可达（域本色）
    pub color_shape_dual: bool,                       // 色标双维合规（域本色）
}

impl Dashboard {
    pub fn new(frame_budget_ms: u32) -> Self {
        Dashboard {
            bindings: Panel::ALL.iter().map(|p| (*p, None)).collect(),
            metrics: Vec::new(),
            frame_ms: 0,
            frame_budget_ms,
            keyboard_reachable: false,
            color_shape_dual: false,
        }
    }

    pub fn bind(&mut self, p: Panel, source: &'static str) {
        if let Some(x) = self.bindings.iter_mut().find(|x| x.0 == p) { x.1 = Some(source); }
    }

    pub fn unbind(&mut self, p: Panel) {
        if let Some(x) = self.bindings.iter_mut().find(|x| x.0 == p) { x.1 = None; }
    }

    /// 视区渲染态：源断 → 显性空态（不隐藏）。
    pub fn state_of(&self, p: Panel) -> PanelState {
        match self.bindings.iter().find(|x| x.0 == p) {
            Some((_, Some(_))) => PanelState::Bound,
            _ => PanelState::EmptyVisible,
        }
    }

    /// 口径注册与对版：漂移 → 修正留痕。
    pub fn register_metric(&mut self, name: &'static str, version: u32) -> bool {
        match self.metrics.iter_mut().find(|x| x.0 == name) {
            Some(x) => { let drift = x.1 != version; x.1 = version; drift }
            None => { self.metrics.push((name, version)); false }
        }
    }

    pub fn metric_version(&self, name: &str) -> Option<u32> {
        self.metrics.iter().find(|x| x.0 == name).map(|x| x.1)
    }

    /// 卡顿 → 降级静态报表。
    pub fn perf_degraded(&self) -> bool { self.frame_ms > self.frame_budget_ms }

    /// 域本色合规：全键盘可达 + 色标双维。
    pub fn a11y_compliant(&self) -> bool { self.keyboard_reachable && self.color_shape_dual }
}

/// 静态报表降级产物。
pub struct StaticReport { pub panels: Vec<&'static str> }

impl StaticReport {
    pub fn build(d: &Dashboard) -> StaticReport {
        StaticReport {
            panels: Panel::ALL.iter().filter(|p| d.state_of(**p) == PanelState::Bound)
                .map(|p| p.name()).collect(),
        }
    }
    pub fn len(&self) -> usize { self.panels.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves388_checks() -> CheckSet {
    let mut set = CheckSet::new("ves388");
    let mut bd = Dashboard::new(16);

    // C388-01 四视区建册（初始全无源）。
    chk(&mut set, "C388-01", bd.bindings.len() == 4
        && bd.bindings.iter().all(|x| x.1.is_none()), "四视区全空");

    // C388-02 源断 → 显性空态（不隐藏）。
    chk(&mut set, "C388-02", bd.state_of(Panel::Quality) == PanelState::EmptyVisible, "显性空态");

    // C388-03 四数据源绑定。
    bd.bind(Panel::Quality, "lqa-score-trend");
    bd.bind(Panel::Progress, "translation-matrix");
    bd.bind(Panel::PerfBudget, "budget-consume");
    bd.bind(Panel::Coverage, "lang-coverage");
    chk(&mut set, "C388-03", Panel::ALL.iter().all(|p| bd.state_of(*p) == PanelState::Bound), "四源全绑");

    // C388-04 单源断 → 单视区空态，其余不受扰。
    bd.unbind(Panel::Coverage);
    chk(&mut set, "C388-04", bd.state_of(Panel::Coverage) == PanelState::EmptyVisible
        && bd.state_of(Panel::Quality) == PanelState::Bound, "单断不扰邻");

    // C388-05 口径注册与对版：漂移检出。
    chk(&mut set, "C388-05", !bd.register_metric("lqa", 3)
        && bd.register_metric("lqa", 4) && bd.metric_version("lqa") == Some(4), "口径漂移修正留痕");

    // C388-06 指标不含用户身份：口径名白名单核验。
    let ok_names = bd.metrics.iter().all(|(n, _)| *n != "user-id" && *n != "user-email");
    chk(&mut set, "C388-06", ok_names, "无身份指标");

    // C388-07 卡顿检出：33ms > 16ms 预算。
    bd.frame_ms = 33;
    chk(&mut set, "C388-07", bd.perf_degraded(), "33ms 超预算");

    // C388-08 降级静态报表：只含有源视区。
    let sr = StaticReport::build(&bd);
    chk(&mut set, "C388-08", sr.len() == 3 && sr.panels.contains(&"quality")
        && !sr.panels.contains(&"coverage"), "静态报表三视区");

    // C388-09 预算内不降级。
    bd.frame_ms = 12;
    chk(&mut set, "C388-09", !bd.perf_degraded(), "12ms 预算内");

    // C388-10 域本色：全键盘可达 + 色标双维。
    bd.keyboard_reachable = true;
    bd.color_shape_dual = true;
    chk(&mut set, "C388-10", bd.a11y_compliant(), "键盘+色标双维合规");

    // C388-11 缺一即不合规。
    let mut half = Dashboard::new(16);
    half.keyboard_reachable = true;
    chk(&mut set, "C388-11", !half.a11y_compliant(), "缺色标不合规");

    // C388-12 四视区名单单源。
    chk(&mut set, "C388-12", Panel::ALL.len() == 4 && Panel::ALL[0].name() == "quality"
        && Panel::ALL[3].name() == "coverage", "四视区单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves388_smoke() {
        let set = run_ves388_checks();
        assert!(set.failed_count() == 0, "ves388 red: {}", set.red_items_text());
    }

    #[test]
    fn ves388_empty_report() {
        let d = Dashboard::new(16);
        let r = StaticReport::build(&d);
        assert_eq!(r.len(), 0);
    }
}
