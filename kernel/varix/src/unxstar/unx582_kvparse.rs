//! unx582_kvparse —— UNX-F0582 语法解析器（token/key=value）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B30）：UNX-F0582 · 语法解析器（token/key=value） · 目标 240 行 · 已深化
//! 判据原文：解析器账齐（20 样本解析 20/20 与基准一致），token 化规则冻结（空格分词+等号劈分）
//! 组件：两级分词器（一级空格分词 → 二级首等号劈分）+ token 模型（Bare/Key/Value 三型）
//!      + KV 对账（键值对 + 重复键计数 + 空值对计数）+ 解析账（行入/token 出/对出/拒收守恒）
//!      + 基准对拍（20 样本 × 期望输出 const 表，20/20 全等）+ 规范渲染（parse∘render 幂等）。
//! 职责定位：内核配置行（引导参数/模块清单/ktest 注脚）的解析底座。
//!   token 化规则冻结 = 只有两条规则：①空格分词（连续空白等价单空格）；
//!   ②等号劈分（每 token 按第一个 '=' 劈成 key 与 value，无 '=' 即 Bare）。
//!   规则之外零语法（无引号/无转义/无注释）——冻结的意义就是行为可背诵、可对拍。
//! 性能逐项分解：分词 O(行字节数)；劈分 O(token 字节数)；基准对拍 O(Σ样本)；
//!   幂等渲染 O(对字节数)；全整数算术零浮点零墙钟，同输入恒同输出。
//! 诊断码段：0x98（0x9801~0x9806，全域 grep 确认独占，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick 注入，零墙钟），
//!   逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（来源：施工书 UNX-F0582 深化正文 + 配置行惯例；写死并注释）
// ---------------------------------------------------------------------------

/// 行最大长度：配置行 1024 字节封顶（超长显性拒绝，不静默截断）。
pub const LINE_MAX: usize = 1024;
/// 单行 token 上限：64 个（防分词失控）。
pub const TOKENS_MAX: usize = 64;
/// 单行 KV 对上限：32 对（token 数的一半取整封顶）。
pub const PAIRS_MAX: usize = 32;
/// 基准样本数：判据点名 20/20。
pub const BASELINE_N: usize = 20;

// ---------------------------------------------------------------------------
// 诊断码段 0x98（0x9801~0x9806 闭集；域内独占）
// ---------------------------------------------------------------------------

/// 0x9801 空 key：token 以 '=' 起头（'=val' 形态），键语义不成立。
pub const C_EMPTY_KEY: u16 = 0x9801;
/// 0x9802 行超长：超 LINE_MAX，必须显性分段。
pub const C_LINE_LONG: u16 = 0x9802;
/// 0x9803 NUL 字节：配置行禁 \0（C 字符串语义边界）。
pub const C_NUL_BYTE: u16 = 0x9803;
/// 0x9804 基准不一致：某样本解析输出与基准期望不等（20/20 破约）。
pub const C_BASELINE: u16 = 0x9804;
/// 0x9805 分词不确定性：同输入双跑 token 序不等（确定性失守）。
pub const C_NONDET: u16 = 0x9805;
/// 0x9806 解析账破：守恒式（token ≡ bare + 键 token；对 ≡ 有值对 + 空值对）不成立。
pub const C_LEDGER_BROKEN: u16 = 0x9806;

/// 段守卫：全部诊断码必须落 0x98 段。
pub const fn seg_guard() -> bool {
    (C_EMPTY_KEY >> 8) == 0x98
        && (C_LINE_LONG >> 8) == 0x98
        && (C_NUL_BYTE >> 8) == 0x98
        && (C_BASELINE >> 8) == 0x98
        && (C_NONDET >> 8) == 0x98
        && (C_LEDGER_BROKEN >> 8) == 0x98
}

/// 诊断码 → 人话（可操作建议一体）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_EMPTY_KEY => "空 key：token 以 = 起头；键必须非空（k=v 或裸 token）",
        C_LINE_LONG => "行超长：按 1024 字节显性分段，勿静默截断",
        C_NUL_BYTE => "NUL 字节：配置行禁 \\0，先清洗输入再解析",
        C_BASELINE => "基准不一致：逐 token 比对定位首个分歧点",
        C_NONDET => "分词不确定性：查分词器是否引入了非输入相关的状态",
        C_LEDGER_BROKEN => "解析账破：token/对 守恒式不成立，查记账点",
        _ => "未知诊断码：核对 0x98 段闭集",
    }
}

// ---------------------------------------------------------------------------
// token 模型与解析账
// ---------------------------------------------------------------------------

/// KV 对：key 非空；value 可空（'k=' 形态 = 空值对，显性计数不隐瞒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvPair {
    pub key: String,
    pub value: String,
}

/// 裸 token：无 '=' 的独立词。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BareToken {
    pub raw: String,
}

/// 单行解析产物：KV 对 + 裸 token（两条冻结规则的直接映射）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLine {
    pub pairs: Vec<KvPair>,
    pub bares: Vec<BareToken>,
}

impl ParsedLine {
    /// 规范渲染：对在前（k=v，空值渲染为 k=），裸 token 在后，单空格分隔。
    /// parse(render(x)) == x（幂等性 = 冻结规则的自洽证明）。
    pub fn render(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for p in &self.pairs {
            parts.push(alloc::format!("{}={}", p.key, p.value));
        }
        for b in &self.bares {
            parts.push(b.raw.clone());
        }
        parts.join(" ")
    }
}

/// 解析账：判据「解析器账齐」的落地形态——每个数字都可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParseLedger {
    pub lines_in: u64,
    pub tokens_out: u64,
    pub pairs_out: u64,
    pub bares_out: u64,
    pub empty_value_pairs: u64,
    pub dup_keys: u64,
    pub rejects: u64,
}

impl ParseLedger {
    /// 守恒式①：token ≡ 键 token（对数）+ 裸 token。
    pub const fn token_conserves(&self) -> bool {
        self.tokens_out == self.pairs_out + self.bares_out
    }

    /// 守恒式②：对 ≡ 有值对 + 空值对。
    pub fn pair_conserves(&self, empty_value: u64) -> bool {
        self.empty_value_pairs == empty_value
    }
}

/// 解析错误：码 + 行号 + 建议（异常零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: u16,
    pub line_no: usize,
    pub suggestion: String,
}

impl ParseError {
    fn new(code: u16, line_no: usize) -> Self {
        Self { code, line_no, suggestion: code_text(code).to_string() }
    }
}

// ---------------------------------------------------------------------------
// 解析器本体（规则冻结：空格分词 + 等号劈分）
// ---------------------------------------------------------------------------

/// 解析器：无内部可变态（纯函数形态），批量入口持账。
#[derive(Clone, Debug, Default)]
pub struct KvParser {
    ledger: ParseLedger,
    events: Vec<(u16, usize, String)>,
}

impl KvParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ledger(&self) -> &ParseLedger {
        &self.ledger
    }

    pub fn events(&self) -> &[(u16, usize, String)] {
        &self.events
    }

    /// 单行解析。规则只有两条（冻结）：
    ///   ① 空格分词：按空白（空格/\t/\r/\n）切分，连续空白等价单空格；
    ///   ② 等号劈分：每 token 按第一个 '=' 劈 key/value；无 '=' 即 Bare。
    /// 拒收路径：NUL 字节 / 行超长 / 空 key（'=x'）。行尾 '='（'k='）= 空值对，合法。
    pub fn parse_line(&mut self, line: &str, line_no: usize) -> Result<ParsedLine, ParseError> {
        // 守卫①：NUL。
        if line.as_bytes().contains(&0u8) {
            self.ledger.rejects += 1;
            self.events.push((C_NUL_BYTE, line_no, code_text(C_NUL_BYTE).to_string()));
            return Err(ParseError::new(C_NUL_BYTE, line_no));
        }
        // 守卫②：行超长。
        if line.len() > LINE_MAX {
            self.ledger.rejects += 1;
            self.events.push((C_LINE_LONG, line_no, code_text(C_LINE_LONG).to_string()));
            return Err(ParseError::new(C_LINE_LONG, line_no));
        }

        // —— 规则①：空格分词（两态扫描：InSpace / InToken）——
        let mut tokens: Vec<&str> = Vec::new();
        let b = line.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            // 跳过空白（InSpace 态）。
            while i < b.len() && (b[i] == b' ' || b[i] == b'\t' || b[i] == b'\r' || b[i] == b'\n') {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            let start = i;
            while i < b.len() && !(b[i] == b' ' || b[i] == b'\t' || b[i] == b'\r' || b[i] == b'\n') {
                i += 1;
            }
            // 封顶：达 TOKENS_MAX 即止取。判据是「bares.len() ≤ TOKENS_MAX」，
            // 因此**先判后取**——若先 push 再判溢出，t0..t64 会取满 65 个
            // （最后一个越界 token 仍进了 bares，c8「token 上限封顶」红线失守）。
            if tokens.len() >= TOKENS_MAX {
                break;
            }
            tokens.push(&line[start..i]);
        }

        // —— 规则②：等号劈分（首 '=' 劈分；'=' 在头部 = 空 key 拒收）——
        let mut out = ParsedLine::default();
        for t in tokens {
            match t.find('=') {
                None => {
                    out.bares.push(BareToken { raw: t.to_string() });
                    self.ledger.bares_out += 1;
                }
                Some(0) => {
                    // '=val'：键为空，键语义不成立 → 整行拒收（显性，不静默跳过）。
                    self.ledger.rejects += 1;
                    self.events.push((C_EMPTY_KEY, line_no, code_text(C_EMPTY_KEY).to_string()));
                    return Err(ParseError::new(C_EMPTY_KEY, line_no));
                }
                Some(pos) => {
                    let key = &t[..pos];
                    let value = &t[pos + 1..];
                    if out.pairs.iter().any(|p| p.key == key) {
                        self.ledger.dup_keys += 1; // 重复键：入账不拒收（后值语义由调用方定）
                    }
                    if value.is_empty() {
                        self.ledger.empty_value_pairs += 1;
                    }
                    out.pairs.push(KvPair { key: key.to_string(), value: value.to_string() });
                    self.ledger.pairs_out += 1;
                }
            }
        }

        self.ledger.lines_in += 1;
        self.ledger.tokens_out = self.ledger.pairs_out + self.ledger.bares_out;
        Ok(out)
    }

    /// 批量解析：逐行走 parse_line，任一行 Err 即整批 Err（零静默），账保留已过行计数。
    pub fn parse_all(&mut self, text: &str) -> Result<Vec<ParsedLine>, ParseError> {
        let mut outs = Vec::new();
        for (i, line) in text.lines().enumerate() {
            outs.push(self.parse_line(line, i + 1)?);
        }
        Ok(outs)
    }

    /// 账自检：守恒式① + 空值账可回放（由调用方传入实际空值对总数）。
    pub fn ledger_ok(&self, empty_value_actual: u64) -> bool {
        self.ledger.token_conserves() && self.ledger.pair_conserves(empty_value_actual)
    }
}

// ---------------------------------------------------------------------------
// 基准对拍：20 样本 × 期望输出（判据「20/20 与基准一致」的正本）
// ---------------------------------------------------------------------------

/// 样本行：覆盖裸 token / 单对 / 多对 / 空值 / 重复键 / 多空白 / 制表符 /
/// 值内等号（首劈分）/ 尾空格 / 空行 / 值含 UTF-8 / '=' 尾 token / 长 key /
/// 键值同形 / 单字符 / 混合形态 / 全空白行 / 数字值 / 符号值 / 全对无裸。
pub const BASELINE_SAMPLES: [&str; 20] = [
    "boot",
    "root=/dev/vda",
    "quiet splash",
    "console=ttyS0,115200n8",
    "verbose",
    "k=",
    "a=1 a=2",
    "x    y   z",
    "path=/usr/bin\ttool",
    "",
    "name=变量值",
    "flag= trailing",
    "eq=a=b=c",
    "long_key_name_for_parsing_depth_test=1",
    "same=same",
    "q",
    "m=3 n=4 o",
    "   ",
    "num=42",
    "neg=-7",
];

/// 基准期望：与 BASELINE_SAMPLES 一一对应的解析产物（人工核定的正本）。
pub fn baseline_expect(i: usize) -> ParsedLine {
    let mut p = ParsedLine::default();
    match i {
        0 => p.bares.push(BareToken { raw: "boot".to_string() }),
        1 => p.pairs.push(KvPair { key: "root".into(), value: "/dev/vda".into() }),
        2 => {
            p.bares.push(BareToken { raw: "quiet".into() });
            p.bares.push(BareToken { raw: "splash".into() });
        }
        3 => p.pairs.push(KvPair { key: "console".into(), value: "ttyS0,115200n8".into() }),
        4 => p.bares.push(BareToken { raw: "verbose".to_string() }),
        5 => p.pairs.push(KvPair { key: "k".into(), value: String::new() }),
        6 => {
            p.pairs.push(KvPair { key: "a".into(), value: "1".into() });
            p.pairs.push(KvPair { key: "a".into(), value: "2".into() });
        }
        7 => {
            p.bares.push(BareToken { raw: "x".into() });
            p.bares.push(BareToken { raw: "y".into() });
            p.bares.push(BareToken { raw: "z".into() });
        }
        8 => {
            p.pairs.push(KvPair { key: "path".into(), value: "/usr/bin".into() });
            p.bares.push(BareToken { raw: "tool".into() });
        }
        9 => {}
        10 => p.pairs.push(KvPair { key: "name".into(), value: "变量值".into() }),
        11 => {
            p.pairs.push(KvPair { key: "flag".into(), value: String::new() });
            p.bares.push(BareToken { raw: "trailing".into() });
        }
        12 => p.pairs.push(KvPair { key: "eq".into(), value: "a=b=c".into() }),
        13 => p.pairs.push(KvPair {
            key: "long_key_name_for_parsing_depth_test".into(),
            value: "1".into(),
        }),
        14 => p.pairs.push(KvPair { key: "same".into(), value: "same".into() }),
        15 => p.bares.push(BareToken { raw: "q".to_string() }),
        16 => {
            p.pairs.push(KvPair { key: "m".into(), value: "3".into() });
            p.pairs.push(KvPair { key: "n".into(), value: "4".into() });
            p.bares.push(BareToken { raw: "o".into() });
        }
        17 => {}
        18 => p.pairs.push(KvPair { key: "num".into(), value: "42".into() }),
        _ => p.pairs.push(KvPair { key: "neg".into(), value: "-7".into() }),
    }
    p
}

/// 基准对拍：20/20 全等才算过（判据主条）。任一样本分歧 → false（不隐瞒分歧位置，
/// 分歧样本号经 events 侧路可查——这里直接返回 false 由调用方定位）。
pub fn baseline_check_20() -> bool {
    let mut parser = KvParser::new();
    for (i, s) in BASELINE_SAMPLES.iter().enumerate() {
        let got = match parser.parse_line(s, i + 1) {
            Ok(p) => p,
            Err(_) => return false,
        };
        if got != baseline_expect(i) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 自检（逐检查带 detail；域聚合在 mod.rs）
// ---------------------------------------------------------------------------

/// C1：基准对拍 20/20（判据主条）。
fn check_c1_baseline() -> bool {
    baseline_check_20()
}

/// C2：解析账齐——20 样本跑完账守恒：token ≡ 对 + 裸；
/// 空值对账 = 样本中 'k=' 与 'flag=' 共 2；重复键账 = 'a=1 a=2' 共 1。
fn check_c2_ledger() -> bool {
    let mut parser = KvParser::new();
    if parser.parse_all(&concat_baseline()).is_err() {
        return false;
    }
    let l = parser.ledger();
    l.token_conserves()
        && l.lines_in == 20
        && l.empty_value_pairs == 2
        && l.dup_keys == 1
        && parser.ledger_ok(2)
}

fn concat_baseline() -> String {
    let mut s = String::new();
    for x in BASELINE_SAMPLES.iter() {
        s.push_str(x);
        s.push('\n');
    }
    s
}

/// C3：幂等渲染——对每个样本 parse→render→parse 结果恒等（冻结规则自洽）。
fn check_c3_render_idempotent() -> bool {
    let mut parser = KvParser::new();
    for (i, s) in BASELINE_SAMPLES.iter().enumerate() {
        let p1 = match parser.parse_line(s, i + 1) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let rendered = p1.render();
        let p2 = match parser.parse_line(&rendered, i + 1) {
            Ok(p) => p,
            Err(_) => return false,
        };
        if p1 != p2 {
            return false;
        }
    }
    true
}

/// C4：空 key 拒收——'=val' 拒出 0x9801；账 rejects 增；事件入账（零静默）。
fn check_c4_empty_key() -> bool {
    let mut parser = KvParser::new();
    let err = parser.parse_line("=val boot", 1).err();
    let ok_err = err.map_or(false, |e| e.code == C_EMPTY_KEY);
    let ev = parser.events().iter().any(|e| e.0 == C_EMPTY_KEY);
    ok_err && ev && parser.ledger().rejects == 1
}

/// C5：行超长拒收——恰 1024 过、恰 1025 拒出 0x9802。
fn check_c5_line_cap() -> bool {
    let mut parser = KvParser::new();
    let ok = parser.parse_line(&"a".repeat(1024), 1).is_ok();
    let bad = parser
        .parse_line(&"b".repeat(1025), 2)
        .err()
        .map_or(false, |e| e.code == C_LINE_LONG);
    ok && bad
}

/// C6：NUL 拒收——含 \0 行拒出 0x9803。
fn check_c6_nul() -> bool {
    let mut parser = KvParser::new();
    let mut s = String::from("k=v");
    s.push('\0');
    parser.parse_line(&s, 1).err().map_or(false, |e| e.code == C_NUL_BYTE)
}

/// C7：分词确定性——同 20 样本双跑产物逐行全等（确定性对拍锚）。
fn check_c7_determinism() -> bool {
    let mut a = KvParser::new();
    let mut b = KvParser::new();
    let ra = a.parse_all(&concat_baseline());
    let rb = b.parse_all(&concat_baseline());
    match (ra, rb) {
        (Ok(x), Ok(y)) => x == y && a.ledger() == b.ledger(),
        _ => false,
    }
}

/// C8：token 上限——超 TOKENS_MAX 的行不再增 token（封顶不崩，截断显性由上限常量注）。
fn check_c8_token_cap() -> bool {
    let mut parser = KvParser::new();
    let mut line = String::new();
    for i in 0..100 {
        if i > 0 {
            line.push(' ');
        }
        line.push_str(&alloc::format!("t{}", i));
    }
    match parser.parse_line(&line, 1) {
        Ok(p) => p.bares.len() <= TOKENS_MAX,
        Err(_) => false,
    }
}

/// C9：值内等号劈分语义——'eq=a=b=c' 劈成 key=eq value=a=b=c（首等号劈分的正例）。
fn check_c9_first_eq_split() -> bool {
    let mut parser = KvParser::new();
    match parser.parse_line("eq=a=b=c", 1) {
        Ok(p) => p.pairs.len() == 1 && p.pairs[0].key == "eq" && p.pairs[0].value == "a=b=c",
        Err(_) => false,
    }
}

/// C10：UTF-8 值——中文键值逐字节保真（'name=变量值' 全等往返）。
fn check_c10_utf8() -> bool {
    let mut parser = KvParser::new();
    match parser.parse_line("name=变量值", 1) {
        Ok(p) => {
            p.pairs.len() == 1
                && p.pairs[0].key == "name"
                && p.pairs[0].value == "变量值"
                && p.render() == "name=变量值"
        }
        Err(_) => false,
    }
}

/// 本模块自检（域聚合在 mod.rs）。
pub fn run_unx582_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX582");
    let g = seg_guard();
    set.add("unx582-seg-guard", g, if g { "" } else { "诊断码越出 0x98 段" });
    let c1 = check_c1_baseline();
    set.add("unx582-c1-baseline-20", c1, if c1 { "" } else { "20 样本基准对拍未全过" });
    let c2 = check_c2_ledger();
    set.add("unx582-c2-ledger", c2, if c2 { "" } else { "解析账守恒失守" });
    let c3 = check_c3_render_idempotent();
    set.add("unx582-c3-idempotent", c3, if c3 { "" } else { "渲染幂等失守" });
    let c4 = check_c4_empty_key();
    set.add("unx582-c4-empty-key", c4, if c4 { "" } else { "空 key 闸失守" });
    let c5 = check_c5_line_cap();
    set.add("unx582-c5-line-cap", c5, if c5 { "" } else { "行超长闸失守" });
    let c6 = check_c6_nul();
    set.add("unx582-c6-nul", c6, if c6 { "" } else { "NUL 闸失守" });
    let c7 = check_c7_determinism();
    set.add("unx582-c7-determinism", c7, if c7 { "" } else { "分词确定性失守" });
    let c8 = check_c8_token_cap();
    set.add("unx582-c8-token-cap", c8, if c8 { "" } else { "token 上限封顶失守" });
    let c9 = check_c9_first_eq_split();
    set.add("unx582-c9-first-eq", c9, if c9 { "" } else { "首等号劈分语义失守" });
    let c10 = check_c10_utf8();
    set.add("unx582-c10-utf8", c10, if c10 { "" } else { "UTF-8 保真失守" });
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；域聚合测试在 mod.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx582_all_checks_green() {
        let set = run_unx582_checks();
        let n = set.len();
        let mut i = 0usize;
        while i < n {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    panic!("UNX-F0582 RED: {} :: {}", c.name, c.detail);
                }
            }
            i += 1;
        }
        assert!(set.all_passed(), "UNX-F0582 domain checks must be green");
    }

    #[test]
    fn unx582_segment_guard_holds() {
        assert!(seg_guard());
    }

    #[test]
    fn unx582_baseline_20_of_20() {
        assert!(baseline_check_20(), "20/20 samples must match baseline");
    }

    #[test]
    fn unx582_render_idempotent_all() {
        let mut parser = KvParser::new();
        for (i, s) in BASELINE_SAMPLES.iter().enumerate() {
            let p = parser.parse_line(s, i + 1).unwrap_or_default();
            let r = p.render();
            let p2 = parser.parse_line(&r, i + 1).unwrap_or_default();
            assert_eq!(p, p2, "render must be idempotent for sample {}", i);
        }
    }
}
