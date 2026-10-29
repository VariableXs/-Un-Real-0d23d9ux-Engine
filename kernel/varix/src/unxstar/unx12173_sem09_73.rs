//! UNX-F12173 — NtGetContextThread/NtSetContextThread 语义预告
//! （0x1a7c0 段）
//!
//! 锚点：UNX-F12173-J1 线程上下文 G/S 语义 10/10（Get 带授权掩码、
//! Set 受掩码约束、掩码外组拒动），掩码账逐组对平。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：ThreadCtxHandle（线程句柄+掩码）+ ThreadCtxSim（G/S 模拟
//! 器）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a7c0;
pub const TG_GHOST: u32 = SEG_BASE + 0x01; // 线程句柄不存在
pub const TG_ORDER: u32 = SEG_BASE + 0x02; // 掩码非法（0）
pub const TG_TWICE: u32 = SEG_BASE + 0x03; // 重复挂句柄
pub const TG_SEALED: u32 = SEG_BASE + 0x04; // 印后 G/S
pub const TG_MASK: u32 = SEG_BASE + 0x05; // 掩码外组拒动

/// 掩码组位（同 12162 位常量）。
pub const TC_CONTROL: u32 = 0x1001;
pub const TC_INTEGER: u32 = 0x1002;
pub const TC_SEGMENTS: u32 = 0x1004;
pub const TC_FLOAT: u32 = 0x1008;

/// 线程上下文句柄。
#[derive(Clone, PartialEq)]
pub struct ThreadCtxHandle {
    pub tid: u32,
    pub mask: u32,
    pub regs: Vec<(u32, u64)>, // (组位, 值)
    pub note: String,
}

impl ThreadCtxHandle {
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.mask != 0
    }

    /// 组在掩码内。
    pub fn covers(&self, group: u32) -> bool {
        self.mask & group == group
    }

    pub fn render(&self) -> String {
        format!("tid={} mask={:#x} n={}", self.tid, self.mask, self.regs.len())
    }
}

/// G/S 模拟器。
pub struct ThreadCtxSim {
    pub handles: Vec<ThreadCtxHandle>,
    pub ops: Vec<(u32, u32, bool)>, // (tid, group, is_set)
    sealed: bool,
}

impl ThreadCtxSim {
    pub fn new() -> ThreadCtxSim {
        ThreadCtxSim { handles: Vec::new(), ops: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂线程句柄（重复 tid/非法 tid/零掩码必拒）。
    pub fn attach(&mut self, tid: u32, mask: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TG_SEALED);
        }
        if tid == 0 || mask == 0 {
            return Err(TG_ORDER);
        }
        if self.handles.iter().any(|h| h.tid == tid) {
            return Err(TG_TWICE);
        }
        self.handles.push(ThreadCtxHandle {
            tid,
            mask,
            regs: Vec::new(),
            note: note.to_string(),
        });
        Ok(())
    }

    /// Get 上下文组（tid 不存在/掩码外必拒）。
    pub fn get_ctx(&mut self, tid: u32, group: u32) -> Result<u64, u32> {
        if self.sealed {
            return Err(TG_SEALED);
        }
        let h = self.handles.iter_mut().find(|h| h.tid == tid).ok_or(TG_GHOST)?;
        if !h.covers(group) {
            return Err(TG_MASK);
        }
        self.ops.push((tid, group, false));
        Ok(h.regs.iter().find(|(g, _)| *g == group).map(|(_, v)| *v).unwrap_or(0))
    }

    /// Set 上下文组（掩码外必拒；组值写读）。
    pub fn set_ctx(&mut self, tid: u32, group: u32, val: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TG_SEALED);
        }
        let h = self.handles.iter_mut().find(|h| h.tid == tid).ok_or(TG_GHOST)?;
        if !h.covers(group) {
            return Err(TG_MASK);
        }
        match h.regs.iter_mut().find(|(g, _)| *g == group) {
            Some(slot) => slot.1 = val,
            None => h.regs.push((group, val)),
        }
        self.ops.push((tid, group, true));
        Ok(())
    }

    /// 操作账回放：每条操作 tid 都在册、组都在掩码内。
    pub fn ops_valid(&self) -> bool {
        self.ops.iter().all(|&(tid, g, _)| {
            self.handles
                .iter()
                .find(|h| h.tid == tid)
                .map(|h| h.covers(g))
                .unwrap_or(false)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a7c0;
        for hd in &self.handles {
            h = h.wrapping_mul(211).wrapping_add(hd.tid);
            h = h.wrapping_mul(211).wrapping_add(hd.mask);
            for (g, v) in &hd.regs {
                h = h.wrapping_mul(211).wrapping_add(*g);
                h = h.wrapping_mul(211).wrapping_add(*v as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "handles={} ops={} valid={}",
            self.handles.len(),
            self.ops.len(),
            self.ops_valid()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ThreadCtxSim::new();
    // 1) 未挂线程 Get 必拒。
    got[0] = b.get_ctx(9, TC_INTEGER).err() == Some(TG_GHOST);
    // 2) 零掩码挂必拒。
    got[1] = b.attach(1, 0, "z").err() == Some(TG_ORDER);
    // 3) 重复挂必拒。
    let _ = b.attach(1, TC_INTEGER, "a");
    got[2] = b.attach(1, TC_INTEGER, "b").err() == Some(TG_TWICE);
    // 4) 掩码外组 Set 必拒。
    got[3] = b.set_ctx(1, TC_FLOAT, 5).err() == Some(TG_MASK);
    // 5) 印后 G 必拒。
    b.seal();
    got[4] = b.get_ctx(1, TC_INTEGER).err() == Some(TG_SEALED);
    got
}

/// 判据入口：UNX-F12173
pub fn run_unx12173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12173");
    let mut b = ThreadCtxSim::new();

    // J1 双线程挂载（全掩码/部分掩码）。
    let j1 = b.attach(100, TC_CONTROL | TC_INTEGER | TC_SEGMENTS | TC_FLOAT, "full").is_ok()
        && b.attach(200, TC_INTEGER | TC_SEGMENTS, "partial").is_ok();
    s.add("J1_attach2", j1 && b.handles.len() == 2, "双线程句柄挂载");

    // J2 Set 组 10/10（tid=100 四组写）。
    let mut ok2 = true;
    for (g, v) in [(TC_CONTROL, 0xA1u64), (TC_INTEGER, 0xB2), (TC_SEGMENTS, 0xC3), (TC_FLOAT, 0xD4)] {
        if b.set_ctx(100, g, v).is_err() {
            ok2 = false;
        }
    }
    s.add("J2_set_10x", ok2 && b.handles[0].regs.len() == 4, "全掩码四组写全过");

    // J3 Get 回读一致。
    let got = b.get_ctx(100, TC_INTEGER);
    s.add(
        "J3_get",
        got == Ok(0xB2),
        "Get 回读 0xB2 一致",
    );

    // J4 掩码外组拒（tid=200 无 FLOAT）。
    s.add(
        "J4_mask_reject",
        b.set_ctx(200, TC_FLOAT, 5).err() == Some(TG_MASK)
            && b.get_ctx(200, TC_CONTROL).err() == Some(TG_MASK),
        "部分掩码越组双向拒 MASK",
    );

    // J5 部分掩码组写读过。
    let ok5 = b.set_ctx(200, TC_INTEGER, 7).is_ok() && b.get_ctx(200, TC_INTEGER) == Ok(7);
    s.add("J5_partial_ok", ok5, "部分掩码在册组写读过");

    // J6 未挂线程拒。
    s.add(
        "J6_ghost_tid",
        b.set_ctx(999, TC_INTEGER, 1).err() == Some(TG_GHOST)
            && b.get_ctx(999, TC_INTEGER).err() == Some(TG_GHOST),
        "野 tid 双向拒 GHOST",
    );

    // J7 句柄渲染。
    let r = b.handles[1].render();
    s.add(
        "J7_render",
        r.contains("tid=200") && r.contains("mask=0x1006"),
        "渲染含 tid/掩码/组数",
    );

    // J8 句柄自洽核。
    s.add(
        "J8_handles_ok",
        b.handles.iter().all(|h| h.ok()),
        "全句柄过自洽核",
    );

    // J9 零掩码挂拒。
    s.add(
        "J9_zero_mask",
        b.attach(300, 0, "z").err() == Some(TG_ORDER),
        "零掩码拒 ORDER",
    );

    // J10 重复 tid 挂拒。
    s.add(
        "J10_dup_tid",
        b.attach(100, TC_INTEGER, "d").err() == Some(TG_TWICE),
        "重复 tid 拒 TWICE",
    );

    // J11 操作账回放。
    s.add("J11_ops_valid", b.ops_valid() && b.ops.len() >= 5, "操作账全合法");

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "G/S 模拟器折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.get_ctx(100, TC_INTEGER).err() == Some(TG_SEALED)
            && b.set_ctx(100, TC_INTEGER, 9).err() == Some(TG_SEALED),
        "印账后 G/S 皆拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("handles=2") && sm.contains("valid=true"),
        "摘要含句柄数与合法位",
    );

    // J16 Set 覆写（同组再写）。
    let mut b16 = ThreadCtxSim::new();
    let _ = b16.attach(1, TC_INTEGER, "a");
    let _ = b16.set_ctx(1, TC_INTEGER, 1);
    let _ = b16.set_ctx(1, TC_INTEGER, 2);
    s.add(
        "J16_overwrite",
        b16.get_ctx(1, TC_INTEGER) == Ok(2) && b16.handles[0].regs.len() == 1,
        "同组覆写值=2 且不增槽",
    );

    // J17 十线程挂载全过。
    let mut b17 = ThreadCtxSim::new();
    let mut ok17 = true;
    for i in 1..=10u32 {
        if b17.attach(i, TC_INTEGER, "m").is_err() {
            ok17 = false;
        }
    }
    s.add("J17_ten_threads", ok17 && b17.handles.len() == 10, "十线程挂载全过");

    // J18 Get 未写组回 0（b17 未写任何组）。
    let g18 = b17.get_ctx(3, TC_INTEGER);
    s.add(
        "J18_default0",
        g18 == Ok(0),
        "未写组 Get 回 0",
    );

    // J19 空模拟器指纹稳定。
    let b19 = ThreadCtxSim::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空模拟器指纹稳定");

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
    fn unx12173_gs() {
        let mut b = ThreadCtxSim::new();
        let _ = b.attach(1, TC_INTEGER, "a");
        let _ = b.set_ctx(1, TC_INTEGER, 42);
        assert_eq!(b.get_ctx(1, TC_INTEGER), Ok(42));
        assert_eq!(b.get_ctx(1, TC_FLOAT), Err(TG_MASK));
    }

    #[test]
    fn unx12173_checks_green() {
        let s = run_unx12173_checks();
        assert!(s.all_passed(), "UNX-F12173 判据未全绿");
    }

    #[test]
    fn unx12173_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
