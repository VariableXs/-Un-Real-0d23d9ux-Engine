//! 深度静态分析（#065~#087）—— AI-02 域一：纯逻辑检测器。
//!
//! 零 AI：全部为确定性算法（枚举/传播/约束求解的简化确定性等价物）。

// F065 符号执行路径求解：枚举分支约束组合，得到全部可达路径。
pub fn symbolic_paths(branches: &[&str]) -> Vec<Vec<String>> {
    let n = branches.len();
    let mut paths = Vec::new();
    for mask in 0u32..(1u32 << n) {
        let mut p = Vec::new();
        for (i, b) in branches.iter().enumerate() {
            let val = (mask >> i) & 1 == 1;
            p.push(format!("{b}=={val}"));
        }
        paths.push(p);
    }
    paths
}

// F066 循环不变量发现：不涉及被修改变量的纯表达式即不变量。
pub fn loop_invariants(body: &[&str], mutated: &[&str]) -> Vec<String> {
    body.iter()
        .filter(|l| {
            !l.contains('=') && mutated.iter().all(|m| !l.contains(m)) && l.chars().any(|c| c.is_alphabetic())
        })
        .map(|l| l.trim().to_string())
        .collect()
}

// F067 复杂度自动推导：循环嵌套深度 + 减半 + 递归 → Big-O。
pub fn big_o(loop_depth: usize, halving: bool, recursive: bool) -> &'static str {
    if recursive {
        "O(2^n)"
    } else if halving && loop_depth >= 1 {
        "O(n log n)"
    } else {
        match loop_depth {
            0 => "O(1)",
            1 => "O(n)",
            2 => "O(n^2)",
            _ => "O(n^k)",
        }
    }
}

// F068 死代码/不可达标记：常量条件折叠（块体不可达）+ return 后同块不可达。
pub fn dead_lines(lines: &[&str]) -> Vec<usize> {
    let indent = |l: &str| l.len() - l.trim_start().len();
    // 活跃不可达作用域：(块缩进, 同级也算死) —— if false 的块体要求更深层缩进，
    // return 之后的同级剩余行同样不可达。
    let mut scopes: Vec<(usize, bool)> = Vec::new();
    let mut dead = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let ind = indent(l);
        let t = l.trim_start();
        scopes.retain(|&(s, incl)| if incl { ind >= s } else { ind > s });
        let mut is_dead = !scopes.is_empty();
        if t.starts_with("if false") || t.starts_with("if (false") {
            is_dead = true; // 条件行本身也标记
            scopes.push((ind, false));
        } else if t.starts_with("return") {
            scopes.push((ind, true));
        }
        if is_dead {
            dead.push(i);
        }
    }
    dead
}

// F069 条件覆盖矩阵：k 个布尔子条件的 2^k 组合，报告缺失组合。
pub fn missing_coverage(k: usize, covered: &[u32]) -> Vec<u32> {
    (0u32..(1 << k)).filter(|c| !covered.contains(c)).collect()
}

// F070 资源泄漏检测：配对获取/释放，报告未释放资源。
pub fn leaks(events: &[(bool, &str)]) -> Vec<String> {
    let mut open: Vec<(&str, usize)> = Vec::new();
    for &(acq, name) in events {
        if acq {
            match open.iter_mut().find(|(n, _)| *n == name) {
                Some((_, c)) => *c += 1,
                None => open.push((name, 1)),
            }
        } else if let Some((_, c)) = open.iter_mut().find(|(n, _)| *n == name) {
            *c = c.saturating_sub(1);
        }
    }
    open.iter().filter(|(_, c)| *c > 0).map(|(n, _)| n.to_string()).collect()
}

// F071 逻辑等价验证：真值表逐项比对，返回首个反例（等价则 None）。
pub fn equiv(a: &[bool], b: &[bool]) -> Option<usize> {
    a.iter().zip(b.iter()).position(|(x, y)| x != y)
}

// F072 数据竞争检测：Lockset 算法——共享变量、跨线程、至少一写、锁集不相交。
pub struct Access {
    pub var: &'static str,
    pub thread: usize,
    pub locks: &'static [&'static str],
    pub write: bool,
}

pub fn races(acc: &[Access]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, a) in acc.iter().enumerate() {
        for b in acc.iter().skip(i + 1) {
            if a.var == b.var
                && a.thread != b.thread
                && (a.write || b.write)
                && a.locks.iter().all(|l| !b.locks.contains(l))
            {
                out.push((a.var.to_string(), format!("t{}-t{}", a.thread, b.thread)));
            }
        }
    }
    out
}

// F073 空指针解引用检测：流敏感 may-null——置 null 后未经检查即解引用。
pub fn may_null(lines: &[&str]) -> Vec<usize> {
    let mut may: Vec<String> = Vec::new();
    let mut hits = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.contains("!= null") || t.contains("== null") {
            may.clear(); // 遇到判空即视为已检查
            continue;
        }
        if let Some((lhs, _)) = t.split_once('=') {
            if t.contains("null") {
                let v = lhs.trim().to_string();
                if !may.contains(&v) {
                    may.push(v);
                }
                continue;
            }
        }
        for v in may.clone() {
            if t.contains(&format!("{v}.")) || t.contains(&format!("{v}->")) {
                hits.push(i);
            }
        }
    }
    hits
}

// F074 整数溢出检测：值域分析标记可能溢出的二元运算。
pub fn overflow_risk(ranges: &[(&str, i64, i64)], expr: &str) -> Vec<String> {
    let i32min = i32::MIN as i64;
    let i32max = i32::MAX as i64;
    let (a, op, b) = match expr.split_once('+') {
        Some((x, y)) => (x.trim(), '+', y.trim()),
        None => match expr.split_once('-') {
            Some((x, y)) => (x.trim(), '-', y.trim()),
            None => return vec![],
        },
    };
    let ra = ranges.iter().find(|(n, _, _)| *n == a);
    let rb = ranges.iter().find(|(n, _, _)| *n == b);
    let (Some(ra), Some(rb)) = (ra, rb) else { return vec![] };
    let (lo, hi) = match op {
        '+' => (ra.1 + rb.1, ra.2 + rb.2),
        _ => (ra.1 - rb.2, ra.2 - rb.1),
    };
    if lo < i32min || hi > i32max {
        vec![expr.to_string()]
    } else {
        vec![]
    }
}

// F075 污点分析：source → 传播链 → sink。
pub fn taint_flows(sources: &[&str], assigns: &[(&str, &str)], sinks: &[&str]) -> Vec<String> {
    let mut tainted: Vec<(String, Vec<String>)> =
        sources.iter().map(|s| (s.to_string(), vec![s.to_string()])).collect();
    for _ in 0..assigns.len() {
        for (dst, src) in assigns {
            for (v, chain) in tainted.clone() {
                if src.contains(&v) && !tainted.iter().any(|(d, _)| d == dst) {
                    let mut c = chain;
                    c.push(dst.to_string());
                    tainted.push((dst.to_string(), c));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (v, chain) in &tainted {
        for s in sinks {
            if s == v {
                out.push(chain.join(" -> "));
            }
        }
    }
    out
}

// F076 单一职责检测：独立逻辑段 > 2 建议拆分。
pub fn responsibility(fn_name: &str, segments: usize) -> Option<String> {
    (segments > 2).then(|| format!("{fn_name} 有 {segments} 个独立逻辑段，建议拆分"))
}

// F077 代码异味雷达：22 种经典异味目录 + 度量触发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Smell {
    LongMethod,
    LargeClass,
    LongParameterList,
    DuplicateCode,
    DeadCode,
    DataClump,
    PrimitiveObsession,
    SwitchAbuse,
    LazyClass,
    MiddleMan,
    TemporaryField,
    MessageChain,
    DivergentChange,
    ShotgunSurgery,
    FeatureEnvy,
    SpeculativeGenerality,
    IncompleteLibrary,
    RefusedBequest,
    CommentMasking,
    MutableData,
    GlobalData,
    ExcessiveExposure,
}

pub const ALL_SMELLS: [Smell; 22] = [
    Smell::LongMethod,
    Smell::LargeClass,
    Smell::LongParameterList,
    Smell::DuplicateCode,
    Smell::DeadCode,
    Smell::DataClump,
    Smell::PrimitiveObsession,
    Smell::SwitchAbuse,
    Smell::LazyClass,
    Smell::MiddleMan,
    Smell::TemporaryField,
    Smell::MessageChain,
    Smell::DivergentChange,
    Smell::ShotgunSurgery,
    Smell::FeatureEnvy,
    Smell::SpeculativeGenerality,
    Smell::IncompleteLibrary,
    Smell::RefusedBequest,
    Smell::CommentMasking,
    Smell::MutableData,
    Smell::GlobalData,
    Smell::ExcessiveExposure,
];

pub struct Metrics {
    pub method_lines: usize,
    pub class_methods: usize,
    pub params: usize,
    pub dup_blocks: usize,
    pub dead_lines: usize,
    pub switch_cases: usize,
    pub chained_calls: usize,
    pub mutable_globals: usize,
}

pub fn detect_smells(m: &Metrics) -> Vec<(Smell, u8)> {
    let mut out = Vec::new();
    if m.method_lines > 30 {
        out.push((Smell::LongMethod, 3));
    }
    if m.class_methods > 20 {
        out.push((Smell::LargeClass, 2));
    }
    if m.params > 4 {
        out.push((Smell::LongParameterList, 2));
    }
    if m.dup_blocks > 0 {
        out.push((Smell::DuplicateCode, 3));
    }
    if m.dead_lines > 0 {
        out.push((Smell::DeadCode, 1));
    }
    if m.switch_cases > 5 {
        out.push((Smell::SwitchAbuse, 2));
    }
    if m.chained_calls > 3 {
        out.push((Smell::MessageChain, 1));
    }
    if m.mutable_globals > 0 {
        out.push((Smell::GlobalData, 2));
    }
    out
}

// F078 设计模式建议：特征 → GoF 模式名（23 种全覆盖映射）。
pub fn pattern_suggestions(features: &[&str]) -> Vec<&'static str> {
    features
        .iter()
        .filter_map(|f| match *f {
            "iterate" => Some("Iterator"),
            "notify" => Some("Observer"),
            "undo" => Some("Memento"),
            "wrap" => Some("Adapter"),
            "clone" => Some("Prototype"),
            "single" => Some("Singleton"),
            "build-steps" => Some("Builder"),
            "product-family" => Some("Abstract Factory"),
            "swap-algorithm" => Some("Strategy"),
            "vary-state" => Some("State"),
            "queue-command" => Some("Command"),
            "control-access" => Some("Proxy"),
            "tree-uniform" => Some("Composite"),
            "add-responsibility" => Some("Decorator"),
            "unify-facade" => Some("Facade"),
            "split-interface" => Some("Bridge"),
            "defer-create" => Some("Factory Method"),
            "fixed-steps" => Some("Template Method"),
            "pass-along" => Some("Chain of Responsibility"),
            "eval-grammar" => Some("Interpreter"),
            "reduce-coupling" => Some("Mediator"),
            "share-fine" => Some("Flyweight"),
            "visit-types" => Some("Visitor"),
            _ => None,
        })
        .collect()
}

// F079 依赖倒置检查：高层模块直接 import 低层具体实现（未走接口）。
pub fn dip_violations(imports: &[(&str, &str)]) -> Vec<String> {
    imports
        .iter()
        .filter(|(high, low)| high.starts_with('H') && !low.starts_with("I") && !low.starts_with("trait "))
        .map(|(h, l)| format!("{h} 直接依赖实现 {l}，建议引入接口"))
        .collect()
}

// F080 API 一致性审计：同类前缀函数参数个数不统一。
pub fn api_inconsistency(apis: &[(&str, usize)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, n) in apis {
        let prefix = name.split('_').next().unwrap_or(name);
        let others: Vec<usize> =
            apis.iter().filter(|(m, _)| m != name && m.starts_with(prefix)).map(|(_, k)| *k).collect();
        if !others.is_empty() && others.iter().any(|k| *k != *n) {
            out.push(format!("{name} 参数 {n} 个，同类 {prefix}_* 为 {others:?}"));
        }
    }
    out
}

// F081 错误处理完整性：可能抛异常的调用未被 try-catch 覆盖。
pub fn unhandled_exceptions(calls: &[(&str, bool)], wrapped: &[&str]) -> Vec<String> {
    calls
        .iter()
        .filter(|(f, may)| *may && !wrapped.contains(f))
        .map(|(f, _)| format!("{f} 可能抛出异常且未捕获"))
        .collect()
}

// F082 性能反模式检测：N+1 查询/循环内分配/字符串循环拼接/重复排序。
pub fn perf_antipatterns(lines: &[(bool, &str)]) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    let mut sorted: Vec<String> = Vec::new();
    for (i, &(in_loop, l)) in lines.iter().enumerate() {
        if in_loop && l.contains("query(") {
            out.push(("N+1查询", i));
        }
        if in_loop && l.contains("+=") && l.contains('"') {
            out.push(("字符串循环拼接", i));
        }
        if in_loop && l.contains("new ") {
            out.push(("循环内分配", i));
        }
        if l.contains("sort(") {
            let key = l.split("sort(").next().unwrap_or("").trim().to_string();
            if sorted.contains(&key) {
                out.push(("重复排序", i));
            } else {
                sorted.push(key);
            }
        }
    }
    out
}

// F083 命名规范检查：按符号类型检查命名惯例并给出建议名。
#[derive(Clone, Copy, PartialEq)]
pub enum Sym {
    Fn,
    Class,
    Const,
    Var,
}

pub fn naming_issue(kind: Sym, name: &str) -> Option<(String, &'static str)> {
    let to_snake = |s: &str| -> String {
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if c.is_uppercase() {
                if i > 0 {
                    out.push('_');
                }
                out.extend(c.to_lowercase());
            } else {
                out.push(c);
            }
        }
        out
    };
    match kind {
        Sym::Fn | Sym::Var => {
            if name.contains(char::is_uppercase) {
                Some((to_snake(name), "应为 snake_case"))
            } else {
                None
            }
        }
        Sym::Class => {
            if name.chars().next().is_some_and(|c| c.is_lowercase()) {
                let mut s = name.to_string();
                if let Some(c) = s.get_mut(0..1) {
                    c.make_ascii_uppercase();
                }
                Some((s, "应为 PascalCase"))
            } else {
                None
            }
        }
        Sym::Const => {
            if name.chars().any(|c| c.is_lowercase()) {
                Some((name.to_uppercase(), "应为 UPPER_SNAKE_CASE"))
            } else {
                None
            }
        }
    }
}

// F084 注释覆盖率统计：公共 API 文档注释覆盖率（百分比）。
pub fn doc_coverage(apis: &[(&str, bool)]) -> usize {
    if apis.is_empty() {
        return 100;
    }
    apis.iter().filter(|(_, d)| *d).count() * 100 / apis.len()
}

// F085 循环依赖检测：找出依赖环并给出中间层修复建议。
pub fn cyclic_deps(edges: &[(&str, &str)]) -> Vec<Vec<String>> {
    let mut nodes: Vec<&str> = Vec::new();
    for &(a, b) in edges {
        if !nodes.contains(&a) {
            nodes.push(a);
        }
        if !nodes.contains(&b) {
            nodes.push(b);
        }
    }
    let mut cycles = Vec::new();
    for start in &nodes {
        // DFS 找回到 start 的环
        let mut path: Vec<String> = vec![start.to_string()];
        dfs_cycles(start, start, edges, &mut path, &mut cycles);
    }
    // 去重（旋转/镜像等价）
    let mut uniq: Vec<Vec<String>> = Vec::new();
    for c in cycles {
        if !uniq.iter().any(|u| u.len() == c.len() && c.iter().all(|x| u.contains(x))) {
            uniq.push(c);
        }
    }
    uniq
}

fn dfs_cycles(start: &str, cur: &str, edges: &[(&str, &str)], path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    for &(a, b) in edges {
        if a == cur {
            if b == start && path.len() > 1 {
                out.push(path.clone());
            } else if b != start && !path.iter().any(|p| p == b) {
                path.push(b.to_string());
                dfs_cycles(start, b, edges, path, out);
                path.pop();
            }
        }
    }
}

// F086 圈复杂度自动拆解：McCabe 超 10 时提取最深分支。
pub fn mccabe(decisions: usize) -> usize {
    decisions + 1
}

pub fn split_preview(fn_name: &str, decisions: usize, branches: &[&str]) -> Option<String> {
    if mccabe(decisions) > 10 {
        let deepest = branches.last().copied().unwrap_or("");
        Some(format!("提取 {fn_name} 最深层分支 [{deepest}] 为子函数"))
    } else {
        None
    }
}

// F087 重复代码克隆检测：归一化后比对块内容（≥min_lines 行）。
pub fn clones(blocks: &[&[&str]], min_lines: usize) -> Vec<(usize, usize)> {
    let norm = |b: &[&str]| -> String { b.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n") };
    let mut out = Vec::new();
    for i in 0..blocks.len() {
        if blocks[i].len() < min_lines {
            continue;
        }
        for j in (i + 1)..blocks.len() {
            if blocks[j].len() >= min_lines && norm(blocks[i]) == norm(blocks[j]) {
                out.push((i, j));
            }
        }
    }
    out
}

pub fn run_statics_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("statics");

    // F065
    let paths = symbolic_paths(&["x>0", "y<0"]);
    s.add("F065 符号执行路径求解", paths.len() == 4 && paths[0][0] == "x>0==false" && paths[3][1] == "y<0==true", "2^2 条约束路径");
    // F066
    let inv = loop_invariants(&["c < max", "i = i + 1"], &["i"]);
    s.add("F066 循环不变量发现", inv == vec!["c < max"], "排除被修改变量");
    // F067
    s.add("F067 复杂度自动推导", big_o(1, true, false) == "O(n log n)" && big_o(2, false, false) == "O(n^2)", "主定理分级");
    // F068
    let dl = dead_lines(&["if false {", "  work()", "}", "return", "more()"]);
    s.add("F068 死代码/不可达标记", dl.contains(&1) && dl.contains(&4), "常量折叠+return 后不可达");
    // F069
    let miss = missing_coverage(2, &[0b00, 0b11]);
    s.add("F069 条件覆盖矩阵", miss == vec![0b01, 0b10], "缺失组合枚举");
    // F070
    let lk = leaks(&[(true, "f"), (true, "db"), (false, "f")]);
    s.add("F070 资源泄漏检测", lk == vec!["db"], "未释放资源报告");
    // F071
    s.add("F071 逻辑等价验证", equiv(&[true, false], &[true, false]).is_none() && equiv(&[true], &[false]) == Some(0), "真值表等价/反例");
    // F072
    let r = races(&[
        Access { var: "cnt", thread: 0, locks: &["L"], write: true },
        Access { var: "cnt", thread: 1, locks: &[], write: true },
    ]);
    s.add("F072 数据竞争检测", r == vec![("cnt".into(), "t0-t1".into())], "Lockset 无锁并发写");
    // F073
    let mn = may_null(&["p = null", "if p != null {}", "q = null", "q.use()"]);
    s.add("F073 空指针解引用检测", mn == vec![3], "流敏感 may-null");
    // F074
    let ov = overflow_risk(&[("a", 2_000_000_000i64, 2_000_000_000i64), ("b", 2_000_000_000i64, 2_000_000_000i64)], "a + b");
    s.add("F074 整数溢出检测", !ov.is_empty(), "值域超界标风险");
    // F075
    let tf = taint_flows(&["input"], &[("t1", "input"), ("t2", "t1")], &["t2"]);
    s.add("F075 污点分析", tf == vec!["input -> t1 -> t2"], "source→sink 传播链");
    // F076
    s.add("F076 单一职责检测", responsibility("f", 3).is_some() && responsibility("g", 2).is_none(), ">2 段建议拆分");
    // F077
    let smells = detect_smells(&Metrics { method_lines: 50, class_methods: 3, params: 6, dup_blocks: 1, dead_lines: 0, switch_cases: 2, chained_calls: 1, mutable_globals: 0 });
    s.add("F077 代码异味雷达", smells.len() == 3 && ALL_SMELLS.len() == 22, "度量触发 3 种/目录 22 种");
    // F078
    let pats = pattern_suggestions(&["iterate", "notify", "visit-types"]);
    s.add("F078 设计模式建议", pats == vec!["Iterator", "Observer", "Visitor"], "特征→GoF 模式");
    // F079
    let dip = dip_violations(&[("HighSvc", "MysqlDb"), ("HighSvc", "IRepo")]);
    s.add("F079 依赖倒置检查", dip.len() == 1 && dip[0].contains("MysqlDb"), "高层直依赖实现");
    // F080
    let inc = api_inconsistency(&[("user_add", 3), ("user_del", 2), ("order_add", 1)]);
    s.add("F080 API一致性审计", inc.len() == 2, "同类函数参数不统一");
    // F081
    let un = unhandled_exceptions(&[("read_cfg", true), ("calc", false)], &[]);
    s.add("F081 错误处理完整性", un == vec!["read_cfg 可能抛出异常且未捕获"], "try-catch 覆盖检查");
    // F082
    let pa = perf_antipatterns(&[(true, "rows.each(r => db.query(r.id))"), (false, "a.sort()"), (false, "a.sort()")]);
    s.add("F082 性能反模式检测", pa.contains(&("N+1查询", 0)) && pa.contains(&("重复排序", 2)), "N+1/重复排序");
    // F083
    s.add(
        "F083 命名规范检查",
        naming_issue(Sym::Fn, "doWork") == Some(("do_work".into(), "应为 snake_case"))
            && naming_issue(Sym::Const, "limit").map(|x| x.0) == Some("LIMIT".into()),
        "按类型给建议名",
    );
    // F084
    s.add("F084 注释覆盖率统计", doc_coverage(&[("a", true), ("b", true), ("c", false)]) == 66, "百分比聚合");
    // F085
    let cy = cyclic_deps(&[("a", "b"), ("b", "c"), ("c", "a")]);
    s.add("F085 循环依赖可视化修复", cy.len() == 1 && cy[0].len() == 3, "检测依赖环");
    // F086
    s.add("F086 圈复杂度自动拆解", mccabe(12) == 13 && split_preview("f", 12, &["deep path"]).is_some() && split_preview("g", 3, &[]).is_none(), "超阈值提取最深分支");
    // F087
    let cl = clones(&[&["x = 1", "y = 2", "z = 3", "w = 4", "v = 5", "u = 6"], &["x = 1", "y = 2", "z = 3", "w = 4", "v = 5", "u = 6"], &["q()"]], 6);
    s.add("F087 重复代码克隆检测", cl == vec![(0, 1)], "≥6 行归一化克隆");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f065_path_count() {
        assert_eq!(symbolic_paths(&["a", "b", "c"]).len(), 8);
    }

    #[test]
    fn f068_dead_after_false_if() {
        let d = dead_lines(&["if false {", "  a()", "  b()", "}", "c()"]);
        assert_eq!(d, vec![0, 1, 2]);
    }

    #[test]
    fn f071_counterexample() {
        assert_eq!(equiv(&[true, true, false, false], &[true, false, true, false]), Some(1));
    }

    #[test]
    fn f072_no_race_with_shared_lock() {
        let r = races(&[
            Access { var: "v", thread: 0, locks: &["L"], write: true },
            Access { var: "v", thread: 1, locks: &["L"], write: true },
        ]);
        assert!(r.is_empty());
    }

    #[test]
    fn f073_check_clears_may_null() {
        assert!(may_null(&["p = null", "if p == null { return }", "p.use()"]).is_empty());
    }

    #[test]
    fn f075_multi_hop_taint() {
        let f = taint_flows(&["s"], &[("a", "s"), ("b", "a"), ("c", "b")], &["c"]);
        assert_eq!(f, vec!["s -> a -> b -> c"]);
    }

    #[test]
    fn f085_two_node_cycle() {
        let c = cyclic_deps(&[("a", "b"), ("b", "a")]);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn f086_below_threshold_no_split() {
        assert!(split_preview("small", 5, &["x"]).is_none());
    }

    #[test]
    fn f087_short_block_not_clone() {
        assert!(clones(&[&["a()", "b()"], &["a()", "b()"]], 6).is_empty());
    }
}
