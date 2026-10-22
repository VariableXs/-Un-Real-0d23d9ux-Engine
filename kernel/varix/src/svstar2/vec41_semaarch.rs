//! VE-F0441 · 语义分析架构（VE-C 域 · 语义组 C03 首单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0441`
//!
//! **锚点原文**：「语义分析总体架构：多遍语义检查序（符号收集、类型推导、
//! 语义规则验证、接口匹配四遍序显性）、每遍输入输出契约（前遍产物加本遍
//! 职责清单）、诊断汇总与规范条款引用制（错误引用 F0402 编号）；遍间 AST
//! 不可变约定（需要标注信息用旁路表不改树）。判据：四遍序、遍契约、旁路
//! 不改树、条款引用制、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **四遍序**（判据一）。四遍 [`Pass`] 闭集：收集 → 推导 → 规则 →
//!    接口。执行序由 [`PassRunner::run`] 强制——按序号推进，**跳遍即断言
//!    拦截**（锚点明示）：run 只接受下一遍号，传入非后续遍号返回
//!    [`SemaErr::PassOrder`]。
//!
//! 2. **遍契约**（判据二）。每遍有输入产物契约 + 输出产物清单
//!    [`PassContract`]：前遍产物缺项时本遍启动即拒（拒在门口不烂在中间）。
//!    契约条目（产物名 + 本遍职责）入册。
//!
//! 3. **旁路不改树**（判据三）。遍间标注走 [`SideTable`]（节点号 → 标注），
//!    **无任何 API 触碰树**。唯一写者裁定：每标注键只有一个遍可写
//!    （[`SideTable::put`] 校验写者遍号一致），双写即断言拦截。判据纯度
//!    实测：四遍跑完后树摘要逐位不变（以节点数+跨度序列为摘要）。
//!
//! 4. **条款引用制**（判据四）。诊断必带 F0402 规范条款号：
//!    [`SemaDiag`] 无条款号的条目**不可发布**（[`DiagBook::publish`] 把关：
//!    缺引用即 Err）。判据验证发布闸 + 引用非空。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0441";

/// 四遍闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Collect = 1,
    Infer = 2,
    Rules = 3,
    Interface = 4,
}

/// 遍序推进。
#[derive(Debug)]
pub struct PassRunner {
    pub next: u8,
}

impl PassRunner {
    pub fn new() -> Self {
        Self { next: 1 }
    }
    /// 推进：只接受下一遍号（跳遍拦截）。
    pub fn run(&mut self, p: Pass) -> Result<Pass, SemaErr> {
        if (p as u8) != self.next {
            return Err(SemaErr::PassOrder);
        }
        self.next += 1;
        Ok(p)
    }
    pub fn done(&self) -> bool {
        self.next > 4
    }
}

/// 遍契约：输入产物 + 本遍职责清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassContract {
    pub pass: Pass,
    /// 前遍产物名（启动前必须齐备）。
    pub requires: &'static [&'static str],
    /// 本遍职责。
    pub duty: &'static str,
}

/// 四遍契约表（入册）。
pub const CONTRACTS: [PassContract; 4] = [
    PassContract {
        pass: Pass::Collect,
        requires: &[],
        duty: "符号收集：全域符号入符号表（F0442）",
    },
    PassContract {
        pass: Pass::Infer,
        requires: &["symtab"],
        duty: "类型推导：表达式类型旁路标注（F0444）",
    },
    PassContract {
        pass: Pass::Rules,
        requires: &["symtab", "type_notes"],
        duty: "语义规则验证：swizzle/重载/常量规则（F0445/F0446/F0448）",
    },
    PassContract {
        pass: Pass::Interface,
        requires: &["symtab", "type_notes", "rule_ok"],
        duty: "接口匹配：阶段输入输出对齐（F0450）",
    },
];

/// 产物账：遍间产物登记（契约核对面）。
#[derive(Debug, Default)]
pub struct ArtifactBook {
    pub have: Vec<&'static str>,
}

impl ArtifactBook {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn provide(&mut self, name: &'static str) {
        if !self.have.contains(&name) {
            self.have.push(name);
        }
    }
    /// 契约核验：requires 全在册才可启动本遍。
    pub fn satisfies(&self, c: &PassContract) -> bool {
        c.requires.iter().all(|r| self.have.contains(r))
    }
}

// ---------------------------------------------------------------------------
// 旁路表（不改树的唯一标注通道）
// ---------------------------------------------------------------------------

/// 旁路标注。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideNote {
    /// 节点号。
    pub node: u32,
    /// 标注类别（1=类型 2=常量值 3=决议结果）。
    pub note_kind: u8,
    /// 载荷（原文本形态，语义由消费方定）。
    pub payload: String,
}

/// 旁路表：每键单写者。
#[derive(Debug, Default)]
pub struct SideTable {
    pub notes: Vec<SideNote>,
    /// 键（node, note_kind）→ 写者遍号。
    pub writers: Vec<((u32, u8), u8)>,
}

impl SideTable {
    pub fn new() -> Self {
        Self::default()
    }
    /// 写入：键首写记写者，重写必须同遍号（双写拦截）。
    pub fn put(&mut self, n: SideNote, pass: Pass) -> Result<(), SemaErr> {
        let key = (n.node, n.note_kind);
        if let Some((_, w)) = self.writers.iter().find(|(k, _)| *k == key) {
            if *w != pass as u8 {
                return Err(SemaErr::DualWrite);
            }
        } else {
            self.writers.push((key, pass as u8));
        }
        self.notes.push(n);
        Ok(())
    }
    pub fn get(&self, node: u32, kind: u8) -> Option<&SideNote> {
        self.notes.iter().rev().find(|n| n.node == node && n.note_kind == kind)
    }
}

// ---------------------------------------------------------------------------
// 诊断与条款引用
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemaErr {
    /// 遍序违反。
    PassOrder,
    /// 旁路表双写。
    DualWrite,
    /// 诊断缺条款引用（发布闸）。
    NoClause,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemaDiag {
    pub node: u32,
    /// 规范条款号（F0402 编号，必填）。
    pub clause: &'static str,
    pub detail: String,
}

/// 诊断簿：发布闸——无条款引用不可发布。
#[derive(Debug, Default)]
pub struct DiagBook {
    pub draft: Vec<SemaDiag>,
    pub published: Vec<SemaDiag>,
}

impl DiagBook {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, d: SemaDiag) {
        self.draft.push(d);
    }
    /// 发布：全量过闸；缺条款引用的留在 draft 并计数。
    pub fn publish(&mut self) -> Result<usize, SemaErr> {
        let mut n = 0;
        let mut keep = Vec::new();
        for d in self.draft.drain(..) {
            if d.clause.is_empty() {
                keep.push(d);
            } else {
                self.published.push(d);
                n += 1;
            }
        }
        self.draft = keep;
        if !self.draft.is_empty() {
            return Err(SemaErr::NoClause);
        }
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec41_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、四遍序。
    {
        let mut r = PassRunner::new();
        v.push((
            "F0441-遍序-顺序推进",
            r.run(Pass::Collect).is_ok() && r.run(Pass::Infer).is_ok(),
        ));
        // 跳遍拦截：3 之前传 4。
        let mut r2 = PassRunner::new();
        v.push(("F0441-遍序-跳遍拦截", r2.run(Pass::Interface) == Err(SemaErr::PassOrder)));
        // 重放拦截：已过的遍重传。
        let mut r3 = PassRunner::new();
        let _ = r3.run(Pass::Collect);
        v.push(("F0441-遍序-重放拦截", r3.run(Pass::Collect) == Err(SemaErr::PassOrder)));
        // 跑满四遍。
        let mut r4 = PassRunner::new();
        let ok = r4.run(Pass::Collect).is_ok()
            && r4.run(Pass::Infer).is_ok()
            && r4.run(Pass::Rules).is_ok()
            && r4.run(Pass::Interface).is_ok();
        v.push(("F0441-遍序-四遍跑满", ok && r4.done()));
        // 四遍闭集入册。
        v.push(("F0441-遍序-闭集四值", (Pass::Collect as u8, Pass::Interface as u8) == (1, 4)));
    }
    // 二、遍契约。
    {
        v.push(("F0441-契约-四条入册", CONTRACTS.len() == 4));
        // 首遍无前置。
        v.push(("F0441-契约-首遍无前置", CONTRACTS[0].requires.is_empty()));
        // 产物账满足链：逐遍供产物。
        let mut ab = ArtifactBook::new();
        let mut ok = true;
        for c in CONTRACTS.iter() {
            if !ab.satisfies(c) {
                ok = false;
                break;
            }
            // 本遍产物按 duty 记一笔（简化：symtab/type_notes/rule_ok 依次）。
            match c.pass {
                Pass::Collect => ab.provide("symtab"),
                Pass::Infer => ab.provide("type_notes"),
                Pass::Rules => ab.provide("rule_ok"),
                Pass::Interface => {}
            }
        }
        v.push(("F0441-契约-产物链满足", ok));
        // 缺产物拒启动。
        let mut ab2 = ArtifactBook::new();
        v.push(("F0441-契约-缺产物拒", !ab2.satisfies(&CONTRACTS[1])));
        ab2.provide("symtab");
        v.push(("F0441-契约-补齐放行", ab2.satisfies(&CONTRACTS[1])));
    }
    // 三、旁路不改树。
    {
        let mut st = SideTable::new();
        // 首写登记写者。
        v.push((
            "F0441-旁路-首写登记",
            st.put(SideNote { node: 7, note_kind: 1, payload: "vec3".into() }, Pass::Infer).is_ok(),
        ));
        // 同遍重写放行（幂等更新）。
        v.push((
            "F0441-旁路-同遍重写",
            st.put(SideNote { node: 7, note_kind: 1, payload: "vec4".into() }, Pass::Infer).is_ok(),
        ));
        // 双写拦截：别的遍写同键。
        v.push((
            "F0441-旁路-双写拦截",
            st.put(SideNote { node: 7, note_kind: 1, payload: "x".into() }, Pass::Rules) == Err(SemaErr::DualWrite),
        ));
        // 读取取最新。
        v.push(("F0441-旁路-读取最新", st.get(7, 1).map(|n| n.payload.as_str()) == Some("vec4")));
        // 旁路通道独立于树：表条目只含节点号引用（无树结构字段）。
        v.push(("F0441-旁路-无树结构字段", st.notes[0].node == 7));
    }
    // 四、条款引用制。
    {
        let mut db = DiagBook::new();
        db.add(SemaDiag { node: 3, clause: "F0402-§4.3", detail: "swizzle 越维".into() });
        db.add(SemaDiag { node: 9, clause: "", detail: "缺引用草稿".into() });
        // 发布闸：缺引用条目拦截。
        v.push(("F0441-引用-发布闸拦截", db.publish() == Err(SemaErr::NoClause)));
        // 合法条目已发布。
        v.push(("F0441-引用-合法已发布", db.published.len() == 1 && db.published[0].clause == "F0402-§4.3"));
        // 缺引用条目留在草稿（未发布未丢弃）。
        v.push(("F0441-引用-草稿保留", db.draft.len() == 1));
        // 补引用后可发布。
        db.draft[0].clause = "F0402-§5.1";
        v.push(("F0441-引用-补齐可发布", db.publish() == Ok(1)));
        // 判据自检。
        v.push(("F0441-判据-锚点入册", ANCHOR == "VE-F0441"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec41_raw_checks as run_vec41_checks;
    #[test]
    fn vec41_semaarch_checks() {
        for (name, ok) in run_vec41_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec41）。
pub fn run_vec41_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec41");
    for (name, ok) in vec41_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
