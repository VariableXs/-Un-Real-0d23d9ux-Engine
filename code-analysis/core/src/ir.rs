//! 项目加载与统一语义 IR 构建（#001 承接 + core 输出契约）。
//!
//! 七级：项目→模块→子系统→文件→类→函数→行。
//! 模块/子系统层由目录结构推导（root/mod/sub/file）。

use crate::model::{Edge, EdgeKind, Node, NodeKind, ProjectIR, StmtKind, Tree};
use crate::parser::{lang_of, parse_file};
use std::fs;
use std::path::Path;

/// 打开项目目录，构建 ProjectIR（部署总纲 `open_project(root) -> ProjectIR`）。
pub fn open_project(root: &Path) -> std::io::Result<ProjectIR> {
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let mut ir = ProjectIR {
        name: name.clone(),
        tree: Tree::with_root(&name, NodeKind::Project),
        ..Default::default()
    };

    // 模块级（一级子目录）分组
    let mut mod_ids: Vec<(String, usize)> = Vec::new();
    let mut pending_edges: Vec<Edge> = Vec::new();
    let entries = collect_files(root)?;
    for (rel, full) in entries {
        let parts: Vec<&str> = rel.split(['/', '\\']).collect();
        ir.file_count += 1;

        let src = fs::read_to_string(&full).unwrap_or_default();
        ir.loc += src.lines().count();
        let lang = lang_of(&rel).to_string();
        match ir.lang_stats.iter_mut().find(|(l, _)| *l == lang) {
            Some((_, c)) => *c += 1,
            None => ir.lang_stats.push((lang, 1)),
        }

        // 模块节点（一级目录）与子系统节点（二级目录）
        let mod_name = if parts.len() > 1 {
            parts[0].to_string()
        } else {
            "(root)".to_string()
        };
        let iroot = ir.tree.root;
        let mod_id = ensure_child(&mut ir, iroot, &mod_name, NodeKind::Module, &mut mod_ids);
        let file_parent = if parts.len() > 2 {
            let sub = parts[parts.len() - 2].to_string();
            let mut cache: Vec<(String, usize)> = Vec::new();
            ensure_child(&mut ir, mod_id, &sub, NodeKind::Subsystem, &mut cache)
        } else {
            mod_id
        };

        // 文件 AST
        let ftree = parse_file(parts[parts.len() - 1], &src);
        let file_parent = if parts.len() > 2 { file_parent } else { file_parent };
        let file_id = ir.nodes_len();
        remap_subtree(&mut ir, &ftree, ftree.root, file_parent, &rel);

        // 收集跨文件调用边：func@file → callee
        let mut edges = Vec::new();
        collect_edges(&ir.tree, file_id, &rel, &mut edges);
        pending_edges.append(&mut edges);
    }
    ir.calls = pending_edges;
    Ok(ir)
}

fn ensure_child(
    ir: &mut ProjectIR,
    parent: usize,
    name: &str,
    kind: NodeKind,
    cache: &mut Vec<(String, usize)>,
) -> usize {
    if let Some((_, id)) = cache.iter().find(|(n, _)| n == name) {
        // 校验确为该 parent 的孩子
        if ir.tree.nodes[parent].children.contains(id) {
            return *id;
        }
    }
    let id = ir.tree.add(parent, Node::new(0, kind, name, (0, 0)));
    cache.push((name.to_string(), id));
    id
}

fn remap_subtree(
    ir: &mut ProjectIR,
    ftree: &Tree,
    id: usize,
    parent: usize,
    file_rel: &str,
) -> usize {
    let n = &ftree.nodes[id];
    let mut nn = Node::new(0, n.kind, n.name.clone(), n.span);
    nn.stmt = n.stmt;
    nn.text = n.text.clone();
    nn.callee = n.callee.clone();
    nn.target = n.target.clone();
    if n.kind == NodeKind::File {
        nn.name = file_rel.to_string();
    }
    let new_id = ir.tree.add(parent, nn);
    for &c in &n.children {
        remap_subtree(ir, ftree, c, new_id, file_rel);
    }
    new_id
}

fn collect_files(root: &Path) -> std::io::Result<Vec<(String, std::path::PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for e in fs::read_dir(&dir)? {
            let p = e?.path();
            if p.is_dir() {
                // 跳过构建产物与依赖目录
                let dn = p.file_name().unwrap().to_string_lossy().to_string();
                if matches!(dn.as_str(), "target" | "node_modules" | ".git" | "dist" | "build") {
                    continue;
                }
                let sub = if rel.is_empty() {
                    dn
                } else {
                    format!("{rel}/{dn}")
                };
                stack.push((p.clone(), sub));
            } else {
                let r = if rel.is_empty() {
                    p.file_name().unwrap().to_string_lossy().to_string()
                } else {
                    format!("{}/{}", rel, p.file_name().unwrap().to_string_lossy())
                };
                out.push((r, p));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn collect_edges(tree: &Tree, file_id: usize, _file_rel: &str, out: &mut Vec<Edge>) {
    // 遍历该文件子树，为函数级调用建立 from=func@file, to=callee。
    let mut cur_func = String::new();
    let mut stack = vec![file_id];
    while let Some(id) = stack.pop() {
        let n = &tree.nodes[id];
        if n.kind == NodeKind::Func {
            cur_func = format!("{}@{}", n.name, _file_rel);
        }
        if n.stmt == Some(StmtKind::Call) {
            if let Some(callee) = &n.callee {
                out.push(Edge {
                    from: cur_func.clone(),
                    to: callee.clone(),
                    kind: EdgeKind::Call,
                });
            }
        }
        for &c in &n.children {
            stack.push(c);
        }
    }
}

impl ProjectIR {
    pub fn nodes_len(&self) -> usize {
        self.tree.nodes.len()
    }

    /// 找到指定全名的函数节点。
    pub fn find_func(&self, name: &str) -> Option<usize> {
        self.tree
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Func && n.name == name)
            .map(|n| n.id)
    }

    /// 七级下钻路径：从根到 id。
    pub fn path_to(&self, id: usize) -> Vec<usize> {
        let mut path = vec![id];
        let mut cur = id;
        while let Some(p) = self.parent_of(cur) {
            path.push(p);
            cur = p;
        }
        path.reverse();
        path
    }

    pub fn parent_of(&self, id: usize) -> Option<usize> {
        for (i, n) in self.tree.nodes.iter().enumerate() {
            if n.children.contains(&id) {
                return Some(i);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ir_build_minimal() {
        let dir = std::env::temp_dir().join(format!("ca-ir-test-{}", std::process::id()));
        let sub = dir.join("net");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("client.ts"),
            "fn connect() {\n open()\n}\nfn close() {\n open()\n}\n",
        )
        .unwrap();
        let ir = open_project(&dir).unwrap();
        assert_eq!(ir.file_count, 1);
        assert!(ir.loc >= 4);
        assert!(ir.calls.iter().any(|e| e.to.contains("open")));
        let mods = ir.drill(ir.tree.root);
        assert_eq!(ir.tree.nodes[mods[0]].kind, NodeKind::Module);
        fs::remove_dir_all(&dir).ok();
    }
}
