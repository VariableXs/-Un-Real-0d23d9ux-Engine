//! VE-F0588 · 多文件项目热编译
//!
//! include 图的编辑态增量维护（基于 F0414 解析产物，增删文件与边即时更新）、
//! 循环包含拒绝（构建期与编辑态同规则）、变更传播：单文件变更按依赖闭包失效
//! （F0582 级联的多文件扩展——下游文件全部失效、上游复用）、跨文件符号变更的
//! 语义级失效（签名变更触发下游语义重算）；项目级缓存一致性：多文件产物缓存的
//! 项目指纹联合校验。
//!
//! 跨批对接：上游 F0414 include、F0582 级联；下游 F0590 缓存、F0584 级联回滚。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// include 图（编辑态增量维护）
#[derive(Clone, Debug, Default)]
pub struct IncludeGraph {
    pub files: Vec<String>,
    /// 有向边：from include to
    pub edges: Vec<(String, String)>,
}

impl IncludeGraph {
    pub fn add_file(&mut self, f: &str) -> bool {
        if f.is_empty() || self.files.iter().any(|x| x == f) {
            return false;
        }
        self.files.push(f.to_string());
        true
    }

    pub fn remove_file(&mut self, f: &str) -> bool {
        if !self.files.iter().any(|x| x == f) {
            return false;
        }
        self.files.retain(|x| x != f);
        self.edges.retain(|(a, b)| a != f && b != f);
        true
    }

    /// 边即时更新：增边
    pub fn add_edge(&mut self, from: &str, to: &str) -> bool {
        if from == to || !self.files.contains(&from.to_string()) || !self.files.contains(&to.to_string()) {
            return false;
        }
        if self.edges.iter().any(|(a, b)| a == from && b == to) {
            return false;
        }
        self.edges.push((from.to_string(), to.to_string()));
        true
    }

    pub fn remove_edge(&mut self, from: &str, to: &str) -> bool {
        let n = self.edges.len();
        self.edges.retain(|(a, b)| !(a == from && b == to));
        self.edges.len() != n
    }

    /// 循环包含拒绝：DFS 检测环
    pub fn has_cycle(&self) -> bool {
        fn dfs(g: &IncludeGraph, cur: &str, stack: &mut Vec<String>) -> bool {
            if stack.iter().any(|s| s == cur) {
                return true;
            }
            stack.push(cur.to_string());
            for (a, b) in &g.edges {
                if a == cur && dfs(g, b, stack) {
                    return true;
                }
            }
            stack.pop();
            false
        }
        for f in &self.files {
            if dfs(self, f, &mut Vec::new()) {
                return true;
            }
        }
        false
    }

    /// 依赖闭包：变更文件的下游全失效
    pub fn downstream_closure(&self, changed: &str) -> Vec<String> {
        let mut seen = Vec::new();
        let mut queue = vec![changed.to_string()];
        while let Some(cur) = queue.pop() {
            for (a, b) in &self.edges {
                if *a == cur && !seen.contains(b) {
                    seen.push(b.clone());
                    queue.push(b.clone());
                }
            }
        }
        seen
    }

    /// 上游复用集合
    pub fn upstream(&self, changed: &str) -> Vec<String> {
        self.files.iter().filter(|f| !self.downstream_closure(f).contains(&changed.to_string()) && *f != changed).cloned().collect()
    }
}

/// 语义级失效：签名变更触发下游语义重算
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 仅正文变化：词法/语法级失效
    BodyEdit,
    /// 签名变化：下游语义级失效
    SignatureChange,
}

/// 变更传播结果
#[derive(Clone, Debug)]
pub struct Propagation {
    /// 失效文件（含语义级标记）
    pub invalidated: Vec<(String, bool)>,
}

impl Propagation {
    pub fn semantic_affected(&self) -> Vec<String> {
        self.invalidated.iter().filter(|(_, sem)| *sem).map(|(f, _)| f.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.invalidated.len()
    }

    pub fn is_empty(&self) -> bool {
        self.invalidated.is_empty()
    }
}

/// 传播执行
pub fn propagate(graph: &IncludeGraph, changed: &str, kind: ChangeKind) -> Propagation {
    let downstream = graph.downstream_closure(changed);
    let invalidated = downstream
        .into_iter()
        .map(|f| {
            let sem = kind == ChangeKind::SignatureChange;
            (f, sem)
        })
        .collect();
    Propagation { invalidated }
}

/// 项目指纹：多文件产物缓存的项目级联合校验
#[derive(Clone, Debug, Default)]
pub struct ProjectFingerprint {
    /// 文件 → 内容哈希
    pub hashes: Vec<(String, u64)>,
}

impl ProjectFingerprint {
    pub fn update(&mut self, file: &str, hash: u64) -> bool {
        if file.is_empty() {
            return false;
        }
        match self.hashes.iter_mut().find(|(f, _)| f == file) {
            Some(e) => {
                e.1 = hash;
                true
            }
            None => {
                self.hashes.push((file.to_string(), hash));
                true
            }
        }
    }

    pub fn get(&self, file: &str) -> Option<u64> {
        self.hashes.iter().find(|(f, _)| f == file).map(|(_, h)| *h)
    }

    /// 联合校验：所有登记文件哈希一致
    pub fn joint_check(&self, other: &ProjectFingerprint) -> bool {
        self.hashes.iter().all(|(f, h)| other.get(f).map(|oh| oh == *h).unwrap_or(false))
    }

    /// 指纹失配 → 项目级失效
    pub fn mismatch_files(&self, other: &ProjectFingerprint) -> Vec<String> {
        self.hashes
            .iter()
            .filter(|(f, h)| other.get(f).map(|oh| oh != *h).unwrap_or(true))
            .map(|(f, _)| f.clone())
            .collect()
    }
}

pub fn run_vec188_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec188");
    let mut g = IncludeGraph::default();
    // 族一：图增量维护
    s.add("图-增文件", g.add_file("a.wg") && g.add_file("b.wg") && g.add_file("c.wg"), "addf");
    s.add("图-重复拒绝", !g.add_file("a.wg"), "dup");
    s.add("图-增边", g.add_edge("a.wg", "b.wg") && g.add_edge("b.wg", "c.wg"), "adde");
    s.add("图-重复边拒绝", !g.add_edge("a.wg", "b.wg"), "dupe");
    s.add("图-自环拒绝", !g.add_edge("a.wg", "a.wg"), "self");
    s.add("图-未登记边拒绝", !g.add_edge("a.wg", "z.wg"), "unk");
    s.add("图-删边", g.remove_edge("b.wg", "c.wg") && !g.remove_edge("b.wg", "c.wg"), "rme");
    g.add_edge("b.wg", "c.wg");
    // 族二：循环包含拒绝
    s.add("环-无环", !g.has_cycle(), "acyclic");
    g.add_edge("c.wg", "a.wg");
    s.add("环-成环检出", g.has_cycle(), "cycle");
    g.remove_edge("c.wg", "a.wg");
    s.add("环-删边解环", !g.has_cycle(), "fixed");
    // 族三：闭包传播
    let down = g.downstream_closure("a.wg");
    s.add("闭包-下游全失效", down.contains(&"b.wg".to_string()) && down.contains(&"c.wg".to_string()), "down");
    s.add("闭包-不含自身", !down.contains(&"a.wg".to_string()), "nself");
    // 族四：传播结果
    let p1 = propagate(&g, "a.wg", ChangeKind::BodyEdit);
    s.add("传播-正文编辑失效", p1.len() == 2 && p1.semantic_affected().is_empty(), "body");
    let p2 = propagate(&g, "a.wg", ChangeKind::SignatureChange);
    s.add("传播-签名语义级失效", p2.semantic_affected() == vec!["b.wg".to_string(), "c.wg".to_string()], "sem");
    s.add("传播-孤立文件零传播", propagate(&g, "c.wg", ChangeKind::SignatureChange).len() == 0, "iso");
    // 族五：上游复用
    let up = g.upstream("c.wg");
    s.add("上游-复用集合", up.contains(&"a.wg".to_string()) && !up.contains(&"c.wg".to_string()), "up");
    // 族六：项目指纹
    let mut pf1 = ProjectFingerprint::default();
    pf1.update("a.wg", 111);
    pf1.update("b.wg", 222);
    let mut pf2 = ProjectFingerprint::default();
    pf2.update("a.wg", 111);
    pf2.update("b.wg", 222);
    s.add("指纹-联合校验一致", pf1.joint_check(&pf2), "joint");
    pf2.update("b.wg", 333);
    s.add("指纹-失配检出", !pf1.joint_check(&pf2) && pf1.mismatch_files(&pf2) == vec!["b.wg".to_string()], "mismatch");
    s.add("指纹-单文件哈希", pf1.get("a.wg") == Some(111), "get");
    s.add("指纹-空文件拒绝", !pf1.update("", 1), "empty");
    // 族七：删文件即时维护
    g.remove_file("c.wg");
    s.add("删文-文件移除", !g.files.contains(&"c.wg".to_string()), "rmf");
    s.add("删文-邻边清理", !g.edges.iter().any(|(_, b)| b == "c.wg"), "rme2");
    s.add("删文-未登记拒绝", !g.remove_file("z.wg"), "rmunk");
    s
}
