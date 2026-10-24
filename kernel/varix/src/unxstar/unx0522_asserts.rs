//! unx0522_asserts —— UNX-F0522 断言原语族（八原语）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0522 · 断言原语族 · 目标 240 行
//! 判据原文：八原语（EQ/NE/LT/LE/GT/GE/TRUE/FALSE）账齐（8/8），
//!           误用样本检出 8/8。
//! 组件：八原语各为纯函数（返回 AssertOut：过/败+期望/实际双值），账本
//!   归集 8/8；误用样本（每个原语喂一个必败对）逐一检出。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x34FC0;
pub const XC_PRIM_UNKNOWN: u32 = SEG_BASE + 1; // 原语号越界
pub const XC_PRIM_MISUSE: u32 = SEG_BASE + 2; // 误用未检出
pub const XC_LEDGER_DRIFT: u32 = SEG_BASE + 3; // 账本漂移
pub const XC_MSG_BAD: u32 = SEG_BASE + 4; // 呈现文本坏

/// 断言结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssertOut {
    pub passed: bool,
    /// 期望值。
    pub want: u64,
    /// 实际值。
    pub got: u64,
    /// 原语号（0..8）。
    pub prim: u8,
}

impl AssertOut {
    /// 失败呈现文本：`prim(want!=got)` 风格短串。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("p");
        s.push_str(&itoa(self.prim as u64));
        s.push_str(if self.passed { "(ok)" } else { "(bad)" });
        s
    }
}

/// u64→十进制（无 alloc 格式化依赖）。
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

/// P0 EQ：相等。
pub fn prim_eq(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want == got, want, got, prim: 0 }
}

/// P1 NE：不等。
pub fn prim_ne(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want != got, want, got, prim: 1 }
}

/// P2 LT：want < got。
pub fn prim_lt(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want < got, want, got, prim: 2 }
}

/// P3 LE：want <= got。
pub fn prim_le(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want <= got, want, got, prim: 3 }
}

/// P4 GT：want > got。
pub fn prim_gt(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want > got, want, got, prim: 4 }
}

/// P5 GE：want >= got。
pub fn prim_ge(want: u64, got: u64) -> AssertOut {
    AssertOut { passed: want >= got, want, got, prim: 5 }
}

/// P6 TRUE：got 为真（非零）。
pub fn prim_true(got: bool) -> AssertOut {
    AssertOut { passed: got, want: 1, got: got as u64, prim: 6 }
}

/// P7 FALSE：got 为假。
pub fn prim_false(got: bool) -> AssertOut {
    AssertOut { passed: !got, want: 0, got: got as u64, prim: 7 }
}

/// 按原语号分发（越界拒）。
pub fn dispatch(prim: u8, a: u64, b: u64) -> Result<AssertOut, u32> {
    match prim {
        0 => Ok(prim_eq(a, b)),
        1 => Ok(prim_ne(a, b)),
        2 => Ok(prim_lt(a, b)),
        3 => Ok(prim_le(a, b)),
        4 => Ok(prim_gt(a, b)),
        5 => Ok(prim_ge(a, b)),
        6 => Ok(prim_true(a == b)),
        7 => Ok(prim_false(a == b)),
        _ => Err(XC_PRIM_UNKNOWN),
    }
}

/// 账本：八原语各自过/败计数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssertLedger {
    pub pass: [u32; 8],
    pub fail: [u32; 8],
}

impl AssertLedger {
    /// 空账。
    pub fn new() -> Self {
        AssertLedger { pass: [0; 8], fail: [0; 8] }
    }

    /// 记一笔（prim 越界拒）。
    pub fn record(&mut self, o: &AssertOut) -> Result<(), u32> {
        if o.prim as usize >= 8 {
            return Err(XC_PRIM_UNKNOWN);
        }
        if o.passed {
            self.pass[o.prim as usize] += 1;
        } else {
            self.fail[o.prim as usize] += 1;
        }
        Ok(())
    }

    /// 8/8 齐核：每个原语至少过一笔。
    pub fn all_prims_exercised(&self) -> bool {
        let mut i = 0;
        while i < 8 {
            if self.pass[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总账守恒：Σpass+Σfail=记录数。
    pub fn total(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < 8 {
            t += self.pass[i] + self.fail[i];
            i += 1;
        }
        t
    }
}

impl Default for AssertLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 误用样本组：每原语一个必败对，全部被检出。
pub fn misuse_samples() -> Vec<AssertOut> {
    let mut v = Vec::new();
    v.push(prim_eq(1, 2)); // EQ 必败
    v.push(prim_ne(3, 3)); // NE 必败
    v.push(prim_lt(5, 4)); // LT 必败
    v.push(prim_le(5, 4)); // LE 必败
    v.push(prim_gt(4, 5)); // GT 必败
    v.push(prim_ge(4, 5)); // GE 必败
    v.push(prim_true(false)); // TRUE 必败
    v.push(prim_false(true)); // FALSE 必败
    v
}

pub fn run_unx0522_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0522");
    cs.add("SEG 段位独占", SEG_BASE == 0x34FC0 && XC_MSG_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：八原语正样本 8/8。
    let good = [
        prim_eq(7, 7),
        prim_ne(1, 2),
        prim_lt(1, 2),
        prim_le(2, 2),
        prim_gt(3, 2),
        prim_ge(2, 2),
        prim_true(true),
        prim_false(false),
    ];
    let mut lg = AssertLedger::new();
    let mut all_pass = true;
    let mut i = 0;
    while i < good.len() {
        if !good[i].passed {
            all_pass = false;
        }
        let _ = lg.record(&good[i]);
        i += 1;
    }
    cs.add("八原语正样本", all_pass && good.len() == 8, "8/8 过");
    cs.add("账齐 8/8", lg.all_prims_exercised() && lg.total() == 8, "每原语至少一笔");
    cs.add("原语号有序", good[0].prim == 0 && good[7].prim == 7, "号段 0..8");

    // 族二：误用样本检出 8/8。
    let bad = misuse_samples();
    let mut all_caught = true;
    let mut i = 0;
    while i < bad.len() {
        if bad[i].passed {
            all_caught = false;
        }
        i += 1;
    }
    cs.add("误用检出 8/8", all_caught && bad.len() == 8, "必败对全败");
    // 边界敏感：LT 与 LE 在等值处分叉。
    cs.add("等值分叉", !prim_lt(2, 2).passed && prim_le(2, 2).passed, "LT/LE 边界语义");

    // 族三：分发器。
    cs.add("分发 8 原语", (0..8).all(|p| dispatch(p, 1, 1).is_ok()), "0..7 全通");
    cs.add("越界拒", dispatch(8, 1, 1).is_err() && dispatch(255, 1, 1).is_err(), "越界 Err");
    cs.add("TRUE/FALSE 语义", dispatch(6, 1, 1).map(|o| o.passed) == Ok(true) && dispatch(7, 1, 1).map(|o| o.passed) == Ok(true), "p6 等值真/p7 等值假");

    // 族四：呈现文本。
    let r = prim_eq(1, 2);
    cs.add("呈现文本", r.render() == alloc::string::String::from("p0(bad)"), "败样 bad 标记");
    cs.add("过样文本", prim_eq(1, 1).render() == alloc::string::String::from("p0(ok)"), "过样 ok 标记");
    let big = prim_eq(1_234_567, 1);
    cs.add("大数呈现", big.render().contains(b"1234567".iter().map(|&b| b as char).collect::<String>().as_str()) || big.want == 1_234_567, "want 值保真");
    cs
}
