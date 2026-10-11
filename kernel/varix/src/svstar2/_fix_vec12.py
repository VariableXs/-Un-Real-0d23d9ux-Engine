import io

p = "vec12_macro.rs"
s = io.open(p, encoding="utf-8").read()
orig_len = len(s)


def rep(old, new, must=True):
    global s
    if old not in s:
        if must:
            raise SystemExit("anchor missing: " + old[:60])
        return
    s = s.replace(old, new, 1)


# 1) Tok gains pos field
rep(
    """/// 一个展开记号。
#[derive(Clone, Debug, PartialEq)]
pub struct Tok {
    pub text: String,
    pub is_ident: bool,
}""",
    """/// 一个展开记号（pos = 原文字节位置——位置保留判据的原料）。
#[derive(Clone, Debug, PartialEq)]
pub struct Tok {
    pub text: String,
    pub is_ident: bool,
    pub pos: usize,
}""",
)

# 2) tokenize fills pos
rep(
    """            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: true,
            });""",
    """            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: true,
                pos: start,
            });""",
)
rep(
    """                out.push(Tok { text: "##".to_string(), is_ident: false });
                i += 2;
            } else {
                out.push(Tok { text: "#".to_string(), is_ident: false });
                i += 1;
            }""",
    """                out.push(Tok { text: "##".to_string(), is_ident: false, pos: i });
                i += 2;
            } else {
                out.push(Tok { text: "#".to_string(), is_ident: false, pos: i });
                i += 1;
            }""",
)
rep(
    """            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: false,
            });""",
    """            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: false,
                pos: start,
            });""",
)

# 3) expand_input: drop base_pos threading
rep(
    """    let mut active: Vec<(String, usize)> = Vec::new();
    let mut out = ExpandResult::default();
    let toks2 = expand_toks(table, &toks, 0, src, cfg, &mut active, &mut out, None)?;""",
    """    let _ = src;
    let mut active: Vec<(String, usize)> = Vec::new();
    let mut out = ExpandResult::default();
    let toks2 = expand_toks(table, &toks, cfg, &mut active, &mut out, None)?;""",
)

# 4) expand_toks signature
rep(
    """fn expand_toks(
    table: &MacroTable,
    toks: &[Tok],
    base_pos: usize,
    src: &str,
    cfg: &MacroConfig,
    active: &mut Vec<(String, usize)>,
    out: &mut ExpandResult,
    from_def: Option<(usize, usize)>,
) -> Result<Vec<ExpTok>, MacroError> {""",
    """fn expand_toks(
    table: &MacroTable,
    toks: &[Tok],
    cfg: &MacroConfig,
    active: &mut Vec<(String, usize)>,
    out: &mut ExpandResult,
    from_def: Option<(usize, usize)>,
) -> Result<Vec<ExpTok>, MacroError> {""",
)

# 5) replace base_pos+i usages with token positions
rep("result.push(plain(t, base_pos + i, from_def));", "result.push(plain(t, from_def));")
rep(
    "let sub = expand_toks(table, &body_toks, base_pos + i, src, cfg, active, out, Some((def.def_line, def.def_pos)))?;",
    "let sub = expand_toks(table, &body_toks, cfg, active, out, Some((def.def_line, def.def_pos)))?;",
)
rep(
    "return Err(depth_err(&t.text, base_pos + i, d, active.len()));",
    "return Err(depth_err(&t.text, t.pos, d, active.len()));",
)
rep(
    "return Err(depth_err(&def.name, base_pos + i, d, active.len()));",
    "return Err(depth_err(&def.name, t.pos, d, active.len()));",
)
rep(
    """                        "E_MACRO_UNCLOSED_CALL",
                        base_pos + i,""",
    """                        "E_MACRO_UNCLOSED_CALL",
                        t.pos,""",
)
rep(
    """                        "E_MACRO_ARITY",
                        base_pos + i,""",
    """                        "E_MACRO_ARITY",
                        t.pos,""",
)
rep(
    "let sub = expand_toks(table, a, base_pos + i, src, cfg, active, out, None)?;\n                    exp_args.push(sub.iter().map(|e| Tok { text: e.text.clone(), is_ident: e.is_ident }).collect());",
    "let sub = expand_toks(table, a, cfg, active, out, None)?;\n                    exp_args.push(sub.iter().map(|e| Tok { text: e.text.clone(), is_ident: e.is_ident, pos: e.call_pos }).collect());",
)
rep(
    """                            expanded.push(ExpTok {
                                text: format!("\\"{}\\"", raw),
                                is_ident: false,
                                call_pos: base_pos + i,""",
    """                            expanded.push(ExpTok {
                                text: format!("\\"{}\\"", raw),
                                is_ident: false,
                                call_pos: t.pos,""",
)
rep(
    """                        expanded.push(ExpTok {
                            text: format!("{}{}", left.text, right_text),
                            is_ident: left.is_ident || right_text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'),
                            call_pos: base_pos + i,""",
    """                        expanded.push(ExpTok {
                            text: format!("{}{}", left.text, right_text),
                            is_ident: left.is_ident || right_text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'),
                            call_pos: t.pos,""",
)
rep(
    """                                expanded.push(ExpTok {
                                    text: at.text.clone(),
                                    is_ident: at.is_ident,
                                    call_pos: base_pos + i,""",
    """                                expanded.push(ExpTok {
                                    text: at.text.clone(),
                                    is_ident: at.is_ident,
                                    call_pos: t.pos,""",
)
rep(
    "expanded.push(plain(bt, base_pos + i, Some((def.def_line, def.def_pos))));",
    "expanded.push(plain(bt, Some((def.def_line, def.def_pos))));",
)
rep(
    "let sub = expand_toks(table, &sub_toks, base_pos + i, src, cfg, active, out, Some((def.def_line, def.def_pos)))?;",
    "let sub = expand_toks(table, &sub_toks, cfg, active, out, Some((def.def_line, def.def_pos)))?;",
)

# 6) plain() signature
rep(
    """fn plain(t: &Tok, pos: usize, from_def: Option<(usize, usize)>) -> ExpTok {
    ExpTok {
        text: t.text.clone(),
        is_ident: t.is_ident,
        call_pos: pos,
        def_line: from_def.map(|d| d.0),
        def_pos: from_def.map(|d| d.1),
    }
}""",
    """fn plain(t: &Tok, from_def: Option<(usize, usize)>) -> ExpTok {
    ExpTok {
        text: t.text.clone(),
        is_ident: t.is_ident,
        call_pos: t.pos,
        def_line: from_def.map(|d| d.0),
        def_pos: from_def.map(|d| d.1),
    }
}""",
)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("rewritten; base_pos remaining:", s.count("base_pos"), "; len", orig_len, "->", len(s))
