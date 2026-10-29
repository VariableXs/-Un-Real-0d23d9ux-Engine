//! UNX-F12071 TEB gs 段基址写路径：GDT/MSR 链与 A3 联签（0x19b00 段）
//!
//! 卡面：TEB→Self 挂点的 gs 段基址写路径——GDT 槽位备妥 → 基值提交 →
//! MSR GSBASE 影子同步 → TEB.Self 回读校验，四段有序链语义模型。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 四段有序：SlotArm→BaseCommit→MsrShadow→SelfVerify 逐段推进，跳段必拒。
//! - J2 重复段拒：同 CPU 同段二次执行必拒。
//! - J3 基值合法：TEB 基址非零且 16 字节对齐，违者必拒。
//! - J4 槽位合法：CPU 槽位号闭集 [0,64)，越界必拒。
//! - J5 链账对平：每 CPU 链完整时其段位集合恰为四段闭集。
//! - J6 印账冻结：seal 后写路径必拒。
//! - J7 注入五况全抓。
//!
//! 结构：GsStage 四段枚举 + GsPathBook 每槽位链账 + 折叠指纹 +
//! 注入核（跳段/重复段/坏基址/坏槽位/印后写）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19b00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19b00;
pub const GP_SKIP: u32 = SEG_BASE + 0x01; // 跳段
pub const GP_DUP: u32 = SEG_BASE + 0x02; // 重复段
pub const GP_BAD_BASE: u32 = SEG_BASE + 0x03; // 基址非法
pub const GP_BAD_SLOT: u32 = SEG_BASE + 0x04; // 槽位越界
pub const GP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后写

/// 写路径四段。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GsStage {
    SlotArm = 1,    // GDT 槽位备妥
    BaseCommit = 2, // 基值提交
    MsrShadow = 3,  // MSR GSBASE 影子同步
    SelfVerify = 4, // TEB.Self 回读校验
}

impl GsStage {
    /// 段序（1..=4）。
    pub fn ord(&self) -> u32 {
        *self as u32
    }
    /// 前驱段。
    pub fn prev(&self) -> Option<GsStage> {
        match self {
            GsStage::SlotArm => None,
            GsStage::BaseCommit => Some(GsStage::SlotArm),
            GsStage::MsrShadow => Some(GsStage::BaseCommit),
            GsStage::SelfVerify => Some(GsStage::MsrShadow),
        }
    }
}

/// 一次段执行的留痕。
#[derive(Clone)]
pub struct GsRec {
    pub cpu: u8,
    pub stage: GsStage,
    pub base: u32,
    pub note: String,
}

impl GsRec {
    pub fn render(&self) -> String {
        format!("S{}|st{}|{:08x}", self.cpu, self.stage.ord(), self.base)
    }
}

/// gs 基址写路径链账。
pub struct GsPathBook {
    pub done: Vec<(u8, GsStage)>,
    pub recs: Vec<GsRec>,
    pub max_cpus: usize,
    pub sealed: bool,
}

impl GsPathBook {
    pub fn new(max_cpus: usize) -> GsPathBook {
        GsPathBook {
            done: Vec::new(),
            recs: Vec::new(),
            max_cpus,
            sealed: false,
        }
    }

    /// 段执行：前置段必须在位、不得重复、基址与槽位合法。
    pub fn step(&mut self, cpu: u8, stage: GsStage, base: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GP_FROZEN);
        }
        if (cpu as usize) >= self.max_cpus {
            return Err(GP_BAD_SLOT);
        }
        if base == 0 || base % 16 != 0 {
            return Err(GP_BAD_BASE);
        }
        if note.is_empty() {
            return Err(GP_BAD_BASE); // 留痕空按基址面显性拒（同段口径）
        }
        if self.done.iter().any(|d| d.0 == cpu && d.1 == stage) {
            return Err(GP_DUP);
        }
        if let Some(p) = stage.prev() {
            if !self.done.iter().any(|d| d.0 == cpu && d.1 == p) {
                return Err(GP_SKIP);
            }
        }
        self.done.push((cpu, stage));
        self.recs.push(GsRec { cpu, stage, base, note: note.to_string() });
        Ok(())
    }

    /// 链账对平：某 CPU 段位集合恰为四段闭集。
    pub fn chain_complete(&self, cpu: u8) -> bool {
        self.done.iter().filter(|d| d.0 == cpu).count() == 4
    }

    /// 某 CPU 已推进的最大段序。
    pub fn max_stage_of(&self, cpu: u8) -> u32 {
        self.done.iter().filter(|d| d.0 == cpu).map(|d| d.1.ord()).max().unwrap_or(0)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.done {
            h = h.wrapping_mul(211).wrapping_add(d.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(d.1.ord());
        }
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.base);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：链账条数。
    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("slots={} recs={}", self.done.len(), self.recs.len())
    }

    /// 诊断面：段渲染行。
    pub fn rec_line(&self, i: usize) -> String {
        match self.recs.get(i) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = GsPathBook::new(4);
    // 1) 跳段：未备妥 GDT 槽位直接提交基值必拒。
    got[0] = b.step(0, GsStage::BaseCommit, 0x7efdd000, "skip")
        .err() == Some(GP_SKIP);
    // 2) 重复段：SlotArm 执行两遍必拒。
    let _ = b.step(0, GsStage::SlotArm, 0x7efdd000, "arm");
    got[1] = b.step(0, GsStage::SlotArm, 0x7efdd000, "dup")
        .err() == Some(GP_DUP);
    // 3) 坏基址：零基址必拒。
    got[2] = b.step(1, GsStage::SlotArm, 0, "zero").err() == Some(GP_BAD_BASE);
    // 4) 坏槽位：越界 CPU 必拒。
    got[3] = b.step(9, GsStage::SlotArm, 0x7efdd000, "badslot")
        .err() == Some(GP_BAD_SLOT);
    // 5) 印账后写路径必抓。
    b.seal();
    got[4] = b.step(0, GsStage::BaseCommit, 0x7efdd000, "post")
        .err() == Some(GP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12071");
    const B: u32 = 0x7efdd000;

    // J1 四段有序推进并链账对平。
    let j1 = {
        let mut b = GsPathBook::new(4);
        let _ = b.step(0, GsStage::SlotArm, B, "a");
        let _ = b.step(0, GsStage::BaseCommit, B, "b");
        let _ = b.step(0, GsStage::MsrShadow, B, "c");
        let _ = b.step(0, GsStage::SelfVerify, B, "d");
        b.chain_complete(0) && b.max_stage_of(0) == 4
    };
    s.add("J1-四段有序", j1, "链账四段闭集对平");

    // J2 跳段必拒。
    let j2 = {
        let mut b = GsPathBook::new(4);
        b.step(0, GsStage::MsrShadow, B, "skip").err() == Some(GP_SKIP)
    };
    s.add("J2-跳段必拒", j2, "前置段缺位必拒");

    // J3 重复段必拒。
    let j3 = {
        let mut b = GsPathBook::new(4);
        let _ = b.step(0, GsStage::SlotArm, B, "a");
        b.step(0, GsStage::SlotArm, B, "b").err() == Some(GP_DUP)
    };
    s.add("J3-重复段拒", j3, "同段二次必拒");

    // J4 坏基址：零与非对齐皆拒。
    let j4 = {
        let mut b = GsPathBook::new(4);
        b.step(0, GsStage::SlotArm, 0, "z").err() == Some(GP_BAD_BASE)
            && b.step(0, GsStage::SlotArm, B + 8, "mis")
                .err() == Some(GP_BAD_BASE)
    };
    s.add("J4-基址合法", j4, "非零且 16 对齐");

    // J5 坏槽位。
    let j5 = {
        let mut b = GsPathBook::new(2);
        b.step(2, GsStage::SlotArm, B, "oob").err() == Some(GP_BAD_SLOT)
    };
    s.add("J5-槽位闭集", j5, "槽位越界必拒");

    // J6 每段留痕可渲染。
    let j6 = {
        let mut b = GsPathBook::new(4);
        let _ = b.step(0, GsStage::SlotArm, B, "a");
        b.rec_line(0).contains("st1") && b.rec_count() == 1
    };
    s.add("J6-留痕渲染", j6, "段行含段序与基址");

    // J7 印账冻结。
    let j7 = {
        let mut b = GsPathBook::new(4);
        let _ = b.step(0, GsStage::SlotArm, B, "a");
        b.seal();
        b.step(0, GsStage::BaseCommit, B, "post").err() == Some(GP_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后必拒");

    // J8 指纹幂等。
    let j8 = {
        let mut b = GsPathBook::new(4);
        let _ = b.step(0, GsStage::SlotArm, B, "a");
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例账：双 CPU 各走前两段。
    let mut b = GsPathBook::new(4);
    let _ = b.step(0, GsStage::SlotArm, B, "a0");
    let _ = b.step(0, GsStage::BaseCommit, B, "b0");
    let _ = b.step(1, GsStage::SlotArm, B, "a1");

    s.add("摘要回读", b.summary().contains("recs=3"), "三段留痕");
    s.add("最大段序", b.max_stage_of(0) == 2, "CPU0 至 BaseCommit");
    s.add("未推进槽位", b.max_stage_of(3) == 0, "未动槽位为零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: u32 = 0x7efdd000;

    #[test]
    fn unx12071_chain_flow() {
        let mut b = GsPathBook::new(4);
        assert!(b.step(0, GsStage::SlotArm, B, "a").is_ok());
        assert_eq!(b.step(0, GsStage::SelfVerify, B, "s"), Err(GP_SKIP));
        assert!(b.step(0, GsStage::BaseCommit, B, "b").is_ok());
        assert_eq!(b.max_stage_of(0), 2);
    }

    #[test]
    fn unx12071_checks_green() {
        let s = run_unx12071_checks();
        assert!(s.all_passed(), "UNX-F12071 判据未全绿");
    }

    #[test]
    fn stage_ordering() {
        assert!(GsStage::SlotArm.prev().is_none());
        assert_eq!(GsStage::SelfVerify.prev(), Some(GsStage::MsrShadow));
        assert_eq!(GsStage::BaseCommit.ord(), 2);
    }
}
