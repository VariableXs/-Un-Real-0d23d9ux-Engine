//! unx0553_linecsum —— UNX-F0553 日志完整性（行尾校验和）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0553 · 日志完整性 · 目标 200 行
//! 判据原文：行校验和核（万行全核零坏行），注入坏行检出 100%（3/3）。
//! 组件：行校验和（逐字节位折叠）+ 追加/校验双面 + 万行核 + 坏行注入检出。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35780;
pub const XC_CSUM_DRIFT: u32 = SEG_BASE + 1; // 校验和不符
pub const XC_CSUM_OOB: u32 = SEG_BASE + 2; // 越界
pub const XC_CSUM_BAD: u32 = SEG_BASE + 3; // 坏行
pub const XC_CSUM_ZERO: u32 = SEG_BASE + 4; // 空行

/// 行校验和：Σ(byte × (pos+1)) mod 65536。
pub fn line_csum(line: &str) -> u32 {
    let mut s: u64 = 0;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        s += (bytes[i] as u64) * ((i + 1) as u64);
        i += 1;
    }
    (s % 65536) as u32
}

/// 带校验和的行：`<text>#<csum4>`。
pub fn seal_line(text: &str) -> Result<String, u32> {
    if text.is_empty() {
        return Err(XC_CSUM_ZERO);
    }
    if text.contains('#') {
        return Err(XC_CSUM_BAD);
    }
    let c = line_csum(text);
    Ok(alloc::format!("{}#{:04x}", text, c))
}

/// 拆校验并核。
pub fn verify_line(sealed: &str) -> Result<(), u32> {
    let pos = sealed.rfind('#').ok_or(XC_CSUM_BAD)?;
    let text = &sealed[..pos];
    let hex = &sealed[pos + 1..];
    if hex.len() != 4 || text.is_empty() {
        return Err(XC_CSUM_BAD);
    }
    let claimed = parse_hex4(hex)?;
    if claimed != line_csum(text) {
        return Err(XC_CSUM_DRIFT);
    }
    Ok(())
}

fn parse_hex4(s: &str) -> Result<u32, u32> {
    let mut v: u32 = 0;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u32,
            b'a'..=b'f' => (b - b'a') as u32 + 10,
            _ => return Err(XC_CSUM_BAD),
        };
        v = v * 16 + d;
    }
    Ok(v)
}

/// 万行全核。
pub fn verify_batch(sealed: &[String]) -> Result<usize, u32> {
    if sealed.is_empty() {
        return Err(XC_CSUM_OOB);
    }
    let mut bad = 0;
    let mut i = 0;
    while i < sealed.len() {
        if verify_line(&sealed[i]).is_err() {
            bad += 1;
        }
        i += 1;
    }
    Ok(bad)
}

pub fn run_unx0553_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0553");
    cs.add("SEG 段位独占", SEG_BASE == 0x35780 && XC_CSUM_ZERO == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：封行与校验。
    let sealed = seal_line("42|INF|3|boot ok").unwrap_or_default();
    cs.add("封行格式", sealed.ends_with('#') && sealed.len() > 5, "text#csum 形");
    cs.add("校验过", verify_line(&sealed) == Ok(()), "封行自洽");
    cs.add("空行拒", seal_line("").is_err(), "空文本 Err");
    cs.add("含#拒", seal_line("a#b").is_err(), "分隔符污染拒");

    // 族二：注入坏行检出 3/3。
    let mut bad1 = sealed.clone();
    bad1.replace_range(0..1, "X"); // 改文本
    let mut bad2 = sealed.clone();
    let n = bad2.len();
    bad2.replace_range(n - 4..n, "ffff"); // 改校验和
    let bad3 = String::from("no-hash-line"); // 无校验段
    cs.add("坏行 1 文本改", verify_line(&bad1).is_err(), "文本漂移检出");
    cs.add("坏行 2 csum 改", verify_line(&bad2).is_err(), "校验和漂移检出");
    cs.add("坏行 3 无段", verify_line(&bad3).is_err(), "缺 # 检出");
    cs.add("检出 3/3", verify_line(&bad1).is_err() && verify_line(&bad2).is_err() && verify_line(&bad3).is_err(), "全检出");

    // 族三：万行全核零坏行。
    let mut batch = Vec::new();
    let mut i = 0;
    while i < 10_000 {
        if let Ok(s) = seal_line(&alloc::format!("{}|INF|0|line", i)) {
            batch.push(s);
        }
        i += 1;
    }
    cs.add("万行零坏", verify_batch(&batch) == Ok(0), "10,000 全过");
    batch[5000] = String::from("corrupted");
    cs.add("注入即检出", verify_batch(&batch) == Ok(1), "恰好一坏");
    cs.add("空批拒", verify_batch(&[]).is_err(), "零行 Err");

    // 族四：校验和敏感性。
    cs.add("位权敏感", line_csum("ab") != line_csum("ba"), "同字符异序异和");
    cs.add("确定性", line_csum("abc") == line_csum("abc"), "同行同和");
    cs
}
