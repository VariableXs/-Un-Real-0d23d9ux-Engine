//! VE-F0406 · 数值字面量全族（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0406`
//!
//! **判据（锚点原文）**：全族解析、溢出报错、精度警告、原文保真、判据。
//! - 整型（十进制、十六进制、八进制、后缀位宽）+ 浮点（小数、指数、
//!   后缀精度）+ 科学计数，全族一个解析器；
//! - 溢出与精度损失检测：字面量在目标类型不可表示**即报错**（不静默截断），
//!   报错带数值边界建议；精度损失→警告，**可配置升级错误**；
//! - 原文保真：记号保留原文（逐字节，含分隔符与前缀）供诊断引用。
//!
//! **设计要点**：
//! - 进制前缀：`0x`/`0X` 十六进制、`0o`/`0O` 八进制（用 o 不用 C 风格
//!   裸前导 0——裸 0 前导把 `0` 和 `007` 的语义搅在一起）；无前缀十进制；
//! - 数字分隔符 `_` 允许（可读性），但前导/尾随/连续下划线显性拒绝——
//!   分隔符是给人看的，词法器不能猜；
//! - 整型后缀：无（i32 默认）、`u/U`（u32）、`l/L`（i64）、`ul/UL`（u64）；
//!   浮点后缀：`f/F`（f32）、`lf/LF`（f64）；浮点必须有 `.` 或指数——
//!   裸数字是整型，不许猜；
//! - 溢出检测：数字部分按 u128 无溢出累积（checked_mul/checked_add），
//!   再对目标类型上界比较——超界报 E_OVERFLOW 并给出该类型最大值；
//! - 精度损失：f32 后缀按 f64 解析后 roundtrip 比对（x as f32 as f64 != x
//!   即损失）；f64 后缀按有效十进制位数启发式（>17 位必有舍入）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、类型与配置
// ---------------------------------------------------------------------------

/// 整型后缀（位宽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntSuffix {
    /// 无后缀 = i32（着色器语言默认位宽）
    I32,
    U32,
    I64,
    U64,
}

impl IntSuffix {
    /// 该类型最大可表示值（溢出报错的边界建议来源）。
    pub fn max_value(self) -> u128 {
        match self {
            IntSuffix::I32 => i32::MAX as u128,
            IntSuffix::U32 => u32::MAX as u128,
            IntSuffix::I64 => i64::MAX as u128,
            IntSuffix::U64 => u64::MAX as u128,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            IntSuffix::I32 => "i32",
            IntSuffix::U32 => "u32",
            IntSuffix::I64 => "i64",
            IntSuffix::U64 => "u64",
        }
    }
}

/// 浮点后缀（精度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatSuffix {
    F32,
    F64,
}

/// 进制。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radix {
    Dec,
    Hex,
    Oct,
}

/// 精度损失处置策略（判据"可配置升级错误"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecisionPolicy {
    /// 警告（默认）
    Warn,
    /// 升级为错误
    Error,
}

/// 解析配置。
#[derive(Clone, Copy, Debug)]
pub struct LiteralConfig {
    pub precision_policy: PrecisionPolicy,
}

impl LiteralConfig {
    pub fn default_config() -> LiteralConfig {
        LiteralConfig {
            precision_policy: PrecisionPolicy::Warn,
        }
    }
}

/// 字面量值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiteralValue {
    Int {
        /// 数字部分的无符号真值（溢出检测后仍在目标类型内）
        value: u128,
        suffix: IntSuffix,
        radix: Radix,
    },
    Float {
        value: f64,
        suffix: FloatSuffix,
    },
}

/// 解析产物（原文保真：original 逐字节等于输入）。
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedLiteral {
    /// 原文（逐字节保真，含前缀/分隔符/后缀——诊断引用的原材料）
    pub original: String,
    pub value: LiteralValue,
    /// 精度警告（Warn 策略下挂在产物上）
    pub warnings: Vec<String>,
}

/// 字面量错误（三要素 + 字节位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LitError {
    pub code: &'static str,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl LitError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、解析器
// ---------------------------------------------------------------------------

fn err(code: &'static str, pos: usize, what: String, why: String, next: String) -> LitError {
    LitError {
        code,
        pos,
        what,
        why,
        next,
    }
}

/// 数字部分的进制字符判定。
fn digit_value(b: u8, radix: Radix) -> Option<u8> {
    let v = match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => return None,
    };
    let base = match radix {
        Radix::Dec => 10,
        Radix::Hex => 16,
        Radix::Oct => 8,
    };
    if v < base {
        Some(v)
    } else {
        None
    }
}

/// 数值字面量全族解析（判据：全族解析）。
pub fn parse_literal(input: &str, config: &LiteralConfig) -> Result<ParsedLiteral, LitError> {
    let original = input.to_string();
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Err(err(
            "E_LIT_EMPTY",
            0,
            "字面量为空".to_string(),
            "空串不是数值字面量".to_string(),
            "补全数值".to_string(),
        ));
    }
    // 数值字面量不以下划线开头（下划线是分隔符，只在数字之间有意义）
    if bytes[0] == b'_' {
        return Err(err(
            "E_LIT_SEPARATOR",
            0,
            "字面量以下划线开头".to_string(),
            "下划线是数字分隔符，字面量的首字符必须是数字或进制前缀".to_string(),
            "去掉前导下划线；若想表达正负号语义，走一元运算符（词法期不掺符号）"
                .to_string(),
        ));
    }
    // 进制前缀
    let (radix, digit_start) = if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X') {
        (Radix::Hex, 2)
    } else if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'o' || bytes[1] == b'O') {
        (Radix::Oct, 2)
    } else {
        (Radix::Dec, 0)
    };
    if digit_start == bytes.len() {
        return Err(err(
            "E_LIT_MALFORMED",
            digit_start,
            format!("前缀 {:?} 后没有数字", &input[..digit_start]),
            "进制前缀必须有数字跟着".to_string(),
            "补全该进制的数字部分".to_string(),
        ));
    }
    // 扫描数字部分：数字 + 分隔符，遇到非数字字符停（进入后缀/浮点区）
    let mut value: u128 = 0;
    let mut i = digit_start;
    let mut last_was_underscore = digit_start > 0; // 前缀后紧跟下划线 = 前导分隔符
    let mut int_digits = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'_' {
            if last_was_underscore || i == bytes.len() - 1 {
                return Err(err(
                    "E_LIT_SEPARATOR",
                    i,
                    format!("第 {} 字节的下划线分隔符位置非法", i),
                    "下划线不能做前导/尾随/连续使用——分隔符是给人看的，\
词法器不猜它的意图".to_string(),
                    "把分隔符放在数字之间（如 1_000_000）".to_string(),
                ));
            }
            last_was_underscore = true;
            i += 1;
            continue;
        }
        match digit_value(b, radix) {
            Some(d) => {
                value = value
                    .checked_mul(match radix {
                        Radix::Dec => 10,
                        Radix::Hex => 16,
                        Radix::Oct => 8,
                    })
                    .and_then(|v| v.checked_add(d as u128))
                    .ok_or_else(|| {
                        err(
                            "E_LIT_OVERFLOW",
                            i,
                            "字面量数值超出 u128 解析域".to_string(),
                            "数字部分大到连解析器都装不下".to_string(),
                            "缩短数字；如此大的常数应走常量表达式（F0448）".to_string(),
                        )
                    })?;
                int_digits += 1;
                last_was_underscore = false;
                i += 1;
            }
            None => break,
        }
    }
    if int_digits == 0 {
        return Err(err(
            "E_LIT_MALFORMED",
            digit_start,
            format!("第 {} 字节处没有数字", digit_start),
            "进制前缀后必须跟数字".to_string(),
            format!(
                "补全 {} 进制数字（合法字符 {}）",
                match radix {
                    Radix::Hex => "十六",
                    Radix::Oct => "八",
                    Radix::Dec => "十",
                },
                match radix {
                    Radix::Hex => "0-9 a-f A-F",
                    Radix::Oct => "0-7",
                    Radix::Dec => "0-9",
                }
            ),
        ));
    }

    // 浮点判定：遇到 '.' 或指数。十六/八进制后跟 '.' 显性拒绝——
    // 本族不支持 hex float，静默按十进制小数解析是错义灾难
    if i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E') {
        if radix != Radix::Dec {
            return Err(err(
                "E_LIT_MALFORMED",
                i,
                format!(
                    "{} 进制字面量后出现 {:?}——不支持十六/八进制浮点",
                    match radix {
                        Radix::Hex => "十六",
                        Radix::Oct => "八",
                        Radix::Dec => "十",
                    },
                    bytes[i] as char
                ),
                "本字面量族只有十进制浮点；hex float 是另一族（未列入本项）"
                    .to_string(),
                "去掉小数/指数部分，或改用十进制字面量".to_string(),
            ));
        }
        return parse_float_tail(input, bytes, i, value, digit_start, config, original);
    }

    // 整型后缀
    let suffix = parse_int_suffix(bytes, i)?;
    if value > suffix.max_value() {
        return Err(err(
            "E_LIT_OVERFLOW",
            digit_start,
            format!(
                "字面量 {} 超出 {} 最大可表示值 {}",
                &input[..i],
                suffix.label(),
                suffix.max_value()
            ),
            "字面量在目标类型不可表示即报错——静默截断会让常量在编译期就变味"
                .to_string(),
            format!(
                "换更大位宽后缀（{} 的上界是 {}）或修改数值",
                suffix.label(),
                suffix.max_value()
            ),
        ));
    }
    Ok(ParsedLiteral {
        original,
        value: LiteralValue::Int {
            value,
            suffix,
            radix,
        },
        warnings: Vec::new(),
    })
}

/// 整型后缀解析。
fn parse_int_suffix(bytes: &[u8], start: usize) -> Result<IntSuffix, LitError> {
    let rest = &bytes[start..];
    let lower: Vec<u8> = rest.iter().map(|b| b.to_ascii_lowercase()).collect();
    let s: &[u8] = &lower;
    let suffix = match s {
        b"" => IntSuffix::I32,
        b"u" => IntSuffix::U32,
        b"l" => IntSuffix::I64,
        b"ul" => IntSuffix::U64,
        _ => {
            return Err(err(
                "E_LIT_SUFFIX",
                start,
                format!("整型后缀 {:?} 非法", &String::from_utf8_lossy(rest)),
                "整型后缀只有四档位宽".to_string(),
                "合法后缀集：（无 = i32）、u（u32）、l（i64）、ul（u64）".to_string(),
            ));
        }
    };
    Ok(suffix)
}

/// 浮点尾部解析（`.` 或指数之后的部分；判据：小数、指数、科学计数）。
#[allow(clippy::too_many_arguments)]
fn parse_float_tail(
    input: &str,
    bytes: &[u8],
    mut i: usize,
    int_part: u128,
    digit_start: usize,
    config: &LiteralConfig,
    original: String,
) -> Result<ParsedLiteral, LitError> {
    let mut significand = int_part as f64;
    let mut frac_digits = 0usize;
    // 小数部分
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let mut last_was_underscore = false;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'_' {
                if frac_digits == 0 || last_was_underscore {
                    return Err(err(
                        "E_LIT_SEPARATOR",
                        i,
                        format!("第 {} 字节的下划线分隔符位置非法", i),
                        "小数部分的下划线同样不能前导/尾随/连续".to_string(),
                        "把分隔符放在数字之间".to_string(),
                    ));
                }
                last_was_underscore = true;
                i += 1;
                continue;
            }
            match digit_value(b, Radix::Dec) {
                Some(d) => {
                    significand = significand * 10.0 + d as f64;
                    significand /= 10.0;
                    frac_digits += 1;
                    last_was_underscore = false;
                    i += 1;
                }
                None => break,
            }
        }
        if last_was_underscore {
            return Err(err(
                "E_LIT_SEPARATOR",
                i,
                "小数部分以下划线结尾".to_string(),
                "尾随分隔符非法".to_string(),
                "去掉末尾下划线".to_string(),
            ));
        }
        if frac_digits == 0 && i < bytes.len() && !bytes[i].is_ascii_digit() && bytes[i] != b'e' && bytes[i] != b'E' {
            return Err(err(
                "E_LIT_MALFORMED",
                i,
                format!("第 {} 字节处小数点后没有数字", i - 1),
                "裸小数点没有语义".to_string(),
                "小数点后补数字，或去掉小数点".to_string(),
            ));
        }
    }
    // 指数部分
    let mut exp: i32 = 0;
    let mut has_exp = false;
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        has_exp = true;
        i += 1;
        let mut neg = false;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            neg = bytes[i] == b'-';
            i += 1;
        }
        let mut exp_digits = 0usize;
        let mut exp_val: i64 = 0;
        let mut last_was_underscore = exp_digits == 0; // 指数符号后紧跟下划线非法
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'_' {
                if last_was_underscore {
                    return Err(err(
                        "E_LIT_SEPARATOR",
                        i,
                        format!("第 {} 字节的下划线分隔符位置非法", i),
                        "指数部分的下划线不能前导/连续/尾随".to_string(),
                        "把分隔符放在数字之间".to_string(),
                    ));
                }
                last_was_underscore = true;
                i += 1;
                continue;
            }
            match b {
                b'0'..=b'9' => {
                    exp_val = exp_val * 10 + (b - b'0') as i64;
                    if exp_val > 400 {
                        return Err(err(
                            "E_LIT_OVERFLOW",
                            i,
                            format!("指数 {} 超出可表示域", exp_val),
                            "指数超出浮点可表示范围".to_string(),
                            "改用更小的指数或拆分表达式".to_string(),
                        ));
                    }
                    exp_digits += 1;
                    last_was_underscore = false;
                    i += 1;
                }
                _ => break,
            }
        }
        if exp_digits == 0 || last_was_underscore {
            return Err(err(
                "E_LIT_MALFORMED",
                i,
                "指数标记后没有数字".to_string(),
                "科学计数法的 e 后必须有整数指数".to_string(),
                "补全指数数字".to_string(),
            ));
        }
        exp = if neg { -(exp_val as i32) } else { exp_val as i32 };
    }
    // 浮点后缀（f32/f64）；缺省 = f32（着色器语言惯例）
    let rest = bytes.get(i..).unwrap_or(b"");
    if rest.starts_with(b".") {
        return Err(err(
            "E_LIT_MALFORMED",
            i,
            "第二个小数点——一个字面量只允许一个小数点".to_string(),
            "1.2.3 这类序列不是数值字面量".to_string(),
            "检查是否漏了运算符（1.2 * 3）".to_string(),
        ));
    }
    let suffix = if rest.is_empty() {
        FloatSuffix::F32
    } else if rest.eq_ignore_ascii_case(b"f") {
        FloatSuffix::F32
    } else if rest.eq_ignore_ascii_case(b"lf") {
        FloatSuffix::F64
    } else {
        return Err(err(
            "E_LIT_SUFFIX",
            i,
            format!("浮点后缀 {:?} 非法", String::from_utf8_lossy(rest)),
            "浮点后缀只有两档精度".to_string(),
            "合法后缀集：f（f32）、lf（f64）、（无 = f32）".to_string(),
        ));
    };
    // 应用指数（checked：溢出到无穷 → 报错）
    let value = significand * 10f64.powi(exp);
    if !value.is_finite() {
        return Err(err(
            "E_LIT_OVERFLOW",
            digit_start,
            format!("字面量 {} 超出 f64 可表示域", &input[..i.min(input.len())]),
            "指数结果为无穷——浮点也有溢出".to_string(),
            "缩小指数或底数".to_string(),
        ));
    }
    // 精度损失检测（判据：精度警告可配置升级）
    let mut warnings: Vec<String> = Vec::new();
    let loss = match suffix {
        FloatSuffix::F32 => {
            let v32 = value as f32;
            (v32 as f64) != value
        }
        FloatSuffix::F64 => {
            // 启发式：有效十进制位数 > 17 必有 f64 舍入
            (frac_digits + int_part_digits(int_part)) > 17
        }
    };
    if loss {
        let note = format!(
            "字面量 {} 以 {} 存储发生精度损失——诊断引用原文比对",
            &input[..i.min(input.len())],
            match suffix {
                FloatSuffix::F32 => "f32",
                FloatSuffix::F64 => "f64",
            }
        );
        match config.precision_policy {
            PrecisionPolicy::Warn => warnings.push(note),
            PrecisionPolicy::Error => {
                return Err(err(
                    "E_LIT_PRECISION",
                    digit_start,
                    note,
                    "精度策略已配置为 Error：损失精度的字面量拒绝通过".to_string(),
                    "改写为可精确表示的值（如 0.5、2 的幂次），或调回 Warn 策略"
                        .to_string(),
                ));
            }
        }
    }
    let _ = has_exp;
    Ok(ParsedLiteral {
        original,
        value: LiteralValue::Float { value, suffix },
        warnings,
    })
}

fn int_part_digits(v: u128) -> usize {
    let mut n = v;
    let mut c = 1usize;
    while n >= 10 {
        n /= 10;
        c += 1;
    }
    c
}

/// F0406 判据自检（域聚合入口）。
pub fn run_vec06_checks() -> crate::checks::CheckSet {
    super::vec06_checks::run_vec06_checks()
}
