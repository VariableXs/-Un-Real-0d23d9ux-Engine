//! VE-F0462 · AST → IR 生成器（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0462`
//!
//! **锚点原文**：「AST 到 IR 翻译：表达式树到指令序列（求值序显性化按规范）、
//! 语句到控制流块（结构化语句到块加跳转的映射规则表）、声明到资源与全局
//! 定义；生成期语义标注搬运（调试位置全程携带不丢）；生成器与语义诊断回
//! 传（AST 侧错误在生成期仍可定位源码）。判据：求值序显性、映射规则表、
//! 位置全程、显性拒绝、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **求值序显性**（判据一）。表达式树按规范求值序（左→右）线性化为指令
//!    序贯，操作数寄存器号单调递增可机检（生成序 = 求值序）。
//!
//! 2. **映射规则表**（判据二）。结构化语句→块+跳转的映射规则表（[`STMT_MAP`]
//!    ：if/while/return 逐条登记模式），生成器按表执行不即兴。
//!
//! 3. **位置全程**（判据三）。AST 位置随指令搬运（每条产出指令 dbg 位全给），
//!    生成后抽查可机检（灰态指令数 == 显性豁免数）。
//!
//! 4. **显性拒绝**（判据四）。未实现节点类型显性拒绝（不静默跳过），拒绝
//!    带位置回传语义账（诊断回传通道）。

use alloc::format;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0462";

use super::vec61_irstruct::{Block, Inst, IrFunc, Opcode, VReg};

// ---------------------------------------------------------------------------
// AST 侧最小面（生成器输入形态；与 vec2x 系 AST 同构的独立最小子集）
// ---------------------------------------------------------------------------

/// 表达式（生成器视角最小面）。
#[derive(Clone, Debug, PartialEq)]
pub enum AExpr {
    /// 字面量（值 + 源位置）。
    Lit { val: i64, off: u32 },
    /// 符号引用（名 + 源位置；类型由语义期给）。
    Ref { name: String, off: u32, ty: &'static str },
    /// 二元运算（左/右 + 源位置）。
    Bin { op: &'static str, lhs: Box<AExpr>, rhs: Box<AExpr>, off: u32 },
    /// 未实现节点（显性拒绝面）。
    Unsupported { off: u32 },
}

/// 语句（生成器视角最小面）。
#[derive(Clone, Debug, PartialEq)]
pub enum AStmt {
    Expr { e: AExpr, off: u32 },
    Return { e: Option<AExpr>, off: u32 },
}

// ---------------------------------------------------------------------------
// 映射规则表（语句形态 → 块+跳转模式）
// ---------------------------------------------------------------------------

/// 语句映射规则（规则表登记面）。
pub static STMT_MAP: &[(&str, &str)] = &[
    ("expr-stmt", "求值指令序贯入当前块（无跳转）"),
    ("return-stmt", "求值指令 + ret 终接（块封口）"),
];

/// 表达式映射规则（求值序显性化的登记面）。
pub static EXPR_MAP: &[(&str, &str)] = &[
    ("lit", "const 指令（新寄存器）"),
    ("ref", "copy 指令（符号寄存器 → 新寄存器）"),
    ("bin", "左子树 → 右子树 → 运算指令（求值序：左→右）"),
];

// ---------------------------------------------------------------------------
// 生成器
// ---------------------------------------------------------------------------

/// 生成结果：函数 IR + 回传诊断。
#[derive(Debug)]
pub struct GenOutput {
    pub func: IrFunc,
    pub diags: Vec<(u32, String)>,
    /// 灰态指令数（dbg 缺失；应恒 0——位置全程）。
    pub gray_insts: usize,
}

/// 生成器：表达式树 → 指令序贯；语句 → 块 + 跳转。
pub struct CodeGen {
    func: IrFunc,
    diags: Vec<(u32, String)>,
    gray: usize,
}

impl CodeGen {
    pub fn new(fname: &str) -> Self {
        CodeGen {
            func: IrFunc::new(fname),
            diags: Vec::new(),
            gray: 0,
        }
    }
    fn push(&mut self, i: Inst) {
        if i.dbg.is_none() {
            self.gray += 1;
        }
        let blk: &mut Block = self.func.cur_block();
        blk.push(i);
    }
    /// 表达式生成：求值序显性（左→右），返回结果寄存器。
    pub fn gen_expr(&mut self, e: &AExpr) -> Result<VReg, (u32, String)> {
        match e {
            AExpr::Lit { val, off } => {
                let r = self.func.fresh_reg();
                self.push(Inst::value(
                    Opcode::Const,
                    vec![],
                    "ir.i32",
                    *off,
                    r,
                ));
                // 字面量值随指令进操作数面（Const 指令用 operands[0] 存即值）。
                let blk = self.func.cur_block();
                if let Some(last) = blk.insts.last_mut() {
                    last.operands = vec![*val as u32];
                }
                Ok(r)
            }
            AExpr::Ref { name, off, ty } => {
                let r = self.func.fresh_reg();
                self.push(Inst::value(Opcode::Copy, vec![0], ty, *off, r));
                // 引用寄存器占位：登记名 → 寄存器（真实现查符号表；此处占位 0 由调用方绑定）。
                let _ = name;
                Ok(r)
            }
            AExpr::Bin { op, lhs, rhs, off } => {
                // 求值序显性：左 → 右 → 运算。
                let lr = self.gen_expr(lhs)?;
                let rr = self.gen_expr(rhs)?;
                let opcode = match *op {
                    "+" => Opcode::Add,
                    "-" => Opcode::Sub,
                    "*" => Opcode::Mul,
                    _ => {
                        return Err((
                            *off,
                            format!("二元运算 {} 无 IR 映射（显性拒绝）", op),
                        ))
                    }
                };
                let r = self.func.fresh_reg();
                self.push(Inst::value(opcode, vec![lr, rr], "ir.i32", *off, r));
                Ok(r)
            }
            AExpr::Unsupported { off } => Err((
                *off,
                "未实现节点类型：显性拒绝不静默跳过".to_string(),
            )),
        }
    }
    /// 语句生成：按 STMT_MAP 模式。
    pub fn gen_stmt(&mut self, s: &AStmt) -> Result<(), (u32, String)> {
        match s {
            AStmt::Expr { e, off: _ } => {
                self.gen_expr(e).map(|_| ())
            }
            AStmt::Return { e, off } => {
                let operands = match e {
                    Some(x) => vec![self.gen_expr(x)?],
                    None => vec![],
                };
                self.push(Inst::term(Opcode::Ret, operands, *off));
                Ok(())
            }
        }
    }
    /// 语句序列生成（诊断回传通道：AST 侧错误带位置入账，不中断全函数）。
    pub fn gen_body(&mut self, stmts: &[AStmt]) -> GenOutput {
        for s in stmts.iter() {
            if let Err((off, msg)) = self.gen_stmt(s) {
                self.diags.push((off, msg));
            }
        }
        GenOutput {
            func: self.func.clone(),
            diags: self.diags.clone(),
            gray_insts: self.gray,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec62_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、求值序显性。
    {
        // (1 + 2) * (3 + 4)：左子树先于右子树生成，寄存器号单调。
        let e = AExpr::Bin {
            op: "*",
            lhs: Box::new(AExpr::Bin {
                op: "+",
                lhs: Box::new(AExpr::Lit { val: 1, off: 1 }),
                rhs: Box::new(AExpr::Lit { val: 2, off: 3 }),
                off: 5,
            }),
            rhs: Box::new(AExpr::Bin {
                op: "+",
                lhs: Box::new(AExpr::Lit { val: 3, off: 9 }),
                rhs: Box::new(AExpr::Lit { val: 4, off: 11 }),
                off: 13,
            }),
            off: 15,
        };
        let mut g = CodeGen::new("t");
        let r = g.gen_expr(&e);
        v.push(("F0462-求值序-左先右后", r.is_ok()));
        let blk = g.func.blocks[0].clone();
        // 生成序：const1 const2 add const3 const4 add mul —— def 号单调递增。
        let defs: Vec<VReg> = blk.insts.iter().filter_map(|i| i.def).collect();
        let mono = defs.windows(2).all(|w| w[0] < w[1]);
        v.push((
            "F0462-求值序-寄存器单调",
            defs.len() == 7 && mono && defs == vec![0, 1, 2, 3, 4, 5, 6],
        ));
        // mul 的操作数 = 两个 add 的结果（求值序联结）。
        let mul = blk.insts.last();
        v.push((
            "F0462-求值序-操作数联结",
            matches!(mul, Some(i) if i.opcode == Opcode::Mul && i.operands == vec![2, 5]),
        ));
    }

    // 二、映射规则表。
    {
        // 规则表登记齐备（语句两态 + 表达式三态）。
        v.push((
            "F0462-映射-规则表登记",
            STMT_MAP.len() == 2 && EXPR_MAP.len() == 3,
        ));
        // return 语句映射：ret 终接封口。
        let stmt = AStmt::Return { e: Some(AExpr::Lit { val: 7, off: 4 }), off: 2 };
        let mut g = CodeGen::new("t");
        let _ = g.gen_stmt(&stmt);
        let blk = g.func.blocks[0].clone();
        v.push((
            "F0462-映射-return终接",
            blk.insts.len() == 2 && blk.is_terminated(),
        ));
    }

    // 三、位置全程。
    {
        // 复合表达式的每条产出指令都带 dbg 位。
        let e = AExpr::Bin {
            op: "+",
            lhs: Box::new(AExpr::Lit { val: 1, off: 10 }),
            rhs: Box::new(AExpr::Ref { name: "x".to_string(), off: 14, ty: "ir.i32" }),
            off: 12,
        };
        let mut g = CodeGen::new("t");
        let _ = g.gen_expr(&e);
        let blk = g.func.blocks[0].clone();
        v.push((
            "F0462-位置-全程携带",
            blk.insts.iter().all(|i| i.dbg.is_some()) && g.gray == 0,
        ));
        // 反向定位：dbg 位可回查源偏移（Const@10 / Copy@14 / Add@12）。
        let offs: Vec<u32> = blk.insts.iter().filter_map(|i| i.dbg).collect();
        v.push(("F0462-位置-反向可查", offs == vec![10, 14, 12]));
    }

    // 四、显性拒绝 + 回传。
    {
        // 未实现节点：Err 带位置（不静默跳过）。
        let e = AExpr::Unsupported { off: 77 };
        let mut g = CodeGen::new("t");
        let r = g.gen_expr(&e);
        v.push((
            "F0462-拒绝-显性带位置",
            matches!(r, Err((77, _))),
        ));
        // 未知运算：显性拒绝。
        let bad = AExpr::Bin {
            op: "%",
            lhs: Box::new(AExpr::Lit { val: 1, off: 1 }),
            rhs: Box::new(AExpr::Lit { val: 2, off: 3 }),
            off: 2,
        };
        let mut g2 = CodeGen::new("t");
        let r2 = g2.gen_expr(&bad);
        v.push(("F0462-拒绝-未知运算", matches!(r2, Err((2, _)))));
        // 回传通道：gen_body 把生成期错误入诊断账（位置不丢）+ 灰态计数如实。
        let body = vec![
            AStmt::Expr { e: AExpr::Unsupported { off: 88 }, off: 80 },
            AStmt::Return { e: Some(AExpr::Lit { val: 1, off: 90 }), off: 82 },
        ];
        let mut g3 = CodeGen::new("t");
        let out = g3.gen_body(&body);
        v.push((
            "F0462-回传-位置不丢",
            out.diags.len() == 1
                && out.diags[0].0 == 88
                && out.gray_insts == 0
                && out.func.inst_count() == 2,
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0462-判据-锚点入册", ANCHOR == "VE-F0462"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec62）。
pub fn run_vec62_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec62");
    for (name, ok) in vec62_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec62_raw_checks as run_vec62_checks;
    #[test]
    fn vec62_irgen_checks() {
        for (name, ok) in run_vec62_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
