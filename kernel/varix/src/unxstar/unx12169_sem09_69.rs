//! UNX-F12169 — 段寄存器语义：cs/ss/ds/es/gs/fs（0x1a740 段）
//!
//! 锚点：UNX-F12169-J1 六段寄存器 10/10（x64 用户形制 CS=0x33/
//! SS=DS=ES=GS=0x2B/FS=0x53；RPL/CPL/TI 位拆解；内核形制 CS=0x08
//! /SS=0x18），段选择子校验、形制切换全账对平。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：SegSel（段选择子位型）+ SegBank（六段库）+ 形制切换 +
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a740;
pub const SG_RPL: u32 = SEG_BASE + 0x01; // RPL 越权
pub const SG_ORDER: u32 = SEG_BASE + 0x02; // 段序非法
pub const SG_SEALED: u32 = SEG_BASE + 0x03; // 印后切
pub const SG_GHOST: u32 = SEG_BASE + 0x04; // 野段名
pub const SG_TABLE: u32 = SEG_BASE + 0x05; // TI 位型非法

pub const SEG_NAMES: [&str; 6] = ["cs", "ss", "ds", "es", "fs", "gs"];

/// 段选择子位型：RPL[1:0] / TI[2] / Index[15:3]。
#[derive(Clone, Copy, PartialEq, Default)]
pub struct SegSel {
    pub raw: u16,
}

impl SegSel {
    pub fn new(raw: u16) -> SegSel {
        SegSel { raw }
    }

    pub fn rpl(&self) -> u8 {
        (self.raw & 0x3) as u8
    }

    pub fn ti(&self) -> u8 {
        ((self.raw >> 2) & 0x1) as u8
    }

    pub fn index(&self) -> u16 {
        self.raw >> 3
    }

    /// GDT 选择子（TI=0）。
    pub fn is_gdt(&self) -> bool {
        self.ti() == 0
    }

    pub fn render(&self) -> String {
        format!(
            "{:#04x} i={} ti={} rpl={}",
            self.raw, self.index(), self.ti(), self.rpl()
        )
    }
}

/// 六段库。
pub struct SegBank {
    pub sel: [SegSel; 6],
    kernel_mode: bool,
    sealed: bool,
}

impl SegBank {
    pub fn new() -> SegBank {
        SegBank { sel: SegBank::user_form(), kernel_mode: false, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn kernel_mode(&self) -> bool {
        self.kernel_mode
    }

    /// 用户形制：CS=0x33 SS/DS/ES/GS=0x2B FS=0x53。
    pub fn user_form() -> [SegSel; 6] {
        [SegSel::new(0x33), SegSel::new(0x2B), SegSel::new(0x2B), SegSel::new(0x2B), SegSel::new(0x53), SegSel::new(0x2B)]
    }

    /// 内核形制：CS=0x08 SS/DS/ES=0x18 FS/GS=0x30。
    pub fn kernel_form() -> [SegSel; 6] {
        [SegSel::new(0x08), SegSel::new(0x18), SegSel::new(0x18), SegSel::new(0x18), SegSel::new(0x30), SegSel::new(0x30)]
    }

    /// 按名读段。
    pub fn get(&self, name: &str) -> Result<SegSel, u32> {
        let idx = SEG_NAMES.iter().position(|n| *n == name).ok_or(SG_GHOST)?;
        Ok(self.sel[idx])
    }

    /// 形制切换（印后拒；重复同形制拒）。
    pub fn switch_mode(&mut self, kernel: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(SG_SEALED);
        }
        if self.kernel_mode == kernel {
            return Err(SG_ORDER);
        }
        self.kernel_mode = kernel;
        self.sel = if kernel { SegBank::kernel_form() } else { SegBank::user_form() };
        Ok(())
    }

    /// 全段形制一致核：各段值与目标形制逐段对平。
    pub fn form_consistent(&self) -> bool {
        let want = if self.kernel_mode { SegBank::kernel_form() } else { SegBank::user_form() };
        self.sel == want
    }

    /// RPL 全检：用户态全段 RPL=3、内核态 CS RPL=0。
    pub fn rpl_ok(&self) -> bool {
        if self.kernel_mode {
            self.sel[0].rpl() == 0
        } else {
            self.sel.iter().all(|s| s.rpl() == 3)
        }
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a740;
        for s in &self.sel {
            h = h.wrapping_mul(211).wrapping_add(s.raw as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.kernel_mode as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "k={} cs={:#04x} consistent={}",
            self.kernel_mode, self.sel[0].raw, self.form_consistent()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SegBank::new();
    // 1) 野段名读必拒。
    got[0] = b.get("xy").err() == Some(SG_GHOST);
    // 2) 同形制重复切换必拒。
    got[1] = b.switch_mode(false).err() == Some(SG_ORDER);
    // 3) RPL 越权选择子判非法（用户态塞 RPL=0 → RPL 核红）。
    let bad = SegSel::new(0x30);
    let mut ub = SegBank::new();
    ub.sel[0] = bad;
    got[2] = bad.rpl() == 0 && !ub.rpl_ok();
    // 4) LDT 选择子（TI=1）在 GDT 语境判非。
    let ldt = SegSel::new(0x7);
    got[3] = !ldt.is_gdt();
    // 5) 印后切必拒。
    b.seal();
    got[4] = b.switch_mode(true).err() == Some(SG_SEALED);
    got
}

/// 判据入口：UNX-F12169
pub fn run_unx12169_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12169");
    let mut b = SegBank::new();

    // J1 用户形制六段全对。
    let want = [0x33u16, 0x2B, 0x2B, 0x2B, 0x53, 0x2B];
    let got: Vec<u16> = b.sel.iter().map(|x| x.raw).collect();
    s.add(
        "J1_user_form",
        got == want && b.form_consistent(),
        "用户形制 CS33/SS2B/FS53 逐段对平",
    );

    // J2 用户态 RPL 全 3。
    s.add("J2_user_rpl", b.rpl_ok(), "用户态全段 RPL=3");

    // J3 按名读段 6/6。
    let mut j3 = true;
    for (i, n) in SEG_NAMES.iter().enumerate() {
        if b.get(n).map(|x| x.raw) != Ok(want[i]) {
            j3 = false;
        }
    }
    s.add("J3_by_name_6x", j3, "六段按名读全对");

    // J4 野段名拒。
    s.add(
        "J4_ghost_name",
        b.get("zz").err() == Some(SG_GHOST) && b.get("CS").err() == Some(SG_GHOST),
        "野名/大小写敏感拒 GHOST",
    );

    // J5 选择子位拆解（0x33：i=6 ti=0 rpl=3）。
    let sel33 = SegSel::new(0x33);
    s.add(
        "J5_bits",
        sel33.index() == 6 && sel33.ti() == 0 && sel33.rpl() == 3,
        "0x33 拆 i6/ti0/rpl3",
    );

    // J6 渲染。
    let r = sel33.render();
    s.add(
        "J6_render",
        r.contains("0x33") && r.contains("rpl=3"),
        "渲染含值/索引/RPL",
    );

    // J7 切内核形制。
    let sw = b.switch_mode(true);
    s.add(
        "J7_to_kernel",
        sw.is_ok() && b.kernel_mode() && b.get("cs").map(|x| x.raw) == Ok(0x08),
        "切内核 CS=0x08",
    );

    // J8 内核形制全段对平。
    let kwant = [0x08u16, 0x18, 0x18, 0x18, 0x30, 0x30];
    let kgot: Vec<u16> = b.sel.iter().map(|x| x.raw).collect();
    s.add(
        "J8_kernel_form",
        kgot == kwant && b.form_consistent(),
        "内核形制逐段对平",
    );

    // J9 内核态 CS RPL=0。
    s.add(
        "J9_kernel_rpl",
        b.rpl_ok() && b.get("cs").map(|x| x.rpl()) == Ok(0),
        "内核 CS RPL=0",
    );

    // J10 切回用户态。
    let back = b.switch_mode(false);
    s.add(
        "J10_back_user",
        back.is_ok() && !b.kernel_mode() && b.get("cs").map(|x| x.raw) == Ok(0x33),
        "切回用户 CS=0x33",
    );

    // J11 同形制重复切拒。
    s.add(
        "J11_dup_switch",
        b.switch_mode(false).err() == Some(SG_ORDER),
        "同形制切拒 ORDER",
    );

    // J12 LDT 位型。
    let ldt = SegSel::new(0x7);
    s.add(
        "J12_ldt",
        ldt.ti() == 1 && !ldt.is_gdt() && ldt.index() == 0,
        "TI=1 为 LDT 且索引 0",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "段库折叠指纹一致");

    // J14 切换动指纹。
    let f_before = b.fold();
    let _ = b.switch_mode(true);
    s.add("J14_fp_moves", b.fold() != f_before, "切形制动指纹");

    // J15 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J15_inject_all", caught == 5, "5 注入点全检出");

    // J16 印账受控。
    b.seal();
    s.add(
        "J16_sealed",
        b.switch_mode(false).err() == Some(SG_SEALED),
        "印账后切拒",
    );

    // J17 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J17_summary",
        sm.contains("k=true") && sm.contains("cs=0x08"),
        "摘要含形制与 CS",
    );

    // J18 十次形制往返全对。
    let mut b18 = SegBank::new();
    let mut ok18 = true;
    for i in 0..10 {
        if b18.switch_mode(i % 2 == 0).is_err() || !b18.form_consistent() {
            ok18 = false;
        }
    }
    s.add("J18_flip10", ok18, "十次形制切换全一致");

    // J19 空库指纹稳定。
    let b19 = SegBank::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空段库指纹稳定");

    // J20 静态形制函数双查。
    let u = SegBank::user_form();
    let k = SegBank::kernel_form();
    s.add(
        "J20_forms",
        u[0].raw == 0x33 && k[0].raw == 0x08,
        "双形制函数首段对平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12169_segs() {
        let mut b = SegBank::new();
        assert_eq!(b.get("cs").map(|x| x.raw), Ok(0x33));
        assert!(b.switch_mode(true).is_ok());
        assert_eq!(b.get("cs").map(|x| x.raw), Ok(0x08));
    }

    #[test]
    fn unx12169_checks_green() {
        let s = run_unx12169_checks();
        assert!(s.all_passed(), "UNX-F12169 判据未全绿");
    }

    #[test]
    fn unx12169_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
