//! unx585_valparse —— UNX-F0585 三型值解析（布尔/整数/字符串）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0585 · 三型值解析（布尔/整数/字符串）· 目标 220 行 · 已深化
//! 判据原文：三型解析闭集完备——布尔两态/整数十进制带号界内/字符串转义闭集，
//!           歧义与畸形 100% 拒绝并落账
//! 组件：三型值模型（Bool/Int/Str，全整数确定性：字符串以字节存）
//!       + 布尔解析（true/false 两态闭集，1/0 与 yes/no 明确不收——歧义拒绝）
//!       + 整数解析（十进制带号 + i64 界校验 + 前导零/空串拒绝）
//!       + 字符串解析（转义闭集 \n\t\\\" 四种，未知转义拒绝）
//!       + 解析账（append-only：型别/成功位/拒绝码三列）
//!       + 对拍核（注入核四类必抓）。
//! 职责定位：配置面唯一权威值解析——boot 参数与策略面共享本解析；
//!   歧义拒绝语义保证同一输入恒同型；解析账使每次拒绝可追。
//! 性能逐项分解：布尔 O(串长)；整数 O(串长)；字符串 O(串长)；
//!   落账 O(1) 均摊。全整数确定性，零浮点。
//! 诊断码段：0xE380（0xE381~0xE388 闭集；空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const VP_VERSION: &str = "unx585_valparse-v1";

/// 诊断段基址（0xE380 段 · 本模块独占）。
pub const VP_SEG_BASE: u16 = 0xE380;

/// XcCode 闭集（0xE381~0xE388）。
pub const XC_BOOL_AMBIG: u32 = 0xE381;
pub const XC_INT_EMPTY: u32 = 0xE382;
pub const XC_INT_RANGE: u32 = 0xE383;
pub const XC_INT_DIGIT: u32 = 0xE384;
pub const XC_STR_ESCAPE: u32 = 0xE385;
pub const XC_TYPE_AMBIG: u32 = 0xE386;
pub const XC_INJECT_MISSED: u32 = 0xE387;
pub const XC_LEDGER_BROKEN: u32 = 0xE388;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_BOOL_AMBIG => ("布尔歧义（1/0/yes/no 不属两态闭集）", "布尔面只收 true/false 字面量"),
        XC_INT_EMPTY => ("整数为空串（无有效数字）", "配置值至少一位十进制数字"),
        XC_INT_RANGE => ("整数越界（超出 i64 表示界）", "按 i64 上下界截拒"),
        XC_INT_DIGIT => ("整数含非十进制数字或非法前导零", "十进制闭集 0~9，前导零只许 0 本身"),
        XC_STR_ESCAPE => ("字符串含未知转义序列", "转义闭集 \\n \\t \\\\ \\\" 四种"),
        XC_TYPE_AMBIG => ("值型别歧义（双型均可行未显性择一）", "调用面显式指定目标型别"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("解析账 append-only 性被破坏（序号跳号）", "检查落账路径是否绕过账务封装"),
        _ => ("未知诊断码", "查 0xE380 段闭集登记表"),
    }
}

/// 三型值（全整数确定性存储）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    Bool(bool),
    Int(i64),
    /// 解析后的字节串（转义已展开）。
    Str(Vec<u8>),
}

/// 解析错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    BoolAmbig,
    IntEmpty,
    IntRange,
    IntDigit(u8),
    StrEscape(u8),
    LedgerBroken(u32),
}

/// 解析账一行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct ParseRow {
    /// 目标型别（0=Bool 1=Int 2=Str）。
    pub ty: u8,
    /// 成功位。
    pub ok: u8,
    /// 拒绝码（成功为 0）。
    pub code: u32,
}

/// 解析器（带账）。
pub struct ValParser {
    rows: Vec<ParseRow>,
    next_seq: u32,
}

impl ValParser {
    pub fn new() -> ValParser {
        ValParser {
            rows: Vec::new(),
            next_seq: 1,
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.rows.len() + 1
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn row_at(&self, i: usize) -> Option<ParseRow> {
        self.rows.get(i).copied()
    }

    fn record(&mut self, ty: u8, ok: bool, code: u32) {
        let seq = self.next_seq;
        self.rows.push(ParseRow {
            ty,
            ok: ok as u8,
            code,
        });
        self.next_seq = seq + 1;
    }

    /// 布尔解析（两态闭集，歧义拒绝）。
    pub fn parse_bool(&mut self, s: &[u8]) -> Result<bool, ParseError> {
        let r = parse_bool_pure(s);
        match &r {
            Ok(_) => self.record(0, true, 0),
            Err(_) => self.record(0, false, XC_BOOL_AMBIG as u32),
        }
        r
    }

    /// 整数解析（十进制带号 + i64 界 + 前导零闭集）。
    pub fn parse_int(&mut self, s: &[u8]) -> Result<i64, ParseError> {
        let r = parse_int_pure(s);
        match &r {
            Ok(_) => self.record(1, true, 0),
            Err(e) => {
                let c = match e {
                    ParseError::IntEmpty => XC_INT_EMPTY,
                    ParseError::IntRange => XC_INT_RANGE,
                    ParseError::IntDigit(_) => XC_INT_DIGIT,
                    _ => XC_INT_DIGIT,
                };
                self.record(1, false, c as u32);
            }
        }
        r
    }

    /// 字符串解析（转义闭集四型，未知转义拒绝）。
    pub fn parse_str(&mut self, s: &[u8]) -> Result<Vec<u8>, ParseError> {
        let r = parse_str_pure(s);
        match &r {
            Ok(_) => self.record(2, true, 0),
            Err(e) => {
                let c = match e {
                    ParseError::StrEscape(_) => XC_STR_ESCAPE,
                    _ => XC_STR_ESCAPE,
                };
                self.record(2, false, c as u32);
            }
        }
        r
    }
}

/// 纯布尔解析（无账版）。
pub fn parse_bool_pure(s: &[u8]) -> Result<bool, ParseError> {
    if s == b"true" {
        return Ok(true);
    }
    if s == b"false" {
        return Ok(false);
    }
    Err(ParseError::BoolAmbig)
}

/// 纯整数解析。
pub fn parse_int_pure(s: &[u8]) -> Result<i64, ParseError> {
    if s.is_empty() {
        return Err(ParseError::IntEmpty);
    }
    let (neg, digits) = match s[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(ParseError::IntEmpty);
    }
    // 前导零闭集：'0' 本身或 '0' 开头长度 1；否则首数字非 0
    if digits.len() > 1 && digits[0] == b'0' {
        return Err(ParseError::IntDigit(digits[0]));
    }
    let mut acc: i64 = 0;
    for &d in digits {
        if !d.is_ascii_digit() {
            return Err(ParseError::IntDigit(d));
        }
        let v = (d - b'0') as i64;
        acc = match acc.checked_mul(10).and_then(|x| {
            if neg {
                x.checked_sub(v)
            } else {
                x.checked_add(v)
            }
        }) {
            Some(x) => x,
            None => return Err(ParseError::IntRange),
        };
    }
    Ok(acc)
}

/// 纯字符串解析（转义闭集：\n \t \\ \" 四种）。
pub fn parse_str_pure(s: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        if i + 1 >= s.len() {
            return Err(ParseError::StrEscape(b'\\'));
        }
        let c = s[i + 1];
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            other => return Err(ParseError::StrEscape(other)),
        }
        i += 2;
    }
    Ok(out)
}

/// 注入核：四类畸形 + 守卫 10/10 必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    // 1) 布尔歧义（yes）
    if parse_bool_pure(b"yes").is_err() {
        caught += 1;
    }
    // 2) 整数空串
    if parse_int_pure(b"").is_err() {
        caught += 1;
    }
    // 3) 整数越界（i64 上界溢出）
    if parse_int_pure(b"99999999999999999999").is_err() {
        caught += 1;
    }
    // 4) 前导零拒绝
    if parse_int_pure(b"007").is_err() {
        caught += 1;
    }
    // 5) 非数字
    if parse_int_pure(b"12x4").is_err() {
        caught += 1;
    }
    // 6) 未知转义
    if parse_str_pure(b"a\\qb").is_err() {
        caught += 1;
    }
    // 7) 转义展开正确（\\n → 0x0A）
    if parse_str_pure(b"a\\nb").map(|v| v == [b'a', b'\n', b'b']).unwrap_or(false) {
        caught += 1;
    }
    // 8) 负数边界值恰好可表示（i64::MIN）
    if parse_int_pure(b"-9223372036854775808") == Ok(i64::MIN) {
        caught += 1;
    }
    // 9) 账序守恒（多型混合记账）
    let mut p = ValParser::new();
    let _ = p.parse_bool(b"true");
    let _ = p.parse_int(b"42");
    let _ = p.parse_str(b"ok");
    let _ = p.parse_bool(b"maybe");
    if p.ledger_ok() && p.rows() == 4 && p.row_at(3).map(|r| r.ok == 0).unwrap_or(false) {
        caught += 1;
    }
    // 10) 账拒绝码可辨（Bool 歧义行 code=XC_BOOL_AMBIG）
    if p.row_at(3).map(|r| r.code == XC_BOOL_AMBIG as u32).unwrap_or(false) {
        caught += 1;
    }
    if caught != 10 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx585_checks`）：判据逐条可失败断言。
pub fn run_unx585_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx585");

    // C585-01 布尔两态闭集（true/false 收，歧义拒）
    {
        let ok = parse_bool_pure(b"true") == Ok(true)
            && parse_bool_pure(b"false") == Ok(false)
            && parse_bool_pure(b"1").is_err()
            && parse_bool_pure(b"yes").is_err();
        s.add("C585-01-布尔两态闭集", ok, if ok { "" } else { "布尔闭集失守" });
    }

    // C585-02 整数十进制带号 + 界内
    {
        let ok = parse_int_pure(b"42") == Ok(42)
            && parse_int_pure(b"-17") == Ok(-17)
            && parse_int_pure(b"+9") == Ok(9)
            && parse_int_pure(b"0") == Ok(0);
        s.add("C585-02-整数带号界内", ok, if ok { "" } else { "整数解析失守" });
    }

    // C585-03 整数越界拒绝（i64::MAX+1）
    {
        let ok = parse_int_pure(b"9223372036854775808").is_err()
            && parse_int_pure(b"-9223372036854775809").is_err();
        s.add("C585-03-整数越界拒", ok, if ok { "" } else { "越界漏放" });
    }

    // C585-04 前导零闭集（0 本身收，007 拒）
    {
        let ok = parse_int_pure(b"0") == Ok(0) && parse_int_pure(b"007").is_err();
        s.add("C585-04-前导零闭集", ok, if ok { "" } else { "前导零语义失守" });
    }

    // C585-05 转义闭集四型展开 + 未知转义拒
    {
        let ok = parse_str_pure(b"\\n\\t\\\\\\\"")
            .map(|v| v == [b'\n', b'\t', b'\\', b'"'])
            .unwrap_or(false)
            && parse_str_pure(b"\\x").is_err()
            && parse_str_pure(b"trailing\\").is_err();
        s.add("C585-05-转义闭集四型", ok, if ok { "" } else { "转义面失守" });
    }

    // C585-06 解析账三列可追（型别/成功位/拒绝码）
    {
        let mut p = ValParser::new();
        let _ = p.parse_int(b"12x4");
        let ok = p.row_at(0)
            .map(|r| r.ty == 1 && r.ok == 0 && r.code == XC_INT_DIGIT as u32)
            .unwrap_or(false);
        s.add("C585-06-解析账三列", ok, if ok { "" } else { "账列缺失" });
    }

    // C585-07 注入 10/10 全抓
    {
        let ok = inject_probe() == Ok(true);
        s.add("C585-07-注入全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    s
}
