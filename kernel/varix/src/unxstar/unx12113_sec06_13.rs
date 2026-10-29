//! UNX-F12113 RtlStringCbPrintfW 类安全格式化预告（0x1a040 段）
//!
//! 卡面：安全格式化——格式串语法闭集（%s/%u/%x/%c/%%）、未知指示符拒、
//! 输出容量预检（截断拒）、参数个数与指示符个数对平。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 指示符闭集：仅 %s/%u/%x/%c/%% 五种，未知指示符必拒。
//! - J2 参数对平：参数个数须恰等指示符个数（%% 不耗参）。
//! - J3 容量预检：预估输出长（含 NUL 位）越目标容必拒。
//! - J4 目标在场：未登记目标必拒。
//! - J5 正路径：合法格式串 + 足参 + 足容时输出长入账。
//! - J6 印账冻结：seal 后格式化必拒。
//! - J7 注入五况全抓。
//!
//! 结构：FmtDest 目标档 + FmtBook 目标册 + 解析核（指示符闭集 +
//! 参数对平 + 容量预估）+ 折叠指纹（211 进制卷积）+
//! 注入核（未知指示符/缺参/多参/容不足/印后格式化）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a040 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a040;
pub const FM_UNKNOWN_SPEC: u32 = SEG_BASE + 0x01; // 未知指示符
pub const FM_ARG_MISMATCH: u32 = SEG_BASE + 0x02; // 参数个数失配
pub const FM_NO_ROOM: u32 = SEG_BASE + 0x03; // 容不足
pub const FM_NO_DEST: u32 = SEG_BASE + 0x04; // 无目标
pub const FM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后格式化

/// 一次格式化的参数值。
#[derive(Clone, Copy)]
pub enum FmtArg {
    Str(u32),  // 字符串（以字节长记账）
    Uint(u32), // 无符号整型
    Hex(u32),  // 十六进制
    Char(u16), // 单字符
}

impl FmtArg {
    /// 预估输出字节数（UTF-16 字节口径）。
    pub fn out_bytes(&self) -> u32 {
        match self {
            FmtArg::Str(n) => *n,
            FmtArg::Uint(v) => digits10(*v) * 2,
            FmtArg::Hex(v) => hex_digits(*v) * 2,
            FmtArg::Char(_) => 2,
        }
    }
}

/// 十进制位数。
pub fn digits10(mut v: u32) -> u32 {
    if v == 0 {
        return 1;
    }
    let mut n = 0;
    while v > 0 {
        n += 1;
        v /= 10;
    }
    n
}

/// 十六进制位数。
pub fn hex_digits(mut v: u32) -> u32 {
    if v == 0 {
        return 1;
    }
    let mut n = 0;
    while v > 0 {
        n += 1;
        v >>= 4;
    }
    n
}

/// 解析核：指示符闭集 + 参数对平 + 输出长预估（含 NUL 位 2）。
/// 格式串以 UTF-16 单元序列给定；'%' 后必须跟合法指示符。
pub fn fmt_check(fmt: &[u16], args: &[FmtArg]) -> Result<u32, u32> {
    let mut need: u32 = 0;
    let mut specs: u32 = 0;
    let mut i = 0usize;
    while i < fmt.len() {
        if fmt[i] == 0x25 {
            // '%' — 必须有后继指示符
            if i + 1 >= fmt.len() {
                return Err(FM_UNKNOWN_SPEC);
            }
            match fmt[i + 1] {
                0x25 => {
                    // %% 字面 %，不耗参
                    need += 2;
                }
                0x73 => {
                    // %s
                    let a = args.get(specs as usize).ok_or(FM_ARG_MISMATCH)?;
                    if !matches!(a, FmtArg::Str(_)) {
                        return Err(FM_ARG_MISMATCH);
                    }
                    need += a.out_bytes();
                    specs += 1;
                }
                0x75 => {
                    // %u
                    let a = args.get(specs as usize).ok_or(FM_ARG_MISMATCH)?;
                    if !matches!(a, FmtArg::Uint(_)) {
                        return Err(FM_ARG_MISMATCH);
                    }
                    need += a.out_bytes();
                    specs += 1;
                }
                0x78 => {
                    // %x
                    let a = args.get(specs as usize).ok_or(FM_ARG_MISMATCH)?;
                    if !matches!(a, FmtArg::Hex(_)) {
                        return Err(FM_ARG_MISMATCH);
                    }
                    need += a.out_bytes();
                    specs += 1;
                }
                0x63 => {
                    // %c
                    let a = args.get(specs as usize).ok_or(FM_ARG_MISMATCH)?;
                    if !matches!(a, FmtArg::Char(_)) {
                        return Err(FM_ARG_MISMATCH);
                    }
                    need += a.out_bytes();
                    specs += 1;
                }
                _ => return Err(FM_UNKNOWN_SPEC),
            }
            i += 2;
        } else {
            need += 2;
            i += 1;
        }
    }
    if specs != args.len() as u32 {
        return Err(FM_ARG_MISMATCH);
    }
    Ok(need + 2) // 含 NUL 位
}

/// 格式化目标档。
#[derive(Clone, Copy)]
pub struct FmtDest {
    pub id: u32,
    pub max: u32,
    pub len: u32,
}

/// 目标册。
pub struct FmtBook {
    pub dests: Vec<FmtDest>,
    pub sealed: bool,
}

impl FmtBook {
    pub fn new() -> FmtBook {
        FmtBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FM_FROZEN);
        }
        if id == 0 {
            return Err(FM_NO_DEST);
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(FM_NO_DEST);
        }
        self.dests.push(FmtDest { id, max, len: 0 });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 格式化核：解析 → 目标在场 → 容足 → 长度入账。
    pub fn printf(&mut self, id: u32, fmt: &[u16], args: &[FmtArg]) -> Result<u32, u32> {
        if self.sealed {
            return Err(FM_FROZEN);
        }
        let need = fmt_check(fmt, args)?;
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(FM_NO_DEST),
        };
        if need > self.dests[i].max {
            return Err(FM_NO_ROOM);
        }
        self.dests[i].len = need - 2;
        Ok(need)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.dests {
            h = h.wrapping_mul(211).wrapping_add(d.id);
            h = h.wrapping_mul(211).wrapping_add(d.max);
            h = h.wrapping_mul(211).wrapping_add(d.len);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("dests={}", self.dests.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.dests.iter().find(|d| d.id == id) {
            Some(d) => format!("W{}|{}+{}", d.id, d.len, d.max),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = FmtBook::new();
    let _ = b.add_dest(4, 64);
    let pct = 0x25u16;
    // 1) 未知指示符必拒（%q）。
    got[0] = fmt_check(&[pct, 0x71], &[]).err() == Some(FM_UNKNOWN_SPEC);
    // 2) 缺参必拒（%s 无参）。
    got[1] = fmt_check(&[pct, 0x73], &[]).err() == Some(FM_ARG_MISMATCH);
    // 3) 多参必拒。
    got[2] = fmt_check(&[0x41], &[FmtArg::Uint(1)]).err() == Some(FM_ARG_MISMATCH);
    // 4) 容不足必拒。
    let _ = b.add_dest(5, 4);
    got[3] = b.printf(5, &[0x41, 0x42, 0x43, 0x44], &[]).err() == Some(FM_NO_ROOM);
    // 5) 印账后格式化必抓。
    b.seal();
    got[4] = b.printf(4, &[0x41], &[]).err() == Some(FM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12113");
    let pct = 0x25u16;

    // J1 指示符闭集：五种皆过、%q 拒。
    let j1 = fmt_check(&[pct, 0x73], &[FmtArg::Str(4)]).is_ok()
        && fmt_check(&[pct, 0x75], &[FmtArg::Uint(9)]).is_ok()
        && fmt_check(&[pct, 0x78], &[FmtArg::Hex(9)]).is_ok()
        && fmt_check(&[pct, 0x63], &[FmtArg::Char(0x41)]).is_ok()
        && fmt_check(&[pct, 0x25], &[]).is_ok()
        && fmt_check(&[pct, 0x71], &[]).err() == Some(FM_UNKNOWN_SPEC);
    s.add("J1-指示符闭集", j1, "s/u/x/c/% 五过一拒");

    // J2 参数对平：缺参与多参皆拒。
    let j2 = fmt_check(&[pct, 0x75, pct, 0x75], &[FmtArg::Uint(1)])
        .err() == Some(FM_ARG_MISMATCH)
        && fmt_check(&[0x41], &[FmtArg::Uint(1)]).err() == Some(FM_ARG_MISMATCH);
    s.add("J2-参数对平", j2, "缺多皆拒");

    // J3 类型失配拒（%s 配整数）。
    let j3 = fmt_check(&[pct, 0x73], &[FmtArg::Uint(1)])
        .err() == Some(FM_ARG_MISMATCH);
    s.add("J3-类型失配拒", j3, "指示符与参型须对");

    // J4 容量预估口径。
    let j4 = digits10(0) == 1
        && digits10(99999) == 5
        && hex_digits(0) == 1
        && hex_digits(0xFFFF) == 4;
    s.add("J4-容量预估", j4, "位数函数口径");

    // J5 尾随孤立 % 拒。
    let j5 = fmt_check(&[0x41, pct], &[]).err() == Some(FM_UNKNOWN_SPEC);
    s.add("J5-尾随%拒", j5, "无指示符必拒");

    // J6 目标容足放行。
    let j6 = {
        let mut b = FmtBook::new();
        let _ = b.add_dest(1, 32);
        let need = b.printf(1, &[0x41, pct, 0x75], &[FmtArg::Uint(123)])
            .unwrap_or(0);
        need == 10 && b.dests[0].len == 8
    };
    s.add("J6-容足放行", j6, "need=10、len=8");

    // J7 容不足拒。
    let j7 = {
        let mut b = FmtBook::new();
        let _ = b.add_dest(1, 6);
        b.printf(1, &[0x41, 0x42, 0x43], &[]).err() == Some(FM_NO_ROOM)
    };
    s.add("J7-容不足拒", j7, "need 8 > max 6 必拒");

    // J8 无目标拒。
    let j8 = {
        let mut b = FmtBook::new();
        b.printf(9, &[0x41], &[]).err() == Some(FM_NO_DEST)
    };
    s.add("J8-无目标拒", j8, "未登记必拒");

    // J9 回读渲染。
    let j9 = {
        let mut b = FmtBook::new();
        let _ = b.add_dest(3, 32);
        b.line_of(3).contains("W3|0+32") && b.line_of(4) == "missing"
    };
    s.add("J9-回读渲染", j9, "目标行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut b = FmtBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.printf(1, &[0x41], &[]).err() == Some(FM_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = FmtBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = FmtBook::new();
    let _ = b.add_dest(1, 64);
    let _ = b.printf(1, &[pct, 0x73, 0x3A, pct, 0x78], &[FmtArg::Str(6), FmtArg::Hex(0x2F)]);

    s.add("摘要回读", b.summary().contains("dests=1"), "单目标在册");
    s.add("长度回读", b.dests[0].len == 12, "6+1+4+1=12");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12113_fmt_flow() {
        let pct = 0x25u16;
        assert!(fmt_check(&[pct, 0x75], &[FmtArg::Uint(42)]).is_ok());
        assert_eq!(fmt_check(&[pct, 0x71], &[]), Err(FM_UNKNOWN_SPEC));
        assert_eq!(digits10(1234), 4);
        assert_eq!(hex_digits(0x1234), 4);
    }

    #[test]
    fn unx12113_checks_green() {
        let s = run_unx12113_checks();
        assert!(s.all_passed(), "UNX-F12113 判据未全绿");
    }

    #[test]
    fn book_printf_flow() {
        let mut b = FmtBook::new();
        let _ = b.add_dest(1, 16);
        assert_eq!(b.printf(1, &[0x41], &[]), Ok(4));
        assert_eq!(b.printf(9, &[0x41], &[]), Err(FM_NO_DEST));
    }
}
