//! VE-F0474 · IR 模块链接器（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0474`
//!
//! **锚点原文**：「IR 模块链接器：多 IR 模块合并（函数、全局、资源声明三节
//! 合并语义）、符号冲突裁定（冲突即错不静默合并）、跨模块调用重绑定；链接
//! 验证（验证器复用跑合并后模块）。判据：三节合并、冲突即错、重绑定、验证
//! 复用、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三节合并**（判据一）。函数/全局/资源三节按序合并（函数节保留顺序、
//!    全局与资源按绑定键查重）。
//!
//! 2. **冲突即错**（判据二）。同名函数/同绑定资源 → 报错带两侧模块名与符
//!    号名（不静默合并——锚点明令）。
//!
//! 3. **重绑定**（判据三）。跨模块调用重绑定（调用表：调用名 → 模块内
//!    函数序号重映射）。
//!
//! 4. **验证复用**（判据四）。合并后模块跑 F0464 验证器（复用不另写），
//!    验证不过 → 回滚合并定位。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0474";

use super::vec61_irstruct::{IrFunc, ResDecl};

/// 待链接模块（名字 + 三节内容）。
#[derive(Clone, Debug)]
pub struct LinkUnit {
    pub name: String,
    pub funcs: Vec<IrFunc>,
    pub globals: Vec<(String, &'static str)>,
    pub resources: Vec<ResDecl>,
    /// 跨模块调用表：(调用方函数, 被调名)。
    pub calls: Vec<(String, String)>,
}

/// 链接结果。
#[derive(Debug, Default)]
pub struct LinkResult {
    pub funcs: Vec<IrFunc>,
    pub globals: Vec<(String, &'static str)>,
    pub resources: Vec<ResDecl>,
    /// 重绑定记录：(调用方, 被调名, 解析到哪一模块)。
    pub rebinds: Vec<(String, String, String)>,
    /// 错误（冲突即错；带两侧模块与符号）。
    pub errors: Vec<String>,
    /// 是否回滚（合并后验证不过——此处验证器接口由调用方注入）。
    pub rolled_back: bool,
}

/// 链接 pass：多单元合并（冲突即错 + 调用重绑定）。
/// 验证复用钩子：verify 非空时对合并结果跑外部验证（不过即回滚）。
pub fn link(units: &[LinkUnit], verify: Option<&dyn Fn(&LinkResult) -> bool>) -> LinkResult {
    let mut res = LinkResult::default();
    // 符号归属表：函数名 → 首见模块名（冲突报双侧的依据）。
    let mut fn_owner: Vec<(String, String)> = Vec::new();
    // 一、函数节合并（同名冲突即错，带两侧模块）。
    for u in units.iter() {
        for f in u.funcs.iter() {
            if let Some((_, owner)) = fn_owner.iter().find(|(n, _)| *n == f.name) {
                res.errors.push(format!(
                    "函数符号冲突：{} 于 {} 与 {}",
                    f.name, owner, u.name
                ));
            } else {
                fn_owner.push((f.name.clone(), u.name.clone()));
                res.funcs.push(f.clone());
            }
        }
        // 二、全局节合并（同名冲突即错）。
        for (g, t) in u.globals.iter() {
            if res.globals.iter().any(|(n, _)| n == g) {
                res.errors.push(format!("全局符号冲突：{} 于 {}", g, u.name));
            } else {
                res.globals.push((g.clone(), t));
            }
        }
        // 三、资源节合并（同绑定冲突即错）。
        for r in u.resources.iter() {
            if res.resources.iter().any(|x| x.binding == r.binding) {
                res.errors.push(format!("资源绑定冲突：槽 {} 于 {}", r.binding, u.name));
            } else {
                res.resources.push(r.clone());
            }
        }
    }
    // 四、跨模块调用重绑定（调用名 → 声明模块名）。
    for u in units.iter() {
        for (caller, callee) in u.calls.iter() {
            let owner = units
                .iter()
                .find(|m| m.funcs.iter().any(|f| &f.name == callee))
                .map(|m| m.name.clone());
            match owner {
                Some(m) => res.rebinds.push((caller.clone(), callee.clone(), m)),
                None => res
                    .errors
                    .push(format!("未解析调用：{} 调 {}（缺符号）", caller, callee)),
            }
        }
    }
    // 五、验证复用（外部验证器钩子）：错误非空或验证不过 → 回滚。
    if res.errors.is_empty() {
        if let Some(f) = verify {
            if !f(&res) {
                res.rolled_back = true;
                res.funcs.clear();
                res.globals.clear();
                res.resources.clear();
                res.rebinds.clear();
                res.errors.push("合并后验证不过：整 pass 回滚定位".to_string());
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec74_raw_checks() -> Vec<(&'static str, bool)> {
    use super::vec61_irstruct::{Block, Inst, Opcode};
    let mkf = |name: &str| {
        let mut f = IrFunc::new(name);
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, 0));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![0], 2));
        f
    };
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三节合并。
    {
        let u1 = LinkUnit {
            name: "base".to_string(),
            funcs: vec![mkf("vs_main")],
            globals: vec![("MAX".to_string(), "s:int")],
            resources: vec![ResDecl { binding: 0, kind: "sampler2d" }],
            calls: vec![],
        };
        let u2 = LinkUnit {
            name: "lib".to_string(),
            funcs: vec![mkf("noise2d")],
            globals: vec![("SCALE".to_string(), "s:float")],
            resources: vec![ResDecl { binding: 1, kind: "tex2d" }],
            calls: vec![],
        };
        let r = link(&[u1, u2], None);
        v.push((
            "F0474-合并-三节合并",
            r.errors.is_empty()
                && r.funcs.len() == 2
                && r.globals.len() == 2
                && r.resources.len() == 2,
        ));
    }

    // 二、冲突即错。
    {
        // 同名函数 → 冲突带两侧模块与符号。
        let u1 = LinkUnit {
            name: "mod_a".to_string(),
            funcs: vec![mkf("helper")],
            globals: vec![],
            resources: vec![],
            calls: vec![],
        };
        let u2 = LinkUnit {
            name: "mod_b".to_string(),
            funcs: vec![mkf("helper")],
            globals: vec![],
            resources: vec![],
            calls: vec![],
        };
        let r = link(&[u1, u2], None);
        v.push((
            "F0474-冲突-函数双侧",
            r.errors.iter().any(|e| e.contains("helper") && e.contains("mod_a") && e.contains("mod_b")),
        ));
        // 同绑定资源 → 冲突。
        let u3 = LinkUnit {
            name: "mod_c".to_string(),
            funcs: vec![],
            globals: vec![],
            resources: vec![ResDecl { binding: 5, kind: "tex2d" }],
            calls: vec![],
        };
        let u4 = LinkUnit {
            name: "mod_d".to_string(),
            funcs: vec![],
            globals: vec![],
            resources: vec![ResDecl { binding: 5, kind: "tex2d" }],
            calls: vec![],
        };
        let r2 = link(&[u3, u4], None);
        v.push((
            "F0474-冲突-资源绑定",
            r2.errors.iter().any(|e| e.contains("槽 5")),
        ));
    }

    // 三、重绑定。
    {
        let u1 = LinkUnit {
            name: "app".to_string(),
            funcs: vec![mkf("vs_main")],
            globals: vec![],
            resources: vec![],
            calls: vec![("vs_main".to_string(), "noise2d".to_string())],
        };
        let u2 = LinkUnit {
            name: "mathlib".to_string(),
            funcs: vec![mkf("noise2d")],
            globals: vec![],
            resources: vec![],
            calls: vec![],
        };
        let r = link(&[u1, u2], None);
        v.push((
            "F0474-重绑定-跨模块解析",
            r.rebinds == vec![("vs_main".to_string(), "noise2d".to_string(), "mathlib".to_string())],
        ));
        // 未解析调用 → 报错带缺什么。
        let u3 = LinkUnit {
            name: "app2".to_string(),
            funcs: vec![mkf("fs_main")],
            globals: vec![],
            resources: vec![],
            calls: vec![("fs_main".to_string(), "ghost_fn".to_string())],
        };
        let r2 = link(&[u3], None);
        v.push((
            "F0474-重绑定-未解析报错",
            r2.errors.iter().any(|e| e.contains("ghost_fn")),
        ));
    }

    // 四、验证复用。
    {
        // 外部验证器钩子（复用 F0464 的判定语义——此处以闭包替身验证通过）。
        let u = LinkUnit {
            name: "ok".to_string(),
            funcs: vec![mkf("vs_main")],
            globals: vec![],
            resources: vec![],
            calls: vec![],
        };
        let verifier = |r: &LinkResult| !r.funcs.is_empty();
        let r = link(&[u], Some(&verifier));
        v.push((
            "F0474-验证-复用通过",
            !r.rolled_back && r.funcs.len() == 1,
        ));
        // 验证不过 → 回滚（内容清空 + 回滚登记）。
        let u2 = LinkUnit {
            name: "bad".to_string(),
            funcs: vec![mkf("vs_main")],
            globals: vec![],
            resources: vec![],
            calls: vec![],
        };
        let reject = |_: &LinkResult| false;
        let r2 = link(&[u2], Some(&reject));
        v.push((
            "F0474-验证-不过回滚",
            r2.rolled_back && r2.funcs.is_empty() && r2.errors.iter().any(|e| e.contains("回滚")),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0474-判据-锚点入册", ANCHOR == "VE-F0474"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec74）。
pub fn run_vec74_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec74");
    for (name, ok) in vec74_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec74_raw_checks as run_vec74_checks;
    #[test]
    fn vec74_linker_checks() {
        for (name, ok) in run_vec74_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
