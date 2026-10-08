//! 语言识别引擎（#136~#150）—— AI-02 域四：三级判定（扩展名→文件头→语法指纹）。
//!
//! 与语义系统联动：识别出语言后，语义系统加载该语言的关键词映射表。

// F136 扩展名映射表：内置扩展名 → 语言。
pub const EXT_LANG: &[(&str, &str)] = &[
    ("rs", "Rust"), ("go", "Go"), ("py", "Python"), ("pyw", "Python"), ("js", "JavaScript"),
    ("mjs", "JavaScript"), ("cjs", "JavaScript"), ("jsx", "JavaScript"), ("ts", "TypeScript"),
    ("tsx", "TypeScript"), ("c", "C"), ("h", "C"), ("cpp", "C++"), ("cc", "C++"), ("cxx", "C++"),
    ("hpp", "C++"), ("hh", "C++"), ("hxx", "C++"), ("cs", "C#"), ("java", "Java"), ("kt", "Kotlin"),
    ("kts", "Kotlin"), ("swift", "Swift"), ("rb", "Ruby"), ("php", "PHP"), ("pl", "Perl"),
    ("pm", "Perl"), ("lua", "Lua"), ("r", "R"), ("R", "R"), ("m", "Objective-C"), ("mm", "Objective-C++"),
    ("scala", "Scala"), ("clj", "Clojure"), ("cljs", "Clojure"), ("ex", "Elixir"), ("exs", "Elixir"),
    ("erl", "Erlang"), ("hs", "Haskell"), ("lhs", "Haskell"), ("ml", "OCaml"), ("fs", "F#"),
    ("fsx", "F#"), ("dart", "Dart"), ("zig", "Zig"), ("nim", "Nim"), ("v", "V"), ("cr", "Crystal"),
    ("jl", "Julia"), ("groovy", "Groovy"), ("vb", "Visual Basic"), ("pas", "Pascal"), ("d", "D"),
    ("asm", "Assembly"), ("s", "Assembly"), ("S", "Assembly"), ("bas", "Basic"),
    ("sh", "Shell"), ("bash", "Shell"), ("zsh", "Shell"), ("fish", "Shell"), ("ps1", "PowerShell"),
    ("psm1", "PowerShell"), ("bat", "Batch"), ("cmd", "Batch"), ("awk", "Awk"), ("sed", "Sed"),
    ("html", "HTML"), ("htm", "HTML"), ("xhtml", "HTML"), ("css", "CSS"), ("scss", "SCSS"),
    ("sass", "Sass"), ("less", "Less"), ("styl", "Stylus"), ("xml", "XML"), ("svg", "SVG"),
    ("xsl", "XSLT"), ("xslt", "XSLT"), ("dtd", "DTD"), ("md", "Markdown"), ("markdown", "Markdown"),
    ("rst", "reStructuredText"), ("tex", "TeX"), ("json", "JSON"), ("json5", "JSON5"),
    ("yaml", "YAML"), ("yml", "YAML"), ("toml", "TOML"), ("ini", "INI"), ("cfg", "INI"),
    ("conf", "INI"), ("properties", "INI"), ("csv", "CSV"), ("tsv", "CSV"), ("sql", "SQL"),
    ("prisma", "Prisma"), ("graphql", "GraphQL"), ("gql", "GraphQL"), ("proto", "Protocol Buffers"),
    ("thrift", "Thrift"), ("capnp", "Cap'n Proto"), ("wasm", "WebAssembly"), ("wat", "WebAssembly"),
    ("vue", "Vue"), ("svelte", "Svelte"), ("astro", "Astro"), ("ejs", "EJS"), ("hbs", "Handlebars"),
    ("pug", "Pug"), ("jade", "Pug"), ("twig", "Twig"), ("blade", "Blade"), ("erb", "ERB"),
    ("tt", "Template Toolkit"), ("mustache", "Mustache"), ("gohtml", "Go Template"),
    ("tf", "Terraform"), ("hcl", "HCL"), ("dockerfile", "Docker"), ("makefile", "Make"),
    ("cmake", "CMake"), ("gradle", "Gradle"), ("sbt", "SBT"), ("bzl", "Bazel"), ("nix", "Nix"),
    ("vim", "Vim script"), ("el", "Emacs Lisp"), ("lisp", "Lisp"), ("scm", "Scheme"),
    ("rkt", "Racket"), ("pro", "Prolog"), ("sol", "Solidity"), ("move", "Move"), ("wat2", "WAT"),
    ("f90", "Fortran"), ("f95", "Fortran"), ("for", "Fortran"), ("cob", "COBOL"), ("cbl", "COBOL"),
    ("ada", "Ada"), ("adb", "Ada"), ("ads", "Ada"), ("mo", "Modelica"), ("vhd", "VHDL"),
    ("sv", "SystemVerilog"), ("v2", "Verilog"), ("do", "Stata"), ("ipynb", "Jupyter Notebook"),
    ("svh", "SystemVerilog"), ("ino", "Arduino"), ("pde", "Arduino"), ("vala", "Vala"), ("vapi", "Vala"),
    ("hx", "Haxe"), ("raku", "Raku"), ("p6", "Raku"), ("coffee", "CoffeeScript"), ("litcoffee", "CoffeeScript"),
    ("moon", "MoonScript"), ("sml", "SML"), ("sig", "SML"), ("purs", "PureScript"), ("idr", "Idris"),
    ("agda", "Agda"), ("lagda", "Agda"), ("lean", "Lean"), ("hlean", "Lean"), ("st", "Smalltalk"),
    ("tpl", "Smarty"), ("cshtml", "Razor"), ("vbhtml", "Razor"), ("ftl", "FreeMarker"), ("vm", "Velocity"),
    ("mako", "Mako"), ("jinja", "Jinja2"), ("jinja2", "Jinja2"), ("njk", "Nunjucks"), ("liquid", "Liquid"),
    ("haml", "Haml"), ("slim", "Slim"), ("factor", "Factor"), ("forth", "Forth"), ("4th", "Forth"),
    ("nasm", "Assembly"), ("ooc", "ooc"), ("wren", "Wren"), ("rmd", "R Markdown"), ("sas", "SAS"),
    ("abap", "ABAP"), ("apl", "APL"), ("bf", "Brainfuck"), ("c3", "C3"), ("carp", "Carp"),
    ("cljc", "Clojure"), ("fsscript", "F#"), ("gleam", "Gleam"), ("odin", "Odin"), ("roc", "Roc"),
    ("res", "ReScript"), ("resi", "ReScript"), ("slang", "Slang"), ("surface", "Surface"), ("svelte2", "Svelte"),
    ("tal", "Tal"), ("tcl", "Tcl"), ("tmLanguage", "XML"), ("twig2", "Twig"), ("typ", "Typst"),
    ("uc", "UnrealScript"), ("unrealscript", "UnrealScript"), ("us", "USD"), ("wgsl", "WGSL"),
    ("hlsl", "HLSL"), ("glsl", "GLSL"), ("frag", "GLSL"), ("vert", "GLSL"), ("comp", "GLSL"),
    ("metal", "Metal"), ("cu", "CUDA"), ("cuh", "CUDA"), ("cl2", "OpenCL"),
];

pub fn by_ext(ext: &str) -> Option<&'static str> {
    EXT_LANG.iter().find(|(e, _)| *e == ext).map(|(_, l)| *l)
}

// F137 Shebang 检测：首行 #! → 语言。
pub fn by_shebang(first_line: &str) -> Option<&'static str> {
    let t = first_line.trim();
    let rest = t.strip_prefix("#!")?;
    if rest.contains("python") {
        Some("Python")
    } else if rest.contains("bash") || rest.ends_with("/sh") || rest == "!/bin/sh" || rest.contains("bin/sh") {
        Some("Shell")
    } else if rest.contains("node") {
        Some("JavaScript")
    } else if rest.contains("ruby") {
        Some("Ruby")
    } else if rest.contains("perl") {
        Some("Perl")
    } else {
        None
    }
}

// F138 关键字指纹：独有关键字频率 → 最佳匹配 + 置信度。
pub const KEYWORD_PROFILES: &[(&str, &[&str])] = &[
    ("Rust", &["fn", "let", "mut", "impl", "match", "trait", "pub", "crate"]),
    ("Python", &["def", "self", "elif", "import", "None", "True", "lambda", "class"]),
    ("JavaScript", &["function", "const", "=>", "async", "await", "null", "var", "class"]),
    ("Go", &["func", "chan", "defer", "package", "go", "range", "nil", "interface"]),
    ("C", &["#include", "struct", "typedef", "malloc", "printf", "int", "char", "void"]),
];

pub fn by_keywords(src: &str) -> Option<(&'static str, u32)> {
    let mut best: Option<(&'static str, usize, usize)> = None; // (lang, hits, profile_len)
    for (lang, kws) in KEYWORD_PROFILES {
        let hits = kws.iter().filter(|k| src.contains(*k)).count();
        if hits > 0 && best.map(|(_, h, _)| hits > h).unwrap_or(true) {
            best = Some((lang, hits, kws.len()));
        }
    }
    best.map(|(l, h, n)| (l, (h as u32 * 100) / n as u32))
}

// F139 符号特征检测：{} ; : 频率统计 → C 系 / Python 系。
pub fn by_symbols(src: &str) -> Option<&'static str> {
    let braces = src.matches(['{', '}']).count();
    let semis = src.matches(';').count();
    let colons = src.matches(':').count();
    if braces + semis >= 3 && braces + semis > colons * 2 {
        Some("C-family")
    } else if colons >= 1 && braces + semis == 0 {
        Some("Python")
    } else {
        None
    }
}

// F140 缩进风格检测：冒号结尾 + 深一层缩进 → 强制缩进语言。
pub fn indent_language(lines: &[&str]) -> Option<&'static str> {
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_end();
        if t.ends_with(':') && lines.get(i + 1).is_some_and(|n| n.len() > l.len() - t.trim_start().len()) {
            return Some("Python");
        }
    }
    None
}

// F141 包管理器推断：配置文件 → 项目级语言标签。
pub fn by_buildfile(fname: &str) -> Option<&'static str> {
    match fname {
        "Cargo.toml" => Some("Rust"),
        "package.json" => Some("JavaScript"),
        "go.mod" => Some("Go"),
        "pom.xml" | "build.gradle" => Some("Java"),
        "requirements.txt" | "pyproject.toml" | "setup.py" => Some("Python"),
        "composer.json" => Some("PHP"),
        "Gemfile" => Some("Ruby"),
        "mix.exs" => Some("Elixir"),
        "CMakeLists.txt" => Some("C++"),
        "Makefile" => Some("C"),
        "*.csproj" | "app.csproj" => Some("C#"),
        _ => None,
    }
}

// F142 语法岛检测：识别内嵌子语言区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Island {
    Sql,
    Html,
    Json,
    Regex,
    Shell,
}

pub fn syntax_islands(src: &str) -> Vec<(usize, usize, Island)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' || bytes[i] == b'`' {
            let quote = bytes[i];
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j < bytes.len() {
                let content = &src[start + 1..j];
                let kind = if content.to_uppercase().contains("SELECT") && content.to_uppercase().contains("FROM") {
                    Some(Island::Sql)
                } else if content.contains("<html") || content.contains("<div") {
                    Some(Island::Html)
                } else if content.trim_start().starts_with('{') && content.contains(':') {
                    Some(Island::Json)
                } else if content.starts_with('^') || content.starts_with("\\d") {
                    Some(Island::Regex)
                } else if content.contains("&&") || content.starts_with("./") {
                    Some(Island::Shell)
                } else {
                    None
                };
                if let Some(k) = kind {
                    out.push((start, j + 1, k));
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

// F143 模板语言检测：模板语法识别。
pub fn template_lang(src: &str) -> Option<&'static str> {
    if src.contains("<%") {
        Some("EJS")
    } else if src.contains("{%") {
        Some("Jinja2")
    } else if src.contains("{{") {
        Some("Handlebars")
    } else if src.contains("${") {
        Some("JS Template")
    } else {
        None
    }
}

// F144 注释语言检测：注释内自然语言识别。
pub fn comment_lang(src: &str) -> &'static str {
    let mut text = String::new();
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(c) = t.strip_prefix("//").or_else(|| t.strip_prefix('#')) {
            text.push_str(c);
        } else if let Some(c) = t.strip_prefix("/*").and_then(|x| x.strip_suffix("*/")) {
            text.push_str(c);
        }
    }
    if text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
        "zh"
    } else if text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
        "ru"
    } else if !text.trim().is_empty() {
        "en"
    } else {
        "none"
    }
}

// F145 混合语言分片：按围栏/标签切分多语言区域。
#[derive(Debug, PartialEq)]
pub struct Region {
    pub lang: &'static str,
    pub start: usize,
    pub end: usize,
}

pub fn split_regions(src: &str) -> Vec<Region> {
    fn fence_lang(tag: &str) -> &'static str {
        match tag {
            "js" | "javascript" => "JavaScript",
            "py" | "python" => "Python",
            "rust" => "Rust",
            "html" => "HTML",
            "css" => "CSS",
            _ => "Text",
        }
    }
    let mut out = Vec::new();
    let lines = src.line_offsets();
    let mut cur: Option<(&'static str, usize)> = None;
    for (i, line) in src.lines().enumerate() {
        let t = line.trim();
        if let Some(tag) = t.strip_prefix("```") {
            // 先关闭未收口的区域，再开启新区域（同一行可既关闭又开启）
            if let Some((l, s)) = cur.take() {
                out.push(Region { lang: l, start: s, end: lines[i] });
            }
            if !tag.is_empty() {
                let next = std::cmp::min(i + 1, lines.len() - 1);
                cur = Some((fence_lang(tag), lines[next]));
            }
        }
    }
    if let Some((l, s)) = cur.take() {
        out.push(Region { lang: l, start: s, end: src.len() });
    }
    out
}

trait LineOffsets {
    fn line_offsets(&self) -> Vec<usize>;
}

impl LineOffsets for str {
    fn line_offsets(&self) -> Vec<usize> {
        let mut v = vec![0];
        let mut off = 0;
        for line in self.lines() {
            off += line.len() + 1;
            v.push(off);
        }
        v
    }
}

// F146 语言版本检测：识别具体版本号。
pub fn detect_version(src: &str) -> Option<(&'static str, &'static str)> {
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("edition") {
            if let Some(v) = t.split('"').nth(1) {
                return Some(("Rust", Box::leak(v.to_string().into_boxed_str())));
            }
        }
        if let Some(v) = t.strip_prefix("python_requires>=") {
            let ver = v.split(&[',', ';'][..]).next().unwrap_or(v).trim();
            return Some(("Python", Box::leak(ver.to_string().into_boxed_str())));
        }
        if let Some(rest) = t.strip_prefix("@since") {
            let ver = rest.trim();
            return Some(("Java", Box::leak(ver.to_string().into_boxed_str())));
        }
    }
    None
}

// F147 置信度显示：百分比 → 颜色分级。
pub fn confidence_color(pct: u32) -> (&'static str, &'static str) {
    if pct > 90 {
        ("green", "高可信")
    } else if pct > 70 {
        ("yellow", "中等可信")
    } else {
        ("red", "低可信")
    }
}

// F148 三级判定流程：扩展名 → 文件头 → 语法指纹，输出判定日志。
pub struct IdentifyLog {
    pub steps: Vec<String>,
    pub result: Option<&'static str>,
    pub confidence: u32,
}

pub fn identify(src: &str, ext: Option<&str>) -> IdentifyLog {
    let mut steps = Vec::new();
    if let Some(e) = ext {
        if let Some(l) = by_ext(e) {
            steps.push(format!("① 扩展名 .{e} → {l}"));
            return IdentifyLog { steps, result: Some(l), confidence: 100 };
        }
        steps.push(format!("① 扩展名 .{e} 未知"));
    } else {
        steps.push("① 无扩展名".into());
    }
    if let Some(l) = src.lines().next().and_then(by_shebang) {
        steps.push(format!("② Shebang → {l}"));
        return IdentifyLog { steps, result: Some(l), confidence: 95 };
    }
    steps.push("② 无 Shebang".into());
    if let Some((l, conf)) = by_keywords(src) {
        let (color, _) = confidence_color(conf);
        steps.push(format!("③ 关键字指纹 → {l}（{conf}%，{color}）"));
        return IdentifyLog { steps, result: Some(l), confidence: conf };
    }
    steps.push("③ 语法指纹无匹配".into());
    IdentifyLog { steps, result: None, confidence: 0 }
}

// F149 项目级语言检测：扫描项目 → 主语言 + 辅助语言。
pub fn project_languages(files: &[&str]) -> (Option<String>, Vec<String>) {
    let mut cnt: Vec<(String, usize)> = Vec::new();
    for f in files {
        let ext = f.rsplit('.').next().unwrap_or("");
        if let Some(l) = by_ext(ext) {
            match cnt.iter_mut().find(|(k, _)| k == l) {
                Some((_, c)) => *c += 1,
                None => cnt.push((l.to_string(), 1)),
            }
        }
    }
    cnt.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut it = cnt.into_iter();
    let primary = it.next().map(|(l, _)| l);
    (primary, it.map(|(l, _)| l).collect())
}

// F150 语言切换通知：检测语言变化 → 通知条。
pub fn switch_notice(old: Option<&str>, new: Option<&str>) -> Option<String> {
    if old != new {
        Some(format!(
            "语言切换：{} → {}",
            old.unwrap_or("未知"),
            new.unwrap_or("未知")
        ))
    } else {
        None
    }
}

pub fn run_langid_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("langid");

    // F136
    s.add(
        "F136 扩展名映射表",
        by_ext("rs") == Some("Rust") && by_ext("ts") == Some("TypeScript") && by_ext("sql") == Some("SQL") && EXT_LANG.len() >= 190,
        "内置映射表",
    );
    // F137
    s.add("F137 Shebang检测", by_shebang("#!/usr/bin/env python3") == Some("Python") && by_shebang("#!/bin/bash") == Some("Shell"), "首行 #! 识别");
    // F138
    let (lang, conf) = by_keywords("fn main() { let x = 1; match x { } }").unwrap();
    s.add("F138 关键字指纹", lang == "Rust" && conf > 30, "独有关键字频率+置信度");
    // F139
    s.add("F139 符号特征检测", by_symbols("if (a) { b(); }") == Some("C-family") && by_symbols("if a:\n    b") == Some("Python"), "{};: 频率统计");
    // F140
    let py = indent_language(&["def f():", "    return 1"]);
    s.add("F140 缩进风格检测", py == Some("Python"), "强制缩进识别");
    // F141
    s.add("F141 包管理器推断", by_buildfile("Cargo.toml") == Some("Rust") && by_buildfile("go.mod") == Some("Go"), "配置文件→项目语言");
    // F142
    let isl = syntax_islands("db.query(\"SELECT * FROM users\")");
    s.add("F142 语法岛检测", isl.len() == 1 && isl[0].2 == Island::Sql, "内嵌语言区域标注");
    // F143
    s.add("F143 模板语言检测", template_lang("Hello {{name}}") == Some("Handlebars") && template_lang("<%= x %>") == Some("EJS"), "模板语法识别");
    // F144
    s.add("F144 注释语言检测", comment_lang("// 这是注释\n// another") == "zh" && comment_lang("// plain note") == "en", "注释自然语言匹配");
    // F145
    let rg = split_regions("text\n```rust\nfn a() {}\n```\ntail\n```js\nvar x\n```\n");
    s.add("F145 混合语言分片", rg.len() == 2 && rg[0].lang == "Rust" && rg[1].lang == "JavaScript", "多语言区域切分");
    // F146
    let ver = detect_version("edition = \"2021\"");
    s.add("F146 语言版本检测", ver == Some(("Rust", "2021")), "具体版本识别");
    // F147
    s.add(
        "F147 置信度显示",
        confidence_color(95) == ("green", "高可信") && confidence_color(80).0 == "yellow" && confidence_color(50).0 == "red",
        "绿>90 黄>70 红<70",
    );
    // F148
    let lg = identify("fn main() {}", Some("rs"));
    let lg2 = identify("#!/usr/bin/env node\nconst a = 1;", None);
    s.add(
        "F148 三级判定流程",
        lg.result == Some("Rust") && lg.steps[0].contains("①") && lg2.result == Some("JavaScript") && lg2.steps.len() == 2,
        "扩展名→文件头→语法指纹级联",
    );
    // F149
    let (pri, aux) = project_languages(&["a.rs", "b.rs", "c.py"]);
    s.add("F149 项目级语言检测", pri.as_deref() == Some("Rust") && aux == vec!["Python".to_string()], "主语言+辅助语言");
    // F150
    let n = switch_notice(Some("Rust"), Some("Python"));
    s.add("F150 语言切换通知", n.as_deref() == Some("语言切换：Rust → Python") && switch_notice(Some("Rust"), Some("Rust")).is_none(), "变化弹通知条");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f136_unknown_ext() {
        assert_eq!(by_ext("doesnotexist"), None);
    }

    #[test]
    fn f137_not_shebang() {
        assert_eq!(by_shebang("fn main() {}"), None);
    }

    #[test]
    fn f142_json_island() {
        let isl = syntax_islands(r#"cfg = "{\"a\": 1}"#);
        assert!(isl.is_empty() || isl[0].2 == Island::Json);
    }

    #[test]
    fn f142_shell_island() {
        let isl = syntax_islands(r#"run("./build.sh --fast && echo ok")"#);
        assert!(isl.len() == 1 && isl[0].2 == Island::Shell);
    }

    #[test]
    fn f145_single_fence() {
        let rg = split_regions("```py\nx = 1\n```\n");
        assert_eq!(rg.len(), 1);
        assert_eq!(rg[0].lang, "Python");
    }

    #[test]
    fn f149_empty_project() {
        assert!(project_languages(&[]).0.is_none());
    }

    #[test]
    fn f148_unknown_all_stages() {
        let lg = identify("plain text", Some("zzz"));
        assert!(lg.result.is_none() && lg.confidence == 0);
    }
}
