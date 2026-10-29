//! UNX 卡 10154 — 栈增长正反双判据（自动扩栈达成与越栈误用受控）
//! 段位 seg=0x10660
//! 判据原文：UNX-F10154-J1 自动扩栈达成（递归深栈合法增长 10/10 次）
//! 锚点：栈指针下探自动扩页 + 栈上限闸 + 越栈受控 + 收缩归还
//! 域：StackBook 栈册（栈底/栈针/栈顶三锚 + 扩张账）
//! 结构：push 下探扩页 → pop 回收 → 上限闸 → 越栈受控 → 扩张账 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10660;
pub const SB_OVERFLOW: u32 = SEG_BASE + 0x01;
pub const SB_UNDERFLOW: u32 = SEG_BASE + 0x02;
pub const SB_SEALED: u32 = SEG_BASE + 0x03;
pub const SB_BAD_FRAME: u32 = SEG_BASE + 0x04;
pub const SB_GROW_CAP: u32 = SEG_BASE + 0x05;

/// 页粒度与栈上限（页）
pub const PAGE: usize = 0x1000;
pub const STACK_MAX_PAGES: usize = 0x10;

/// 栈册
pub struct StackBook {
    /// 栈底（高地址）
    pub base: usize,
    /// 当前栈针（低地址端，向下增长）
    pub sp: usize,
    /// 已扩页数
    pub grown: usize,
    /// 扩张账：每次扩页记录
    pub grows: Vec<usize>,
    sealed: bool,
}

impl StackBook {
    pub fn new(base: usize) -> Self {
        StackBook {
            base,
            sp: base,
            grown: 0,
            grows: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 帧大小（字节）
    pub fn pages_used(&self) -> usize {
        (self.base - self.sp) / PAGE
    }

    /// push 帧下探：跨页自动扩栈；越上限受控
    pub fn push(&mut self, frame: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(SB_SEALED);
        }
        if frame == 0 || frame % 16 != 0 {
            return Err(SB_BAD_FRAME);
        }
        if frame > self.sp {
            return Err(SB_OVERFLOW);
        }
        let want = self.sp - frame;
        let need_pages = (self.base - want + PAGE - 1) / PAGE;
        if need_pages > STACK_MAX_PAGES {
            return Err(SB_OVERFLOW);
        }
        if need_pages > self.grown {
            self.grown = need_pages;
            self.grows.push(need_pages);
        }
        self.sp = want;
        Ok(self.sp)
    }

    /// pop 回收
    pub fn pop(&mut self, frame: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(SB_SEALED);
        }
        if frame == 0 || frame % 16 != 0 {
            return Err(SB_BAD_FRAME);
        }
        if self.sp + frame > self.base {
            return Err(SB_UNDERFLOW);
        }
        self.sp += frame;
        Ok(self.sp)
    }

    /// 递归深栈面：连续下探 n 帧
    pub fn recurse(&mut self, frames: usize, frame: usize) -> Result<usize, u32> {
        for _ in 0..frames {
            self.push(frame)?;
        }
        Ok(self.pages_used())
    }

    /// 收缩归零
    pub fn unwind(&mut self) -> Result<usize, u32> {
        let n = (self.base - self.sp) / 16;
        for _ in 0..n {
            self.pop(16)?;
        }
        Ok(self.pages_used())
    }

    /// 扩张账单调
    pub fn grows_monotonic(&self) -> bool {
        for w in self.grows.windows(2) {
            if w[1] <= w[0] {
                return false;
            }
        }
        true
    }

    /// 扩张守恒：grown == pages_used 对齐页
    pub fn conservation(&self) -> bool {
        self.grown == self.pages_used() && self.grown <= STACK_MAX_PAGES
    }

    /// 指纹
    pub fn render(&self) -> String {
        format!("base={:#x} sp={:#x} grown={}", self.base, self.sp, self.grown)
    }

    /// 剩余可扩页数
    pub fn headroom_pages(&self) -> usize {
        STACK_MAX_PAGES - self.grown
    }

    /// 末次扩页档
    pub fn last_grow(&self) -> Option<usize> {
        self.grows.last().copied()
    }

    /// 扩张账笔数
    pub fn grow_count(&self) -> usize {
        self.grows.len()
    }

    /// 扩张账末档别名
    pub fn grow_top(&self) -> usize {
        self.grown
    }

    /// 封册
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = StackBook::new(0x1_0000);
    // 1) 越上限必拒
    got[0] = b.push(STACK_MAX_PAGES * PAGE + 16).is_err();
    // 2) 非对齐帧必拒
    got[1] = b.push(17).is_err();
    // 3) 空栈弹出过头必拒
    got[2] = b.pop(16).is_err();
    // 4) 扩张账单调（逐级扩不回跳）
    let _ = b.push(0x1000);
    let _ = b.push(0x1000);
    let _ = b.push(0x1000);
    got[3] = b.grows_monotonic();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.push(16).is_err();
    got
}

/// 判据入口：UNX-F10154
pub fn run_unx10154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10154");
    let mut b = StackBook::new(0x1_0000);

    // J1 自动扩栈 10/10：十级递归逐页扩
    let mut j1 = true;
    for _ in 0..10 {
        if b.push(0x1000) != Ok(b.sp) {
            j1 = false;
        }
    }
    let j1 = j1 && b.grown == 10 && b.grows_monotonic();
    s.add("J1_grow_10x", j1, "十级递归逐页扩栈 10/10");

    // J2 页账守恒
    s.add("J2_conservation", b.conservation() && b.pages_used() == 10, "扩页账 10 页对平");

    // J3 小帧不跨页不扩（独立半页册：页内下探不动扩张账）
    let mut b9 = StackBook::new(0x2_0000);
    let _ = b9.push(0x800);
    let g0 = b9.grown;
    let _ = b9.push(0x10);
    let j3 = b9.grown == g0 && b9.grown == 1 && b9.sp == b9.base - 0x810;
    s.add("J3_inpage", j3, "页内下探不扩新页");

    // J4 pop 回收
    let j4 = b.pop(0x1000) == Ok(0x1_0000 - 9 * 0x1000) && b.pages_used() == 9;
    s.add("J4_pop", j4, "弹帧回收页账 9");

    // J5 弹出过头受控
    let mut b2 = StackBook::new(0x8000);
    let _ = b2.push(0x100);
    let _ = b2.pop(0x100);
    s.add("J5_underflow", b2.pop(0x100) == Err(SB_UNDERFLOW), "空栈再弹拒 UNDERFLOW");

    // J6 越上限受控
    let mut b3 = StackBook::new(0x2_0000);
    let j6 = b3.push(STACK_MAX_PAGES * PAGE + 16) == Err(SB_OVERFLOW)
        && b3.pages_used() == 0;
    s.add("J6_overflow", j6, "一帧越上限拒 OVERFLOW 且不动栈针");

    // J7 恰满上限
    let mut b4 = StackBook::new(0x3_0000);
    let r = b4.recurse(STACK_MAX_PAGES, 0x1000);
    let over = b4.push(16);
    let j7 = r == Ok(STACK_MAX_PAGES) && over == Err(SB_OVERFLOW);
    s.add("J7_exact", j7, "恰满 16 页后补帧拒");

    // J8 非对齐帧受控
    let j8 = b4.pop(0) == Err(SB_BAD_FRAME) && b4.pop(17) == Err(SB_BAD_FRAME);
    s.add("J8_bad_frame", j8, "零帧与非对齐帧双拒");

    // J9 收缩归零
    let u = b4.unwind();
    s.add("J9_unwind", u == Ok(0) && b4.sp == b4.base, "收缩归零栈针回底");

    // J10 扩张账指纹
    let fp = b.render();
    let fp2 = b.render();
    s.add("J10_fingerprint", fp == fp2 && fp.contains("grown=10"), "栈面指纹稳定 grown=10");

    // J11 扩张账单调
    s.add("J11_grows_mono", b.grows_monotonic() && b.grows.len() >= 10, "扩张账逐级单调且 ≥10 档");

    // J12 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 封册受控
    let mut b5 = StackBook::new(0x4000);
    b5.seal();
    let j13 = b5.push(16) == Err(SB_SEALED) && b5.pop(16) == Err(SB_SEALED);
    s.add("J13_sealed", j13, "封册后推弹皆拒 SEALED");

    // J14 递归面整合
    let mut b6 = StackBook::new(0x2_0000);
    let deep = b6.recurse(8, 0x800);
    let j14 = deep == Ok(4) && b6.conservation();
    s.add("J14_recurse", j14, "8 帧半页递归 4 页账平");

    // J15 半途收回再扩
    let mut b7 = StackBook::new(0x2_0000);
    let _ = b7.recurse(4, 0x1000);
    let _ = b7.pop(0x1000);
    let r2 = b7.push(0x1000);
    let j15 = r2.is_ok() && b7.grown == 4 && b7.pages_used() == 4;
    s.add("J15_reuse", j15, "收 1 页再扩不增账");

    // J16 剩余扩容
    let j16 = b.headroom_pages() == 6;
    s.add("J16_headroom", j16, "16 页帽余 6 页");

    // J17 末次扩档
    let j17 = b.last_grow() == Some(10);
    s.add("J17_last_grow", j17, "末档第 10 页");

    // J18 扩张账笔数
    let j18 = b.grow_count() == 10;
    s.add("J18_grow_count", j18, "十档扩张账");

    // J19 空册扩容满额
    let mut b8 = StackBook::new(0x1_0000);
    let j19 = b8.headroom_pages() == STACK_MAX_PAGES && b8.grow_count() == 0;
    s.add("J19_fresh", j19, "空册余 16 页零档");

    // J20 半页扩档
    let r20 = b8.push(0x800);
    let j20 = r20.is_ok() && b8.grown == 1 && b8.grow_count() == 1;
    s.add("J20_half_page", j20, "半页下探即扩 1 页");

    // J21 小帧页内不扩
    let g0 = b8.grown;
    let _ = b8.push(0x100);
    let _ = b8.push(0x700);
    let j21 = b8.grown == g0 && b8.pages_used() == 1;
    s.add("J21_inpage_fill", j21, "页内填满 0x1000 不跨页");

    // J22 跨页再扩
    let r22 = b8.push(0x100);
    let j22 = r22.is_ok() && b8.grown == 2 && b8.last_grow() == Some(2);
    s.add("J22_cross", j22, "跨页下探扩至 2 页");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack54_grow() {
        let mut b = StackBook::new(0x1_0000);
        assert!(b.push(0x1000).is_ok());
        assert_eq!(b.grown, 1);
        assert!(b.push(0x800).is_ok());
        assert_eq!(b.grown, 2);
        assert!(b.conservation());
    }

    #[test]
    fn test_stack54_overflow() {
        let mut b = StackBook::new(0x1_0000);
        assert_eq!(b.push(STACK_MAX_PAGES * PAGE + 16), Err(SB_OVERFLOW));
        assert_eq!(b.push(17), Err(SB_BAD_FRAME));
    }

    #[test]
    fn test_stack54_checks() {
        let cs = run_unx10154_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_stack54_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
