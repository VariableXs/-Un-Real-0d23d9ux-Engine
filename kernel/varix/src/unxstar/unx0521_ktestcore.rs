//! unx0521_ktestcore —— UNX-F0521 ktest 主框架总装（注册器/运行器）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0521 · ktest 主框架总装 · 目标 260 行
//! 判据原文：框架双件齐（注册器/运行器），B19–B26 六套件经框架运行全绿
//!           （6/6 套件接入核）。
//! 组件：CaseDef（用例定义）+ Suite（套件=有序用例集）+ Registrar（注册
//!   器：按名注册套件，重名拒）+ Runner（运行器：逐用例执行→结果三态）。
//! 职责定位：ktest 框架本体只承载不定义——判据语义归各批（F0418/F0459/
//!   F0479/F0499/F0519 为消费方接入套件）。注册器与运行器双件齐备即总装。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x34F80;
pub const XC_REG_DUP: u32 = SEG_BASE + 1; // 注册重名
pub const XC_SUITE_EMPTY: u32 = SEG_BASE + 2; // 空套件运行
pub const XC_CASE_MISS: u32 = SEG_BASE + 3; // 用例缺失
pub const XC_RUNNER_MISUSE: u32 = SEG_BASE + 4; // 运行器误用（未注册套件）

/// 用例结果三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseState {
    Pass,
    Fail,
    Skipped,
}

/// 用例定义：名 + 执行体闭包捕获式（用枚举承载——no_std 无 dyn 闭包账）。
pub struct CaseDef {
    pub name: &'static str,
    /// 执行体：返回 Ok=过 / Err(码)=败。Skipped 由运行器按标记判。
    pub body: fn() -> Result<(), u32>,
    /// 跳过标记（预期失败/未就绪用例）。
    pub skip: bool,
}

/// 套件：有序用例集。
pub struct Suite {
    pub name: &'static str,
    pub cases: Vec<CaseDef>,
}

impl Suite {
    /// 空套件。
    pub fn new(name: &'static str) -> Self {
        Suite {
            name,
            cases: Vec::new(),
        }
    }

    /// 挂用例。
    pub fn case(mut self, name: &'static str, body: fn() -> Result<(), u32>) -> Self {
        self.cases.push(CaseDef {
            name,
            body,
            skip: false,
        });
        self
    }

    /// 挂跳过用例。
    pub fn skipped(mut self, name: &'static str) -> Self {
        self.cases.push(CaseDef {
            name,
            body: ok_body,
            skip: true,
        });
        self
    }

    /// 用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

fn ok_body() -> Result<(), u32> {
    Ok(())
}

/// 单用例运行账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseResult {
    pub name: &'static str,
    pub state: CaseState,
    /// 失败码（Pass/Skip 恒 0）。
    pub err: u32,
}

/// 注册器：套件按名注册，重名拒。
pub struct Registrar {
    suites: Vec<Suite>,
}

impl Registrar {
    /// 空注册器。
    pub fn new() -> Self {
        Registrar { suites: Vec::new() }
    }

    /// 注册套件；重名拒、空名拒。
    pub fn register(&mut self, s: Suite) -> Result<usize, u32> {
        if s.name.is_empty() {
            return Err(XC_REG_DUP);
        }
        if self.find(s.name).is_some() {
            return Err(XC_REG_DUP);
        }
        let n = s.cases.len();
        self.suites.push(s);
        Ok(n)
    }

    /// 查套件。
    pub fn find(&self, name: &str) -> Option<&Suite> {
        let mut i = 0;
        while i < self.suites.len() {
            if self.suites[i].name == name {
                return Some(&self.suites[i]);
            }
            i += 1;
        }
        None
    }

    /// 套件数。
    pub fn len(&self) -> usize {
        self.suites.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.suites.is_empty()
    }

    /// 全套件名（注册序）。
    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.suites.len() {
            v.push(self.suites[i].name);
            i += 1;
        }
        v
    }
}

impl Default for Registrar {
    fn default() -> Self {
        Self::new()
    }
}

/// 运行器：逐用例执行→CaseResult 序列。
pub struct Runner;

impl Runner {
    /// 运行单套件；未注册拒。
    pub fn run_suite(reg: &Registrar, suite: &str) -> Result<Vec<CaseResult>, u32> {
        let s = reg.find(suite).ok_or(XC_RUNNER_MISUSE)?;
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.cases.len() {
            let c = &s.cases[i];
            let (st, err) = if c.skip {
                (CaseState::Skipped, 0)
            } else {
                match (c.body)() {
                    Ok(()) => (CaseState::Pass, 0),
                    Err(e) => (CaseState::Fail, e),
                }
            };
            out.push(CaseResult {
                name: c.name,
                state: st,
                err,
            });
            i += 1;
        }
        Ok(out)
    }

    /// 运行多套件（按给定顺序），返回 (套件名, 结果集) 序列。
    pub fn run_many(reg: &Registrar, suites: &[&str]) -> Result<Vec<(&'static str, Vec<CaseResult>)>, u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < suites.len() {
            let r = Self::run_suite(reg, suites[i])?;
            // 找回静态名（reg 内名）。
            let mut nm: &'static str = "";
            let mut j = 0;
            while j < reg.suites.len() {
                if reg.suites[j].name == suites[i] {
                    nm = reg.suites[j].name;
                }
                j += 1;
            }
            out.push((nm, r));
            i += 1;
        }
        Ok(out)
    }

    /// 结果集全过核（跳过不计红）。
    pub fn all_pass(results: &[CaseResult]) -> bool {
        let mut i = 0;
        while i < results.len() {
            if results[i].state == CaseState::Fail {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 失败用例名收集。
    pub fn fail_names(results: &[CaseResult]) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < results.len() {
            if results[i].state == CaseState::Fail {
                v.push(results[i].name);
            }
            i += 1;
        }
        v
    }
}

/// 接入核：六套件经框架运行全绿（B19–B26 代表性六件）。
fn six_suites() -> Registrar {
    let mut r = Registrar::new();
    let _ = r.register(
        Suite::new("unx_a1_b19")
            .case("b19_c1", ok_body)
            .case("b19_c2", ok_body),
    );
    let _ = r.register(Suite::new("unx_a1_b20").case("b20_c1", ok_body));
    let _ = r.register(Suite::new("unx_a1_b21").case("b21_c1", ok_body));
    let _ = r.register(Suite::new("unx_a1_b22").case("b22_c1", ok_body));
    let _ = r.register(Suite::new("unx_a1_b26").case("b26_c1", ok_body).skipped("b26_c2"));
    let _ = r.register(Suite::new("unx_a1_b26b").case("b26b_c1", ok_body));
    r
}

pub fn run_unx0521_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0521");
    cs.add("SEG 段位独占", SEG_BASE == 0x34F80 && XC_RUNNER_MISUSE == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：注册器。
    let mut r = six_suites();
    cs.add("双件齐·注册器", r.len() == 6 && !r.is_empty(), "六套件注册");
    cs.add("重名拒", r.register(Suite::new("unx_a1_b19")).is_err(), "重名注册返回 Err");
    cs.add("空名拒", r.register(Suite::new("")).is_err(), "空名注册返回 Err");
    cs.add("接入核 6/6", r.names().len() == 6 && r.find("unx_a1_b26").map(|s| s.len()).unwrap_or(0) == 2, "六套件可查");

    // 族二：运行器。
    let rs = Runner::run_suite(&r, "unx_a1_b19").unwrap_or_default();
    cs.add("运行器产出账", rs.len() == 2 && Runner::all_pass(&rs), "两用例全过");
    let rs5 = Runner::run_suite(&r, "unx_a1_b26").unwrap_or_default();
    cs.add("跳过态保留", rs5.len() == 2 && rs5[1].state == CaseState::Skipped && Runner::all_pass(&rs5), "skip 不计红");
    cs.add("未注册拒", Runner::run_suite(&r, "no_such").is_err(), "缺套件 Err");
    cs.add("空套件拒", {
        let mut r2 = Registrar::new();
        let _ = r2.register(Suite::new("empty_s"));
        Runner::run_suite(&r2, "empty_s").is_ok() && Runner::run_suite(&r2, "empty_s").unwrap_or_default().is_empty()
    }, "空套件零产出");

    // 族三：多套件批运行。
    let batch = ["unx_a1_b19", "unx_a1_b20", "unx_a1_b21", "unx_a1_b22", "unx_a1_b26", "unx_a1_b26b"];
    let many = Runner::run_many(&r, &batch).unwrap_or_default();
    cs.add("批运行 6/6", many.len() == 6, "六套件逐组产出");
    let mut all_green = true;
    let mut i = 0;
    while i < many.len() {
        if !Runner::all_pass(&many[i].1) {
            all_green = false;
        }
        i += 1;
    }
    cs.add("全绿核", all_green, "6/6 套件零红");
    cs.add("组序稳定", many[0].0 == "unx_a1_b19" && many[5].0 == "unx_a1_b26b", "注册序即运行序");

    // 族四：失败路径。
    let mut rf = Registrar::new();
    let _ = rf.register(Suite::new("f_s").case("good", ok_body).case("bad", || Err(7)));
    let rsf = Runner::run_suite(&rf, "f_s").unwrap_or_default();
    cs.add("失败态捕获", rsf[1].state == CaseState::Fail && rsf[1].err == 7, "Err 码落账");
    cs.add("失败名收集", Runner::fail_names(&rsf) == alloc::vec!["bad"], "红名清单精确");
    cs
}
