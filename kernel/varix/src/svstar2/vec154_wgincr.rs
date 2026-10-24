//! VE-F0554 · WGSL 增量重编译
//!
//! WGSL 单翻译单元语义下无模块系统——任意声明可影响整体，增量只能做块级粗粒度
//! 并诚实声明边界：词法块级 diff 定位变更声明、变更声明及其依赖闭包重生成、
//! 未变声明文本直沿用；保守档为默认档；与 GLSL/HLSL 增量家族（F0511/F0535）
//! 差异显性：WGSL 无增量安全边界故不做语句级增量。
//!
//! 跨批对接：上游 F0541 WGSL 架构；家族同构 F0511/F0535；下游 F0582 增量级联。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 声明块：词法块级 diff 的基本单位
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decl {
    /// 声明名（fn/struct/var 等名字）
    pub name: String,
    /// 声明文本（规范化后）
    pub text: String,
    /// 依赖的其它声明名
    pub deps: Vec<String>,
}

impl Decl {
    pub fn new(name: &str, text: &str, deps: &[&str]) -> Self {
        Decl {
            name: name.to_string(),
            text: text.to_string(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }
}

/// 词法块级 diff 结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockDiff {
    /// 全等：无需增量
    Same,
    /// 变更声明名列表
    Changed(Vec<String>),
    /// diff 定位失败（无法按块切分）
    DiffFailed,
}

/// 块级 diff：按声明名比对文本，定位变更声明
pub fn block_diff(old: &[Decl], new: &[Decl]) -> BlockDiff {
    if old.len() != new.len() {
        // 声明数变化视为无法安全增量（保守）
        return BlockDiff::DiffFailed;
    }
    let mut changed = Vec::new();
    for (o, n) in old.iter().zip(new.iter()) {
        if o.name != n.name {
            return BlockDiff::DiffFailed;
        }
        if o.text != n.text {
            changed.push(n.name.clone());
        }
    }
    if changed.is_empty() {
        BlockDiff::Same
    } else {
        BlockDiff::Changed(changed)
    }
}

/// 依赖闭包：变更声明及其依赖闭包（WGSL 无模块系统，任一声明可影响整体）
pub fn closure_of(changed: &[String], new: &[Decl]) -> Vec<String> {
    let mut set: Vec<String> = Vec::new();
    let mut queue: Vec<String> = changed.to_vec();
    while let Some(name) = queue.pop() {
        if set.contains(&name) {
            continue;
        }
        set.push(name.clone());
        if let Some(d) = new.iter().find(|d| d.name == name) {
            for dep in &d.deps {
                if !set.contains(dep) {
                    queue.push(dep.clone());
                }
            }
        }
    }
    set.sort();
    set
}

/// 增量档位：保守档为默认档
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IncrMode {
    /// 保守档：默认，闭包超阈值或 diff 失败即整单元重编
    Conservative,
    /// 全量档：显式选择
    Full,
}

/// 增量收益账本：编辑期保底延迟对比全量
#[derive(Clone, Debug)]
pub struct BenefitLedger {
    /// 全量重编耗时（抽象单位）
    pub full_cost: u32,
    /// 实际增量耗时
    pub incr_cost: u32,
    /// 收益登记次数
    pub records: u32,
}

impl BenefitLedger {
    pub fn new(full_cost: u32) -> Self {
        BenefitLedger { full_cost, incr_cost: full_cost, records: 0 }
    }

    /// 收益诚实登记：每次增量记录实际耗时
    pub fn record(&mut self, incr_cost: u32) {
        self.incr_cost = incr_cost;
        self.records += 1;
    }

    /// 收益率（千分位）：1 - 增量/全量；无收益返回 0
    pub fn rate(&self) -> u32 {
        if self.full_cost == 0 || self.incr_cost >= self.full_cost {
            return 0;
        }
        1000 - (self.incr_cost * 1000 / self.full_cost)
    }
}

/// 增量重编译计划
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncrPlan {
    /// 未变声明文本直沿用（名 → 原文本）
    Reuse(Vec<String>),
    /// 需重生成的声明名
    Regen(Vec<String>),
    /// 整单元重编（回退路径）
    WholeUnit,
}

/// 计划编排：diff → 闭包 → 保守阈值判定 → 计划产出
pub fn plan_incr(old: &[Decl], new: &[Decl], closure_threshold: usize, mode: IncrMode) -> IncrPlan {
    if mode == IncrMode::Full {
        return IncrPlan::WholeUnit;
    }
    let diff = block_diff(old, new);
    let changed = match diff {
        BlockDiff::Same => return IncrPlan::Regen(vec![]),
        BlockDiff::DiffFailed => return IncrPlan::WholeUnit,
        BlockDiff::Changed(c) => c,
    };
    let closure = closure_of(&changed, new);
    if closure.len() > closure_threshold {
        // 依赖闭包超阈值 → 放弃增量直走全量
        return IncrPlan::WholeUnit;
    }
    let reuse: Vec<String> = new.iter().filter(|d| !closure.contains(&d.name)).map(|d| d.name.clone()).collect();
    IncrPlan::Reuse(reuse)
}

/// 快照失配告警：沿用文本与当前期望比对
pub fn snapshot_check(reused_text: &str, expected: &str) -> bool {
    reused_text == expected
}

pub fn run_vec154_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec154");
    let old = vec![
        Decl::new("a", "let a = 1;", &[]),
        Decl::new("b", "let b = a + 1;", &["a"]),
        Decl::new("c", "let c = b + 2;", &["b"]),
    ];
    // 族一：块级 diff
    let same = block_diff(&old, &old);
    s.add("diff-全等Same", same == BlockDiff::Same, "same");
    let mut new1 = old.clone();
    new1[1].text = "let b = a + 2;".to_string();
    s.add("diff-单块变更", block_diff(&old, &new1) == BlockDiff::Changed(vec!["b".to_string()]), "chg1");
    let mut new2 = old.clone();
    new2.push(Decl::new("d", "let d = 0;", &[]));
    s.add("diff-声明数变失败", block_diff(&old, &new2) == BlockDiff::DiffFailed, "fail");
    let mut new3 = old.clone();
    new3[0].name = "z".to_string();
    s.add("diff-改名失败", block_diff(&old, &new3) == BlockDiff::DiffFailed, "rename");
    // 族二：依赖闭包
    let clo1 = closure_of(&["b".to_string()], &old);
    s.add("闭包-自身加依赖", clo1 == vec!["a".to_string(), "b".to_string()], "clo1");
    let clo2 = closure_of(&["c".to_string()], &old);
    s.add("闭包-传递闭包", clo2 == vec!["a".to_string(), "b".to_string(), "c".to_string()], "clo2");
    let clo3 = closure_of(&["zz".to_string()], &old);
    s.add("闭包-未知声明仅自身", clo3 == vec!["zz".to_string()], "unk");
    // 族三：保守档计划
    let p_same = plan_incr(&old, &old, 2, IncrMode::Conservative);
    s.add("计划-全等零重生成", p_same == IncrPlan::Regen(vec![]), "zero");
    let p_small = plan_incr(&old, &new1, 3, IncrMode::Conservative);
    match &p_small {
        IncrPlan::Reuse(reuse) => {
            s.add("计划-未变沿用", reuse.contains(&"c".to_string()) && reuse.contains(&"d".to_string()), "reuse");
        }
        _ => s.add("计划-未变沿用", false, "wrong-arm"),
    }
    // 族四：回退整编
    let p_full = plan_incr(&old, &new1, 3, IncrMode::Full);
    s.add("回退-全量档整编", p_full == IncrPlan::WholeUnit, "full");
    let p_fail = plan_incr(&old, &new2, 3, IncrMode::Conservative);
    s.add("回退-diff失败整编", p_fail == IncrPlan::WholeUnit, "fail");
    // 闭包超阈值：改 b 的闭包是 {a,b}（阈值 2 内），改 c 的闭包是 {a,b,c}（阈值 2 超）
    let mut new4 = old.clone();
    new4[2].text = "let c = b + 9;".to_string();
    let p_big = plan_incr(&old, &new4, 2, IncrMode::Conservative);
    s.add("回退-闭包超阈整编", p_big == IncrPlan::WholeUnit, "thresh");
    // 族五：收益账本诚实登记
    let mut led = BenefitLedger::new(100);
    s.add("账本-初值无收益", led.rate() == 0 && led.records == 0, "init");
    led.record(40);
    s.add("账本-登记生效", led.records == 1 && led.rate() == 600, "rec");
    led.record(100);
    s.add("账本-无收益如实为0", led.rate() == 0, "zero");
    let mut led2 = BenefitLedger::new(0);
    s.add("账本-零全量防除零", led2.rate() == 0, "div0");
    // 族六：快照失配告警
    s.add("快照-一致通过", snapshot_check("let a = 1;", "let a = 1;"), "ok");
    s.add("快照-漂移告警", !snapshot_check("let a = 1;", "let a = 2;"), "drift");
    s
}
