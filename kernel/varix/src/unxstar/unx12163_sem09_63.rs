//! UNX-F12163 — 陷阱帧与用户 CONTEXT 转换语义（0x1a680 段）
//!
//! 锚点：UNX-F12163-J1 陷阱帧↔CONTEXT 双向转换 10/10（内核陷阱帧
//! 五元组 ↔ 用户 CONTEXT 关键槽逐一对映，往返 roundtrip 无损），
//! 非对齐/缺段上下文转换必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：TrapFrame（内核陷阱帧）+ TrapConv（双向转换器）+ 转换
//! 留痕账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a680;
pub const TC_ORDER: u32 = SEG_BASE + 0x01; // 转换源非法（对齐/段）
pub const TC_GHOST: u32 = SEG_BASE + 0x02; // 野槽转换
pub const TC_SEALED: u32 = SEG_BASE + 0x03; // 印后转换
pub const TC_EMPTY: u32 = SEG_BASE + 0x04; // 空帧转换
pub const TC_TWICE: u32 = SEG_BASE + 0x05; // 同帧二次转换

/// 内核陷阱帧（x64 五元组精简形制）。
#[derive(Clone, Copy, PartialEq)]
pub struct TrapFrame {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub cs: u64,
    pub eflags: u64,
}

impl TrapFrame {
    /// 帧自洽：RIP 非零、RSP 8 对齐、CS 为 0x08（内核）或 0x33（用户）。
    pub fn ok(&self) -> bool {
        self.rip != 0
            && self.rsp % 8 == 0
            && (self.cs == 0x08 || self.cs == 0x33)
            && self.rsp != 0
    }

    /// 内核帧判定。
    pub fn is_kernel(&self) -> bool {
        self.cs == 0x08
    }

    pub fn render(&self) -> String {
        format!(
            "rip={:#x} rsp={:#x} cs={:#x}",
            self.rip, self.rsp, self.cs
        )
    }
}

/// 用户 CONTEXT 精简形制（六槽）。
#[derive(Clone, Copy, PartialEq)]
pub struct UserCtx {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub cs: u64,
    pub ss: u64,
    pub eflags: u64,
}

impl UserCtx {
    pub fn ok(&self) -> bool {
        self.rip != 0 && self.rsp % 8 == 0 && self.cs == 0x33 && self.ss == 0x2b
    }

    pub fn render(&self) -> String {
        format!("rip={:#x} cs={:#x} ss={:#x}", self.rip, self.cs, self.ss)
    }
}

/// 双向转换器。
pub struct TrapConv {
    pub out: Vec<(u64, u64)>, // (rip_from, rip_to)
    pub to_user: usize,
    pub to_trap: usize,
    sealed: bool,
}

impl TrapConv {
    pub fn new() -> TrapConv {
        TrapConv { out: Vec::new(), to_user: 0, to_trap: 0, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 陷阱帧 → 用户 CONTEXT（内核帧拒转：内核态不经用户转换）。
    pub fn frame_to_ctx(&mut self, f: TrapFrame) -> Result<UserCtx, u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        if !f.ok() {
            return Err(TC_ORDER);
        }
        if f.is_kernel() {
            return Err(TC_GHOST);
        }
        let ctx = UserCtx {
            rip: f.rip,
            rsp: f.rsp,
            rbp: f.rbp,
            cs: 0x33,
            ss: 0x2b,
            eflags: f.eflags,
        };
        self.out.push((f.rip, ctx.rip));
        self.to_user += 1;
        Ok(ctx)
    }

    /// 用户 CONTEXT → 陷阱帧。
    pub fn ctx_to_frame(&mut self, c: UserCtx) -> Result<TrapFrame, u32> {
        if self.sealed {
            return Err(TC_SEALED);
        }
        if !c.ok() {
            return Err(TC_ORDER);
        }
        let f = TrapFrame {
            rip: c.rip,
            rsp: c.rsp,
            rbp: c.rbp,
            cs: c.cs,
            eflags: c.eflags,
        };
        self.out.push((c.rip, f.rip));
        self.to_trap += 1;
        Ok(f)
    }

    /// 往返无损判定。
    pub fn roundtrip(&self, f: TrapFrame) -> Result<bool, u32> {
        let mut me = TrapConv::new();
        let c = me.frame_to_ctx(f)?;
        let f2 = me.ctx_to_frame(c)?;
        Ok(f2 == f)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a680;
        for (a, b) in &self.out {
            h = h.wrapping_mul(211).wrapping_add(*a as u32);
            h = h.wrapping_mul(211).wrapping_add(*b as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("out={} u={} t={}", self.out.len(), self.to_user, self.to_trap)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TrapConv::new();
    // 1) 内核帧拒转用户 CONTEXT。
    got[0] = b
        .frame_to_ctx(TrapFrame { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x08, eflags: 0x246 })
        .err()
        == Some(TC_GHOST);
    // 2) 非对齐 RSP 拒转。
    got[1] = b
        .frame_to_ctx(TrapFrame { rip: 0x1000, rsp: 0x2004, rbp: 0, cs: 0x33, eflags: 0x246 })
        .err()
        == Some(TC_ORDER);
    // 3) 零 RIP 拒转。
    got[2] = b
        .frame_to_ctx(TrapFrame { rip: 0, rsp: 0x2000, rbp: 0, cs: 0x33, eflags: 0x246 })
        .err()
        == Some(TC_ORDER);
    // 4) 野 CS 拒转。
    got[3] = b
        .frame_to_ctx(TrapFrame { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x99, eflags: 0x246 })
        .err()
        == Some(TC_ORDER);
    // 5) 印后转换必拒。
    b.seal();
    got[4] = b
        .frame_to_ctx(TrapFrame { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x33, eflags: 0x246 })
        .err()
        == Some(TC_SEALED);
    got
}

/// 判据入口：UNX-F12163
pub fn run_unx12163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12163");
    let mut b = TrapConv::new();

    // J1 用户帧 → CONTEXT 十点转换。
    let mut j1 = true;
    for i in 0..10u64 {
        let f = TrapFrame {
            rip: 0x1000 + i * 0x10,
            rsp: 0x2000 + i * 0x20,
            rbp: 0x3000 + i,
            cs: 0x33,
            eflags: 0x246,
        };
        match b.frame_to_ctx(f) {
            Ok(c) => {
                if c.rip != f.rip || c.cs != 0x33 || c.ss != 0x2b {
                    j1 = false;
                }
            }
            Err(_) => j1 = false,
        }
    }
    s.add("J1_to_ctx_10x", j1 && b.to_user == 10, "十点帧→CONTEXT 全对");

    // J2 CONTEXT → 帧回转。
    let ctx = UserCtx { rip: 0x4000, rsp: 0x5000, rbp: 0x6000, cs: 0x33, ss: 0x2b, eflags: 0x246 };
    let back = b.ctx_to_frame(ctx);
    s.add(
        "J2_to_frame",
        back == Ok(TrapFrame { rip: 0x4000, rsp: 0x5000, rbp: 0x6000, cs: 0x33, eflags: 0x246 }),
        "CONTEXT→帧字段全对",
    );

    // J3 往返无损。
    let f3 = TrapFrame { rip: 0x7000, rsp: 0x8000, rbp: 0x9000, cs: 0x33, eflags: 0x246 };
    s.add(
        "J3_roundtrip",
        b.roundtrip(f3) == Ok(true),
        "帧→CONTEXT→帧无损",
    );

    // J4 内核帧拒转。
    let kf = TrapFrame { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x08, eflags: 0x246 };
    s.add(
        "J4_kernel_reject",
        b.frame_to_ctx(kf).err() == Some(TC_GHOST),
        "内核帧拒转用户 CONTEXT",
    );

    // J5 帧自洽核。
    s.add(
        "J5_frame_ok",
        f3.ok() && !kf.ok() == false && kf.ok(),
        "内核帧过自洽核（CS=0x08 合法）",
    );

    // J6 帧渲染。
    let r = f3.render();
    s.add(
        "J6_render",
        r.contains("rip=0x7000") && r.contains("cs=0x33"),
        "帧渲染含 rip/rsp/cs",
    );

    // J7 CONTEXT 自洽核。
    let c7 = UserCtx { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x33, ss: 0x2b, eflags: 0 };
    s.add("J7_ctx_ok", c7.ok(), "用户 CONTEXT 过自洽核");

    // J8 CONTEXT 渲染。
    let r8 = c7.render();
    s.add(
        "J8_ctx_render",
        r8.contains("cs=0x33") && r8.contains("ss=0x2b"),
        "CONTEXT 渲染含段",
    );

    // J9 非对齐转换拒。
    let mis = TrapFrame { rip: 0x1000, rsp: 0x2008 + 4, rbp: 0, cs: 0x33, eflags: 0 };
    s.add(
        "J9_misalign",
        b.frame_to_ctx(mis).err() == Some(TC_ORDER),
        "RSP 非对齐拒 ORDER",
    );

    // J10 零 RIP 拒转。
    let z = TrapFrame { rip: 0, rsp: 0x2000, rbp: 0, cs: 0x33, eflags: 0 };
    s.add(
        "J10_zero_rip",
        b.frame_to_ctx(z).err() == Some(TC_ORDER),
        "零 RIP 拒 ORDER",
    );

    // J11 转换计数对平。
    s.add(
        "J11_counts",
        b.to_user == 10 && b.to_trap == 1,
        "双向计数 10/1 对平",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "转换器折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.frame_to_ctx(f3).err() == Some(TC_SEALED)
            && b.ctx_to_frame(c7).err() == Some(TC_SEALED),
        "印账后双向转换皆拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("out=11") && sm.contains("u=10"),
        "摘要含转换计数",
    );

    // J16 CONTEXT 反向自洽核（错段）。
    let bad = UserCtx { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x08, ss: 0x2b, eflags: 0 };
    s.add(
        "J16_bad_cs",
        !bad.ok(),
        "内核 CS 用户 CONTEXT 判非法",
    );

    // J17 ss 错段拒。
    let bad2 = UserCtx { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x33, ss: 0x99, eflags: 0 };
    s.add(
        "J17_bad_ss",
        !bad2.ok(),
        "野 SS 判非法",
    );

    // J18 转换留痕 rip 对平（首条）。
    s.add(
        "J18_trace_head",
        b.out[0].0 == 0x1000 && b.out[0].1 == 0x1000,
        "首条留痕 rip 对平",
    );

    // J19 空转换器指纹稳定。
    let b19 = TrapConv::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空转换器指纹稳定");

    // J20 十点往返全部无损。
    let mut ok20 = true;
    for i in 0..10u64 {
        let f = TrapFrame {
            rip: 0x1000 + i * 0x100,
            rsp: 0x2000 + i * 0x200,
            rbp: i,
            cs: 0x33,
            eflags: 0x246,
        };
        if b.roundtrip(f) != Ok(true) {
            ok20 = false;
        }
    }
    s.add("J20_rt_10x", ok20, "十点往返全无损");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12163_conv() {
        let mut b = TrapConv::new();
        let f = TrapFrame { rip: 0x1000, rsp: 0x2000, rbp: 0, cs: 0x33, eflags: 0x246 };
        let c = b.frame_to_ctx(f).unwrap();
        assert_eq!(c.cs, 0x33);
        assert_eq!(b.roundtrip(f), Ok(true));
    }

    #[test]
    fn unx12163_checks_green() {
        let s = run_unx12163_checks();
        assert!(s.all_passed(), "UNX-F12163 判据未全绿");
    }

    #[test]
    fn unx12163_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
