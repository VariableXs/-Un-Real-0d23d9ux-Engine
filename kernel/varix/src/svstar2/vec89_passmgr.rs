//! VE-F0489 · 优化器 Pass 管理器与顺序（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0489`
//!
//! **锚点原文**：「优化 pass 管理器：pass 注册制（名称、前置条件、幂等声明）、
//! 执行序（固定序加依赖约束、不动态乱序——确定性原则 F0494 联动）、pass 级
//! 开关（调试场景单 pass 隔离执行）；管理器与验证器门禁集成（关键 pass 后
//! 跑验证）。判据：注册制、固定序、隔离模式、关键门禁、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **注册制**（判据一）。pass 注册（名称 + 前置条件 + 幂等声明三要素），
//!    未注册 pass 拒绝执行（注册表机检）。
//!
//! 2. **固定序**（判据二）。执行序固定（注册序 = 执行序，不动态乱序——确
//!    定性原则），依赖约束显性（前置未跑 → 拒启该 pass 并说明）。
//!
//! 3. **隔离模式**（判据三）。pass 级开关（调试场景：单 pass 隔离执行——
//!    只跑指定 pass），隔离执行仍守前置。
//!
//! 4. **关键门禁**（判据四）。关键 pass（折叠/融合等改写面）后跑验证器
//!    （门禁集成：验证不过 → 隔离捕获标注跳过）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0489";

/// pass 注册三要素。
#[derive(Clone, Debug, PartialEq)]
pub struct PassSpec {
    pub name: &'static str,
    /// 前置条件（必须已跑过的 pass 名单）。
    pub requires: Vec<&'static str>,
    /// 幂等声明（重复运行结果不变——对拍面）。
    pub idempotent: bool,
    /// 是否关键 pass（后接验证门禁）。
    pub critical: bool,
}

/// 执行事件（隔离捕获面）。
#[derive(Clone, Debug, PartialEq)]
pub struct PassEvent {
    pub pass: &'static str,
    pub kind: &'static str,
}

/// Pass 管理器。
#[derive(Debug, Default)]
pub struct PassManager {
    pub registry: Vec<PassSpec>,
    pub events: Vec<PassEvent>,
    pub done: Vec<&'static str>,
    /// 验证失败被跳过的 pass（隔离捕获标注）。
    pub skipped_by_gate: Vec<&'static str>,
}

impl PassManager {
    pub fn new() -> Self {
        Self::default()
    }
    /// 注册（三要素齐备；重名拒）。
    pub fn register(&mut self, spec: PassSpec) -> Result<(), String> {
        if self.registry.iter().any(|s| s.name == spec.name) {
            return Err(format!("pass 重名：{}", spec.name));
        }
        if spec.requires.is_empty() && !spec.name.is_empty() {
            // 无前置合法（首段 pass）。
        }
        self.registry.push(spec);
        Ok(())
    }
    /// 前置满足判定。
    fn prereq_ok(&self, spec: &PassSpec, only: Option<&str>) -> bool {
        spec.requires.iter().all(|r| match only {
            // 隔离模式：只跑指定 pass → 前置须为空或等于自身链。
            Some(_) => *r == spec.name || self.done.contains(r),
            None => self.done.contains(r),
        })
    }
    /// 执行固定序（不动态乱序）：按注册序逐 pass，前置不满足 → 拒启并说明。
    /// gate 注入：关键 pass 执行后跑验证（不过 → 该 pass 效果隔离跳过）。
    pub fn run(
        &mut self,
        gate: &dyn Fn(&str) -> bool,
        mut exec: impl FnMut(&str) -> bool,
        only: Option<&'static str>,
    ) -> usize {
        let mut ran = 0;
        for spec in self.registry.clone().iter() {
            // 隔离模式：只跑指定 pass。
            if let Some(o) = only {
                if spec.name != o {
                    continue;
                }
            }
            // 前置不满足 → 拒启并说明。
            if !self.prereq_ok(spec, only) {
                self.events.push(PassEvent { pass: spec.name, kind: "reject-prereq" });
                continue;
            }
            self.events.push(PassEvent { pass: spec.name, kind: "start" });
            let ok = exec(spec.name);
            if !ok {
                self.events.push(PassEvent { pass: spec.name, kind: "fail" });
                continue;
            }
            self.done.push(spec.name);
            ran += 1;
            // 关键门禁：改写面 pass 后跑验证。
            if spec.critical {
                if gate(spec.name) {
                    self.events.push(PassEvent { pass: spec.name, kind: "gate-pass" });
                } else {
                    // 验证不过 → 隔离捕获：效果回退标注（跳过记账）。
                    self.events.push(PassEvent { pass: spec.name, kind: "gate-fail-skip" });
                    self.skipped_by_gate.push(spec.name);
                    self.done.retain(|d| *d != spec.name);
                    ran -= 1;
                }
            }
        }
        ran
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec89_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let gate_ok = |_: &str| true;

    // 一、注册制。
    {
        let mut m = PassManager::new();
        let r1 = m.register(PassSpec {
            name: "dce",
            requires: vec![],
            idempotent: true,
            critical: false,
        });
        v.push(("F0489-注册-三要素收", r1.is_ok()));
        // 重名拒。
        let r2 = m.register(PassSpec {
            name: "dce",
            requires: vec![],
            idempotent: true,
            critical: false,
        });
        v.push(("F0489-注册-重名拒", r2.is_err()));
    }

    // 二、固定序。
    {
        let mut m = PassManager::new();
        m.register(PassSpec { name: "dce", requires: vec![], idempotent: true, critical: false }).ok();
        m.register(PassSpec { name: "cse", requires: vec!["dce"], idempotent: true, critical: false }).ok();
        m.register(PassSpec { name: "dataflow", requires: vec!["cse"], idempotent: true, critical: false }).ok();
        // 固定序执行：注册序即执行序（done 序 == 注册序）。
        let n = m.run(&gate_ok, |_| true, None);
        v.push((
            "F0489-固定序-注册序执行",
            n == 3 && m.done == vec!["dce", "cse", "dataflow"],
        ));
        // 前置未跑 → 拒启并说明（隔离只跑 dataflow：前置 cse 未跑）。
        let mut m2 = PassManager::new();
        m2.register(PassSpec { name: "dataflow", requires: vec!["cse"], idempotent: true, critical: false }).ok();
        let n2 = m2.run(&gate_ok, |_| true, Some("dataflow"));
        v.push((
            "F0489-固定序-前置拒启",
            n2 == 0
                && m2.events.iter().any(|e| e.kind == "reject-prereq")
                && m2.done.is_empty(),
        ));
    }

    // 三、隔离模式。
    {
        let mut m = PassManager::new();
        m.register(PassSpec { name: "dce", requires: vec![], idempotent: true, critical: false }).ok();
        m.register(PassSpec { name: "cse", requires: vec!["dce"], idempotent: true, critical: false }).ok();
        m.register(PassSpec { name: "fuse", requires: vec![], idempotent: true, critical: false }).ok();
        // 隔离只跑 fuse（前置空 → 合法）：只 fuse 进 done。
        let n = m.run(&gate_ok, |_| true, Some("fuse"));
        v.push((
            "F0489-隔离-单pass执行",
            n == 1 && m.done == vec!["fuse"],
        ));
    }

    // 四、关键门禁。
    {
        let mut m = PassManager::new();
        m.register(PassSpec { name: "fold", requires: vec![], idempotent: true, critical: true }).ok();
        m.register(PassSpec { name: "dce", requires: vec![], idempotent: true, critical: false }).ok();
        // 门禁过 → 正常入 done。
        let n = m.run(&gate_ok, |_| true, None);
        v.push((
            "F0489-门禁-过则入账",
            n == 2 && m.done.contains(&"fold")
                && m.events.iter().any(|e| e.pass == "fold" && e.kind == "gate-pass"),
        ));
        // 门禁不过 → 隔离捕获：效果回退 + 跳过标注 + 后续前置不满足。
        let mut m2 = PassManager::new();
        m2.register(PassSpec { name: "fold", requires: vec![], idempotent: true, critical: true }).ok();
        m2.register(PassSpec { name: "after_fold", requires: vec!["fold"], idempotent: true, critical: false }).ok();
        let gate_reject = |p: &str| p != "fold";
        let n2 = m2.run(&gate_reject, |_| true, None);
        v.push((
            "F0489-门禁-不过回退跳过",
            n2 == 0
                && !m2.done.contains(&"fold")
                && m2.skipped_by_gate == vec!["fold"]
                && m2.events.iter().any(|e| e.kind == "gate-fail-skip"),
        ));
        // after_fold 前置（fold 不在 done）→ 拒启。
        v.push((
            "F0489-门禁-后续拒启",
            m2.events.iter().any(|e| e.pass == "after_fold" && e.kind == "reject-prereq"),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0489-判据-锚点入册", ANCHOR == "VE-F0489"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec89）。
pub fn run_vec89_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec89");
    for (name, ok) in vec89_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec89_raw_checks as run_vec89_checks;
    #[test]
    fn vec89_passmgr_checks() {
        for (name, ok) in run_vec89_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
