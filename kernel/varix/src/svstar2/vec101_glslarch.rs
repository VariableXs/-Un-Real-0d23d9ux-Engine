//! VE-F0501 · GLSL 后端架构（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0501`
//!
//! **锚点原文**：「GLSL 后端总体架构：IR 到 GLSL 文本生成管线（指令选择、
//! 文本发射、格式化三段）、目标 GLSL 版本与扩展上下文（版本决定可用构造）、
//! 与编译流水线集成（F0479 编排的末段）；架构决策记录（为什么文本而非进程
//! 内——生态兼容优先）。判据：三段管线、版本上下文、生态优先、决策记录、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三段管线**（判据一）。指令选择 → 文本发射 → 格式化三段顺序执行，
//!    段间传递中间产物（选中的模板 → 未格式化文本 → 成品文本）；跳段
//!    （未选择即发射）是架构违规。
//!
//! 2. **版本上下文**（判据二）。目标版本决定可用构造集——构造在版本表外
//!    一律显性拒绝（不静默降级到未登记路径）。
//!
//! 3. **生态优先**（判据三）。决策记录登记「文本而非进程内」的裁决与理由
//!    （生态兼容优先），并给出反方案的代价对照——决策不是口令，是记录。
//!
//! 4. **决策记录**（判据四）。架构裁决逐条入 [`DECISIONS`]，每条含结论、
//!    理由、反方案代价；缺任何一环视为未记录。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0501";

// ---------------------------------------------------------------------------
// 一、三段管线
// ---------------------------------------------------------------------------

/// 管线段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Select,
    Emit,
    Format,
}

/// 中间产物（段间传递物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub stage: Stage,
    pub text: String,
}

/// GLSL 生成管线：三段顺序执行，跳段违规。
pub struct Pipe {
    version: u32,
    trace: Vec<Stage>,
}

impl Pipe {
    /// 建管线：版本必须是已登记版本。
    pub fn new(version: u32) -> Result<Pipe, u32> {
        if !SUPPORTED_VERSIONS.contains(&version) {
            return Err(XC_VER);
        }
        Ok(Pipe { version, trace: Vec::new() })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// 指令选择段：操作码 → 模板名。
    pub fn select(&mut self, op: &str) -> Result<Artifact, u32> {
        if !TEMPLATE_OPS.contains(&op) {
            return Err(XC_NOTPL);
        }
        self.trace.push(Stage::Select);
        Ok(Artifact { stage: Stage::Select, text: format!("tpl:{}", op) })
    }

    /// 文本发射段：模板名 + 操作数 → 未格式化行。
    pub fn emit(&mut self, sel: &Artifact, operand: &str) -> Result<Artifact, u32> {
        if sel.stage != Stage::Select {
            return Err(XC_STAGE);
        }
        let tpl = match sel.text.strip_prefix("tpl:") {
            Some(t) => t,
            None => return Err(XC_NOTPL),
        };
        self.trace.push(Stage::Emit);
        Ok(Artifact { stage: Stage::Emit, text: format!("{} {}", tpl, operand) })
    }

    /// 格式化段：补分号与缩进。
    pub fn format(&mut self, em: &Artifact, indent: u32) -> Result<Artifact, u32> {
        if em.stage != Stage::Emit {
            return Err(XC_STAGE);
        }
        if indent > 8 {
            return Err(XC_INDENT);
        }
        self.trace.push(Stage::Format);
        let pad = "    ".repeat(indent as usize);
        Ok(Artifact { stage: Stage::Format, text: format!("{}{};", pad, em.text) })
    }

    /// 段序完整性：三段按序出现。
    pub fn in_order(&self) -> bool {
        self.trace == [Stage::Select, Stage::Emit, Stage::Format]
    }
}

pub const XC_VER: u32 = 1;
pub const XC_NOTPL: u32 = 2;
pub const XC_STAGE: u32 = 3;
pub const XC_INDENT: u32 = 4;

/// 已登记版本表。
pub const SUPPORTED_VERSIONS: [u32; 4] = [120, 330, 450, 460];
/// 指令选择段模板操作码表。
pub const TEMPLATE_OPS: [&str; 8] = [
    "add", "sub", "mul", "mad", "dp3", "dp4", "texld", "mov",
];

// ---------------------------------------------------------------------------
// 二、版本上下文（版本决定可用构造）
// ---------------------------------------------------------------------------

/// 构造（语言特性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctor {
    Layout,
    UboBlock,
    SsboBlock,
    BitwiseOp,
    ComputeEntry,
}

/// 构造可用性按版本裁定：表内可用、表外显性拒绝。
pub fn ctor_allowed(version: u32, c: Ctor) -> Result<(), u32> {
    let ok = match c {
        Ctor::Layout => version >= 140,
        Ctor::UboBlock => version >= 140,
        Ctor::SsboBlock => version >= 430,
        Ctor::BitwiseOp => version >= 130,
        Ctor::ComputeEntry => version >= 430,
    };
    if ok && SUPPORTED_VERSIONS.contains(&version) {
        Ok(())
    } else if !SUPPORTED_VERSIONS.contains(&version) {
        Err(XC_VER)
    } else {
        Err(XC_CTOR)
    }
}

pub const XC_CTOR: u32 = 5;

// ---------------------------------------------------------------------------
// 三、架构决策记录
// ---------------------------------------------------------------------------

/// 架构决策条目。
#[derive(Clone, Copy, Debug)]
pub struct Decision {
    pub verdict: &'static str,
    pub why: &'static str,
    /// 反方案及其代价。
    pub alt_cost: &'static str,
}

/// 架构决策正本（生态优先裁决在册）。
pub const DECISIONS: [Decision; 2] = [
    Decision {
        verdict: "文本生成而非进程内编译",
        why: "生态兼容优先：任意驱动/工具链可直接消费文本产物",
        alt_cost: "进程内：性能高但绑死特定驱动 ABI，生态面收窄",
    },
    Decision {
        verdict: "三段管线而非单遍直发",
        why: "选段可测、发射可换、格式化可调，段间产物可断点审查",
        alt_cost: "单遍：快但不可分段验证，黄金对拍无锚点",
    },
];

/// 决策记录完整性。
pub fn decisions_ok() -> bool {
    DECISIONS.iter().all(|d| !d.verdict.is_empty() && !d.why.is_empty() && !d.alt_cost.is_empty())
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec101_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec101");

    // 族一：三段管线。
    let mut p = match Pipe::new(450) {
        Ok(p) => p,
        Err(_) => { s.add("建管线败", false, "不应到"); return s; }
    };
    let sel = match p.select("mad") {
        Ok(a) => a,
        Err(_) => { s.add("选段败", false, "不应到"); return s; }
    };
    let em = match p.emit(&sel, "r0, a, b, c") {
        Ok(a) => a,
        Err(_) => { s.add("发射败", false, "不应到"); return s; }
    };
    let fm = match p.format(&em, 1) {
        Ok(a) => a,
        Err(_) => { s.add("格式化败", false, "不应到"); return s; }
    };
    s.add("三段按序", p.in_order(), "选→发→格");
    s.add("成品带分号缩进", fm.text == "    mad r0, a, b, c;", "文本形态");
    s.add("跳段拒", matches!(p.format(&sel, 0), Err(XC_STAGE)), "未发射先格式化");
    s.add("表外操作码拒", matches!(p.select("rsq"), Err(XC_NOTPL)), "模板缺失显性");
    s.add("缩进超界拒", matches!(p.format(&em, 9), Err(XC_INDENT)), "≤8 层");

    // 族二：版本上下文。
    s.add("未登记版本拒", matches!(Pipe::new(330 + 120), Err(XC_VER)), "版本表外");
    s.add("SSBO 需 430+", ctor_allowed(450, Ctor::SsboBlock).is_ok(), "高版本可用");
    s.add("低版本 SSBO 拒", matches!(ctor_allowed(330, Ctor::SsboBlock), Err(XC_CTOR)), "显性拒绝");
    s.add("位运算需 130+", ctor_allowed(120, Ctor::BitwiseOp) == Err(XC_CTOR), "版本决定构造");
    s.add("计算入口需 430+", ctor_allowed(460, Ctor::ComputeEntry).is_ok(), "460 可用");

    // 族三：决策记录。
    s.add("裁决记录齐备", decisions_ok(), "结论+理由+反方案");
    s.add("生态优先在册", DECISIONS.iter().any(|d| d.verdict.contains("文本") && d.why.contains("生态")), "架构裁决");

    // 族四：版本表一致性。
    s.add("版本表有序", SUPPORTED_VERSIONS.windows(2).all(|w| w[0] < w[1]), "升序登记");
    s.add("模板表非空且唯一", {
        let mut ops = TEMPLATE_OPS.to_vec();
        let n = ops.len();
        ops.sort();
        ops.dedup();
        ops.len() == n && n >= 8
    }, "无重复模板");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_ok() {
        let mut p = Pipe::new(460).ok().unwrap();
        let a = p.select("dp4").ok().unwrap();
        let b = p.emit(&a, "r1, x, y").ok().unwrap();
        let c = p.format(&b, 0).ok().unwrap();
        assert!(p.in_order());
        assert!(c.text.ends_with(';'));
    }
}
