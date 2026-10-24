//! VE-F0552 · WGSL 一致性套件
//!
//! WGSL 一致性测试套件三层：功能层（全族生成用例）、对拍层（文本快照对拍为主——
//! 生态执行环境受限显性声明）、回归层（随修入册）；fast 档为常态运行档
//! （full 档依赖生态工具到位随闸门登记）；结果聚合入收口。
//!
//! 跨批对接：上游 F0541-F0551；下游 F0560 组收口证据准备。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 套件三层
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteLayer {
    /// 功能层：全族生成用例
    Func,
    /// 对拍层：文本快照对拍
    Diff,
    /// 回归层：随修入册
    Regression,
}

/// 档位：fast 常态运行，full 依赖生态工具
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Fast,
    Full,
}

/// 工具到位闸门：full 档随闸门登记
#[derive(Clone, Debug)]
pub struct ToolGate {
    /// 生态工具是否到位
    pub tools_ready: bool,
    /// 登记时间戳（单调序号）
    pub registered_at: u64,
}

impl ToolGate {
    pub fn new(tools_ready: bool, registered_at: u64) -> Self {
        ToolGate { tools_ready, registered_at }
    }

    /// fast 恒可行；full 仅当工具到位
    pub fn allows(&self, m: Mode) -> bool {
        match m {
            Mode::Fast => true,
            Mode::Full => self.tools_ready,
        }
    }
}

/// 单条用例
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Case {
    pub layer: SuiteLayer,
    pub name: String,
    /// 期望 WGSL 文本（快照对拍用）
    pub expect: String,
    /// 实际 WGSL 文本
    pub actual: String,
    /// 回归用例关联的修复号（回归层用）
    pub fix_ref: Option<String>,
}

impl Case {
    pub fn new(layer: SuiteLayer, name: &str, expect: &str, actual: &str) -> Self {
        Case {
            layer,
            name: name.to_string(),
            expect: expect.to_string(),
            actual: actual.to_string(),
            fix_ref: None,
        }
    }

    pub fn regression(name: &str, expect: &str, actual: &str, fix_ref: &str) -> Self {
        let mut c = Case::new(SuiteLayer::Regression, name, expect, actual);
        c.fix_ref = Some(fix_ref.to_string());
        c
    }

    pub fn pass(&self) -> bool {
        self.expect == self.actual
    }
}

/// 用例执行器：按档位过滤并执行
pub struct Runner {
    pub gate: ToolGate,
    pub mode: Mode,
}

impl Runner {
    pub fn new(gate: ToolGate, mode: Mode) -> Self {
        Runner { gate, mode }
    }

    /// 档位声明如实：full 不满足闸门时降级 fast
    pub fn effective_mode(&self) -> Mode {
        if self.gate.allows(self.mode) {
            self.mode
        } else {
            Mode::Fast
        }
    }

    /// 执行：fast 档仅跑功能层与回归层；full 档三层全跑
    pub fn run<'a>(&self, cases: &'a [Case]) -> Vec<(&'a str, bool)> {
        let eff = self.effective_mode();
        let mut out = Vec::new();
        for c in cases {
            let included = match eff {
                Mode::Fast => c.layer != SuiteLayer::Diff,
                Mode::Full => true,
            };
            if included {
                out.push((c.name.as_str(), c.pass()));
            }
        }
        out
    }

    /// 结果聚合：通过率（千分位），供收口消费
    pub fn aggregate(&self, results: &[(&str, bool)]) -> u32 {
        if results.is_empty() {
            return 1000;
        }
        let pass = results.iter().filter(|r| r.1).count();
        (pass * 1000 / results.len()) as u32
    }
}

/// 回归册：随修入册
#[derive(Clone, Debug, Default)]
pub struct RegressionBook {
    /// 入册条目（修复号 → 用例名）
    entries: Vec<(String, String)>,
}

impl RegressionBook {
    pub fn new() -> Self {
        RegressionBook::default()
    }

    /// 随修入册：回归用例失败修复后必须入册
    pub fn record(&mut self, fix_ref: &str, case_name: &str) -> bool {
        if fix_ref.is_empty() || case_name.is_empty() {
            return false;
        }
        if self.entries.iter().any(|(f, c)| f == fix_ref && c == case_name) {
            return false;
        }
        self.entries.push((fix_ref.to_string(), case_name.to_string()));
        true
    }

    /// 复现阻断：入册条目对应的用例必须存在且通过
    pub fn verify_block(&self, cases: &[Case]) -> Option<String> {
        for (fix, case) in &self.entries {
            match cases.iter().find(|c| &c.name == case) {
                None => return Some(alloc::format!("missing case for fix {}", fix)),
                Some(c) if !c.pass() => return Some(alloc::format!("case {} still failing", case)),
                Some(_) => {}
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 快照对拍判定：失配 → 判定流程（返回处置结论）
pub enum DiffVerdict {
    /// 快照需更新（实现有意变更，走登记）
    UpdateSnapshot,
    /// 实现回归（需阻断修复）
    ImplRegression,
    /// 快照本身过期（格式规范升级）
    StaleSnapshot,
}

/// 判定流程：根据登记标记决定失配结论
pub fn judge_diff(expect_changed: bool, impl_changed: bool, spec_bumped: bool) -> DiffVerdict {
    if spec_bumped {
        DiffVerdict::StaleSnapshot
    } else if expect_changed {
        DiffVerdict::UpdateSnapshot
    } else if impl_changed {
        DiffVerdict::ImplRegression
    } else {
        DiffVerdict::UpdateSnapshot
    }
}

pub fn run_vec152_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec152");
    // 族一：三层用例
    let f1 = Case::new(SuiteLayer::Func, "func-vertex", "fn vs_main(){}", "fn vs_main(){}");
    let d1 = Case::new(SuiteLayer::Diff, "diff-snapshot", "@group(0) @binding(0)", "@group(0) @binding(0)");
    let r1 = Case::regression("reg-42", "let x = 1;", "let x = 1;", "FIX-42");
    let cases = vec![f1, d1, r1];
    s.add("三层-用例齐备", cases.len() == 3, "n3");
    s.add("回归-修复号关联", cases[2].fix_ref == Some("FIX-42".to_string()), "fixref");
    s.add("用例-通过判定", cases.iter().all(|c| c.pass()), "allpass");
    // 族二：档位如实与闸门
    let gate_on = ToolGate::new(true, 7);
    let gate_off = ToolGate::new(false, 7);
    s.add("闸门-fast恒可行", gate_on.allows(Mode::Fast) && gate_off.allows(Mode::Fast), "fast");
    s.add("闸门-full需工具", gate_on.allows(Mode::Full) && !gate_off.allows(Mode::Full), "full");
    let rn_full = Runner::new(gate_on, Mode::Full);
    s.add("档位-工具有效full", rn_full.effective_mode() == Mode::Full, "full-ok");
    let rd_full = Runner::new(gate_off.clone(), Mode::Full);
    s.add("档位-工具缺降级fast", rd_full.effective_mode() == Mode::Fast, "down");
    // 族三：执行过滤——fast 档常态运行仅功能+回归
    let run_fast = Runner::new(gate_off.clone(), Mode::Fast);
    let res_fast = run_fast.run(&cases);
    s.add("fast-跳过对拍层", res_fast.len() == 2, "skip-diff");
    let res_full = rn_full.run(&cases);
    s.add("full-三层全跑", res_full.len() == 3, "all");
    s.add("聚合-全过千分位", run_fast.aggregate(&res_fast) == 1000, "agg");
    // 族四：聚合率计算
    let fail_case = Case::new(SuiteLayer::Func, "f", "a", "b");
    let res2 = vec![("p", true), ("q", false)];
    s.add("聚合-半过500", run_fast.aggregate(&res2) == 500, "half");
    s.add("聚合-空集满分", run_fast.aggregate(&[]) == 1000, "empty");
    let _ = &fail_case;
    // 族五：回归册随修入册
    let mut book = RegressionBook::new();
    s.add("回归册-入册成功", book.record("FIX-42", "reg-42"), "rec");
    s.add("回归册-重复拒绝", !book.record("FIX-42", "reg-42"), "dup");
    s.add("回归册-空串拒绝", !book.record("", "x") && !book.record("f", ""), "empty");
    s.add("回归册-验证通过", book.verify_block(&cases).is_none(), "verify");
    // 族六：复现阻断
    let broken_reg = vec![Case::regression("reg-42", "a", "b", "FIX-42")];
    s.add("复现-失配阻断", book.verify_block(&broken_reg).is_some(), "block");
    let missing_reg = vec![Case::new(SuiteLayer::Regression, "other", "a", "a")];
    s.add("复现-缺用例阻断", book.verify_block(&missing_reg).is_some(), "miss");
    // 族七：快照失配判定流程
    let v1 = judge_diff(true, false, false);
    s.add("判定-快照更新", matches!(v1, DiffVerdict::UpdateSnapshot), "upd");
    let v2 = judge_diff(false, true, false);
    s.add("判定-实现回归", matches!(v2, DiffVerdict::ImplRegression), "reg");
    let v3 = judge_diff(false, false, true);
    s.add("判定-快照过期", matches!(v3, DiffVerdict::StaleSnapshot), "stale");
    s
}
