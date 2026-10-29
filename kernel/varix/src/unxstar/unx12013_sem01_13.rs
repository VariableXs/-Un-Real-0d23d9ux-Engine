//! UNX-F12013 KAFFINITY 进程亲和性掩码语义（0x193C0 段）
//!
//! 判据原文：亲和性掩码必须为系统处理器掩码的非空子集——零掩码即
//! 拒跑；越出系统位集的位非法；掩码一经钉死（pin）不可再改；活跃位
//! 计数须与置位面一致。
//!
//! 结构：亲和账（AffinBook：系统掩码 + 进程掩码 + 钉死位）+ 置掩核
//! （四码守卫）+ 操作账（KaRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12013_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x193C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x193C0;
pub const KA_ZERO: u32 = SEG_BASE + 0x01; // 零掩码
pub const KA_OUT_OF_SET: u32 = SEG_BASE + 0x02; // 越出系统位集
pub const KA_PINNED: u32 = SEG_BASE + 0x03; // 钉死后再改
pub const KA_SAME: u32 = SEG_BASE + 0x04; // 同值重置（空变更）
pub const KA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 一笔掩码操作的账目。
#[derive(Clone)]
pub struct KaRec {
    pub seq: u32,
    pub mask: u64,
    pub pinned: bool,
    pub note: String,
}

impl KaRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.mask != 0
    }
}

/// 亲和账模型。
pub struct AffinBook {
    /// 系统处理器掩码（上界）。
    pub sys_mask: u64,
    /// 当前进程掩码。
    pub mask: u64,
    /// 钉死位。
    pub pinned: bool,
    pub ledger: Vec<KaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AffinBook {
    pub fn new(sys_mask: u64) -> AffinBook {
        AffinBook {
            sys_mask,
            mask: sys_mask, // 初始全系统可用
            pinned: false,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 置掩核：非空 + 系统子集 + 未钉死 + 异值。
    pub fn set_mask(&mut self, mask: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KA_FROZEN);
        }
        if note.is_empty() {
            return Err(KA_ZERO);
        }
        if mask == 0 {
            return Err(KA_ZERO);
        }
        if mask & !self.sys_mask != 0 {
            return Err(KA_OUT_OF_SET);
        }
        if self.pinned {
            return Err(KA_PINNED);
        }
        if mask == self.mask {
            return Err(KA_SAME);
        }
        let rec = KaRec { seq: self.seq_next, mask, pinned: false, note: note.to_string() };
        if !rec.ok() {
            return Err(KA_ZERO);
        }
        self.mask = mask;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 钉死核：掩码在位后钉死。
    pub fn pin(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KA_FROZEN);
        }
        if note.is_empty() || self.pinned || self.mask == 0 {
            return Err(KA_PINNED);
        }
        self.pinned = true;
        self.ledger.push(KaRec {
            seq: self.seq_next,
            mask: self.mask,
            pinned: true,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 活跃位计数。
    pub fn popcount(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 子集判定复核。
    pub fn subset_of_sys(&self) -> bool {
        self.mask & !self.sys_mask == 0
    }

    /// 位查询单处理器。
    pub fn has_cpu(&self, bit: u32) -> bool {
        if bit >= 64 {
            return false;
        }
        self.mask & (1u64 << bit) != 0
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add((self.sys_mask & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add((self.mask & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add(self.pinned as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AffinBook::new(0xFF); // 8 处理器系统
    // 1) 零掩码 → KA_ZERO。
    got[0] = b.set_mask(0, "z").err() == Some(KA_ZERO);
    // 2) 越出系统位集（bit 8-15 不在 8 处理器内）→ KA_OUT_OF_SET。
    got[1] = b.set_mask(0xFF00, "o").err() == Some(KA_OUT_OF_SET);
    // 3) 钉死后再改 → KA_PINNED。
    let _ = b.set_mask(0x0F, "set");
    let _ = b.pin("p");
    got[2] = b.set_mask(0x01, "post-pin").err() == Some(KA_PINNED);
    // 4) 同值重置 → KA_SAME。
    let mut c = AffinBook::new(0xFF);
    let _ = c.set_mask(0x0C, "a");
    got[3] = c.set_mask(0x0C, "dup").err() == Some(KA_SAME);
    // 5) 印账后动作 → KA_FROZEN。
    c.seal();
    got[4] = c.set_mask(0x03, "post").err() == Some(KA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12013");

    // J1 正路径：置掩→位查询→计数一致（位 2/3/9/11）。
    let j1 = {
        let mut b = AffinBook::new(0xFFFF);
        let _ = b.set_mask(0b1010_0000_1100, "mask");
        b.popcount() == 4 && b.has_cpu(2) && b.has_cpu(3) && b.has_cpu(9) && b.has_cpu(11)
            && !b.has_cpu(1) && b.subset_of_sys()
    };
    s.add("J1-置掩计数", j1, "四活跃位对账");

    // J2 零掩码拒。
    let j2 = {
        let mut b = AffinBook::new(0xFF);
        b.set_mask(0, "z").err() == Some(KA_ZERO)
    };
    s.add("J2-零掩码拒", j2, "零掩码拒跑");

    // J3 越出系统位集拒：单比特与多比特双面。
    let j3 = {
        let mut b = AffinBook::new(0x0F);
        let hi = b.set_mask(0x10, "hi").err() == Some(KA_OUT_OF_SET);
        let far = b.set_mask(1u64 << 63, "far").err() == Some(KA_OUT_OF_SET);
        hi && far
    };
    s.add("J3-越集拒", j3, "系统外位必拒");

    // J4 钉死语义：pin 后 set 拒，pin 前 set 过。
    let j4 = {
        let mut b = AffinBook::new(0xFF);
        let _ = b.set_mask(0x03, "a");
        let _ = b.pin("p");
        b.set_mask(0x01, "b").err() == Some(KA_PINNED) && b.popcount() == 2
    };
    s.add("J4-钉死拒改", j4, "钉后不可改");

    // J5 同值重置拒。
    let j5 = {
        let mut b = AffinBook::new(0xFF);
        let _ = b.set_mask(0x05, "a");
        b.set_mask(0x05, "dup").err() == Some(KA_SAME)
    };
    s.add("J5-同值拒", j5, "空变更不入账");

    // J6 初始掩码：全系统可用。
    let j6 = {
        let b = AffinBook::new(0x3F);
        b.mask == 0x3F && b.popcount() == 6 && !b.pinned
    };
    s.add("J6-初始全集", j6, "初始全系统可用");

    // J7 边界收：先收子集再收系统全集（全集合法）。
    let j7 = {
        let mut b = AffinBook::new(0xFF);
        let _ = b.set_mask(0x0F, "sub");
        b.set_mask(0xFF, "full").is_ok() && b.popcount() == 8
    };
    s.add("J7-全集边界收", j7, "系统全集合法");

    // J8 位查询越界安全：bit≥64 恒假。
    let j8 = {
        let b = AffinBook::new(u64::MAX);
        !b.has_cpu(64) && !b.has_cpu(99) && b.has_cpu(63)
    };
    s.add("J8-位查询越界", j8, "越界位恒假");

    // J9 账目留痕：set/pin 逐笔合法。
    let j9 = {
        let mut b = AffinBook::new(0xFF);
        let _ = b.set_mask(0x10, "a");
        let _ = b.pin("p");
        b.ledger.len() == 2 && b.ledger_ok() && b.ledger[1].pinned
    };
    s.add("J9-留痕对账", j9, "set+pin 两笔");

    // J10 印账冻结。
    let j10 = {
        let mut b = AffinBook::new(0xFF);
        let _ = b.set_mask(0x0C, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.set_mask(0x01, "post").err() == Some(KA_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：4 处理器子系统。
    let mut b = AffinBook::new(0x0F);
    let _ = b.set_mask(0x05, "sum");
    s.add("样例子集", b.subset_of_sys(), "掩码为系统子集");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12013_mask_flow() {
        let mut b = AffinBook::new(0xFF);
        assert!(b.set_mask(0x81, "a").is_ok());
        assert_eq!(b.set_mask(0, "z"), Err(KA_ZERO));
        assert_eq!(b.set_mask(0x100, "o"), Err(KA_OUT_OF_SET));
        assert!(b.pin("p").is_ok());
        assert_eq!(b.set_mask(0x01, "x"), Err(KA_PINNED));
        assert_eq!(b.popcount(), 2);
    }

    #[test]
    fn unx12013_checks_green() {
        let s = run_unx12013_checks();
        assert!(s.all_passed(), "UNX-F12013 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12013_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
