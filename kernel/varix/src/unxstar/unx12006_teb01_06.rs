//! UNX-F12006 TEB 与 NT_TIB 语义实装（0x192E0 段）
//!
//! 判据原文：NT_TIB 三锚自洽——栈基（StackBase）必须高于栈限
//! （StackLimit），栈限必须低于栈基且两者同页带（差值不超用户栈上限）；
//! Self 指针必须精确等于 TEB 自身地址（自指锚）；异常链锚初始为空链
//! 表示，不得悬挂野值。
//!
//! 结构：TIB 锚册（四锚：base/limit/self/exception）+ 锚校核（五码守卫）
//! + 锚账（TibRec 逐次校验留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12006_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x192E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x192E0;
pub const TB_BAD_ANCHOR: u32 = SEG_BASE + 0x01; // 零锚悬挂
pub const TB_STACK_INVERT: u32 = SEG_BASE + 0x02; // 栈限≥栈基
pub const TB_SELF_MISMATCH: u32 = SEG_BASE + 0x03; // Self ≠ TEB 地址
pub const TB_SPAN_WILD: u32 = SEG_BASE + 0x04; // 栈带尺寸超上限
pub const TB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍改锚

/// 用户栈带尺寸上限（语义档钉值：16MB）。
pub const MAX_STACK_SPAN: u64 = 0x0100_0000;

/// NT_TIB 锚册。
pub struct TibModel {
    pub stack_base: u64,
    pub stack_limit: u64,
    pub self_ptr: u64,
    pub teb_addr: u64,
    /// 异常链锚（0 = 空链合法表示）。
    pub exception_list: u64,
    pub ledger: Vec<TibRec>,
    pub sealed: bool,
}

/// 一次锚校验的留痕。
#[derive(Clone)]
pub struct TibRec {
    pub tag: u32,
    pub base: u64,
    pub limit: u64,
    pub note: String,
}

impl TibRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.tag > 0
    }
}

impl TibModel {
    /// 建册：teb_addr 为 Self 锚的对标值。
    pub fn new(teb_addr: u64) -> TibModel {
        TibModel {
            stack_base: 0,
            stack_limit: 0,
            self_ptr: 0,
            teb_addr,
            exception_list: 0,
            ledger: Vec::new(),
            sealed: false,
        }
    }

    /// 锚挂接核：一次挂全四锚并校验。
    pub fn attach(
        &mut self,
        base: u64,
        limit: u64,
        self_ptr: u64,
        exception_list: u64,
        tag: u32,
        note: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if note.is_empty() || tag == 0 {
            return Err(TB_BAD_ANCHOR);
        }
        if base == 0 || limit == 0 {
            return Err(TB_BAD_ANCHOR);
        }
        if limit >= base {
            return Err(TB_STACK_INVERT);
        }
        if self_ptr != self.teb_addr {
            return Err(TB_SELF_MISMATCH);
        }
        // 栈带尺寸：base-limit 不得超过上限（野栈带拒收）。
        if base - limit > MAX_STACK_SPAN {
            return Err(TB_SPAN_WILD);
        }
        self.stack_base = base;
        self.stack_limit = limit;
        self.self_ptr = self_ptr;
        self.exception_list = exception_list;
        self.ledger.push(TibRec { tag, base, limit, note: note.to_string() });
        Ok(())
    }

    /// 空链合法表示判定。
    pub fn empty_exception_chain(&self) -> bool {
        self.exception_list == 0
    }

    /// 三锚自洽复核（挂接后恒真；未挂接为假）。
    pub fn anchors_self_consistent(&self) -> bool {
        self.stack_base > 0
            && self.stack_limit > 0
            && self.stack_limit < self.stack_base
            && self.self_ptr == self.teb_addr
    }

    /// 栈带尺寸回读。
    pub fn span(&self) -> u64 {
        if self.stack_base > self.stack_limit {
            self.stack_base - self.stack_limit
        } else {
            0
        }
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add((self.stack_base & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add((self.stack_limit & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add((self.self_ptr & 0xFFFF) as u32);
        h = h.wrapping_mul(211).wrapping_add(self.exception_list as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.tag);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let teb = 0x7EF_DD000u64;
    // 1) 零锚悬挂：base=0 → TB_BAD_ANCHOR。
    let mut a = TibModel::new(teb);
    got[0] =
        a.attach(0, teb - 0x10000, teb, 0, 1, "z").err() == Some(TB_BAD_ANCHOR);
    // 2) 栈限≥栈基（倒挂）→ TB_STACK_INVERT。
    let mut b = TibModel::new(teb);
    got[1] = b
        .attach(teb - 0x10000, teb, teb, 0, 2, "inv")
        .err() == Some(TB_STACK_INVERT);
    // 3) Self 错挂：self_ptr ≠ teb_addr → TB_SELF_MISMATCH。
    let mut c = TibModel::new(teb);
    got[2] = c
        .attach(teb, teb - 0x10000, teb + 8, 0, 3, "self")
        .err() == Some(TB_SELF_MISMATCH);
    // 4) 野栈带：差值超上限 → TB_SPAN_WILD。
    let mut d = TibModel::new(teb);
    got[3] = d
        .attach(teb, teb - MAX_STACK_SPAN - 1, teb, 0, 4, "wild")
        .err() == Some(TB_SPAN_WILD);
    // 5) 空留痕挂接 → TB_BAD_ANCHOR（tag 守卫同分支）。
    let mut e = TibModel::new(teb);
    got[4] =
        e.attach(teb, teb - 0x10000, teb, 0, 5, "").err() == Some(TB_BAD_ANCHOR);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12006_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12006");

    // J1 正路径：合法四锚挂接 + 三锚自洽。
    let j1 = {
        let teb = 0x7EF_DD000u64;
        let mut t = TibModel::new(teb);
        let _ = t.attach(teb, teb - 0x0010_0000, teb, 0, 1, "user-stack");
        t.anchors_self_consistent()
            && t.empty_exception_chain()
            && t.span() == 0x0010_0000
    };
    s.add("J1-四锚正路径", j1, "合法栈带自洽");

    // J2 零锚拒。
    let j2 = {
        let teb = 0x7EF_DE000u64;
        let mut t = TibModel::new(teb);
        t.attach(teb, 0, teb, 0, 1, "z").err() == Some(TB_BAD_ANCHOR)
    };
    s.add("J2-零锚拒", j2, "栈限零悬挂必拒");

    // J3 倒挂拒：limit==base 也算倒（空带非法）。
    let j3 = {
        let teb = 0x7EF_DF000u64;
        let mut t = TibModel::new(teb);
        t.attach(teb, teb, teb, 0, 1, "eq").err() == Some(TB_STACK_INVERT)
    };
    s.add("J3-倒挂拒", j3, "limit==base 非法");

    // J4 Self 锚精确性：差一字节即拒。
    let j4 = {
        let teb = 0x7EF_E0000u64;
        let mut t = TibModel::new(teb);
        t.attach(teb, teb - 0x1000, teb - 1, 0, 1, "off")
            .err() == Some(TB_SELF_MISMATCH)
    };
    s.add("J4-Self精确", j4, "Self 差一字节必拒");

    // J5 野栈带拒：恰好超一字节。
    let j5 = {
        let teb = 0x7EF_E1000u64;
        let mut t = TibModel::new(teb);
        t.attach(teb, teb - MAX_STACK_SPAN - 1, teb, 0, 1, "w")
            .err() == Some(TB_SPAN_WILD)
    };
    s.add("J5-野栈带拒", j5, "超上限一字节必拒");

    // J6 边界收：恰好等于上限的栈带合法。
    let j6 = {
        let teb = 0x7EF_E2000u64;
        let mut t = TibModel::new(teb);
        t.attach(teb, teb - MAX_STACK_SPAN, teb, 0, 1, "edge").is_ok()
            && t.span() == MAX_STACK_SPAN
    };
    s.add("J6-边界收", j6, "上限栈带合法");

    // J7 空链表示：异常链 0 为合法空链。
    let j7 = {
        let teb = 0x7EF_E3000u64;
        let mut t = TibModel::new(teb);
        let _ = t.attach(teb, teb - 0x2000, teb, 0, 1, "e");
        t.empty_exception_chain()
    };
    s.add("J7-空链表示", j7, "0=空链合法");

    // J8 挂接留痕：逐次留痕合法且 tag 递增可查。
    let j8 = {
        let teb = 0x7EF_E4000u64;
        let mut t = TibModel::new(teb);
        let _ = t.attach(teb, teb - 0x3000, teb, 0, 7, "a");
        t.ledger.len() == 1 && t.ledger_ok() && t.ledger[0].tag == 7
    };
    s.add("J8-挂接留痕", j8, "tag=7 落账");

    // J9 印账冻结。
    let j9 = {
        let teb = 0x7EF_E5000u64;
        let mut t = TibModel::new(teb);
        let _ = t.attach(teb, teb - 0x4000, teb, 0, 1, "a");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2
            && t.attach(teb, teb - 0x5000, teb, 0, 2, "post").err() == Some(TB_FROZEN)
    };
    s.add("J9-指纹封账", j9, "指纹幂等且印后必拒");

    // J10 未挂接态：锚全零时自洽判定为假。
    let j10 = {
        let t = TibModel::new(0x7EF_E6000);
        !t.anchors_self_consistent()
    };
    s.add("J10-未挂接假", j10, "未挂接不自洽");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：内核线程 TEB（栈带 64KB）。
    let mut t = TibModel::new(0xFFFF_F800_C000_1000);
    let _ = t.attach(
        0xFFFF_F800_C000_1000,
        0xFFFF_F800_C000_0000,
        0xFFFF_F800_C000_1000,
        0,
        9,
        "ktib",
    );
    s.add("样例自洽", t.anchors_self_consistent(), "内核栈带过校");
    s.add("样例留痕", t.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12006_anchor_flow() {
        let teb = 0x7EF_F0000u64;
        let mut t = TibModel::new(teb);
        assert!(t.attach(teb, teb - 0x10000, teb, 0, 1, "s").is_ok());
        assert_eq!(
            t.attach(teb - 0x10000, teb, teb, 0, 2, "inv"),
            Err(TB_STACK_INVERT)
        );
        assert_eq!(t.span(), 0x10000);
        assert!(t.anchors_self_consistent());
    }

    #[test]
    fn unx12006_checks_green() {
        let s = run_unx12006_checks();
        assert!(s.all_passed(), "UNX-F12006 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12006_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
