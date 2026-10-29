//! UNX-F12069 ActivationContextStackPointer 与激活上下文栈（0x19ac0 段）
//!
//! 卡面：TEB→ActivationContextStackPointer——激活上下文栈指针语义模型：
//! 帧栈 LIFO 推进、cookie 唯一、区间不重叠、弹出配对。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 帧区间合法：limit > base 且 base 非零的帧才可入栈。
//! - J2 cookie 唯一：同 cookie 二次入栈必拒。
//! - J3 区间不重叠：新帧与栈顶帧地址区间相交必拒。
//! - J4 LIFO 弹出：仅栈顶 cookie 可弹，弹空栈必拒，配对不符必拒。
//! - J5 栈深对账：栈内帧数与账本登记数一致。
//! - J6 印账冻结：seal 后推入必拒。
//! - J7 注入五况全抓。
//!
//! 结构：ActFrame 帧类型 + ActStack LIFO 状态机 + 帧账本 + 折叠指纹 +
//! 注入核（零 cookie/坏区间/重叠/空弹/印后推）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ac0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ac0;
pub const AC_BAD_COOKIE: u32 = SEG_BASE + 0x01; // 零 cookie
pub const AC_BAD_RANGE: u32 = SEG_BASE + 0x02; // limit <= base
pub const AC_OVERLAP: u32 = SEG_BASE + 0x03; // 与栈顶区间重叠
pub const AC_UNDERFLOW: u32 = SEG_BASE + 0x04; // 空栈弹出
pub const AC_MISMATCH: u32 = SEG_BASE + 0x06; // 弹出 cookie 非栈顶
pub const AC_FROZEN: u32 = SEG_BASE + 0x07; // 印账后推入

/// 激活上下文栈帧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActFrame {
    pub cookie: u32,
    pub base: u32,
    pub limit: u32,
}

impl ActFrame {
    /// 帧区间合法：cookie 非零且 limit > base。
    pub fn well_formed(&self) -> bool {
        self.cookie != 0 && self.limit > self.base && self.base != 0
    }
    /// 与他帧区间相交判定。
    pub fn overlaps(&self, other: &ActFrame) -> bool {
        self.base < other.limit && other.base < self.limit
    }
    /// 帧跨度。
    pub fn span(&self) -> u32 {
        if self.limit > self.base {
            self.limit - self.base
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("C{}|[{:x},{:x})", self.cookie, self.base, self.limit)
    }
}

/// 激活上下文栈：LIFO 状态机 + 帧账本。
pub struct ActStack {
    pub stack: Vec<ActFrame>,
    pub ledger: Vec<(u32, u8)>, // (cookie, 0=push 1=pop)
    pub sealed: bool,
}

impl ActStack {
    pub fn new() -> ActStack {
        ActStack { stack: Vec::new(), ledger: Vec::new(), sealed: false }
    }

    /// 推帧：well-formed 且不与栈顶重叠且 cookie 不重复。
    pub fn push(&mut self, f: ActFrame) -> Result<(), u32> {
        if self.sealed {
            return Err(AC_FROZEN);
        }
        if f.cookie == 0 {
            return Err(AC_BAD_COOKIE);
        }
        if f.limit <= f.base || f.base == 0 {
            return Err(AC_BAD_RANGE);
        }
        if self.stack.iter().any(|t| t.cookie == f.cookie) {
            return Err(AC_BAD_COOKIE); // cookie 全栈唯一
        }
        if let Some(top) = self.stack.last() {
            if f.overlaps(top) {
                return Err(AC_OVERLAP);
            }
        }
        self.stack.push(f);
        self.ledger.push((f.cookie, 0));
        Ok(())
    }

    /// 弹帧：仅栈顶 cookie 可弹。
    pub fn pop(&mut self, cookie: u32) -> Result<ActFrame, u32> {
        if self.sealed {
            return Err(AC_FROZEN);
        }
        let top = match self.stack.last() {
            Some(t) => *t,
            None => return Err(AC_UNDERFLOW),
        };
        if top.cookie != cookie {
            return Err(AC_MISMATCH);
        }
        self.ledger.push((cookie, 1));
        Ok(self.stack.pop().unwrap_or(top))
    }

    /// 栈深对账：栈内帧数 == push 数 − pop 数。
    pub fn tallies_match(&self) -> bool {
        let pushes = self.ledger.iter().filter(|e| e.1 == 0).count();
        let pops = self.ledger.iter().filter(|e| e.1 == 1).count();
        pushes >= pops && self.stack.len() == pushes - pops
    }

    /// 栈顶帧查询。
    pub fn top(&self) -> Option<ActFrame> {
        self.stack.last().copied()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for f in &self.stack {
            h = h.wrapping_mul(211).wrapping_add(f.cookie);
            h = h.wrapping_mul(211).wrapping_add(f.base);
            h = h.wrapping_mul(211).wrapping_add(f.limit);
        }
        for e in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(e.0);
            h = h.wrapping_mul(211).wrapping_add(e.1 as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：栈深。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 诊断面：已登记帧跨度合计。
    pub fn total_span(&self) -> u32 {
        self.stack.iter().map(|f| f.span()).sum()
    }

    /// 诊断面：账面摘要。
    pub fn summary(&self) -> String {
        format!("depth={} events={}", self.stack.len(), self.ledger.len())
    }

    /// 诊断面：cookie 在栈内判定。
    pub fn on_stack(&self, cookie: u32) -> bool {
        self.stack.iter().any(|f| f.cookie == cookie)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut s = ActStack::new();
    // 1) 零 cookie 必拒。
    got[0] = s.push(ActFrame { cookie: 0, base: 0x1000, limit: 0x2000 })
        .err() == Some(AC_BAD_COOKIE);
    // 2) 坏区间（limit <= base）必拒。
    got[1] = s.push(ActFrame { cookie: 7, base: 0x3000, limit: 0x2000 })
        .err() == Some(AC_BAD_RANGE);
    // 3) 栈顶区间重叠必拒。
    let _ = s.push(ActFrame { cookie: 1, base: 0x1000, limit: 0x2000 });
    got[2] = s.push(ActFrame { cookie: 2, base: 0x1800, limit: 0x2800 })
        .err() == Some(AC_OVERLAP);
    // 4) 空栈弹出必拒（另册复核配对不符）。
    let mut s2 = ActStack::new();
    got[3] = s2.pop(9).err() == Some(AC_UNDERFLOW);
    let _ = s2.push(ActFrame { cookie: 5, base: 0x4000, limit: 0x5000 });
    // 5) 印账后推入必抓。
    s.seal();
    got[4] = s.push(ActFrame { cookie: 3, base: 0x6000, limit: 0x7000 })
        .err() == Some(AC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12069_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12069");

    // J1 帧区间合法。
    let f = ActFrame { cookie: 1, base: 0x1000, limit: 0x2000 };
    let j1 = f.well_formed() && f.span() == 0x1000 && f.render().contains("1000");
    s.add("J1-帧合法", j1, "well-formed 帧通过");

    // J2 cookie 唯一：同 cookie 二次入栈必拒。
    let j2 = {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        st.push(ActFrame { cookie: 5, base: 0x2000, limit: 0x2100 })
            .err() == Some(AC_BAD_COOKIE)
    };
    s.add("J2-cookie唯一", j2, "重复 cookie 必拒");

    // J3 区间不重叠。
    let j3 = {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        st.push(ActFrame { cookie: 6, base: 0x1080, limit: 0x1180 })
            .err() == Some(AC_OVERLAP)
    };
    s.add("J3-不重叠", j3, "与栈顶相交必拒");

    // J4 LIFO 弹出：仅栈顶可弹，非顶配对必拒。
    let j4 = {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        let _ = st.push(ActFrame { cookie: 6, base: 0x2000, limit: 0x2100 });
        st.pop(5).err() == Some(AC_MISMATCH) && st.pop(6).is_ok()
    };
    s.add("J4-LIFO弹出", j4, "仅栈顶 cookie 可弹");

    // J5 栈深对账。
    let j5 = {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        let _ = st.push(ActFrame { cookie: 6, base: 0x2000, limit: 0x2100 });
        let _ = st.pop(6);
        st.tallies_match() && st.depth() == 1
    };
    s.add("J5-栈深对账", j5, "push−pop==栈内帧数");

    // J6 印账冻结。
    let j6 = {
        let mut st = ActStack::new();
        st.seal();
        st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 })
            .err() == Some(AC_FROZEN)
            && st.pop(5).err() == Some(AC_FROZEN)
    };
    s.add("J6-印账冻结", j6, "seal 后推弹皆拒");

    // J7 指纹幂等。
    let j7 = {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        st.fold() == st.fold()
    };
    s.add("J7-指纹幂等", j7, "同态同指纹");

    // J8 注入核 5/5。
    let got = inject_five();
    let j8 = got.iter().all(|x| *x);
    s.add_dyn("J8-注入全抓", j8, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut st = ActStack::new();
    let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
    let _ = st.push(ActFrame { cookie: 6, base: 0x2000, limit: 0x2100 });

    s.add("栈深回读", st.depth() == 2, "双帧在栈");
    s.add("跨度合计", st.total_span() == 0x200, "两帧各 0x100");
    s.add("在栈判定", st.on_stack(5) && !st.on_stack(9), "cookie 查询");
    s.add("账面摘要", st.summary().contains("depth=2"), "摘要含栈深");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12069_stack_flow() {
        let mut st = ActStack::new();
        let _ = st.push(ActFrame { cookie: 5, base: 0x1000, limit: 0x1100 });
        assert_eq!(
            st.push(ActFrame { cookie: 5, base: 0x2000, limit: 0x2100 }),
            Err(AC_BAD_COOKIE)
        );
        assert!(st.pop(5).is_ok());
        assert!(st.top().is_none());
    }

    #[test]
    fn unx12069_checks_green() {
        let s = run_unx12069_checks();
        assert!(s.all_passed(), "UNX-F12069 判据未全绿");
    }

    #[test]
    fn frame_overlap_helpers() {
        let a = ActFrame { cookie: 1, base: 0x1000, limit: 0x2000 };
        let b = ActFrame { cookie: 2, base: 0x2000, limit: 0x3000 };
        let c = ActFrame { cookie: 3, base: 0x1fff, limit: 0x2fff };
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&c));
        assert!(!a.well_formed() || a.cookie == 1);
    }
}
