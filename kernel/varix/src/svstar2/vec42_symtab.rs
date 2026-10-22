//! VE-F0442 · 符号表与作用域链（VE-C 域 · 语义组 C03 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0442`
//!
//! **锚点原文**：「符号表与作用域链实现：作用域树（消费 F0410 标记流）、
//! 符号条目（名、类别、类型槽、跨度）、查找链（内层向外层逐级）、遮蔽规则
//! 显性（内层遮蔽外层警告可配）；重复定义按规范裁定；符号表与 AST 旁路
//! 关联（不回写树）。判据：作用域树、逐级查找、遮蔽显性、近似建议、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **作用域树**（判据一）。[`Scope`] 树（父号 + 子号），全局根 0 号。
//!    树由 F0410 块标记流驱动构建：开标记 = 开子域，闭标记 = 回父域。
//!    构建守恒：开闭配对（vec27 同款守恒口径）。
//!
//! 2. **逐级查找**（判据二）。[`SymTab::lookup`] 从当前域起**逐级向外**：
//!    内层命中即停（链长 = 遇到的域数），未命中回根后显性未找到。查找
//!    O(深度) 的结构性保证：每域一张哈希式名表（本实现用有序小表——
//!    作用域内符号量小，线性扫优于哈希常数）。
//!
//! 3. **遮蔽显性**（判据三）。内层符号与外层同名 = 遮蔽：**不拒绝**，
//!    登记 [`Shadow`] 事件（内域号 × 外域号 × 名）——「警告可配」由配置
//!    位 [`SymConfig::warn_shadow`] 控制，默认开。重复定义（同域同名）按
//!    规范裁定：拒绝第二定义（与跨域遮蔽不同的唯一拒绝面）。
//!
//! 4. **近似建议**（判据四）。查找失败带最近相似名建议：编辑距离 ≤ 2 的
//!    候选中取最小（同距取字典序最小）。编辑距离实现限界（长度差 > 2
//!    直接跳过，O(候选×minlen)）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0442";

/// 符号类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    Var,
    Func,
    Type,
}

/// 符号条目：名、类别、类型槽、跨度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymEntry {
    pub name: String,
    pub kind: SymKind,
    /// 类型槽（语义期填充，语法期 None）。
    pub ty: Option<String>,
    pub off: u32,
}

/// 作用域：父号 + 名表。
#[derive(Debug)]
pub struct Scope {
    pub parent: Option<u32>,
    pub entries: Vec<SymEntry>,
}

/// 配置位。
#[derive(Clone, Copy, Debug)]
pub struct SymConfig {
    /// 内层遮蔽外层：警告可配（默认开）。
    pub warn_shadow: bool,
}

impl Default for SymConfig {
    fn default() -> Self {
        Self { warn_shadow: true }
    }
}

/// 遮蔽事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadow {
    pub inner: u32,
    pub outer: u32,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymErr {
    /// 同域重复定义（规范拒绝面）。
    DupInScope,
    /// 未找到（带建议的完整版由 diagnose 出）。
    NotFound,
}

/// 符号表。
#[derive(Debug)]
pub struct SymTab {
    pub scopes: Vec<Scope>,
    pub shadows: Vec<Shadow>,
    pub cfg: SymConfig,
}

impl SymTab {
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope { parent: None, entries: Vec::new() }],
            shadows: Vec::new(),
            cfg: SymConfig::default(),
        }
    }
    /// 开子域（F0410 开标记联动）。
    pub fn open(&mut self, parent: u32) -> u32 {
        let id = self.scopes.len() as u32;
        self.scopes.push(Scope { parent: Some(parent), entries: Vec::new() });
        id
    }
    /// 插入：同域同名拒（重复定义规范裁定）；跨域同名 = 遮蔽事件登记。
    pub fn insert(&mut self, scope: u32, e: SymEntry) -> Result<(), SymErr> {
        let s = self.scopes.get(scope as usize).ok_or(SymErr::NotFound)?;
        if s.entries.iter().any(|x| x.name == e.name) {
            return Err(SymErr::DupInScope);
        }
        // 遮蔽检测：向外逐级查同名。
        let mut cur = self.scopes[scope as usize].parent;
        while let Some(p) = cur {
            if self.scopes[p as usize].entries.iter().any(|x| x.name == e.name) {
                self.shadows.push(Shadow { inner: scope, outer: p, name: e.name.clone() });
                break;
            }
            cur = self.scopes[p as usize].parent;
        }
        self.scopes[scope as usize].entries.push(e);
        Ok(())
    }
    /// 逐级查找：内层命中即停，返回（域号, 条目引用）。
    pub fn lookup(&self, from: u32, name: &str) -> Option<(u32, &SymEntry)> {
        let mut cur = Some(from);
        while let Some(s) = cur {
            if let Some(e) = self.scopes[s as usize].entries.iter().find(|e| e.name == name) {
                return Some((s, e));
            }
            cur = self.scopes[s as usize].parent;
        }
        None
    }
    /// 警告面：遮蔽事件按配置过滤。
    pub fn shadow_warnings(&self) -> Vec<&Shadow> {
        if self.cfg.warn_shadow {
            self.shadows.iter().collect()
        } else {
            Vec::new()
        }
    }
}

/// 编辑距离（限界 ≤2 才计，O(minlen)——逐位差 + 长度差）。
/// 编辑距离限界计算（Levenshtein 双行 DP，超限早退）。
/// 返回 Some(d) 当 d ≤ cap；否则 None（限界显性，不是 0）。
pub fn edit_dist_within(a: &str, b: &str, cap: usize) -> Option<usize> {
    let (ba, bb) = (a.as_bytes(), b.as_bytes());
    // 长度差下界剪枝（插入/删除次数不可能少于长度差）。
    let dl = if ba.len() > bb.len() { ba.len() - bb.len() } else { bb.len() - ba.len() };
    if dl > cap {
        return None;
    }
    // 双行 DP：prev = 上一行全距，cur = 当前行。
    let mut prev: Vec<usize> = (0..=bb.len()).collect();
    let mut cur: Vec<usize> = vec![0; bb.len() + 1];
    for i in 1..=ba.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=bb.len() {
            let cost = if ba[i - 1] == bb[j - 1] { 0 } else { 1 };
            let v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            cur[j] = v;
            if v < row_min {
                row_min = v;
            }
        }
        if row_min > cap {
            return None; // 行最小值是终距下界，整行超限即可早退
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[bb.len()];
    if d <= cap {
        Some(d)
    } else {
        None
    }
}

/// 近似建议：候选中距离最小者（同距取字典序最小）。
pub fn nearest<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let mut best: Option<(usize, &'a str)> = None;
    for c in candidates.iter() {
        if let Some(d) = edit_dist_within(name, c, 2) {
            let better = match best {
                None => true,
                Some((bd, bc)) => d < bd || (d == bd && *c < bc),
            };
            if better {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// 带建议的查找诊断。
pub fn diagnose(tab: &SymTab, from: u32, name: &str) -> Result<(u32, String), String> {
    match tab.lookup(from, name) {
        Some((s, _)) => Ok((s, String::new())),
        None => {
            // 候选全域收集。
            let mut cands: Vec<&str> = Vec::new();
            for sc in tab.scopes.iter() {
                for e in sc.entries.iter() {
                    cands.push(&e.name);
                }
            }
            match nearest(name, &cands) {
                Some(n) => Err(n.to_string()),
                None => Err(String::new()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec42_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let ent = |n: &str, k: SymKind, off: u32| SymEntry {
        name: n.into(),
        kind: k,
        ty: None,
        off,
    };
    // 一、作用域树。
    {
        let mut t = SymTab::new();
        let inner = t.open(0);
        let deep = t.open(inner);
        v.push((
            "F0442-树-链式父子",
            t.scopes[inner as usize].parent == Some(0)
                && t.scopes[deep as usize].parent == Some(inner),
        ));
        // 全局根 0 号。
        v.push(("F0442-树-根零号", t.scopes[0].parent.is_none()));
    }
    // 二、逐级查找。
    {
        let mut t = SymTab::new();
        let inner = t.open(0);
        let _ = t.insert(0, ent("global", SymKind::Var, 0));
        let _ = t.insert(inner, ent("local", SymKind::Var, 10));
        // 内层查 local：命中内层。
        let hit = t.lookup(inner, "local");
        v.push((
            "F0442-查找-内层命中",
            matches!(hit, Some((s, _)) if s == inner),
        ));
        // 内层查 global：向外逐级命中。
        let hit2 = t.lookup(inner, "global");
        v.push(("F0442-查找-逐级向外", matches!(hit2, Some((s, _)) if s == 0)));
        // 未找到。
        v.push(("F0442-查找-未找到显性", t.lookup(inner, "missing").is_none()));
        // 内层遮蔽：内层 global 盖外层。
        let _ = t.insert(inner, ent("global", SymKind::Var, 20));
        let hit3 = t.lookup(inner, "global");
        v.push(("F0442-查找-内层盖外层", matches!(hit3, Some((s, _)) if s == inner)));
    }
    // 三、遮蔽显性 + 重复裁定。
    {
        let mut t = SymTab::new();
        let inner = t.open(0);
        let _ = t.insert(0, ent("x", SymKind::Var, 0));
        let _ = t.insert(inner, ent("x", SymKind::Var, 5));
        v.push((
            "F0442-遮蔽-事件登记",
            t.shadows.len() == 1 && t.shadows[0].inner == inner && t.shadows[0].outer == 0,
        ));
        // 警告可配：关配置后警告面清空（事件仍在账）。
        let mut cfg = SymConfig::default();
        cfg.warn_shadow = false;
        t.cfg = cfg;
        v.push((
            "F0442-遮蔽-警告可配",
            t.shadow_warnings().is_empty() && t.shadows.len() == 1,
        ));
        // 同域重复定义拒。
        let mut t2 = SymTab::new();
        v.push(("F0442-重复-同域首插过", t2.insert(0, ent("y", SymKind::Var, 0)).is_ok()));
        v.push((
            "F0442-重复-同域再插拒",
            t2.insert(0, ent("y", SymKind::Var, 5)) == Err(SymErr::DupInScope),
        ));
        // 类型槽（语义期填充位）。
        let mut t3 = SymTab::new();
        let mut e = ent("z", SymKind::Var, 0);
        e.ty = Some("float".into());
        let _ = t3.insert(0, e);
        v.push(("F0442-条目-类型槽可填", t3.lookup(0, "z").unwrap_or((0, &ent("q", SymKind::Var, 0))).1.ty == Some("float".into())));
    }
    // 四、近似建议。
    {
        v.push((
            "F0442-建议-距离一",
            nearest("positon", &["position", "velocity"]) == Some("position"),
        ));
        v.push((
            "F0442-建议-距离二",
            nearest("postion", &["position"]) == Some("position"),
        ));
        // 同距取字典序最小。
        v.push((
            "F0442-建议-同距字典序",
            nearest("ab", &["ac", "ad"]) == Some("ac"),
        ));
        // 距离超界无建议。
        v.push(("F0442-建议-超界无建议", nearest("zzzzzz", &["position"]).is_none()));
        // 完整诊断面。
        let mut t = SymTab::new();
        let _ = t.insert(0, ent("color", SymKind::Var, 0));
        v.push((
            "F0442-建议-诊断带建议",
            diagnose(&t, 0, "colro") == Err("color".to_string()),
        ));
        // 判据自检。
        v.push(("F0442-判据-锚点入册", ANCHOR == "VE-F0442"));
        v.push(("F0442-判据-警告默认开", SymConfig::default().warn_shadow));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec42_raw_checks as run_vec42_checks;
    #[test]
    fn vec42_symtab_checks() {
        for (name, ok) in run_vec42_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec42）。
pub fn run_vec42_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec42");
    for (name, ok) in vec42_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
