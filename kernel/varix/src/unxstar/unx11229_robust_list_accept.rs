//! UNX-F11229 set_robust_list 接受态账（0x1f829 段）
//!
//! 锚点：UNX-F11229-J1 robust list 注册接受语义（接收不解析）与线程退出
//! 遍历缺席态显式登记，glibc 假设不破。
//! 域：CoRun·UNX-C5-B02——内核只记住 head 与 len，不跟随链表；退出时不做
//! 遍历这件事必须写成「显式缺席」，而不是悄悄返回成功。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f829;
pub const RB_PTR: u32 = SEG_BASE + 0x01; // head 为空
pub const RB_LEN: u32 = SEG_BASE + 0x02; // len 非 24
pub const RB_REG: u32 = SEG_BASE + 0x03; // 线程重录/参数非法
pub const RB_SCAN: u32 = SEG_BASE + 0x04; // 遍历被请求但缺席
pub const RB_ALIGN: u32 = SEG_BASE + 0x05; // head 未对齐
pub const RB_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const RB_OVER: u32 = SEG_BASE + 0x07; // 超容

/// robust_list_head 的尺寸与登记上限。
pub const HEAD_LEN: u64 = 24;
pub const WANT: usize = 3;
pub const CAP: usize = 8;
pub const RB_TAG: u64 = 0x1f829a5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次接受态登记（head 视为不透明地址）。
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u32,
    pub head: u64,
    pub len: u64,
    pub accepted: bool,
}

impl Entry {
    pub fn shaped(&self) -> bool {
        self.head != 0 && self.head % 8 == 0 && self.len == HEAD_LEN
    }
}

/// robust list 接受账。
#[derive(Clone, Debug)]
pub struct RobustReg {
    pub regs: Vec<Entry>,
    pub scan_claims: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RobustReg {
    pub fn new() -> RobustReg {
        RobustReg { regs: Vec::new(), scan_claims: 0, frozen: false, root: None }
    }

    /// 接收：空指针/错长/错位/重号四路拒；只存不解析。
    pub fn register(&mut self, id: u32, head: u64, len: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(RB_FROZEN);
        }
        if head == 0 {
            return Err(RB_PTR);
        }
        if head % 8 != 0 {
            return Err(RB_ALIGN);
        }
        if len != HEAD_LEN {
            return Err(RB_LEN);
        }
        if id == 0 || self.regs.iter().any(|r| r.id == id) {
            return Err(RB_REG);
        }
        if self.regs.len() >= CAP {
            return Err(RB_OVER);
        }
        self.regs.push(Entry { id, head, len, accepted: true });
        Ok(())
    }

    /// 退出遍历：本实现显式缺席，任何请求都拒并计数。
    pub fn scan_on_exit(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(RB_FROZEN);
        }
        self.scan_claims = self.scan_claims.saturating_add(1);
        Err(RB_SCAN)
    }

    /// 接收不解析：所有 head 均按不透明地址保存。
    pub fn opaque(&self) -> bool {
        self.regs.iter().all(|r| r.accepted)
    }

    /// 缺席态显式：从未声明做过遍历。
    pub fn absent(&self) -> bool {
        self.scan_claims == 0
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(RB_FROZEN);
        }
        if self.regs.len() != WANT {
            return Err(RB_REG);
        }
        if !self.regs.iter().all(|r| r.shaped()) {
            return Err(RB_PTR);
        }
        if !self.opaque() {
            return Err(RB_REG);
        }
        if !self.absent() {
            return Err(RB_SCAN);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("robust {} accepted scan {}", self.regs.len(), self.absent())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = RB_TAG;
        let mut k = 0usize;
        while k < self.regs.len() {
            h = (h ^ self.regs[k].id as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.regs[k].head).wrapping_mul(FP_STEP);
            h = (h ^ self.regs[k].len).wrapping_mul(FP_STEP);
            h = (h ^ self.regs[k].accepted as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.scan_claims as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 三个 robust_list_head 地址（八字对齐）。
pub const HEADS: [u64; 3] = [0x7f30_0000_0000, 0x7f30_0000_0018, 0x7f30_0000_0030];

/// 组一张三线程接收态账。
pub fn build() -> RobustReg {
    let mut r = RobustReg::new();
    let mut k = 0usize;
    while k < WANT {
        let _ = r.register(k as u32 + 1, HEADS[k], HEAD_LEN);
        k += 1;
    }
    r
}

/// 四路对抗：空 head、错长、错位、声明遍历后再冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = RobustReg::new();
    out[0] = a.register(1, 0, HEAD_LEN) == Err(RB_PTR);
    out[1] = a.register(1, HEADS[0], 16) == Err(RB_LEN);
    out[2] = a.register(1, HEADS[0] + 4, HEAD_LEN) == Err(RB_ALIGN);
    let mut b = build();
    let _ = b.scan_on_exit();
    out[3] = b.freeze() == Err(RB_SCAN);
    out
}

/// 判据入口。
pub fn run_unx11229_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11229");
    let mut r = build();
    let root = r.freeze().unwrap_or(0);

    s.add("J1-三线程", r.regs.len() == WANT, "三条 robust list 接收在册");
    s.add("J2-接收不解析", r.opaque(), "head 一律按不透明地址保存，未跟随链表");
    s.add("J3-长度二十四", r.regs.iter().all(|x| x.len == HEAD_LEN), "每项 len 等于 robust_list_head 尺寸");
    s.add("J4-地址对齐", r.regs.iter().all(|x| x.head % 8 == 0), "每项 head 八字对齐");
    s.add("J5-退出遍历缺席", {
        let mut x = build();
        x.absent() && x.scan_on_exit() == Err(RB_SCAN)
    }, "遍历请求一律显式拒为 RB_SCAN");
    s.add("J6-冻存根", root != 0 && r.root == Some(root), "接受账冻存产出非零根");

    s.add("J7-冻后登记拒", r.register(9, HEADS[0] + 0x48, HEAD_LEN) == Err(RB_FROZEN), "冻存后登记拒为 RB_FROZEN");
    s.add("J8-冻后遍历拒", r.scan_on_exit() == Err(RB_FROZEN), "冻存后遍历请求拒为 RB_FROZEN");
    s.add("J9-空头拒", {
        let mut x = RobustReg::new();
        x.register(1, 0, HEAD_LEN) == Err(RB_PTR)
    }, "head 为零拒为 RB_PTR");
    s.add("J10-错长拒", {
        let mut x = RobustReg::new();
        x.register(1, HEADS[0], 32) == Err(RB_LEN)
    }, "len 非二十四拒为 RB_LEN");

    let mut d = build();
    let _ = d.scan_on_exit();
    s.add("J11-缺席计数", d.scan_claims == 1 && d.freeze() == Err(RB_SCAN), "一次遍历请求即记账并阻止冻存");

    let mut e = build();
    let _ = e.register(1, HEADS[1], HEAD_LEN);
    let dup = e.register(1, HEADS[2], HEAD_LEN);
    let _ = e.register(2, HEADS[2], HEAD_LEN);
    s.add("J12-重号拒", dup == Err(RB_REG), "线程号重录拒为 RB_REG");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "空头/错长/错位/声明遍历四路全拒");
    s.add_dyn(
        "J14-接收明细",
        r.regs.len() == 3 && r.regs.iter().all(|x| x.shaped()),
        &format!("接收 {} 条，遍历声明 {}，缺席态 {}", r.regs.len(), r.scan_claims, r.absent()),
    );
    s.add_dyn(
        "J15-账根",
        root == r.fold(),
        &format!("根 0x{:x}，首头 0x{:x}", root, r.regs[0].head),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11229_robust_list_flow() {
        let mut r = build();
        assert!(r.opaque());
        assert!(r.absent());
        let root = r.freeze().unwrap_or(0);
        assert!(root != 0);
        assert_eq!(r.scan_on_exit(), Err(RB_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11229_checks_green() {
        let s = run_unx11229_checks();
        assert!(s.all_passed(), "UNX-F11229 判据未全绿");
    }
}
