//! unx0606_pathnorm —— UNX-F0606 路径规范化账（'./' 前缀核）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0606 路径规范化账（'./' 前缀核）  <SEG 0x4d80>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：规范化账齐（名长 ≤255 界内逐条核——'./' 前缀剥
//!   离+双斜线归一），异常路径（'..' 与绝对路径）检出 100%。
//!
//! 设计纪律：
//!   1. 规范化三规则（写死）：① 剥离前导 "./"（可重复：././a →
//      a）；② 连续 '/' 归一为单 '/'；③ 末尾 '/' 剥离（根 "/" 除
//      外——根输入即红，cpio 条目名不允许绝对路径）；
//   2. 异常路径检出 100%：含 ".." 段 → 红；以 '/' 开头（绝对）→
//      红；空名 → 红；名长 > 255 → 红（判据界）；
//   3. 名长账：规范化后名长 ≤255 逐条核，账注（原长/净长）；
//   4. 规范化幂等：norm(norm(x)) == norm(x)（账内断言）；
//   5. 批量账：逐条目规范化 + 异常检出账，守恒：入账数 = 平账数
//      + 异常数；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4d80 段为本模块独占（SEG_BASE=0x4d80），判别式
//   SEG_BASE+1..+N；与 0x4d60/0x4da0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PN_VERSION: &str = "unx0606_pathnorm-v1";

/// 诊断段基址（0x4d80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4D80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4d81 空名。
pub const XC_EMPTY_NAME: u16 = SEG_BASE + 1;
/// 0x4d82 绝对路径（前导 '/'）。
pub const XC_ABSOLUTE: u16 = SEG_BASE + 2;
/// 0x4d83 父上溯段（".." 出现）。
pub const XC_DOTDOT: u16 = SEG_BASE + 3;
/// 0x4d84 名长越界（>255）。
pub const XC_NAME_LONG: u16 = SEG_BASE + 5 - 1; // 0x4d84
/// 0x4d85 规范化不幂等。
pub const XC_NOT_IDEMPOTENT: u16 = SEG_BASE + 5;
/// 0x4d86 批账不守恒。
pub const XC_BATCH_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x4d87 非法字节（路径含控制字符 <0x20）。
pub const XC_BAD_CHAR: u16 = SEG_BASE + 7;

/// 名长界（写死：255）。
pub const NAME_MAX: usize = 255;

/// 规范化主函数：三规则 + 异常检出。
/// 返回净名（规范化后）；异常路径红码。
pub fn normalize(raw: &str) -> Result<String, u16> {
    if raw.is_empty() {
        return Err(XC_EMPTY_NAME);
    }
    // 控制字符检出。
    for c in raw.bytes() {
        if c < 0x20 {
            return Err(XC_BAD_CHAR);
        }
    }
    // 绝对路径检出。
    if raw.starts_with('/') {
        return Err(XC_ABSOLUTE);
    }
    // ".." 段检出（段级精确匹配，前缀 "./../" 或恰 ".."）。
    let mut seg_start = 0usize;
    let rb = raw.as_bytes();
    let mut i = 0usize;
    while i <= rb.len() {
        if i == rb.len() || rb[i] == b'/' {
            let seg = &rb[seg_start..i];
            if seg == b".." {
                return Err(XC_DOTDOT);
            }
            seg_start = i + 1;
        }
        i += 1;
    }
    // 规则 1：剥离前导 "./"（可重复）。
    let mut s = raw;
    while s.starts_with("./") {
        s = &s[2..];
    }
    if s.is_empty() {
        // "./" 本身剥完为空 → 空名红。
        return Err(XC_EMPTY_NAME);
    }
    // 规则 2：连续 '/' 归一。
    let mut out = String::new();
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(c);
    }
    // 规则 3：末尾 '/' 剥离。
    while out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        return Err(XC_EMPTY_NAME);
    }
    // 名长账。
    if out.len() > NAME_MAX {
        return Err(XC_NAME_LONG);
    }
    Ok(out)
}

/// 幂等断言：norm(norm(x)) == norm(x)。
pub fn idempotent(raw: &str) -> Result<(), u16> {
    let a = normalize(raw)?;
    let b = normalize(&a)?;
    if a != b {
        return Err(XC_NOT_IDEMPOTENT);
    }
    Ok(())
}

/// 批量账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormEntry {
    /// 原名。
    pub raw: String,
    /// 净名（平账时有效）。
    pub norm: String,
    /// 异常码（0 = 平账）。
    pub red: u16,
    /// 原长。
    pub raw_len: u32,
    /// 净长。
    pub norm_len: u32,
}

/// 批量账。
#[derive(Debug, Default)]
pub struct NormBook {
    pub entries: Vec<NormEntry>,
    /// 平账数。
    pub ok_count: u32,
    /// 异常数。
    pub red_count: u32,
}

impl NormBook {
    /// 逐条目规范化入账。
    pub fn feed(&mut self, raw: &str) -> Result<(), u16> {
        let raw_len = raw.len() as u32;
        match normalize(raw) {
            Ok(n) => {
                let nl = n.len() as u32;
                if nl > NAME_MAX as u32 {
                    return Err(XC_NAME_LONG);
                }
                self.entries.push(NormEntry {
                    raw: String::from(raw),
                    norm: n,
                    red: 0,
                    raw_len,
                    norm_len: nl,
                });
                self.ok_count += 1;
            }
            Err(e) => {
                self.entries.push(NormEntry {
                    raw: String::from(raw),
                    norm: String::new(),
                    red: e,
                    raw_len,
                    norm_len: 0,
                });
                self.red_count += 1;
            }
        }
        Ok(())
    }

    /// 异常检出完整性（判据 100%）：异常码 ∈ 已知红码闭集。
    pub fn reds_legal(&self) -> Result<(), u16> {
        for e in &self.entries {
            if e.red != 0 {
                match e.red {
                    XC_EMPTY_NAME | XC_ABSOLUTE | XC_DOTDOT | XC_NAME_LONG | XC_BAD_CHAR => {}
                    _ => return Err(XC_BATCH_UNBALANCED),
                }
            }
        }
        Ok(())
    }

    /// 守恒：入账数 = 平账数 + 异常数。
    pub fn conserved(&self) -> Result<(), u16> {
        if (self.ok_count + self.red_count) as usize != self.entries.len() {
            return Err(XC_BATCH_UNBALANCED);
        }
        Ok(())
    }

    /// 平账名表。
    pub fn norm_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.red == 0)
            .map(|e| e.norm.clone())
            .collect()
    }
}

/// 自例程：'./' 前缀剥离 + 双斜线归一 + 末斜线剥离 + 异常 100%
/// 检出 + 幂等 + 批账守恒 + 错误路径。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 规则 1：'./' 前缀剥离（可重复）。
    if normalize("./a")? != "a" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    if normalize("./././a/b")? != "a/b" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    // 规则 2：双斜线归一。
    if normalize("a//b///c")? != "a/b/c" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    // 规则 3：末斜线剥离。
    if normalize("dir/")? != "dir" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    // 复合：./a//b/ → a/b。
    if normalize("./a//b/")? != "a/b" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    // 纯名不动。
    if normalize("usr/bin")? != "usr/bin" {
        return Err(XC_NOT_IDEMPOTENT);
    }
    // 幂等面（合法输入）。
    idempotent("./a//b/")?;
    idempotent("ok/path")?;

    // 异常检出 100%：空/绝对/../超长/控制字符。
    if normalize("") != Err(XC_EMPTY_NAME) {
        return Err(XC_EMPTY_NAME);
    }
    if normalize("./") != Err(XC_EMPTY_NAME) {
        return Err(XC_EMPTY_NAME);
    }
    if normalize("/etc/passwd") != Err(XC_ABSOLUTE) {
        return Err(XC_ABSOLUTE);
    }
    if normalize("a/../b") != Err(XC_DOTDOT) {
        return Err(XC_DOTDOT);
    }
    if normalize("..") != Err(XC_DOTDOT) {
        return Err(XC_DOTDOT);
    }
    if normalize("./..") != Err(XC_DOTDOT) {
        return Err(XC_DOTDOT);
    }
    let long = "L".repeat(NAME_MAX + 1);
    if normalize(&long) != Err(XC_NAME_LONG) {
        return Err(XC_NAME_LONG);
    }
    // 界内 255 合法。
    let edge = "E".repeat(NAME_MAX);
    if normalize(&edge)?.len() != NAME_MAX {
        return Err(XC_NAME_LONG);
    }
    if normalize("a\u{1}b") != Err(XC_BAD_CHAR) {
        return Err(XC_BAD_CHAR);
    }

    // 批量账。
    let mut book = NormBook::default();
    for raw in ["./a", "b//c", "/abs", "../up", "dir/", "ok"] {
        book.feed(raw)?;
    }
    book.reds_legal()?;
    book.conserved()?;
    // 平 4 异常 2。
    if book.ok_count != 4 || book.red_count != 2 {
        return Err(XC_BATCH_UNBALANCED);
    }
    let names = book.norm_names();
    if names.len() != 4 || names[0] != "a" || names[1] != "b/c" {
        return Err(XC_BATCH_UNBALANCED);
    }
    // 账注：原长/净长逐条核。
    let e0 = &book.entries[0];
    if e0.raw_len != 3 || e0.norm_len != 1 {
        return Err(XC_BATCH_UNBALANCED);
    }
    Ok(0)
}
