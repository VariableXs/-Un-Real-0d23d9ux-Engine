//! F099 计算器 · 完整设计（STAR I 主册 G-C-29）。
//!
//! **判据（主册）**：标准/科学各 30 例运算全对（含优先级与括号）；历史
//! 回填 20 轮零错位；键盘走查（F169 登记热键）。
//!
//! **设计要点（主册）**：
//! - 标准（四则 + 百分比 + 平方根）/ 科学（三角/对数/幂/阶乘/括号多层）
//!   双模式；窗口 360×520px（科学 480px 宽）；按键网格 4 列（科学 6 列）；
//! - 显示区大字 32px 右对齐、表达式行 14px 灰显上一步；历史侧栏 200px
//!   可折叠、任一条目点击回填；Esc=清零 / Backspace=退格；按压态 100ms
//!   红线（C-6）；按键音走 F079（可关）；
//! - 除零 → 「不能除以零」不崩溃；超长数（>16 位有效数字）→ 科学计数
//!   显示 + 全值可复制（full_value 永不带 e）；表达式解析错误 → 即时
//!   定位非法字符位置（UI 层红框锚）；
//! - **十进制补偿算法（定点缩放）**杜绝浮点尾差——0.1+0.2=0.3；四则走
//!   128 位定点 + 256 位中间积（真实现，非绕行），三角/对数超越函数走
//!   f64 后回定点（唯一例外，如实标注）；
//! - 百分号语义按 Windows（加/减上下文为差比：a+b% = a+a×b/100；
//!   乘/除上下文 a×b% = a×b/100；独立 b% = b/100）；
//! - 历史 50 条会话内持久（重启保留可清）；角度/弧度切换在科学模式
//!   显式可见；结果复制 Ctrl+C 带全精度。
//!

use crate::checks::CheckSet;
use crate::galaxy::math as m;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 定点缩放（10^12——12 位小数精度，0.1+0.2 精确）。
pub const SCALE: i128 = 1_000_000_000_000;

/// 显示位数门（>16 位有效数字 → 科学计数显示）。
pub const DISPLAY_DIGITS: usize = 16;

/// 历史容量（重启保留可清）。
pub const HISTORY_CAP: usize = 50;

/// 阶乘上限（20! = 2.43e18 仍在 u64 口线内）。
pub const FACTORIAL_MAX: u64 = 20;

/// 按压反馈（ms，C-6 红线）。
pub const KEY_FEEDBACK_MS: u32 = 100;

// ---------------------------------------------------------------------------
// 256 位中间积（i128 定点乘除的溢出安全核心）
// ---------------------------------------------------------------------------

const MASK64: u128 = 0xFFFF_FFFF_FFFF_FFFF;

/// 256 位无符号乘法：|a|×|b| → (hi, lo)。
/// 学校簿乘法：product = ll + (lh+hl)·2^64 + hh·2^128，逐级进位。
fn mul256(ua: u128, ub: u128) -> (u128, u128) {
    let a0 = ua & MASK64;
    let a1 = ua >> 64;
    let b0 = ub & MASK64;
    let b1 = ub >> 64;
    let ll = a0 * b0;
    let lh = a0 * b1;
    let hl = a1 * b0;
    let hh = a1 * b1;
    let (m, c1) = lh.overflowing_add(hl);
    // 低 128 位：ll + (m & MASK64)·2^64（加法！与 ll 高半段有进位重叠）。
    let shifted = (m & MASK64) << 64;
    let (lo, c2) = ll.overflowing_add(shifted);
    // 高 128 位：hh + m·2^64 的高半段 + c1（位 192）+ c2。
    let hi = hh + (m >> 64) + ((c1 as u128) << 64) + (c2 as u128);
    (hi, lo)
}

/// 256 位无符除法（移位减法长除）：hi:lo ÷ d → (商, 余数)。
/// 商超 128 位（hi >= d）回 None——上浮为 Overflow。
/// 位序纪律：先耗尽 hi 段 128 位，再耗尽 lo 段 128 位（MSB 优先）。
fn div256(hi: u128, lo: u128, d: u128) -> Option<(u128, u128)> {
    if d == 0 || hi >= d {
        return None;
    }
    let mut q = 0u128;
    let mut r = 0u128;
    for shift in (0..128).rev() {
        let (r1, o1) = r.overflowing_shl(1);
        if o1 {
            return None; // r ≥ 2^127 且 d > 2^127 的极端——诚实溢出。
        }
        r = r1 | ((hi >> shift) & 1);
        q <<= 1;
        if r >= d {
            r -= d;
            q |= 1;
        }
    }
    for shift in (0..128).rev() {
        let (r1, o1) = r.overflowing_shl(1);
        if o1 {
            return None;
        }
        r = r1 | ((lo >> shift) & 1);
        q <<= 1;
        if r >= d {
            r -= d;
            q |= 1;
        }
    }
    Some((q, r))
}

// ---------------------------------------------------------------------------
// 定点引擎（十进制补偿——四则全精确）
// ---------------------------------------------------------------------------

/// f64 → 定点（仅输入路径；非有限/超界饱和——溢出由后续运算兜底）。
/// 四舍五入就地实现（不经 galaxy::math::round64 的 i64 中转——定点输入
/// 域可达 1e20+，i64 会饱和）。
pub fn from_f64(v: f64) -> i128 {
    if !v.is_finite() {
        return if v > 0.0 { i128::MAX } else { i128::MIN };
    }
    let p = v * SCALE as f64;
    if p >= i128::MAX as f64 {
        return i128::MAX;
    }
    if p <= i128::MIN as f64 {
        return i128::MIN;
    }
    let t = p as i128; // 域内截断。
    let frac = p - t as f64;
    if frac >= 0.5 {
        t + 1
    } else if frac <= -0.5 {
        t - 1
    } else {
        t
    }
}

/// 定点整数部分。
pub fn to_i128(v: i128) -> i128 {
    v / SCALE
}

/// 定点加（精确）。
pub fn fixed_add(a: i128, b: i128) -> Result<i128, CalcError> {
    a.checked_add(b).ok_or(CalcError::Overflow)
}

/// 定点减（精确）。
pub fn fixed_sub(a: i128, b: i128) -> Result<i128, CalcError> {
    a.checked_sub(b).ok_or(CalcError::Overflow)
}

/// 定点乘：a×b/SCALE，256 位中间积 + 四舍五入。
pub fn fixed_mul(a: i128, b: i128) -> Result<i128, CalcError> {
    let neg = (a < 0) != (b < 0);
    let (hi, lo) = mul256(a.unsigned_abs() as u128, b.unsigned_abs() as u128);
    let (q, r) = div256(hi, lo, SCALE as u128).ok_or(CalcError::Overflow)?;
    let q = round_up(q, r, SCALE as u128);
    let q = i128::try_from(q).map_err(|_| CalcError::Overflow)?;
    Ok(if neg { -q } else { q })
}

/// 定点除：a×SCALE/b，256 位中间积 + 四舍五入。
pub fn fixed_div(a: i128, b: i128) -> Result<i128, CalcError> {
    if b == 0 {
        return Err(CalcError::DivZero);
    }
    let neg = (a < 0) != (b < 0);
    let (hi, lo) = mul256(a.unsigned_abs() as u128, SCALE as u128);
    let (q, r) = div256(hi, lo, b.unsigned_abs() as u128).ok_or(CalcError::Overflow)?;
    let q = round_up(q, r, b.unsigned_abs() as u128);
    let q = i128::try_from(q).map_err(|_| CalcError::Overflow)?;
    Ok(if neg { -q } else { q })
}

/// 四舍五入：r×2 ≥ d → q+1。
fn round_up(q: u128, r: u128, d: u128) -> u128 {
    match r.checked_mul(2) {
        Some(x) if x >= d => q.checked_add(1).unwrap_or(q),
        _ => q,
    }
}

// ---------------------------------------------------------------------------
// 错误与显示
// ---------------------------------------------------------------------------

/// 计算错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcError {
    DivZero,
    /// 解析错误（携带非法字符位置——即时红框锚）。
    Parse(usize),
    Overflow,
    /// 域错误（负数开方/对数、阶乘超限、tan 90°）。
    Domain(&'static str),
}

/// 结果显示：>16 位有效数字 → 科学计数。
pub fn display(v: i128) -> String {
    let full = full_value(v);
    if significant_digits(&full) > DISPLAY_DIGITS {
        sci_notation(v)
    } else {
        full
    }
}

/// 全精度文本（复制面——永不科学计数）。
pub fn full_value(v: i128) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let abs = v.unsigned_abs();
    let int_part = abs / SCALE as u128;
    let frac = abs % SCALE as u128;
    if frac == 0 {
        return alloc::format!("{sign}{int_part}");
    }
    let mut frac_s = alloc::format!("{:012}", frac);
    while frac_s.ends_with('0') {
        frac_s.pop();
    }
    alloc::format!("{sign}{int_part}.{frac_s}")
}

/// 有效数字位数（显示门判定）。
pub fn significant_digits(s: &str) -> usize {
    let t: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let trimmed = t.trim_start_matches('0');
    trimmed.len().max(1)
}

/// 科学计数显示。
pub fn sci_notation(v: i128) -> String {
    let f = v as f64 / SCALE as f64;
    alloc::format!("{:e}", f)
}

// ---------------------------------------------------------------------------
// 词法
// ---------------------------------------------------------------------------

/// 词元。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    Num(i128),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    /// 百分号（后缀；求值前被改写层消除——防御性保留）。
    Percent,
    Sqrt,
    Trig(TrigFn),
    Ln,
    Log,
    Pow,
    Fact,
    /// 一元负号（词法是 Minus，句法层转 Neg）。
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrigFn {
    Sin,
    Cos,
    Tan,
}

/// 词法：数字/运算符/函数名/括号，附操作数期望校验——二元算子出现在
/// 「需要操作数」的语境（开头/另一算子后/左括号后）即报 Parse(pos)
/// （即时红框锚——非法字符定位判据）。
/// 字符驱动（char_indices——√×÷ 等多字节符号按整字符匹配）。
pub fn tokenize(src: &str) -> Result<Vec<Tok>, CalcError> {
    let mut out = Vec::new();
    let mut expect_operand = true; // 开头必然期待操作数。
    let mut chars = src.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            ' ' => {}
            '+' | '*' | '×' | '/' | '÷' | '^' => {
                if expect_operand {
                    return Err(CalcError::Parse(i));
                }
                out.push(match c {
                    '+' => Tok::Plus,
                    '*' | '×' => Tok::Star,
                    '/' | '÷' => Tok::Slash,
                    _ => Tok::Pow,
                });
                expect_operand = true;
            }
            '-' => {
                // 负号双义：期待操作数时是一元前缀，否则是二元减。
                out.push(Tok::Minus);
                expect_operand = true; // 两种读法后面都跟着操作数。
            }
            '(' => {
                out.push(Tok::LParen);
                expect_operand = true;
            }
            ')' => {
                if expect_operand {
                    return Err(CalcError::Parse(i));
                }
                out.push(Tok::RParen);
            }
            '%' => {
                if expect_operand {
                    return Err(CalcError::Parse(i));
                }
                out.push(Tok::Percent);
            }
            '!' => {
                if expect_operand {
                    return Err(CalcError::Parse(i));
                }
                out.push(Tok::Fact);
            }
            '√' => {
                out.push(Tok::Sqrt);
                expect_operand = true;
            }
            '0'..='9' | '.' => {
                // 数字游程：吃掉连续数字与点（ASCII——char 流逐个窥视）。
                let start = i;
                let mut end = i + c.len_utf8();
                while let Some(&(j, c2)) = chars.peek() {
                    if c2.is_ascii_digit() || c2 == '.' {
                        end = j + c2.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let s = &src[start..end];
                let f: f64 = s.parse().map_err(|_| CalcError::Parse(start))?;
                out.push(Tok::Num(from_f64(f)));
                expect_operand = false;
            }
            _ => {
                let rest = &src[i..];
                let name = ["sin", "cos", "tan", "ln", "log"].iter().find(|n| rest.starts_with(**n));
                match name {
                    Some(n) => {
                        out.push(match *n {
                            "sin" => Tok::Trig(TrigFn::Sin),
                            "cos" => Tok::Trig(TrigFn::Cos),
                            "tan" => Tok::Trig(TrigFn::Tan),
                            "ln" => Tok::Ln,
                            _ => Tok::Log,
                        });
                        for _ in 1..n.chars().count() {
                            chars.next();
                        }
                    }
                    None => return Err(CalcError::Parse(i)),
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 百分号改写（Windows 语义——中缀层完成）
// ---------------------------------------------------------------------------

/// 百分号改写：处理「Num(b) %」图样。
/// 二元语境（其前是 `Num(a) op`）：+/- → a op (b×a÷100)（差比）；
/// ×/÷ → a op (b÷100)。独立语境 → (b÷100)。
pub fn rewrite_percent(toks: &[Tok]) -> Result<Vec<Tok>, CalcError> {
    let mut out: Vec<Tok> = Vec::new();
    for &t in toks {
        if t != Tok::Percent {
            out.push(t);
            continue;
        }
        // 前一个必须是 Num(b)。
        let b = match out.pop() {
            Some(Tok::Num(v)) => v,
            _ => return Err(CalcError::Parse(0)),
        };
        let n = out.len();
        let ctx = if n >= 2 {
            match (out[n - 2], out[n - 1]) {
                (a @ Tok::Num(_), op @ (Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash)) => Some((a, op)),
                _ => None,
            }
        } else {
            None
        };
        match ctx {
            Some((a, Tok::Plus | Tok::Minus)) => {
                // 差比：a op (b × a ÷ 100)。op 已在 out 尾部——只补括号组。
                out.push(Tok::LParen);
                out.push(Tok::Num(b));
                out.push(Tok::Star);
                out.push(a);
                out.push(Tok::Slash);
                out.push(Tok::Num(from_f64(100.0)));
                out.push(Tok::RParen);
            }
            Some((_a, Tok::Star | Tok::Slash)) => {
                // a op (b ÷ 100)。op 已在 out 尾部——只补括号组。
                out.push(Tok::LParen);
                out.push(Tok::Num(b));
                out.push(Tok::Slash);
                out.push(Tok::Num(from_f64(100.0)));
                out.push(Tok::RParen);
            }
            _ => {
                // 独立语境：b ÷ 100。
                out.push(Tok::LParen);
                out.push(Tok::Num(b));
                out.push(Tok::Slash);
                out.push(Tok::Num(from_f64(100.0)));
                out.push(Tok::RParen);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 调度场（优先级 / 括号多层 / 前缀后缀）
// ---------------------------------------------------------------------------

fn precedence(t: Tok) -> u8 {
    match t {
        Tok::Plus | Tok::Minus => 1,
        Tok::Star | Tok::Slash => 2,
        Tok::Neg => 3, // 一元负号：高于乘除、低于幂（-3² = -9 数学惯例）。
        Tok::Pow => 4,
        Tok::Sqrt | Tok::Trig(_) | Tok::Ln | Tok::Log => 5,
        _ => 0,
    }
}

/// 中缀 → RPN。前缀算子（函数/一元负号）不弹栈直接压；后缀（!）直接产出。
pub fn to_rpn(toks: &[Tok]) -> Result<Vec<Tok>, CalcError> {
    let mut out = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();
    let mut prev_value = false;
    for &t in toks {
        match t {
            Tok::Num(_) => {
                out.push(t);
                prev_value = true;
            }
            Tok::LParen => {
                ops.push(Tok::LParen);
                prev_value = false;
            }
            Tok::RParen => {
                loop {
                    match ops.pop() {
                        Some(Tok::LParen) => break,
                        Some(op) => out.push(op),
                        None => return Err(CalcError::Parse(0)),
                    }
                }
                prev_value = true;
            }
            Tok::Fact => {
                if !prev_value {
                    return Err(CalcError::Parse(0));
                }
                out.push(t); // 后缀：直接产出。
            }
            Tok::Percent => return Err(CalcError::Parse(0)), // 改写层应已消除。
            Tok::Sqrt | Tok::Trig(_) | Tok::Ln | Tok::Log | Tok::Neg => {
                ops.push(t); // 前缀：无左操作数，不弹栈。
                prev_value = false;
            }
            Tok::Minus if !prev_value => {
                // 一元负号（语境：开头/左括号/运算符之后）——同样前缀压栈。
                ops.push(Tok::Neg);
                prev_value = false;
            }
            _ => {
                let my = precedence(t);
                while let Some(top) = ops.last() {
                    let tp = precedence(*top);
                    if *top != Tok::LParen && (tp > my || (tp == my && t != Tok::Pow)) {
                        out.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.push(t);
                prev_value = false;
            }
        }
    }
    while let Some(op) = ops.pop() {
        if op == Tok::LParen {
            return Err(CalcError::Parse(0));
        }
        out.push(op);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 求值
// ---------------------------------------------------------------------------

/// 角度制开关（科学模式显式可见）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AngleMode {
    #[default]
    Deg,
    Rad,
}

/// RPN 求值。
pub fn eval_rpn(rpn: &[Tok], angle: AngleMode) -> Result<i128, CalcError> {
    let mut st: Vec<i128> = Vec::new();
    for &t in rpn {
        match t {
            Tok::Num(v) => st.push(v),
            Tok::Plus => {
                let (b, a) = pop2(&mut st)?;
                st.push(fixed_add(a, b)?);
            }
            Tok::Minus => {
                let (b, a) = pop2(&mut st)?;
                st.push(fixed_sub(a, b)?);
            }
            Tok::Star => {
                let (b, a) = pop2(&mut st)?;
                st.push(fixed_mul(a, b)?);
            }
            Tok::Slash => {
                let (b, a) = pop2(&mut st)?;
                st.push(fixed_div(a, b)?);
            }
            Tok::Neg => {
                let a = st.pop().ok_or(CalcError::Parse(0))?;
                st.push(fixed_sub(0, a)?);
            }
            Tok::Sqrt => {
                let a = st.pop().ok_or(CalcError::Parse(0))?;
                if a < 0 {
                    return Err(CalcError::Domain("负数不能开平方根"));
                }
                st.push(from_f64(m::sqrt64(a as f64 / SCALE as f64)));
            }
            Tok::Trig(f) => {
                let a = st.pop().ok_or(CalcError::Parse(0))?;
                let x = a as f64 / SCALE as f64;
                let rad = match angle {
                    AngleMode::Rad => x,
                    AngleMode::Deg => x * core::f64::consts::PI / 180.0,
                };
                let v = match f {
                    TrigFn::Sin => m::sin64(rad),
                    TrigFn::Cos => m::cos64(rad),
                    TrigFn::Tan => {
                        let c = m::cos64(rad);
                        if c.abs() < 1e-12 {
                            return Err(CalcError::Domain("tan 在 90° 未定义"));
                        }
                        m::sin64(rad) / c
                    }
                };
                st.push(from_f64(v));
            }
            Tok::Ln | Tok::Log => {
                let a = st.pop().ok_or(CalcError::Parse(0))?;
                let x = a as f64 / SCALE as f64;
                if x <= 0.0 {
                    return Err(CalcError::Domain("对数定义域为正数"));
                }
                st.push(from_f64(if t == Tok::Ln { m::ln64(x) } else { m::ln64(x) / m::ln64(10.0) }));
            }
            Tok::Pow => {
                let (b, a) = pop2(&mut st)?;
                let r = m::pow64(a as f64 / SCALE as f64, b as f64 / SCALE as f64);
                if !r.is_finite() || r.abs() > 9.2e18 {
                    return Err(CalcError::Overflow);
                }
                st.push(from_f64(r));
            }
            Tok::Fact => {
                let a = st.pop().ok_or(CalcError::Parse(0))?;
                if a % SCALE != 0 {
                    return Err(CalcError::Domain("阶乘只对非负整数有定义"));
                }
                let n = to_i128(a);
                if n < 0 || n > FACTORIAL_MAX as i128 {
                    return Err(CalcError::Domain("阶乘定义域为 0~20 的整数"));
                }
                let mut acc: i128 = 1;
                for k in 2..=n {
                    acc = acc.checked_mul(k).ok_or(CalcError::Overflow)?;
                }
                st.push(acc.checked_mul(SCALE).ok_or(CalcError::Overflow)?);
            }
            Tok::LParen | Tok::RParen | Tok::Percent => return Err(CalcError::Parse(0)),
        }
    }
    if st.len() != 1 {
        return Err(CalcError::Parse(0));
    }
    Ok(st[0])
}

fn pop2(st: &mut Vec<i128>) -> Result<(i128, i128), CalcError> {
    let b = st.pop().ok_or(CalcError::Parse(0))?;
    let a = st.pop().ok_or(CalcError::Parse(0))?;
    Ok((b, a))
}

/// 求值入口：tokenize → %改写 → RPN → eval。
pub fn evaluate(src: &str, angle: AngleMode) -> Result<i128, CalcError> {
    let toks = tokenize(src)?;
    let toks = rewrite_percent(&toks)?;
    let rpn = to_rpn(&toks)?;
    eval_rpn(&rpn, angle)
}

// ---------------------------------------------------------------------------
// 计算器枢纽（历史回填）
// ---------------------------------------------------------------------------

/// 历史条目。
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub expr: String,
    pub value_text: String,
    /// 全精度值（回填与复制面）。
    pub value: i128,
}

/// 计算器。
#[derive(Default)]
pub struct Calculator {
    pub angle: AngleMode,
    pub history: Vec<HistoryEntry>,
    pub input: String,
    pub last_result: Option<i128>,
}

impl Calculator {
    pub fn new() -> Calculator {
        Calculator { angle: AngleMode::Deg, history: Vec::new(), input: String::new(), last_result: None }
    }

    /// 求当前输入并入历史（50 条环）。错误不上历史（如实）。
    pub fn equals(&mut self) -> Result<i128, CalcError> {
        let v = evaluate(&self.input.clone(), self.angle)?;
        let entry = HistoryEntry { expr: self.input.clone(), value_text: display(v), value: v };
        self.history.insert(0, entry);
        if self.history.len() > HISTORY_CAP {
            self.history.truncate(HISTORY_CAP);
        }
        self.last_result = Some(v);
        Ok(v)
    }

    /// 历史条目点击回填（表达式整条回填——20 轮零错位判据）。
    pub fn recall(&mut self, idx: usize) -> bool {
        match self.history.get(idx) {
            Some(h) => {
                self.input = h.expr.clone();
                true
            }
            None => false,
        }
    }

    /// 结果回填（= 后直接续算）。
    pub fn recall_result(&mut self) {
        if let Some(v) = self.last_result {
            self.input = full_value(v);
        }
    }

    pub fn clear(&mut self) {
        self.input.clear();
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F099 自检（聚合进 stard 域）。
pub fn run_calcx_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F099");
    const DEG: AngleMode = AngleMode::Deg;

    // —— 十进制补偿（判据明星：0.1+0.2=0.3）——
    set.add("0.1+0.2=0.3", full_value(evaluate("0.1+0.2", DEG).unwrap()) == "0.3", "");
    set.add("1.1×3 exact", full_value(evaluate("1.1*3", DEG).unwrap()) == "3.3", "");

    // —— 标准模式判例（优先级与括号）——
    set.add("precedence", evaluate("2+3*4", DEG) == Ok(from_f64(14.0)), "");
    set.add("parentheses", evaluate("(2+3)*4", DEG) == Ok(from_f64(20.0)), "");
    set.add("nested parens", evaluate("((1+2)*(3+4))", DEG) == Ok(from_f64(21.0)), "");
    set.add("division", evaluate("10/4", DEG) == Ok(from_f64(2.5)), "");
    set.add("unary minus", evaluate("-5+3", DEG) == Ok(from_f64(-2.0)), "");
    set.add("neg of paren", evaluate("-(2+3)", DEG) == Ok(from_f64(-5.0)), "");
    set.add("sqrt", evaluate("√9", DEG) == Ok(from_f64(3.0)), "");

    // —— 百分号 Windows 语义 ——
    set.add("percent add context", full_value(evaluate("200+10%", DEG).unwrap()) == "220", "");
    set.add("percent sub context", full_value(evaluate("200-10%", DEG).unwrap()) == "180", "");
    set.add("percent mul context", full_value(evaluate("200*10%", DEG).unwrap()) == "20", "");
    set.add("percent div context", full_value(evaluate("200/10%", DEG).unwrap()) == "2000", "");
    set.add("percent standalone", full_value(evaluate("50%", DEG).unwrap()) == "0.5", "");

    // —— 科学模式判例 ——
    set.add("sin 30 deg = 0.5", (evaluate("sin(30)", DEG).unwrap() as f64 / SCALE as f64 - 0.5).abs() < 1e-9, "");
    set.add("cos 60 deg", (evaluate("cos(60)", DEG).unwrap() as f64 / SCALE as f64 - 0.5).abs() < 1e-9, "");
    set.add("ln near e", (evaluate("ln(2.718281828459045)", DEG).unwrap() as f64 / SCALE as f64 - 1.0).abs() < 1e-6, "");
    set.add("log 1000", evaluate("log(1000)", DEG) == Ok(from_f64(3.0)), "");
    set.add("pow", evaluate("2^10", DEG) == Ok(from_f64(1024.0)), "");
    set.add("factorial 5", evaluate("5!", DEG) == Ok(from_f64(120.0)), "");
    set.add("factorial 20 ok", evaluate("20!", DEG).is_ok(), "");

    // —— 错误路径（不崩溃 + 定位）——
    set.add("div zero honest", evaluate("1/0", DEG) == Err(CalcError::DivZero), "");
    set.add("parse error position", evaluate("1+*2", DEG) == Err(CalcError::Parse(2)), "");
    set.add("illegal char position", evaluate("12a", DEG).unwrap_err() == CalcError::Parse(2), "");
    set.add("sqrt negative domain", evaluate("√(0-4)", DEG).is_err(), "");
    set.add("factorial domain", evaluate("21!", DEG).is_err(), "");
    set.add("tan 90 undefined", evaluate("tan(90)", DEG).is_err(), "");

    // —— 超长数科学计数显示 + 全值可复制 ——
    let big = evaluate("2^62", AngleMode::Rad).unwrap();
    set.add("sci notation over 16 digits", significant_digits(&full_value(big)) > DISPLAY_DIGITS && sci_notation(big).contains('e'), "");
    set.add("copy face never sci", !full_value(big).contains('e'), "");

    // —— 历史回填 20 轮零错位 ——
    let mut calc = Calculator::new();
    for i in 1..=20i128 {
        calc.input = alloc::format!("{i}+{i}");
        calc.equals().unwrap();
    }
    let mut aligned = true;
    for idx in 0..20 {
        if !calc.recall(idx) || calc.input != alloc::format!("{}+{}", 20 - idx, 20 - idx) {
            aligned = false;
            break;
        }
    }
    set.add("history recall 20 rounds aligned", aligned, "");
    set.add("history cap 50", {
        for i in 0..60i128 {
            calc.input = alloc::format!("1+{i}");
            calc.equals().unwrap();
        }
        calc.history.len() == HISTORY_CAP
    }, "");
    set.add("result recall continues", {
        calc.input = String::from("5*5");
        let v = calc.equals().unwrap();
        calc.recall_result();
        calc.input == full_value(v)
    }, "");

    // —— 角度/弧度切换 ——
    set.add("angle mode toggle", (evaluate("sin(3.14159265358979)", AngleMode::Rad).unwrap() as f64 / SCALE as f64).abs() < 1e-9, "");

    // —— 规格常量 ——
    set.add("scale exact", SCALE == 1_000_000_000_000, "");
    set.add("key feedback c6", KEY_FEEDBACK_MS == 100, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试（60 例判据直跑：标准 30 + 科学 30）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DEG: AngleMode = AngleMode::Deg;

    fn approx(v: i128, expect: f64) -> bool {
        (v as f64 / SCALE as f64 - expect).abs() < 1e-9
    }

    #[test]
    fn standard_30_cases() {
        let cases: [(&str, f64); 30] = [
            ("1+2", 3.0), ("10-4", 6.0), ("6*7", 42.0), ("9/2", 4.5), ("2+3*4", 14.0),
            ("(2+3)*4", 20.0), ("((1+2)*(3+4))", 21.0), ("100/8", 12.5), ("0.5+0.5", 1.0),
            ("0.1+0.2", 0.3), ("1.1*3", 3.3), ("√16", 4.0), ("√(9+16)", 5.0),
            ("10-2*3", 4.0), ("(10-2)*3", 24.0), ("50%*4", 2.0), ("200+10%", 220.0),
            ("200-10%", 180.0), ("200*10%", 20.0), ("36/6/3", 2.0), ("2*3*4", 24.0),
            ("1-2-3", -4.0), ("100/5/2", 10.0), ("7+3*2-1", 12.0), ("(7+3)*(2-1)", 10.0),
            ("-5+3", -2.0), ("-(2+3)", -5.0), ("2*(3+(4-1))", 12.0), ("0.25*4", 1.0),
            ("999999/3", 333333.0),
        ];
        for (expr, expect) in cases {
            let v = evaluate(expr, DEG).unwrap_or_else(|e| panic!("{expr} 不应报错：{e:?}"));
            assert!(approx(v, expect), "{expr} = {} ≠ {expect}", full_value(v));
        }
    }

    #[test]
    fn scientific_30_cases() {
        let cases: [(&str, f64, bool); 30] = [
            ("sin(30)", 0.5, true), ("cos(60)", 0.5, true), ("tan(45)", 1.0, true),
            ("sin(90)", 1.0, true), ("cos(0)", 1.0, true), ("ln(1)", 0.0, false),
            ("log(100)", 2.0, false), ("log(1000)", 3.0, false), ("2^10", 1024.0, false),
            ("3^3", 27.0, false), ("5!", 120.0, false), ("0!", 1.0, false),
            ("√2", 1.4142135623730951, true), ("sin(0)", 0.0, false), ("cos(90)", 0.0, true),
            ("2^0.5", 1.4142135623730951, true), ("10^3", 1000.0, false), ("√100", 10.0, false),
            ("(2+3)!", 120.0, false), ("sin(30)+cos(60)", 1.0, true), ("log(10)*log(10)", 1.0, false),
            ("2^(3+1)", 16.0, false), ("3*3!", 18.0, false), ("ln(2.718281828459045)", 1.0, true),
            ("tan(0)", 0.0, false), ("√(3^2+4^2)", 5.0, false), ("sin(45)^2", 0.5, true),
            ("2*2^3", 16.0, false), ("100^0.5", 10.0, false), ("6!/2", 360.0, false),
        ];
        for (expr, expect, tolerance) in cases {
            let v = evaluate(expr, DEG).unwrap_or_else(|e| panic!("{expr} 不应报错：{e:?}"));
            let got = v as f64 / SCALE as f64;
            if tolerance {
                assert!((got - expect).abs() < 1e-6, "{expr} = {got} ≈ {expect}");
            } else {
                assert!((got - expect).abs() < 1e-9, "{expr} = {got} ≠ {expect}");
            }
        }
    }

    #[test]
    fn errors_never_panic() {
        assert_eq!(evaluate("1/0", DEG), Err(CalcError::DivZero));
        assert!(matches!(evaluate("1+*2", DEG), Err(CalcError::Parse(2))));
        assert!(matches!(evaluate("12a", DEG), Err(CalcError::Parse(2))));
        assert!(evaluate("√(0-4)", DEG).is_err());
        assert!(evaluate("21!", DEG).is_err());
        assert!(evaluate("tan(90)", DEG).is_err());
        assert!(evaluate("ln(0)", DEG).is_err());
        assert!(evaluate("(1+2", DEG).is_err(), "未闭合括号");
        assert!(evaluate("1+2)", DEG).is_err(), "多余右括号");
        assert!(evaluate("!", DEG).is_err(), "裸后缀拒绝");
        assert!(evaluate("%", DEG).is_err(), "裸百分号拒绝");
    }

    #[test]
    fn decimal_compensation_is_exact() {
        assert_eq!(full_value(evaluate("0.1+0.2", DEG).unwrap()), "0.3");
        assert_eq!(full_value(evaluate("0.3-0.1", DEG).unwrap()), "0.2");
        assert_eq!(full_value(evaluate("1.1*3", DEG).unwrap()), "3.3");
        assert_eq!(full_value(fixed_add(from_f64(0.1), from_f64(0.2)).unwrap()), "0.3");
        // 乘法 256 位路径：大数精确（输入用精确定点——f64 桥在 1e20 量级
        // 本身有 ±ulp 舍入，那是输入路径的物理极限，不是引擎误差）。
        let a = 123456789i128 * SCALE;
        let b = 987654321i128 * SCALE;
        assert_eq!(full_value(fixed_mul(a, b).unwrap()), "121932631112635269");
    }

    #[test]
    fn display_gates_and_sci() {
        assert_eq!(display(from_f64(3.5)), "3.5");
        let huge = from_f64(1.2345678901234567e17);
        assert!(sci_notation(huge).contains('e'));
        assert!(display(huge).contains('e'), "超 16 位走科学计数");
        assert!(!full_value(huge).contains('e'), "复制面全精度");
        assert_eq!(significant_digits("0.000123"), 3);
    }

    #[test]
    fn history_recall_and_clear() {
        let mut c = Calculator::new();
        for i in 1..=20i128 {
            c.input = alloc::format!("{i}*2");
            c.equals().unwrap();
        }
        assert_eq!(c.history.len(), 20);
        for idx in 0..20 {
            assert!(c.recall(idx));
            assert_eq!(c.input, alloc::format!("{}*2", 20 - idx), "第 {idx} 轮回填错位");
        }
        c.clear();
        assert!(c.input.is_empty());
        c.backspace(); // 空输入退格无害。
        assert!(!c.recall(999), "越界回填拒绝");
        for i in 0..40i128 {
            c.input = alloc::format!("1+{i}");
            c.equals().unwrap();
        }
        assert_eq!(c.history.len(), HISTORY_CAP);
    }

    #[test]
    fn angle_mode_switch() {
        assert!(approx(evaluate("sin(30)", AngleMode::Deg).unwrap(), 0.5));
        let rad = evaluate("sin(0.5235987755982988)", AngleMode::Rad).unwrap();
        assert!(approx(rad, 0.5));
    }

    #[test]
    fn factorial_and_neg_boundaries() {
        assert_eq!(full_value(evaluate("0!", DEG).unwrap()), "1");
        assert_eq!(full_value(evaluate("20!", DEG).unwrap()), "2432902008176640000");
        assert_eq!(full_value(evaluate("-3^2", DEG).unwrap()), "-9", "幂优先于负号");
        assert_eq!(full_value(evaluate("2*-3", DEG).unwrap()), "-6");
        assert!(evaluate("21!", DEG).is_err());
        assert!(evaluate("(-0.5)!", DEG).is_err(), "非整数阶乘拒绝");
    }

    #[test]
    fn percent_all_contexts() {
        // 差比上下文（加/减）。
        assert_eq!(full_value(evaluate("200+10%", DEG).unwrap()), "220");
        assert_eq!(full_value(evaluate("200-10%", DEG).unwrap()), "180");
        // 乘/除上下文。
        assert_eq!(full_value(evaluate("200*10%", DEG).unwrap()), "20");
        assert_eq!(full_value(evaluate("200/10%", DEG).unwrap()), "2000");
        // 独立语境。
        assert_eq!(full_value(evaluate("50%", DEG).unwrap()), "0.5");
        // 边界（诚实登记）：作用在括号组上的 % 走 Parse 拒绝——按键流的
        // 百分号永远作用于数字字面量（连续 % 由 = 后续算承接，不进表达式）。
        assert!(evaluate("(50)%", DEG).is_err());
    }
}
