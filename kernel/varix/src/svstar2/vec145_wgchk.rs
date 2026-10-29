//! VE-F0545 · WGSL 强校验适应（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0545`
//!
//! **锚点原文**：「WGSL 强校验适应：WGSL 规范强校验项逐类适应（类型严格、
//! 初始化必达、地址空间规则三类校验的生成侧预防——生成即合规而非生成后修）、
//! 预防规则登记表、校验失败回传处理（F0531 机制复用）。判据：生成即合规、
//! 三类校验、预防登记、失败回溯、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **生成即合规**（判据一）。三类强校验在生成侧预防（未初始化变量自动
//!    补零值、混型自动转、空间误用生成前拦截），预防先于校验。
//!
//! 2. **三类校验**（判据二）。类型严格 / 初始化必达 / 地址空间规则三类
//!    逐类有预防规则。
//!
//! 3. **预防登记**（判据三）。预防规则表单源登记（规则名 × 触发条件 ×
//!    预防动作），规则缺失即校验失败缺口。
//!
//! 4. **失败回溯**（判据四）。生成后校验失败回溯到预防规则缺口（哪条规则
//!    没拦住），缺口登记显性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0545";

// ---------------------------------------------------------------------------
// 一、三类校验
// ---------------------------------------------------------------------------

/// 校验类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chk {
    TypeStrict,
    InitMust,
    SpaceRule,
}

/// 预防规则。
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub chk: Chk,
    pub name: &'static str,
    /// 触发条件描述。
    pub cond: &'static str,
    /// 预防动作。
    pub act: &'static str,
}

/// 预防规则正本表（三类各至少一）。
pub const RULES: [Rule; 4] = [
    Rule { chk: Chk::TypeStrict, name: "混型转换", cond: "操作数含 i32/f32 混合", act: "插入 f32()/i32() 显性转换" },
    Rule { chk: Chk::InitMust, name: "零值初始化", cond: "let/var 声明无初值", act: "自动补零值（0.0/0u/vec4f()）" },
    Rule { chk: Chk::SpaceRule, name: "只读误写拦截", cond: "uniform 空间出现写操作", act: "生成前拒绝并提示 storage" },
    Rule { chk: Chk::SpaceRule, name: "workgroup 作用域", cond: "workgroup 变量在函数内声明", act: "提升到模块级" },
];

/// 规则覆盖：三类全有规则。
pub fn rules_covered() -> bool {
    [Chk::TypeStrict, Chk::InitMust, Chk::SpaceRule]
        .iter()
        .all(|c| RULES.iter().any(|r| r.chk == *c))
}

// ---------------------------------------------------------------------------
// 二、生成侧预防引擎
// ---------------------------------------------------------------------------

/// 预防结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prevent {
    /// 命中的规则名。
    pub rule: &'static str,
    /// 预防后的文本。
    pub text: String,
}

/// 未初始化声明预防：自动补零值。
pub fn prevent_init(ty: &str, name: &str, has_init: bool) -> Option<Prevent> {
    if has_init {
        return None;
    }
    let zero = match ty {
        "f32" => "0.0",
        "i32" | "u32" => "0",
        "bool" => "false",
        "vec4<f32>" => "vec4<f32>()",
        _ => return None,
    };
    Some(Prevent { rule: "零值初始化", text: format!("var {} = {};", name, zero) })
}

/// uniform 写操作预防：生成前拦截。
pub fn prevent_uniform_write(space: &str, is_write: bool) -> Option<Prevent> {
    if space == "uniform" && is_write {
        Some(Prevent { rule: "只读误写拦截", text: "ERR: uniform 不可写，应改 storage".to_string() })
    } else {
        None
    }
}

/// workgroup 作用域预防。
pub fn prevent_workgroup(scope: &str, ty_wg: bool) -> Option<Prevent> {
    if ty_wg && scope == "fn" {
        Some(Prevent { rule: "workgroup 作用域", text: "HOIST: 提升到模块级".to_string() })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 三、失败回溯
// ---------------------------------------------------------------------------

/// 校验失败记录 → 缺口规则名。
#[derive(Default)]
pub struct FailLog {
    pub gaps: Vec<String>,
}

impl FailLog {
    pub fn new() -> FailLog {
        FailLog { gaps: Vec::new() }
    }

    /// 回溯：失败类别 → 应有规则 → 缺口登记。
    pub fn trace(&mut self, chk: Chk, failed_rule: &str) {
        let should = RULES.iter().any(|r| r.chk == chk && r.name == failed_rule);
        if !should {
            self.gaps.push(format!("{} 无预防规则", failed_rule));
        }
    }

    pub fn len(&self) -> usize {
        self.gaps.len()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec145_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec145");

    // 族一：三类覆盖。
    s.add("三类全有规则", rules_covered(), "类型/初始化/空间");
    s.add("规则四件", RULES.len() == 4, "正本规模");
    s.add("规则带动作", RULES.iter().all(|r| !r.act.is_empty() && !r.cond.is_empty()), "登记完整");

    // 族二：初始化预防。
    let p1 = prevent_init("f32", "x", false);
    s.add("补零值", p1.as_ref().map(|p| p.text == "var x = 0.0;") == Some(true), "生成即合规");
    s.add("向量零值", prevent_init("vec4<f32>", "v", false).map(|p| p.text.contains("vec4<f32>()")) == Some(true), "构造形态");
    s.add("有初值不干预", prevent_init("f32", "x", true).is_none(), "不冗余");
    s.add("未知类型不猜", prevent_init("f64", "x", false).is_none(), "显性缺口");

    // 族三：空间预防。
    let p2 = prevent_uniform_write("uniform", true);
    s.add("uniform 误写拦截", p2.as_ref().map(|p| p.text.contains("storage")) == Some(true), "生成前拒");
    s.add("读不拦", prevent_uniform_write("uniform", false).is_none(), "只拦写");
    s.add("storage 写放行", prevent_uniform_write("storage", true).is_none(), "可写空间");
    let p3 = prevent_workgroup("fn", true);
    s.add("workgroup 提升", p3.is_some(), "作用域预防");
    s.add("模块级不拦", prevent_workgroup("module", true).is_none(), "合规不干预");

    // 族四：失败回溯。
    let mut fl = FailLog::new();
    fl.trace(Chk::TypeStrict, "混型转换"); // 规则在册 → 非缺口
    fl.trace(Chk::InitMust, "魔法初始化"); // 规则不在册 → 缺口
    s.add("缺口可见", fl.len() == 1, "一缺口");
    s.add("缺口带原因", fl.gaps[0].contains("魔法初始化"), "哪条没拦住");
    s.add("在册规则不进缺口", fl.trace(Chk::SpaceRule, "只读误写拦截") == () && fl.len() == 1, "登记一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevent() {
        let p = prevent_init("i32", "n", false).unwrap();
        assert_eq!(p.text, "var n = 0;");
        assert!(rules_covered());
    }
}
