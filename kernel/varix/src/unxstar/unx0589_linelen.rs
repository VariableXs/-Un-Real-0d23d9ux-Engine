//! unx0589_linelen —— UNX-F0589 参数行长度守卫（单 token 界）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0589 | 参数行长度守卫（单 token 界） | 200 | 已深化 |
//!   | 行长守卫齐（token 128 界 + 行 1024 界），越界检出 4/4 与基准一致 |
//!
//! 链路定位：上游 F0588 引号账产出 token 边界，本条在边界上落**长度
//!   硬界**——命令行参数账的两条界：单 token 128 字节（含引号原文）、
//!   整行 1024 字节。两条界都是**预先拒收**（先量后收，不存在「收进来
//!   再截断」——截断是语义破坏）。越界检出 4 样基准：token 超 128、
//!   行超 1024、token 恰 128（过界=收）、行恰 1024（过界=收）。
//!   下游 F0590 缺省值账只接收本条放行的 token；F0596 五数概括账对
//!   本条的长度分布落统计面。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（LEN_SEMANTICS）：
//!   1. 单 token 界 128：token 原文（含引号）字节数；> 128 拒，
//!      == 128 收（「过界」判据口径：等于界值不算越界）；
//!   2. 整行界 1024：空格分隔前整行字节数；> 1024 拒整行（行拒 =
//!      行内全部 token 连坐拒收，逐 token 入拒收账）；
//!   3. 分词：空格（0x20）分词；引号内空格不分词（F0588 已定义引号
//!      边界——本条**先做引号感知分词**再量长度）；
//!   4. 拒收入账不中断：越界 token 入拒收账后继续扫余下行；
//!   5. 空行：零 token，不算错也不入账（报告里计「空行数」）。
//!
//! 组件清单：
//!   1. TOKEN_CAP / LINE_CAP      两条硬界常量（128/1024）
//!   2. LenError                  诊断码 0x4A20~0x4A2F（5 变体中文）
//!   3. tokenize_lenient          引号感知分词（产出 token 边界）
//!   4. TokenLen                  token 长度账目（原文长 + 判定）
//!   5. LineGuard                 行守卫主体：量行 → 分词 → 逐 token
//!                                量长 → 通过/拒收分账 + 报告 + 指纹
//!   6. fnv64                     FNV-1a 指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const LG_VERSION: &str = "unx0589_linelen-v1";

/// 诊断段基址（0x4A20 段 · 本模块独占 · 择取实录：0x4A00 归 F0588）。
pub const LG_SEG_BASE: u16 = 0x4A20;
/// 诊断段顶。
pub const LG_SEG_TOP: u16 = 0x4A2F;

/// 单 token 硬界（字节；含引号原文）。
pub const TOKEN_CAP: usize = 128;
/// 整行硬界（字节）。
pub const LINE_CAP: usize = 1024;

// ═══════════════════════════════════════════════════════════════════════
// 1. LenError —— 诊断码闭集
// ═══════════════════════════════════════════════════════════════════════

/// 行长守卫诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LenError {
    /// 单 token 超 128 界。
    TokenOver = 0x4A20,
    /// 整行超 1024 界（行内连坐拒收）。
    LineOver = 0x4A21,
    /// token 内引号未闭合（分词阶段即拒——转 F0588 语义）。
    Unbalanced = 0x4A22,
    /// 零 token 行被误要求判定（调用方口径错）。
    EmptyLine = 0x4A23,
}

impl LenError {
    /// 诊断码。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 中文诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            LenError::TokenOver => {
                "单 token 超 128 字节界。建议：拆分为多 token 或缩短值；\
                 界是预先拒收的硬界，不做截断。"
            }
            LenError::LineOver => {
                "整行超 1024 字节界，行内 token 连坐拒收。建议：\
                 拆行为多行或减少参数个数。"
            }
            LenError::Unbalanced => {
                "token 内引号未闭合，分词即拒。建议：先修引号配对\
                 （联动 F0588 未闭合拒收语义）。"
            }
            LenError::EmptyLine => {
                "空行无可判定 token。建议：调用方先判空再进守卫，\
                 或按「空行计数」口径归零账。"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. tokenize_lenient —— 引号感知分词
// ═══════════════════════════════════════════════════════════════════════

/// 引号感知分词产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenizeResult {
    /// 成功：token 原文列表（含引号）。
    Ok(Vec<String>),
    /// 引号失衡（开引号在行尾前未闭合——F0588 拒收语义前移到分词）。
    Unbalanced { at: usize },
}

/// 按空格分词，引号内空格不分词；反斜杠转义的空格亦不分词。
///
/// 返回的 token 是**原文切片**（含引号），长度判定用原文长。
pub fn tokenize_lenient(line: &str) -> TokenizeResult {
    let b = line.as_bytes();
    let mut toks: Vec<String> = Vec::new();
    let mut i = 0usize;
    let n = b.len();

    while i < n {
        // 跳过空白。
        while i < n && b[i] == b' ' {
            i += 1;
        }
        if i >= n {
            break;
        }
        let start = i;
        let mut quote: Option<u8> = None;
        while i < n {
            let c = b[i];
            if c == b'\\' && i + 1 < n && b[i + 1] == b' ' {
                // 转义空格：不终止 token。
                i += 2;
                continue;
            }
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                    // 转义的引号字符不闭合。
                    else if c == b'\\' && i + 1 < n && b[i + 1] == q {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                None => {
                    if c == b' ' {
                        break;
                    }
                    if c == b'"' || c == b'\'' {
                        quote = Some(c);
                    }
                    i += 1;
                }
            }
        }
        if quote.is_some() {
            return TokenizeResult::Unbalanced { at: start };
        }
        toks.push(String::from_utf8_lossy(&b[start..i]).into_owned());
    }
    TokenizeResult::Ok(toks)
}

// ═══════════════════════════════════════════════════════════════════════
// 3. TokenLen / LineGuard —— 账目与守卫主体
// ═══════════════════════════════════════════════════════════════════════

/// 单 token 长度账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenLen {
    /// 原文（截 64 字节存证）。
    pub head: String,
    /// 原文字节长。
    pub len: usize,
    /// 判定：true = 放行（len <= TOKEN_CAP）。
    pub passed: bool,
}

/// 行守卫判定产物。
#[derive(Clone, Debug)]
pub struct LineVerdict {
    /// 行长（字节）。
    pub line_len: usize,
    /// 行级判定（false = 行超界，token 连坐拒）。
    pub line_ok: bool,
    /// token 账目。
    pub tokens: Vec<TokenLen>,
    /// 通过 token 数。
    pub passed_n: usize,
    /// 拒收 token 数。
    pub rejected_n: usize,
    /// 行级/单 token 诊断（全部通过时为 None）。
    pub err: Option<LenError>,
}

/// 行长守卫主体。
#[derive(Clone, Debug, Default)]
pub struct LineGuard {
    /// 历史判定账。
    pub verdicts: Vec<LineVerdict>,
    /// 累计空行数。
    pub empty_lines: u32,
    /// 累计越界 token 数。
    pub over_tokens: u32,
    /// 累计越界行数。
    pub over_lines: u32,
}

impl LineGuard {
    /// 空守卫。
    pub fn new() -> Self {
        Self::default()
    }

    /// 判定一行。语义见头注 LEN_SEMANTICS。
    pub fn guard(&mut self, line: &str) -> LineVerdict {
        let line_len = line.len();

        // 行级预检（先量后分——超界连坐）。
        if line_len > LINE_CAP {
            self.over_lines += 1;
            let v = LineVerdict {
                line_len,
                line_ok: false,
                tokens: Vec::new(),
                passed_n: 0,
                rejected_n: 0,
                err: Some(LenError::LineOver),
            };
            self.verdicts.push(v.clone());
            return v;
        }

        // 分词。
        let toks = match tokenize_lenient(line) {
            TokenizeResult::Ok(t) => t,
            TokenizeResult::Unbalanced { .. } => {
                let v = LineVerdict {
                    line_len,
                    line_ok: false,
                    tokens: Vec::new(),
                    passed_n: 0,
                    rejected_n: 0,
                    err: Some(LenError::Unbalanced),
                };
                self.verdicts.push(v.clone());
                return v;
            }
        };

        if toks.is_empty() {
            self.empty_lines += 1;
            let v = LineVerdict {
                line_len,
                line_ok: true,
                tokens: Vec::new(),
                passed_n: 0,
                rejected_n: 0,
                err: None,
            };
            self.verdicts.push(v.clone());
            return v;
        }

        // 逐 token 量长（先量后收——不存在截断）。
        let mut tokens = Vec::new();
        let mut passed_n = 0usize;
        let mut rejected_n = 0usize;
        for t in &toks {
            let len = t.len();
            let passed = len <= TOKEN_CAP;
            if passed {
                passed_n += 1;
            } else {
                rejected_n += 1;
                self.over_tokens += 1;
            }
            tokens.push(TokenLen {
                head: t.chars().take(64).collect(),
                len,
                passed,
            });
        }

        let v = LineVerdict {
            line_len,
            line_ok: rejected_n == 0,
            tokens,
            passed_n,
            rejected_n,
            err: if rejected_n > 0 { Some(LenError::TokenOver) } else { None },
        };
        self.verdicts.push(v.clone());
        v
    }

    /// 全账健康：零越界（token 级 + 行级）。
    pub fn all_within(&self) -> bool {
        self.over_tokens == 0 && self.over_lines == 0
    }

    /// 长度分布摘要（min/max/总字节/token 数——F0596 五数概括的进料）。
    pub fn len_stats(&self) -> (usize, usize, usize, usize) {
        let mut min = usize::MAX;
        let mut max = 0usize;
        let mut sum = 0usize;
        let mut n = 0usize;
        for v in &self.verdicts {
            for t in &v.tokens {
                if t.len < min {
                    min = t.len;
                }
                if t.len > max {
                    max = t.len;
                }
                sum += t.len;
                n += 1;
            }
        }
        if n == 0 {
            (0, 0, 0, 0)
        } else {
            (min, max, sum, n)
        }
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = format!(
            "行长守卫账 · 行 {}（空 {}）· token 越界 {} · 行越界 {}",
            self.verdicts.len(),
            self.empty_lines,
            self.over_tokens,
            self.over_lines
        );
        for v in &self.verdicts {
            let line = if !v.line_ok {
                format!(
                    "\n  行 {}B ✗ {}",
                    v.line_len,
                    v.err.map(|e| e.detail()).unwrap_or("未知")
                )
            } else if v.tokens.is_empty() {
                "\n  （空行）".to_string()
            } else {
                let mut l = format!("\n  行 {}B ✓ {} token", v.line_len, v.passed_n);
                for t in &v.tokens {
                    if !t.passed {
                        l.push_str(&format!("\n    「{}」{}B 越界", t.head, t.len));
                    }
                }
                l
            };
            s.push_str(&line);
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 构造 n 字节 token（'x' 填充）。
    fn tok(n: usize) -> String {
        let mut s = String::new();
        for _ in 0..n {
            s.push('x');
        }
        s
    }

    /// 判据：越界检出 4/4 与基准一致。
    /// 基准四样：token 超 128（拒）/ 行超 1024（连坐拒）/ token 恰 128（收）/
    /// 行恰 1024（收——行界口径：等于不算越）。
    #[test]
    fn boundary_detection_four_ways() {
        let mut set = CheckSet::new("unxstar-unx0589");
        let mut g = LineGuard::new();

        // 样 1：token 129B > 128 → 拒。
        let v1 = g.guard(&tok(129));
        set.add("越界1·token129B 拒", !v1.line_ok && v1.rejected_n == 1, "129>128");
        set.add(
            "越界1·诊断码 TokenOver",
            v1.err == Some(LenError::TokenOver),
            "E_TOKEN_OVER",
        );

        // 样 2：行 1025B > 1024 → 整行连坐拒（分词都不做）。
        let mut long_line = String::new();
        for i in 0..9 {
            if i > 0 {
                long_line.push(' ');
            }
            long_line.push_str(&tok(114));
        }
        // 9×114 + 8 空格 = 1034B > 1024
        let v2 = g.guard(&long_line);
        set.add("越界2·行1034B 连坐拒", !v2.line_ok, "1024<1034");
        set.add(
            "越界2·诊断码 LineOver",
            v2.err == Some(LenError::LineOver),
            "E_LINE_OVER",
        );

        // 样 3：token 恰 128B → 收（等于界不算越）。
        let v3 = g.guard(&tok(128));
        set.add("过界1·token128B 收", v3.line_ok && v3.passed_n == 1, "128==CAP");

        // 样 4：行恰 1024B（多 token 合计）→ 收。
        // 8×114 + 7 = 919… 精确构造：8 token 各 127B + 7 空格 = 1023B；
        // 再补 1 字节 → 9 token 不好整。改用 4×255+3 = 1023，不行超 token 界。
        // 用 2 token：511 + 510 + 1 空格 = 1022；再 +1 = 1023。
        // 最简：8×127 + 7 = 1023；+1 字节行尾？行界量的是整行。
        // 构造 1024B 整行：9 token × 113 + 8 空格 = 1025 超。
        // 113×9=1017+8=1025 超。112×9=1008+8=1016。加 8 → 不行。
        // 用「1 token 1024B」？token 界 128 先拒——语义冲突。
        // 正确构造：多 token 合计恰 1024：t=127×8=1016 + 7 空格=1023；
        // 改 127×7+126+7空格 = 889+126+7=1022。127×7+127+7=1023。
        // 127×8=1016，7 空格=1023，差 1 → 用 1 token 128B 换：127×7=889+128+8空格=1025。
        // 直接：token 各 120 × 8 = 960 + 7 空格 = 967；不凑。
        // 用 4 token：255 超。3 token 340 超。2 token 511/512 超 token 界。
        // 结论：token 界 128 下整行最大 = 8×128+7 = 1031 可超行界——
        // 恰 1024 的行必存在：8×127+7空格+1？= 1023+1 = 1024 需要 8 token
        // 中有一个 128：7×127+128+7 = 889+128+7 = 1024 ✓
        let mut exact = String::new();
        for i in 0..8 {
            if i > 0 {
                exact.push(' ');
            }
            exact.push_str(&tok(if i == 7 { 128 } else { 127 }));
        }
        set.add("构造·恰 1024B", exact.len() == 1024, "exact");
        let v4 = g.guard(&exact);
        set.add("过界2·行恰1024B 收", v4.line_ok, "1024==CAP");
        set.add("过界2·token 全收", v4.rejected_n == 0, "no reject");

        set.add("全账健康", g.all_within(), "clean");
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：引号感知分词（引号内空格不分词）+ 失衡前移拒收。
    #[test]
    fn quote_aware_tokenization() {
        let mut set = CheckSet::new("unxstar-unx0589");
        let mut g = LineGuard::new();

        // 引号内空格：`note="a b c" tail` → 2 token。
        match tokenize_lenient("note=\"a b c\" tail") {
            TokenizeResult::Ok(toks) => {
                set.add("分词·2 token", toks.len() == 2, "2");
                set.add(
                    "分词·引号内空格保留",
                    toks[0] == "note=\"a b c\"",
                    "quoted",
                );
            }
            _ => set.add("分词·2 token", false, "误失衡"),
        }
        // 转义空格不分词。
        match tokenize_lenient(r"a\ b c") {
            TokenizeResult::Ok(toks) => {
                set.add("分词·转义空格保留", toks.len() == 2, r"a\ b|c");
            }
            _ => set.add("分词·转义空格保留", false, "误失衡"),
        }
        // 失衡：开引号未闭合 → 分词即拒（F0588 语义前移）。
        match tokenize_lenient("a \"unclosed") {
            TokenizeResult::Unbalanced { .. } => {
                set.add("分词·失衡前移拒", true, "E_UNBALANCED");
            }
            _ => set.add("分词·失衡前移拒", false, "漏检"),
        }
        // 守卫路径的失衡诊断。
        let v = g.guard("a 'unclosed");
        set.add(
            "守卫·失衡诊断码",
            v.err == Some(LenError::Unbalanced),
            "E_UNBALANCED",
        );
        // 空行计数（不算错）。
        let ev = g.guard("   ");
        set.add("空行·零 token 不算错", ev.line_ok && ev.tokens.is_empty(), "empty");
        set.add("空行·计数入账", g.empty_lines == 1, "count=1");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：长度分布统计进料 + 指纹稳定。
    #[test]
    fn stats_and_fingerprint() {
        let mut set = CheckSet::new("unxstar-unx0589");
        let mut g = LineGuard::new();
        let _ = g.guard(&tok(3));
        let _ = g.guard(&tok(200)); // 越界（仍入 token 账——拒收也统计）
        let _ = g.guard(&tok(50));

        let (min, max, sum, n) = g.len_stats();
        set.add("统计·min=3", min == 3, "min");
        set.add("统计·max=200", max == 200, "max");
        set.add("统计·sum=253", sum == 253, "sum");
        set.add("统计·n=3", n == 3, "n");
        set.add("健康·有越界不空绿", !g.all_within(), "over>0");

        let f1 = g.fingerprint();
        let f2 = g.fingerprint();
        set.add("指纹·稳定", f1 == f2, "stable");
        set.add("指纹·非零", f1 != 0, "nonzero");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
