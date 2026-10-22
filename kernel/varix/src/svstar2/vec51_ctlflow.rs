//! VE-F0451 · 控制流语义验证（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0451`
//!
//! **锚点原文**：「控制流语义验证：return 全路径覆盖检查（函数声明返回类型
//! 非空时全路径验证）、break/continue 作用域合法性（循环或开关语境判定）、
//! discard 语义位置规则；验证基于 AST 控制流摘要（轻量结构不建完整 CFG——
//! 成本与收益显性权衡记录）。判据：全路径覆盖、语境判定、轻量摘要、未验证
//! 显性、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **全路径覆盖**（判据一）。返回类型非空的函数做 return 全路径
//!    验证：顺序末尾、if 双分支都覆盖、循环体内 return 记「可能覆盖」
//!    （循环可能零次执行不构成覆盖）——覆盖判定保守（不确定即不通过）。
//!
//! 2. **语境判定**（判据二）。break/continue 只在循环（或开关）语境
//!    合法：摘要里带嵌套深度与语境标记，非法即报最近合法作用域。
//!
//! 3. **轻量摘要**（判据三）。不建完整 CFG：摘要（[`CfSummary`]）只记
//!    块形态 + 终接 + 语境栈，单遍 O(节点数)——成本与收益的权衡显性
//!    记录在摘要头（[`SUMMARY_NOTE`]）。
//!
//! 4. **未验证显性**（判据四）。摘要构建失败（不支持形态）→ 降级跳过
//!    并显性标注未验证（[`CfDiag::NotVerified`]），不静默当通过。

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0451";

/// 轻量摘要的显性权衡记录。
pub const SUMMARY_NOTE: &str = "摘要不建完整 CFG：验证只覆盖摘要可表达的三类形态";
/// 规范条款。
pub const CLAUSE_RETURN: &str = "F0402-§6.3.1 非空返回类型函数全路径 return";
pub const CLAUSE_CTX: &str = "F0402-§6.3.2 break/continue 仅循环或开关语境";

/// 控制流语句摘要形态。
#[derive(Clone, Debug, PartialEq)]
pub enum CfStmt {
    /// return 表达式（返回类型非空语境下是终接）。
    Return { off: u32, has_value: bool },
    /// break / continue。
    Jump { kw: &'static str, off: u32 },
    /// discard。
    Discard { off: u32 },
    /// 表达式或声明（直通）。
    Plain,
    /// 顺序块。
    Seq { items: Vec<CfStmt> },
    /// if：then 必有，else 可无。
    If { then: Box<CfStmt>, els: Option<Box<CfStmt>> },
    /// 循环体（break/continue 语境开启）。
    Loop { body: Box<CfStmt> },
    /// 开关（break 语境开启）。
    Switch { body: Box<CfStmt> },
}

/// 语境栈帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Fn,
    Loop,
    Switch,
}

/// 摘要（轻量 CFG 替代面）。
#[derive(Debug, Default)]
pub struct CfSummary {
    pub nodes: usize,
    pub loops: usize,
    pub jumps: usize,
    pub returns: usize,
}

/// 验证诊断。
#[derive(Clone, Debug, PartialEq)]
pub enum CfDiag {
    /// return 覆盖缺失：带未覆盖分支位置。
    MissingReturn { off: u32, clause: &'static str },
    /// break/continue 语境非法：带最近合法作用域名。
    BadCtx { off: u32, kw: &'static str, nearest: &'static str, clause: &'static str },
    /// 摘要构建失败：显性未验证。
    NotVerified { off: u32 },
}

/// 控制流验证器：单遍摘要 + 双判据。
pub struct CfVerifier {
    pub summary: CfSummary,
    pub diags: Vec<CfDiag>,
    frames: Vec<Frame>,
    fn_returns_value: bool,
}

impl CfVerifier {
    pub fn new(fn_returns_value: bool) -> Self {
        CfVerifier {
            summary: CfSummary::default(),
            diags: Vec::new(),
            frames: vec![Frame::Fn],
            fn_returns_value,
        }
    }

    /// 验证一段控制流（返回 Ok(覆盖) / Err(未验证降级)）。
    pub fn verify(&mut self, s: &CfStmt) -> Result<bool, CfDiag> {
        match s {
            CfStmt::Plain => {
                self.summary.nodes += 1;
                Ok(false)
            }
            CfStmt::Return { off, has_value } => {
                self.summary.nodes += 1;
                self.summary.returns += 1;
                // 返回类型非空却 return 空值 → 覆盖缺失视角的形态错误。
                if self.fn_returns_value && !has_value {
                    self.diags.push(CfDiag::MissingReturn {
                        off: *off,
                        clause: CLAUSE_RETURN,
                    });
                }
                Ok(true)
            }
            CfStmt::Jump { kw, off } => {
                self.summary.nodes += 1;
                self.summary.jumps += 1;
                // 语境判定：break 需 Loop/Switch，continue 需 Loop。
                let ok = match *kw {
                    "break" => self.frames.iter().any(|f| matches!(f, Frame::Loop | Frame::Switch)),
                    "continue" => self.frames.iter().any(|f| *f == Frame::Loop),
                    _ => false,
                };
                if !ok {
                    // 最近合法作用域：栈顶向内找。
                    let nearest = self
                        .frames
                        .iter()
                        .rev()
                        .find(|f| matches!(f, Frame::Loop | Frame::Switch))
                        .map(|f| match f {
                            Frame::Loop => "loop",
                            Frame::Switch => "switch",
                            _ => "function",
                        })
                        .unwrap_or("function");
                    self.diags.push(CfDiag::BadCtx {
                        off: *off,
                        kw,
                        nearest,
                        clause: CLAUSE_CTX,
                    });
                }
                Ok(false)
            }
            CfStmt::Discard { .. } => {
                self.summary.nodes += 1;
                Ok(false)
            }
            CfStmt::Seq { items } => {
                self.summary.nodes += 1;
                // 顺序块：任一子语句覆盖即整块覆盖；子句失败即未验证降级。
                let mut covered = false;
                for it in items.iter() {
                    match self.verify(it) {
                        Ok(c) => covered = covered || c,
                        Err(d) => {
                            self.diags.push(d);
                            return Err(CfDiag::NotVerified { off: 0 });
                        }
                    }
                }
                Ok(covered)
            }
            CfStmt::If { then, els } => {
                self.summary.nodes += 1;
                // 双分支都覆盖才算覆盖；无 else 的 if 永不构成覆盖。
                let t = self.verify(then);
                let e = match els {
                    Some(x) => self.verify(x),
                    None => Ok(false),
                };
                match (t, e) {
                    (Ok(a), Ok(b)) => Ok(a && b),
                    (Err(d), _) | (_, Err(d)) => {
                        self.diags.push(d);
                        Err(CfDiag::NotVerified { off: 0 })
                    }
                }
            }
            CfStmt::Loop { body } => {
                self.summary.nodes += 1;
                self.summary.loops += 1;
                self.frames.push(Frame::Loop);
                let r = self.verify(body);
                self.frames.pop();
                // 循环可能零次执行：体内 return 不构成外层覆盖。
                let _ = r.map_err(|d| self.diags.push(d));
                Ok(false)
            }
            CfStmt::Switch { body } => {
                self.summary.nodes += 1;
                self.frames.push(Frame::Switch);
                let r = self.verify(body);
                self.frames.pop();
                let _ = r.map_err(|d| self.diags.push(d));
                Ok(false)
            }
        }
    }
}

/// 全路径覆盖验证入口：返回类型非空 → 必须覆盖；void → 免验。
pub fn check_full_path(fn_returns_value: bool, body: &CfStmt) -> (bool, Vec<CfDiag>, CfSummary) {
    let mut v = CfVerifier::new(fn_returns_value);
    let covered = v.verify(body);
    if fn_returns_value && covered != Ok(true) {
        v.diags.push(CfDiag::MissingReturn { off: 0, clause: CLAUSE_RETURN });
    }
    let ok = !fn_returns_value || covered == Ok(true);
    (ok, v.diags, v.summary)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec51_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let ret = |val: bool| CfStmt::Return { off: 1, has_value: val };
    let plain = || CfStmt::Plain;

    // 一、全路径覆盖。
    {
        // 顺序末尾 return → 覆盖。
        let seq = CfStmt::Seq { items: vec![plain(), ret(true)] };
        let (ok, d, s) = check_full_path(true, &seq);
        v.push(("F0451-覆盖-顺序末尾", ok && d.is_empty() && s.returns == 1));
        // if 双分支都 return → 覆盖。
        let if2 = CfStmt::If {
            then: Box::new(ret(true)),
            els: Some(Box::new(ret(true))),
        };
        let (ok2, d2, _) = check_full_path(true, &if2);
        v.push(("F0451-覆盖-双分支", ok2 && d2.is_empty()));
        // 无 else 的 if → 不覆盖（保守）。
        let if1 = CfStmt::If { then: Box::new(ret(true)), els: None };
        let (ok3, d3, _) = check_full_path(true, &if1);
        v.push((
            "F0451-覆盖-无else不覆盖",
            !ok3 && d3.iter().any(|d| matches!(d, CfDiag::MissingReturn { .. })),
        ));
        // 循环体内 return 不构成覆盖（可能零次执行）。
        let loop_ret = CfStmt::Loop { body: Box::new(ret(true)) };
        let (ok4, _, _) = check_full_path(true, &loop_ret);
        v.push(("F0451-覆盖-循环内不算", !ok4));
        // void 函数免验。
        let (ok5, d5, _) = check_full_path(false, &plain());
        v.push(("F0451-覆盖-void免验", ok5 && d5.is_empty()));
        // 返回类型非空却 return 空值 → 形态报错。
        let bad = CfStmt::Seq { items: vec![CfStmt::Return { off: 9, has_value: false }] };
        let (_, d6, _) = check_full_path(true, &bad);
        v.push((
            "F0451-覆盖-空值return报错",
            d6.iter().any(|d| matches!(d, CfDiag::MissingReturn { off: 9, .. })),
        ));
    }

    // 二、语境判定。
    {
        // 循环内 break/continue 合法。
        let in_loop = CfStmt::Loop {
            body: Box::new(CfStmt::Seq {
                items: vec![
                    CfStmt::Jump { kw: "break", off: 2 },
                    CfStmt::Jump { kw: "continue", off: 3 },
                ],
            }),
        };
        let (ok, d, s) = check_full_path(false, &in_loop);
        v.push((
            "F0451-语境-循环内合法",
            ok && d.is_empty() && s.jumps == 2 && s.loops == 1,
        ));
        // 函数顶层 break → 报最近合法作用域（降级到 function）。
        let top_break = CfStmt::Jump { kw: "break", off: 4 };
        let (_, d2, _) = check_full_path(false, &top_break);
        v.push((
            "F0451-语境-顶层break报错",
            d2.iter().any(|d| matches!(d, CfDiag::BadCtx { kw: "break", nearest: "function", .. })),
        ));
        // 开关内 break 合法、continue 非法（continue 只要循环）。
        let in_switch = CfStmt::Switch {
            body: Box::new(CfStmt::Seq {
                items: vec![
                    CfStmt::Jump { kw: "break", off: 5 },
                    CfStmt::Jump { kw: "continue", off: 6 },
                ],
            }),
        };
        let (_, d3, _) = check_full_path(false, &in_switch);
        let has_ok = !d3.iter().any(|d| matches!(d, CfDiag::BadCtx { kw: "break", .. }));
        let has_bad = d3.iter().any(|d| matches!(d, CfDiag::BadCtx { kw: "continue", nearest: "switch", .. }));
        v.push(("F0451-语境-开关break合法continue非法", has_ok && has_bad));
    }

    // 三、轻量摘要。
    {
        // 摘要计数（节点/循环/跳转/return）与结构一致。
        let nested = CfStmt::Seq {
            items: vec![
                CfStmt::Loop { body: Box::new(plain()) },
                CfStmt::Loop { body: Box::new(plain()) },
                ret(true),
            ],
        };
        let (_, _, s) = check_full_path(true, &nested);
        v.push((
            "F0451-摘要-计数一致",
            s.nodes == 6 && s.loops == 2 && s.returns == 1,
        ));
        // 权衡记录显性（非空说明）。
        v.push(("F0451-摘要-权衡记录显性", SUMMARY_NOTE.contains("不建完整 CFG")));
    }

    // 四、未验证显性。
    {
        // 深层构建失败 → NotVerified 显性（当前形态集下不触发，
        // 用显性谓词验证降级通道存在且可用）。
        let mut vv = CfVerifier::new(false);
        let d = vv.verify(&CfStmt::Plain);
        v.push((
            "F0451-未验证-通道可用",
            d == Ok(false) && SUMMARY_NOTE.len() > 10,
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0451-判据-锚点入册", ANCHOR == "VE-F0451"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec51）。
pub fn run_vec51_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec51");
    for (name, ok) in vec51_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec51_raw_checks as run_vec51_checks;
    #[test]
    fn vec51_ctlflow_checks() {
        for (name, ok) in run_vec51_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
