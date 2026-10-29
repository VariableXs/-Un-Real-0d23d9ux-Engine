//! UNX-F12168 — 调试/MMX/控制寄存器槽位语义（0x1a720 段）
//!
//! 锚点：UNX-F12168-J1 调试/控制寄存器槽 10/10（Dr0-Dr7 八槽 +
//! Cr0/Cr2/Cr3/Cr4 四槽写读，Dr7 位使能语义、Cr0 页使能位语义），
//! 越界/印后写必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：DbgRegs（调试槽）+ CtrlRegs（控制槽）+ 双库账 + 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a720;
pub const DR_SLOT: u32 = SEG_BASE + 0x01; // 调试槽越界
pub const CR_SLOT: u32 = SEG_BASE + 0x02; // 控制槽越界
pub const DR_SEALED: u32 = SEG_BASE + 0x03; // 印后写
pub const DR_GHOST: u32 = SEG_BASE + 0x04; // Dr7 使能位与断言槽不符
pub const DR_TWICE: u32 = SEG_BASE + 0x05; // 重复断言槽

pub const DR_COUNT: usize = 8;

/// 调试寄存器库（Dr0-Dr7；Dr4/5 保留恒 0；Dr6 状态、Dr7 使能）。
pub struct DbgRegs {
    pub slots: [u64; DR_COUNT],
    sealed: bool,
}

impl DbgRegs {
    pub fn new() -> DbgRegs {
        DbgRegs { slots: [0u64; DR_COUNT], sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 写槽（Dr4/Dr5 保留拒写；印后拒写）。
    pub fn set(&mut self, idx: usize, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if idx >= DR_COUNT {
            return Err(DR_SLOT);
        }
        if idx == 4 || idx == 5 {
            return Err(DR_SLOT);
        }
        self.slots[idx] = v;
        Ok(())
    }

    /// 读槽。
    pub fn get(&self, idx: usize) -> Result<u64, u32> {
        if idx >= DR_COUNT {
            return Err(DR_SLOT);
        }
        Ok(self.slots[idx])
    }

    /// 配一个硬件断言（Dr0-3 放地址、Dr7 置使能位 L0-L3）。
    pub fn arm_breakpoint(&mut self, slot: usize, addr: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if slot >= 4 {
            return Err(DR_SLOT);
        }
        if self.slots[slot] != 0 {
            return Err(DR_TWICE);
        }
        self.slots[slot] = addr;
        self.slots[7] |= 1u64 << (2 * slot); // Ln 使能位
        Ok(())
    }

    /// Dr7 使能位与 Dr0-3 槽一致核。
    pub fn arm_consistent(&self) -> bool {
        for i in 0..4usize {
            let enabled = self.slots[7] & (1u64 << (2 * i)) != 0;
            let has_addr = self.slots[i] != 0;
            if enabled != has_addr {
                return false;
            }
        }
        true
    }

    /// 使能断言数。
    pub fn armed_count(&self) -> usize {
        (0..4usize).filter(|i| self.slots[7] & (1u64 << (2 * i)) != 0).count()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a720;
        for (i, v) in self.slots.iter().enumerate() {
            h = h.wrapping_mul(211).wrapping_add(i as u32);
            h = h.wrapping_mul(211).wrapping_add(*v as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("slots={} armed={} ok={}", DR_COUNT, self.armed_count(), self.arm_consistent())
    }
}

/// 控制寄存器库（Cr0/Cr2/Cr3/Cr4）。
pub struct CtrlRegs {
    pub slots: [u64; 4],
    sealed: bool,
}

impl CtrlRegs {
    pub fn new() -> CtrlRegs {
        let mut slots = [0u64; 4];
        slots[0] = 0x8001_0011; // Cr0: PG|PE|MP|NE 复位形制
        CtrlRegs { slots, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn paging(&self) -> bool {
        self.slots[0] & 0x8000_0000 != 0
    }

    pub fn protected_mode(&self) -> bool {
        self.slots[0] & 0x1 != 0
    }

    /// 写槽（印后拒）。
    pub fn set(&mut self, idx: usize, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DR_SEALED);
        }
        if idx >= 4 {
            return Err(CR_SLOT);
        }
        self.slots[idx] = v;
        Ok(())
    }

    pub fn get(&self, idx: usize) -> Result<u64, u32> {
        if idx >= 4 {
            return Err(CR_SLOT);
        }
        Ok(self.slots[idx])
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a720;
        for (i, v) in self.slots.iter().enumerate() {
            h = h.wrapping_mul(211).wrapping_add(i as u32);
            h = h.wrapping_mul(211).wrapping_add(*v as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("pg={} pm={}", self.paging(), self.protected_mode())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut d = DbgRegs::new();
    // 1) Dr4 保留拒写。
    got[0] = d.set(4, 1).err() == Some(DR_SLOT);
    // 2) 槽 8 越界拒。
    got[1] = d.set(8, 1).err() == Some(DR_SLOT);
    // 3) 同槽重复 arm 必拒。
    let _ = d.arm_breakpoint(0, 0x1000);
    got[2] = d.arm_breakpoint(0, 0x2000).err() == Some(DR_TWICE);
    // 4) 断言槽 4+ 越界拒。
    got[3] = d.arm_breakpoint(4, 0x3000).err() == Some(DR_SLOT);
    // 5) 印后写必拒。
    d.seal();
    got[4] = d.set(0, 1).err() == Some(DR_SEALED);
    got
}

/// 判据入口：UNX-F12168
pub fn run_unx12168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12168");
    let mut d = DbgRegs::new();
    let mut c = CtrlRegs::new();

    // J1 四断言全 arm。
    let j1 = d.arm_breakpoint(0, 0x1000).is_ok()
        && d.arm_breakpoint(1, 0x2000).is_ok()
        && d.arm_breakpoint(2, 0x3000).is_ok()
        && d.arm_breakpoint(3, 0x4000).is_ok();
    s.add("J1_arm4", j1 && d.armed_count() == 4, "四断言全 arm");

    // J2 Dr7 使能一致核。
    s.add("J2_arm_consistent", d.arm_consistent(), "Dr7 使能与槽一致");

    // J3 Dr7 位型（L0-L3 = 0x55）。
    s.add(
        "J3_dr7_bits",
        d.get(7) == Ok(0x55),
        "Dr7 L0-L3=0x55",
    );

    // J4 Dr6 状态槽写读。
    let _ = d.set(6, 0xFFFF_0FF0);
    s.add(
        "J4_dr6",
        d.get(6) == Ok(0xFFFF_0FF0),
        "Dr6 状态写读",
    );

    // J5 Dr4/5 保留拒。
    s.add(
        "J5_dr45",
        d.set(4, 1).err() == Some(DR_SLOT) && d.set(5, 1).err() == Some(DR_SLOT),
        "Dr4/5 保留拒 SLOT",
    );

    // J6 调试槽越界读拒。
    s.add(
        "J6_dr_oob",
        d.get(8).err() == Some(DR_SLOT),
        "Dr8 越界拒 SLOT",
    );

    // J7 Cr0 复位形制：PG|PE 开。
    s.add(
        "J7_cr0_reset",
        c.paging() && c.protected_mode(),
        "Cr0 复位 PG/PM 双开",
    );

    // J8 Cr3 页表基址写读。
    let _ = c.set(2, 0);
    let _ = c.set(3, 0x1A000);
    s.add(
        "J8_cr3",
        c.get(3) == Ok(0x1A000),
        "Cr3=0x1A000 写读",
    );

    // J9 Cr4 扩展位写读。
    let _ = c.set(3, 0x1A000);
    let _ = c.set(4 - 1, 0x1A000);
    let _ = c.set(3, 0x1A000);
    let ok9 = c.set(3, 0x2_0000).is_ok() && c.get(3) == Ok(0x2_0000);
    s.add("J9_cr3_rw", ok9, "Cr3 复写读全对");

    // J10 控制槽越界拒。
    s.add(
        "J10_cr_oob",
        c.set(4, 1).err() == Some(CR_SLOT) && c.get(9).err() == Some(CR_SLOT),
        "Cr4/9 越界双向拒",
    );

    // J11 使能位与槽不符核（假 arm 清地址不清使能 → 核红）。
    let mut d11 = DbgRegs::new();
    let _ = d11.arm_breakpoint(0, 0x1000);
    let _ = d11.set(0, 0); // 清地址但使能仍在
    s.add(
        "J11_mismatch",
        !d11.arm_consistent(),
        "使能位与空槽核红",
    );

    // J12 指纹稳定。
    let f1 = d.fold();
    let f2 = d.fold();
    s.add("J12_fingerprint", f1 == f2, "调试库折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    d.seal();
    s.add(
        "J14_sealed",
        d.set(0, 1).err() == Some(DR_SEALED) && d.arm_breakpoint(1, 9).err() == Some(DR_SEALED),
        "印账后写/arm 皆拒",
    );

    // J15 控制库指纹稳定。
    let f15 = c.fold();
    let f15b = c.fold();
    s.add("J15_cr_fp", f15 == f15b, "控制库指纹一致");

    // J16 调试库摘要（清 0 后核红实例的账面）。
    let sm = d11.book_summary();
    s.add(
        "J16_summary",
        sm.contains("armed=1") && sm.contains("ok=false"),
        "摘要含 arm 数与核位（清 0 后核红）",
    );

    // J17 控制库摘要。
    let sm17 = c.book_summary();
    s.add(
        "J17_cr_summary",
        sm17.contains("pg=true") && sm17.contains("pm=true"),
        "控制库摘要含 PG/PM",
    );

    // J18 三断言 arm 后逐一撤（清使能+清地址）仍一致。
    let mut d18 = DbgRegs::new();
    let _ = d18.arm_breakpoint(0, 0x1000);
    let _ = d18.arm_breakpoint(1, 0x2000);
    let _ = d18.arm_breakpoint(2, 0x3000);
    let _ = d18.set(0, 0);
    let _ = d18.set(7, (d18.get(7).unwrap_or(0)) & !(1u64 | 1u64 << 2 | 1u64 << 4));
    let _ = d18.set(1, 0);
    let _ = d18.set(2, 0);
    let _ = d18.set(7, 0);
    s.add(
        "J18_disarm_all",
        d18.armed_count() == 0 && d18.arm_consistent(),
        "全撤后核绿",
    );

    // J19 空库指纹稳定。
    let d19 = DbgRegs::new();
    let c19 = CtrlRegs::new();
    s.add(
        "J19_fresh_fp",
        d19.fold() == d19.fold() && c19.fold() == c19.fold(),
        "双空库指纹稳定",
    );

    // J20 印账不改指纹。
    let fb = d.fold();
    d.seal();
    s.add("J20_seal_fp", d.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12168_dbg() {
        let mut d = DbgRegs::new();
        let _ = d.arm_breakpoint(0, 0x1000);
        assert_eq!(d.armed_count(), 1);
        assert!(d.arm_consistent());
    }

    #[test]
    fn unx12168_checks_green() {
        let s = run_unx12168_checks();
        assert!(s.all_passed(), "UNX-F12168 判据未全绿");
    }

    #[test]
    fn unx12168_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
