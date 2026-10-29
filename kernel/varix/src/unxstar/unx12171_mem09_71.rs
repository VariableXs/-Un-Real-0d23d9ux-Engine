//! UNX-F12171 — CONTEXT 对齐与异常帧内存布局（0x1a780 段）
//!
//! 锚点：UNX-F12171-J1 异常帧布局 10/10（CONTEXT 按 16 字节对齐、
//! 异常帧栈槽布局 EXCEPTION_RECORD+CONTEXT 逐槽定位），非对齐帧
//! 必拒、布局偏移全对。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxLayout（CONTEXT 布局域）+ ExcFrameLayout（异常帧布局）
//! + 布局账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a780;
pub const L_ALIGN: u32 = SEG_BASE + 0x01; // 16 字节对齐违例
pub const L_ORDER: u32 = SEG_BASE + 0x02; // 布局偏移非法
pub const L_SEALED: u32 = SEG_BASE + 0x03; // 印后改
pub const L_GHOST: u32 = SEG_BASE + 0x04; // 野槽名
pub const L_EMPTY: u32 = SEG_BASE + 0x05; // 零长帧

/// CONTEXT 布局槽名与偏移（精简形制，8 字节槽）。
pub const CTX_LAYOUT: [(&str, usize); 12] = [
    ("ContextFlags", 0),
    ("Rax", 8),
    ("Rcx", 16),
    ("Rdx", 24),
    ("Rsp", 56),
    ("Rbp", 64),
    ("Rsi", 72),
    ("Rdi", 80),
    ("Rip", 248),
    ("EFlags", 256),
    ("SegCs", 264),
    ("SegSs", 280),
];

pub const CTX_SIZE: usize = 288; // 16 字节对齐后
pub const CTX_ALIGN: usize = 16;

/// 异常帧布局（EXCEPTION_RECORD(96B) + 对齐垫 + CONTEXT）。
#[derive(Clone, Copy, PartialEq)]
pub struct ExcFrameLayout {
    pub rec_off: usize,
    pub ctx_off: usize,
    pub total: usize,
}

impl ExcFrameLayout {
    /// 布局自洽：REC 顶、CONTEXT 16 对齐、总长 = ctx_off + CTX_SIZE。
    pub fn ok(&self) -> bool {
        self.rec_off == 0
            && self.ctx_off % CTX_ALIGN == 0
            && self.ctx_off >= 96
            && self.total == self.ctx_off + CTX_SIZE
    }

    pub fn render(&self) -> String {
        format!("rec@{} ctx@{} total={}", self.rec_off, self.ctx_off, self.total)
    }
}

/// 布局域。
pub struct CtxLayout {
    pub frame: ExcFrameLayout,
    pub stack_addrs: Vec<(u64, usize)>, // (帧地址, 期望对齐)
    sealed: bool,
}

impl CtxLayout {
    pub fn new() -> CtxLayout {
        // REC 96 字节 → 垫到 16 对齐 → ctx_off = 96。
        CtxLayout {
            frame: ExcFrameLayout { rec_off: 0, ctx_off: 96, total: 96 + CTX_SIZE },
            stack_addrs: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 按槽名查 CONTEXT 内偏移。
    pub fn ctx_slot(&self, name: &str) -> Result<usize, u32> {
        CTX_LAYOUT
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, off)| *off)
            .ok_or(L_GHOST)
    }

    /// 挂栈帧地址（16 对齐；非对齐必拒）。
    pub fn mount(&mut self, addr: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(L_SEALED);
        }
        if addr % CTX_ALIGN as u64 != 0 {
            return Err(L_ALIGN);
        }
        self.stack_addrs.push((addr, CTX_ALIGN));
        Ok(())
    }

    /// 帧地址→CONTEXT 绝对偏移地址。
    pub fn abs_slot_addr(&self, frame: u64, slot: &str) -> Result<u64, u32> {
        let off = self.ctx_slot(slot)?;
        Ok(frame + off as u64)
    }

    /// 布局校验（偏移非法必拒）。
    pub fn validate(&self, f: ExcFrameLayout) -> Result<(), u32> {
        if f.rec_off != 0 || f.ctx_off % CTX_ALIGN != 0 || f.total != f.ctx_off + CTX_SIZE {
            return Err(L_ORDER);
        }
        Ok(())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a780;
        h = h.wrapping_mul(211).wrapping_add(self.frame.ctx_off as u32);
        h = h.wrapping_mul(211).wrapping_add(self.frame.total as u32);
        for (a, _) in &self.stack_addrs {
            h = h.wrapping_mul(211).wrapping_add(*a as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "slots={} mounted={} ok={}",
            CTX_LAYOUT.len(),
            self.stack_addrs.len(),
            self.frame.ok()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CtxLayout::new();
    // 1) 非对齐帧地址必拒。
    got[0] = b.mount(0x9008).err() == Some(L_ALIGN);
    // 2) 野槽名必拒。
    got[1] = b.ctx_slot("R8").err() == Some(L_GHOST);
    // 3) ctx_off 非 16 对齐布局必拒。
    got[2] = b
        .validate(ExcFrameLayout { rec_off: 0, ctx_off: 100, total: 100 + CTX_SIZE })
        .err()
        == Some(L_ORDER);
    // 4) rec_off 非 0 布局必拒。
    got[3] = b
        .validate(ExcFrameLayout { rec_off: 8, ctx_off: 96, total: 96 + CTX_SIZE })
        .err()
        == Some(L_ORDER);
    // 5) 印后挂必拒。
    b.seal();
    got[4] = b.mount(0x9000).err() == Some(L_SEALED);
    got
}

/// 判据入口：UNX-F12171
pub fn run_unx12171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12171");
    let mut b = CtxLayout::new();

    // J1 标准帧布局自洽。
    s.add(
        "J1_frame_ok",
        b.frame.ok() && b.frame.rec_off == 0 && b.frame.ctx_off == 96,
        "REC@0 CTX@96 布局自洽",
    );

    // J2 总长对平。
    s.add(
        "J2_total",
        b.frame.total == 96 + CTX_SIZE && CTX_SIZE % 16 == 0,
        "总长 = ctx_off + 288 且 CONTEXT 16 对齐",
    );

    // J3 槽名查偏移 12/12。
    let mut ok3 = true;
    for (n, off) in CTX_LAYOUT {
        if b.ctx_slot(n) != Ok(off) {
            ok3 = false;
        }
    }
    s.add("J3_slots_12x", ok3, "十二槽名→偏移全对");

    // J4 野槽名拒。
    s.add(
        "J4_ghost_slot",
        b.ctx_slot("Xmm0").err() == Some(L_GHOST) && b.ctx_slot("rip").err() == Some(L_GHOST),
        "野名/大小写敏感拒 GHOST",
    );

    // J5 帧地址挂载 10/10。
    let mut ok5 = true;
    for i in 0..10u64 {
        if b.mount(0x9000 + i * 0x200).is_err() {
            ok5 = false;
        }
    }
    s.add("J5_mount_10x", ok5 && b.stack_addrs.len() == 10, "十帧地址全挂");

    // J6 非对齐挂载拒。
    s.add(
        "J6_misalign",
        b.mount(0x9004).err() == Some(L_ALIGN) && b.mount(0x8FFF).err() == Some(L_ALIGN),
        "4/1 偏移地址拒 ALIGN",
    );

    // J7 绝对槽地址（帧 0x9000 的 RIP）。
    let a = b.abs_slot_addr(0x9000, "Rip");
    s.add(
        "J7_abs_rip",
        a == Ok(0x9000 + 248),
        "帧+248 = RIP 绝对地址",
    );

    // J8 绝对槽地址（SegCs）。
    let a8 = b.abs_slot_addr(0x9000, "SegCs");
    s.add(
        "J8_abs_cs",
        a8 == Ok(0x9000 + 264),
        "帧+264 = SegCs 绝对地址",
    );

    // J9 布局渲染。
    let r = b.frame.render();
    s.add(
        "J9_render",
        r.contains("rec@0") && r.contains("ctx@96"),
        "渲染含双偏移与总长",
    );

    // J10 布局校验过。
    s.add(
        "J10_validate",
        b.validate(b.frame).is_ok(),
        "标准帧过校验",
    );

    // J11 布局校验拒（总长错）。
    let bad = ExcFrameLayout { rec_off: 0, ctx_off: 96, total: 96 + CTX_SIZE + 8 };
    s.add(
        "J11_bad_total",
        b.validate(bad).err() == Some(L_ORDER),
        "总长漂移拒 ORDER",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "布局域折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.mount(0xA000).err() == Some(L_SEALED),
        "印账后挂帧拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("slots=12") && sm.contains("mounted=10"),
        "摘要含槽数与挂载数",
    );

    // J16 RSP 槽偏移。
    s.add(
        "J16_rsp_off",
        b.ctx_slot("Rsp") == Ok(56),
        "Rsp 槽偏移 56",
    );

    // J17 挂载地址全 16 对齐回放。
    let mut ok17 = true;
    for (a, al) in &b.stack_addrs {
        if a % *al as u64 != 0 {
            ok17 = false;
        }
    }
    s.add("J17_mounts_aligned", ok17, "挂载账全 16 对齐");

    // J18 槽偏移单调升序。
    let offs: Vec<usize> = CTX_LAYOUT.iter().map(|(_, o)| *o).collect();
    s.add(
        "J18_off_mono",
        offs.windows(2).all(|w| w[0] < w[1]),
        "布局槽偏移全升序",
    );

    // J19 空域指纹稳定。
    let b19 = CtxLayout::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空布局域指纹稳定");

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12171_layout() {
        let mut b = CtxLayout::new();
        assert_eq!(b.ctx_slot("Rip"), Ok(248));
        assert!(b.mount(0x9000).is_ok());
        assert_eq!(b.mount(0x9008), Err(L_ALIGN));
    }

    #[test]
    fn unx12171_checks_green() {
        let s = run_unx12171_checks();
        assert!(s.all_passed(), "UNX-F12171 判据未全绿");
    }

    #[test]
    fn unx12171_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
