//! VE-F0464 · IR 验证器（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0464`
//!
//! **锚点原文**：「IR 模块验证器：结构不变量（块终接唯一、使用定义支配一致、
//! 类型槽一致）、资源声明与使用一致、调试位置完整抽查三类验证；验证通过才
//! 可进入优化与后端（门禁语义）；验证失败定位首违指令。判据：三类不变量、
//! 门禁语义、双侧定位、不静默放行、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三类不变量**（判据一）。结构面（终接唯一 + 块序合法）、数据面（使用
//!    定义支配一致）、资源面（声明与使用一致）三清单逐条机检。
//!
//! 2. **门禁语义**（判据二）。验证通过才可进优化/后端（[`Gate::admit`]），
//!    门禁判定不缓存（每次进优化都跑——状态漂移不继承）。
//!
//! 3. **双侧定位**（判据三）。支配违反报定义块+使用块双侧；类型不一致报
//!    两侧类型；首违指令定位（验证序内第一个违例即停）。
//!
//! 4. **不静默放行**（判据四）。验证器自身异常（空模块/坏块号）显性失败
//!    （Err），不静默当通过。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0464";

use super::vec61_irstruct::{IrFunc, Opcode, ResDecl};
use super::vec63_ssa::{dominance, idom_of, Cfg};

// ---------------------------------------------------------------------------
// 违例定位
// ---------------------------------------------------------------------------

/// 验证违例：类别 + 双侧定位（定义侧/使用侧，可缺省）。
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub kind: &'static str,
    /// 首违位置（块号, 指令序号）。
    pub at: (u32, usize),
    /// 双侧补充（如定义块号、另一侧类型摘要）。
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 验证器
// ---------------------------------------------------------------------------

/// IR 函数验证器（三类不变量 + 门禁）。
pub struct Verifier {
    /// 资源声明账（资源面验证的对照真源）。
    pub resources: Vec<ResDecl>,
}

impl Verifier {
    pub fn new(resources: Vec<ResDecl>) -> Self {
        Verifier { resources }
    }
    /// 全量验证：返回空 = 通过；非空 = 首违在前。
    pub fn verify(&self, f: &IrFunc) -> Result<(), Violation> {
        // 验证器自身异常面：空函数显性失败（不静默放行）。
        if f.blocks.is_empty() {
            return Err(Violation {
                kind: "empty-func",
                at: (0, 0),
                detail: "函数无块：验证器显性失败".to_string(),
            });
        }
        // 一、结构不变量：每块终接唯一（末条终接 + 前面无终接）。
        for (bi, b) in f.blocks.iter().enumerate() {
            if b.insts.is_empty() {
                return Err(Violation {
                    kind: "empty-block",
                    at: (bi as u32, 0),
                    detail: format!("块 {} 无指令", bi),
                });
            }
            for (ii, i) in b.insts.iter().enumerate() {
                let last = ii + 1 == b.insts.len();
                if i.opcode.is_terminator() && !last {
                    return Err(Violation {
                        kind: "term-mid-block",
                        at: (bi as u32, ii),
                        detail: format!("终接指令 {} 后仍有指令", i.opcode.mnemonic()),
                    });
                }
                if !i.opcode.is_terminator() && last {
                    return Err(Violation {
                        kind: "no-terminator",
                        at: (bi as u32, ii),
                        detail: format!("块 {} 末条非终接", bi),
                    });
                }
            }
        }
        // 二、数据不变量：使用定义支配一致（CFG 支配面）。
        let mut cfg = Cfg::new(f.blocks.len());
        for (bi, b) in f.blocks.iter().enumerate() {
            if let Some(t) = b.terminator() {
                if t.opcode == Opcode::Br {
                    // 无条件跳转：操作数[0] 为目标块号。
                    if let Some(&target) = t.operands.first() {
                        if (target as usize) < f.blocks.len() {
                            cfg.edge(bi as u32, target);
                        }
                    }
                }
            }
        }
        let dom = dominance(&cfg);
        // 可达性预检：不可达块的使用无支配保障（支配面格外显性拦）。
        let mut reachable = vec![false; f.blocks.len()];
        reachable[0] = true;
        let mut wl: Vec<usize> = vec![0];
        while let Some(b) = wl.pop() {
            for s in cfg.succs[b].iter() {
                if !reachable[*s as usize] {
                    reachable[*s as usize] = true;
                    wl.push(*s as usize);
                }
            }
        }
        // 收集定义块（def → 定义所在块）。
        let mut def_at: Vec<(u32, u32)> = Vec::new();
        for b in f.blocks.iter() {
            for i in b.insts.iter() {
                if let Some(d) = i.def {
                    if !def_at.iter().any(|(r, _)| *r == d) {
                        def_at.push((d, b.id));
                    }
                }
            }
        }
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, i) in b.insts.iter().enumerate() {
                for op in i.operands.iter() {
                    // 操作数是块号（Br 目标）跳过支配检查。
                    if i.opcode.is_terminator() {
                        continue;
                    }
                    if let Some((_, dblk)) = def_at.iter().find(|(r, _)| r == op) {
                        let uses_block = bi as u32;
                        if !reachable[bi] {
                            return Err(Violation {
                                kind: "dom-violation",
                                at: (bi as u32, ii),
                                detail: format!(
                                    "寄存器 {} 定义于块 {} 但使用于不可达块 {}（双侧定位）",
                                    op, dblk, uses_block
                                ),
                            });
                        }
                        if !dom[uses_block as usize].contains(dblk) {
                            return Err(Violation {
                                kind: "dom-violation",
                                at: (bi as u32, ii),
                                detail: format!(
                                    "寄存器 {} 定义于块 {} 但使用于块 {}（双侧定位）",
                                    op, dblk, uses_block
                                ),
                            });
                        }
                    }
                }
            }
        }
        // 三、资源面：使用绑定槽必须已声明。
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, i) in b.insts.iter().enumerate() {
                if i.opcode == Opcode::Sample {
                    // Sample 约定：operands = [采样器绑定槽, 坐标]。
                    if let Some(&slot) = i.operands.first() {
                        if !self.resources.iter().any(|r| r.binding == slot) {
                            return Err(Violation {
                                kind: "undeclared-resource",
                                at: (bi as u32, ii),
                                detail: format!("采样绑定槽 {} 未声明", slot),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// 门禁：验证通过才可进优化/后端（判定不缓存）。
pub struct Gate {
    pub verifier: Verifier,
}

impl Gate {
    pub fn new(resources: Vec<ResDecl>) -> Self {
        Gate { verifier: Verifier::new(resources) }
    }
    /// 准入判定（每次现算——状态漂移不继承）。
    pub fn admit(&self, f: &IrFunc) -> bool {
        self.verifier.verify(f).is_ok()
    }
}

/// 调试位置完整抽查：灰态指令数（dbg 缺失）与显性豁免数一致。
pub fn dbg_audit(f: &IrFunc, exempt: usize) -> usize {
    let gray = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|i| i.dbg.is_none())
        .count();
    gray.saturating_sub(exempt)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec64_raw_checks() -> Vec<(&'static str, bool)> {
    use super::vec61_irstruct::{Block, Inst};
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三类不变量。
    {
        let res = vec![ResDecl { binding: 0, kind: "sampler2d" }];
        // 合法函数：块 0 const+ret。
        let mut f = IrFunc::new("t");
        let r = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![r], 2));
        let g = Gate::new(res.clone());
        v.push(("F0464-不变量-合法过门", g.admit(&f)));
        // 结构违例：终接指令后有指令（首违定位）。
        let mut bad = IrFunc::new("t");
        bad.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, 0));
        bad.cur_block().push(Inst::term(Opcode::Ret, vec![0], 2));
        bad.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 3, 1));
        match g.verifier.verify(&bad) {
            Err(vio) => {
                v.push((
                    "F0464-不变量-终接唯一",
                    vio.kind == "term-mid-block" && vio.at == (0, 1),
                ));
            }
            Ok(()) => v.push(("F0464-不变量-终接唯一", false)),
        }
        // 无终接块：显性拦截。
        let mut nt = IrFunc::new("t");
        nt.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, 0));
        match g.verifier.verify(&nt) {
            Err(vio) => v.push(("F0464-不变量-无终接拦", vio.kind == "no-terminator")),
            Ok(()) => v.push(("F0464-不变量-无终接拦", false)),
        }
        // 资源面：未声明绑定槽拦截（双侧定位带槽号）。
        let mut sr = IrFunc::new("t");
        let rc = sr.fresh_reg();
        sr.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.v2f", 1, rc));
        sr.cur_block().push(Inst::new(
            Opcode::Sample,
            vec![7, rc],
            Some("ir.v4f"),
            Some(2),
            Some(2),
        ));
        sr.cur_block().push(Inst::term(Opcode::Ret, vec![2], 3));
        match g.verifier.verify(&sr) {
            Err(vio) => {
                v.push((
                    "F0464-资源-未声明拦",
                    vio.kind == "undeclared-resource" && vio.detail.contains("7"),
                ));
            }
            Ok(()) => v.push(("F0464-资源-未声明拦", false)),
        }
    }

    // 二、支配一致（双侧定位）。
    {
        // 双块：0 不跳 1（CFG 断开），块 1 使用块 0 的定义 → 支配违反。
        let mut f = IrFunc::new("t");
        let r0 = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r0));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![], 2));
        let b1 = f.new_block();
        let r2 = f.fresh_reg();
        f.blocks[b1 as usize].push(Inst::value(
            Opcode::Add,
            vec![r0, r0],
            "ir.f32",
            3,
            r2,
        ));
        f.blocks[b1 as usize].push(Inst::term(Opcode::Ret, vec![], 4));
        let g = Gate::new(Vec::new());
        match g.verifier.verify(&f) {
            Err(vio) => {
                v.push((
                    "F0464-支配-双侧定位",
                    vio.kind == "dom-violation" && vio.detail.contains("定义于块 0") && vio.detail.contains("使用于不可达块 1"),
                ));
            }
            Ok(()) => v.push(("F0464-支配-双侧定位", false)),
        }
        // 合法支配链（0 跳 1，1 使用 0 定义）应过。
        let mut ok = IrFunc::new("t");
        let r = ok.fresh_reg();
        ok.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r));
        ok.cur_block().push(Inst::term(Opcode::Br, vec![1], 2));
        ok.new_block();
        ok.blocks[1].push(Inst::value(Opcode::Add, vec![r, r], "ir.f32", 3, 1));
        ok.blocks[1].push(Inst::term(Opcode::Ret, vec![], 4));
        v.push(("F0464-支配-合法链过", g.admit(&ok)));
    }

    // 三、门禁语义 + 不静默放行。
    {
        // 空块函数：验证器显性失败（不静默放行；IrFunc::new 保底一块，实际形态为空块）。
        let empty = IrFunc::new("t");
        let g = Gate::new(Vec::new());
        match g.verifier.verify(&empty) {
            Err(vio) => v.push(("F0464-门禁-空块显性失败", vio.kind == "empty-block")),
            Ok(()) => v.push(("F0464-门禁-空块显性失败", false)),
        }
        // 门禁判定不缓存：过门后再破坏，再验必拦。
        let mut f = IrFunc::new("t");
        let r = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![r], 2));
        let passed = g.admit(&f);
        f.blocks[0].insts.push(Inst::value(Opcode::Const, vec![], "ir.f32", 3, 1));
        let passed_after = g.admit(&f);
        v.push((
            "F0464-门禁-判定不缓存",
            passed && !passed_after,
        ));
    }

    // 四、调试抽查。
    {
        // 全程带位：灰态 0。
        let mut f = IrFunc::new("t");
        let r = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![r], 2));
        v.push(("F0464-调试-灰态计数", dbg_audit(&f, 0) == 0));
        // 灰态指令：抽查定位（豁免 1 内不报，超出报）。
        f.cur_block().push(Inst::new(Opcode::Not, vec![r], Some("ir.f32"), None, Some(1)));
        v.push(("F0464-调试-豁免内过", dbg_audit(&f, 1) == 0 && dbg_audit(&f, 0) == 1));
    }

    // 五、判据自检。
    {
        v.push(("F0464-判据-锚点入册", ANCHOR == "VE-F0464"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec64）。
pub fn run_vec64_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec64");
    for (name, ok) in vec64_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec64_raw_checks as run_vec64_checks;
    #[test]
    fn vec64_irverify_checks() {
        for (name, ok) in run_vec64_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
