//! VE-F0412 · 宏定义与展开（VE-C 域 · 着色器系统 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0412`
//!
//! **判据（锚点原文）**：记号级展开、递归冻结、双侧定位、深度上限、判据。
//! - **对象宏与函数式宏**：定义登记（宏名 × 参数表 × 替换体）进宏表——
//!   FNV-1a 哈希 + 桶链，查表 O(1)；
//! - **展开引擎**：记号级替换（不是文本盲替）、参数代入、`#` 串接与
//!   `##` 粘贴算子语义（`#参数` → 参数实文的字符串字面量；`左##右` →
//!   两记号拼接成单记号）；
//! - **递归展开防护**：自引用宏**冻结语义**（展开中宏名的再次出现按
//!   原样保留——蓝漆规则，不静默死循环）；展开**深度上限**可配（超限
//!   报错指向宏定义处）；
//! - 参数个数不符 → 报错**双侧定位**（定义处 + 调用处同时给出）；
//!   重定义 → 按规范裁定（策略可配：警告默认 / 错误显性）；
//! - 展开产物回灌词法流**位置信息保留**：每个产物记号携带调用处与
//!   宏定义处双侧坐标（诊断可溯）。
//!
//! 性能逐项分解：展开 O(展开量)；查表 O(1) 哈希；深度 O(1) 计数。

use super::vec05_ident::{is_first_byte, is_ident_byte};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、宏表（名 × 参数 × 替换体；FNV 哈希 O(1) 查表）
// ---------------------------------------------------------------------------

/// 重定义处置策略（判据：按规范裁定——警告或错误显性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedefPolicy {
    /// 重定义发警告、按新定义覆盖（默认）
    Warn,
    /// 重定义即错误
    Error,
}

/// 宏配置（深度上限 + 重定义策略）。
#[derive(Clone, Copy, Debug)]
pub struct MacroConfig {
    pub max_depth: usize,
    pub redef: RedefPolicy,
}

impl MacroConfig {
    pub const DEFAULT_MAX_DEPTH: usize = 64;

    pub fn default_config() -> MacroConfig {
        MacroConfig {
            max_depth: MacroConfig::DEFAULT_MAX_DEPTH,
            redef: RedefPolicy::Warn,
        }
    }
}

/// 一条宏定义（params=None = 对象宏；Some = 函数式宏）。
#[derive(Clone, Debug, PartialEq)]
pub struct MacroDef {
    pub name: String,
    pub params: Option<Vec<String>>,
    /// 替换体原文（展开时记号级处理）
    pub body: String,
    /// 定义位置（双侧定位的定义侧）
    pub def_line: usize,
    pub def_pos: usize,
}

/// 宏错误（三要素 + 调用处与定义处双侧坐标）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroError {
    pub code: &'static str,
    /// 调用处位置
    pub pos: usize,
    /// 定义处位置（双侧定位；无定义侧 = None）
    pub def_pos: Option<usize>,
    pub def_line: Option<usize>,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl MacroError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

fn err(code: &'static str, pos: usize, def: Option<(usize, usize)>, what: String, why: String, next: String) -> MacroError {
    MacroError {
        code,
        pos,
        def_pos: def.map(|d| d.1),
        def_line: def.map(|d| d.0),
        what,
        why,
        next,
    }
}

/// FNV-1a 64 位哈希（无外部依赖的 O(1) 索引）。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

const BUCKETS: usize = 64;

/// 宏表：FNV 哈希 + 桶链（判据：查表 O(1) 哈希）。
#[derive(Clone, Debug, Default)]
pub struct MacroTable {
    buckets: Vec<Vec<MacroDef>>,
}

impl MacroTable {
    pub fn new() -> MacroTable {
        MacroTable {
            buckets: (0..BUCKETS).map(|_| Vec::new()).collect(),
        }
    }

    /// 登记宏定义；重定义按策略裁定（Warn=覆盖+返回警告文本；Error=拒绝）。
    pub fn define(&mut self, def: MacroDef, cfg: &MacroConfig) -> Result<Option<String>, MacroError> {
        let b = self.bucket(&def.name);
        if let Some(existing) = b.iter().find(|m| m.name == def.name) {
            let same = existing.params.is_some() == def.params.is_some()
                && existing.body == def.body;
            if same {
                // 同名同体：幂等登记，无警告
                return Ok(None);
            }
            match cfg.redef {
                RedefPolicy::Error => {
                    return Err(err(
                        "E_MACRO_REDEF",
                        def.def_pos,
                        Some((existing.def_line, existing.def_pos)),
                        format!("宏 {} 重定义（第 {} 行 vs 第 {} 行）", def.name, existing.def_line, def.def_line),
                        "重定义让展开结果依赖登记顺序——规范要求显性裁定".to_string(),
                        "改名或删除旧定义；确需覆盖请把策略调回 Warn".to_string(),
                    ));
                }
                RedefPolicy::Warn => {
                    // 覆盖旧定义
                    let warn = format!(
                        "宏 {} 重定义（旧定义在第 {} 行）——按新定义覆盖",
                        def.name, existing.def_line
                    );
                    let idx = b.iter().position(|m| m.name == def.name).unwrap_or(0);
                    b[idx] = def;
                    return Ok(Some(warn));
                }
            }
        }
        b.push(def);
        Ok(None)
    }

    /// O(1) 查表。
    pub fn lookup(&self, name: &str) -> Option<&MacroDef> {
        self.bucket_of(name).iter().find(|m| m.name == name)
    }

    fn bucket(&mut self, name: &str) -> &mut Vec<MacroDef> {
        let idx = (fnv1a(name) as usize) % BUCKETS;
        &mut self.buckets[idx]
    }

    /// 只读桶访问（lookup 用）。
    fn bucket_of(&self, name: &str) -> &Vec<MacroDef> {
        let idx = (fnv1a(name) as usize) % BUCKETS;
        &self.buckets[idx]
    }
}

// ---------------------------------------------------------------------------
// 二、记号化（词法级最小切分；标识符规则复用 vec05 单一实现）
// ---------------------------------------------------------------------------

/// 一个展开记号（pos = 原文字节位置——位置保留判据的原料）。
#[derive(Clone, Debug, PartialEq)]
pub struct Tok {
    pub text: String,
    pub is_ident: bool,
    pub pos: usize,
}

/// 最小记号化：标识符（vec05 字节规则）/ 粘贴与串接算子 / 其余非空白
/// 连续段并成一个标点记号（记号级展开的最小单元）。
pub fn tokenize(s: &str) -> Vec<Tok> {
    let b = s.as_bytes();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if is_first_byte(c) {
            let start = i;
            i += 1;
            while i < b.len() && is_ident_byte(b[i]) {
                i += 1;
            }
            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: true,
                pos: start,
            });
        } else if c == b'#' {
            // # 或 ## 成独立算子记号
            if i + 1 < b.len() && b[i + 1] == b'#' {
                out.push(Tok { text: "##".to_string(), is_ident: false, pos: i });
                i += 2;
            } else {
                out.push(Tok { text: "#".to_string(), is_ident: false, pos: i });
                i += 1;
            }
        } else {
            // 标点段：`,()` 恒为单字符记号（参数切分与括号配对依赖）；
            // 其余标点连写归一段（如 ->、::），遇空白/标识符/#/括号类断开
            let start = i;
            let c0 = b[i];
            i += 1;
            if !matches!(c0, b',' | b'(' | b')') {
                while i < b.len() {
                    let x = b[i];
                    if x.is_ascii_whitespace()
                        || is_first_byte(x)
                        || x == b'#'
                        || matches!(x, b',' | b'(' | b')')
                    {
                        break;
                    }
                    i += 1;
                }
            }
            out.push(Tok {
                text: s[start..i].to_string(),
                is_ident: false,
                pos: start,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 三、展开引擎
// ---------------------------------------------------------------------------

/// 展开产物记号（判据：位置信息保留——双侧坐标）。
#[derive(Clone, Debug, PartialEq)]
pub struct ExpTok {
    pub text: String,
    pub is_ident: bool,
    /// 调用处字节位置
    pub call_pos: usize,
    /// 宏定义处坐标（来自宏体的记号才有）
    pub def_line: Option<usize>,
    pub def_pos: Option<usize>,
}

/// 展开结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpandResult {
    pub text: String,
    pub tokens: Vec<ExpTok>,
    pub warnings: Vec<String>,
}

/// 从 #define 指令体解析宏定义（上游 F0411 指令流的消费形态）：
/// "NAME" / "NAME(a,b) body"——名字后紧贴左括号才是函数式宏（带空格
/// 是对象宏，参数表属于替换体——C 系惯例语义）。
pub fn parse_definition(text: &str, def_line: usize, def_pos: usize) -> Result<MacroDef, MacroError> {
    let b = text.as_bytes();
    let name_end = b
        .iter()
        .position(|&c| !is_ident_byte(c))
        .unwrap_or(b.len());
    if name_end == 0 {
        return Err(err(
            "E_MACRO_NAME",
            def_pos,
            None,
            "宏定义缺宏名".to_string(),
            "#define 后第一个词是宏名——缺失则登记无主".to_string(),
            "补全宏名（标识符规则见 F0405）".to_string(),
        ));
    }
    let name = text[..name_end].to_string();
    let rest = &text[name_end..];
    // 函数式宏：名字后零空白紧贴 '('
    let params = if rest.starts_with('(') {
        let close = rest.find(')').ok_or_else(|| {
            err(
                "E_MACRO_PARAMS",
                def_pos,
                None,
                format!("宏 {} 的参数表缺收括号", name),
                "参数表 (…) 必须闭合".to_string(),
                "补右括号".to_string(),
            )
        })?;
        let inner = &rest[1..close];
        let ps: Vec<String> = inner
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        Some(ps)
    } else {
        None
    };
    let body = match &params {
        Some(_) => {
            let close = rest.find(')').unwrap_or(0);
            rest[close + 1..].trim_start().to_string()
        }
        None => rest.trim_start().to_string(),
    };
    Ok(MacroDef {
        name,
        params,
        body,
        def_line,
        def_pos,
    })
}

/// 全输入展开主入口（判据：记号级展开 / 递归冻结 / 深度上限）。
pub fn expand_input(table: &MacroTable, src: &str, cfg: &MacroConfig) -> Result<ExpandResult, MacroError> {
    let toks = tokenize(src);
    let mut active: Vec<(String, usize)> = Vec::new();
    let mut out = ExpandResult::default();
    let _ = src;
    let toks2 = expand_toks(table, &toks, cfg, &mut active, &mut out, None)?;
    out.tokens = toks2;
    out.text = out
        .tokens
        .iter()
        .map(|t| t.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(out)
}

/// 展开记号序列：标识符命中宏表 → 收参 → 展开体 → 递归重扫。
#[allow(clippy::too_many_arguments)]
fn expand_toks(
    table: &MacroTable,
    toks: &[Tok],
    cfg: &MacroConfig,
    active: &mut Vec<(String, usize)>,
    out: &mut ExpandResult,
    from_def: Option<(usize, usize)>,
) -> Result<Vec<ExpTok>, MacroError> {
    let mut result: Vec<ExpTok> = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        let t = &toks[i];
        if !t.is_ident {
            result.push(plain(t, from_def));
            i += 1;
            continue;
        }
        // 活跃集冻结：自引用按原样保留（判据：递归冻结）
        if active.iter().any(|(n, _)| *n == t.text) {
            result.push(plain(t, from_def));
            i += 1;
            continue;
        }
        let def = match table.lookup(&t.text) {
            Some(d) => d.clone(),
            None => {
                result.push(plain(t, from_def));
                i += 1;
                continue;
            }
        };
        match &def.params {
            None => {
                // 对象宏：体记号化 → 递归重扫（深度 +1）
                active.push((def.name.clone(), def.def_pos));
                if active.len() > cfg.max_depth {
                    let d = (def.def_line, def.def_pos);
                    return Err(depth_err(&t.text, t.pos, d, active.len()));
                }
                // 体记号的调用处坐标统一改写为本次调用的位置（位置保留判据）
                let mut body_toks = tokenize(&def.body);
                for bt in body_toks.iter_mut() {
                    bt.pos = t.pos;
                }
                let mut sub = expand_toks(table, &body_toks, cfg, active, out, Some((def.def_line, def.def_pos)))?;
                result.append(&mut sub);
                active.pop();
                i += 1;
            }
            Some(params) => {
                // 函数式宏：下一个记号必须是 '('（否则按普通标识符保留）
                let lp = toks.get(i + 1);
                match lp {
                    Some(lp) if lp.text == "(" => {}
                    _ => {
                        result.push(plain(t, from_def));
                        i += 1;
                        continue;
                    }
                }
                // 收参（括号嵌套计数）
                let mut args: Vec<Vec<Tok>> = Vec::new();
                let mut cur: Vec<Tok> = Vec::new();
                let mut depth_paren = 0usize;
                let mut j = i + 2;
                let mut closed = false;
                while j < toks.len() {
                    if toks[j].text == "(" {
                        depth_paren += 1;
                        cur.push(toks[j].clone());
                    } else if toks[j].text == ")" {
                        if depth_paren == 0 {
                            closed = true;
                            break;
                        }
                        depth_paren -= 1;
                        cur.push(toks[j].clone());
                    } else if toks[j].text == "," && depth_paren == 0 {
                        args.push(cur.clone());
                        cur.clear();
                    } else {
                        cur.push(toks[j].clone());
                    }
                    j += 1;
                }
                if !closed {
                    return Err(err(
                        "E_MACRO_UNCLOSED_CALL",
                        t.pos,
                        Some((def.def_line, def.def_pos)),
                        format!("宏 {} 的调用缺收括号", def.name),
                        "函数式宏调用的实参表必须闭合".to_string(),
                        "补右括号".to_string(),
                    ));
                }
                if !cur.is_empty() || !params.is_empty() {
                    args.push(cur);
                }
                // 参数个数不符：双侧定位（判据）
                if args.len() != params.len() {
                    return Err(err(
                        "E_MACRO_ARITY",
                        t.pos,
                        Some((def.def_line, def.def_pos)),
                        format!(
                            "宏 {} 需要 {} 个参数，调用给了 {} 个",
                            def.name,
                            params.len(),
                            args.len()
                        ),
                        "参数个数不符——实参表与形参表必须一一对应".to_string(),
                        format!(
                            "对照第 {} 行的定义修正实参个数",
                            def.def_line
                        ),
                    ));
                }
                active.push((def.name.clone(), def.def_pos));
                if active.len() > cfg.max_depth {
                    let d = (def.def_line, def.def_pos);
                    return Err(depth_err(&def.name, t.pos, d, active.len()));
                }
                // 实参先全量展开（C 系语义——除 #/## 操作数）
                let mut exp_args: Vec<Vec<Tok>> = Vec::new();
                for a in &args {
                    let sub = expand_toks(table, a, cfg, active, out, None)?;
                    exp_args.push(sub.iter().map(|e| Tok { text: e.text.clone(), is_ident: e.is_ident, pos: e.call_pos }).collect());
                }
                // 体展开：# 串接、## 粘贴、参数代入
                let body_toks = tokenize(&def.body);
                let mut expanded: Vec<ExpTok> = Vec::new();
                let mut k = 0usize;
                while k < body_toks.len() {
                    let bt = &body_toks[k];
                    if bt.text == "#" && k + 1 < body_toks.len() {
                        // 串接：#参数 → 实参实文的字符串字面量
                        let p = &body_toks[k + 1];
                        if let Some(pi) = params.iter().position(|x| *x == p.text) {
                            let raw = args[pi]
                                .iter()
                                .map(|x| x.text.clone())
                                .collect::<Vec<_>>()
                                .join(" ");
                            expanded.push(ExpTok {
                                text: format!("\"{}\"", raw),
                                is_ident: false,
                                call_pos: t.pos,
                                def_line: Some(def.def_line),
                                def_pos: Some(def.def_pos),
                            });
                            k += 2;
                            continue;
                        }
                    }
                    if bt.text == "##" && k + 1 < body_toks.len() && !expanded.is_empty() {
                        // 粘贴：左记号 + 右记号拼成单记号（实参取实文）
                        let r = &body_toks[k + 1];
                        let right_text = match params.iter().position(|x| *x == r.text) {
                            Some(ri) => args[ri]
                                .iter()
                                .map(|x| x.text.clone())
                                .collect::<Vec<_>>()
                                .join(" "),
                            None => r.text.clone(),
                        };
                        let left = expanded.pop().unwrap();
                        expanded.push(ExpTok {
                            text: format!("{}{}", left.text, right_text),
                            is_ident: left.is_ident || right_text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'),
                            call_pos: t.pos,
                            def_line: Some(def.def_line),
                            def_pos: Some(def.def_pos),
                        });
                        k += 2;
                        continue;
                    }
                    // 参数代入（代入已展开实参）或原样
                    if bt.is_ident {
                        if let Some(pi) = params.iter().position(|x| *x == bt.text) {
                            for at in &exp_args[pi] {
                                expanded.push(ExpTok {
                                    text: at.text.clone(),
                                    is_ident: at.is_ident,
                                    call_pos: t.pos,
                                    def_line: Some(def.def_line),
                                    def_pos: Some(def.def_pos),
                                });
                            }
                            k += 1;
                            continue;
                        }
                    }
                    expanded.push(plain(bt, Some((def.def_line, def.def_pos))));
                    k += 1;
                }
                // 产物重扫：体里可能还有别的宏
                let sub_toks: Vec<Tok> = expanded
                    .iter()
                    .map(|e| Tok { text: e.text.clone(), is_ident: e.is_ident, pos: e.call_pos })
                    .collect();
                let sub = expand_toks(table, &sub_toks, cfg, active, out, Some((def.def_line, def.def_pos)))?;
                result.extend(sub);
                active.pop();
                i = j + 1;
            }
        }
    }
    Ok(result)
}

fn plain(t: &Tok, from_def: Option<(usize, usize)>) -> ExpTok {
    ExpTok {
        text: t.text.clone(),
        is_ident: t.is_ident,
        call_pos: t.pos,
        def_line: from_def.map(|d| d.0),
        def_pos: from_def.map(|d| d.1),
    }
}

fn depth_err(name: &str, pos: usize, def: (usize, usize), depth: usize) -> MacroError {
    err(
        "E_MACRO_DEPTH",
        pos,
        Some(def),
        format!("宏 {} 展开深度超过上限（当前 {} 层）", name, depth),
        "深度超限报错指向定义处——互递归展开不冻结会无限进行".to_string(),
        "检查互引用的宏对（自引用已被冻结，互引用需要显性截断）".to_string(),
    )
}

/// F0412 判据自检（域聚合入口）。
pub fn run_vec12_checks() -> crate::checks::CheckSet {
    super::vec12_checks::run_vec12_checks()
}
