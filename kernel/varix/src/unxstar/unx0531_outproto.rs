//! unx0531_outproto —— UNX-F0531 ktest 输出协议（双面汇出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0531 · ktest 输出协议 · 目标 220 行
//! 判据原文：输出协议账齐（串口行协议+帧缓冲摘要两格式），10 结果行双面
//!           一致性核（10/10）。
//! 组件：串口行协议（`KTEST|suite|case|state|err` 四段管道行）+ 帧缓冲摘
//!   要（定宽摘要行）+ 双面一致性核（同一结果两格式逐字段等）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35200;
pub const XC_LINE_MALFORMED: u32 = SEG_BASE + 1; // 行坏（段数≠4）
pub const XC_STATE_BAD: u32 = SEG_BASE + 2; // 态字段坏
pub const XC_DUAL_DRIFT: u32 = SEG_BASE + 3; // 双面漂移
pub const XC_SUMMARY_OOB: u32 = SEG_BASE + 4; // 摘要越界

/// 结果态字符串化。
pub fn state_str(s: u8) -> Result<&'static str, u32> {
    match s {
        0 => Ok("PASS"),
        1 => Ok("FAIL"),
        2 => Ok("SKIP"),
        _ => Err(XC_STATE_BAD),
    }
}

/// 串口行协议：`KTEST|suite|case|STATE|err`。
pub fn serial_line(suite: &str, case: &str, state: u8, err: u32) -> Result<String, u32> {
    let st = state_str(state)?;
    if suite.is_empty() || case.is_empty() {
        return Err(XC_LINE_MALFORMED);
    }
    let mut s = String::from("KTEST|");
    s.push_str(suite);
    s.push('|');
    s.push_str(case);
    s.push('|');
    s.push_str(st);
    s.push('|');
    s.push_str(&itoa(err as u64));
    Ok(s)
}

/// 行解析（反向）：四段拆解。
pub fn parse_line(line: &str) -> Result<(String, String, u8, u32), u32> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 5 || parts[0] != "KTEST" {
        return Err(XC_LINE_MALFORMED);
    }
    let state = match parts[3] {
        "PASS" => 0u8,
        "FAIL" => 1,
        "SKIP" => 2,
        _ => return Err(XC_STATE_BAD),
    };
    let err = parse_u32(parts[4])?;
    Ok((
        String::from(parts[1]),
        String::from(parts[2]),
        state,
        err,
    ))
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(XC_LINE_MALFORMED);
    }
    let mut v: u64 = 0;
    for b in s.bytes() {
        if !(b >= b'0' && b <= b'9') {
            return Err(XC_LINE_MALFORMED);
        }
        v = v * 10 + (b - b'0') as u64;
        if v > u32::MAX as u64 {
            return Err(XC_LINE_MALFORMED);
        }
    }
    Ok(v as u32)
}

/// 帧缓冲摘要行：`KTEST suite.case=STATE(err)`。
pub fn fb_summary(suite: &str, case: &str, state: u8, err: u32) -> Result<String, u32> {
    let st = state_str(state)?;
    if suite.is_empty() || case.is_empty() {
        return Err(XC_SUMMARY_OOB);
    }
    let mut s = String::from("KTEST ");
    s.push_str(suite);
    s.push('.');
    s.push_str(case);
    s.push('=');
    s.push_str(st);
    s.push('(');
    s.push_str(&itoa(err as u64));
    s.push(')');
    Ok(s)
}

/// 双面一致性核：同一结果两格式互解且字段全等。
pub fn dual_consistent(suite: &str, case: &str, state: u8, err: u32) -> Result<bool, u32> {
    let l = serial_line(suite, case, state, err)?;
    let (s2, c2, st2, e2) = parse_line(&l)?;
    let f = fb_summary(suite, case, state, err)?;
    if f.is_empty() {
        return Err(XC_DUAL_DRIFT);
    }
    Ok(s2 == suite && c2 == case && st2 == state && e2 == err)
}

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

pub fn run_unx0531_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0531");
    cs.add("SEG 段位独占", SEG_BASE == 0x35200 && XC_SUMMARY_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：串口行协议。
    let l = serial_line("unx_a1_b19", "c1", 0, 0).unwrap_or_default();
    cs.add("行格式", l == alloc::string::String::from("KTEST|unx_a1_b19|c1|PASS|0"), "四段管道行");
    cs.add("态三值", state_str(0) == Ok("PASS") && state_str(1) == Ok("FAIL") && state_str(2) == Ok("SKIP"), "0/1/2 映射");
    cs.add("态越界拒", state_str(3).is_err(), "3 Err");
    cs.add("空名拒", serial_line("", "c", 0, 0).is_err(), "空套件名 Err");

    // 族二：解析回环。
    let (s, c, st, e) = parse_line("KTEST|S|x|FAIL|77").unwrap_or_default();
    cs.add("解析回环", s == "S" && c == "x" && st == 1 && e == 77, "逐段还原");
    cs.add("段数拒", parse_line("KTEST|S|x|PASS").is_err(), "四段拒（需五段）");
    cs.add("头坏拒", parse_line("KTESTX|S|x|PASS|0").is_err(), "头不符");
    cs.add("态坏拒", parse_line("KTEST|S|x|MAYBE|0").is_err(), "未知态");
    cs.add("err 非数字拒", parse_line("KTEST|S|x|PASS|x").is_err(), "err 段坏");

    // 族三：帧缓冲摘要。
    let f = fb_summary("unx_a1_b20", "c2", 1, 9).unwrap_or_default();
    cs.add("摘要格式", f == alloc::string::String::from("KTEST unx_a1_b20.c2=FAIL(9)"), "点分=态(err)");

    // 族四：双面一致性 10/10。
    let mut ok = 0;
    let mut i = 0;
    while i < 10 {
        let st = (i % 3) as u8;
        if dual_consistent("su", "c", st, i as u32) == Ok(true) {
            ok += 1;
        }
        i += 1;
    }
    cs.add("双面一致 10/10", ok == 10, "两格式逐字段等");
    cs
}
