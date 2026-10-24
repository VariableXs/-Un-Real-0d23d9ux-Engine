//! VE-F0531 · HLSL 错误回传（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0531`
//!
//! **锚点原文**：「HLSL 工具链错误回传：编译错误解析（dxc 与 fxc 两种输出
//! 格式解析器）、行号到 IR 到源码双级回溯（F0508 机制同构复用）、错误分类与
//! 怪癖登记（工具链版本怪癖单独维度）。判据：双格式解析、机制复用、工具链
//! 怪癖维、断链标注、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双格式解析**（判据一）。dxc 格式（`file:line:col: error X####`）与
//!    fxc 格式（`file(line,col): error X####`）各自解析器，行号都提取。
//!
//! 2. **机制复用**（判据二）。双级回溯链（行→IR→源）与 F0508 同构，断链
//!    标注登记。
//!
//! 3. **工具链怪癖维**（判据三）。怪癖登记带工具链版本维度（同怪癖不同
//!    版本表现不同），逐版本命中。
//!
//! 4. **断链标注**（判据四）。回溯断链显性登记缺口（复用 F0508 语义）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0531";

// ---------------------------------------------------------------------------
// 一、双格式解析器
// ---------------------------------------------------------------------------

/// 工具链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Dxc,
    Fxc,
}

/// 解析出的错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PErr {
    pub line: u32,
    pub code: String,
}

/// dxc 格式：`shader.hlsl:12:5: error X3000: msg`
pub fn parse_dxc(out: &str) -> Vec<PErr> {
    let mut v: Vec<PErr> = Vec::new();
    for l in out.lines() {
        // file:line:col: error CODE: msg
        let parts: Vec<&str> = l.splitn(4, ':').collect();
        if parts.len() >= 4 && parts[3].trim_start().starts_with("error") {
            if let Ok(line) = parts[1].trim().parse::<u32>() {
                let code = parts[3]
                    .trim_start()
                    .trim_start_matches("error")
                    .trim()
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                v.push(PErr { line, code });
            }
        }
    }
    v
}

/// fxc 格式：`shader.hlsl(12,5): error X3000: msg`
pub fn parse_fxc(out: &str) -> Vec<PErr> {
    let mut v: Vec<PErr> = Vec::new();
    for l in out.lines() {
        let open = match l.find('(') {
            Some(i) => i,
            None => continue,
        };
        let close = match l.find(')') {
            Some(i) => i,
            None => continue,
        };
        let inner = &l[open + 1..close];
        let mut it = inner.split(',');
        let line: u32 = match it.next().and_then(|s| s.trim().parse().ok()) {
            Some(x) => x,
            None => continue,
        };
        if let Some(ep) = l.find("error") {
            let code = l[ep..].trim_start_matches("error").trim().split(':').next().unwrap_or("").trim().to_string();
            v.push(PErr { line, code });
        }
    }
    v
}

/// 统一入口。
pub fn parse(t: Tool, out: &str) -> Vec<PErr> {
    match t {
        Tool::Dxc => parse_dxc(out),
        Tool::Fxc => parse_fxc(out),
    }
}

// ---------------------------------------------------------------------------
// 二、双级回溯（复用 F0508 机制）
// ---------------------------------------------------------------------------

/// 回溯。
pub fn backtrack(line: u32, line_map: &[(u32, u32)], src_map: &[(u32, u32)]) -> (Option<u32>, Option<u32>) {
    let ir = line_map.iter().find(|(g, _)| *g == line).map(|(_, i)| *i);
    let src = match ir {
        Some(i) => src_map.iter().find(|(n, _)| *n == i).map(|(_, s)| *s),
        None => None,
    };
    (ir, src)
}

/// 断链标注。
pub fn broken(ir: Option<u32>, src: Option<u32>) -> bool {
    ir.is_none() || src.is_none()
}

// ---------------------------------------------------------------------------
// 三、工具链版本怪癖维
// ---------------------------------------------------------------------------

/// 怪癖条目（版本维）。
#[derive(Clone, Copy, Debug)]
pub struct Quirk {
    pub tool: Tool,
    /// 工具链版本（dxc 编译器版本号 / fxc 版本号）。
    pub ver: u32,
    pub cond: &'static str,
    pub evade: &'static str,
}

pub const QUIRKS: [Quirk; 3] = [
    Quirk { tool: Tool::Dxc, ver: 2023, cond: "switch fallthrough", evade: "补 break 显性化" },
    Quirk { tool: Tool::Dxc, ver: 2022, cond: "matrix 访问越界", evade: "静态裁剪访问" },
    Quirk { tool: Tool::Fxc, ver: 1074, cond: "texLod in loop", evade: "Lod 提升出循环" },
];

/// 版本维命中。
pub fn quirk_hit(t: Tool, ver: u32, msg: &str) -> Option<&'static Quirk> {
    QUIRKS.iter().find(|q| q.tool == t && q.ver == ver && msg.contains(q.cond))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec131_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec131");

    // 族一：dxc 解析。
    let d = parse_dxc("shader.hlsl:12:5: error X3000: syntax error\nok line\nshader.hlsl:3:1: error X4532: type mismatch");
    s.add("dxc 双错误", d.len() == 2, "行号提取");
    s.add("dxc 行号", d[0].line == 12 && d[1].line == 3, "逐行定位");
    s.add("dxc 错误码", d[0].code == "X3000", "码提取");
    s.add("dxc 忽略健康行", !d.iter().any(|e| e.line == 0), "无假错误");

    // 族二：fxc 解析。
    let f = parse_fxc("shader.hlsl(7,10): error X3521: bad index\ngood(1,1): warning W4000");
    s.add("fxc 单错误", f.len() == 1, "括号格式");
    s.add("fxc 行号", f[0].line == 7, "行号提取");
    s.add("fxc 码", f[0].code == "X3521", "码提取");
    s.add("warning 过滤", !f.iter().any(|e| e.code.starts_with('W')), "只收 error");

    // 族三：统一入口与回溯。
    let lm = [(12u32, 40u32), (7, 41)];
    let sm = [(40u32, 100u32)];
    let (ir, src) = backtrack(12, &lm, &sm);
    s.add("四级链通", ir == Some(40) && src == Some(100), "行→IR→源");
    let (ir2, src2) = backtrack(7, &lm, &sm);
    s.add("源级断链标注", broken(ir2, src2), "缺源映射");
    let (ir3, _) = backtrack(99, &lm, &sm);
    s.add("行级断链标注", broken(ir3, None), "无映射行");

    // 族四：工具链版本怪癖。
    s.add("版本命中", quirk_hit(Tool::Dxc, 2023, "switch fallthrough in gs").is_some(), "同版本同条件");
    s.add("版本不符不命中", quirk_hit(Tool::Dxc, 2022, "switch fallthrough").is_none(), "版本维隔离");
    s.add("工具不符不命中", quirk_hit(Tool::Fxc, 2023, "switch fallthrough").is_none(), "工具维隔离");
    s.add("fxc 独立怪癖", quirk_hit(Tool::Fxc, 1074, "texLod in loop fails").map(|q| q.evade.contains("提升")) == Some(true), "逐工具登记");
    s.add("怪癖表三件", QUIRKS.len() == 3, "正本规模");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let d = parse_dxc("f.hlsl:1:1: error X1: bad");
        assert_eq!(d[0].line, 1);
        let f = parse_fxc("f.hlsl(2,3): error X2: bad");
        assert_eq!(f[0].line, 2);
    }
}
