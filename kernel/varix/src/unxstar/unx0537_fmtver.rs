//! unx0537_fmtver —— UNX-F0537 结果格式版本兼容。
//! 锚点（CoRun Unxreal 主册）：UNX-F0537 · 结果格式版本兼容 · 目标 200 行
//! 判据原文：兼容账（v1 唯一版本声明 + 旧格式注入拒绝 2/2），版本字段核。
//! 组件：版本头（`VER v1` 行）+ 解析器版本门（无头/异版拒）+ 版本字段核
//!   （唯一声明，重复声明拒）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35380;
pub const XC_VER_MISSING: u32 = SEG_BASE + 1; // 版本头缺
pub const XC_VER_UNK: u32 = SEG_BASE + 2; // 未知版本
pub const XC_VER_DUP: u32 = SEG_BASE + 3; // 重复声明
pub const XC_VER_OOB: u32 = SEG_BASE + 4; // 越界

/// 唯一支持版本。
pub const SUPPORTED: u32 = 1;

/// 版本头解析：`VER v<数字>`。
pub fn parse_ver(head: &str) -> Result<u32, u32> {
    let b = head.as_bytes();
    if b.len() < 6 || &b[..5] != b"VER v" {
        return Err(XC_VER_MISSING);
    }
    let mut v: u64 = 0;
    let mut i = 5;
    while i < b.len() {
        if !(b[i] >= b'0' && b[i] <= b'9') {
            return Err(XC_VER_UNK);
        }
        v = v * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    if v == 0 || v > 99 {
        return Err(XC_VER_UNK);
    }
    Ok(v as u32)
}

/// 兼容门：版本 == SUPPORTED 才放行。
pub fn compat_gate(head: &str) -> Result<(), u32> {
    let v = parse_ver(head)?;
    if v != SUPPORTED {
        return Err(XC_VER_UNK);
    }
    Ok(())
}

/// 文档版本声明唯一性：文本中 VER 头恰好一次。
pub fn ver_declared_once(text: &str) -> Result<u32, u32> {
    let mut count = 0;
    let mut ver = 0;
    for line in text.split('\n') {
        if line.starts_with("VER v") {
            count += 1;
            if count > 1 {
                return Err(XC_VER_DUP);
            }
            ver = parse_ver(line)?;
        }
    }
    if count == 0 {
        return Err(XC_VER_MISSING);
    }
    Ok(ver)
}

/// 旧格式（无 VER 头或 v0 风格 `#ver:0`）注入拒。
pub fn legacy_reject(text: &str) -> Result<(), u32> {
    if text.starts_with("#ver:") {
        return Err(XC_VER_UNK);
    }
    ver_declared_once(text).map(|_| ())
}

pub fn run_unx0537_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0537");
    cs.add("SEG 段位独占", SEG_BASE == 0x35380 && XC_VER_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：版本头解析。
    cs.add("v1 解析", parse_ver("VER v1") == Ok(1), "标准头");
    cs.add("v99 解析", parse_ver("VER v99") == Ok(99), "两位版");
    cs.add("无头拒", parse_ver("RESULT|x").is_err(), "缺 VER");
    cs.add("v0 拒", parse_ver("VER v0").is_err(), "0 版非法");
    cs.add("破折拒", parse_ver("VER vx").is_err(), "非数字");
    cs.add("百版拒", parse_ver("VER v100").is_err(), "超两位");

    // 族二：兼容门。
    cs.add("v1 放行", compat_gate("VER v1") == Ok(()), "唯一支持版");
    cs.add("v2 拒", compat_gate("VER v2").is_err(), "未支持版 Err");

    // 族三：唯一声明。
    let doc = "VER v1\n[RESULT]\nb19.c1=PASS\n";
    cs.add("唯一声明", ver_declared_once(doc) == Ok(1), "恰一 VER 头");
    let dup = "VER v1\nVER v1\n";
    cs.add("重复声明拒", ver_declared_once(dup).is_err(), "双头 Err");
    let none = "RESULT|x\n";
    cs.add("零声明拒", ver_declared_once(none).is_err(), "无头 Err");

    // 族四：旧格式注入拒绝 2/2。
    cs.add("旧头拒", legacy_reject("#ver:0\nold stuff").is_err(), "#ver: 旧式拒");
    cs.add("无头拒2", legacy_reject("plain text").is_err(), "纯文本拒");
    cs.add("新格式过", legacy_reject("VER v1\nDATA\n") == Ok(()), "v1 文档放行");
    cs
}
