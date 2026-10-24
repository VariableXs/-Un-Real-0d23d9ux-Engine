//! VE-F0572 · SPIR-V 一致性套件
//!
//! 三层（功能层全族生成用例、对拍层反汇编快照加 F0569 语义声明、回归层随修入册）；
//! 档位如实：fast 档常态（spirv-val 加快照抽样）、full 档随闸门（全量快照加批量验证）；
//! 与 F0509/F0533/F0552 三后端套件同构家族（第四成员），聚合视图进 C 域四联审（F0598）；
//! 用例规模与运行时长预算登记（fast 分钟级）。
//!
//! 跨批对接：上游 F0561-F0571 全组；家族同构 F0509/F0533/F0552；下游 F0580 组收口、F0598 四联审。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 套件三层
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Func,
    Diff,
    Regression,
}

/// 档位
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// fast 常态：spirv-val 加快照抽样
    Fast,
    /// full 随闸门：全量快照加批量验证
    Full,
}

/// 工具闸门
#[derive(Clone, Debug)]
pub struct Gate {
    pub tools_ready: bool,
    pub registered_at: u64,
}

impl Gate {
    pub fn allows(&self, m: Mode) -> bool {
        match m {
            Mode::Fast => true,
            Mode::Full => self.tools_ready,
        }
    }
}

/// 用例
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Case {
    pub layer: Layer,
    pub name: String,
    pub expect: Vec<String>,
    pub actual: Vec<String>,
    pub fix_ref: Option<String>,
}

impl Case {
    pub fn new(layer: Layer, name: &str, expect: &[&str], actual: &[&str]) -> Self {
        Case {
            layer,
            name: name.to_string(),
            expect: expect.iter().map(|s| s.to_string()).collect(),
            actual: actual.iter().map(|s| s.to_string()).collect(),
            fix_ref: None,
        }
    }

    pub fn regression(name: &str, expect: &[&str], actual: &[&str], fix: &str) -> Self {
        let mut c = Case::new(Layer::Regression, name, expect, actual);
        c.fix_ref = Some(fix.to_string());
        c
    }

    pub fn pass(&self) -> bool {
        self.expect == self.actual
    }
}

/// 快照抽样：fast 档按步长抽样对拍
pub fn sample_by_step<'a>(names: &[&'a str], step: usize) -> Vec<&'a str> {
    if step == 0 {
        return Vec::new();
    }
    names.iter().enumerate().filter(|(i, _)| i % step == 0).map(|(_, n)| *n).collect()
}

/// 执行器
pub struct SuiteRunner {
    pub gate: Gate,
    pub mode: Mode,
}

impl SuiteRunner {
    pub fn effective(&self) -> Mode {
        if self.gate.allows(self.mode) {
            self.mode
        } else {
            Mode::Fast
        }
    }

    /// fast：功能层+回归层全跑，对拍层抽样；full：全量
    pub fn run<'a>(&self, cases: &'a [Case], sample_step: usize) -> Vec<(&'a str, bool)> {
        let eff = self.effective();
        let mut out = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            let included = match eff {
                Mode::Full => true,
                Mode::Fast => match c.layer {
                    Layer::Diff => i % sample_step.max(1) == 0,
                    _ => true,
                },
            };
            if included {
                out.push((c.name.as_str(), c.pass()));
            }
        }
        out
    }

    /// 聚合（千分位）
    pub fn aggregate(&self, results: &[(&str, bool)]) -> u32 {
        if results.is_empty() {
            return 1000;
        }
        (results.iter().filter(|r| r.1).count() * 1000 / results.len()) as u32
    }
}

/// 时长预算登记（fast 分钟级）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeBudget {
    pub max_fast_secs: u32,
    pub actual_fast_secs: u32,
}

impl TimeBudget {
    pub fn within(&self) -> bool {
        self.actual_fast_secs <= self.max_fast_secs
    }
}

/// 家族聚合视图接口：进 C 域四联审（F0598）
#[derive(Clone, Debug, Default)]
pub struct FamilyView {
    /// 四成员套件名与通过率
    pub members: Vec<(&'static str, u32)>,
}

impl FamilyView {
    pub fn add(&mut self, name: &'static str, rate: u32) -> bool {
        if self.members.iter().any(|(n, _)| *n == name) {
            return false;
        }
        self.members.push((name, rate));
        true
    }

    /// 四联审齐备：四成员到齐
    pub fn quorum(&self) -> bool {
        self.members.len() == 4
    }

    /// 最低通过率
    pub fn min_rate(&self) -> u32 {
        self.members.iter().map(|(_, r)| *r).min().unwrap_or(0)
    }
}

/// 回归册
#[derive(Clone, Debug, Default)]
pub struct RegBook {
    entries: Vec<(String, String)>,
}

impl RegBook {
    pub fn record(&mut self, fix: &str, case: &str) -> bool {
        if fix.is_empty() || case.is_empty() || self.entries.iter().any(|(f, c)| f == fix && c == case) {
            return false;
        }
        self.entries.push((fix.to_string(), case.to_string()));
        true
    }

    /// 复现阻断
    pub fn verify(&self, cases: &[Case]) -> Option<String> {
        for (fix, case) in &self.entries {
            match cases.iter().find(|c| &c.name == case) {
                None => return Some(alloc::format!("missing case for fix {}", fix)),
                Some(c) if !c.pass() => return Some(alloc::format!("case {} still failing", case)),
                Some(_) => {}
            }
        }
        None
    }
}

pub fn run_vec172_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec172");
    // 族一：三层用例
    let cases = vec![
        Case::new(Layer::Func, "func-vert", &["OpCapability Shader"], &["OpCapability Shader"]),
        Case::new(Layer::Diff, "diff-snap", &["OpMemoryModel"], &["OpMemoryModel"]),
        Case::regression("reg-11", &["OpLoad"], &["OpLoad"], "FIX-11"),
    ];
    s.add("三层-齐备", cases.len() == 3, "n3");
    s.add("用例-全过", cases.iter().all(|c| c.pass()), "pass");
    s.add("回归-修复号", cases[2].fix_ref == Some("FIX-11".to_string()), "fix");
    // 族二：档位如实
    let g_on = Gate { tools_ready: true, registered_at: 1 };
    let g_off = Gate { tools_ready: false, registered_at: 1 };
    s.add("闸门-fast恒可行", g_off.allows(Mode::Fast), "fast");
    s.add("闸门-full需工具", g_on.allows(Mode::Full) && !g_off.allows(Mode::Full), "full");
    let r_full = SuiteRunner { gate: g_on, mode: Mode::Full };
    let r_fast = SuiteRunner { gate: g_off, mode: Mode::Full };
    s.add("档位-工具有效", r_full.effective() == Mode::Full, "eff1");
    s.add("档位-缺工具降级", r_fast.effective() == Mode::Fast, "eff2");
    // 族三：fast 抽样
    let res_f = r_fast.run(&cases, 2);
    s.add("fast-对拍抽样隔条", res_f.len() == 2 && res_f.iter().any(|(n, _)| *n == "diff-snap"), "sample");
    let res_full = r_full.run(&cases, 2);
    s.add("full-全量", res_full.len() == 3, "all");
    s.add("聚合-千分位", r_full.aggregate(&res_full) == 1000, "agg");
    // 族四：抽样函数
    let names = ["a", "b", "c", "d", "e"];
    s.add("抽样-步长2", sample_by_step(&names, 2) == vec!["a", "c", "e"], "step2");
    s.add("抽样-步长0空", sample_by_step(&names, 0).is_empty(), "step0");
    // 族五：时长预算
    let tb = TimeBudget { max_fast_secs: 300, actual_fast_secs: 120 };
    s.add("预算-分钟级达标", tb.within(), "ok");
    let tb2 = TimeBudget { max_fast_secs: 300, actual_fast_secs: 400 };
    s.add("预算-超时检出", !tb2.within(), "over");
    // 族六：家族聚合视图（四联审）
    let mut fv = FamilyView::default();
    fv.add("glsl-suite", 1000);
    fv.add("hlsl-suite", 990);
    fv.add("wgsl-suite", 995);
    s.add("视图-三员未齐", !fv.quorum(), "q3");
    fv.add("spirv-suite", 980);
    s.add("视图-四联审齐", fv.quorum(), "q4");
    s.add("视图-最低率", fv.min_rate() == 980, "min");
    s.add("视图-重复拒绝", !fv.add("glsl-suite", 999), "dup");
    // 族七：回归册
    let mut rb = RegBook::default();
    s.add("册-入册", rb.record("FIX-11", "reg-11"), "rec");
    s.add("册-重复拒绝", !rb.record("FIX-11", "reg-11"), "dup");
    s.add("册-验证通过", rb.verify(&cases).is_none(), "verify");
    let broken = vec![Case::regression("reg-11", &["a"], &["b"], "FIX-11")];
    s.add("册-失配阻断", rb.verify(&broken).is_some(), "block");
    s
}
