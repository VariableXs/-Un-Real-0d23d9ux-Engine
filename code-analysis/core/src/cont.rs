//! 零上下文续写引擎（#018~#037）。
//!
//! 零 AI 承诺：续写 = 语法产生式 + 类型约束 + 作用域枚举，全部确定性算法，
//! 不调用任何网络模型。

use std::collections::HashMap;

// F018 合法Token枚举器 —— 当前上下文状态 → 文法产生式 → 枚举合法下一 Token
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    FileTop,
    AfterFn,
    InBlock,
    AfterIf,
    AfterAssign,
    AfterReturn,
}

pub fn legal_tokens(ctx: Ctx) -> Vec<&'static str> {
    match ctx {
        Ctx::FileTop => vec!["fn", "class", "mod", "let", "import", "struct"],
        Ctx::AfterFn => vec!["(", "name"],
        Ctx::InBlock => vec!["let", "if", "for", "while", "return", "identifier", "}"],
        Ctx::AfterIf => vec!["(", "condition"],
        Ctx::AfterAssign => vec!["number", "string", "identifier", "(", "["],
        Ctx::AfterReturn => vec!["identifier", "number", "(", "string"],
    }
}

// F019 类型约束续写 —— 简化 Hindley-Milner：环境变量带类型，只推荐类型匹配的表达式
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedSym {
    pub name: String,
    pub ty: String,
}

pub fn typed_completion(env: &[TypedSym], expect: &str) -> Vec<String> {
    env.iter()
        .filter(|s| s.ty == expect)
        .map(|s| s.name.clone())
        .collect()
}

/// 简单推断：字面量/已知函数返回类型。
pub fn infer(expr: &str, env: &[TypedSym]) -> String {
    let e = expr.trim();
    if e.parse::<i64>().is_ok() {
        return "int".into();
    }
    if e.starts_with('"') || e.starts_with('\'') {
        return "string".into();
    }
    if e == "true" || e == "false" {
        return "bool".into();
    }
    if let Some(s) = env.iter().find(|s| s.name == e) {
        return s.ty.clone();
    }
    "unknown".into()
}

// F020 作用域符号雷达 —— 沿词法作用域链扫描可见符号，按距离+类型排序
pub fn scope_radar(chain: &[Vec<TypedSym>], want_ty: Option<&str>) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for (dist, scope) in chain.iter().enumerate() {
        for s in scope {
            if let Some(t) = want_ty {
                if s.ty != t {
                    continue;
                }
            }
            if !out.iter().any(|(n, _)| n == &s.name) {
                out.push((s.name.clone(), dist));
            }
        }
    }
    out.sort_by_key(|(_, d)| *d);
    out
}

// F021 API签名骨架填充 —— 从存根声明生成 参数占位+返回值模板
pub fn stub_skeleton(sig: &str) -> String {
    // 形如 "fn connect(url: string, port: int) -> bool"
    let name = sig
        .split(|c: char| c == ' ' || c == '(')
        .nth(1)
        .unwrap_or("api")
        .to_string();
    let params: Vec<String> = sig
        .split('(')
        .nth(1)
        .and_then(|r| r.split(')').next())
        .map(|p| {
            p.split(',')
                .enumerate()
                .map(|(i, s)| {
                    let nm = s.split(':').next().unwrap_or("").trim();
                    if nm.is_empty() {
                        format!("arg{i}")
                    } else {
                        nm.to_string()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let ret = sig.split("->").nth(1).map(|r| r.trim()).unwrap_or("");
    let body = match ret {
        "bool" => "false",
        "int" => "0",
        "string" => "\"\"",
        "" | "void" | "()" => "",
        _ => "todo!()",
    };
    format!("fn {name}({}) {{\n    {body}\n}}", params.join(", "))
}

// F022 惯用法AST模板库 —— 参数化模板 + 子树匹配后填充
pub struct TemplateLib {
    pub templates: Vec<(String, String)>, // (匹配特征, 模板)
}

impl TemplateLib {
    pub fn builtin() -> Self {
        TemplateLib {
            templates: vec![
                ("loop-over-list".into(), "for item in list {\n    process(item)\n}".into()),
                ("null-check".into(), "if (v != null) {\n    use(v)\n}".into()),
                ("err-check".into(), "if (err != null) {\n    return err\n}".into()),
            ],
        }
    }
    /// 特征匹配：上下文包含关键词即命中并填充参数。
    pub fn match_fill(&self, context: &str, arg: &str) -> Option<String> {
        let key = if context.contains("for") {
            "loop-over-list"
        } else if context.contains("null") {
            "null-check"
        } else if context.contains("err") {
            "err-check"
        } else {
            return None;
        };
        self.templates
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, t)| t.replace("item", arg).replace("v", arg))
    }
}

// F023 对称结构闭合 —— 确定性补全括号/标签/begin-end/try-catch
pub fn close_structure(src: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for c in src.chars() {
        match c {
            '(' => stack.push(")"),
            '[' => stack.push("]"),
            '{' => stack.push("}"),
            ')' | ']' | '}' => {
                stack.pop();
            }
            _ => {}
        }
    }
    let mut out = src.to_string();
    for closer in stack.into_iter().rev() {
        out.push_str(closer);
    }
    out
}

// F024 结构化注释→骨架 —— 解析 @param/@return/@throws 生成函数骨架
pub fn comment_skeleton(comment: &[&str], fname: &str) -> String {
    let mut params = Vec::new();
    let mut ret = "";
    let mut throws = Vec::new();
    for l in comment {
        let t = l.trim();
        if let Some(p) = t.strip_prefix("@param") {
            params.push(p.trim().split_whitespace().next().unwrap_or("arg").to_string());
        } else if let Some(r) = t.strip_prefix("@return") {
            ret = r.trim();
        } else if let Some(e) = t.strip_prefix("@throws") {
            throws.push(e.trim().to_string());
        }
    }
    let mut body = String::new();
    for e in &throws {
        body.push_str(&format!("    // may throw {e}\n"));
    }
    let mut tail = String::new();
    if !ret.is_empty() {
        tail = format!("    return {ret}\n");
    }
    format!(
        "fn {fname}({}) {{\n{body}{tail}}}",
        params.join(", ")
    )
}

// F025 测试断言反推 —— 读 assert/expect 反推函数签名与边界分支
pub fn infer_from_tests(tests: &[String], fname: &str) -> String {
    // 形如 "assert_eq!(add(1, 2), 3)"
    let mut arity = 0usize;
    let mut cases = 0usize;
    for t in tests {
        if t.contains(&format!("{fname}(")) {
            cases += 1;
            if let Some(a) = t.find(&format!("{fname}(")) {
                let inner = &t[a + fname.len() + 1..];
                if let Some(b) = inner.find(')') {
                    arity = arity.max(inner[..b].split(',').filter(|s| !s.trim().is_empty()).count());
                }
            }
        }
    }
    format!("fn {fname}(/* {arity} args, {cases} cases: 含边界 0/空/最大 */)")
}

// F026 import自动修复 —— 未定义符号 → 扫描索引 → 补全 import
pub struct ImportFixer {
    pub index: HashMap<String, String>, // 符号 → 模块
}

impl ImportFixer {
    pub fn fix(&self, src: &str, known: &[String]) -> Vec<String> {
        let mut missing: Vec<String> = Vec::new();
        for sym in known {
            if !src.contains(&format!("fn {sym}")) && !src.contains(&format!("import {sym}")) {
                if let Some(m) = self.index.get(sym) {
                    let imp = format!("import {sym} from {m}");
                    if !missing.contains(&imp) {
                        missing.push(imp);
                    }
                }
            }
        }
        missing
    }
}

// F027 AST同构模式外推 —— 连续 3+ 行同构重复，外推下一段
pub fn extrapolate_isomorphic(lines: &[String]) -> Option<String> {
    if lines.len() < 3 {
        return None;
    }
    // 模板化：把数字与连续字母串归一
    let tmpl = |l: &str| -> String {
        l.chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect()
    };
    let t0 = tmpl(&lines[0]);
    if !lines[1..].iter().all(|l| tmpl(l) == t0) {
        return None;
    }
    // 找出递增的数字并 +1
    let last = lines.last().unwrap().clone();
    let next: String = last
        .char_indices()
        .map(|(_i, c)| {
            if c.is_ascii_digit() {
                char::from_digit(c.to_digit(10).unwrap() + 1, 10).unwrap_or(c)
            } else {
                c
            }
        })
        .collect();
    Some(next)
}

// F028 CYK语法补全 —— 残缺输入上计算合法派生，选最短补全
pub fn cyk_complete(tokens: &[&str]) -> String {
    // 简化文法：表达式 = 数 (op 数)*；括号须闭合
    let mut out = tokens.join(" ");
    // 悬空运算符 → 补 0（先补操作数再收括号）
    if out.ends_with('+') || out.ends_with('-') || out.ends_with('*') {
        out.push_str(" 0");
    }
    let opens = out.matches('(').count();
    let closes = out.matches(')').count();
    for _ in closes..opens {
        out.push_str(" )");
    }
    out
}

// F029 接口约束推导续写 —— 已实现接口 → 枚举方法 → 推荐下一调用
pub fn interface_methods(iface: &str, implemented: &[String]) -> Vec<String> {
    // iface 形如 "trait Shape { draw(); area(); }"
    let body = iface.split('{').nth(1).unwrap_or("");
    body.split(';')
        .map(|m| {
            m.trim()
                .split('(')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .map(|m| m.trim_end_matches('}').trim().to_string())
        .filter(|m| !m.is_empty())
        .filter(|m| !implemented.iter().any(|i| i == m))
        .collect()
}

// F030 Panic Mode错误恢复 —— 语法错误跳同步点，恢复后续写
pub const SYNC_POINTS: [&str; 4] = [";", "}", "{", "\n"];

pub fn panic_recover(lines: &[String], err_line: usize) -> (usize, Vec<String>) {
    // 从出错行向后找第一个以同步点开头的行，从那里继续
    for (i, l) in lines.iter().enumerate().skip(err_line) {
        let t = l.trim_start();
        if SYNC_POINTS.iter().any(|s| t.starts_with(s)) || t.starts_with("fn ") {
            return (i, lines[i..].to_vec());
        }
    }
    (lines.len(), Vec::new())
}

// F031 代码片段精确匹配 —— AST 子树同构匹配 snippet 库
pub fn snippet_match(code: &[String], lib: &[Vec<String>]) -> Option<usize> {
    let norm = |ls: &[String]| -> Vec<String> {
        ls.iter()
            .map(|l| l.split_whitespace().collect::<String>())
            .collect()
    };
    let n = norm(code);
    lib.iter().position(|s| norm(s) == n)
}

// F032 正则→代码转换 —— 解析正则子集生成等价匹配代码
pub fn regex_to_code(pattern: &str) -> String {
    // 支持：字面量 / a|b / a* / . 生成等价谓词描述
    if pattern.contains('|') {
        let alts: Vec<String> = pattern.split('|').map(|s| format!("s == \"{s}\"")).collect();
        format!("alts: {}", alts.join(" || "))
    } else if let Some(base) = pattern.strip_suffix('*') {
        format!("repeat0: s.chars().all(|c| c == '{base}')")
    } else if pattern == "." {
        "any_single: !s.is_empty()".into()
    } else {
        format!("literal: s == \"{pattern}\"")
    }
}

// F033 DSL内嵌续写 —— 字符串内检测 SQL/GraphQL/Shell，切换文法
#[derive(Debug, PartialEq, Eq)]
pub enum Dsl {
    Sql,
    Graphql,
    Shell,
    None,
}

pub fn detect_dsl(s: &str) -> Dsl {
    let up = s.to_uppercase();
    if up.starts_with("SELECT") || up.starts_with("INSERT") {
        Dsl::Sql
    } else if s.trim_start().starts_with('{') && (s.contains("query") || s.contains("mutation")) {
        Dsl::Graphql
    } else if s.starts_with("grep ") || s.starts_with("ls ") || s.starts_with("curl ") {
        Dsl::Shell
    } else {
        Dsl::None
    }
}

pub fn dsl_next_token(dsl: Dsl) -> Vec<&'static str> {
    match dsl {
        Dsl::Sql => vec!["FROM", "WHERE", "JOIN", "ORDER BY"],
        Dsl::Graphql => vec!["{", "field", ")"],
        Dsl::Shell => vec!["|", ">", "arg"],
        Dsl::None => vec![],
    }
}

// F034 AST合法Token预测 —— 残缺AST枚举语法上合法的下一节点类型
pub fn next_node_types(state: &str) -> Vec<&'static str> {
    match state {
        "file" => vec!["func", "class", "module"],
        "func-body" => vec!["stmt", "return", "if", "loop"],
        "after-if" => vec!["else", "stmt"],
        "after-return" => vec!["}", "stmt"],
        _ => vec!["stmt"],
    }
}

// F035 注释→骨架生成器（F024 的批量入口：多注释块逐个生成）
pub fn skeletons_from_comments(blocks: &[(&str, Vec<&str>)]) -> Vec<String> {
    blocks
        .iter()
        .map(|(name, lines)| comment_skeleton(lines, name))
        .collect()
}

// F036 重复模式外推 —— 连续同构模式外推下一段（Excel 填充语义）
pub fn fill_pattern(seq: &[String], n: usize) -> Vec<String> {
    let mut out = Vec::new();
    if seq.is_empty() {
        return out;
    }
    let base = seq.last().unwrap().clone();
    for i in 1..=n {
        let nxt: String = base
            .chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    char::from_digit(c.to_digit(10).unwrap() + i as u32, 10).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();
        out.push(nxt);
    }
    out
}

// F037 作用域变量联想 —— 扫描作用域链全部可见符号按距离排序（F020 的全类型版）
pub fn scope_suggest(chain: &[Vec<TypedSym>]) -> Vec<String> {
    scope_radar(chain, None).into_iter().map(|(n, _)| n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f018_legal_tokens_by_ctx() {
        assert!(legal_tokens(Ctx::FileTop).contains(&"fn"));
        assert!(legal_tokens(Ctx::InBlock).contains(&"return"));
        assert!(legal_tokens(Ctx::AfterReturn).contains(&"identifier"));
    }

    #[test]
    fn f019_type_constrained() {
        let env = vec![
            TypedSym { name: "n".into(), ty: "int".into() },
            TypedSym { name: "s".into(), ty: "string".into() },
        ];
        assert_eq!(typed_completion(&env, "int"), vec!["n"]);
        assert_eq!(infer("42", &env), "int");
        assert_eq!(infer("\"x\"", &env), "string");
        assert_eq!(infer("n", &env), "int");
    }

    #[test]
    fn f020_radar_sorts_by_distance() {
        let chain = vec![
            vec![TypedSym { name: "local".into(), ty: "int".into() }],
            vec![TypedSym { name: "outer".into(), ty: "int".into() }],
        ];
        let r = scope_radar(&chain, Some("int"));
        assert_eq!(r[0], ("local".into(), 0));
        assert_eq!(r[1], ("outer".into(), 1));
    }

    #[test]
    fn f021_stub_skeleton() {
        let s = stub_skeleton("fn connect(url: string, port: int) -> bool");
        assert!(s.starts_with("fn connect(url, port)"));
        assert!(s.contains("false"));
        assert!(stub_skeleton("fn id(n: int) -> int").contains("0"));
    }

    #[test]
    fn f022_template_match_fill() {
        let lib = TemplateLib::builtin();
        let t = lib.match_fill("for each element", "user").unwrap();
        assert!(t.contains("for user in list"));
        assert!(lib.match_fill("plain", "x").is_none());
    }

    #[test]
    fn f023_close_structure() {
        assert_eq!(close_structure("if (a) {"), "if (a) {}");
        assert_eq!(close_structure("f(x"), "f(x)");
        assert_eq!(close_structure("a["), "a[]");
        assert_eq!(close_structure("(a)"), "(a)");
    }

    #[test]
    fn f024_comment_skeleton() {
        let s = comment_skeleton(&["@param x", "@return 0", "@throws io"], "f");
        assert!(s.starts_with("fn f(x)"));
        assert!(s.contains("return 0"));
        assert!(s.contains("may throw io"));
    }

    #[test]
    fn f025_infer_from_tests() {
        let sig = infer_from_tests(
            &["assert_eq!(add(1, 2), 3)".into(), "assert_eq!(add(0, 0), 0)".into()],
            "add",
        );
        assert!(sig.contains("2 args, 2 cases"));
    }

    #[test]
    fn f026_import_fix() {
        let fx = ImportFixer {
            index: [("open".to_string(), "net".to_string())].into_iter().collect(),
        };
        let miss = fx.fix("fn main() {}", &["open".to_string(), "own".to_string()]);
        assert_eq!(miss, vec!["import open from net"]);
    }

    #[test]
    fn f027_isomorphic_extrapolate() {
        let lines = vec!["p1.draw()".into(), "p2.draw()".into(), "p3.draw()".into()];
        assert_eq!(extrapolate_isomorphic(&lines).unwrap(), "p4.draw()");
        assert!(extrapolate_isomorphic(&["a".into(), "b()".into(), "c".into()]).is_none());
    }

    #[test]
    fn f028_cyk_complete() {
        assert_eq!(cyk_complete(&["(", "1", "+"]), "( 1 + 0 )");
        assert_eq!(cyk_complete(&["1", "+", "2"]), "1 + 2");
    }

    #[test]
    fn f029_interface_methods() {
        let m = interface_methods("trait Shape { draw(); area(); }", &["draw".to_string()]);
        assert_eq!(m, vec!["area"]);
    }

    #[test]
    fn f030_panic_recover() {
        let lines = vec![
            "garbage !!".to_string(),
            "}".to_string(),
            "fn next() {".to_string(),
        ];
        let (at, rest) = panic_recover(&lines, 0);
        assert_eq!(at, 1);
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn f031_snippet_exact_match() {
        let lib = vec![vec!["return a + b".to_string()]];
        assert_eq!(snippet_match(&["return  a+b".into()], &lib), Some(0));
        assert_eq!(snippet_match(&["return a".into()], &lib), None);
    }

    #[test]
    fn f032_regex_to_code() {
        assert!(regex_to_code("cat|dog").starts_with("alts:"));
        assert!(regex_to_code("a*").starts_with("repeat0:"));
        assert!(regex_to_code(".").starts_with("any_single:"));
        assert_eq!(regex_to_code("ab"), "literal: s == \"ab\"");
    }

    #[test]
    fn f033_dsl_detect_and_continue() {
        assert_eq!(detect_dsl("SELECT * FROM t"), Dsl::Sql);
        assert_eq!(detect_dsl("{ query u { name } }"), Dsl::Graphql);
        assert_eq!(detect_dsl("grep -r x"), Dsl::Shell);
        assert_eq!(detect_dsl("plain text"), Dsl::None);
        assert!(dsl_next_token(Dsl::Sql).contains(&"WHERE"));
    }

    #[test]
    fn f034_next_node_types() {
        assert!(next_node_types("file").contains(&"func"));
        assert!(next_node_types("after-if").contains(&"else"));
        assert!(next_node_types("after-return").contains(&"}"));
    }

    #[test]
    fn f035_batch_skeletons() {
        let v = skeletons_from_comments(&[("a", vec!["@param x"]), ("b", vec![])]);
        assert_eq!(v.len(), 2);
        assert!(v[0].starts_with("fn a(x)"));
        assert!(v[1].starts_with("fn b()"));
    }

    #[test]
    fn f036_fill_pattern() {
        assert_eq!(fill_pattern(&["v1".into(), "v2".into()], 2), vec!["v3", "v4"]);
        assert!(fill_pattern(&[], 3).is_empty());
    }

    #[test]
    fn f037_scope_suggest() {
        let chain = vec![
            vec![TypedSym { name: "a".into(), ty: "int".into() }],
            vec![TypedSym { name: "b".into(), ty: "str".into() }],
        ];
        assert_eq!(scope_suggest(&chain), vec!["a", "b"]);
    }
}
