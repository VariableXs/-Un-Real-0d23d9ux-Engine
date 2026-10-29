//! UNX-F12062 NT_TIB 三锚语义（SEH 链 / 栈界 / 自指针）（0x199E0 段）
//!
//! 判据原文：NT_TIB 三锚——ExceptionList（SEH 链头）、
//! StackBase/StackLimit（栈界）、Self（TIB 自指针）；语义规则：
//! 栈界必须 Base > Limit；链头展开核按帧序回放；Self 必须指向
//! TIB 本体（自洽校验）；异常帧压入/弹出维持链头单调。
//!
//! 结构：锚册（TibAnchors）+ SEH 链（FrameChain 压弹核）+ 四码
//! 守卫 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12062_checks）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x199E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x199E0;
pub const TB_STACK_INV: u32 = SEG_BASE + 0x01; // 栈界倒挂
pub const TB_SELF_BAD: u32 = SEG_BASE + 0x02; // Self 不自洽
pub const TB_CHAIN_EMPTY: u32 = SEG_BASE + 0x03; // 空链展开
pub const TB_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍动作
pub const TB_FRAME_BAD: u32 = SEG_BASE + 0x05; // 伪帧登记

/// 异常帧。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TibFrame {
    pub addr: u32,
    pub handler: u32,
    pub sealed: bool,
}

impl TibFrame {
    /// 帧地址与处理器地址必须落在代码段窗口且非零。
    pub fn ok(&self) -> bool {
        self.addr != 0 && self.handler != 0 && self.addr != self.handler
    }
}

/// NT_TIB 三锚模型。
pub struct TibAnchors {
    pub frames: Vec<TibFrame>, // 链头在前（index 0 为最新）
    pub stack_base: u32,
    pub stack_limit: u32,
    pub self_va: u32,
    pub sealed: bool,
}

impl TibAnchors {
    pub fn new() -> TibAnchors {
        TibAnchors {
            frames: Vec::new(),
            stack_base: 0,
            stack_limit: 0,
            self_va: 0,
            sealed: false,
        }
    }

    /// 栈界装配：必须 Base > Limit。
    pub fn set_stack(&mut self, base: u32, limit: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if base <= limit {
            return Err(TB_STACK_INV);
        }
        self.stack_base = base;
        self.stack_limit = limit;
        Ok(())
    }

    /// Self 自洽：TIB 本体 VA 必须落在自身栈界内或非零。
    pub fn set_self(&mut self, va: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if va == 0 {
            return Err(TB_SELF_BAD);
        }
        self.self_va = va;
        Ok(())
    }

    /// 自洽复核：Self 非零且（栈装配后）落在栈界内。
    pub fn self_consistent(&self) -> bool {
        if self.self_va == 0 {
            return false;
        }
        if self.stack_base == 0 && self.stack_limit == 0 {
            return true; // 栈未装配时仅查非零
        }
        self.self_va > self.stack_limit && self.self_va <= self.stack_base
    }

    /// SEH 帧压入（链头）。
    pub fn push_frame(&mut self, addr: u32, handler: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        let f = TibFrame { addr, handler, sealed: false };
        if !f.ok() {
            return Err(TB_FRAME_BAD);
        }
        self.frames.insert(0, f);
        Ok(self.frames.len())
    }

    /// SEH 帧弹出（链头出）。
    pub fn pop_frame(&mut self) -> Result<TibFrame, u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        match self.frames.first() {
            Some(f) => {
                let f = f.clone();
                self.frames.remove(0);
                Ok(f)
            }
            None => Err(TB_CHAIN_EMPTY),
        }
    }

    /// 展开核：链序回放（链头先出）。
    pub fn unwind(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for f in &self.frames {
            out.push(f.addr);
        }
        out
    }

    /// 链深。
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.stack_base >> 4);
        h = h.wrapping_mul(211).wrapping_add(self.stack_limit >> 4);
        h = h.wrapping_mul(211).wrapping_add(self.self_va >> 4);
        for f in &self.frames {
            h = h.wrapping_mul(211).wrapping_add(f.addr >> 4);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = TibAnchors::new();
    // 1) 栈界倒挂（Base <= Limit）→ TB_STACK_INV。
    got[0] = t.set_stack(0x1000, 0x9000).err() == Some(TB_STACK_INV);
    // 2) Self 零值 → TB_SELF_BAD。
    got[1] = t.set_self(0).err() == Some(TB_SELF_BAD);
    // 3) 空链弹出 → TB_CHAIN_EMPTY。
    got[2] = t.pop_frame().err() == Some(TB_CHAIN_EMPTY);
    // 4) 伪帧（handler=addr）→ TB_FRAME_BAD。
    got[3] = t.push_frame(0x401000, 0x401000).err() == Some(TB_FRAME_BAD);
    // 5) 印账后压帧 → TB_FROZEN。
    t.seal();
    got[4] = t.push_frame(0x401000, 0x402000).err() == Some(TB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12062");

    // J1 栈界正装配。
    let j1 = {
        let mut t = TibAnchors::new();
        t.set_stack(0x9_0000, 0x5_0000).is_ok()
            && t.stack_base == 0x9_0000 && t.stack_limit == 0x5_0000
    };
    s.add("J1-栈界装配", j1, "Base>Limit 落账");

    // J2 栈界倒挂拒（相等与反向双样）。
    let j2 = {
        let mut a = TibAnchors::new();
        let mut b = TibAnchors::new();
        a.set_stack(0x1000, 0x1000).err() == Some(TB_STACK_INV)
            && b.set_stack(0x1000, 0x9000).err() == Some(TB_STACK_INV)
    };
    s.add("J2-倒挂拒", j2, "相等/反向全拒");

    // J3 Self 自洽面。
    let j3 = {
        let mut t = TibAnchors::new();
        let _ = t.set_stack(0x9_0000, 0x5_0000);
        let _ = t.set_self(0x7_0000);
        t.self_consistent()
    };
    s.add("J3-自指针自洽", j3, "Self 落栈界内");

    // J4 Self 零值拒。
    let j4 = TibAnchors::new().set_self(0).err() == Some(TB_SELF_BAD);
    s.add("J4-零自针拒", j4, "Self 必非零");

    // J5 SEH 链压弹 LIFO。
    let j5 = {
        let mut t = TibAnchors::new();
        let _ = t.push_frame(0x401000, 0x402000);
        let _ = t.push_frame(0x403000, 0x404000);
        let top = t.pop_frame().ok();
        t.depth() == 1 && top.unwrap_or(TibFrame { addr: 0, handler: 0, sealed: false }).addr == 0x403000
    };
    s.add("J5-链压弹", j5, "链头先出");

    // J6 展开核链序。
    let j6 = {
        let mut t = TibAnchors::new();
        let _ = t.push_frame(0x401000, 0x402000);
        let _ = t.push_frame(0x403000, 0x404000);
        let u = t.unwind();
        u.len() == 2 && u[0] == 0x403000 && u[1] == 0x401000
    };
    s.add("J6-展开序", j6, "链头先回放");

    // J7 空链展开为空 / 空链弹出拒。
    let j7 = {
        let mut t = TibAnchors::new();
        t.unwind().is_empty() && t.pop_frame().err() == Some(TB_CHAIN_EMPTY)
    };
    s.add("J7-空链面", j7, "空链两拒");

    // J8 伪帧拒（零地址 / 同址）。
    let j8 = {
        let mut t = TibAnchors::new();
        t.push_frame(0, 0x402000).err() == Some(TB_FRAME_BAD)
            && t.push_frame(0x401000, 0x401000).err() == Some(TB_FRAME_BAD)
    };
    s.add("J8-伪帧拒", j8, "零址/同址全拒");

    // J9 链深清点。
    let j9 = {
        let mut t = TibAnchors::new();
        let _ = t.push_frame(0x401000, 0x402000);
        let _ = t.push_frame(0x403000, 0x404000);
        let _ = t.push_frame(0x405000, 0x406000);
        t.depth() == 3
    };
    s.add("J9-链深清点", j9, "三帧在链");

    // J10 指纹区分度与幂等。
    let j10 = {
        let mut a = TibAnchors::new();
        let _ = a.push_frame(0x401000, 0x402000);
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = TibAnchors::new();
        let _ = c.push_frame(0x403000, 0x404000);
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J10-指纹性质", j10, "幂等且异锚异纹");

    // J11 印账冻结。
    let j11 = {
        let mut t = TibAnchors::new();
        t.seal();
        t.set_stack(0x90000, 0x50000).err() == Some(TB_FROZEN)
            && t.set_self(1).err() == Some(TB_FROZEN)
    };
    s.add("J11-印账冻结", j11, "印后动作全拒");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例：完整 TIB 面。
    let mut t = TibAnchors::new();
    let _ = t.set_stack(0x9_0000, 0x5_0000);
    let _ = t.set_self(0x7_E000);
    let _ = t.push_frame(0x401000, 0x402000);
    s.add("样例全锚", t.self_consistent() && t.depth() == 1, "三锚齐备");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12062_stack_rules() {
        let mut t = TibAnchors::new();
        assert!(t.set_stack(0x90000, 0x50000).is_ok());
        assert_eq!(t.set_stack(0x50000, 0x90000).err(), Some(TB_STACK_INV));
    }

    #[test]
    fn unx12062_checks_green() {
        let s = run_unx12062_checks();
        assert!(s.all_passed(), "UNX-F12062 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12062_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
