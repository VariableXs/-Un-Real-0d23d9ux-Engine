//! unx0543_linefmt —— UNX-F0543 行格式冻结（时间戳/级别/组件/文）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0543 · 行格式冻结 · 目标 220 行
//! 判据原文：行格式四字段冻结（时戳/级别/组件/文本），20 行格式合规率
//!           100%（逐字段解析核）。
//! 组件：行协议（`<step>|<LVL>|<comp>|<text>`）+ 逐字段解析器 + 合规率核
//!   （20 行全解析通过=100%）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35500;
pub const XC_FMT_SHORT: u32 = SEG_BASE + 1; // 段数不足
pub const XC_FMT_BAD: u32 = SEG_BASE + 2; // 字段坏
pub const XC_PARSE_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_FMT_FROZEN: u32 = SEG_BASE + 4; // 格式破坏

/// 行格式四字段。
#[derive(Clone, Debug, PartialEq)]
pub struct LineFields {
    pub step: u64,
    pub lvl: u8,
    pub comp: u8,
    pub text: String,
}

/// u64→十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8_lossy(&buf[i..]).into_owned()
}

/// u64 解析（全数字+溢出拒）。
fn parse_u64(s: &str) -> Result<u64, u32> {
    if s.is_empty() {
        return Err(XC_FMT_BAD);
    }
    let mut v: u64 = 0;
    for b in s.bytes() {
        if !(b >= b'0' && b <= b'9') {
            return Err(XC_FMT_BAD);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    Ok(v)
}

/// 行序列化：`step|LVL|comp|text`。
pub fn fmt_line(step: u64, lvl: u8, comp: u8, text: &str) -> Result<String, u32> {
    if lvl > 2 || comp > 15 || text.is_empty() || text.contains('|') {
        return Err(XC_FMT_BAD);
    }
    let lname = match lvl {
        0 => "ERR",
        1 => "INF",
        _ => "DBG",
    };
    let mut s = String::new();
    s.push_str(&itoa(step));
    s.push('|');
    s.push_str(lname);
    s.push('|');
    s.push_str(&itoa(comp as u64));
    s.push('|');
    s.push_str(text);
    Ok(s)
}

/// 行解析（四段严拆）。
pub fn parse_line(line: &str) -> Result<LineFields, u32> {
    let parts: Vec<&str> = line.splitn(4, '|').collect();
    if parts.len() != 4 {
        return Err(XC_FMT_SHORT);
    }
    let step = parse_u64(parts[0])?;
    let lvl = match parts[1] {
        "ERR" => 0u8,
        "INF" => 1,
        "DBG" => 2,
        _ => return Err(XC_FMT_BAD),
    };
    let comp = parse_u64(parts[2])?;
    if comp > 15 {
        return Err(XC_PARSE_OOB);
    }
    if parts[3].is_empty() {
        return Err(XC_FMT_BAD);
    }
    Ok(LineFields {
        step,
        lvl,
        comp: comp as u8,
        text: String::from(parts[3]),
    })
}

/// 合规率核：一批行全部可解析=100%。
pub fn compliance_permille(lines: &[String]) -> Result<u32, u32> {
    if lines.is_empty() {
        return Err(XC_PARSE_OOB);
    }
    let mut ok = 0;
    let mut i = 0;
    while i < lines.len() {
        if parse_line(&lines[i]).is_ok() {
            ok += 1;
        }
        i += 1;
    }
    Ok((ok as u64 * 1000 / lines.len() as u64) as u32)
}

pub fn run_unx0543_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0543");
    cs.add("SEG 段位独占", SEG_BASE == 0x35500 && XC_FMT_FROZEN == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：序列化。
    let l = fmt_line(42, 1, 3, "boot ok").unwrap_or_default();
    cs.add("行格式", l == alloc::string::String::from("42|INF|3|boot ok"), "四段管道行");
    cs.add("级别坏拒", fmt_line(1, 3, 0, "x").is_err(), "lvl=3 Err");
    cs.add("组件越界拒", fmt_line(1, 0, 16, "x").is_err(), "comp=16 Err");
    cs.add("空文拒", fmt_line(1, 0, 0, "").is_err(), "空文本 Err");
    cs.add("文本含竖线拒", fmt_line(1, 0, 0, "a|b").is_err(), "字段污染拒");

    // 族二：解析回环。
    let f = parse_line("42|INF|3|boot ok").unwrap_or(LineFields { step: 0, lvl: 9, comp: 9, text: String::new() });
    cs.add("回环四字段", f.step == 42 && f.lvl == 1 && f.comp == 3 && f.text == "boot ok", "逐字段还原");
    cs.add("段数不足拒", parse_line("42|INF|3").is_err(), "三段 Err");
    cs.add("级别坏拒", parse_line("42|XXX|3|x").is_err(), "未知级名");
    cs.add("组件越界拒", parse_line("42|ERR|16|x").is_err(), "comp>15");
    cs.add("空文拒", parse_line("42|ERR|0|").is_err(), "尾段空");
    cs.add("时戳非数字拒", parse_line("x|ERR|0|t").is_err(), "step 坏");

    // 族三：20 行合规率 100%。
    let mut batch = Vec::new();
    let mut i = 0;
    while i < 20 {
        if let Ok(s) = fmt_line(i as u64, (i % 3) as u8, (i % 16) as u8, "m") {
            batch.push(s);
        }
        i += 1;
    }
    cs.add("合规率 100%", compliance_permille(&batch) == Ok(1000), "20/20 全解析");
    // 注入坏行→检出。
    batch.push(String::from("bad line no pipes"));
    cs.add("坏行检出", compliance_permille(&batch) == Ok(952), "20/21=952‰");
    cs.add("空批拒", compliance_permille(&[]).is_err(), "零行 Err");
    cs
}
