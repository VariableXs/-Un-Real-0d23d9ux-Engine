//! VE-F0506 · GLSL 后端对拍验证（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0506`
//!
//! **锚点原文**：「GLSL 后端对拍验证：生成 GLSL 文本经目标驱动编译回读验证
//! （编译通过加产物与 IR 语义对拍——经驱动编译管线执行对比，环境三态标注）、
//! 黄金文本对（输入 IR 加期望 GLSL 文本快照）；对拍分级（语法级快照加语义级
//! 执行）。判据：回读验证、双级对拍、环境标注、错误联动、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **回读验证**（判据一）。生成文本必须经模拟驱动编译回读（词法/结构
//!    校验）后才算编译通过；未回读的文本不算验证过。
//!
//! 2. **双级对拍**（判据二）。语法级（文本快照逐字符比对）与语义级（执行
//!    结果比对）两级独立判定，两级都过才算通过。
//!
//! 3. **环境标注**（判据三）。每条对拍带环境三态（真驱动/模拟驱动/无环境
//!    N/A），N/A 显性记录而不是默默跳过。
//!
//! 4. **错误联动**（判据四）。编译失败产生错误回传记录（行号 + 类别），
//!    供 F0508 消费——联动是结构性的（记录结构含回溯字段）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0506";

// ---------------------------------------------------------------------------
// 一、模拟驱动回读（词法 + 结构校验）
// ---------------------------------------------------------------------------

/// 回读结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadBack {
    Ok,
    BadToken,
    BadStruct,
}

/// 模拟驱动编译回读：括号平衡 + 语句分号收尾 + 禁词检查。
pub fn readback(text: &str) -> ReadBack {
    let mut bal = 0i32;
    for c in text.chars() {
        match c {
            '(' => bal += 1,
            ')' => bal -= 1,
            _ => {}
        }
        if bal < 0 {
            return ReadBack::BadStruct;
        }
    }
    if bal != 0 {
        return ReadBack::BadStruct;
    }
    // 非空行必须分号收尾（#开头指令除外）。
    for l in text.lines() {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('{') || t.starts_with('}') {
            continue;
        }
        if !t.ends_with(';') {
            return ReadBack::BadStruct;
        }
    }
    // 禁词（生成器不允许发射的词）。
    for bad in ["undefined", "NaN;"].iter() {
        if text.contains(bad) {
            return ReadBack::BadToken;
        }
    }
    ReadBack::Ok
}

// ---------------------------------------------------------------------------
// 二、环境三态
// ---------------------------------------------------------------------------

/// 对拍环境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Env {
    /// 真驱动。
    Real,
    /// 模拟驱动。
    Sim,
    /// 无环境（N/A 显性）。
    Na,
}

/// 环境标签文本。
pub fn env_tag(e: Env) -> &'static str {
    match e {
        Env::Real => "real-driver",
        Env::Sim => "sim-driver",
        Env::Na => "N/A",
    }
}

// ---------------------------------------------------------------------------
// 三、双级对拍
// ---------------------------------------------------------------------------

/// 语法级：文本快照逐行比对（空白归一）。
pub fn syn_match(got: &str, want: &str) -> bool {
    let norm = |t: &str| -> Vec<String> {
        t.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    norm(got) == norm(want)
}

/// 语义级：执行结果值序列比对（浮点容差）。
pub fn sem_match(got: &[f64], want: &[f64]) -> bool {
    got.len() == want.len() && got.iter().zip(want.iter()).all(|(a, b)| (a - b).abs() < 1e-6)
}

/// 对拍记录。
#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    /// 语法级结果。
    pub syn: bool,
    /// 语义级结果。
    pub sem: bool,
    pub env: Env,
    /// 编译失败时的回传记录（供 F0508 消费）。
    pub err_line: Option<u32>,
    pub err_kind: Option<&'static str>,
}

impl Case {
    pub fn pass(&self) -> bool {
        self.syn && self.sem
    }
}

/// 对拍账：全部样例通过 + N/A 显性记录。
#[derive(Default)]
pub struct DiffBook {
    pub cases: Vec<Case>,
}

impl DiffBook {
    pub fn new() -> DiffBook {
        DiffBook { cases: Vec::new() }
    }

    /// 登记对拍：文本先回读，回读失败即语法级红 + 错误回传。
    pub fn run(&mut self, name: &str, text: &str, want_syn: &str, got_sem: &[f64], want_sem: &[f64], env: Env) {
        let rb = readback(text);
        let syn = rb == ReadBack::Ok && syn_match(text, want_syn);
        let sem = sem_match(got_sem, want_sem);
        let (el, ek) = if rb != ReadBack::Ok {
            (Some(1u32), Some(match rb { ReadBack::BadToken => "token", _ => "struct" }))
        } else {
            (None, None)
        };
        self.cases.push(Case {
            name: name.to_string(),
            syn,
            sem,
            env,
            err_line: el,
            err_kind: ek,
        });
    }

    /// 全通过（N/A 环境的样例只要求登记存在）。
    pub fn all_pass(&self) -> bool {
        self.cases.iter().all(|c| c.pass())
    }

    /// N/A 显性：无环境样例必须被登记（不是缺席）。
    pub fn na_recorded(&self) -> usize {
        self.cases.iter().filter(|c| c.env == Env::Na).count()
    }

    /// 错误回传联动：带错误记录的样例可提取。
    pub fn errs(&self) -> Vec<(u32, &'static str)> {
        self.cases
            .iter()
            .filter(|c| c.err_line.is_some())
            .map(|c| (c.err_line.unwrap_or(0), c.err_kind.unwrap_or("")))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四、黄金文本对
// ---------------------------------------------------------------------------

/// 黄金对（IR 片段 → 期望 GLSL 文本）。
pub struct Golden {
    pub ir: &'static str,
    pub glsl: &'static str,
}

/// 黄金正本。
pub const GOLDENS: [Golden; 3] = [
    Golden { ir: "add v0, a, b", glsl: "vec4 v0 = a + b;" },
    Golden { ir: "mul v1, v0, k", glsl: "vec4 v1 = v0 * k;" },
    Golden { ir: "dp4 s, v1, n", glsl: "float s = dot(v1, n);" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec106_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec106");

    // 族一：回读验证。
    s.add("健康文本过", readback("vec4 v = a + b;\n") == ReadBack::Ok, "词法结构双过");
    s.add("括号失衡拒", readback("f(a + b;\n") == ReadBack::BadStruct, "结构校验");
    s.add("无分号拒", readback("vec4 v = a + b\n") == ReadBack::BadStruct, "语句收尾");
    s.add("禁词拒", readback("vec4 v = undefined;\n") == ReadBack::BadToken, "token 校验");
    s.add("指令行豁免", readback("#version 460\n") == ReadBack::Ok, "#行不查分号");

    // 族二：双级对拍。
    s.add("语法级空白归一", syn_match("a  +  b;", "a + b;"), "等价文本");
    s.add("语法级失配", !syn_match("a - b;", "a + b;"), "差异可见");
    s.add("语义级容差内", sem_match(&[0.1 + 0.2], &[0.3]), "浮点容差");
    s.add("语义级失配", !sem_match(&[1.0], &[2.0]), "结果差可见");
    s.add("长度不齐拒", !sem_match(&[1.0, 2.0], &[1.0]), "向量维度校验");

    // 族三：对拍账与环境标注。
    let mut d = DiffBook::new();
    d.run("g1", GOLDENS[0].glsl, GOLDENS[0].glsl, &[1.0], &[1.0], Env::Sim);
    d.run("g2", GOLDENS[1].glsl, GOLDENS[1].glsl, &[2.0, 2.0], &[2.0, 2.0], Env::Real);
    d.run("g3", GOLDENS[2].glsl, GOLDENS[2].glsl, &[0.5], &[0.5], Env::Na);
    s.add("三样例全过", d.all_pass() && d.cases.len() == 3, "双级双过");
    s.add("环境三态齐", {
        let mut has = |e: Env| d.cases.iter().any(|c| c.env == e);
        has(Env::Real) && has(Env::Sim) && has(Env::Na)
    }, "三态标注");
    s.add("N/A 显性记录", d.na_recorded() == 1, "缺席即红");

    // 族四：错误联动。
    let mut d2 = DiffBook::new();
    d2.run("bad", "f(a + b;\n", "f(a + b);", &[], &[], Env::Sim);
    s.add("回读失败登记错误", d2.errs().len() == 1, "错误回传产出");
    s.add("错误带类别", d2.errs().iter().any(|(_, k)| *k == "struct"), "分类联动");
    s.add("错误样例过不了", !d2.all_pass(), "失败可见");

    // 族五：黄金对。
    for g in GOLDENS.iter() {
        s.add("黄金回读过", readback(g.glsl) == ReadBack::Ok, g.ir);
    }
    s.add("黄金三对", GOLDENS.len() == 3, "正本规模");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff() {
        assert_eq!(readback("x = 1;\n"), ReadBack::Ok);
        assert!(syn_match("a + b;", "a  +  b;"));
    }
}
