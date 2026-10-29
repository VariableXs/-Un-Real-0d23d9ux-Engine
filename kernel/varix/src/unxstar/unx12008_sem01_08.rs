//! UNX-F12008 RTL_USER_PROCESS_PARAMETERS 进程参数块语义（0x19320 段）
//!
//! 判据原文：进程参数块的字符串字段（CommandLine/ImagePathName/
//! WindowTitle/DesktopInfo）遵循 UNICODE_STRING 约定——Length 以字节计
//! 且必须为偶数（UTF-16），Length ≤ MaximumLength，非空 Length 必须挂
//! 非零缓冲指针，缓冲指针必须 2 字节对齐；空串合法（Length=0 且缓冲
//! 可为 0）。
//!
//! 结构：参数块（ParBlock：五字符串字段槽 + 参数区尺寸账）+ 字段装配核
//! （五码守卫）+ 装配账（ParRec 逐字段留痕）+ 折叠指纹 + 印账冻结 +
//! 注入核 + 判据聚合（run_unx12008_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19320 独占 0x20）。
pub const SEG_BASE: u32 = 0x19320;
pub const PP_LEN_GT_CAP: u32 = SEG_BASE + 0x01; // Length > MaximumLength
pub const PP_ODD_LEN: u32 = SEG_BASE + 0x02; // 非偶字节长（非 UTF-16）
pub const PP_NULL_BUF: u32 = SEG_BASE + 0x03; // 非空长却挂零缓冲
pub const PP_UNALIGNED: u32 = SEG_BASE + 0x04; // 缓冲非 2 字节对齐
pub const PP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍装配

/// 参数块字符串字段闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParField {
    ImagePathName = 1,
    CommandLine = 2,
    WindowTitle = 3,
    DesktopInfo = 4,
    ShellInfo = 5,
}

/// 一笔字段装配的账目。
#[derive(Clone)]
pub struct ParRec {
    pub seq: u32,
    pub field: ParField,
    pub len: u16,
    pub cap: u16,
    pub buf: u64,
    pub note: String,
}

impl ParRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.len <= self.cap
    }
}

/// 参数块模型。
pub struct ParBlock {
    pub slots: Vec<(ParField, u16, u16, u64)>, // (field, len, cap, buf)
    pub ledger: Vec<ParRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ParBlock {
    pub fn new() -> ParBlock {
        ParBlock { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 字段装配核：五码守卫后落账。
    pub fn set(
        &mut self,
        f: ParField,
        len: u16,
        cap: u16,
        buf: u64,
        note: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_FROZEN);
        }
        if note.is_empty() {
            return Err(PP_LEN_GT_CAP);
        }
        if len > cap {
            return Err(PP_LEN_GT_CAP);
        }
        if len & 1 != 0 {
            return Err(PP_ODD_LEN);
        }
        if len > 0 && buf == 0 {
            return Err(PP_NULL_BUF);
        }
        if buf != 0 && buf & 1 != 0 {
            return Err(PP_UNALIGNED);
        }
        let rec = ParRec { seq: self.seq_next, field: f, len, cap, buf, note: note.to_string() };
        if !rec.ok() {
            return Err(PP_LEN_GT_CAP);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        if let Some(slot) = self.slots.iter_mut().find(|s| s.0 == f) {
            slot.1 = len;
            slot.2 = cap;
            slot.3 = buf;
        } else {
            self.slots.push((f, len, cap, buf));
        }
        Ok(())
    }

    /// 字段读取（len, cap, buf）。
    pub fn get(&self, f: ParField) -> Option<(u16, u16, u64)> {
        self.slots.iter().find(|s| s.0 == f).map(|s| (s.1, s.2, s.3))
    }

    /// 空串判定：字段已装配且 Length=0。
    pub fn is_empty_str(&self, f: ParField) -> bool {
        self.get(f).map(|(l, _, _)| l == 0).unwrap_or(false)
    }

    /// 空串合法核对：Length=0 时缓冲允许为零。
    pub fn empty_with_null_buf_ok(&self, f: ParField) -> bool {
        match self.get(f) {
            Some((l, _, b)) => l == 0 && b == 0,
            None => false,
        }
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 槽账对平：账条数 == 槽数。
    pub fn slots_tally(&self) -> bool {
        self.slots.len() == self.ledger.len()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(s.1 as u32);
            h = h.wrapping_mul(211).wrapping_add(s.2 as u32);
            h = h.wrapping_mul(211).wrapping_add((s.3 & 0xFFFF) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 诊断面：参数块摘要。
    pub fn summary(&self) -> String {
        format!("fields={}|recs={}", self.slots.len(), self.ledger.len())
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = ParBlock::new();
    // 1) Length > MaximumLength → PP_LEN_GT_CAP。
    got[0] = p.set(ParField::CommandLine, 9, 8, 0x1000, "gt").err() == Some(PP_LEN_GT_CAP);
    // 2) 奇数字节长 → PP_ODD_LEN。
    got[1] = p.set(ParField::CommandLine, 7, 8, 0x1000, "odd").err() == Some(PP_ODD_LEN);
    // 3) 非空长挂零缓冲 → PP_NULL_BUF。
    got[2] = p.set(ParField::CommandLine, 8, 8, 0, "nb").err() == Some(PP_NULL_BUF);
    // 4) 缓冲非 2 字节对齐 → PP_UNALIGNED。
    got[3] =
        p.set(ParField::CommandLine, 8, 8, 0x1001, "ua").err() == Some(PP_UNALIGNED);
    // 5) 印账后仍装配 → PP_FROZEN。
    let _ = p.set(ParField::ImagePathName, 2, 4, 0x2000, "img");
    p.seal();
    got[4] = p.set(ParField::WindowTitle, 2, 4, 0x3000, "post").err() == Some(PP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12008");

    // J1 正路径：五字段装配回读一致。
    let j1 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::ImagePathName, 44, 64, 0x1000, "img");
        let _ = p.set(ParField::CommandLine, 22, 32, 0x1100, "cmd");
        let _ = p.set(ParField::WindowTitle, 0, 16, 0, "title");
        p.get(ParField::ImagePathName) == Some((44, 64, 0x1000))
            && p.get(ParField::CommandLine) == Some((22, 32, 0x1100))
            && p.slots_tally()
    };
    s.add("J1-五字段装配", j1, "三字段装配对账");

    // J2 长超容拒。
    let j2 = {
        let mut p = ParBlock::new();
        p.set(ParField::CommandLine, 10, 8, 0x1000, "g").err() == Some(PP_LEN_GT_CAP)
    };
    s.add("J2-长超容拒", j2, "Length≤MaximumLength");

    // J3 奇数长拒。
    let j3 = {
        let mut p = ParBlock::new();
        p.set(ParField::CommandLine, 3, 8, 0x1000, "o").err() == Some(PP_ODD_LEN)
    };
    s.add("J3-奇长拒", j3, "UTF-16 偶字节");

    // J4 非空长零缓冲拒。
    let j4 = {
        let mut p = ParBlock::new();
        p.set(ParField::CommandLine, 8, 8, 0, "n").err() == Some(PP_NULL_BUF)
    };
    s.add("J4-零缓冲拒", j4, "非空长必挂缓冲");

    // J5 缓冲对齐：奇地址拒、偶地址收。
    let j5 = {
        let mut p = ParBlock::new();
        let bad = p.set(ParField::DesktopInfo, 4, 8, 0x1001, "u").err() == Some(PP_UNALIGNED);
        let good = p.set(ParField::DesktopInfo, 4, 8, 0x1000, "a").is_ok();
        bad && good
    };
    s.add("J5-缓冲对齐", j5, "2 字节对齐约束");

    // J6 空串合法：Length=0 且缓冲=0 收下。
    let j6 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::ShellInfo, 0, 8, 0, "empty");
        p.is_empty_str(ParField::ShellInfo)
            && p.empty_with_null_buf_ok(ParField::ShellInfo)
    };
    s.add("J6-空串合法", j6, "零长零缓冲收下");

    // J7 账目留痕：逐笔合法且 seq 递增。
    let j7 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::ImagePathName, 4, 8, 0x1000, "a");
        let _ = p.set(ParField::CommandLine, 6, 8, 0x1100, "b");
        p.ledger.len() == 2 && p.ledger_ok()
            && p.ledger[0].seq == 1 && p.ledger[1].seq == 2
    };
    s.add("J7-账目留痕", j7, "seq 递增对账");

    // J8 同字段覆写：槽值随最后一次装配。
    let j8 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::WindowTitle, 4, 8, 0x1000, "a");
        let _ = p.set(ParField::WindowTitle, 6, 8, 0x1200, "b");
        p.get(ParField::WindowTitle) == Some((6, 8, 0x1200)) && p.slots.len() == 1
    };
    s.add("J8-覆写取末", j8, "槽值取最后装配");

    // J9 印账冻结。
    let j9 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::CommandLine, 2, 4, 0x1000, "a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2
            && p.set(ParField::CommandLine, 4, 4, 0x1000, "post").err() == Some(PP_FROZEN)
    };
    s.add("J9-指纹封账", j9, "指纹幂等且印后必拒");

    // J10 摘要面。
    let j10 = {
        let mut p = ParBlock::new();
        let _ = p.set(ParField::CommandLine, 2, 4, 0x1000, "a");
        let t = p.summary();
        t.contains("fields=1") && t.contains("recs=1")
    };
    s.add("J10-摘要面", j10, "摘要含字段与账数");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：典型 cmd 启动参数块。
    let mut p = ParBlock::new();
    let _ = p.set(ParField::ImagePathName, 28, 32, 0x4000, "img");
    let _ = p.set(ParField::CommandLine, 40, 48, 0x4100, "cmd");
    let _ = p.set(ParField::DesktopInfo, 18, 24, 0x4200, "desk");
    s.add("样例三槽对平", p.slots_tally(), "槽账同数");
    s.add("样例账面合法", p.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12008_param_flow() {
        let mut p = ParBlock::new();
        assert!(p.set(ParField::CommandLine, 10, 12, 0x1000, "a").is_ok());
        assert_eq!(p.set(ParField::CommandLine, 13, 12, 0x1000, "b"), Err(PP_LEN_GT_CAP));
        assert_eq!(p.set(ParField::CommandLine, 11, 12, 0x1000, "c"), Err(PP_ODD_LEN));
        assert_eq!(p.get(ParField::CommandLine), Some((10, 12, 0x1000)));
    }

    #[test]
    fn unx12008_checks_green() {
        let s = run_unx12008_checks();
        assert!(s.all_passed(), "UNX-F12008 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12008_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
