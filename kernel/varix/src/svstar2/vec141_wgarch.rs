//! VE-F0541 · WGSL 后端架构（近直通）（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0541`
//!
//! **锚点原文**：「WGSL 后端总体架构（近直通定位声明）：IR 到 WGSL 文本生成
//! （内部 IR 设计即贴近 WGSL 语义模型，故称近直通——转换面小）、强类型与
//! 显性化语义天然对齐度高、目标上下文（WGSL 规范版本锁定策略）；与 GLSL/
//! HLSL 后端框架同构（框架第三用复用声明）。判据：近直通定位、版本锁定、
//! 框架第三用、假设如实、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **近直通定位**（判据一）。直通假设（转换面小）量化登记：每指令的
//!    转换步数上界（≤2 步），超界处如实改直通为映射。
//!
//! 2. **版本锁定**（判据二）。WGSL 规范版本锁定策略（锁死单版本，不追
//!    动态规范），锁外版本显性拒绝。
//!
//! 3. **框架第三用**（判据三）。三段管线与对拍框架第三次复用声明在册
//!    （GLSL 第一用、HLSL 第二用、WGSL 第三用）。
//!
//! 4. **假设如实**（判据四）。对齐度假设的失效处显性登记（发现一处登记
//!    一处，不硬撑直通）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0541";

/// WGSL 规范版本锁定。
pub const WGSL_LOCKED: u32 = 1;

/// 框架复用谱系（第三用声明）。
pub const FRAME_USES: [&str; 3] = ["glsl(F0501)", "hlsl(F0521)", "wgsl(F0541)"];

/// 直通转换步数上界。
pub const PASS_BOUND: u32 = 2;

/// 直通假设评估：每指令转换步数 ≤ 上界即直通可行。
pub fn direct_ok(op: &str, steps: u32) -> bool {
    let known = ["add", "mul", "dot", "load", "store", "ret"];
    known.contains(&op) && steps <= PASS_BOUND
}

/// 假设失效登记。
#[derive(Default)]
pub struct AssumpGap {
    pub gaps: Vec<String>,
}

impl AssumpGap {
    pub fn new() -> AssumpGap {
        AssumpGap { gaps: Vec::new() }
    }

    /// 失效处登记（如实：不硬撑直通）。
    pub fn reg(&mut self, op: &str, why: &str) {
        self.gaps.push(format!("{}: {}", op, why));
    }

    pub fn len(&self) -> usize {
        self.gaps.len()
    }
}

/// 版本锁定校验。
pub fn ver_ok(v: u32) -> Result<(), u32> {
    if v == WGSL_LOCKED {
        Ok(())
    } else {
        Err(XC_LOCK)
    }
}

pub const XC_LOCK: u32 = 1;

/// 三段管线（第三用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Select,
    Emit,
    Format,
}

pub struct WPipe {
    trace: Vec<Stage>,
}

impl WPipe {
    pub fn new() -> WPipe {
        WPipe { trace: Vec::new() }
    }

    pub fn select(&mut self, op: &str) -> Result<&'static str, u32> {
        let t = match op {
            "add" => "f32.add",
            "mul" => "f32.mul",
            "dot" => "dot",
            "load" => "arrayLoad",
            "store" => "arrayStore",
            _ => return Err(XC_NOOP),
        };
        self.trace.push(Stage::Select);
        Ok(t)
    }

    pub fn emit(&mut self, tpl: &str, args: &str) -> Result<String, u32> {
        if args.is_empty() {
            return Err(XC_NOARGS);
        }
        self.trace.push(Stage::Emit);
        Ok(format!("{}({})", tpl, args))
    }

    pub fn format(&mut self, em: &str, indent: u32) -> Result<String, u32> {
        if indent > 8 {
            return Err(XC_INDENT);
        }
        self.trace.push(Stage::Format);
        Ok(format!("{}{};", "  ".repeat(indent as usize), em))
    }

    pub fn in_order(&self) -> bool {
        self.trace == [Stage::Select, Stage::Emit, Stage::Format]
    }
}

pub const XC_NOOP: u32 = 2;
pub const XC_NOARGS: u32 = 3;
pub const XC_INDENT: u32 = 4;

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec141_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec141");

    // 族一：近直通定位。
    s.add("直通可行", direct_ok("add", 1), "一步转换");
    s.add("步数上界", direct_ok("dot", 2) && !direct_ok("dot", 3), "≤2 步");
    s.add("表外指令非直通", !direct_ok("mesh", 1), "如实改映射");

    // 族二：版本锁定。
    s.add("锁定版本过", ver_ok(WGSL_LOCKED).is_ok(), "锁死单版");
    s.add("锁外拒", matches!(ver_ok(2), Err(XC_LOCK)), "不追动态规范");

    // 族三：框架第三用。
    s.add("三用谱系", FRAME_USES.len() == 3 && FRAME_USES[2].contains("wgsl"), "复用声明");
    s.add("第一用 GLSL", FRAME_USES[0].contains("F0501"), "谱系锚点");

    // 族四：假设失效如实。
    let mut g = AssumpGap::new();
    g.reg("atomics", "WGSL 原子域约束超出直通面");
    s.add("失效登记可见", g.len() == 1, "不硬撑直通");
    s.add("登记带理由", g.gaps[0].contains("原子"), "如实记录");

    // 族五：三段管线（第三用）。
    let mut p = WPipe::new();
    let t = match p.select("dot") {
        Ok(t) => t,
        Err(_) => { s.add("选段败", false, "不应到"); return s; }
    };
    let em = match p.emit(t, "r, a, b") {
        Ok(e) => e,
        Err(_) => { s.add("发射败", false, "不应到"); return s; }
    };
    let fm = match p.format(&em, 1) {
        Ok(f) => f,
        Err(_) => { s.add("格式化败", false, "不应到"); return s; }
    };
    s.add("三段按序", p.in_order(), "第三用复用");
    s.add("WGSL 函数式", fm == "  dot(r, a, b);", "内建文本");
    s.add("表外拒", matches!(p.select("rsq"), Err(XC_NOOP)), "显性拒绝");
    s.add("空参拒", matches!(p.emit("dot", ""), Err(XC_NOARGS)), "参数必填");
    s.add("缩进超界拒", matches!(p.format("x()", 9), Err(XC_INDENT)), "≤8");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe() {
        let mut p = WPipe::new();
        let t = p.select("add").ok().unwrap();
        let e = p.emit(t, "r, a, b").ok().unwrap();
        assert!(p.format(&e, 0).is_ok());
        assert!(p.in_order());
    }
}
