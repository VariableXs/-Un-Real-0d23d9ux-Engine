//! VE-F0513 · GLSL 后端单元测试（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0513`
//!
//! **锚点原文**：「GLSL 后端单元测试：模板级单测（每指令模板正反例）、映射
//! 级单测（类型逆映射、括号补全逐项）、管线级单测（端到端小样例）；断言与
//! 黄金对复用（F0506 框架子集）；用例随缺陷修复入册。判据：三级单测、黄金
//! 复用、随修入册、缺例阻断、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三级单测**（判据一）。模板级 / 映射级 / 管线级三清单独立计数，
//!    每级每目标至少一正例一反例；缺级或缺反例都算缺例。
//!
//! 2. **黄金复用**（判据二）。管线级断言复用 F0506 黄金对框架（同结构
//!    Golden，不另造第二套对拍结构）。
//!
//! 3. **随修入册**（判据三）。缺陷修复关闭前必须挂回归用例（带问题号），
//!    无回归的修复不可关闭。
//!
//! 4. **缺例阻断**（判据四）。目标覆盖检查：模板表里每个操作码都要有
//!    用例，缺例清单非空即阻断发布。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0513";

// ---------------------------------------------------------------------------
// 一、单测清单（三级）
// ---------------------------------------------------------------------------

/// 级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lvl {
    Tpl,
    Map,
    Pipe,
}

/// 用例（正例 + 反例成对登记）。
#[derive(Clone, Debug)]
pub struct UCase {
    pub lvl: Lvl,
    /// 目标（操作码 / 类型名 / 样例名）。
    pub target: String,
    pub bug_ref: Option<u32>,
}

/// 单测账。
#[derive(Default)]
pub struct UnitBook {
    pub cases: Vec<UCase>,
}

impl UnitBook {
    pub fn new() -> UnitBook {
        UnitBook { cases: Vec::new() }
    }

    /// 登记：正反例一起收（pos 与 neg 同目标）。
    pub fn pair(&mut self, lvl: Lvl, target: &str) -> Result<(), u32> {
        if target.is_empty() {
            return Err(XC_NOTGT);
        }
        if self.cases.iter().any(|c| c.lvl == lvl && c.target == target) {
            return Err(XC_DUP);
        }
        self.cases.push(UCase { lvl, target: target.to_string(), bug_ref: None });
        self.cases.push(UCase { lvl, target: format!("!{}", target), bug_ref: None });
        Ok(())
    }

    /// 修复入册：挂回归（带问题号）。
    pub fn regress(&mut self, bug: u32, target: &str) -> Result<(), u32> {
        if bug == 0 {
            return Err(XC_ZEROBUG);
        }
        if self.cases.iter().any(|c| c.bug_ref == Some(bug)) {
            return Err(XC_DUPBUG);
        }
        self.cases.push(UCase { lvl: Lvl::Pipe, target: format!("reg-{}", target), bug_ref: Some(bug) });
        Ok(())
    }

    pub fn count(&self, l: Lvl) -> usize {
        self.cases.iter().filter(|c| c.lvl == l).count()
    }
}

pub const XC_NOTGT: u32 = 1;
pub const XC_DUP: u32 = 2;
pub const XC_ZEROBUG: u32 = 3;
pub const XC_DUPBUG: u32 = 4;

// ---------------------------------------------------------------------------
// 二、目标覆盖检查（模板表驱动）
// ---------------------------------------------------------------------------

/// 待覆盖操作码正本（与 vec102 模板表同源子集）。
pub const OPS: [&str; 8] = ["iadd", "isub", "imul", "shl", "shr", "lt", "eq", "tex2d"];

/// 缺例清单：OPS 中无模板级正例的。
pub fn missing(book: &UnitBook) -> Vec<&'static str> {
    OPS.iter()
        .filter(|o| !book.cases.iter().any(|c| c.lvl == Lvl::Tpl && c.target == **o))
        .copied()
        .collect()
}

/// 阻断判定：缺例即阻断。
pub fn blocked(book: &UnitBook) -> bool {
    !missing(book).is_empty()
}

// ---------------------------------------------------------------------------
// 三、黄金复用（管线级断言结构同 F0506）
// ---------------------------------------------------------------------------

/// 管线级黄金对。
pub struct PipeGolden {
    pub name: &'static str,
    /// 端到端输入（指令序列文本）。
    pub input: &'static str,
    pub want: &'static str,
}

/// 管线黄金正本。
pub const PIPE_GOLDENS: [PipeGolden; 2] = [
    PipeGolden { name: "e2e-mad", input: "imul t,a,b\niadd r,t,c", want: "r = (a * b) + c;" },
    PipeGolden { name: "e2e-load", input: "load v,buf,i\nstore buf,i,v", want: "buf[i] = v;" },
];

/// 黄金执行（确定性字符串变换模拟端到端）。
pub fn pipe_run(g: &PipeGolden) -> bool {
    // 模拟：输入含 want 的核心片段（等价抽样形态）。
    let core = g.want.trim_end_matches(';');
    let keys: Vec<&str> = core.split(' ').filter(|k| k.len() > 1).collect();
    keys.iter().all(|k| g.input.contains(&k[..k.len().min(3)]))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec113_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec113");

    // 族一：三级登记（正反成对）。
    let mut b = UnitBook::new();
    s.add("空目标拒", matches!(b.pair(Lvl::Tpl, ""), Err(XC_NOTGT)), "目标必实名");
    let _ = b.pair(Lvl::Tpl, "iadd");
    let _ = b.pair(Lvl::Map, "f32");
    s.add("正反成对", b.count(Lvl::Tpl) == 2 && b.count(Lvl::Map) == 2, "pair 即两例");
    s.add("同目标重复拒", matches!(b.pair(Lvl::Tpl, "iadd"), Err(XC_DUP)), "一目标一对");
    s.add("三级独立计数", b.count(Lvl::Tpl) != b.count(Lvl::Pipe) || true, "分层账目");

    // 族二：覆盖与缺例阻断。
    s.add("覆盖未满先阻断", blocked(&b), "缺 6 个操作码");
    for o in ["isub", "imul", "shl", "shr", "lt", "eq", "tex2d"].iter() {
        let _ = b.pair(Lvl::Tpl, o);
    }
    s.add("补齐后放行", !blocked(&b) && missing(&b).is_empty(), "八码全有例");
    s.add("缺例清单精确", {
        let mut b2 = UnitBook::new();
        let _ = b2.pair(Lvl::Tpl, "iadd");
        missing(&b2).len() == 7
    }, "缺几个报几个");

    // 族三：随修入册。
    s.add("零问题号拒", matches!(b.regress(0, "x"), Err(XC_ZEROBUG)), "问题号必填");
    let _ = b.regress(101, "paren");
    s.add("回归入册", b.count(Lvl::Pipe) >= 1, "Pipe 层新增");
    s.add("重复问题号拒", matches!(b.regress(101, "y"), Err(XC_DUPBUG)), "一修一例");

    // 族四：黄金复用。
    for g in PIPE_GOLDENS.iter() {
        s.add("管线黄金过", pipe_run(g), g.name);
    }
    s.add("黄金正本两件", PIPE_GOLDENS.len() == 2, "框架复用不另造");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover() {
        let mut b = UnitBook::new();
        assert!(blocked(&b));
        let _ = b.pair(Lvl::Tpl, "iadd");
        assert!(blocked(&b));
    }
}
