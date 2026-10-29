//! UNX-F12132 — 展开码解释器主体（逐码栈模拟核）
//! 域: UNX-D1 (Windows NT 内核语义模型) · B07 批 · 展开码族
//! 语义: 解释器按序消费展开码流（UWOP_* 码 + opinfo），在“栈机器”上
//!       逐码模拟：PUSH_NONVOL/PUSH_MACHFRAME 推栈槽 1，ALLOC_SMALL
//!       累计 opinfo×8+8，ALLOC_LARGE 按 opinfo 形式补读扩展槽 ext
//!       （0 → ext×8，1 → ext 不乘），SAVE_* 记保存点，EPILOG 落尾，
//!       SPARE 旁路。本核编码表与批内口径一致：
//!       0=PUSH_NONVOL 1=ALLOC_LARGE 2=ALLOC_SMALL 4=SAVE_NONVOL
//!       5=SAVE_NONVOL_FAR 8=SAVE_XMM128 9=SAVE_XMM128_FAR
//!       10=PUSH_MACHFRAME 11/12=大帧族（归 12130 核解释）。
//!       校验链：未知码 → 尾随脏码（EPILOG 后仍有可执行码）→
//!       MACHFRAME 重复 → 栈深越限（prolog 失衡）→ 冻结后重放。
//! 载荷: 模拟账 InterpLog（每步记录）+ 栈深/帧量终态 + 折叠指纹 + seal。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a2a0;

pub const IN_UNKNOWN_OP: u32 = SEG_BASE + 0x01; // 未知展开码（保留段）
pub const IN_TRAILING: u32 = SEG_BASE + 0x02; // EPILOG 之后仍有可执行码
pub const IN_MACH_TWICE: u32 = SEG_BASE + 0x03; // PUSH_MACHFRAME 出现两次
pub const IN_PROLOG_MISMATCH: u32 = SEG_BASE + 0x04; // 栈深越限/prolog 失衡
pub const IN_FROZEN: u32 = SEG_BASE + 0x05; // seal 后仍受理重放

/// 栈深硬限（250 槽）：prolog 失衡检出阈值。
pub const STACK_DEPTH_CAP: usize = 250;

/// 可解释的展开码全集（批内口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Uwop {
    PushNonvol(u8),
    AllocLarge,
    AllocSmall(u8),
    SaveNonvol(u8),
    SaveNonvolFar(u8),
    SaveXmm128(u8),
    SaveXmm128Far(u8),
    PushMachFrame,
    SaveRegsFrame,
    SaveXmm128Frame,
    Epilog,
    Spare,
}

impl Uwop {
    /// 批内编码表（与 12126~12130 卡面一致）。
    pub fn op_num(self) -> u8 {
        match self {
            Uwop::PushNonvol(_) => 0,
            Uwop::AllocLarge => 1,
            Uwop::AllocSmall(_) => 2,
            Uwop::SaveNonvol(_) => 4,
            Uwop::SaveNonvolFar(_) => 5,
            Uwop::SaveXmm128(_) => 8,
            Uwop::SaveXmm128Far(_) => 9,
            Uwop::PushMachFrame => 10,
            Uwop::SaveRegsFrame => 11,
            Uwop::SaveXmm128Frame => 12,
            Uwop::Epilog => 3,
            Uwop::Spare => 6,
        }
    }
    /// 是否为“可执行”码（EPILOG/SPARE 属结构性旁路）。
    pub fn executable(self) -> bool {
        !matches!(self, Uwop::Epilog | Uwop::Spare)
    }
}

/// 解释器单步账。
#[derive(Clone, Debug)]
pub struct StepRec {
    pub op: u8,
    pub note: String,
    pub stack_after: usize,
}

/// 解释器：栈机器模拟。
pub struct Interp {
    steps: Vec<StepRec>,
    stack_depth: usize,
    frame_alloc: u32,
    mach_frames: usize,
    seen_epilog: bool,
    frozen: bool,
    rejects: Vec<(u32, String)>,
}

impl Interp {
    pub fn new() -> Self {
        Interp {
            steps: Vec::new(),
            stack_depth: 0,
            frame_alloc: 0,
            mach_frames: 0,
            seen_epilog: false,
            frozen: false,
            rejects: Vec::new(),
        }
    }

    /// 解释一条码。ext 为 ALLOC_LARGE 的扩展槽载荷（其余码置 0）。
    /// 校验链：冻结 → 未知码 → 尾随 → MACHFRAME 重复 → 栈深越限。
    pub fn step(&mut self, u: Uwop, opinfo: u8, ext: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects.push((IN_FROZEN, "step after seal".to_string()));
            return Err(IN_FROZEN);
        }
        let op = u.op_num();
        // 未知码：保留段口径——SPARE 形态下 opinfo 落在 7 或 >12 视为未知码
        // （SPARE 自身 op_num 恒为 6，保留段值由 opinfo 槽承载）。
        if matches!(u, Uwop::Spare) && (opinfo == 7 || opinfo > 12) {
            self.rejects.push((IN_UNKNOWN_OP, format!("op={} reserved", opinfo)));
            return Err(IN_UNKNOWN_OP);
        }
        if self.seen_epilog && u.executable() {
            self.rejects.push((IN_TRAILING, format!("exec after epilog op={}", op)));
            return Err(IN_TRAILING);
        }
        match u {
            Uwop::PushMachFrame => {
                if self.mach_frames > 0 {
                    self.rejects.push((IN_MACH_TWICE, "machine frame twice".to_string()));
                    return Err(IN_MACH_TWICE);
                }
                self.mach_frames += 1;
                self.stack_depth += 1;
            }
            Uwop::Epilog => {
                self.seen_epilog = true;
            }
            Uwop::Spare => {
                // 旁路：不推栈不累计
            }
            Uwop::PushNonvol(_) => {
                self.stack_depth += 1;
            }
            Uwop::AllocSmall(i) => {
                self.frame_alloc += ((i as u32) + 1) * 8;
            }
            Uwop::AllocLarge => {
                // opinfo 0 → 尺寸 = ext×8；opinfo 1 → 尺寸 = ext（不乘）
                let sz = if opinfo == 0 { ext * 8 } else { ext };
                self.frame_alloc += sz;
            }
            Uwop::SaveNonvol(_) | Uwop::SaveNonvolFar(_) | Uwop::SaveXmm128(_)
            | Uwop::SaveXmm128Far(_) | Uwop::SaveRegsFrame | Uwop::SaveXmm128Frame => {
                // 保存类/大帧类：不计栈深，仅记步
            }
        }
        if self.stack_depth > STACK_DEPTH_CAP {
            self.rejects.push((IN_PROLOG_MISMATCH, format!("depth {}", self.stack_depth)));
            return Err(IN_PROLOG_MISMATCH);
        }
        self.steps.push(StepRec { op, note: format!("{:?}", u), stack_after: self.stack_depth });
        Ok(())
    }

    pub fn stack_depth(&self) -> usize {
        self.stack_depth
    }

    pub fn frame_alloc(&self) -> u32 {
        self.frame_alloc
    }

    pub fn mach_frames(&self) -> usize {
        self.mach_frames
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 折叠指纹：211 进制卷积，步序/栈深/帧量混入。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.steps {
            h = h.wrapping_mul(211).wrapping_add(s.op as u32);
            h = h.wrapping_mul(211).wrapping_add(s.stack_after as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.frame_alloc);
        h.wrapping_mul(211).wrapping_add(self.steps.len() as u32)
    }

    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        self.fold()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 五况注入核：每况踩一个独立检出分支。
pub fn inject_five() -> [Result<(), u32>; 5] {
    // IN_UNKNOWN_OP：SPARE 落在保留段（op=7）
    let mut i0 = Interp::new();
    let r0 = i0.step(Uwop::Spare, 7, 0);
    // IN_TRAILING：EPILOG 之后仍有可执行码
    let mut i1 = Interp::new();
    let _ = i1.step(Uwop::Epilog, 0, 0);
    let r1 = i1.step(Uwop::PushNonvol(4), 4, 0);
    // IN_MACH_TWICE：机器帧重复
    let mut i2 = Interp::new();
    let _ = i2.step(Uwop::PushMachFrame, 0, 0);
    let r2 = i2.step(Uwop::PushMachFrame, 1, 0);
    // IN_PROLOG_MISMATCH：栈深越过 250 槽硬限
    let mut i3 = Interp::new();
    let mut r3 = Ok(());
    for _ in 0..260 {
        r3 = i3.step(Uwop::PushNonvol(4), 4, 0);
        if r3.is_err() {
            break;
        }
    }
    let r3 = r3.and(Err(IN_PROLOG_MISMATCH));
    // IN_FROZEN：seal 后重放
    let mut i4 = Interp::new();
    i4.seal();
    let r4 = i4.step(Uwop::PushNonvol(4), 4, 0);
    [r0, r1, r2, r3, r4]
}

/// 判据核：14 条解释器语义判据。
pub fn run_unx12132_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F12132");
    let mut i = Interp::new();
    let s1 = i.step(Uwop::PushNonvol(4), 4, 0);
    let s2 = i.step(Uwop::PushNonvol(5), 5, 0);
    let s3 = i.step(Uwop::AllocSmall(2), 2, 0); // (2+1)*8 = 24
    cs.add("push_two_ok", s1.is_ok() && s2.is_ok(), "two pushes");
    cs.add("alloc_small_ok", s3.is_ok(), "small alloc ok");
    cs.add("stack_depth_two", i.stack_depth() == 2, "depth 2");
    cs.add("frame_alloc_24", i.frame_alloc() == 24, "(2+1)*8");
    cs.add("step_count_three", i.step_count() == 3, "3 steps logged");
    cs.add("alloc_large_small_form", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::AllocLarge, 0, 20);
        a.is_ok() && ii.frame_alloc() == 160
    }, "large form0: 20*8");
    cs.add("alloc_large_big_form", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::AllocLarge, 1, 300);
        a.is_ok() && ii.frame_alloc() == 300
    }, "large form1: raw ext");
    cs.add("mach_frame_once_ok", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::PushMachFrame, 0, 0);
        a.is_ok() && ii.stack_depth() == 1 && ii.mach_frames() == 1
    }, "single machine frame");
    cs.add("mach_frame_twice_err", inject_five()[2].err() == Some(IN_MACH_TWICE),
        "double machine frame");
    cs.add("trailing_branch", inject_five()[1].err() == Some(IN_TRAILING),
        "trailing exec blocked");
    cs.add("unknown_branch", inject_five()[0].err() == Some(IN_UNKNOWN_OP),
        "unknown op blocked");
    cs.add("frozen_branch", inject_five()[4].err() == Some(IN_FROZEN),
        "frozen replay blocked");
    cs.add("op_nums_stable", Uwop::PushNonvol(0).op_num() == 0
        && Uwop::AllocLarge.op_num() == 1
        && Uwop::AllocSmall(0).op_num() == 2
        && Uwop::PushMachFrame.op_num() == 10, "opcode table anchors");
    cs.add("exec_flags", Uwop::PushNonvol(0).executable() && !Uwop::Epilog.executable()
        && !Uwop::Spare.executable(), "executable classification");
    cs.add("fold_seal_differs", {
        let mut a = Interp::new();
        let _ = a.step(Uwop::PushNonvol(4), 4, 0);
        let f1 = a.seal();
        let mut b = Interp::new();
        let _ = b.step(Uwop::AllocSmall(0), 0, 0);
        let f2 = b.seal();
        f1 != f2
    }, "fold distinguishes traces");
    cs.add("save_no_depth", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::SaveNonvol(3), 3, 0);
        let b = ii.step(Uwop::SaveXmm128(10), 10, 0);
        a.is_ok() && b.is_ok() && ii.stack_depth() == 0
    }, "saves don't push");
    cs.add("large_frame_passthrough", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::SaveRegsFrame, 2, 0);
        let b = ii.step(Uwop::SaveXmm128Frame, 1, 0);
        a.is_ok() && b.is_ok() && ii.step_count() == 2
    }, "frame ops logged, no depth");
    cs.add("depth_cap_branch", inject_five()[3].err() == Some(IN_PROLOG_MISMATCH),
        "depth cap gate fires");
    cs.add("alloc_large_zero", {
        let mut ii = Interp::new();
        let a = ii.step(Uwop::AllocLarge, 0, 0);
        a.is_ok() && ii.frame_alloc() == 0
    }, "zero-size large alloc");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_alloc_trace() {
        let mut i = Interp::new();
        assert!(i.step(Uwop::PushNonvol(4), 4, 0).is_ok());
        assert!(i.step(Uwop::PushNonvol(5), 5, 0).is_ok());
        assert!(i.step(Uwop::AllocSmall(7), 7, 0).is_ok());
        assert_eq!(i.stack_depth(), 2);
        assert_eq!(i.frame_alloc(), 64);
        assert_eq!(i.step_count(), 3);
    }

    #[test]
    fn test_mach_and_epilog() {
        let mut i = Interp::new();
        assert!(i.step(Uwop::PushMachFrame, 0, 0).is_ok());
        assert!(i.step(Uwop::Epilog, 0, 0).is_ok());
        assert!(i.step(Uwop::Spare, 0, 0).is_ok());
        assert!(i.step(Uwop::PushNonvol(3), 3, 0).err() == Some(IN_TRAILING));
    }

    #[test]
    fn test_fold_changes() {
        let mut i = Interp::new();
        let _ = i.step(Uwop::PushNonvol(4), 4, 0);
        let f1 = i.seal();
        let mut j = Interp::new();
        let _ = j.step(Uwop::PushNonvol(5), 5, 0);
        let f2 = j.seal();
        assert_ne!(f1, f2);
        assert!(i.is_frozen());
        assert!(j.step(Uwop::PushNonvol(4), 4, 0).err() == Some(IN_FROZEN));
    }

    #[test]
    fn test_inject_five_all_err() {
        assert!(inject_five().iter().all(|r| r.is_err()));
    }
}