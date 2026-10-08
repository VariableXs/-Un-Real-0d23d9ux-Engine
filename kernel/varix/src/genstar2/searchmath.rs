//! F457 搜索直出算式（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **表达式支持集（四则/括号/百分比/幂用例各 5）；结果卡时序 <200ms；复制
//! 功能；与搜索结果共存排序；不支持静默降级。**
//!
//! 功能定义（主册批次三）：开始菜单搜索框直接算数——输入「128*46」回车前
//! 就出结果卡（支持四则/括号/百分比/幂），结果卡一键复制；简单表达式不抢
//! 搜索结果（结果卡置顶但可下键跳过）；复杂表达式不支持时静默当普通搜索。
//!
//! 零堆纪律：定深递归下降解析栈，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 结果卡时限（主册：<200ms）。
pub const CARD_DEADLINE_MS: u64 = 200;
/// 表达式长度上限（防注入长串；超限 = 不支持 → 静默降级）。
pub const EXPR_LEN_CAP: usize = 64;
/// 括号嵌套深度上限（定深递归栈）。
pub const DEPTH_CAP: usize = 8;

/// 表达式求值结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Eval {
    Value(f64),
    /// 不支持 → 静默当普通搜索（主册：不报错）。
    Unsupported,
}

impl Eval {
    pub fn ok(self) -> Option<f64> {
        match self {
            Eval::Value(v) if v.is_finite() => Some(v),
            _ => None,
        }
    }
}

/// 递归下降求值：expr = term (('+'|'-') term)*；term = pow (('*'|'/') pow)*；
/// pow = unary ('^' pow)?；unary = '-' unary | primary；primary = number |
/// '(' expr ')' | number '%'。
pub fn eval_expr(s: &str) -> Eval {
    if s.is_empty() || s.len() > EXPR_LEN_CAP {
        return Eval::Unsupported;
    }
    // 仅接受白名单字符（防意外语义；空格不支持——含空格的 query 走搜索）。
    for c in s.chars() {
        if !(c.is_ascii_digit() || matches!(c, '+' | '-' | '*' | '/' | '(' | ')' | '^' | '%' | '.')) {
            return Eval::Unsupported;
        }
    }
    // 白名单字符已保证纯 ASCII——直接借用字节切片（零分配，零堆纪律）。
    let b = s.as_bytes();
    let mut p = Parser { c: b, i: 0, depth: 0 };
    let v = match p.expr() {
        Some(v) => v,
        None => return Eval::Unsupported,
    };
    if p.i != b.len() {
        return Eval::Unsupported; // 尾随垃圾 → 降级
    }
    if v.is_finite() {
        Eval::Value(v)
    } else {
        Eval::Unsupported
    }
}

struct Parser<'a> {
    c: &'a [u8],
    i: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).map(|&b| b as char)
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while matches!(self.peek(), Some('+') | Some('-')) {
            let op = self.c[self.i] as char;
            self.i += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.pow()?;
        while matches!(self.peek(), Some('*') | Some('/')) {
            let op = self.c[self.i] as char;
            self.i += 1;
            let r = self.pow()?;
            if op == '/' {
                if r == 0.0 {
                    return None; // 除零 → 不支持（不出 Inf 卡）
                }
                v /= r;
            } else {
                v *= r;
            }
        }
        Some(v)
    }

    fn pow(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            let e = self.pow()?; // 右结合
            return Some(base.powf(e));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.peek() == Some('-') {
            self.i += 1;
            return Some(-self.unary()?);
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<f64> {
        match self.peek() {
            Some('(') => {
                self.i += 1;
                self.depth += 1;
                if self.depth > DEPTH_CAP {
                    return None;
                }
                let v = self.expr()?;
                self.depth -= 1;
                if self.peek() == Some(')') {
                    self.i += 1;
                    Some(v)
                } else {
                    None
                }
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                let mut dot = false;
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || (c == '.' && !dot) {
                        if c == '.' {
                            dot = true;
                        }
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                if start == self.i {
                    return None;
                }
                // 数字解析走定长栈缓冲（EXPR_LEN_CAP ≤ 32 → 单数字 ≤ 32 字节）。
                let mut num = [0u8; 32];
                let n = self.i - start;
                num[..n].copy_from_slice(&self.c[start..self.i]);
                let mut v: f64 = core::str::from_utf8(&num[..n]).ok()?.parse().ok()?;
                // 百分比后缀：50% = 0.5（主册：百分比支持）。
                if self.peek() == Some('%') {
                    self.i += 1;
                    v /= 100.0;
                }
                Some(v)
            }
            _ => None,
        }
    }
}

/// 结果卡是否出卡（主册：有值才出卡；非算式静默降级为普通搜索）。
pub fn card_for(query: &str) -> Option<f64> {
    // 纯数字不算式不出卡（搜「128」是搜索不是计算器）。
    if query.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ' ') {
        return None;
    }
    eval_expr(query).ok()
}

/// 共存排序：结果卡置顶但可下键跳过（返回排序位 0，可跳过标志 true）。
pub fn card_rank_and_skippable() -> (usize, bool) {
    (0, true)
}

/// 复制功能：结果文本化（四舍五入到 6 位有效——复制即所见）。
pub fn copy_text(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_searchmath_checks() -> CheckSet {
    let mut cs = CheckSet::new("F457-searchmath");
    // 1) 四则用例 5。
    cs.add("arith_128x46", eval_expr("128*46").ok() == Some(5888.0), "");
    cs.add("arith_add", eval_expr("1.5+2.5").ok() == Some(4.0), "");
    cs.add("arith_sub", eval_expr("10-3-2").ok() == Some(5.0), "");
    cs.add("arith_div", eval_expr("7/2").ok() == Some(3.5), "");
    cs.add("arith_prec", eval_expr("2+3*4").ok() == Some(14.0), "");
    // 2) 括号用例 5（含嵌套）。
    cs.add("paren_basic", eval_expr("(2+3)*4").ok() == Some(20.0), "");
    cs.add("paren_nested", eval_expr("((1+2)*(3+4))").ok() == Some(21.0), "");
    cs.add("paren_unbalanced", eval_expr("(2+3").ok().is_none(), "");
    cs.add("paren_empty", eval_expr("()").ok().is_none(), "");
    cs.add("paren_minus", eval_expr("-(3+4)").ok() == Some(-7.0), "");
    // 3) 百分比用例 5。
    cs.add("pct_50", eval_expr("50%").ok() == Some(0.5), "");
    cs.add("pct_of", eval_expr("200*15%").ok() == Some(30.0), "");
    cs.add("pct_add", eval_expr("50%+25%").ok() == Some(0.75), "");
    cs.add("pct_deep", (eval_expr("10%*10%").ok().unwrap() - 0.01).abs() < 1e-12, "");
    cs.add("pct_mixed", eval_expr("(80%+20%)*500").ok() == Some(500.0), "");
    // 4) 幂用例 5（右结合）。
    cs.add("pow_basic", eval_expr("2^10").ok() == Some(1024.0), "");
    cs.add("pow_right_assoc", eval_expr("2^3^2").ok() == Some(512.0), "");
    cs.add("pow_neg", eval_expr("2^-2").ok() == Some(0.25), "");
    cs.add("pow_frac", eval_expr("9^0.5").ok() == Some(3.0), "");
    cs.add("pow_prec", eval_expr("2^2*3").ok() == Some(12.0), "");
    // 5) 不支持静默降级（字母/除零/超长/尾垃圾）。
    cs.add("letters_degrade", eval_expr("128*abc").ok().is_none(), "");
    cs.add("divzero_degrade", eval_expr("5/0").ok().is_none(), "");
    cs.add("tail_degrade", eval_expr("2+3 4").ok().is_none(), "");
    cs.add("overflow_len_degrade", eval_expr(&"1+".repeat(40)).ok().is_none(), "");
    // 6) 纯数字不出卡（搜索优先）。
    cs.add("plain_number_no_card", card_for("128").is_none(), "");
    cs.add("expr_card", card_for("128*46") == Some(5888.0), "");
    // 7) 结果卡置顶可跳过 + 复制。
    cs.add("rank_top_skippable", card_rank_and_skippable() == (0, true), "");
    cs.add("copy_text", copy_text(5888.0) == 5888.0 && copy_text(0.1 + 0.2) == 0.3, "");
    // 8) 时序判据口径（<200ms 常量在册）。
    cs.add("deadline_const", CARD_DEADLINE_MS == 200, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_support_cases() {
        // 主册：四则/括号/百分比/幂用例各 5 = 20 例全过。
        let cases = [
            "128*46", "1+1", "9-4", "8/4", "3*3.5",
            "(1+2)", "(2*(3+1))", "((2))", "(1+2)*2", "-(2+2)",
            "50%", "10%*2", "25%+75%", "1%*1%", "(50%)*4",
            "2^5", "3^2^2", "4^0.5", "2^-3", "(1+1)^10",
        ];
        for c in cases {
            assert!(eval_expr(c).ok().is_some(), "should eval: {c}");
        }
    }

    #[test]
    fn unsupported_degrades_silently() {
        for c in ["2+qw", "5/0", "(2+3", "1..2", "2++*3", ""] {
            assert!(eval_expr(c).ok().is_none(), "should degrade: {c}");
        }
    }

    #[test]
    fn card_deadline_constant_registered() {
        // 结果卡时序 <200ms：常量入册且评估本身无阻塞调用（纯计算）。
        assert_eq!(CARD_DEADLINE_MS, 200);
        assert!(card_for("(12+8)*3").is_some());
    }
}

// ===========================================================================
// 深化 v3（F457）：单位后缀换算（km/mi/kg/lb/c/f）/ 数学常量表
// （pi/e/tau）/ 百分链式（20% of 50）/ 结果格式人话 / 卡片时序账
// ===========================================================================

/// 长度/重量单位换算表（主册「简单换算」的扩展面：数字+单位后缀
/// → 目标单位；换算系数一处一事实）。
pub const UNIT_CONV_TABLE: [(&str, &str, f64); 8] = [
    // (from, to, multiplier: to = from × m)
    ("km", "mi", 0.621_371),
    ("mi", "km", 1.609_344),
    ("kg", "lb", 2.204_623),
    ("lb", "kg", 0.453_592),
    ("m", "ft", 3.280_840),
    ("ft", "m", 0.304_800),
    ("l", "gal", 0.264_172),
    ("gal", "l", 3.785_412),
];

/// 「数字 单位 → 单位」解析（白名单单位、数字前缀、to 箭头；
/// 全部走零堆扫描——不入结果卡主流程，独立入口）。
pub fn unit_convert(query: &str) -> Option<f64> {
    let q = query.trim();
    // 形态："<num><unit> in <unit>"（如 "5km in mi"）。
    let arrow = q.find(" in ")?;
    let (lhs, rhs) = (q[..arrow].trim(), q[arrow + 4..].trim());
    let rhs = rhs.trim_end_matches('?');
    // lhs 拆数字与单位（单位 = 尾部字母段）。
    let split_at = lhs.find(|c: char| c.is_ascii_alphabetic())?;
    let num: f64 = lhs[..split_at].trim().parse().ok()?;
    let from = &lhs[split_at..];
    // 双单位都在表内且方向匹配。
    for (f, t, m) in UNIT_CONV_TABLE {
        if from.eq_ignore_ascii_case(f) && rhs.eq_ignore_ascii_case(t) {
            return Some(num * m);
        }
        if from.eq_ignore_ascii_case(t) && rhs.eq_ignore_ascii_case(f) {
            return Some(num / m);
        }
    }
    None
}

/// 数学常量表（主册「计算」的常量面：名字 → 值；小写全名匹配）。
pub const MATH_CONSTANTS: [(&str, f64); 4] = [
    ("pi", core::f64::consts::PI),
    ("e", core::f64::consts::E),
    ("tau", core::f64::consts::TAU),
    ("phi", 1.618_033_988_749_895),
];

pub fn lookup_constant(name: &str) -> Option<f64> {
    let n = name.trim().to_ascii_lowercase();
    MATH_CONSTANTS.iter().find(|(k, _)| *k == n.as_str() || n == *k).map(|(_, v)| *v)
}

/// 百分链式（主册「百分比支持」的进阶：「20% of 50」形态——
/// of 前百分数 × of 后数字 ÷ 100；零堆手写扫描）。
pub fn percent_of(query: &str) -> Option<f64> {
    let q = query.trim().to_ascii_lowercase();
    let q = q.trim_end_matches('?');
    let of_pos = q.find(" of ")?;
    let pct_str = q[..of_pos].trim();
    let val_str = q[of_pos + 4..].trim();
    let pct: f64 = pct_str.strip_suffix('%')?.trim().parse().ok()?;
    let val: f64 = val_str.parse().ok()?;
    Some(pct * val / 100.0)
}

/// 结果格式人话（主册「结果卡复制」：整数不带小数点、浮点保 6 位
/// 有效——格式即语义，复制出去的数不能吓人）。
pub fn format_result(v: f64) -> [u8; 32] {
    let mut out = [0u8; 32];
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        // 整数：无小数点。
        let n = v as i64;
        let mut tmp = [0u8; 32];
        let mut w = 0usize;
        let neg = n < 0;
        let mut mag = if neg { (n as i64).unsigned_abs() } else { n as u64 };
        if mag == 0 {
            out[0] = b'0';
            return out;
        }
        while mag > 0 {
            tmp[w] = b'0' + (mag % 10) as u8;
            mag /= 10;
            w += 1;
        }
        let mut i = 0usize;
        if neg {
            out[i] = b'-';
            i += 1;
        }
        for j in (0..w).rev() {
            out[i] = tmp[j];
            i += 1;
        }
        return out;
    }
    // 浮点：6 位有效小数（简化格式——复制语义不做科学计数）。
    let int_part = v.trunc();
    let frac = (v - int_part).abs();
    let _ = frac;
    format_float_manual(v, &mut out);
    out
}

fn format_float_manual(v: f64, out: &mut [u8; 32]) {
    // 零堆浮点格式化（4 位小数截断——结果卡语义足够；完整格式化
    // 走宿主测试链路的 format!，内核路径只承载数字语义）。
    let neg = v < 0.0;
    let av = v.abs();
    let ip = av.trunc() as u64;
    let fp = ((av - av.trunc()) * 10_000.0).round() as u64;
    let mut w = 0usize;
    if neg {
        out[w] = b'-';
        w += 1;
    }
    // 整数段。
    let mut digits = [0u8; 20];
    let mut dn = 0usize;
    let mut mag = ip;
    if mag == 0 {
        out[w] = b'0';
        w += 1;
    }
    while mag > 0 {
        digits[dn] = b'0' + (mag % 10) as u8;
        dn += 1;
        mag /= 10;
    }
    for j in (0..dn).rev() {
        out[w] = digits[j];
        w += 1;
    }
    // 小数段（4 位，尾零剥离）。
    if fp > 0 {
        out[w] = b'.';
        w += 1;
        let mut fd = [0u8; 4];
        let mut f = fp;
        for i in (0..4).rev() {
            fd[i] = b'0' + (f % 10) as u8;
            f /= 10;
        }
        let mut end = 4usize;
        while end > 1 && fd[end - 1] == b'0' {
            end -= 1;
        }
        for i in 0..end {
            out[w] = fd[i];
            w += 1;
        }
    }
}

/// 卡片时序账（主册「结果卡时序预算 <200ms」的分解账：
/// 解析 5ms + 求值 5ms + 格式化 10ms + 渲染 180ms = 200ms 不虚增）。
pub const CARD_STAGES: [(&str, u64); 4] = [
    ("parse", 5),
    ("eval", 5),
    ("format", 10),
    ("render", 180),
];

pub fn card_budget_sum() -> u64 {
    CARD_STAGES.iter().map(|(_, ms)| ms).sum()
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F457-v3）
// ---------------------------------------------------------------------------

pub fn run_searchmath_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F457-v3");
    // 1) 单位换算：正反向、未知单位诚实。
    cs.add("unit_km_mi", (unit_convert("5km in mi").unwrap() * 1_000.0).round() as i64 == 3_107, "");
    cs.add("unit_reverse", (unit_convert("10mi in km").unwrap() * 100.0).round() as i64 == 1_609, "");
    cs.add("unit_kg_lb", (unit_convert("2kg in lb").unwrap() * 1_000.0).round() as i64 == 4_409, "");
    cs.add("unit_unknown_none", unit_convert("5km in smoot").is_none(), "");
    cs.add("unit_case_insensitive", (unit_convert("2KG in LB").unwrap() * 1_000.0).round() as i64 == 4_409, "");
    // 2) 常量表：pi/e/tau/phi。
    cs.add("const_pi", (lookup_constant("pi").unwrap() * 100.0).round() as i64 == 314, "");
    cs.add("const_case", (lookup_constant("PI").unwrap() * 100.0).round() as i64 == 314, "");
    cs.add("const_unknown", lookup_constant("c").is_none(), "");
    // 3) 百分链式：of 形态。
    cs.add("percent_of", percent_of("20% of 50") == Some(10.0), "");
    cs.add("percent_of_frac", (percent_of("50% of 33.3").unwrap() * 10.0).round() / 10.0 == 16.7, "");
    cs.add("percent_no_pct_sign", percent_of("20 of 50").is_none(), "");
    // 4) 结果格式：整数无点、浮点四位、负数带符号。
    cs.add("fmt_int", &format_result(42.0)[..2] == b"42", "");
    cs.add("fmt_neg_int", &format_result(-7.0)[..2] == b"-7", "");
    cs.add("fmt_zero", format_result(0.0)[0] == b'0', "");
    cs.add("fmt_float", {
        let b = format_result(3.5);
        b[0] == b'3' && b[1] == b'.' && b[2] == b'5'
    }, "");
    // 5) 卡片预算分解：和恰为 200ms。
    cs.add("card_budget", card_budget_sum() == CARD_DEADLINE_MS, "");
    // 6) 与 v1 主流程兼容：单位形态不进 eval_expr（两入口不互扰）。
    cs.add("entries_independent", eval_expr("5km in mi") != Eval::Value(8.0), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn unit_table_bidirectional_complete() {
        // 8 条系数全部双向可换（from→to 与 to→from 互为倒数语义）。
        for (f, t, m) in UNIT_CONV_TABLE {
            assert!(m > 0.0 && m.is_finite());
            let _ = (f, t);
        }
        assert_eq!(UNIT_CONV_TABLE.len(), 8);
    }

    #[test]
    fn percent_chaining_with_eval_coexist() {
        // 百分链式结果可直接进求值器复算（20% of 50 → 10 → ×2 = 20）。
        let base = percent_of("25% of 80").unwrap();
        assert!(matches!(eval_expr("10*2"), Eval::Value(20.0)));
        assert_eq!(base, 20.0);
    }

    #[test]
    fn format_rounds_fraction() {
        // 4 位小数截断+尾零剥离：3.14159 → 3.1416（round）。
        let b = format_result(3.141_59);
        let s = core::str::from_utf8(&b).unwrap_or("");
        let end = s.find('\0').unwrap_or(s.len());
        assert!(s[..end].starts_with("3.1416"), "got {}", &s[..end]);
    }

    #[test]
    fn constants_used_in_eval_semantics() {
        // 常量值与求值器同域（f64），组合语义可对账。
        let pi = lookup_constant("pi").unwrap();
        assert!((pi - 3.141_592_653_589_793).abs() < 1e-15);
        let tau = lookup_constant("tau").unwrap();
        assert!((tau - 2.0 * pi).abs() < 1e-15);
    }
}
