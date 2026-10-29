//! UNX-F11273 musl setjmp/longjmp 消费（0x1f490 段）
//! 纯功能模型：jmp_buf 逐帧保存（栈/帧基/返回址/信号掩码），longjmp 只许
//! 向**更外层已保活**的帧回跳（目标深度不得大于当前深度）；回跳时寄存器与
//! 信号掩码整体恢复，掩码中的 SIGKILL/SIGSTOP 位不接受（内核恒不可屏蔽）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f490 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f490;
/// 目标深度越界或该深度无保活帧。
pub const JL_DEPTH: u32 = SEG_BASE + 0x01;
/// 前向跳转（目标比当前更深）。
pub const JL_ORDER: u32 = SEG_BASE + 0x02;
/// 寄存器形态违规（零址或栈未 16 对齐）。
pub const JL_REG: u32 = SEG_BASE + 0x03;
/// 掩码含不可屏蔽信号。
pub const JL_MASK: u32 = SEG_BASE + 0x04;
/// 同深度重复保存。
pub const JL_DUP: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const JL_FROZEN: u32 = SEG_BASE + 0x06;
/// 帧表已满。
pub const JL_CAP: u32 = SEG_BASE + 0x07;

/// 深度上界（调用链层数）。
pub const DEPTH_MAX: u32 = 15;
/// 帧表定容。
pub const FRAME_CAP: usize = 12;
/// 栈对齐。
pub const SP_ALIGN: u64 = 16;
/// 不可屏蔽信号位（SIGKILL 9 / SIGSTOP 19）。
pub const UNBLOCKABLE: u64 = (1u64 << 9) | (1u64 << 19);

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_73;

/// 一个 jmp_buf 帧：深度 + 三寄存器 + 掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JmpFrame {
    pub depth: u32,
    pub rsp: u64,
    pub rbp: u64,
    pub rip: u64,
    pub mask: u64,
}

impl JmpFrame {
    pub fn render(&self) -> String {
        format!("D{} rsp={:x} rbp={:x} rip={:x} m={:x}", self.depth, self.rsp, self.rbp, self.rip, self.mask)
    }
}

/// 回跳结果：目标帧 + 被恢复的掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JumpOut {
    pub to: u32,
    pub rip: u64,
    pub mask: u64,
    pub depth_delta: u32,
}

/// 帧册：保存 jmp_buf → 回跳恢复 → 审计。
#[derive(Clone, Debug)]
pub struct JumpBook {
    pub frames: Vec<JmpFrame>,
    pub jumps: u32,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl JumpBook {
    pub fn new() -> JumpBook {
        JumpBook { frames: Vec::new(), jumps: 0, accepts: 0, rejects: 0, frozen: false, root: None }
    }

    /// 保存一帧。判定序：封册 → 深度域 → 重深度 → 寄存器形态
    /// → 掩码 → 定容。
    pub fn save(&mut self, depth: u32, rsp: u64, rbp: u64, rip: u64, mask: u64)
        -> Result<usize, u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_FROZEN);
        }
        if depth == 0 || depth > DEPTH_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_DEPTH);
        }
        if self.frames.iter().any(|f| f.depth == depth) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_DUP);
        }
        if rsp == 0 || rbp == 0 || rip == 0 || rsp & (SP_ALIGN - 1) != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_REG);
        }
        if mask & UNBLOCKABLE != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_MASK);
        }
        if self.frames.len() >= FRAME_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_CAP);
        }
        self.frames.push(JmpFrame { depth, rsp, rbp, rip, mask });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.frames.len().wrapping_sub(1))
    }

    /// 取深度对应帧。
    pub fn frame_at(&self, depth: u32) -> Option<JmpFrame> {
        let mut k = 0usize;
        while k < self.frames.len() {
            if let Some(f) = self.frames.get(k) {
                if f.depth == depth {
                    return Some(*f);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// longjmp：只许回跳到「已保活且不深于当前」的帧。
    pub fn longjmp(&mut self, to: u32, cur: u32) -> Result<JumpOut, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_FROZEN);
        }
        if to == 0 || to > DEPTH_MAX || cur == 0 || cur > DEPTH_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_DEPTH);
        }
        if to > cur {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(JL_ORDER);
        }
        let f = match self.frame_at(to) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(JL_DEPTH);
            }
        };
        self.jumps = self.jumps.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(JumpOut {
            to,
            rip: f.rip,
            mask: f.mask,
            depth_delta: cur.wrapping_sub(to),
        })
    }

    /// 掩码恢复自检：回跳后屏蔽字应等于目标帧保存值。
    pub fn mask_restored(&self, out: &JumpOut) -> bool {
        match self.frame_at(out.to) {
            Some(f) => f.mask == out.mask && f.rip == out.rip,
            None => false,
        }
    }

    /// FNV-1a 链指纹：深度/寄存器/掩码/回跳次数序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.jumps as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.frames.len() {
            if let Some(f) = self.frames.get(k) {
                h = (h ^ f.depth as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ f.rsp).wrapping_mul(FNV_PRIME);
                h = (h ^ f.rbp).wrapping_mul(FNV_PRIME);
                h = (h ^ f.rip).wrapping_mul(FNV_PRIME);
                h = (h ^ f.mask).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 四层调用链样本（深度 1..=4）。
pub fn chain() -> JumpBook {
    let mut b = JumpBook::new();
    let mut d = 1u32;
    while d <= 4 {
        let _ = b.save(
            d,
            0x7fff_0000u64.wrapping_sub(d as u64 * 0x100),
            0x7fff_0100u64.wrapping_sub(d as u64 * 0x100),
            0x401000u64.wrapping_add(d as u64 * 0x10),
            1u64 << d,
        );
        d = d.wrapping_add(1);
    }
    b
}

/// Four probes: (a) depth zero, (b) duplicate depth, (c) unaligned rsp,
/// (d) save after freeze.
pub fn inject_four() -> [bool; 4] {
    let mut a = JumpBook::new();
    let bad_a = a.save(0, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err() == Some(JL_DEPTH);

    let mut b = JumpBook::new();
    let _ = b.save(1, 0x7fff_0000, 0x7fff_0100, 0x401000, 0);
    let bad_b = b.save(1, 0x7fff_0200, 0x7fff_0300, 0x401020, 0).err() == Some(JL_DUP);

    let mut c = JumpBook::new();
    let bad_c = c.save(1, 0x7fff_0001, 0x7fff_0100, 0x401000, 0).err() == Some(JL_REG);

    let mut d = JumpBook::new();
    let _ = d.save(1, 0x7fff_0000, 0x7fff_0100, 0x401000, 0);
    let _ = d.freeze();
    let bad_d = d.save(2, 0x7fff_0200, 0x7fff_0300, 0x401020, 0).err() == Some(JL_FROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// Thirteen contract checks over frames, jumps and the unblockable mask.
pub fn run_unx11273_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11273");

    s.add("X1-SEGMENT-DOMAIN", SEG_BASE == 0x1f490 && JL_DEPTH == SEG_BASE + 0x01
        && JL_ORDER == SEG_BASE + 0x02 && JL_REG == SEG_BASE + 0x03
        && JL_MASK == SEG_BASE + 0x04 && JL_DUP == SEG_BASE + 0x05
        && JL_FROZEN == SEG_BASE + 0x06 && JL_CAP == SEG_BASE + 0x07,
        "错误码落在 0x1f490 段连续偏移上");

    let mut b = JumpBook::new();
    for d in 1..=4u32 {
        let _ = b.save(d, 0x7fff_0000u64.wrapping_sub(d as u64 * 0x100),
            0x7fff_0100u64.wrapping_sub(d as u64 * 0x100),
            0x401000u64.wrapping_add(d as u64 * 0x10), 1u64 << d);
    }
    s.add("X2-SAVE-FOUR", b.frames.len() == 4 && b.accepts == 4 && b.rejects == 0,
        "四层嵌套 save 全部入账");

    let f1 = match b.frame_at(1) {
        Some(f) => f,
        None => JmpFrame { depth: 0, rsp: 0, rbp: 0, rip: 0, mask: 0 },
    };
    s.add("X3-REG-EXACT", f1.depth == 1 && f1.rsp == 0x7ffe_ff00 && f1.rbp == 0x7fff_0000
        && f1.rip == 0x401010 && f1.mask == 2, "第一层寄存器快照逐字段正确");

    let jump = b.longjmp(1, 4);
    s.add("X4-CROSS-JUMP", jump == Ok(JumpOut { to: 1, rip: 0x401010, mask: 2, depth_delta: 3 })
        && b.jumps == 1, "由第 4 层跳回第 1 层并记录负向层差");

    let mut f = JumpBook::new();
    let _ = f.save(1, 0x7fff_0000, 0x7fff_0100, 0x401000, 0);
    let bad = f.longjmp(3, 1);
    s.add("X5-FORWARD-GATE", bad == Err(JL_ORDER) && f.rejects == 1,
        "向前跳转（目标层更深）被 JL_ORDER 拒绝");

    let unk = f.longjmp(9, 12);
    s.add("X6-DEPTH-GATE", unk == Err(JL_DEPTH) && f.rejects == 2,
        "不存在的层号被 JL_DEPTH 拒绝并且计数累加");

    let mut m = JumpBook::new();
    let em = m.save(1, 0x7fff_0000, 0x7fff_0100, 0x401000, UNBLOCKABLE | 0x10).err();
    s.add("X7-MASK-GATE", em == Some(JL_MASK) && m.rejects == 1,
        "掩码含 SIGKILL/SIGSTOP 位被 JL_MASK 拒绝");

    let mut d = chain();
    let ed = d.save(2, 0x7fff_0200, 0x7fff_0300, 0x401020, 0).err();
    s.add("X8-DUP-GATE", ed == Some(JL_DUP) && d.frames.len() == 4 && d.rejects == 1,
        "同深度重复保存被 JL_DUP 拒绝");

    let mut r = JumpBook::new();
    let e1 = r.save(1, 0x7fff_0002, 0x7fff_0100, 0x401000, 0).err();
    let e2 = r.save(2, 0x7fff_0200, 0x7fff_0300, 0, 0).err();
    s.add("X9-REG-GATE", e1 == Some(JL_REG) && e2 == Some(JL_REG) && r.rejects == 2,
        "栈未对齐与返回址为零均判 JL_REG");

    let mut dp = JumpBook::new();
    let e3 = dp.save(DEPTH_MAX + 1, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err();
    s.add("X10-DEPTH-MAX", e3 == Some(JL_DEPTH) && dp.rejects == 1,
        "超过 DEPTH_MAX 的层号被 JL_DEPTH 拒绝");

    let mut c = JumpBook::new();
    let mut i = 1u32;
    while i <= FRAME_CAP as u32 {
        let _ = c.save(i, 0x7fff_0000 - i as u64 * 0x100, 0x7fff_0100, 0x401000, 0);
        i = i.wrapping_add(1);
    }
    let ec = c.save(13, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err();
    s.add("X11-CAP-GATE", c.frames.len() == FRAME_CAP && ec == Some(JL_CAP)
        && c.rejects == 1, "帧表满后继续保存被 JL_CAP 拒绝");

    let mut b3 = chain();
    let h0 = b3.fold();
    let h1 = chain().fold();
    if let Some(v) = b3.frames.get_mut(0) {
        v.rip = 0x4010ff;
    }
    s.add("X12-FOLD", h0 != 0 && h0 == h1 && b3.fold() != h0,
        "同构调用链指纹相同，改返回址后变化");

    let mut z = chain();
    let r1 = z.freeze();
    let r2 = z.freeze();
    let ez = z.save(5, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err();
    s.add_dyn("X13-FROZEN-GATE", r1.is_some() && r1 == r2 && ez == Some(JL_FROZEN)
        && z.frames.len() == 4, &format!("根={:?} 封后写={:?}", r1, ez));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11273_flow() {
        let mut b = chain();
        assert_eq!(b.frames.len(), 4);
        assert_eq!(b.accepts, 4);
        assert_eq!(b.rejects, 0);
        assert_eq!(b.frame_at(3).map(|x| x.rip), Some(0x401030));
        assert_eq!(b.frame_at(2).map(|x| x.mask), Some(4));
        let out = match b.longjmp(1, 4) {
            Ok(v) => v,
            Err(_) => JumpOut { to: 0, rip: 0, mask: 0, depth_delta: 0 },
        };
        assert_eq!(out.to, 1);
        assert_eq!(out.rip, 0x401010);
        assert_eq!(out.mask, 2);
        assert_eq!(out.depth_delta, 3);
        assert!(b.mask_restored(&out));
        assert_eq!(b.jumps, 1);
        assert_eq!(b.accepts, 5);
    }

    #[test]
    fn unx11273_gates() {
        let mut b = chain();
        assert_eq!(b.longjmp(3, 1).err(), Some(JL_ORDER));
        assert_eq!(b.longjmp(0, 1).err(), Some(JL_DEPTH));
        assert_eq!(b.longjmp(9, 12).err(), Some(JL_DEPTH));
        assert_eq!(b.save(2, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err(), Some(JL_DUP));
        assert_eq!(b.save(5, 0x7fff_0000, 0x7fff_0100, 0x401000, UNBLOCKABLE).err(),
            Some(JL_MASK));
        assert_eq!(b.save(5, 0x7fff_0001, 0x7fff_0100, 0x401000, 0).err(), Some(JL_REG));
        assert_eq!(b.accepts, 4);
        assert_eq!(b.rejects, 6);
        let _ = b.freeze();
        assert_eq!(b.save(5, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err(), Some(JL_FROZEN));
        assert_eq!(b.longjmp(1, 4).err(), Some(JL_FROZEN));
        assert_eq!(b.rejects, 8);

        let mut c = JumpBook::new();
        let mut i = 1u32;
        while i <= FRAME_CAP as u32 {
            assert!(c.save(i, 0x7fff_0000 - i as u64 * 0x100, 0x7fff_0100, 0x401000, 0).is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(c.save(13, 0x7fff_0000, 0x7fff_0100, 0x401000, 0).err(), Some(JL_CAP));
        assert_eq!(c.accepts, FRAME_CAP as u32);
        assert_eq!(c.rejects, 1);
    }

    #[test]
    fn unx11273_checks_green() {
        let s = run_unx11273_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
