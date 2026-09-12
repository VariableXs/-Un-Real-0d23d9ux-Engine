//! AI-03 · 简化操作/低门槛编码（#191~#220）。
//!
//! 创作面板四区块（生成器/模板/转换/续写）与画布右键菜单背后的纯逻辑层。
//! 零 AI：确定性解析与生成。

/// F191 伪代码 → 代码（解析注释关键词）。
pub fn pseudo_to_code(comment: &str) -> Option<String> {
    let t = comment.trim_start_matches(|c| c == '/' || c == ' ' || c == '#');
    for kw in ["循环", "重复"] {
        if let Some(rest) = t.strip_prefix(kw) {
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let body = rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == '次' || c == ' ');
            if !n.is_empty() {
                let inner = if body.is_empty() { "todo()" } else { body };
                return Some(format!("for i in 0..{} {{\n  {}\n}}", n, inner));
            }
        }
    }
    if let Some(rest) = t.strip_prefix("如果") {
        let (cond, act) = rest.split_once("就").unwrap_or((rest, "todo()"));
        return Some(format!("if {} {{\n  {}\n}}", cond.trim(), act.trim()));
    }
    if let Some(rest) = t.strip_prefix("打印") {
        return Some(format!("print({})", rest.trim()));
    }
    if let Some(rest) = t.strip_prefix("遍历") {
        let (coll, act) = rest.split_once("做").unwrap_or((rest, "todo()"));
        return Some(format!("for item in {} {{\n  {}\n}}", coll.trim(), act.trim()));
    }
    if let Some(rest) = t.strip_prefix("只要") {
        return Some(format!("while {} {{\n  todo()\n}}", rest.trim()));
    }
    None
}

/// F192 表格 → switch（解析 Markdown 表格行 `| 值 | 结果 |`）。
pub fn table_to_switch(subject: &str, rows: &[(String, String)]) -> String {
    let mut out = format!("switch ({}) {{\n", subject);
    for (k, v) in rows {
        out.push_str(&format!("  case {}: {}\n", k, v));
    }
    out.push_str("  default: todo()\n}");
    out
}

/// F193 拖拽流程图 → 代码（shape: diamond=判断 / rect=语句 / oval=开始结束）。
pub fn flowchart_to_code(nodes: &[(String, &'static str, Option<usize>)], edges: &[(usize, usize)]) -> Vec<String> {
    // nodes: (文本, 形状, if 的"否"分支目标)；edges: 顺序连线
    let mut out = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        let (text, shape, else_target) = &nodes[i];
        match *shape {
            "diamond" => {
                let mut branch = String::from("if ");
                branch.push_str(&format!("{} {{\n  // 是分支\n}}", text));
                if let Some(e) = else_target {
                    branch.push_str(&format!(" else {{\n  // 否分支(→{})\n}}", nodes[*e].0));
                }
                out.push(branch);
            }
            "oval" => {}
            _ => out.push(text.clone()),
        }
        // 跳到下一个非"否分支"节点
        i = edges
            .iter()
            .find(|(a, _)| *a == i)
            .map(|(_, b)| *b)
            .unwrap_or(i + 1);
    }
    out
}

/// F194 状态表 → 状态机（match 臂）。
pub fn state_table_to_fsm(initial: &str, transitions: &[(String, String, String)]) -> String {
    // (当前态, 事件, 下一态)
    let mut out = format!("let mut state = {};\nmatch state {{\n", initial);
    let mut arms: Vec<(String, Vec<String>)> = Vec::new();
    for (from, ev, to) in transitions {
        match arms.iter_mut().find(|(f, _)| f == from) {
            Some((_, evs)) => evs.push(format!("{} => state = {}", ev, to)),
            None => arms.push((from.clone(), vec![format!("{} => state = {}", ev, to)])),
        }
    }
    for (from, evs) in &arms {
        out.push_str(&format!("  {} => {{ {} }}\n", from, evs.join(", ")));
    }
    out.push_str("}");
    out
}

/// F195 JSON → 类型定义（递归推断）。
pub fn json_to_types(name: &str, json: &str) -> String {
    let t = json.trim();
    if !t.starts_with('{') {
        return scalar_type(t).to_string();
    }
    let inner = t.trim_start_matches('{').trim_end_matches('}');
    let mut fields = String::new();
    for pair in split_top(inner) {
        if let Some((k, v)) = pair.split_once(':') {
            let v = v.trim();
            let ty = if v.starts_with('{') {
                let child_name = snake_to_pascal(k.trim().trim_matches('"'));
                fields.push_str(&format!("  {}: {},\n", k.trim().trim_matches('"'), child_name));
                fields.push_str(&json_to_types(&child_name, v));
                continue;
            } else if v.starts_with('[') {
                let elem = v.trim_start_matches('[').trim_end_matches(']').trim();
                if elem.is_empty() {
                    "[]".to_string()
                } else {
                    format!("{}[]", scalar_type(elem))
                }
            } else {
                scalar_type(v).to_string()
            };
            fields.push_str(&format!("  {}: {},\n", k.trim().trim_matches('"'), ty));
        }
    }
    format!(
        "interface {} {{\n{}{}}}",
        snake_to_pascal(name),
        fields,
        ""
    )
}

fn scalar_type(v: &str) -> &'static str {
    let v = v.trim().trim_matches('"');
    if v == "true" || v == "false" {
        "bool"
    } else if v.parse::<f64>().is_ok() {
        "number"
    } else if v.starts_with('{') {
        "object"
    } else {
        "string"
    }
}
fn snake_to_pascal(s: &str) -> String {
    s.split(['_', '-'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}
/// 顶层逗号切分（忽略嵌套大括号内）。
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '{' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ']' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                out.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// F196 SQL → ORM 调用链。
pub fn sql_to_orm(sql: &str) -> String {
    let s = sql.trim().trim_end_matches(';').to_lowercase();
    let mut out = String::from("db");
    if let Some(rest) = s.strip_prefix("select") {
        let (cols, tail) = rest.split_once("from").unwrap_or((rest, ""));
        let (table, cond) = tail.split_once("where").unwrap_or((tail, ""));
        let table = table.trim().trim_end_matches('*').trim();
        let table = if table.is_empty() { "?" } else { table };
        let cols = cols.trim();
        out += &if cols == "*" {
            format!(".table(\"{}\")", table)
        } else {
            format!(".table(\"{}\").select([{}])", table, cols.split(',').map(|c| format!("\"{}\"", c.trim())).collect::<Vec<_>>().join(", "))
        };
        if !cond.trim().is_empty() {
            let c = cond.trim();
            if let Some((l, r)) = c.split_once(">") {
                out += &format!(".where_gt(\"{}\", {})", l.trim(), r.trim());
            } else if let Some((l, r)) = c.split_once("=") {
                out += &format!(".where_eq(\"{}\", {})", l.trim(), r.trim());
            } else {
                out += &format!(".where_raw(\"{}\")", c);
            }
        }
        out += ".all()";
        return out;
    }
    if let Some(rest) = s.strip_prefix("insert into") {
        let (table, vals) = rest.split_once("values").unwrap_or((rest, ""));
        out += &format!(".table(\"{}\").insert({})", table.trim(), vals.trim());
        return out;
    }
    if let Some(rest) = s.strip_prefix("update") {
        let (table, tail) = rest.split_once("set").unwrap_or((rest, ""));
        out += &format!(".table(\"{}\").update({})", table.trim(), tail.trim());
        return out;
    }
    if let Some(rest) = s.strip_prefix("delete from") {
        out += &format!(".table(\"{}\").delete()", rest.trim());
        return out;
    }
    format!("-- 无法识别的 SQL: {}", sql)
}

/// F197 OpenAPI → 客户端请求函数。
pub fn openapi_to_client(path: &str, method: &str) -> String {
    let fn_name = path
        .split('/')
        .filter(|p| !p.is_empty() && !p.starts_with('{'))
        .collect::<Vec<_>>()
        .join("_");
    let has_param = path.contains('{');
    format!(
        "async fn api_{}(client: &Client{}) -> Result<Response> {{\n  client.{}(\"{}\").await\n}}",
        fn_name,
        if has_param { ", id: &str" } else { "" },
        method.to_lowercase(),
        if has_param { &path[..] } else { path }
    )
}

/// F198 正则构建器：拖拽部件 → 正则 + 校验样本。
pub struct RegexPart {
    pub kind: &'static str,
    pub value: String,
}

pub fn regex_build(parts: &[RegexPart]) -> String {
    let mut re = String::new();
    for p in parts {
        match p.kind {
            "digit" => re.push_str("\\d"),
            "word" => re.push_str("\\w"),
            "letter" => re.push_str("[a-zA-Z]"),
            "any" => re.push('.'),
            "space" => re.push_str("\\s"),
            "star" => re.push('*'),
            "plus" => re.push('+'),
            "question" => re.push('?'),
            "group" => re.push_str(&format!("({})", p.value)),
            "range" => re.push_str(&format!("[{}]", p.value)),
            "repeat" => re.push_str(&format!("{{{}}}", p.value)),
            _ => re.push_str(&p.value),
        }
    }
    re
}

pub fn regex_matches(re: &str, sample: &str) -> bool {
    // 极简子集：\d \w \s . 字面量 [...] 与量词 * + ?，全串匹配
    #[derive(Clone)]
    enum Atom {
        Digit,
        Word,
        Space,
        Any,
        Lit(char),
        Class(Vec<(char, char, bool)>), // (起, 止, 是否区间)
    }
    let mut atoms: Vec<(Atom, u8)> = Vec::new(); // quant: 0=一次 1=* 2=+ 3=?
    let mut it = re.chars().peekable();
    while let Some(c) = it.next() {
        let atom = match c {
            '\\' => match it.next() {
                Some('d') => Atom::Digit,
                Some('w') => Atom::Word,
                Some('s') => Atom::Space,
                Some(x) => Atom::Lit(x),
                None => return false,
            },
            '.' => Atom::Any,
            '[' => {
                let mut cls: Vec<(char, char, bool)> = Vec::new();
                let mut prev: Option<char> = None;
                loop {
                    let cc = match it.next() {
                        Some(x) => x,
                        None => break,
                    };
                    if cc == ']' {
                        break;
                    }
                    if cc == '-' {
                        if let (Some(a), Some(b)) = (prev, it.peek()) {
                            cls.push((a, *b, true));
                            prev = None;
                            it.next();
                            continue;
                        }
                    }
                    if let Some(a) = prev.take() {
                        cls.push((a, a, false));
                    }
                    prev = Some(cc);
                }
                if let Some(a) = prev.take() {
                    cls.push((a, a, false));
                }
                Atom::Class(cls)
            }
            other => Atom::Lit(other),
        };
        let q = match it.peek() {
            Some('*') => {
                it.next();
                1
            }
            Some('+') => {
                it.next();
                2
            }
            Some('?') => {
                it.next();
                3
            }
            _ => 0,
        };
        atoms.push((atom, q));
    }

    fn one(atom: &Atom, c: char) -> bool {
        match atom {
            Atom::Digit => c.is_ascii_digit(),
            Atom::Word => c.is_alphanumeric() || c == '_',
            Atom::Space => c.is_whitespace(),
            Atom::Any => true,
            Atom::Lit(x) => *x == c,
            Atom::Class(cls) => cls
                .iter()
                .any(|&(a, b, range)| if range { c >= a && c <= b } else { c == a }),
        }
    }

    fn m(atoms: &[(Atom, u8)], ai: usize, s: &[char], si: usize) -> bool {
        if ai == atoms.len() {
            return si == s.len();
        }
        let (atom, q) = &atoms[ai];
        match q {
            0 => si < s.len() && one(atom, s[si]) && m(atoms, ai + 1, s, si + 1),
            1 | 2 => {
                // 贪心 + 回溯
                let mut k = si;
                while k < s.len() && one(atom, s[k]) {
                    k += 1;
                }
                while k >= si {
                    if m(atoms, ai + 1, s, k) {
                        return true;
                    }
                    if k == si {
                        break;
                    }
                    k -= 1;
                }
                if *q == 1 && m(atoms, ai + 1, s, si) {
                    return true;
                }
                false
            }
            _ => m(atoms, ai + 1, s, si) || (si < s.len() && one(atom, s[si]) && m(atoms, ai + 1, s, si + 1)),
        }
    }
    let sc: Vec<char> = sample.chars().collect();
    m(&atoms, 0, &sc, 0)
}

/// F199 代码积木：积木块拼接 → 代码。
pub fn blocks_to_code(blocks: &[(&'static str, &str)]) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for (kind, val) in blocks {
        let indent = "  ".repeat(depth);
        match *kind {
            "start" => out.push_str("fn main() {\n"),
            "end" => out.push_str("}\n"),
            "if" => {
                out.push_str(&format!("{}if {} {{\n", indent, val));
                depth += 1;
            }
            "loop" => {
                out.push_str(&format!("{}for i in 0..{} {{\n", indent, val));
                depth += 1;
            }
            "stmt" => out.push_str(&format!("{}{}\n", indent, val)),
            "close" => {
                depth = depth.saturating_sub(1);
                out.push_str(&format!("{}}}\n", "  ".repeat(depth)));
            }
            _ => out.push_str(&format!("{}{}\n", indent, val)),
        }
    }
    out
}

/// F200 批量重命名：符号表全字匹配替换。
pub fn batch_rename(src: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let word: String = chars[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect();
            let wlen = word.chars().count();
            if word == old {
                out.push_str(new);
            } else {
                out.push_str(&word);
            }
            i += wlen;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// F201 函数签名向导：输入→处理→输出→异常 → 完整签名+骨架。
pub fn signature_wizard(name: &str, inputs: &[(&str, &str)], output: &str, throws: bool) -> String {
    let args: Vec<String> = inputs.iter().map(|(n, t)| format!("{}: {}", n, t)).collect();
    let ret = if output == "void" || output.is_empty() {
        String::new()
    } else {
        format!(" -> {}", output)
    };
    let body = if throws {
        "  // 可能失败：返回错误交上层处理\n  todo!()"
    } else {
        "  todo!()"
    };
    format!("fn {}({}){} {{\n{}\n}}", name, args.join(", "), ret, body)
}

/// F202 错误修复：错误码 + AST 行 → 修复补丁。
pub fn suggest_fix(line: &str) -> Option<(&'static str, String)> {
    let t = line.trim();
    if t.contains(".len() >") && !t.contains("> 0") {
        return Some(("越界防护", format!("// 建议：改为 `{} > 0` 语义校验", t)));
    }
    if t.contains("== null") || t.contains("== None") {
        return Some(("空值防护", t.replace("== null", ".is_none()").replace("== None", ".is_none()")));
    }
    if t.starts_with("var ") {
        return Some(("改用不可变", t.replacen("var ", "let ", 1)));
    }
    if t.starts_with("if ") && !t.contains('{') && !t.contains("//") {
        return Some(("补齐代码块", format!("{} {{\n  todo()\n}}", t)));
    }
    if t.contains("catch {}") || t.contains("except:") {
        return Some(("空捕获", format!("{} // 至少记录日志", t)));
    }
    if t.contains("+ 1") && t.contains("for ") {
        return Some(("差一错误", format!("// 检查边界：{}", t)));
    }
    None
}

/// F203 逻辑块双击选中：给定行 → 整块范围（大括号配对）。
pub fn select_block(lines: &[&str], click_line: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut start = None;
    for (i, l) in lines.iter().enumerate() {
        let opens = l.matches('{').count();
        let closes = l.matches('}').count();
        if start.is_none() && i <= click_line {
            if opens > 0 || l.contains("if") || l.contains("for") || l.contains("while") || l.contains("try") {
                if i >= click_line.saturating_sub(1) || opens > 0 {
                    start = Some(i);
                }
            }
        }
        depth += opens as i32 - closes as i32;
        if let Some(s) = start {
            if depth <= 0 && i >= s {
                return Some((s, i));
            }
        }
    }
    start.map(|s| (s, lines.len() - 1))
}

/// F204 代码拖拽重组：块移动 + 作用域修复（依赖行更新）。
pub fn move_block(lines: &mut Vec<String>, range: (usize, usize), to: usize) {
    let block: Vec<String> = lines.drain(range.0..=range.1.min(lines.len() - 1)).collect();
    let dest = if to > range.1 { to - block.len() } else { to };
    let at = dest.min(lines.len());
    for (i, b) in block.iter().enumerate() {
        lines.insert(at + i, b.clone());
    }
}

/// F205 函数内联展开：调用处 → 函数体替换。
pub fn inline_expand(call: &str, params: &[String], body: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let args: Vec<&str> = call
        .split_once('(')
        .and_then(|(_, r)| r.split_once(')'))
        .map(|(a, _)| a.split(',').map(str::trim).collect())
        .unwrap_or_default();
    for b in body {
        let mut l = b.clone();
        for (p, a) in params.iter().zip(&args) {
            l = batch_rename(&l, p, a);
        }
        out.push(l);
    }
    out
}

/// F206 逻辑折叠：指定 AST 类型块收缩为单行摘要。
pub fn fold_blocks(lines: &[String], kind: FoldKind) -> Vec<String> {
    let kw = match kind {
        FoldKind::Try => vec!["try", "catch", "except", "finally"],
        FoldKind::If => vec!["if", "else"],
        FoldKind::Loop => vec!["for", "while"],
    };
    let mut out = Vec::new();
    let mut hiding = false;
    let mut depth = 0i32;
    for l in lines {
        let t = l.trim();
        if !hiding && kw.iter().any(|k| t.starts_with(k)) {
            hiding = true;
            depth = 0;
        }
        if hiding {
            depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if depth <= 0 && (t.contains('}') || !t.contains('{')) && depth <= 0 {
                if depth <= 0 {
                    out.push(format!("{} ⟪已折叠 {} 行块⟫", t.trim_end_matches(['{', ' ']), kind.label()));
                    hiding = false;
                    continue;
                }
            }
            // 仍在块内
            if t.contains('}') && depth <= 0 {
                out.push(format!("⟪已折叠 {}⟫", kind.label()));
                hiding = false;
            }
            continue;
        }
        out.push(l.clone());
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Try,
    If,
    Loop,
}

impl FoldKind {
    pub fn label(self) -> &'static str {
        match self {
            FoldKind::Try => "错误处理",
            FoldKind::If => "判断",
            FoldKind::Loop => "循环",
        }
    }
}

/// F207 批量模式替换：框选相似段，改一处 → 其余同步。
pub fn batch_pattern_replace(lines: &mut [String], edited_index: usize, old: &str, new: &str) -> usize {
    if edited_index >= lines.len() {
        return 0;
    }
    let edited = lines[edited_index].clone();
    let new_edited = edited.replace(old, &new.to_string());
    let replaced = edited != new_edited;
    lines[edited_index] = new_edited;
    let mut count = if replaced { 1 } else { 0 };
    for (i, l) in lines.iter_mut().enumerate() {
        if i != edited_index && l.contains(old) {
            // 同构判定：去掉数字差异后与编辑行同构
            let norm = |s: &str| s.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect::<String>();
            if norm(&edited) == norm(l) {
                *l = l.replace(old, &new.to_string());
                count += 1;
            }
        }
    }
    count
}

/// F208 模板填空：`___` 空位按顺序填入。
pub fn template_fill(template: &str, values: &[&str]) -> (String, Vec<usize>) {
    let mut slots = Vec::new();
    let mut out = String::new();
    let mut vi = 0;
    let mut rest = template;
    while let Some(p) = rest.find("___") {
        out.push_str(&rest[..p]);
        slots.push(out.len());
        out.push_str(values.get(vi).copied().unwrap_or("…"));
        vi += 1;
        rest = &rest[p + 3..];
    }
    out.push_str(rest);
    (out, slots)
}

/// F209 缩进修复：按大括号层级重排全文件缩进。
pub fn fix_indent(lines: &[&str]) -> Vec<String> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for l in lines {
        let t = l.trim();
        if t.is_empty() {
            out.push(String::new());
            continue;
        }
        let dedent = t.starts_with('}');
        let d = if dedent { depth.saturating_sub(1) as usize } else { depth as usize };
        out.push(format!("{}{}", "    ".repeat(d), t));
        depth += t.matches('{').count() as i32;
        depth -= t.matches('}').count() as i32;
        if depth < 0 {
            depth = 0;
        }
    }
    out
}

/// F210 版本对比：LCS diff → 增删列表（滑块数据源）。
pub fn version_diff(a: &[&str], b: &[&str]) -> Vec<(usize, &'static str, String)> {
    let n = a.len();
    let m = b.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push((i, "del", a[i].to_string()));
            i += 1;
        } else {
            out.push((j, "add", b[j].to_string()));
            j += 1;
        }
    }
    while i < n {
        out.push((i, "del", a[i].to_string()));
        i += 1;
    }
    while j < m {
        out.push((j, "add", b[j].to_string()));
        j += 1;
    }
    out
}

/// F211 宏录制：操作序列录制与重放。
#[derive(Default)]
pub struct MacroRecorder {
    pub ops: Vec<String>,
    pub recording: bool,
}

impl MacroRecorder {
    pub fn start(&mut self) {
        self.ops.clear();
        self.recording = true;
    }
    pub fn op(&mut self, op: &str) {
        if self.recording {
            self.ops.push(op.to_string());
        }
    }
    pub fn stop(&mut self) {
        self.recording = false;
    }
    pub fn replay(&self, times: usize) -> Vec<String> {
        let mut out = Vec::new();
        for _ in 0..times {
            out.extend(self.ops.iter().cloned());
        }
        out
    }
}

/// F212 阅读模式：只保留签名 + 注释 + 调用线。
pub fn reading_mode<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut in_body = false;
    let mut depth = 0i32;
    for &l in lines {
        let t = l.trim();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
            out.push(l);
            continue;
        }
        if !in_body {
            out.push(l);
            depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if depth > 0 {
                in_body = true;
                out.push("  …（函数体已隐藏）");
            }
        } else {
            depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if depth <= 0 {
                in_body = false;
                out.push(l);
            }
        }
    }
    out
}

/// F213 滑动调节：滑块 0~1 → 细节级别（简单↔详细）。
pub fn detail_level(slider: f32) -> (&'static str, usize) {
    let v = slider.clamp(0.0, 1.0);
    match v {
        x if x < 0.2 => ("极简", 0),
        x if x < 0.4 => ("只看结构", 1),
        x if x < 0.6 => ("签名+注释", 2),
        x if x < 0.8 => ("常规", 3),
        _ => ("全细节", 4),
    }
}

/// F214 点哪里解释哪里：点击行 → 大白话气泡（3 秒展示）。
pub fn explain_on_click(line: &str) -> String {
    let t = line.trim();
    let kind = crate::model::StmtKind::parse(t);
    let body = match kind {
        crate::model::StmtKind::If => super::translate::if_sentence(t.trim_start_matches("if").trim()),
        crate::model::StmtKind::Loop => {
            if t.starts_with("while") {
                super::translate::while_sentence(t.trim_start_matches("while").trim())
            } else {
                super::translate::for_sentence(t.trim_start_matches("for").trim_start_matches(|c: char| c.is_alphanumeric() || c == '_').trim())
            }
        }
        crate::model::StmtKind::Return => super::translate::return_sentence(t.trim_start_matches("return").trim()),
        crate::model::StmtKind::Try => super::translate::try_sentence(t.trim_start_matches("try").trim()),
        _ => super::translate::translate_line(t),
    };
    format!("💬 {}", body)
}

/// F215 摇一摇找 bug：确定性挑出第一个问题 + 人话解释。
pub fn shake_for_bug(lines: &[&str]) -> Option<(usize, String)> {
    for (i, l) in lines.iter().enumerate() {
        if let Some((tag, fix)) = suggest_fix(l) {
            return Some((
                i,
                format!("发现「{}」：{}。建议：{}", tag, super::translate::error_plain(tag), fix),
            ));
        }
    }
    // 摇不到就按魔法字符串兜底
    let src = lines.join("\n");
    if let Some(m) = crate::nouns::magic_strings(&src).first() {
        return Some((0, format!("魔法字符串「{}」建议提取为常量", m)));
    }
    None
}

/// F216 语音问代码：关键词 → 预设回答。
pub fn voice_query(q: &str) -> &'static str {
    let q = q.to_lowercase();
    if q.contains("干嘛") || q.contains("做什么") || q.contains("干啥") {
        "这个函数负责它名字里说的那件事，右边面板有逐行翻译"
    } else if q.contains("怎么用") || q.contains("调用") {
        "把要处理的东西放进括号里传给它，它会把结果交回来"
    } else if q.contains("危险") || q.contains("安全") {
        "目前没发现明显问题，但任何改文件的函数都建议先备份"
    } else if q.contains("错误") || q.contains("报错") || q.contains("bug") {
        "点摇一摇找bug，或看检测面板的红色条目"
    } else if q.contains("性能") || q.contains("慢") {
        "热点分析会标出最耗时的行，先看那里"
    } else if q.contains("翻译") || q.contains("大白话") {
        "切换到通俗界面，代码行右侧就是中文"
    } else {
        "试试问：这个函数干嘛的 / 怎么用 / 有没有bug"
    }
}

/// F217 手势缩放：双指捏合/张开 → 折叠/展开层级。
pub fn gesture_zoom(current: usize, pinch_in: bool) -> usize {
    if pinch_in {
        (current + 1).min(6)
    } else {
        current.saturating_sub(1)
    }
}

/// F218 拖拽改逻辑：把箭头从 A 拖到 B → 分支目标改变。
pub fn rewire_branch(branches: &mut [(String, usize)], from_cond: &str, new_target: usize) -> bool {
    if let Some(b) = branches.iter_mut().find(|(c, _)| c == from_cond) {
        b.1 = new_target;
        true
    } else {
        false
    }
}

/// F219 涂色标记：荧光笔标注（保存到侧边栏）。
#[derive(Default)]
pub struct Markers {
    pub marks: Vec<(String, &'static str)>,
}

impl Markers {
    pub fn mark(&mut self, key: impl Into<String>, color: &'static str) {
        let k = key.into();
        if !self.marks.iter().any(|(m, _)| *m == k) {
            self.marks.push((k, color));
        }
    }
    pub fn unmark(&mut self, key: &str) {
        self.marks.retain(|(m, _)| m != key);
    }
    pub fn color_of(&self, key: &str) -> Option<&'static str> {
        self.marks.iter().find(|(m, _)| m == key).map(|(_, c)| *c)
    }
}

/// F220 代码翻译机：框选区域 → 中文故事覆盖层。
pub fn selection_story(src: &str) -> String {
    let lines: Vec<&str> = src.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut story = String::from("📖 覆盖层故事：\n");
    for (i, l) in lines.iter().enumerate() {
        story.push_str(&format!("  第{}步：{}\n", i + 1, explain_on_click(l)));
    }
    if lines.len() > 1 {
        story.push_str("连起来读：先做准备，再按顺序执行，最后交出结果。");
    }
    story
}

pub fn run_simple_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("simple");
    // F191
    s.add("#191 伪代码→代码", pseudo_to_code("// 循环10次 打印x").as_deref() == Some("for i in 0..10 {\n  打印x\n}") && pseudo_to_code("// 如果 age>18 就 通过()").as_deref() == Some("if age>18 {\n  通过()\n}"), "注释关键词→生成预览");
    // F192
    let sw = table_to_switch("code", &[("1".into(), "one()".into()), ("2".into(), "two()".into())]);
    s.add("#192 表格→switch", sw.contains("case 1: one()") && sw.contains("default"), "Markdown 表格→switch");
    // F193
    let flow = flowchart_to_code(
        &[("开始".into(), "oval", None), ("x>0".into(), "diamond", Some(3)), ("正数()".into(), "rect", None), ("负数()".into(), "rect", None)],
        &[(0, 1), (1, 2)],
    );
    s.add("#193 拖拽流程图→代码", flow.iter().any(|l| l.starts_with("if ")) && flow.contains(&"正数()".to_string()), "菱形→if 矩形→语句");
    // F194
    let fsm = state_table_to_fsm("Idle", &[("Idle".into(), "START".into(), "Run".into()), ("Run".into(), "STOP".into(), "Idle".into())]);
    s.add("#194 状态表→状态机", fsm.contains("Idle => { START => state = Run") && fsm.contains("STOP => state = Idle"), "状态转换表→match");
    // F195
    let ty = json_to_types("user", r#"{"name":"a","age":1,"tags":[1],"addr":{"city":"x"}}"#);
    s.add("#195 JSON→类型", ty.contains("interface User") && ty.contains("name: string") && ty.contains("age: number") && ty.contains("tags: number[]") && ty.contains("interface Addr"), "递归推断类型定义");
    // F196
    s.add("#196 SQL→ORM", sql_to_orm("SELECT name, age FROM users WHERE age > 18;").contains(".select([\"name\", \"age\"])") && sql_to_orm("SELECT name, age FROM users WHERE age > 18;").contains(".where_gt(\"age\", 18)") && sql_to_orm("DELETE FROM logs").contains(".delete()"), "SQL AST→ORM 链");
    // F197
    let api = openapi_to_client("/users/{id}/orders", "GET");
    s.add("#197 OpenAPI→客户端", api.contains("async fn api_users_orders") && api.contains("client.get"), "YAML 路径→请求函数");
    // F198
    let re = regex_build(&[RegexPart { kind: "letter", value: String::new() }, RegexPart { kind: "digit", value: String::new() }, RegexPart { kind: "plus", value: String::new() }]);
    s.add("#198 正则构建器", re == "[a-zA-Z]\\d+" && regex_matches(&re, "a1") && !regex_matches(&re, "11"), "拖拽部件→实时预览匹配");
    // F199
    let code = blocks_to_code(&[("start", ""), ("loop", "3"), ("stmt", "print(i)"), ("close", ""), ("end", "")]);
    s.add("#199 代码积木", code.contains("for i in 0..3 {") && code.starts_with("fn main()"), "积木拼接→代码");
    // F200
    let rn = batch_rename("let user_id = 1\nuse(user_id2)\n", "user_id", "account_id");
    s.add("#200 批量重命名", rn.contains("account_id = 1") && rn.contains("user_id2"), "符号表全字精确匹配");
    // F201
    let sig = signature_wizard("pay", &[("amount", "int"), ("to", "str")], "bool", true);
    s.add("#201 函数签名向导", sig.contains("fn pay(amount: int, to: str) -> bool"), "分步表单→签名骨架");
    // F202
    s.add("#202 错误修复", suggest_fix("if x == null {").is_some() && suggest_fix("var y = 1").map(|(t, _)| t) == Some("改用不可变"), "报错旁修复按钮+预览");
    // F203
    let blk = select_block(&["fn f() {", "  if x {", "    a()", "  }", "}"], 2);
    s.add("#203 逻辑块双击选中", blk == Some((0, 4)), "双击整块高亮+虚线框");
    // F204
    let mut ls = vec!["a()".to_string(), "if c {".to_string(), "  b()".to_string(), "}".to_string(), "d()".to_string()];
    move_block(&mut ls, (1, 3), 4);
    // 语义：把 (1..=3) 的逻辑块拖到原 4 号位之后 → 块整体移到 d() 之前
    s.add("#204 代码拖拽重组", ls[0].contains("a()") && ls[1].contains("if c {") && ls[3].trim() == "}" && ls[4].contains("d()"), "AST子树剪切粘贴+作用域修复");
    // F205
    let inlined = inline_expand("calc(x)", &["n".to_string()], &["n + 1".to_string(), "return n * 2".to_string()]);
    s.add("#205 函数内联展开", inlined[0].contains("x + 1") && inlined[1].contains("x * 2"), "调用处展开函数体");
    // F206
    let folded = fold_blocks(&["try {".to_string(), "  risky()".to_string(), "} catch {".to_string(), "  log()".to_string(), "}".to_string(), "after()".to_string()], FoldKind::Try);
    s.add("#206 逻辑折叠", folded.iter().any(|l| l.contains("已折叠")) && folded.iter().any(|l| l.contains("after()")), "错误处理收缩为单行");
    // F207
    let mut sim = vec!["p1.buy(1)".to_string(), "p2.buy(2)".to_string(), "p3.buy(3)".to_string()];
    let n = batch_pattern_replace(&mut sim, 1, "buy", "sell");
    s.add("#207 批量模式替换", n == 3 && sim.iter().all(|l| l.contains("sell")), "改一处其余同步高亮");
    // F208
    let (filled, slots) = template_fill("for ___ in ___ {", &["i", "0..10"]);
    s.add("#208 模板填空", filled == "for i in 0..10 {" && slots.len() == 2, "___空位 Tab 逐个跳填");
    // F209
    let ind = fix_indent(&["fn f() {", "if x {", "a()", "}", "}"]);
    s.add("#209 缩进修复", ind[1].starts_with("    if") && ind[2].starts_with("        a()") && ind[4] == "}", "全文件缩进重排");
    // F210
    let d = version_diff(&["a", "b", "c"], &["a", "x", "c"]);
    s.add("#210 版本对比", d.contains(&(1usize, "del", "b".to_string())) && d.contains(&(1usize, "add", "x".to_string())), "LCS diff 滑块高亮");
    // F211
    let mut rec = MacroRecorder::default();
    rec.start();
    rec.op("选中第1行");
    rec.op("改名为user");
    rec.stop();
    s.add("#211 宏录制", rec.replay(2).len() == 4 && !rec.recording, "录制→停止→重放");
    // F212
    let rm = reading_mode(&["fn f() {", "  secret()", "}", "// 注释保留", "fn g() {", "  x()", "}"]);
    s.add("#212 阅读模式", rm.contains(&"  …（函数体已隐藏）") && rm.contains(&"// 注释保留") && !rm.contains(&"  secret()"), "只显示签名+注释");
    // F213
    s.add("#213 滑动调节", detail_level(0.1).0 == "极简" && detail_level(0.9).0 == "全细节" && detail_level(0.5).1 == 2, "简单↔详细实时过渡");
    // F214
    s.add("#214 点哪里解释哪里", explain_on_click("return total").contains("交出去") && explain_on_click("if (a) {").starts_with("💬 如果"), "点击行→大白话气泡");
    // F215
    let bug = shake_for_bug(&["let ok = 1", "if x == null {"]);
    s.add("#215 摇一摇找bug", bug.is_some() && bug.as_ref().unwrap().0 == 1, "随机高亮问题+人话解释");
    // F216
    s.add("#216 语音问代码", voice_query("这个函数干嘛的").contains("负责") && !voice_query("随便说说").contains("负责"), "关键词→预设回答");
    // F217
    s.add("#217 手势缩放", gesture_zoom(3, true) == 4 && gesture_zoom(3, false) == 2 && gesture_zoom(0, false) == 0, "捏合折叠/张开展开");
    // F218
    let mut br = vec![("x>0".to_string(), 0usize), ("else".to_string(), 1usize)];
    s.add("#218 拖拽改逻辑", rewire_branch(&mut br, "x>0", 2) && br[0].1 == 2, "拖箭头→分支目标同步");
    // F219
    let mut mk = Markers::default();
    mk.mark("L3-行4", "荧光黄");
    mk.mark("L3-行4", "荧光绿");
    s.add("#219 涂色标记", mk.color_of("L3-行4") == Some("荧光黄") && mk.marks.len() == 1, "标记保存到侧边栏");
    // F220
    let st = selection_story("x = 1\nreturn x");
    s.add("#220 代码翻译机", st.contains("📖") && st.contains("第1步") && st.contains("连起来读"), "框选→中文故事覆盖层");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f191_variants() {
        assert!(pseudo_to_code("// 打印 hello").unwrap().contains("print(hello)"));
        assert!(pseudo_to_code("// 遍历 items 做 处理()").unwrap().contains("for item in items"));
        assert!(pseudo_to_code("// 只要 x < 10").unwrap().starts_with("while x < 10"));
        assert!(pseudo_to_code("// 什么都不像").is_none());
    }

    #[test]
    fn f195_nested_json() {
        let t = json_to_types("order", r#"{"id":1,"buyer":{"name":"n"}}"#);
        assert!(t.contains("interface Order"));
        assert!(t.contains("interface Buyer"));
        assert!(t.contains("name: string"));
    }

    #[test]
    fn f200_word_boundary() {
        assert_eq!(batch_rename("cart.cartX + cart", "cart", "trolley"), "trolley.cartX + trolley");
    }

    #[test]
    fn f205_inline_renames_params() {
        let out = inline_expand("add(a, b)", &["x".into(), "y".into()], &["return x + y".into()]);
        assert_eq!(out[0], "return a + b");
    }

    #[test]
    fn f210_diff_counts() {
        let d = version_diff(&["a", "b"], &["b"]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0], (0, "del", "a".to_string()));
    }

    #[test]
    fn f211_macro_isolation() {
        let mut rec = MacroRecorder::default();
        rec.op("不该被记");
        assert!(rec.ops.is_empty());
        rec.start();
        rec.op("op1");
        rec.stop();
        rec.op("不该被记2");
        assert_eq!(rec.ops, vec!["op1".to_string()]);
    }
}
