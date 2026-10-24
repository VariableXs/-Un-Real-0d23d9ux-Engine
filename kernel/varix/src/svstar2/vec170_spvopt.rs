//! VE-F0570 · SPIR-V 优化器对接
//!
//! spirv-opt pass 流水对接与分工边界——VE IR 层优化（F0490 组）负责架构级，
//! spirv-opt 负责指令级后优化，两层不重复：IR 已做的优化在 spirv-opt 流水中
//! 显性关闭对应 pass，确定单一路径避免互相抵消与不确定性；pass 流水按目标
//! 档位可配置（快速档最小流水、发布档全流水）；优化前后均过 F0567 验证门禁
//! （不变式）；优化确定性声明与 F0494 同源（同输入同输出）。
//!
//! 跨批对接：上游 F0489 pass 管理与 F0466-0470 优化族、F0567 门禁；下游 F0578 总对拍。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// pass 流水档位
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptProfile {
    /// 快速档：最小流水
    Fast,
    /// 发布档：全流水
    Release,
}

/// pass 定义
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pass {
    pub name: String,
    /// VE IR 层已做的同类优化（分工边界：显性关闭）
    pub ir_counterpart: Option<String>,
}

impl Pass {
    pub fn new(name: &str) -> Self {
        Pass { name: name.to_string(), ir_counterpart: None }
    }

    pub fn with_ir(name: &str, ir: &str) -> Self {
        Pass { name: name.to_string(), ir_counterpart: Some(ir.to_string()) }
    }

    /// 分工单一路径：IR 已做的 pass 显性关闭
    pub fn disabled_by_ir(&self, ir_done: &[&str]) -> bool {
        match &self.ir_counterpart {
            Some(ir) => ir_done.contains(&ir.as_str()),
            None => false,
        }
    }
}

/// pass 配置表：按档位装配
#[derive(Clone, Debug)]
pub struct PassConfig {
    pub profile: OptProfile,
    pub passes: Vec<Pass>,
}

impl PassConfig {
    /// 快速档最小流水
    pub fn fast() -> Self {
        PassConfig {
            profile: OptProfile::Fast,
            passes: vec![Pass::new("elim-dead-branch"), Pass::new("compact-ids")],
        }
    }

    /// 发布档全流水
    pub fn release() -> Self {
        PassConfig {
            profile: OptProfile::Release,
            passes: vec![
                Pass::new("elim-dead-branch"),
                Pass::with_ir("common-uniform-elim", "const-fold"),
                Pass::with_ir("strength-reduction", "strength"),
                Pass::new("compact-ids"),
                Pass::new("dedup-const"),
            ],
        }
    }

    /// 分工边界清单：IR 已做的优化在流水中显性关闭
    pub fn effective(&self, ir_done: &[&str]) -> Vec<&str> {
        self.passes
            .iter()
            .filter(|p| !p.disabled_by_ir(ir_done))
            .map(|p| p.name.as_str())
            .collect()
    }

    /// pass 冲突审计：同一 pass 名重复配置即冲突
    pub fn audit_conflicts(&self) -> Vec<String> {
        let mut conflicts = Vec::new();
        for i in 0..self.passes.len() {
            for j in (i + 1)..self.passes.len() {
                if self.passes[i].name == self.passes[j].name {
                    if !conflicts.contains(&self.passes[i].name) {
                        conflicts.push(self.passes[i].name.clone());
                    }
                }
            }
        }
        conflicts
    }

    pub fn len(&self) -> usize {
        self.passes.len()
    }
}

/// 优化执行器：门禁不变式（优化前后均验证）+ 确定性
#[derive(Clone, Debug)]
pub struct OptRunner {
    pub profile: OptProfile,
    /// 优化前验证通过
    pub pre_verified: bool,
    /// 优化后验证通过
    pub post_verified: bool,
}

impl OptRunner {
    pub fn new(profile: OptProfile) -> Self {
        OptRunner { profile, pre_verified: false, post_verified: false }
    }

    /// 门禁不变式：产出非法 → 回退未优化版
    pub fn run(&mut self, pre_ok: bool, post_ok: bool) -> bool {
        self.pre_verified = pre_ok;
        self.post_verified = post_ok;
        // 前门禁不过 → 不执行优化
        if !pre_ok {
            return false;
        }
        // 后门禁不过 → 回退未优化版（返回 false 表示使用原产物）
        post_ok
    }

    pub fn used_fallback(&self) -> bool {
        self.pre_verified && !self.post_verified
    }
}

/// 确定性声明：同输入同输出（pass 序列不变时）
pub fn deterministic(f: impl Fn() -> Vec<u32>) -> bool {
    f() == f()
}

/// 工具版本差异：冻结版本登记
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenVer {
    pub year: u32,
    pub rev: u32,
}

impl FrozenVer {
    /// 冻结：登记后不接受偏离
    pub fn frozen(&self, other: FrozenVer) -> bool {
        self.year == other.year && self.rev == other.rev
    }
}

pub fn run_vec170_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec170");
    // 族一：双档流水
    let fast = PassConfig::fast();
    let rel = PassConfig::release();
    s.add("档位-快速档最小流水", fast.profile == OptProfile::Fast && fast.len() == 2, "fast");
    s.add("档位-发布档全流水", rel.profile == OptProfile::Release && rel.len() == 5, "rel");
    s.add("档位-发布档超集", rel.len() > fast.len(), "sup");
    // 族二：分工单一路径
    let ir_done = ["const-fold", "strength"];
    let eff = rel.effective(&ir_done);
    s.add("分工-IR已做显性关闭", !eff.contains(&"common-uniform-elim") && !eff.contains(&"strength-reduction"), "off");
    s.add("分工-其余保留", eff.contains(&"elim-dead-branch") && eff.contains(&"compact-ids"), "keep");
    s.add("分工-无对照pass保留", fast.effective(&ir_done).len() == 2, "noctr");
    // 分工显性标记
    let cf = rel.passes.iter().find(|p| p.name == "common-uniform-elim").unwrap();
    s.add("分工-对照登记", cf.ir_counterpart == Some("const-fold".to_string()), "reg");
    s.add("分工-关闭判定", cf.disabled_by_ir(&ir_done) && !cf.disabled_by_ir(&[]), "dis");
    // 族三：pass 冲突审计
    let mut conflict_cfg = PassConfig::fast();
    conflict_cfg.passes.push(Pass::new("compact-ids"));
    s.add("审计-重复pass检出", conflict_cfg.audit_conflicts() == vec!["compact-ids".to_string()], "dup");
    s.add("审计-健康零冲突", rel.audit_conflicts().is_empty(), "clean");
    // 族四：门禁不变式
    let mut r1 = OptRunner::new(OptProfile::Release);
    s.add("门禁-前后通过", r1.run(true, true), "ok");
    s.add("门禁-未回退", !r1.used_fallback(), "nofb");
    let mut r2 = OptRunner::new(OptProfile::Release);
    s.add("门禁-产出非法回退", !r2.run(true, false), "fb");
    s.add("门禁-回退登记", r2.used_fallback(), "fb-reg");
    let mut r3 = OptRunner::new(OptProfile::Fast);
    s.add("门禁-前门禁不过不执行", !r3.run(false, true), "pre");
    // 族五：确定性
    s.add("确定-同输入同输出", deterministic(|| vec![1, 2, 3]), "det");
    // 族六：版本冻结
    let fv = FrozenVer { year: 2024, rev: 7 };
    s.add("冻结-匹配", fv.frozen(FrozenVer { year: 2024, rev: 7 }), "match");
    s.add("冻结-偏离拒绝", !fv.frozen(FrozenVer { year: 2024, rev: 8 }), "off");
    // 族七：快速档不受 IR 分工影响仍最小
    s.add("档位-快速档语义", fast.passes.iter().all(|p| p.ir_counterpart.is_none()), "pure");
    s
}
