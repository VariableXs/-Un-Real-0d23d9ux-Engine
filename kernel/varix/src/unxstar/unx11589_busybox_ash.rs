//! UNX-F11589 busybox sh(ash) 方言基线判据
//!
//! busybox 的 `sh` 是 ash 方言，既不是 POSIX 的完整实现，也不是 bash 的子集。
//! 本模块把“能不能用”拆成一张**方言基线表**：二十五条特性，每条打三档标记——
//! `ST_OK`（ash 支持且语义与 POSIX 一致）、`ST_DIFF`（ash 支持但有差异）、
//! `ST_NO`（ash 不支持）。支持率按“支持即算”（OK + DIFF）统计，纯净率只算
//! OK；两个百分比都向下取整，且必须满足 支持率 > 纯净率。
//!
//! 基线表之上是四台**语法扫描器**，它们都不做完整解析，只回答“这段脚本在
//! 闸门处是否可放行”：
//!
//! 1. **参数展开分类器**：识别 `${x}`、`${x:-y}`、`${x:=y}`、`${x:?y}`、
//!    `${x:+y}`、`${x#p}`、`${x##p}`、`${x%s}`、`${x%%s}`、`${x/a/b}`、
//!    `${x:1:2}`、`${x^^}`、`${#x}` 十三种形态。分类靠逐字节扫描运算符，
//!    不依赖正则；`${}` 与未闭合的 `${` 都判参数形态非法。
//! 2. **引号扫描器**：单引号内一切原样、双引号内允许转义、反斜杠跳过下一字节，
//!    扫描结束仍有未闭合引号则判不配对；返回成对引号数。
//! 3. **算术扫描器**：`$((` 起算，内部括号可嵌套，深度回到零才算闭合；
//!    未闭合判算术不配对，返回算术段个数。
//! 4. **结构扫描器**：`case/esac` 按词计数，`esac` 多于 `case` 或收尾深度非零
//!    都判结构错误；花括号同样要求收支平衡。
//!
//! 方言闸门 `require(feature, dialect)` 把三档标记与两种方言交叉：
//!
//! - 严格 ash：`ST_NO` 拒绝（`EFEAT`），`ST_DIFF` 放行并回报差异档；
//! - bash 语义：`ST_NO` 与 `ST_DIFF` 都拒绝（`EDIAL`），因为调用方要求的是
//!   bash 行为，而 ash 要么没有、要么不同；
//! - POSIX 语义：只拒绝 `ST_NO`，`ST_DIFF` 放行（POSIX 允许实现差异）。
//!
//! `admit(script, dialect)` 把四台扫描器与特性闸门串成一条链：空脚本先拒，
//! 然后引号、算术、case、花括号依次过闸，最后逐个分类 `${...}` 并按方言
//! 检查特性状态。链的次序是硬约定——引号没配平时谈参数展开没有意义。
//!
//! 闸门次序：`classify`：`${` 前缀 → 收尾 `}` → 空内层 → 运算符扫描；
//! `require`：特性号 → 状态 → 方言；`admit`：空脚本 → 引号 → 算术 → case →
//! 花括号 → 参数展开 → 方言。
//!
//! 账本十四格：八格成功（三类展开、两档特性、支持率、case 计数、引号对数），
//! 六格失败（`${}`、引号未闭合、算术未闭合、case 未闭合、ash 请求不支持特性、
//! bash 请求差异特性）。冻结票号折叠五字段与拒绝计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa20;

/// 参数展开形态非法。
pub const ASH_EPARAM: u32 = SEG_BASE + 0x01;
/// 引号不配对。
pub const ASH_EQUOTE: u32 = SEG_BASE + 0x02;
/// 算术括号不配对。
pub const ASH_EARITH: u32 = SEG_BASE + 0x03;
/// `case/esac` 不配对。
pub const ASH_ECASE: u32 = SEG_BASE + 0x04;
/// `local` 出现在函数外。
pub const ASH_ELOCAL: u32 = SEG_BASE + 0x05;
/// 请求了 ash 不支持的基线特性。
pub const ASH_EFEAT: u32 = SEG_BASE + 0x06;
/// 方言差异未被容许。
pub const ASH_EDIAL: u32 = SEG_BASE + 0x07;
/// 闸门次序或嵌套结构错误。
pub const ASH_EORDER: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const ASH_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const ASH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const ASH_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const ASH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const ASH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 基线特性条数。
pub const FEATURES: usize = 25;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 状态：支持且一致。
pub const ST_OK: u8 = 0;
/// 状态：支持但有差异。
pub const ST_DIFF: u8 = 1;
/// 状态：不支持。
pub const ST_NO: u8 = 2;

/// 方言：busybox ash 严格模式。
pub const DIAL_ASH: u8 = 0;
/// 方言：bash 语义。
pub const DIAL_BASH: u8 = 1;
/// 方言：POSIX 语义。
pub const DIAL_POSIX: u8 = 2;

/// 形态：裸名 `${x}`。
pub const FM_NAME: u8 = 0;
/// 形态：默认值 `${x:-y}`。
pub const FM_DEFAULT: u8 = 1;
/// 形态：赋默认 `${x:=y}`。
pub const FM_ASSIGN: u8 = 2;
/// 形态：报错 `${x:?y}`。
pub const FM_ERROR: u8 = 3;
/// 形态：替代值 `${x:+y}`。
pub const FM_ALT: u8 = 4;
/// 形态：最短前缀裁剪 `${x#p}`。
pub const FM_TRIM_PRE: u8 = 5;
/// 形态：最长前缀裁剪 `${x##p}`。
pub const FM_TRIM_PRE_MAX: u8 = 6;
/// 形态：最短后缀裁剪 `${x%s}`。
pub const FM_TRIM_SUF: u8 = 7;
/// 形态：最长后缀裁剪 `${x%%s}`。
pub const FM_TRIM_SUF_MAX: u8 = 8;
/// 形态：模式替换 `${x/a/b}`。
pub const FM_REPLACE: u8 = 9;
/// 形态：子串 `${x:1:2}`。
pub const FM_SUBSTR: u8 = 10;
/// 形态：大小写变形 `${x^^}`。
pub const FM_CASE: u8 = 11;
/// 形态：长度 `${#x}`。
pub const FM_LEN: u8 = 12;
/// 术语：无名形态（`FM_NAME` 不映射基线表）。
pub const NO_FEATURE: u8 = 0xff;

/// 操作码：形态分类。
pub const OP_CLASSIFY: u8 = 1;
/// 操作码：特性闸门。
pub const OP_REQUIRE: u8 = 2;
/// 操作码：支持率。
pub const OP_RATIO: u8 = 3;
/// 操作码：case 计数。
pub const OP_CASES: u8 = 4;
/// 操作码：引号扫描。
pub const OP_QUOTES: u8 = 5;
/// 操作码：算术扫描。
pub const OP_ARITH: u8 = 6;
/// 操作码：花括号扫描。
pub const OP_BRACES: u8 = 7;
/// 操作码：整脚本放行。
pub const OP_ADMIT: u8 = 8;
/// 操作码：local 作用域。
pub const OP_LOCAL: u8 = 9;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（每字节一次乘法，共八次）。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 基线特性名。
pub const FNAMES: [&str; FEATURES] = [
    "param-default",
    "param-assign",
    "param-error",
    "param-alt",
    "trim-prefix",
    "trim-suffix",
    "replace",
    "substring",
    "case-modify",
    "arithmetic",
    "case-esac",
    "for-in",
    "heredoc",
    "local",
    "double-bracket",
    "array",
    "append-assign",
    "function-keyword",
    "select",
    "process-substitution",
    "source-builtin",
    "printf-q",
    "echo-e",
    "here-string",
    "length",
];

/// 基线状态表：与 `FNAMES` 逐行对应。
pub const STATUS: [u8; FEATURES] = [
    ST_OK,   // ${x:-y}
    ST_OK,   // ${x:=y}
    ST_OK,   // ${x:?y}
    ST_OK,   // ${x:+y}
    ST_OK,   // ${x#p} / ${x##p}
    ST_OK,   // ${x%s} / ${x%%s}
    ST_OK,   // ${x/a/b}
    ST_OK,   // ${x:1:2}
    ST_NO,   // ${x^^} / ${x,,}
    ST_OK,   // $((...))
    ST_OK,   // case/esac
    ST_OK,   // for x in
    ST_OK,   // heredoc
    ST_OK,   // local
    ST_NO,   // [[ ]]
    ST_NO,   // array
    ST_OK,   // x+=y
    ST_DIFF, // function 关键字
    ST_DIFF, // select
    ST_NO,   // <(...)
    ST_DIFF, // source
    ST_NO,   // printf %q
    ST_DIFF, // echo -e
    ST_NO,   // <<<
    ST_OK,   // ${#x}
];

/// 特性名。
pub fn feature_name(id: u8) -> Result<&'static str, u32> {
    match FNAMES.get(id as usize) {
        Some(v) => Ok(v),
        None => Err(ASH_EFEAT),
    }
}

/// 特性状态。
pub fn feature_status(id: u8) -> Result<u8, u32> {
    match STATUS.get(id as usize) {
        Some(v) => Ok(*v),
        None => Err(ASH_EFEAT),
    }
}

/// 按状态统计条数。
pub fn count_status(want: u8) -> Result<u32, u32> {
    if want > ST_NO {
        return Err(ASH_EORDER);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < FEATURES {
        match STATUS.get(i) {
            Some(v) => {
                if *v == want {
                    n += 1;
                }
            }
            None => return Err(ASH_EFEAT),
        }
        i += 1;
    }
    Ok(n)
}

/// 支持率：OK + DIFF 占全部特性的百分比。
pub fn support_pct() -> Result<u32, u32> {
    let ok = count_status(ST_OK)?;
    let diff = count_status(ST_DIFF)?;
    Ok(ok.saturating_add(diff).saturating_mul(100) / FEATURES as u32)
}

/// 纯净率：仅 OK 的百分比。
pub fn clean_pct() -> Result<u32, u32> {
    Ok(count_status(ST_OK)?.saturating_mul(100) / FEATURES as u32)
}

/// 状态标签。
pub fn status_label(st: u8) -> Result<&'static str, u32> {
    match st {
        ST_OK => Ok("支持且一致"),
        ST_DIFF => Ok("支持但有差异"),
        ST_NO => Ok("不支持"),
        _ => Err(ASH_EORDER),
    }
}

/// 一行的基线描述：`name=label`，用于诊断输出。
pub fn feature_line(id: u8) -> Result<String, u32> {
    let name = feature_name(id)?;
    let st = feature_status(id)?;
    let label = status_label(st)?;
    let mut out = String::new();
    out.push_str(name);
    out.push('=');
    out.push_str(label);
    Ok(out)
}

/// 形态 → 基线特性号（裸名为 `NO_FEATURE`）。
pub fn form_feature(form: u8) -> Result<u8, u32> {
    match form {
        FM_NAME => Ok(NO_FEATURE),
        FM_DEFAULT => Ok(0),
        FM_ASSIGN => Ok(1),
        FM_ERROR => Ok(2),
        FM_ALT => Ok(3),
        FM_TRIM_PRE => Ok(4),
        FM_TRIM_PRE_MAX => Ok(4),
        FM_TRIM_SUF => Ok(5),
        FM_TRIM_SUF_MAX => Ok(5),
        FM_REPLACE => Ok(6),
        FM_SUBSTR => Ok(7),
        FM_CASE => Ok(8),
        FM_LEN => Ok(24),
        _ => Err(ASH_EPARAM),
    }
}

/// 形态 → 状态。
pub fn form_status(form: u8) -> Result<u8, u32> {
    let f = form_feature(form)?;
    if f == NO_FEATURE {
        return Ok(ST_OK);
    }
    feature_status(f)
}

/// 参数展开分类：十三种形态，逐字节识别运算符。
pub fn classify(word: &str) -> Result<u8, u32> {
    let b = word.as_bytes();
    if b.len() < 4 {
        return Err(ASH_EPARAM);
    }
    if b.first() != Some(&b'$') {
        return Err(ASH_EPARAM);
    }
    if b.get(1) != Some(&b'{') {
        return Err(ASH_EPARAM);
    }
    if b.last() != Some(&b'}') {
        return Err(ASH_EPARAM);
    }
    let inner = match word.get(2..b.len() - 1) {
        Some(s) => s,
        None => return Err(ASH_EPARAM),
    };
    let ib = inner.as_bytes();
    if ib.is_empty() {
        return Err(ASH_EPARAM);
    }
    if ib.first() == Some(&b'#') {
        return Ok(FM_LEN);
    }
    let mut i = 1usize;
    while i < ib.len() {
        let c = match ib.get(i) {
            Some(v) => *v,
            None => return Err(ASH_EPARAM),
        };
        let next = ib.get(i + 1).copied();
        match c {
            b':' => {
                if next == Some(b'-') {
                    return Ok(FM_DEFAULT);
                }
                if next == Some(b'=') {
                    return Ok(FM_ASSIGN);
                }
                if next == Some(b'?') {
                    return Ok(FM_ERROR);
                }
                if next == Some(b'+') {
                    return Ok(FM_ALT);
                }
                if let Some(n) = next {
                    if n.is_ascii_digit() || n == b' ' {
                        return Ok(FM_SUBSTR);
                    }
                }
                return Err(ASH_EPARAM);
            }
            b'#' => {
                if next == Some(b'#') {
                    return Ok(FM_TRIM_PRE_MAX);
                }
                return Ok(FM_TRIM_PRE);
            }
            b'%' => {
                if next == Some(b'%') {
                    return Ok(FM_TRIM_SUF_MAX);
                }
                return Ok(FM_TRIM_SUF);
            }
            b'/' => return Ok(FM_REPLACE),
            b'^' => return Ok(FM_CASE),
            b',' => return Ok(FM_CASE),
            _ => {}
        }
        i += 1;
    }
    Ok(FM_NAME)
}

/// 引号扫描：返回成对引号数；未闭合则拒绝。反斜杠跳过下一字节。
pub fn scan_quotes(line: &str) -> Result<u32, u32> {
    let b = line.as_bytes();
    let mut i = 0usize;
    let mut single = false;
    let mut double = false;
    let mut pairs = 0u32;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(ASH_EQUOTE),
        };
        if c == b'\\' {
            i = i.saturating_add(2);
            continue;
        }
        if single {
            if c == b'\'' {
                single = false;
                pairs += 1;
            }
        } else if double {
            if c == b'"' {
                double = false;
                pairs += 1;
            }
        } else if c == b'\'' {
            single = true;
        } else if c == b'"' {
            double = true;
        }
        i += 1;
    }
    if single || double {
        Err(ASH_EQUOTE)
    } else {
        Ok(pairs)
    }
}

/// 算术扫描：返回 `$((...))` 段数；括号未配平则拒绝。
pub fn scan_arith(line: &str) -> Result<u32, u32> {
    let b = line.as_bytes();
    let mut i = 0usize;
    let mut found = 0u32;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        if c == b'$' && b.get(i + 1) == Some(&b'(') && b.get(i + 2) == Some(&b'(') {
            found += 1;
            let mut depth = 2i32;
            let mut j = i + 3;
            while j < b.len() {
                match b.get(j) {
                    Some(b'(') => depth += 1,
                    Some(b')') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Some(_) => {}
                    None => break,
                }
                j += 1;
            }
            if depth != 0 {
                return Err(ASH_EARITH);
            }
            i = j.saturating_add(1);
        } else {
            i += 1;
        }
    }
    Ok(found)
}

/// 词分隔符：空白与 shell 的复合命令分隔符（`;`、括号、管道）。
fn is_sep(c: u8) -> bool {
    c == b' '
        || c == b'\t'
        || c == b'\n'
        || c == b'\r'
        || c == b';'
        || c == b'('
        || c == b')'
        || c == b'|'
        || c == b'&'
}

/// 按分隔符切词。
fn next_word(bytes: &[u8], from: usize) -> (Option<&[u8]>, usize) {
    let mut i = from;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(c) => {
                if is_sep(*c) {
                    i += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    if i >= bytes.len() {
        return (None, i);
    }
    let start = i;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(c) => {
                if is_sep(*c) {
                    break;
                }
                i += 1;
            }
            None => break,
        }
    }
    match bytes.get(start..i) {
        Some(w) => (Some(w), i),
        None => (None, i),
    }
}

/// `case/esac` 配对扫描：返回 `case` 个数。
pub fn case_depth(script: &str) -> Result<u32, u32> {
    let b = script.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut cases = 0u32;
    loop {
        let (w, next) = next_word(b, i);
        let w = match w {
            Some(v) => v,
            None => break,
        };
        if w == b"case" {
            depth += 1;
            cases += 1;
        } else if w == b"esac" {
            if depth == 0 {
                return Err(ASH_ECASE);
            }
            depth -= 1;
        }
        i = next;
    }
    if depth != 0 {
        Err(ASH_ECASE)
    } else {
        Ok(cases)
    }
}

/// 花括号收支扫描。
pub fn brace_depth(script: &str) -> Result<u32, u32> {
    let b = script.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut opens = 0u32;
    while i < b.len() {
        match b.get(i) {
            Some(b'{') => {
                depth += 1;
                opens += 1;
            }
            Some(b'}') => {
                depth -= 1;
                if depth < 0 {
                    return Err(ASH_EORDER);
                }
            }
            Some(_) => {}
            None => break,
        }
        i += 1;
    }
    if depth != 0 {
        Err(ASH_EORDER)
    } else {
        Ok(opens)
    }
}

/// `local` 作用域护栏：函数外出现 `local` 拒绝。
pub fn local_guard(line: &str, in_function: bool) -> Result<bool, u32> {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(c) => {
                if *c == b' ' || *c == b'\t' {
                    i += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    let rest = match line.get(i..) {
        Some(v) => v,
        None => return Err(ASH_ELOCAL),
    };
    if rest == "local" {
        return if in_function { Ok(true) } else { Err(ASH_ELOCAL) };
    }
    if rest.starts_with("local ") || rest.starts_with("local\t") {
        return if in_function { Ok(true) } else { Err(ASH_ELOCAL) };
    }
    Ok(false)
}

/// 特性闸门：状态 × 方言。
pub fn require(feature: u8, dialect: u8) -> Result<u8, u32> {
    if dialect > DIAL_POSIX {
        return Err(ASH_EDIAL);
    }
    let st = feature_status(feature)?;
    match dialect {
        DIAL_ASH => {
            if st == ST_NO {
                Err(ASH_EFEAT)
            } else {
                Ok(st)
            }
        }
        DIAL_BASH => {
            if st == ST_NO || st == ST_DIFF {
                Err(ASH_EDIAL)
            } else {
                Ok(st)
            }
        }
        _ => {
            if st == ST_NO {
                Err(ASH_EFEAT)
            } else {
                Ok(st)
            }
        }
    }
}

/// 整脚本放行：五道扫描器 + 参数展开特性闸门。
pub fn admit(script: &str, dialect: u8) -> Result<u32, u32> {
    if script.is_empty() {
        return Err(ASH_EORDER);
    }
    if dialect > DIAL_POSIX {
        return Err(ASH_EDIAL);
    }
    let _ = scan_quotes(script)?;
    let _ = scan_arith(script)?;
    let _ = case_depth(script)?;
    let _ = brace_depth(script)?;
    let b = script.as_bytes();
    let mut i = 0usize;
    let mut expansions = 0u32;
    while i < b.len() {
        let is_open = b.get(i) == Some(&b'$') && b.get(i + 1) == Some(&b'{');
        if !is_open {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let mut close = None;
        while j < b.len() {
            if b.get(j) == Some(&b'}') {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let end = match close {
            Some(v) => v,
            None => return Err(ASH_EPARAM),
        };
        let word = match script.get(i..end + 1) {
            Some(v) => v,
            None => return Err(ASH_EPARAM),
        };
        let form = classify(word)?;
        let st = form_status(form)?;
        if st == ST_NO {
            if dialect == DIAL_ASH || dialect == DIAL_POSIX {
                return Err(ASH_EFEAT);
            }
            return Err(ASH_EDIAL);
        }
        if st == ST_DIFF && dialect == DIAL_BASH {
            return Err(ASH_EDIAL);
        }
        expansions += 1;
        i = end + 1;
    }
    Ok(expansions)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(ASH_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(ASH_EBAL);
        }
        const VALS: [u32; 8] = [1, 6, 9, 0, 1, 76, 1, 2];
        const CODES: [u32; 6] =
            [ASH_EPARAM, ASH_EQUOTE, ASH_EARITH, ASH_ECASE, ASH_EFEAT, ASH_EDIAL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ASH_EBAL),
            };
            if s.slot as usize != i {
                return Err(ASH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ASH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ASH_EVALUE);
                        }
                    }
                    None => return Err(ASH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ASH_EVALUE);
                        }
                    }
                    None => return Err(ASH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ASH_EBAL);
        }
        Ok(())
    }

    /// 票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(ASH_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(ASH_ESEAL)
        }
    }

    /// 冻结票号。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(
        0,
        OP_CLASSIFY,
        classify("${x:-y}").map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        1,
        OP_CLASSIFY,
        classify("${x##*/}").map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        2,
        OP_CLASSIFY,
        classify("${x/a/b}").map(|v| v as u32),
    ));
    let _ = l.register(word_cell(3, OP_REQUIRE, require(9, DIAL_ASH).map(|v| v as u32)));
    let _ = l.register(word_cell(4, OP_REQUIRE, require(17, DIAL_ASH).map(|v| v as u32)));
    let _ = l.register(word_cell(5, OP_RATIO, support_pct()));
    let _ = l.register(word_cell(6, OP_CASES, case_depth("case a in x) ;; esac")));
    let _ = l.register(word_cell(7, OP_QUOTES, scan_quotes("a 'b' \"c\"")));

    let _ = l.register(word_cell(8, OP_CLASSIFY, classify("${}").map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_QUOTES, scan_quotes("'abc")));
    let _ = l.register(word_cell(10, OP_ARITH, scan_arith("$((1+2)")));
    let _ = l.register(word_cell(11, OP_CASES, case_depth("case a in")));
    let _ = l.register(word_cell(12, OP_REQUIRE, require(8, DIAL_ASH).map(|v| v as u32)));
    let _ = l.register(word_cell(13, OP_REQUIRE, require(17, DIAL_BASH).map(|v| v as u32)));

    l
}

/// 四路注入：基线表 / 形态与闸门 / 三台扫描器 / 整脚本放行。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut names_ok = true;
    let mut i = 0usize;
    while i < FEATURES {
        let a = match FNAMES.get(i) {
            Some(v) => *v,
            None => {
                names_ok = false;
                break;
            }
        };
        let mut j = i + 1;
        while j < FEATURES {
            if let Some(b) = FNAMES.get(j) {
                if a == *b {
                    names_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out[0] = FEATURES == 25
        && names_ok
        && count_status(ST_OK) == Ok(15)
        && count_status(ST_DIFF) == Ok(4)
        && count_status(ST_NO) == Ok(6)
        && support_pct() == Ok(76)
        && clean_pct() == Ok(60)
        && status_label(ST_DIFF) == Ok("支持但有差异")
        && feature_name(24) == Ok("length")
        && feature_status(25) == Err(ASH_EFEAT);

    out[1] = classify("${x:-y}") == Ok(FM_DEFAULT)
        && classify("${x##*/}") == Ok(FM_TRIM_PRE_MAX)
        && classify("${x%%*/}") == Ok(FM_TRIM_SUF_MAX)
        && classify("${x/a/b}") == Ok(FM_REPLACE)
        && classify("${x:1:2}") == Ok(FM_SUBSTR)
        && classify("${#x}") == Ok(FM_LEN)
        && classify("${x}") == Ok(FM_NAME)
        && classify("${x^^}") == Ok(FM_CASE)
        && classify("${}") == Err(ASH_EPARAM)
        && classify("${x") == Err(ASH_EPARAM)
        && classify("$x") == Err(ASH_EPARAM)
        && form_status(FM_CASE) == Ok(ST_NO)
        && form_status(FM_NAME) == Ok(ST_OK);

    out[2] = scan_quotes("a 'b' \"c\"") == Ok(2)
        && scan_quotes("a\\'b") == Ok(0)
        && scan_quotes("'abc") == Err(ASH_EQUOTE)
        && scan_arith("$((1+2))") == Ok(1)
        && scan_arith("$((1+(2*3))) $((4))") == Ok(2)
        && scan_arith("$((1+2)") == Err(ASH_EARITH)
        && case_depth("case a in x) ;; esac") == Ok(1)
        && case_depth("case a in x) ;; esac; case b in y) ;; esac") == Ok(2)
        && case_depth("case a in") == Err(ASH_ECASE)
        && case_depth("esac") == Err(ASH_ECASE)
        && brace_depth("f() { local x; }") == Ok(1)
        && brace_depth("} {") == Err(ASH_EORDER);

    let ash_ok = require(9, DIAL_ASH) == Ok(ST_OK);
    let ash_diff = require(17, DIAL_ASH) == Ok(ST_DIFF);
    let ash_no = require(8, DIAL_ASH) == Err(ASH_EFEAT);
    let bash_no = require(8, DIAL_BASH) == Err(ASH_EDIAL);
    let bash_diff = require(17, DIAL_BASH) == Err(ASH_EDIAL);
    let bash_ok = require(9, DIAL_BASH) == Ok(ST_OK);
    let posix_diff = require(17, DIAL_POSIX) == Ok(ST_DIFF);
    let bad_dial = require(9, 9) == Err(ASH_EDIAL);
    let local_out = local_guard("local x=1", false) == Err(ASH_ELOCAL);
    let local_in = local_guard("  local x=1", true) == Ok(true);
    let local_none = local_guard("echo hi", false) == Ok(false);
    let admit_ok = admit("echo ${x:-y}", DIAL_ASH) == Ok(1);
    let admit_two = admit("echo ${x:-y} ${x#p}", DIAL_ASH) == Ok(2);
    let admit_bad = admit("echo ${x^^}", DIAL_ASH) == Err(ASH_EFEAT);
    let admit_quote = admit("echo 'x", DIAL_ASH) == Err(ASH_EQUOTE);
    let admit_empty = admit("", DIAL_ASH) == Err(ASH_EORDER);
    out[3] = ash_ok
        && ash_diff
        && ash_no
        && bash_no
        && bash_diff
        && bash_ok
        && posix_diff
        && bad_dial
        && local_out
        && local_in
        && local_none
        && admit_ok
        && admit_two
        && admit_bad
        && admit_quote
        && admit_empty;

    out
}

/// 十三项判据。
pub fn run_unx11589_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11589.busybox_ash");
    let l = build_ledger();

    s.add(
        "ASH1-二十五条基线表",
        FEATURES == 25
            && FNAMES.len() == FEATURES
            && STATUS.len() == FEATURES
            && feature_name(0) == Ok("param-default")
            && feature_name(24) == Ok("length")
            && feature_name(25) == Err(ASH_EFEAT)
            && feature_status(24) == Ok(ST_OK)
            && feature_line(8).map(|v| v.len() as u32) == Ok(21)
            && feature_line(25) == Err(ASH_EFEAT),
        "static",
    );

    s.add(
        "ASH2-三档分布与双率",
        count_status(ST_OK) == Ok(15)
            && count_status(ST_DIFF) == Ok(4)
            && count_status(ST_NO) == Ok(6)
            && support_pct() == Ok(76)
            && clean_pct() == Ok(60)
            && support_pct() > clean_pct()
            && count_status(9) == Err(ASH_EORDER),
        "static",
    );

    s.add(
        "ASH3-展开形态分类",
        classify("${x}") == Ok(FM_NAME)
            && classify("${x:-y}") == Ok(FM_DEFAULT)
            && classify("${x:=y}") == Ok(FM_ASSIGN)
            && classify("${x:?y}") == Ok(FM_ERROR)
            && classify("${x:+y}") == Ok(FM_ALT)
            && classify("${x#p}") == Ok(FM_TRIM_PRE)
            && classify("${x##p}") == Ok(FM_TRIM_PRE_MAX)
            && classify("${x%s}") == Ok(FM_TRIM_SUF)
            && classify("${x%%s}") == Ok(FM_TRIM_SUF_MAX)
            && classify("${x/a/b}") == Ok(FM_REPLACE)
            && classify("${x:1:2}") == Ok(FM_SUBSTR)
            && classify("${x^^}") == Ok(FM_CASE)
            && classify("${#x}") == Ok(FM_LEN),
        "static",
    );

    s.add(
        "ASH4-形态闸门拒绝非法",
        classify("${}") == Err(ASH_EPARAM)
            && classify("${x") == Err(ASH_EPARAM)
            && classify("$x") == Err(ASH_EPARAM)
            && classify("${x:}") == Err(ASH_EPARAM)
            && form_feature(FM_NAME) == Ok(NO_FEATURE)
            && form_feature(FM_CASE) == Ok(8)
            && form_feature(99) == Err(ASH_EPARAM),
        "static",
    );

    s.add(
        "ASH5-引号扫描配对",
        scan_quotes("a 'b' \"c\"") == Ok(2)
            && scan_quotes("a\\'b") == Ok(0)
            && scan_quotes("'a\"b'") == Ok(1)
            && scan_quotes("'abc") == Err(ASH_EQUOTE)
            && scan_quotes("abc\"") == Err(ASH_EQUOTE),
        "static",
    );

    s.add(
        "ASH6-算术括号配平",
        scan_arith("$((1+2))") == Ok(1)
            && scan_arith("$((1+(2*3))) $((4))") == Ok(2)
            && scan_arith("plain") == Ok(0)
            && scan_arith("$((1+2)") == Err(ASH_EARITH)
            && scan_arith("$(((1))") == Err(ASH_EARITH),
        "static",
    );

    s.add(
        "ASH7-case结构配平",
        case_depth("case a in x) ;; esac") == Ok(1)
            && case_depth("case a in x) ;; esac; case b in y) ;; esac") == Ok(2)
            && case_depth("case a in") == Err(ASH_ECASE)
            && case_depth("esac") == Err(ASH_ECASE)
            && case_depth("casein") == Ok(0),
        "static",
    );

    s.add(
        "ASH8-花括号收支",
        brace_depth("f() { local x; }") == Ok(1)
            && brace_depth("echo ${x}") == Ok(1)
            && brace_depth("} {") == Err(ASH_EORDER)
            && brace_depth("{") == Err(ASH_EORDER),
        "static",
    );

    s.add(
        "ASH9-ash闸门三档",
        require(9, DIAL_ASH) == Ok(ST_OK)
            && require(17, DIAL_ASH) == Ok(ST_DIFF)
            && require(8, DIAL_ASH) == Err(ASH_EFEAT)
            && require(14, DIAL_ASH) == Err(ASH_EFEAT)
            && require(25, DIAL_ASH) == Err(ASH_EFEAT),
        "static",
    );

    s.add(
        "ASH10-bash闸门差异拒绝",
        require(9, DIAL_BASH) == Ok(ST_OK)
            && require(17, DIAL_BASH) == Err(ASH_EDIAL)
            && require(8, DIAL_BASH) == Err(ASH_EDIAL)
            && require(17, DIAL_POSIX) == Ok(ST_DIFF)
            && require(8, DIAL_POSIX) == Err(ASH_EFEAT)
            && require(9, 9) == Err(ASH_EDIAL),
        "static",
    );

    s.add(
        "ASH11-local作用域",
        local_guard("local x=1", false) == Err(ASH_ELOCAL)
            && local_guard("  local x=1", true) == Ok(true)
            && local_guard("echo hi", false) == Ok(false)
            && local_guard("locally", false) == Ok(false),
        "static",
    );

    s.add(
        "ASH12-整脚本放行链",
        admit("echo ${x:-y}", DIAL_ASH) == Ok(1)
            && admit("echo ${x:-y} ${x#p}", DIAL_ASH) == Ok(2)
            && admit("echo ${x^^}", DIAL_ASH) == Err(ASH_EFEAT)
            && admit("echo ${x^^}", DIAL_BASH) == Err(ASH_EDIAL)
            && admit("echo 'x", DIAL_ASH) == Err(ASH_EQUOTE)
            && admit("", DIAL_ASH) == Err(ASH_EORDER),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let one = f.freeze();
    let two = f.freeze();
    let inj = inject_four();
    s.add(
        "ASH13-冻结重放四注入",
        l.audit().is_ok()
            && one.is_ok()
            && one == rep
            && two.is_err()
            && f.is_frozen()
            && f.root() != 0
            && f.rejects() == 1
            && inj[0]
            && inj[1]
            && inj[2]
            && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(1).map(|s| s.value), Some(6));
        assert_eq!(l.sample(12).map(|s| s.code), Some(ASH_EFEAT));
    }

    #[test]
    fn feature_table_shape() {
        assert_eq!(FEATURES, 25);
        assert_eq!(FNAMES.len(), 25);
        assert_eq!(STATUS.len(), 25);
        assert_eq!(feature_name(24), Ok("length"));
        assert_eq!(feature_name(25), Err(ASH_EFEAT));
        assert_eq!(feature_status(8), Ok(ST_NO));
        assert_eq!(status_label(ST_OK), Ok("支持且一致"));
    }

    #[test]
    fn feature_tallies() {
        assert_eq!(count_status(ST_OK), Ok(15));
        assert_eq!(count_status(ST_DIFF), Ok(4));
        assert_eq!(count_status(ST_NO), Ok(6));
        assert_eq!(support_pct(), Ok(76));
        assert_eq!(clean_pct(), Ok(60));
        assert_eq!(count_status(7), Err(ASH_EORDER));
    }

    #[test]
    fn classify_trims() {
        assert_eq!(classify("${x#*/}"), Ok(FM_TRIM_PRE));
        assert_eq!(classify("${x##*/}"), Ok(FM_TRIM_PRE_MAX));
        assert_eq!(classify("${x%*/}"), Ok(FM_TRIM_SUF));
        assert_eq!(classify("${x%%*/}"), Ok(FM_TRIM_SUF_MAX));
        assert_eq!(classify("${x:-d}"), Ok(FM_DEFAULT));
        assert_eq!(classify("${x:2}"), Ok(FM_SUBSTR));
        assert_eq!(classify("${#x}"), Ok(FM_LEN));
    }

    #[test]
    fn classify_rejects() {
        assert_eq!(classify(""), Err(ASH_EPARAM));
        assert_eq!(classify("${}"), Err(ASH_EPARAM));
        assert_eq!(classify("${x"), Err(ASH_EPARAM));
        assert_eq!(classify("$x"), Err(ASH_EPARAM));
        assert_eq!(classify("${x:}"), Err(ASH_EPARAM));
        assert_eq!(classify("${x^}"), Ok(FM_CASE));
    }

    #[test]
    fn form_mapping() {
        assert_eq!(form_feature(FM_DEFAULT), Ok(0));
        assert_eq!(form_feature(FM_TRIM_PRE_MAX), Ok(4));
        assert_eq!(form_feature(FM_SUBSTR), Ok(7));
        assert_eq!(form_feature(FM_LEN), Ok(24));
        assert_eq!(form_feature(FM_NAME), Ok(NO_FEATURE));
        assert_eq!(form_status(FM_CASE), Ok(ST_NO));
        assert_eq!(form_status(FM_REPLACE), Ok(ST_OK));
        assert_eq!(form_feature(20), Err(ASH_EPARAM));
    }

    #[test]
    fn quote_scanning() {
        assert_eq!(scan_quotes("a 'b' \"c\""), Ok(2));
        assert_eq!(scan_quotes("\\'"), Ok(0));
        assert_eq!(scan_quotes("'a b'"), Ok(1));
        assert_eq!(scan_quotes("'a"), Err(ASH_EQUOTE));
        assert_eq!(scan_quotes("\"a"), Err(ASH_EQUOTE));
        assert_eq!(scan_quotes("'a\"b'"), Ok(1));
    }

    #[test]
    fn arith_scanning() {
        assert_eq!(scan_arith("$((1))"), Ok(1));
        assert_eq!(scan_arith("$((1+(2*3)))"), Ok(1));
        assert_eq!(scan_arith("$((1)) $((2))"), Ok(2));
        assert_eq!(scan_arith("x"), Ok(0));
        assert_eq!(scan_arith("$((1)"), Err(ASH_EARITH));
        assert_eq!(scan_arith("$((((1)))"), Err(ASH_EARITH));
    }

    #[test]
    fn case_scanning() {
        assert_eq!(case_depth("case a in esac"), Ok(1));
        assert_eq!(case_depth("case a in case b in esac esac"), Ok(2));
        assert_eq!(case_depth("case a in"), Err(ASH_ECASE));
        assert_eq!(case_depth("esac"), Err(ASH_ECASE));
        assert_eq!(case_depth("showcase"), Ok(0));
    }

    #[test]
    fn brace_scanning() {
        assert_eq!(brace_depth("f() { :; }"), Ok(1));
        assert_eq!(brace_depth("echo ${x}"), Ok(1));
        assert_eq!(brace_depth("{ { } }"), Ok(2));
        assert_eq!(brace_depth("}"), Err(ASH_EORDER));
        assert_eq!(brace_depth("{"), Err(ASH_EORDER));
    }

    #[test]
    fn dialect_gates() {
        assert_eq!(require(9, DIAL_ASH), Ok(ST_OK));
        assert_eq!(require(17, DIAL_ASH), Ok(ST_DIFF));
        assert_eq!(require(8, DIAL_ASH), Err(ASH_EFEAT));
        assert_eq!(require(9, DIAL_BASH), Ok(ST_OK));
        assert_eq!(require(17, DIAL_BASH), Err(ASH_EDIAL));
        assert_eq!(require(8, DIAL_BASH), Err(ASH_EDIAL));
        assert_eq!(require(17, DIAL_POSIX), Ok(ST_DIFF));
        assert_eq!(require(9, 9), Err(ASH_EDIAL));
    }

    #[test]
    fn local_scope_guard() {
        assert_eq!(local_guard("local x=1", false), Err(ASH_ELOCAL));
        assert_eq!(local_guard("\tlocal x", false), Err(ASH_ELOCAL));
        assert_eq!(local_guard("local x", true), Ok(true));
        assert_eq!(local_guard("echo local", false), Ok(false));
    }

    #[test]
    fn admit_chain() {
        assert_eq!(admit("echo ${x:-y}", DIAL_ASH), Ok(1));
        assert_eq!(admit("echo ${x:-y} ${x%%p}", DIAL_ASH), Ok(2));
        assert_eq!(admit("echo $((1+2))", DIAL_ASH), Ok(0));
        assert_eq!(admit("echo ${x^^}", DIAL_ASH), Err(ASH_EFEAT));
        assert_eq!(admit("echo ${x^^}", DIAL_BASH), Err(ASH_EDIAL));
        assert_eq!(admit("echo 'x", DIAL_ASH), Err(ASH_EQUOTE));
        assert_eq!(admit("", DIAL_ASH), Err(ASH_EORDER));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(ASH_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.root(), root);
    }

    #[test]
    fn ledger_slot_guard() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 2, op: OP_CLASSIFY, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad), Err(ASH_ESLOT));
        let good = Sample { slot: 0, op: OP_CLASSIFY, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good), Err(ASH_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11589_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
