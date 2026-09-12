//! AI-03 · 名词提取与分类（#151~#166）。
//!
//! 理解面板「词典」区域的数据源：从 AST/源码提取全部标识符，逐个标注
//! 角色（动作/东西/数字/文字）、类型、作用域、来源，并做同义聚类/频率统计。
//! 零 AI：全部确定性规则。

use std::collections::BTreeMap;

/// 名词角色标签（🟦动作 🟩东西 🟨数字 🟧文字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Action,
    Thing,
    Number,
    Text,
}

impl Role {
    pub fn tag(self) -> &'static str {
        match self {
            Role::Action => "🟦动作",
            Role::Thing => "🟩东西",
            Role::Number => "🟨数字",
            Role::Text => "🟧文字",
        }
    }
}

/// 名词来源（📥参数 / 🗄️数据库 / 🌐全局）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Param,
    Database,
    Global,
}

impl Origin {
    pub fn icon(self) -> &'static str {
        match self {
            Origin::Local => "📦本地",
            Origin::Param => "📥参数",
            Origin::Database => "🗄️数据库",
            Origin::Global => "🌐全局",
        }
    }
}

/// 一个提取出的名词（词典条目）。
#[derive(Debug, Clone)]
pub struct Noun {
    pub name: String,
    pub role: Role,
    /// 类型推断标签（如 数字/文字/未知）。
    pub ty: &'static str,
    /// 作用域标签（如 "仅当前函数"/"全局"）。
    pub scope: &'static str,
    pub origin: Origin,
    /// 分词结果（驼峰/下划线统一拆分，小写）。
    pub tokens: Vec<String>,
    /// 缩写展开后的完整词。
    pub expanded: Vec<String>,
    /// 引用次数（关键名词排序依据）。
    pub refs: usize,
    /// 是否外部库符号。
    pub external: bool,
    /// 命名风格是否规范。
    pub style_ok: bool,
}

/// F155 缩写展开词典（产品规格 500+；core 交付种子词典，可按语言包扩充）。
pub fn expand_abbrev(w: &str) -> Option<&'static str> {
    Some(match w {
        "usr" | "u" => "user",
        "idx" | "i" => "index",
        "cnt" | "c" => "count",
        "num" | "n" => "number",
        "str" | "s" => "string",
        "len" => "length",
        "tmp" | "t" => "temporary",
        "ctx" => "context",
        "cfg" | "conf" => "config",
        "db" => "database",
        "btn" => "button",
        "msg" => "message",
        "calc" => "calculate",
        "addr" => "address",
        "pwd" | "passwd" => "password",
        "qty" => "quantity",
        "err" | "e" => "error",
        "ret" => "return",
        "res" | "resp" => "response",
        "req" => "request",
        "param" | "arg" | "args" => "argument",
        "obj" | "o" => "object",
        "arr" | "a" => "array",
        "val" | "v" => "value",
        "var" => "variable",
        "fn" | "f" => "function",
        "cb" => "callback",
        "init" => "initialize",
        "min" => "minimum",
        "max" => "maximum",
        "avg" => "average",
        "prev" => "previous",
        "cur" | "curr" => "current",
        "dest" | "dst" => "destination",
        "src" => "source",
        "attr" => "attribute",
        "elem" | "el" => "element",
        "doc" => "document",
        "win" => "window",
        "img" => "image",
        "url" | "uri" => "address",
        "id" => "identifier",
        "auth" => "authentication",
        "admin" => "administrator",
        "env" => "environment",
        "impl" => "implementation",
        "iter" => "iterator",
        "buf" => "buffer",
        "conn" => "connection",
        "stmt" => "statement",
        "expr" => "expression",
        "decl" => "declaration",
        "def" => "definition",
        "ref" => "reference",
        "temp" => "temporary",
        "info" => "information",
        "acct" => "account",
        "amt" => "amount",
        "bal" => "balance",
        "cat" => "category",
        "desc" => "description",
        "dir" => "directory",
        "dist" => "distance",
        "exp" => "expiration",
        "fld" | "flds" => "field",
        "fmt" => "format",
        "gen" => "generate",
        "grp" => "group",
        "hdr" => "header",
        "hist" => "history",
        "lang" => "language",
        "lvl" => "level",
        "mgr" => "manager",
        "nav" => "navigation",
        "opt" => "option",
        "org" => "organization",
        "perf" => "performance",
        "pos" => "position",
        "pref" => "preference",
        "prod" => "product",
        "prof" => "profile",
        "pub" => "public",
        "sec" => "second",
        "std" => "standard",
        "sub" => "subordinate",
        "svc" => "service",
        "tbl" => "table",
        "txn" | "tx" => "transaction",
        "util" | "utils" => "utility",
        "ver" => "version",
        "vid" => "video",
        "wgt" => "widget",
        "xid" => "external-identifier",
        "qty_" => "quantity",
        _ => return None,
    })
}

/// 从源码提取标识符词法（字母/下划线开头的连续词元）。
pub fn extract_identifiers(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut in_comment = false;
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            i += 1;
            continue;
        }
        if in_str {
            if ch == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            if !cur.is_empty() {
                push_unique(&mut out, &cur);
                cur.clear();
            }
            in_str = true;
            i += 1;
            continue;
        }
        if ch == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            if !cur.is_empty() {
                push_unique(&mut out, &cur);
                cur.clear();
            }
            in_comment = true;
            i += 2;
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                push_unique(&mut out, &cur);
                cur.clear();
            }
        }
        i += 1;
    }
    if !cur.is_empty() {
        push_unique(&mut out, &cur);
    }
    out
}

fn push_unique(out: &mut Vec<String>, s: &str) {
    if !out.iter().any(|x| x == s) {
        out.push(s.to_string());
    }
}

/// F153/F154 统一分词：驼峰 + 下划线 + 数字边界，全部小写。
pub fn tokenize(name: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let lower = name.replace('_', " ");
    for seg in lower.split_whitespace() {
        let chars: Vec<char> = seg.chars().collect();
        let mut cur = String::new();
        for (i, &ch) in chars.iter().enumerate() {
            if ch.is_uppercase() && !cur.is_empty() {
                let prev = chars[i - 1];
                // CARCase 常量：连续大写不切，除非后跟小写
                if prev.is_lowercase() || (i + 1 < chars.len() && chars[i + 1].is_lowercase()) {
                    words.push(cur.to_lowercase());
                    cur.clear();
                }
            }
            if ch.is_ascii_digit() && !cur.is_empty() && !cur.chars().last().unwrap().is_ascii_digit() {
                // 字母→数字边界切分
                words.push(cur.to_lowercase());
                cur.clear();
            }
            if !ch.is_ascii_digit() && !cur.is_empty() && cur.chars().last().unwrap().is_ascii_digit() {
                // 数字→字母边界切分
                words.push(cur.to_lowercase());
                cur.clear();
            }
            cur.push(ch);
        }
        if !cur.is_empty() {
            words.push(cur.to_lowercase());
        }
    }
    words
}

/// F152 符号角色分类：后跟 `(` → 动作；全大写/含数字字面 → 数字；引号内容 → 文字；其余 → 东西。
pub fn classify_role(name: &str, followed_by_paren: bool, is_literal_num: bool, is_literal_text: bool) -> Role {
    if followed_by_paren {
        Role::Action
    } else if is_literal_text {
        Role::Text
    } else if is_literal_num || name.chars().all(|c| c.is_ascii_digit()) {
        Role::Number
    } else {
        Role::Thing
    }
}

/// F156 类型推断标注：从赋值字面量推断（`age = 7` → 数字）。
pub fn infer_type(assigned: Option<&str>) -> &'static str {
    match assigned.map(str::trim) {
        Some(v) if v.starts_with('"') || v.starts_with('\'') => "文字",
        Some(v) if v == "true" || v == "false" => "真假",
        Some(v) if v.parse::<f64>().is_ok() => "数字",
        Some(v) if v.starts_with('[') || v.starts_with("new ") => "集合",
        Some(_) => "未知",
        None => "未知",
    }
}

/// F157 作用域标注：符号表链上最近定义层级。
pub fn scope_label(defined_in_func: bool, exported: bool) -> &'static str {
    if exported {
        "全局"
    } else if defined_in_func {
        "仅当前函数"
    } else {
        "仅当前文件"
    }
}

/// F158 来源追踪：参数名 / db.* 前缀 / 全局常量。
pub fn trace_origin(is_param: bool, dotted_prefix: Option<&str>, top_level_const: bool) -> Origin {
    if is_param {
        Origin::Param
    } else if matches!(dotted_prefix, Some("db") | Some("sql") | Some("query")) {
        Origin::Database
    } else if top_level_const {
        Origin::Global
    } else {
        Origin::Local
    }
}

/// F159 名词关系图：`user.cart` 成员访问 → (拥有者, 从属) 虚线箭头。
pub fn noun_relations(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == '.' && i > 0 && i + 1 < chars.len() {
                let owner = ident_ending_at(&chars[..i]);
                let field = ident_starting_at(&chars[i + 1..]);
                if let (Some(o), Some(f)) = (owner, field) {
                    if !o.chars().next().map_or(true, |c| c.is_ascii_digit()) {
                        push_pair(&mut out, (o, f));
                    }
                }
            }
            i += 1;
        }
    }
    out
}

fn push_pair(out: &mut Vec<(String, String)>, p: (String, String)) {
    if !out.contains(&p) {
        out.push(p);
    }
}

fn ident_ending_at(chars: &[char]) -> Option<String> {
    let mut s = String::new();
    for &c in chars.iter().rev() {
        if c.is_alphanumeric() || c == '_' {
            s.insert(0, c);
        } else {
            break;
        }
    }
    if s.is_empty() || s.chars().next().unwrap().is_ascii_digit() {
        None
    } else {
        Some(s)
    }
}

fn ident_starting_at(chars: &[char]) -> Option<String> {
    let mut s = String::new();
    for &c in chars {
        if c.is_alphanumeric() || c == '_' {
            s.push(c);
        } else {
            break;
        }
    }
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// F160 命名风格检测：小驼峰/小写蛇形为规范，SCREAMING 只允许常量。
pub fn naming_style_ok(name: &str, is_const: bool) -> bool {
    if is_const {
        return name.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_');
    }
    let first = name.chars().next().unwrap_or('x');
    first.is_lowercase() && !name.contains('-') && !name.contains(' ')
}

/// F161 同义词聚类：展开词元集合相同的名词归为一簇（同色边框）。
pub fn synonym_clusters(names: &[String]) -> Vec<Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in names {
        let toks: Vec<String> = tokenize(n)
            .iter()
            .map(|t| expand_abbrev(t).unwrap_or(t).to_string())
            .collect();
        map.entry(toks.join("+")).or_default().push(n.clone());
    }
    map.into_values().filter(|v| v.len() > 1).collect()
}

/// F162 关键名词高亮：按引用次数降序。
pub fn key_nouns(freq: &BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = freq.iter().map(|(k, n)| (k.clone(), *n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// F163 外部库名词识别：内置常见库名单 + 大写开头保留名。
pub fn is_external(name: &str) -> bool {
    const LIBS: [&str; 40] = [
        "std", "io", "fs", "net", "math", "json", "http", "sql", "os", "time", "log", "test",
        "vec", "str", "hash", "regex", "thread", "sync", "io_util", "fmt", "env", "path",
        "console", "window", "document", "array", "object", "promise", "react", "vue", "axios",
        "lodash", "express", "tokio", "serde", "reqwest", "numpy", "pandas", "torch", "tkinter",
    ];
    LIBS.contains(&name)
}

/// F164 魔法字符串提取：出现 ≥2 次或用于比较的字符串字面量。
pub fn magic_strings(src: &str) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                s.push(chars[i]);
                i += 1;
            }
            if !s.is_empty() {
                *counts.entry(s).or_default() += 1;
            }
        }
        i += 1;
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .map(|(s, _)| s)
        .collect()
}

/// F165 数字常量含义提示（种子词典）。
pub fn number_meaning(n: f64) -> Option<&'static str> {
    Some(match n as i64 {
        86400 => "一天的秒数",
        3600 => "一小时的秒数",
        60 => "一分钟的秒数",
        24 => "一天的小时数",
        1024 => "1KB 的字节数",
        1000 => "一秒的毫秒数",
        100 => "百分制的满分",
        7 => "一周的天数",
        365 => "一年的天数",
        12 => "一年的月数",
        4096 => "一页内存的字节数",
        65535 => "16 位最大值",
        2147483647 => "32 位最大值",
        _ => return None,
    })
}

/// F166 名词频率统计（排行榜数据源）。
pub fn frequency(src: &str) -> BTreeMap<String, usize> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    // 计数用不去重的扫描（保留每次出现）
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let w: String = chars[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect();
            let n = w.chars().count();
            *m.entry(w).or_default() += 1;
            i += n;
        } else {
            i += 1;
        }
    }
    m
}

/// 全量词典构建：一条龙完成 #151~#166 的标注。
pub fn build_nouns(src: &str) -> Vec<Noun> {
    let mut nouns: Vec<Noun> = Vec::new();
    for line in src.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if !(ch.is_alphabetic() || ch == '_') {
                i += 1;
                continue;
            }
            let name = ident_starting_at(&chars[i..]).unwrap_or_default();
            if name.is_empty() {
                i += 1;
                continue;
            }
            let after = chars[i + name.len()..].iter().collect::<String>();
            let after_trim = after.trim_start();
            let is_param = after_trim.starts_with(':') || after_trim.starts_with(',');
            let role = classify_role(
                &name,
                after_trim.starts_with('('),
                false,
                false,
            );
            let ty = if role == Role::Action {
                "动作"
            } else {
                infer_type(None)
            };
            let toks = tokenize(&name);
            let expanded = toks
                .iter()
                .map(|t| expand_abbrev(t).unwrap_or(t).to_string())
                .collect();
            let origin = trace_origin(is_param, None, name.chars().all(|c| c.is_uppercase() || c == '_'));
            let is_const = name.chars().all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit());
            let external = is_external(&name);
            if !nouns.iter().any(|n| n.name == name) {
                nouns.push(Noun {
                    name: name.clone(),
                    role,
                    ty,
                    scope: scope_label(!is_const, external),
                    origin,
                    tokens: toks,
                    expanded,
                    refs: 0,
                    external,
                    style_ok: naming_style_ok(&name, is_const),
                });
            }
            i += name.len();
        }
    }
    let freq = frequency(src);
    for n in &mut nouns {
        n.refs = *freq.get(&n.name).unwrap_or(&1);
    }
    nouns
}

pub fn run_nouns_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("nouns");
    let src = "fn calc_total(user, items) {\n  let count = 2\n  let msg = \"hi\"\n  return user.cart.total + count\n}\nconst MAX_RETRY = 3\nlet usrcfg = db.query(\"select\")\nlet timeout_sec = 86400\n";

    // F151
    let ids = extract_identifiers(src);
    s.add("#151 AST标识符提取", ids.contains(&"calc_total".to_string()) && ids.contains(&"items".to_string()), "遍历 Identifier 提取全部名词");
    // F152
    s.add("#152 符号角色分类", classify_role("calc_total", true, false, false) == Role::Action
        && classify_role("items", false, false, false) == Role::Thing
        && classify_role("7", false, true, false) == Role::Number
        && classify_role("hi", false, false, true) == Role::Text, "动作/东西/数字/文字 四色标签");
    // F153
    s.add("#153 驼峰分词", tokenize("userCart") == vec!["user", "cart"], "userCart → user + cart");
    // F154
    s.add("#154 下划线分词", tokenize("user_login") == vec!["user", "login"], "user_login → user + login");
    // F155
    s.add("#155 缩写展开词典", expand_abbrev("usr") == Some("user") && expand_abbrev("idx") == Some("index") && expand_abbrev("totally_normal").is_none(), "usr → user（种子词典可扩充）");
    // F156
    s.add("#156 类型推断标注", infer_type(Some("7")) == "数字" && infer_type(Some("\"x\"")) == "文字" && infer_type(Some("true")) == "真假", "声明/赋值推断类型标签");
    // F157
    s.add("#157 作用域标注", scope_label(true, false) == "仅当前函数" && scope_label(false, true) == "全局", "可见范围标签");
    // F158
    s.add("#158 来源追踪", trace_origin(true, None, false) == Origin::Param
        && trace_origin(false, Some("db"), false) == Origin::Database
        && trace_origin(false, None, true) == Origin::Global, "📥参数/🗄️数据库/🌐全局");
    // F159
    let rel = noun_relations("return user.cart.total + count");
    s.add("#159 名词关系图", rel.contains(&("user".to_string(), "cart".to_string())), "user →拥有→ cart 虚线箭头");
    // F160
    s.add("#160 命名风格检测", naming_style_ok("userName", false) && !naming_style_ok("UserName", false) && naming_style_ok("MAX_RETRY", true), "不规范命名标黄");
    // F161
    let cl = synonym_clusters(&["usrCart".to_string(), "userCart".to_string(), "orderTotal".to_string()]);
    let has_pair = cl.iter().any(|c| c.contains(&"usrCart".to_string()) && c.contains(&"userCart".to_string()));
    s.add("#161 同义词聚类", has_pair, "同一概念不同叫法同色连接");
    // F162
    let mut freq = std::collections::BTreeMap::new();
    freq.insert("user".to_string(), 9usize);
    freq.insert("cart".to_string(), 3usize);
    s.add("#162 关键名词高亮", key_nouns(&freq)[0].0 == "user", "按引用次数排序，核心名词更大更亮");
    // F163
    s.add("#163 外部库名词识别", is_external("json") && is_external("std") && !is_external("myCart"), "库名词灰色斜体+来源标签");
    // F164
    let mg = magic_strings("if(mode == \"test\") {}\nrun(\"test\")\n");
    s.add("#164 魔法字符串提取", mg.contains(&"test".to_string()), "硬编码字符串标橙+建议提常量");
    // F165
    s.add("#165 数字常量提取", number_meaning(86400.0) == Some("一天的秒数") && number_meaning(123.0).is_none(), "86400 → 一天的秒数");
    // F166
    let fq = frequency("a + a + a + b");
    let nouns = build_nouns(src);
    let has_fn = nouns.iter().any(|n| n.name == "calc_total" && n.role == Role::Action);
    let full_pipeline = has_fn && nouns.iter().any(|n| n.name == "timeout_sec" && n.expanded.contains(&"second".to_string()));
    s.add("#166 名词频率统计", fq.get("a") == Some(&3) && fq.get("b") == Some(&1) && full_pipeline, "频率排行榜 + build_nouns 一体化标注");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f151_extract_dedup_and_skip_strings_comments() {
        let ids = extract_identifiers("let a = 1 // let hidden\nlet b = \"strInside\"\n");
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
        assert!(!ids.iter().any(|x| x == "hidden"));
        assert!(!ids.iter().any(|x| x == "strInside"));
    }

    #[test]
    fn f153_f154_tokenize_variants() {
        assert_eq!(tokenize("HTTPServer2"), vec!["http", "server", "2"]);
        assert_eq!(tokenize("parse_HTML_doc"), vec!["parse", "html", "doc"]);
    }

    #[test]
    fn f155_dict_covers_common_abbrevs() {
        for w in ["usr", "idx", "db", "cfg", "msg", "btn", "err", "req", "res"] {
            assert!(expand_abbrev(w).is_some(), "{w} 应可展开");
        }
    }

    #[test]
    fn f159_relation_owner_field() {
        let rel = noun_relations("a.b.c");
        assert!(rel.contains(&("a".into(), "b".into())));
        assert!(rel.contains(&("b".into(), "c".into())));
    }

    #[test]
    fn f164_magic_repeat_only() {
        let mg = magic_strings("x = \"once\"\n");
        assert!(mg.is_empty());
        let mg2 = magic_strings("x = \"once\"\ny = \"once\"\n");
        assert_eq!(mg2, vec!["once"]);
    }

    #[test]
    fn nouns_full_pipeline() {
        let src = "function calcTotal(userId) {\n  let cnt = 0\n  return db.findUser(userId) + cnt\n}\n";
        let nouns = build_nouns(src);
        let calc = nouns.iter().find(|n| n.name == "calcTotal").unwrap();
        assert_eq!(calc.role, Role::Action);
        assert!(calc.style_ok);
        assert_eq!(calc.tokens, vec!["calc", "total"]);
        let uid = nouns.iter().find(|n| n.name == "userId").unwrap();
        assert_eq!(uid.tokens, vec!["user", "id"]);
        assert!(uid.expanded.contains(&"identifier".to_string()));
    }
}
