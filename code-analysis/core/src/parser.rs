//! 括号语言（C/TS/Rust/Java 风格）轻量解析前端。
//!
//! W1 范围：为 #001~#064 提供确定性 AST——行级扫描 + 大括号配对，
//! 提取 模块/类/函数/语句 层级、调用、赋值、new/free 等事实。
//! 多语言解析前端适配层（C02）在 W4 由 AI-09 扩展。

use crate::model::{Node, NodeKind, StmtKind, Tree};

fn strip_decl<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    let t = line.trim_start();
    // 允许 "pub fn x"、"async function x"、"export class x" 等前缀修饰
    let mut head = t;
    for p in ["pub ", "pub(crate) ", "async ", "export ", "static ", "public ", "private "] {
        if let Some(r) = head.strip_prefix(p) {
            head = r.trim_start();
        }
    }
    head.strip_prefix(kw)
}

/// 取 `name(` 或 `name {` 或 `name :` 前的标识符。
fn ident(rest: &str) -> String {
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if name.is_empty() {
        "<anon>".into()
    } else {
        name
    }
}

/// 从源码文本解析一棵文件级 AST（根 = File 节点）。行区间 0 基、半开。
pub fn parse_file(name: &str, src: &str) -> Tree {
    let mut t = Tree::with_root(name, NodeKind::File);
    // 栈：(节点 id, 是否函数块)
    let mut stack: Vec<usize> = vec![t.root];
    let mut depth = 0usize;

    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with("#") {
            continue;
        }

        // 声明识别（函数 > 类 > 模块）
        let decl = strip_decl(line, "fn ")
            .or_else(|| strip_decl(line, "function "))
            .map(|r| (NodeKind::Func, ident(r)))
            .or_else(|| {
                strip_decl(line, "class ")
                    .or_else(|| strip_decl(line, "struct "))
                    .map(|r| (NodeKind::Class, ident(r)))
            })
            .or_else(|| {
                strip_decl(line, "mod ")
                    .or_else(|| strip_decl(line, "namespace "))
                    .map(|r| (NodeKind::Subsystem, ident(r)))
            });

        if let Some((kind, nm)) = decl {
            let parent = *stack.last().unwrap();
            let id = t.add(parent, Node::new(0, kind, nm, (i, i + 1)));
            if line.contains('{') {
                stack.push(id);
                depth += 1;
            }
            // 单行声明结尾就带 `}` 的情况
            depth = depth.saturating_sub(line.matches('}').count());
            while stack.len() > 1 && depth == 0 && line.contains('}') {
                stack.pop();
                break;
            }
            continue;
        }

        // 普通语句行：归属最近的函数/类/文件
        let parent = *stack.last().unwrap();
        let sk = StmtKind::parse(line);
        let mut n = Node::new(0, NodeKind::Line, format!("L{}", i + 1), (i, i + 1));
        n.stmt = Some(sk);
        n.text = Some(line.to_string());
        match sk {
            StmtKind::Call => {
                // 取 `xxx(` 之前的标识符作为被调者
                if let Some(p) = line.find('(') {
                    let head = &line[..p];
                    let nm: String = head
                        .chars()
                        .rev()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    if !nm.is_empty() {
                        n.callee = Some(nm.trim_start_matches('.').to_string());
                    }
                }
            }
            StmtKind::Assign if line.rsplit('=').next().map_or(false, |r| r.contains('(')) => {
                // 右值为调用：如 `cfg = load(url)` → callee=load
                let rhs = line.split('=').last().unwrap_or("");
                if let Some(p) = rhs.find('(') {
                    let head = &rhs[..p];
                    let nm: String = head
                        .chars()
                        .rev()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    if !nm.is_empty() {
                        n.callee = Some(nm);
                    }
                }
                if let Some(p) = line.find('=') {
                    let lhs = line[..p].trim();
                    let nm: String = lhs
                        .chars()
                        .rev()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    if !nm.is_empty() {
                        n.target = Some(nm);
                    }
                }
            }
            _ => {}
        }
        t.add(parent, n);

        // 大括号进出（块语句 if/for/try 自身不建子树，行归函数）
        let opens = line.matches('{').count();
        let closes = line.matches('}').count();
        depth = depth + opens;
        for _ in 0..closes {
            depth = depth.saturating_sub(1);
            if stack.len() > 1 {
                stack.pop();
            }
        }
    }
    t
}

/// 统计语言（按扩展名粗分），返回 (语言, 文件数)。
pub fn lang_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" => "JavaScript",
        "py" => "Python",
        "java" => "Java",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "hpp" | "cc" => "C++",
        "cs" => "C#",
        "md" | "txt" => "Text",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
mod net {
    class Client {
        fn connect(url) {
            cfg = load(url)
            open(cfg)
            if (cfg.secure) {
                handshake()
            }
            return sock
        }
    }
}
"#;

    #[test]
    fn parse_hierarchy() {
        let t = parse_file("client.ts", SRC);
        assert_eq!(t.node(t.root).kind, NodeKind::File);
        let funcs: Vec<_> = t.nodes.iter().filter(|n| n.kind == NodeKind::Func).collect();
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "connect");
        let calls: Vec<_> = t
            .nodes
            .iter()
            .filter(|n| n.stmt == Some(StmtKind::Call))
            .map(|n| n.callee.clone().unwrap())
            .collect();
        assert!(calls.contains(&"load".to_string()));
        assert!(calls.contains(&"open".to_string()));
        assert!(calls.contains(&"handshake".to_string()));
    }

    #[test]
    fn lang_detect() {
        assert_eq!(lang_of("a/b.rs"), "Rust");
        assert_eq!(lang_of("x.tsx"), "TypeScript");
    }
}
