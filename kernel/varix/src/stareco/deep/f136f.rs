//! 深化层三 · F136 示例应用仓库（2026-09-26 深化批次三）。
//!
//! 补深仓库健康工程面（主册 G-D-11 + 账本回炉扩列方向）：示例依赖图
//! （builds_on 链环检测 + 拓扑序）、行数门禁统计器（≤200 线）、注释
//! 密度核算（30%-60% 区间判定）、CI 连绿账（当连/最长/断点计数）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 依赖图：builds_on 链 → 环检测（三色 DFS 迭代式）+ 拓扑序
// ---------------------------------------------------------------------------

/// 图输入：节点名 + 依赖表（依赖名序号序列）。names 与 deps 同序。
/// 边语义：deps[u] 含 v 表示 u 依赖 v（v 先学/先建）。
pub struct DepGraph<'a> {
    pub names: &'a [&'a str],
    pub deps: &'a [alloc::vec::Vec<usize>],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

/// 环检测（迭代 DFS，三色标记）：Gray 回边即环；返回环上任一节点序号。
pub fn find_cycle(g: &DepGraph) -> Option<usize> {
    let n = g.names.len();
    let mut color = alloc::vec![Color::White; n];
    for start in 0..n {
        if color[start] != Color::White {
            continue;
        }
        color[start] = Color::Gray;
        // 栈元素：(节点, 已消费的依赖游标)。
        let mut stack: alloc::vec::Vec<(usize, usize)> = alloc::vec![(start, 0)];
        while let Some((node, cur)) = stack.pop() {
            if cur < g.deps[node].len() {
                // 当前节点未消费完：压回（游标前进），再压依赖。
                stack.push((node, cur + 1));
                let next = g.deps[node][cur];
                match color[next] {
                    Color::Gray => return Some(next),
                    Color::White => {
                        color[next] = Color::Gray;
                        stack.push((next, 0));
                    }
                    Color::Black => {}
                }
            } else {
                color[node] = Color::Black;
            }
        }
    }
    None
}

/// 拓扑序（Kahn 入度法，ready 序保持节点号升序——稳定教学序）。
/// 有环返回 Err：教学链必须是 DAG。
pub fn topo_order(g: &DepGraph) -> Result<alloc::vec::Vec<usize>, &'static str> {
    if find_cycle(g).is_some() {
        return Err("依赖环：builds_on 链断裂");
    }
    let n = g.names.len();
    // 入度 = 前置依赖数（u 依赖 v ⇒ v 先行 ⇒ u 的入度来自自己的 deps）。
    let mut indeg: alloc::vec::Vec<usize> = g.deps.iter().map(|d| d.len()).collect();
    let mut order: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    let mut ready: alloc::vec::Vec<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
    while let Some(u) = ready.first().copied() {
        ready.remove(0);
        order.push(u);
        for w in 0..n {
            if g.deps[w].contains(&u) {
                indeg[w] -= 1;
                if indeg[w] == 0 {
                    let pos = ready.iter().position(|r| *r > w).unwrap_or(ready.len());
                    ready.insert(pos, w);
                }
            }
        }
    }
    if order.len() != n {
        return Err("拓扑未覆盖全节点：内部矛盾");
    }
    Ok(order)
}

// ---------------------------------------------------------------------------
// 行数门禁：逐文件 ≤200 行（教学约束），示例总计另账
// ---------------------------------------------------------------------------

pub struct LineAudit {
    pub over: alloc::vec::Vec<&'static str>,
    pub total: usize,
}

pub fn audit_lines(files: &[(&'static str, usize)]) -> LineAudit {
    let mut over = alloc::vec::Vec::new();
    let mut total = 0usize;
    for (name, lines) in files {
        total += lines;
        if *lines > 200 {
            over.push(*name);
        }
    }
    LineAudit { over, total }
}

// ---------------------------------------------------------------------------
// 注释密度：教学注释 30%-60% 区间（注释行 / (代码行+注释行)）
// ---------------------------------------------------------------------------

/// 行分类：空行不计入分母；'//' 或 '#' 开头计注释行。
pub fn comment_density(src: &str) -> u32 {
    let mut code = 0u32;
    let mut comments = 0u32;
    for line in src.lines() {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("//") || t.starts_with('#') {
            comments += 1;
        } else {
            code += 1;
        }
    }
    let denom = code + comments;
    if denom == 0 {
        0
    } else {
        comments * 100 / denom
    }
}

pub fn density_in_band(d: u32) -> bool {
    (30..=60).contains(&d)
}

// ---------------------------------------------------------------------------
// CI 连绿账：布尔日历 → 当连/最长/断点数
// ---------------------------------------------------------------------------

pub struct CiStreak {
    pub current: u32,
    pub longest: u32,
    pub breaks: u32,
}

/// 输入按日序（最早在前）；结尾 false 时当连为 0（如实呈现）。
pub fn ci_streak(days: &[bool]) -> CiStreak {
    let mut cur = 0u32;
    let mut longest = 0u32;
    let mut breaks = 0u32;
    for ok in days {
        if *ok {
            cur += 1;
            if cur > longest {
                longest = cur;
            }
        } else {
            breaks += 1;
            cur = 0;
        }
    }
    CiStreak { current: cur, longest, breaks }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136F_TAG: &str = "stareco-F136-deep3";

pub fn run_f136_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F136F_TAG);

    // 依赖图：线性链 hello←clipboard←notepad-lite → 学习序 0,1,2
    let names = ["hello", "clipboard", "notepad-lite"];
    let deps = [alloc::vec::Vec::<usize>::new(), alloc::vec![0], alloc::vec![1]];
    let g = DepGraph { names: &names, deps: &deps };
    set.add("f136f no cycle", find_cycle(&g).is_none(), "线性链无环");
    set.add("f136f topo order", topo_order(&g) == Ok(alloc::vec![0, 1, 2]), "拓扑序 0→1→2");

    // 环检测：a 依赖 b，b 依赖 a
    let names_c = ["a", "b"];
    let deps_c = [alloc::vec![1], alloc::vec![0]];
    let gc = DepGraph { names: &names_c, deps: &deps_c };
    set.add("f136f cycle found", find_cycle(&gc).is_some(), "双向环检出");
    set.add("f136f topo cycle err", topo_order(&gc).is_err(), "有环拒绝拓扑");

    // 行数门禁
    let files = [("hello.rs", 40), ("big.rs", 201), ("ok.rs", 200)];
    let audit = audit_lines(&files);
    set.add("f136f lines over", audit.over == alloc::vec!["big.rs"], "超 200 检出");
    set.add("f136f lines total", audit.total == 441, "总计另账");
    set.add("f136f lines boundary", audit_lines(&[("x", 200)]).over.is_empty(), "200 恰好放行");

    // 注释密度
    let src = "// 教学\nfn a() {}\n// 说明\n// 再说明\nfn b() {}\n\n";
    set.add("f136f density band", density_in_band(comment_density(src)), "3 注释 2 代码=60% 在带内");
    set.add("f136f density pure code", comment_density("fn a() {}\nfn b() {}") == 0, "零注释 0%");
    set.add("f136f density empty", comment_density("") == 0, "空源哨兵");

    // CI 连绿
    let s = ci_streak(&[true, true, false, true, true, true]);
    set.add("f136f streak current", s.current == 3, "当连 3");
    set.add("f136f streak longest", s.longest == 3, "最长 3");
    set.add("f136f streak breaks", s.breaks == 1, "断点 1");
    let s2 = ci_streak(&[true, false]);
    set.add("f136f streak tail false", s2.current == 0 && s2.breaks == 1, "尾断如实归零");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn diamond_dag_topo() {
        // hello ← clipboard ← notepad-lite；hello ← theme
        let names = ["hello", "clipboard", "notepad-lite", "theme"];
        let deps = [
            alloc::vec::Vec::<usize>::new(),
            alloc::vec![0],
            alloc::vec![1],
            alloc::vec![0],
        ];
        let g = DepGraph { names: &names, deps: &deps };
        let order = topo_order(&g).unwrap();
        assert_eq!(order, alloc::vec![0, 1, 2, 3]);
    }

    #[test]
    fn self_cycle_detected() {
        let names = ["a"];
        let deps = [alloc::vec![0]];
        let g = DepGraph { names: &names, deps: &deps };
        assert!(find_cycle(&g).is_some());
        assert!(topo_order(&g).is_err());
    }
}
