//! VE-F0521 · HLSL 后端架构（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0521`
//!
//! **锚点原文**：「HLSL 后端总体架构：IR 到 HLSL 文本生成管线（复用 GLSL
//! 后端三段管线框架第二用复用声明，语义差异全 HLSL 化）、目标模型（SM 版本
//! 与编译工具链 dxc/fxc 双路径上下文）、寄存器与语义绑定体系对接（与桌面
//! Windows 域协作的接口语境）；架构决策记录。判据：框架复用、双工具链、绑定
//! 体系、决策记录、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **框架复用**（判据一）。三段管线（选/发/格）与 GLSL 后端同构，本域
//!    只做 HLSL 语义适配——不另造第二套管线。
//!
//! 2. **双工具链**（判据二）。dxc（现代）/ fxc（传统）双路径上下文逐一路
//!    径登记（SM 支持面、输出格式、诊断形态），路径缺失显性。
//!
//! 3. **绑定体系**（判据三）。寄存器与语义绑定接口对接面（register/space/
//!    语义标记）登记在册，供 F0524 分配器与 F0527 语义映射消费。
//!
//! 4. **决策记录**（判据四）。架构裁决（复用框架、双路径并存）逐条带理由
//!    与反方案代价入册。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0521";

// ---------------------------------------------------------------------------
// 一、双工具链上下文
// ---------------------------------------------------------------------------

/// 工具链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Dxc,
    Fxc,
}

/// 工具链上下文。
#[derive(Clone, Copy, Debug)]
pub struct ToolCtx {
    pub tool: Tool,
    /// 支持的最高 SM 版本（如 66 = 6.6）。
    pub sm_max: u32,
    /// 输出格式标签。
    pub out_fmt: &'static str,
}

/// 双路径正本。
pub const TOOLS: [ToolCtx; 2] = [
    ToolCtx { tool: Tool::Dxc, sm_max: 66, out_fmt: "dxil" },
    ToolCtx { tool: Tool::Fxc, sm_max: 51, out_fmt: "dxbc" },
];

/// 路径选择：SM 版本 > 5.1 必须 dxc（fxc 不支持 SM6）。
pub fn pick_tool(sm: u32) -> Result<&'static ToolCtx, u32> {
    if sm > 51 {
        return TOOLS.iter().find(|t| t.tool == Tool::Dxc).ok_or(XC_NOTOOL);
    }
    TOOLS.iter().find(|t| t.sm_max >= sm).ok_or(XC_NOTOOL)
}

pub const XC_NOTOOL: u32 = 1;

/// SM 版本合法性（4.0 ~ 6.6）。
pub fn sm_ok(sm: u32) -> bool {
    (40..=66).contains(&sm) && (sm % 10) <= 9
}

// ---------------------------------------------------------------------------
// 二、三段管线复用（HLSL 语义适配）
// ---------------------------------------------------------------------------

/// 管线段（与 GLSL 同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Select,
    Emit,
    Format,
}

/// HLSL 模板操作码表。
pub const HLSL_OPS: [&str; 6] = ["add", "mul", "mad", "dot", "sample", "store"];

/// HLSL 适配管线。
pub struct HPipe {
    pub sm: u32,
    trace: Vec<Stage>,
}

impl HPipe {
    pub fn new(sm: u32) -> Result<HPipe, u32> {
        if !sm_ok(sm) {
            return Err(XC_SM);
        }
        Ok(HPipe { sm, trace: Vec::new() })
    }

    pub fn select(&mut self, op: &str) -> Result<&'static str, u32> {
        let t = HLSL_OPS.iter().find(|o| **o == op).ok_or(XC_NOOP)?;
        self.trace.push(Stage::Select);
        Ok(t)
    }

    pub fn emit(&mut self, tpl: &str, args: &str) -> Result<String, u32> {
        if !HLSL_OPS.contains(&tpl) {
            return Err(XC_NOOP);
        }
        if args.is_empty() {
            return Err(XC_NOARGS);
        }
        self.trace.push(Stage::Emit);
        Ok(format!("{}({})", tpl, args))
    }

    pub fn format(&mut self, em: &str, indent: u32) -> Result<String, u32> {
        if em.is_empty() {
            return Err(XC_EMPTY);
        }
        if indent > 8 {
            return Err(XC_INDENT);
        }
        self.trace.push(Stage::Format);
        Ok(format!("{}{};", "    ".repeat(indent as usize), em))
    }

    pub fn in_order(&self) -> bool {
        self.trace == [Stage::Select, Stage::Emit, Stage::Format]
    }
}

pub const XC_SM: u32 = 2;
pub const XC_NOOP: u32 = 3;
pub const XC_NOARGS: u32 = 4;
pub const XC_EMPTY: u32 = 5;
pub const XC_INDENT: u32 = 6;

// ---------------------------------------------------------------------------
// 三、绑定体系对接面
// ---------------------------------------------------------------------------

/// 绑定接口项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindIf {
    /// register 槽（b0/t0/s0/u0 族）。
    pub reg: u32,
    /// space 索引。
    pub space: u32,
}

/// 对接面登记校验：register < 4096 且 space < 16（D3D 约束面）。
pub fn bind_ok(b: BindIf) -> bool {
    b.reg < 4096 && b.space < 16
}

// ---------------------------------------------------------------------------
// 四、架构决策记录
// ---------------------------------------------------------------------------

pub struct Decision {
    pub verdict: &'static str,
    pub why: &'static str,
    pub alt: &'static str,
}

pub const DECISIONS: [Decision; 2] = [
    Decision {
        verdict: "复用 GLSL 三段管线框架",
        why: "段间产物与黄金对拍机制直接复用，双后端一致可测",
        alt: "独立管线：隔离性好但双份维护、对拍框架翻倍",
    },
    Decision {
        verdict: "dxc/fxc 双路径并存",
        why: "SM5 存量覆盖靠 fxc，SM6 现代特性靠 dxc",
        alt: "仅 dxc：放弃 SM5 存量；仅 fxc：放弃 SM6",
    },
];

pub fn decisions_ok() -> bool {
    DECISIONS.iter().all(|d| !d.verdict.is_empty() && !d.why.is_empty() && !d.alt.is_empty())
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec121_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec121");

    // 族一：双工具链。
    s.add("SM6 走 dxc", pick_tool(60).map(|t| t.tool) == Ok(Tool::Dxc), "fxc 不支持 SM6");
    s.add("SM5 走 fxc", pick_tool(50).map(|t| t.tool) == Ok(Tool::Fxc), "存量路径");
    s.add("SM5.1 走 dxc", pick_tool(51).map(|t| t.tool) == Ok(Tool::Dxc), "51 边界归 dxc");
    s.add("SM3 拒", matches!(pick_tool(30), Err(XC_NOTOOL)), "支持面外");
    s.add("双路径在册", TOOLS.len() == 2 && TOOLS[0].tool != TOOLS[1].tool, "dxc+fxc");
    s.add("SM 界内", sm_ok(40) && sm_ok(66) && !sm_ok(70), "4.0~6.6");

    // 族二：三段管线复用。
    let mut p = match HPipe::new(62) {
        Ok(p) => p,
        Err(_) => { s.add("建管线败", false, "不应到"); return s; }
    };
    let tpl = match p.select("mad") {
        Ok(t) => t,
        Err(_) => { s.add("选段败", false, "不应到"); return s; }
    };
    let em = match p.emit(tpl, "r0, a, b, c") {
        Ok(e) => e,
        Err(_) => { s.add("发射败", false, "不应到"); return s; }
    };
    let fm = match p.format(&em, 1) {
        Ok(f) => f,
        Err(_) => { s.add("格式化败", false, "不应到"); return s; }
    };
    s.add("三段按序", p.in_order(), "复用框架");
    s.add("HLSL 函数形态", fm == "    mad(r0, a, b, c);", "函数式文本");
    s.add("表外操作码拒", matches!(p.select("rsq"), Err(XC_NOOP)), "显性拒绝");
    s.add("空参拒", matches!(p.emit("dot", ""), Err(XC_NOARGS)), "参数必填");
    s.add("缩进超界拒", matches!(p.format("x()", 9), Err(XC_INDENT)), "≤8");
    s.add("非法 SM 拒", matches!(HPipe::new(70), Err(XC_SM)), "SM 校验");

    // 族三：绑定体系。
    s.add("绑定合法", bind_ok(BindIf { reg: 0, space: 0 }), "b0 space0");
    s.add("register 超界拒", !bind_ok(BindIf { reg: 4096, space: 0 }), "<4096");
    s.add("space 超界拒", !bind_ok(BindIf { reg: 0, space: 16 }), "<16");

    // 族四：决策记录。
    s.add("裁决记录齐备", decisions_ok(), "结论+理由+反方案");
    s.add("框架复用在册", DECISIONS.iter().any(|d| d.verdict.contains("复用")), "架构裁决");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool() {
        assert_eq!(pick_tool(60).ok().map(|t| t.tool), Some(Tool::Dxc));
        assert_eq!(pick_tool(50).ok().map(|t| t.tool), Some(Tool::Fxc));
    }
}
