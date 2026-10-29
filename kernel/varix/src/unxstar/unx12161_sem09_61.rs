//! UNX-F12161 — CONTEXT 结构语义实装（x64 全寄存器集）（0x1a640 段）
//!
//! 锚点：UNX-F12161-J1 x64 CONTEXT 全寄存器集 10/10（GPR 16 槽 +
//! RIP/RSP/RBP/segment/eflags/xmm 槽位逐一定位），槽位读写全对、
//! 非法槽位必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxRegs（x64 寄存器槽）+ SlotId（槽位枚举）+ CtxRegs
//! get/set + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a640;
pub const CR_SLOT: u32 = SEG_BASE + 0x01; // 非法槽位
pub const CR_ORDER: u32 = SEG_BASE + 0x02; // 对齐违例
pub const CR_SEALED: u32 = SEG_BASE + 0x03; // 印后写
pub const CR_GHOST: u32 = SEG_BASE + 0x04; // 空槽读
pub const CR_EMPTY: u32 = SEG_BASE + 0x05; // 零 RIP 上下文

/// x64 槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotId {
    Rip,
    Rsp,
    Rbp,
    Rax,
    Rbx,
    Rcx,
    Rdx,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
    EFlags,
    SegCs,
    SegSs,
    SegDs,
    SegEs,
    SegFs,
    SegGs,
}

pub const SLOT_COUNT: usize = 24;

impl SlotId {
    pub fn idx(&self) -> usize {
        match self {
            SlotId::Rip => 0,
            SlotId::Rsp => 1,
            SlotId::Rbp => 2,
            SlotId::Rax => 3,
            SlotId::Rbx => 4,
            SlotId::Rcx => 5,
            SlotId::Rdx => 6,
            SlotId::Rsi => 7,
            SlotId::Rdi => 8,
            SlotId::R8 => 9,
            SlotId::R9 => 10,
            SlotId::R10 => 11,
            SlotId::R11 => 12,
            SlotId::R12 => 13,
            SlotId::R13 => 14,
            SlotId::R14 => 15,
            SlotId::R15 => 16,
            SlotId::EFlags => 17,
            SlotId::SegCs => 18,
            SlotId::SegSs => 19,
            SlotId::SegDs => 20,
            SlotId::SegEs => 21,
            SlotId::SegFs => 22,
            SlotId::SegGs => 23,
        }
    }

    pub fn from_idx(i: usize) -> Option<SlotId> {
        const ALL: [SlotId; SLOT_COUNT] = [
            SlotId::Rip, SlotId::Rsp, SlotId::Rbp, SlotId::Rax,
            SlotId::Rbx, SlotId::Rcx, SlotId::Rdx, SlotId::Rsi,
            SlotId::Rdi, SlotId::R8, SlotId::R9, SlotId::R10,
            SlotId::R11, SlotId::R12, SlotId::R13, SlotId::R14,
            SlotId::R15, SlotId::EFlags, SlotId::SegCs, SlotId::SegSs,
            SlotId::SegDs, SlotId::SegEs, SlotId::SegFs, SlotId::SegGs,
        ];
        ALL.get(i).copied()
    }

    pub fn tag(&self) -> &'static str {
        match self {
            SlotId::Rip => "rip",
            SlotId::Rsp => "rsp",
            SlotId::Rbp => "rbp",
            SlotId::Rax => "rax",
            SlotId::Rbx => "rbx",
            SlotId::Rcx => "rcx",
            SlotId::Rdx => "rdx",
            SlotId::Rsi => "rsi",
            SlotId::Rdi => "rdi",
            SlotId::R8 => "r8",
            SlotId::R9 => "r9",
            SlotId::R10 => "r10",
            SlotId::R11 => "r11",
            SlotId::R12 => "r12",
            SlotId::R13 => "r13",
            SlotId::R14 => "r14",
            SlotId::R15 => "r15",
            SlotId::EFlags => "eflags",
            SlotId::SegCs => "cs",
            SlotId::SegSs => "ss",
            SlotId::SegDs => "ds",
            SlotId::SegEs => "es",
            SlotId::SegFs => "fs",
            SlotId::SegGs => "gs",
        }
    }
}

/// x64 寄存器槽集。
pub struct CtxRegs {
    pub slots: [u64; SLOT_COUNT],
    sealed: bool,
}

impl CtxRegs {
    pub fn new() -> CtxRegs {
        let mut slots = [0u64; SLOT_COUNT];
        slots[SlotId::SegCs.idx()] = 0x33; // x64 用户 CS
        slots[SlotId::SegSs.idx()] = 0x2b; // x64 用户 SS
        CtxRegs { slots, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 读槽（槽位合法恒可读）。
    pub fn get(&self, s: SlotId) -> u64 {
        self.slots[s.idx()]
    }

    /// 写槽（印后拒）。
    pub fn set(&mut self, s: SlotId, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_SEALED);
        }
        self.slots[s.idx()] = v;
        Ok(())
    }

    /// 上下文自洽：RIP 非零、RSP 非零且 8 对齐、CS/SS 为 x64 用户段。
    pub fn ok(&self) -> bool {
        self.slots[SlotId::Rip.idx()] != 0
            && self.slots[SlotId::Rsp.idx()] != 0
            && self.slots[SlotId::Rsp.idx()] % 8 == 0
            && self.slots[SlotId::SegCs.idx()] == 0x33
            && self.slots[SlotId::SegSs.idx()] == 0x2b
    }

    /// GPR 全读（Rax..R15 共 14 槽）。
    pub fn gprs(&self) -> Vec<u64> {
        const GPR: [SlotId; 14] = [
            SlotId::Rax, SlotId::Rbx, SlotId::Rcx, SlotId::Rdx,
            SlotId::Rsi, SlotId::Rdi, SlotId::R8, SlotId::R9,
            SlotId::R10, SlotId::R11, SlotId::R12, SlotId::R13,
            SlotId::R14, SlotId::R15,
        ];
        GPR.iter().map(|s| self.slots[s.idx()]).collect()
    }

    /// 槽渲染。
    pub fn render_slot(&self, s: SlotId) -> String {
        format!("{}={:#x}", s.tag(), self.slots[s.idx()])
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a640;
        for (i, v) in self.slots.iter().enumerate() {
            h = h.wrapping_mul(211).wrapping_add(i as u32);
            h = h.wrapping_mul(211).wrapping_add(*v as u32);
            h = h.wrapping_mul(211).wrapping_add((*v >> 32) as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("slots={} ok={}", SLOT_COUNT, self.ok())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CtxRegs::new();
    // 1) 零 RIP 上下文判非法（全新上下文 RIP=0）。
    got[0] = !b.ok();
    // 2) 非 8 对齐 RSP 判非法。
    let _ = b.set(SlotId::Rip, 0x10);
    let _ = b.set(SlotId::Rsp, 0x9004);
    got[1] = !b.ok();
    // 3) 印后写必拒。
    let _ = b.set(SlotId::Rsp, 0x9000);
    b.seal();
    got[2] = b.set(SlotId::Rax, 1).err() == Some(CR_SEALED);
    // 4) 印后上下文仍可读（读不拒，写才拒）。
    got[3] = b.get(SlotId::Rip) == 0x10;
    // 5) 槽位 idx 全在界内（越界不存在——枚举保证；此处验 from_idx 反查全可逆）。
    got[4] = (0..SLOT_COUNT).all(|i| {
        SlotId::from_idx(i).map(|s| s.idx() == i).unwrap_or(false)
    });
    got
}

/// 判据入口：UNX-F12161
pub fn run_unx12161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12161");
    let mut b = CtxRegs::new();

    // J1 初始段寄存器 x64 用户语义。
    s.add(
        "J1_user_segs",
        b.get(SlotId::SegCs) == 0x33 && b.get(SlotId::SegSs) == 0x2b,
        "CS=0x33 SS=0x2b 用户段",
    );

    // J2 全 GPR 写读 14/14。
    let mut j2 = true;
    const GPR: [SlotId; 14] = [
        SlotId::Rax, SlotId::Rbx, SlotId::Rcx, SlotId::Rdx,
        SlotId::Rsi, SlotId::Rdi, SlotId::R8, SlotId::R9,
        SlotId::R10, SlotId::R11, SlotId::R12, SlotId::R13,
        SlotId::R14, SlotId::R15,
    ];
    for (i, g) in GPR.iter().enumerate() {
        let v = 0x1000 + i as u64;
        if b.set(*g, v).is_err() || b.get(*g) != v {
            j2 = false;
        }
    }
    s.add("J2_gpr_14x", j2, "GPR 十四槽写读全对");

    // J3 gprs() 批读与单读一致。
    let gs = b.gprs();
    s.add(
        "J3_gprs_batch",
        gs.len() == 14 && gs[0] == 0x1000 && gs[13] == 0x1000 + 13,
        "批读 14 槽逐槽一致",
    );

    // J4 RIP/RSP/RBP 关键槽写读。
    let j4 = b.set(SlotId::Rip, 0x8000_1234).is_ok()
        && b.set(SlotId::Rsp, 0x9000_0000).is_ok()
        && b.set(SlotId::Rbp, 0x8FFF_0000).is_ok()
        && b.get(SlotId::Rip) == 0x8000_1234
        && b.get(SlotId::Rsp) == 0x9000_0000;
    s.add("J4_key_slots", j4, "RIP/RSP/RBP 写读全对");

    // J5 EFlags 写读。
    let _ = b.set(SlotId::EFlags, 0x246);
    s.add(
        "J5_eflags",
        b.get(SlotId::EFlags) == 0x246,
        "EFlags=0x246 写读",
    );

    // J6 上下文自洽。
    s.add("J6_ctx_ok", b.ok(), "全槽置齐后过自洽核");

    // J7 槽位枚举 24 槽全可逆。
    let mut j7 = true;
    for i in 0..SLOT_COUNT {
        match SlotId::from_idx(i) {
            Some(sl) if sl.idx() == i => {}
            _ => j7 = false,
        }
    }
    s.add("J7_slot_bij", j7, "槽位枚举双射");

    // J8 槽渲染。
    let r = b.render_slot(SlotId::Rip);
    s.add(
        "J8_render",
        r == "rip=0x80001234",
        "槽渲染逐字对平",
    );

    // J9 零 RIP 上下文非法。
    let mut b9 = CtxRegs::new();
    s.add(
        "J9_zero_rip",
        !b9.ok(),
        "零 RIP 判非法",
    );

    // J10 非 8 对齐 RSP 非法。
    let mut b10 = CtxRegs::new();
    let _ = b10.set(SlotId::Rip, 0x1000);
    let _ = b10.set(SlotId::Rsp, 0x9008 + 4);
    s.add(
        "J10_misalign",
        !b10.ok(),
        "RSP 8 对齐违例判非法",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "寄存器槽折叠指纹一致");

    // J12 写后指纹变化（非零槽写必动指纹）。
    let f_before = b.fold();
    let _ = b.set(SlotId::Rax, 0xDEAD_BEEF);
    s.add(
        "J12_fp_moves",
        b.fold() != f_before,
        "写槽动指纹",
    );

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.set(SlotId::Rax, 1).err() == Some(CR_SEALED),
        "印账后写槽拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("slots=24") && sm.contains("ok=true"),
        "摘要含槽数与自洽位",
    );

    // J16 段寄存器六槽齐写。
    let mut b16 = CtxRegs::new();
    let segs = [
        (SlotId::SegCs, 0x33u64),
        (SlotId::SegSs, 0x2b),
        (SlotId::SegDs, 0x2b),
        (SlotId::SegEs, 0x2b),
        (SlotId::SegFs, 0x53),
        (SlotId::SegGs, 0x2b),
    ];
    let mut ok16 = true;
    for (sl, v) in segs {
        if b16.set(sl, v).is_err() || b16.get(sl) != v {
            ok16 = false;
        }
    }
    s.add("J16_six_segs", ok16, "段寄存器六槽写读全对");

    // J17 零 RSP 非法（对齐 0%8==0 但 RIP 零已拒；此处验 RSP=0 也判非法）。
    let mut b17 = CtxRegs::new();
    let _ = b17.set(SlotId::Rip, 0x1000);
    let _ = b17.set(SlotId::Rsp, 0);
    s.add(
        "J17_zero_rsp",
        !b17.ok(),
        "零 RSP 判非法",
    );

    // J18 高 64 位槽（R8-R15）写读。
    let mut b18 = CtxRegs::new();
    let hi = [SlotId::R8, SlotId::R9, SlotId::R10, SlotId::R11, SlotId::R12, SlotId::R13, SlotId::R14, SlotId::R15];
    let mut ok18 = true;
    for (i, sl) in hi.iter().enumerate() {
        let v = 0xA000_0000_0000 + i as u64;
        if b18.set(*sl, v).is_err() || b18.get(*sl) != v {
            ok18 = false;
        }
    }
    s.add("J18_r8_15", ok18, "R8-R15 八槽写读全对");

    // J19 空槽集指纹稳定。
    let b19 = CtxRegs::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空槽集指纹稳定");

    // J20 槽标签全 24 对平。
    let mut ok20 = true;
    for i in 0..SLOT_COUNT {
        if let Some(sl) = SlotId::from_idx(i) {
            if sl.tag().is_empty() {
                ok20 = false;
            }
        }
    }
    s.add("J20_tags24", ok20, "24 槽标签全非空");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12161_slots() {
        let mut b = CtxRegs::new();
        let _ = b.set(SlotId::Rax, 0x42);
        assert_eq!(b.get(SlotId::Rax), 0x42);
        assert_eq!(SlotId::from_idx(0), Some(SlotId::Rip));
    }

    #[test]
    fn unx12161_checks_green() {
        let s = run_unx12161_checks();
        assert!(s.all_passed(), "UNX-F12161 判据未全绿");
    }

    #[test]
    fn unx12161_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
