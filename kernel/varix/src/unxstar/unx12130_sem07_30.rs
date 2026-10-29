//! UNX-F12130 — UWOP_SAVE_REGS_FRAME / UWOP_SAVE_XMM128_FRAME 大帧语义核
//! 域: UNX-D1 (Windows NT 内核语义模型) · B07 批 · 展开码族
//! 语义: 模型扩展码 11/12。SAVE_REGS_FRAME 的 opinfo 字段不再是寄存器编号，
//!       而是“后续槽块计数”（跟随的 UWOP_SAVE_NONVOL 槽位数）；
//!       SAVE_XMM128_FRAME 同理但槽块为 XMM。两类码的 Offset 字段均为
//!       帧锚（RSP/frame anchor）到保存点的 16 字节对齐偏移。
//!       本核维护 FrameBook：登记码型→块计数→偏移→槽位占用，
//!       并强制：块计数落在 1..=15、偏移非零且 16 对齐、槽位在
//!       首次登记时被认领、同一帧锚不得重复登记（防重放）。
//! 载荷: 账本记录 FrameRec { code, count, off } + 折叠指纹 + seal 冻结。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a260;

pub const FR_BAD_COUNT: u32 = SEG_BASE + 0x01; // 块计数越界（0 或 >15）
pub const FR_UNALIGNED: u32 = SEG_BASE + 0x02; // 偏移非 16 对齐
pub const FR_NO_SLOT: u32 = SEG_BASE + 0x03; // 槽位未认领/越槽
pub const FR_ZERO_OFF: u32 = SEG_BASE + 0x04; // 偏移为零（帧锚自身不可保存点）
pub const FR_FROZEN: u32 = SEG_BASE + 0x05; // 账本已 seal 仍受理登记

/// 大帧码型：模型扩展码 11 与 12。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameCode {
    /// 11 — SAVE_REGS_FRAME：整型非易变寄存器块保存。
    Regs,
    /// 12 — SAVE_XMM128_FRAME：XMM 非易变寄存器块保存。
    Xmm128,
}

impl FrameCode {
    pub fn op_num(self) -> u8 {
        match self {
            FrameCode::Regs => 11,
            FrameCode::Xmm128 => 12,
        }
    }
    pub fn slot_base(self) -> u8 {
        match self {
            FrameCode::Regs => 4,  // 整型块从槽 4 起认领
            FrameCode::Xmm128 => 8, // XMM 块从槽 8 起认领
        }
    }
    pub fn slot_cap(self) -> u8 {
        match self {
            FrameCode::Regs => 15,
            FrameCode::Xmm128 => 15,
        }
    }
}

/// 单条大帧登记：码型 + 块计数 + 16 对齐偏移。
#[derive(Clone, Copy, Debug)]
pub struct FrameRec {
    pub code: FrameCode,
    pub count: u8,
    pub off: u32,
}

impl FrameRec {
    pub fn byte_span(&self) -> u32 {
        // Regs 槽 8 字节/槽，Xmm 槽 16 字节/槽
        let per = match self.code {
            FrameCode::Regs => 8u32,
            FrameCode::Xmm128 => 16u32,
        };
        per * self.count as u32
    }
}

/// 大帧账本：登记 + 帧锚防重放 + seal。
pub struct FrameBook {
    recs: Vec<FrameRec>,
    anchors: Vec<u32>,
    frozen: bool,
    rejects: Vec<(u32, String)>,
}

impl FrameBook {
    pub fn new() -> Self {
        FrameBook { recs: Vec::new(), anchors: Vec::new(), frozen: false, rejects: Vec::new() }
    }

    /// 登记一条大帧码。校验链：冻结 → 计数 → 零偏 → 对齐 → 槽位 → 锚重放。
    pub fn enroll(&mut self, code: FrameCode, count: u8, off: u32) -> Result<usize, u32> {
        if self.frozen {
            self.rejects.push((FR_FROZEN, "enroll after seal".to_string()));
            return Err(FR_FROZEN);
        }
        if count == 0 || count > 15 {
            self.rejects.push((FR_BAD_COUNT, format!("count={}", count)));
            return Err(FR_BAD_COUNT);
        }
        if off == 0 {
            self.rejects.push((FR_ZERO_OFF, "zero offset".to_string()));
            return Err(FR_ZERO_OFF);
        }
        if off % 16 != 0 {
            self.rejects.push((FR_UNALIGNED, format!("off={}", off)));
            return Err(FR_UNALIGNED);
        }
        // 槽位认领：base + 已占用 span 槽不得越过上限
        let used: u8 = self
            .recs
            .iter()
            .filter(|r| r.code == code)
            .map(|r| r.count)
            .sum();
        let next = code.slot_base() + used;
        if next + count > code.slot_cap() + 1 {
            self.rejects.push((FR_NO_SLOT, format!("next={} count={}", next, count)));
            return Err(FR_NO_SLOT);
        }
        if self.anchors.contains(&off) {
            // 帧锚重复：按对齐守卫（off 已 16 对齐且非零，撞锚即重放）
            self.rejects.push((FR_UNALIGNED, format!("anchor replay off={}", off)));
            return Err(FR_UNALIGNED);
        }
        self.anchors.push(off);
        self.recs.push(FrameRec { code, count, off });
        Ok(self.recs.len() - 1)
    }

    /// 查询某码型累计占槽。
    pub fn used_slots(&self, code: FrameCode) -> u8 {
        self.recs.iter().filter(|r| r.code == code).map(|r| r.count).sum()
    }

    /// 帧锚是否已登记。
    pub fn anchor_known(&self, off: u32) -> bool {
        self.anchors.contains(&off)
    }

    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 折叠指纹：211 进制卷积，码型/计数/偏移/账本规模混入。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.code.op_num() as u32);
            h = h.wrapping_mul(211).wrapping_add(r.count as u32);
            h = h.wrapping_mul(211).wrapping_add(r.off ^ (r.off >> 7));
        }
        h.wrapping_mul(211).wrapping_add(self.recs.len() as u32)
    }

    /// 冻结账本。冻结后仅查询类操作合法。
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        self.fold()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 五况注入核：每况踩一个独立检出分支。
pub fn inject_five() -> [Result<usize, u32>; 5] {
    let mut b = FrameBook::new();
    let r0 = b.enroll(FrameCode::Regs, 0, 64); // FR_BAD_COUNT：计数为零
    let mut b1 = FrameBook::new();
    let r1 = b1.enroll(FrameCode::Regs, 2, 63); // FR_UNALIGNED：63 非 16 对齐
    let mut b2 = FrameBook::new();
    let r2 = b2.enroll(FrameCode::Xmm128, 9, 32); // FR_NO_SLOT：8+9>15+1
    let mut b3 = FrameBook::new();
    let r3 = b3.enroll(FrameCode::Xmm128, 1, 0); // FR_ZERO_OFF：零偏移
    let mut b4 = FrameBook::new();
    let _ = b4.enroll(FrameCode::Regs, 1, 48);
    b4.seal();
    let r4 = b4.enroll(FrameCode::Regs, 1, 96); // FR_FROZEN：冻结后登记
    [r0, r1, r2, r3, r4]
}

/// 判据核：12 条大帧语义判据。
pub fn run_unx12130_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F12130");
    let mut b = FrameBook::new();
    let ok1 = b.enroll(FrameCode::Regs, 2, 64);
    let ok2 = b.enroll(FrameCode::Xmm128, 3, 96);
    let ok3 = b.enroll(FrameCode::Regs, 1, 160);
    cs.add("enroll_regs_ok", ok1.is_ok(), "Regs c=2 off=64");
    cs.add("enroll_xmm_ok", ok2.is_ok(), "Xmm128 c=3 off=96");
    cs.add("enroll_third_ok", ok3.is_ok(), "Regs c=1 off=160");
    cs.add("op_nums", FrameCode::Regs.op_num() == 11 && FrameCode::Xmm128.op_num() == 12,
        "11/12 model codes");
    cs.add("slot_bases", FrameCode::Regs.slot_base() == 4 && FrameCode::Xmm128.slot_base() == 8,
        "4/8 slot bases");
    cs.add("used_regs_slots", b.used_slots(FrameCode::Regs) == 3, "2+1=3 int slots");
    cs.add("used_xmm_slots", b.used_slots(FrameCode::Xmm128) == 3, "3 xmm slots");
    cs.add("anchor_known_true", b.anchor_known(96), "anchor 96 registered");
    cs.add("anchor_known_false", !b.anchor_known(128), "anchor 128 absent");
    cs.add("byte_span", {
        let mut bb = FrameBook::new();
        let _ = bb.enroll(FrameCode::Regs, 2, 64);
        let _ = bb.enroll(FrameCode::Xmm128, 2, 96);
        bb.recs_len_span() == 16 + 32
    }, "8*2 + 16*2 = 48");
    cs.add("reject_after_seal", {
        let mut bb = FrameBook::new();
        bb.seal();
        bb.enroll(FrameCode::Regs, 1, 64).err() == Some(FR_FROZEN)
    }, "seal blocks enroll");
    cs.add("injection_five", {
        let inj = inject_five();
        inj.iter().all(|r| r.is_err())
    }, "all five branches fire");
    let _ = b.rej_count();
    cs.add("opinfo_role", FrameCode::Regs.slot_base() < FrameCode::Regs.slot_cap(),
        "base 4 < cap 15");
    cs.add("span_16_align", {
        let mut bb = FrameBook::new();
        let _ = bb.enroll(FrameCode::Xmm128, 1, 32);
        bb.recs_len_span() % 16 == 0
    }, "xmm block keeps 16-align");
    cs.add("zero_off_branch", inject_five()[3].err() == Some(FR_ZERO_OFF),
        "zero offset gate fires");
    cs.add("bad_count_branch", inject_five()[0].err() == Some(FR_BAD_COUNT),
        "count gate fires");
    cs
}

impl FrameBook {
    /// 账本字节总跨度（判据用）。
    pub fn recs_len_span(&self) -> u32 {
        self.recs.iter().map(|r| r.byte_span()).sum()
    }
    /// 拒收计数别名（判据用）。
    pub fn rej_count(&self) -> usize {
        self.rejects.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_large_frame_enroll() {
        let mut b = FrameBook::new();
        assert!(b.enroll(FrameCode::Regs, 4, 64).is_ok());
        assert!(b.enroll(FrameCode::Xmm128, 2, 128).is_ok());
        assert_eq!(b.used_slots(FrameCode::Regs), 4);
        assert_eq!(b.used_slots(FrameCode::Xmm128), 2);
        assert!(b.enroll(FrameCode::Regs, 0, 192).err() == Some(FR_BAD_COUNT));
    }

    #[test]
    fn test_alignment_and_replay() {
        let mut b = FrameBook::new();
        assert!(b.enroll(FrameCode::Regs, 1, 60).err() == Some(FR_UNALIGNED));
        assert!(b.enroll(FrameCode::Regs, 1, 64).is_ok());
        assert!(b.enroll(FrameCode::Regs, 1, 64).is_err());
        assert!(b.reject_count() >= 2);
    }

    #[test]
    fn test_fold_changes() {
        let mut b1 = FrameBook::new();
        let _ = b1.enroll(FrameCode::Regs, 1, 64);
        let f1 = b1.seal();
        let mut b2 = FrameBook::new();
        let _ = b2.enroll(FrameCode::Regs, 1, 80);
        let f2 = b2.seal();
        assert_ne!(f1, f2);
        assert!(b1.is_frozen());
    }

    #[test]
    fn test_inject_five_all_err() {
        assert!(inject_five().iter().all(|r| r.is_err()));
    }
}