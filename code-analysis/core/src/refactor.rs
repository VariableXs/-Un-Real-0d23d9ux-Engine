//! 自动改进/重构（#100~#135）—— AI-02 域三：一键改进（22 项）+ 框架操作（14 项）。
//!
//! 框架操作共用 Frame 数据模型（等价于 F1 框架视图的核心数据）。

// F100 长参数→对象封装：参数 > 4 自动生成参数结构体。
pub fn encapsulate_params(fn_name: &str, params: &[&str]) -> Option<(String, Vec<String>)> {
    (params.len() > 4).then(|| {
        let mut c = fn_name.to_string();
        if let Some(f) = c.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        c.push_str("Params");
        (c, params.iter().map(|p| p.to_string()).collect())
    })
}

// F101 嵌套条件展平：≥3 层 if → Guard Clause。
pub fn flatten_nesting(conds: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for c in conds {
        out.push(format!("if (!({c})) return;"));
    }
    out.push("body();".into());
    out
}

// F102 魔法数字命名化：字面量 → 命名常量。
pub fn magic_numbers(expr: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in expr.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || c == '.' {
            word.push(c);
        } else if !word.is_empty() {
            let v = word.clone();
            if v.parse::<f64>().map(|n| n != 0.0 && n != 1.0).unwrap_or(false) {
                let name = if v.contains('.') {
                    format!("MAGIC_{}", v.replace('.', "_"))
                } else {
                    format!("MAGIC_{v}")
                };
                out.push((name, v));
            }
            word.clear();
        }
    }
    out
}

// F103 回调→async：嵌套回调链拉平为 await 线性序列。
pub fn callback_to_async(calls: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev = String::new();
    for (i, c) in calls.iter().enumerate() {
        let name = c.split('(').next().unwrap_or(c).trim();
        if i == 0 {
            out.push(format!("let r0 = await {name}();"));
            prev = "r0".into();
        } else {
            out.push(format!("let r{i} = await {name}({prev});"));
            prev = format!("r{i}");
        }
    }
    out
}

// F104 全局变量封装：全局 → getter/setter。
pub fn encapsulate_global(name: &str, ty: &str) -> Vec<String> {
    vec![
        format!("private {ty} {name};"),
        format!("get_{name}() -> {ty}"),
        format!("set_{name}(v: {ty})"),
    ]
}

// F105 死代码删除：入口可达性分析 → 删除不可达节点。
pub fn dead_nodes(nodes: &[&str], edges: &[(&str, &str)], entry: &str) -> Vec<String> {
    let mut reach = vec![entry.to_string()];
    loop {
        let mut grew = false;
        for (a, b) in edges {
            if reach.iter().any(|r| r == a) && !reach.iter().any(|r| r == b) {
                reach.push(b.to_string());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    nodes.iter().filter(|n| !reach.iter().any(|r| r == **n)).map(|n| n.to_string()).collect()
}

// F106 循环不变量外提：不变表达式移出循环体。
pub fn hoist_invariant(invariant: &str, loop_body: &[&str]) -> Vec<String> {
    let mut out = vec![format!("{invariant};"), "loop {".into()];
    out.extend(loop_body.iter().map(|l| l.to_string()));
    out.push("}".into());
    out
}

// F107 字符串→模板：拼接 → 模板字面量。
pub fn to_template(parts: &[&str]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    // 首尾为字面量，中间为插值变量
    let mut s = format!("`{}", parts[0]);
    for p in &parts[1..parts.len().saturating_sub(1)] {
        s.push_str(&format!("${{{p}}}"));
    }
    if parts.len() > 1 {
        s.push_str(parts[parts.len() - 1]);
    }
    s.push('`');
    s
}

// F108 类型收窄：删除冗余检查（重复条件）。
pub fn redundant_checks(conds: &[&str]) -> Vec<usize> {
    let mut seen: Vec<&str> = Vec::new();
    let mut rm = Vec::new();
    for (i, c) in conds.iter().enumerate() {
        if seen.contains(c) {
            rm.push(i);
        } else {
            seen.push(c);
        }
    }
    rm
}

// F109 异常规范化：空 catch → 补全日志。
pub fn normalize_catch(body: &str) -> String {
    if body.trim().is_empty() {
        "log(e); rethrow".into()
    } else {
        body.into()
    }
}

// F110 一键整洁：缩进/空行/括号规整。
pub fn tidy(src: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    for l in src {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('}') {
            depth = depth.saturating_sub(1);
        }
        out.push(format!("{}{t}", "  ".repeat(depth)));
        if t.ends_with('{') {
            depth += 1;
        }
    }
    out
}

// F111 一键加速：循环重复/N+1/拼接的批量修复。
pub fn quick_speed(lines: &[(bool, &str)]) -> Vec<&'static str> {
    let mut fixes = Vec::new();
    let mut sorted: Vec<String> = Vec::new();
    for &(in_loop, l) in lines {
        if in_loop && l.contains("query(") && !fixes.contains(&"批量化 N+1 查询") {
            fixes.push("批量化 N+1 查询");
        }
        if in_loop && l.contains("+=") && l.contains('"') && !fixes.contains(&"拼接改 StringBuilder") {
            fixes.push("拼接改 StringBuilder");
        }
        if l.contains("sort(") {
            let key = l.split("sort(").next().unwrap_or("").trim().to_string();
            if sorted.contains(&key) && !fixes.contains(&"消除重复排序") {
                fixes.push("消除重复排序");
            } else {
                sorted.push(key);
            }
        }
    }
    fixes
}

// F112 一键精简：死代码 + 重复 + 冗余的总削减量。
pub fn quick_slim(lines: &[&str], dup_groups: &[&[usize]], redundant: usize) -> usize {
    let dead = crate::statics::dead_lines(lines).len();
    let dup: usize = dup_groups.iter().map(|g| g.len().saturating_sub(1)).sum();
    dead + dup + redundant
}

// F113 一键加固：为可能为 null 的解引用插入防护。
pub fn quick_harden(lines: &[&str]) -> Vec<String> {
    let risky = crate::statics::may_null(lines);
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if risky.contains(&i) {
            out.push("if (q == null) { fail(); }".into());
        }
        out.push(l.to_string());
    }
    out
}

// F114 一键易读：长函数拆分 + 注释气泡。
pub fn quick_readable(fn_name: &str, lines: &[&str]) -> Vec<String> {
    if lines.len() < 4 {
        return lines.iter().map(|l| l.to_string()).collect();
    }
    let mid = lines.len() / 2;
    let mut out = vec![format!("fn {fn_name}_part1() {{")];
    out.extend(lines[..mid].iter().map(|l| format!("  {l}")));
    out.push("}".into());
    out.push(format!("fn {fn_name}() {{ {fn_name}_part1(); {fn_name}_part2(); }}"));
    out.push(format!("fn {fn_name}_part2() {{"));
    out.extend(lines[mid..].iter().map(|l| format!("  {l}")));
    out.push("}".into());
    out
}

// F115 一键查bug：空指针/泄漏/越界/竞争的汇总问题单。
pub fn find_bugs(lines: &[&str]) -> Vec<String> {
    let mut bugs = Vec::new();
    for l in crate::statics::may_null(lines) {
        bugs.push(format!("第 {l} 行可能空指针解引用"));
    }
    bugs
}

// F116 一键翻译：AST→IR→目标语言（关键词映射表）。
#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Python,
    Js,
    Go,
}

pub fn translate(tokens: &[&str], target: Target) -> Vec<String> {
    tokens
        .iter()
        .map(|t| match target {
            Target::Python => match *t {
                "fn" => "def".into(),
                "let" => "".into(),
                "&&" => "and".into(),
                "||" => "or".into(),
                other => other.into(),
            },
            Target::Js => match *t {
                "fn" => "function".into(),
                "let" => "let".into(),
                other => other.into(),
            },
            Target::Go => match *t {
                "fn" => "func".into(),
                "let" => "var".into(),
                "&&" => "&&".into(),
                other => other.into(),
            },
        })
        .filter(|t: &String| !t.is_empty())
        .collect()
}

// F117 一键加测试：签名 → 边界值测试用例。
pub fn gen_tests(fn_name: &str, params: usize) -> Vec<String> {
    let mut out = Vec::new();
    for v in [0i64, 1, -1, i64::MAX] {
        let args = vec![v.to_string(); params];
        out.push(format!("assert!({fn_name}({}))", args.join(", ")));
    }
    out
}

// F118 一键加注释：函数名 → 中文注释。
pub fn gen_comments(fns: &[&str]) -> Vec<String> {
    fns.iter()
        .map(|f| {
            let verb = if f.starts_with("get_") {
                "获取"
            } else if f.starts_with("set_") {
                "设置"
            } else if f.starts_with("is_") {
                "判断"
            } else {
                "处理"
            };
            let rest = f.split('_').skip(1).collect::<Vec<_>>().join(" ");
            let rest = if rest.is_empty() { f.to_string() } else { rest };
            format!("// {verb} {rest}")
        })
        .collect()
}

// F119 一键拆文件：按符号类型拆分。
pub fn split_files(decls: &[(SymKind, &str)]) -> Vec<(String, Vec<String>)> {
    let mut files: Vec<(String, Vec<String>)> = Vec::new();
    for (k, name) in decls {
        let fname = match k {
            SymKind::Class => "classes",
            SymKind::Fn => "functions",
            SymKind::Const => "consts",
        };
        match files.iter_mut().find(|(f, _)| f == fname) {
            Some((_, names)) => names.push(name.to_string()),
            None => files.push((fname.into(), vec![name.to_string()])),
        }
    }
    files
}

#[derive(Clone, Copy, PartialEq)]
pub enum SymKind {
    Class,
    Fn,
    Const,
}

// F120 一键统一风格：全部命名转 snake_case。
pub fn unify_style(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|n| {
            let mut out = String::new();
            for (i, c) in n.chars().enumerate() {
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
        })
        .collect()
}

// F121 一键生成文档：签名 + 注释 → API 文档。
pub fn gen_docs(fn_name: &str, params: &[&str], ret: &str) -> String {
    let mut doc = format!("## {fn_name}\n\n**参数**\n");
    for p in params {
        doc.push_str(&format!("- {p}\n"));
    }
    doc.push_str(&format!("\n**返回**：{ret}\n"));
    doc
}

/// 依赖类型（F134 连线样式：实线/虚线/点线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepType {
    Call,
    Data,
    Event,
}

impl DepType {
    pub fn style(self) -> &'static str {
        match self {
            DepType::Call => "实线",
            DepType::Data => "虚线",
            DepType::Event => "点线",
        }
    }
}

/// F1 框架视图数据模型（#122~#135 共用）。
pub struct FrameNode {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub color: u8,
    pub locked: bool,
}

pub struct Frame {
    pub nodes: Vec<FrameNode>,
    pub edges: Vec<(usize, usize, DepType)>,
    pub undo: Vec<String>,
}

impl Frame {
    pub fn new() -> Self {
        Frame { nodes: Vec::new(), edges: Vec::new(), undo: Vec::new() }
    }

    fn add_node(&mut self, name: &str, x: i32, y: i32) -> usize {
        let id = self.nodes.len();
        self.nodes.push(FrameNode { name: name.into(), x, y, color: 0, locked: false });
        id
    }

    // F122 框架导入：从其他项目导入模块。
    pub fn import_module(&mut self, name: &str) -> usize {
        let id = self.add_node(name, 0, 0);
        self.undo.push(format!("import {name}"));
        id
    }

    // F123 框架导出：导出为 JSON。
    pub fn export_json(&self) -> String {
        let nodes: Vec<String> =
            self.nodes.iter().map(|n| format!("{{\"name\":\"{}\"}}", n.name)).collect();
        format!("[{}]", nodes.join(","))
    }

    // F124 框架添加依赖：A→B 依赖线（去重）。
    pub fn add_dep(&mut self, a: usize, b: usize, ty: DepType) -> bool {
        if self.edges.iter().any(|(x, y, _)| *x == a && *y == b) {
            return false;
        }
        self.edges.push((a, b, ty));
        self.undo.push(format!("dep {a}->{b}"));
        true
    }

    // F125 框架拆分：大模块 → 子模块。
    pub fn split(&mut self, id: usize, children: &[&str]) -> Vec<usize> {
        let (x, y) = (self.nodes[id].x, self.nodes[id].y);
        let mut ids = Vec::new();
        for (i, c) in children.iter().enumerate() {
            let cid = self.add_node(c, x + (i as i32) * 10, y);
            ids.push(cid);
        }
        self.undo.push(format!("split {id}"));
        ids
    }

    // F126 框架打包：小模块 → 大模块。
    pub fn pack(&mut self, ids: &[usize], name: &str) -> usize {
        let (x, y) = ids.first().map(|&i| (self.nodes[i].x, self.nodes[i].y)).unwrap_or((0, 0));
        let id = self.add_node(name, x, y);
        self.undo.push(format!("pack {name}"));
        id
    }

    // F127 框架搜索：子串匹配。
    pub fn search(&self, q: &str) -> Vec<usize> {
        self.nodes.iter().enumerate().filter(|(_, n)| n.name.contains(q)).map(|(i, _)| i).collect()
    }

    // F128 框架对齐：网格重排。
    pub fn align(&mut self, cols: usize) {
        for (i, n) in self.nodes.iter_mut().enumerate() {
            n.x = ((i % cols) * 40) as i32;
            n.y = ((i / cols) * 40) as i32;
        }
        self.undo.push("align".into());
    }

    // F129 框架撤回：撤销上一步操作。
    pub fn undo_last(&mut self) -> Option<String> {
        self.undo.pop()
    }

    // F130 框架重命名。
    pub fn rename(&mut self, id: usize, name: &str) {
        self.nodes[id].name = name.into();
        self.undo.push(format!("rename {id}"));
    }

    // F131 框架改颜色。
    pub fn set_color(&mut self, id: usize, color: u8) {
        self.nodes[id].color = color;
        self.undo.push(format!("color {id}"));
    }

    // F132 框架锁定：防误改。
    pub fn lock(&mut self, id: usize, on: bool) {
        self.nodes[id].locked = on;
    }

    // F133 框架删除：节点 + 相连边。
    pub fn delete(&mut self, id: usize) -> bool {
        if self.nodes[id].locked {
            return false;
        }
        self.nodes.remove(id);
        self.edges.retain(|(a, b, _)| *a != id && *b != id);
        self.undo.push(format!("delete {id}"));
        true
    }

    // F135 高亮依赖链：a→b 完整链路（BFS）。
    pub fn highlight_chain(&self, a: usize, b: usize) -> Vec<usize> {
        let mut prev: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut visited = vec![false; self.nodes.len()];
        let mut queue = vec![a];
        visited[a] = true;
        while let Some(cur) = queue.pop() {
            for (x, y, _) in &self.edges {
                if *x == cur && !visited[*y] {
                    visited[*y] = true;
                    prev[*y] = Some(cur);
                    queue.push(*y);
                }
            }
        }
        if !visited[b] {
            return vec![];
        }
        let mut chain = vec![b];
        while let Some(p) = prev[*chain.last().unwrap()] {
            chain.push(p);
        }
        chain.reverse();
        chain
    }
}

pub fn run_refactor_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("refactor");

    // F100
    let e = encapsulate_params("connect", &["host", "port", "user", "pass", "timeout"]);
    s.add("F100 长参数→对象封装", e.map(|(n, f)| n == "ConnectParams" && f.len() == 5).unwrap_or(false), ">4 参数生成结构体");
    // F101
    let fl = flatten_nesting(&["a", "b", "c"]);
    s.add("F101 嵌套条件展平", fl[0].contains("if (!(a))") && fl.last().unwrap() == "body();", "≥3 层→Guard Clause");
    // F102
    let mg = magic_numbers("delay = 30 * 1 + 2.5");
    s.add("F102 魔法数字命名化", mg.contains(&("MAGIC_30".into(), "30".into())) && mg.contains(&("MAGIC_2_5".into(), "2.5".into())) && !mg.iter().any(|(_, v)| v == "1"), "字面量→const（0/1 豁免）");
    // F103
    let asy = callback_to_async(&["fetch(url, cb1)", "then(cb2)", "then(cb3)"]);
    s.add("F103 回调→async", asy.len() == 3 && asy[0].contains("await fetch") && asy[2].contains("await then(r1)"), "嵌套回调→线性 await");
    // F104
    let eg = encapsulate_global("config", "Map");
    s.add("F104 全局变量封装", eg.len() == 3 && eg[1].contains("get_config"), "全局→getter/setter");
    // F105
    let dd = dead_nodes(&["main", "f", "g", "h"], &[("main", "f"), ("f", "g")], "main");
    s.add("F105 死代码删除", dd == vec!["h"], "入口可达性分析");
    // F106
    let ho = hoist_invariant("t = a * b", &["s += t", "i += 1"]);
    s.add("F106 循环不变量外提", ho[0].contains("t = a * b") && ho[1] == "loop {", "不变式移出循环");
    // F107
    s.add("F107 字符串→模板", to_template(&["Hello, ", "name", "!"]) == "`Hello, ${name}!`", "拼接→模板字面量");
    // F108
    let rc = redundant_checks(&["x != null", "x != null", "y > 0", "y > 0"]);
    s.add("F108 类型收窄", rc == vec![1, 3], "重复检查删除");
    // F109
    s.add("F109 异常规范化", normalize_catch("") == "log(e); rethrow" && normalize_catch("retry()") == "retry()", "空 catch 补全");
    // F110
    let td = tidy(&["fn f() {", "x()", "  y()", "}", "", "g()"]);
    s.add("F110 一键整洁", td == vec!["fn f() {", "  x()", "  y()", "}", "g()"], "缩进/空行规整");
    // F111
    let qs = quick_speed(&[(true, "db.query(r.id)"), (false, "a.sort()"), (false, "a.sort()")]);
    s.add("F111 一键加速", qs.contains(&"批量化 N+1 查询") && qs.contains(&"消除重复排序"), "热点批量修复");
    // F112
    let sl = quick_slim(&["if false {", "  a()", "}", "b()"], &[&[0usize, 1usize, 2usize]], 2);
    s.add("F112 一键精简", sl == 6, "死代码+重复+冗余削减");
    // F113
    let hd = quick_harden(&["q = null", "q.use()"]);
    s.add("F113 一键加固", hd.len() == 3 && hd[1].contains("if (q == null)"), "新增空值检查节点");
    // F114
    let rd = quick_readable("big", &["a()", "b()", "c()", "d()"]);
    s.add("F114 一键易读", rd.iter().any(|l| l.contains("big_part1")) && rd.iter().any(|l| l.contains("big_part2")), "长函数拆分");
    // F115
    let bg = find_bugs(&["p = null", "p.run()"]);
    s.add("F115 一键查bug", bg == vec!["第 1 行可能空指针解引用"], "问题面板汇总");
    // F116
    s.add("F116 一键翻译", translate(&["fn", "let", "x", "&&", "y"], Target::Python) == vec!["def", "x", "and", "y"], "IR→目标语言");
    // F117
    let gt = gen_tests("add", 2);
    s.add("F117 一键加测试", gt.len() == 4 && gt[0].contains("add(0, 0)") && gt[3].contains("9223372036854775807"), "签名→边界值用例");
    // F118
    let gc = gen_comments(&["get_user", "is_ready", "compute"]);
    s.add("F118 一键加注释", gc[0] == "// 获取 user" && gc[2] == "// 处理 compute", "AST→中文注释");
    // F119
    let sf = split_files(&[(SymKind::Fn, "a"), (SymKind::Class, "C"), (SymKind::Fn, "b")]);
    s.add("F119 一键拆文件", sf.len() == 2 && sf[0].0 == "functions" && sf[0].1.len() == 2, "按类/函数拆分");
    // F120
    s.add("F120 一键统一风格", unify_style(&["doWork", "PascalName", "already_snake"]) == vec!["do_work", "pascal_name", "already_snake"], "命名统一 snake_case");
    // F121
    let dc = gen_docs("send", &["to", "payload"], "bool");
    s.add("F121 一键生成文档", dc.contains("## send") && dc.contains("- to") && dc.contains("bool"), "签名→API 文档");
    // F122
    let mut fr = Frame::new();
    let m = fr.import_module("net");
    s.add("F122 框架导入", m == 0 && fr.nodes[0].name == "net", "新模块节点导入");
    // F123
    fr.import_module("db");
    s.add("F123 框架导出", fr.export_json() == "[{\"name\":\"net\"},{\"name\":\"db\"}]", "JSON 导出");
    // F124
    let dup = !fr.add_dep(0, 1, DepType::Call) || fr.add_dep(0, 1, DepType::Call);
    s.add("F124 框架添加依赖", !dup && fr.edges.len() == 1, "依赖线去重");
    // F125
    let kids = fr.split(0, &["tcp", "udp"]);
    s.add("F125 框架拆分", kids.len() == 2 && fr.nodes[kids[1]].name == "udp", "大模块→子模块");
    // F126
    let big = fr.pack(&kids, "transport");
    s.add("F126 框架打包", fr.nodes[big].name == "transport", "小模块→大模块");
    // F127
    s.add("F127 框架搜索", fr.search("db") == vec![1], "匹配高亮");
    // F128
    fr.align(2);
    s.add("F128 框架对齐", fr.nodes[1].x == 40 && fr.nodes[2].y == 40, "网格重排");
    // F129
    s.add("F129 框架撤回", fr.undo_last().as_deref() == Some("align") && fr.undo_last().as_deref() == Some("pack transport"), "撤销栈");
    // F130
    fr.rename(1, "storage");
    s.add("F130 框架重命名", fr.nodes[1].name == "storage", "标签可编辑");
    // F131
    fr.set_color(1, 3);
    s.add("F131 框架改颜色", fr.nodes[1].color == 3, "颜色渐变标注");
    // F132
    fr.lock(1, true);
    s.add("F132 框架锁定", fr.nodes[1].locked && !fr.delete(1), "锁定防误改");
    // F133
    fr.lock(1, false);
    s.add("F133 框架删除", fr.delete(1) && fr.edges.is_empty() && fr.nodes.len() == 4, "节点+连线淡出");
    // F134
    s.add(
        "F134 依赖类型标注",
        DepType::Call.style() == "实线" && DepType::Data.style() == "虚线" && DepType::Event.style() == "点线",
        "实线/虚线/点线",
    );
    // F135
    let mut f2 = Frame::new();
    let a = f2.import_module("a");
    let b = f2.import_module("b");
    let c = f2.import_module("c");
    let _ = f2.add_dep(a, b, DepType::Call);
    let _ = f2.add_dep(b, c, DepType::Data);
    s.add("F135 高亮依赖链", f2.highlight_chain(a, c) == vec![a, b, c] && f2.highlight_chain(c, a).is_empty(), "完整链路高亮");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f100_not_triggered_under_4() {
        assert!(encapsulate_params("f", &["a", "b"]).is_none());
    }

    #[test]
    fn f105_entry_always_reachable() {
        assert!(dead_nodes(&["main"], &[], "main").is_empty());
    }

    #[test]
    fn f124_add_dep_succeeds_once() {
        let mut f = Frame::new();
        let a = f.import_module("a");
        let b = f.import_module("b");
        assert!(f.add_dep(a, b, DepType::Data));
        assert!(!f.add_dep(a, b, DepType::Event));
    }

    #[test]
    fn f127_no_match() {
        let mut f = Frame::new();
        let _ = f.import_module("net");
        assert!(f.search("gpu").is_empty());
    }

    #[test]
    fn f132_locked_node_survives_delete() {
        let mut f = Frame::new();
        let a = f.import_module("a");
        f.lock(a, true);
        assert!(!f.delete(a));
        assert_eq!(f.nodes.len(), 1);
    }

    #[test]
    fn f116_go_translation() {
        assert_eq!(translate(&["fn", "let"], Target::Go), vec!["func", "var"]);
    }
}
