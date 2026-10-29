//! UNX-F12167 — XMM 寄存器保存语义与对齐（0x1a700 段）
//!
//! 锚点：UNX-F12167-J1 XMM 槽 10/10（XMM0-15 十六槽低 64 位精简
//! 保存），16 字节对齐校验、槽位写读、批量保存账全对。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：XmmSlot（128 位精简为 lo/hi 两 u64）+ XmmBank（十六槽）+
//! 保存账 + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a700;
pub const XM_SLOT: u32 = SEG_BASE + 0x01; // 槽号越界
pub const XM_ALIGN: u32 = SEG_BASE + 0x02; // 16 字节对齐违例
pub const XM_SEALED: u32 = SEG_BASE + 0x03; // 印后写
pub const XM_EMPTY: u32 = SEG_BASE + 0x04; // 空槽读
pub const XM_TWICE: u32 = SEG_BASE + 0x05; // 重复批存

pub const XMM_COUNT: usize = 16;

/// XMM 槽（lo/hi 两个 u64）。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct XmmSlot {
    pub lo: u64,
    pub hi: u64,
}

impl XmmSlot {
    pub fn new(lo: u64, hi: u64) -> XmmSlot {
        XmmSlot { lo, hi }
    }

    pub fn is_zero(&self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    pub fn render(&self) -> String {
        format!("[{:#x}:{:#x}]", self.hi, self.lo)
    }
}

/// XMM 十六槽库。
pub struct XmmBank {
    pub slots: [XmmSlot; XMM_COUNT],
    pub saves: Vec<(u64, usize)>, // (16 对齐地址, 保存槽数)
    sealed: bool,
}

impl XmmBank {
    pub fn new() -> XmmBank {
        XmmBank {
            slots: [XmmSlot::default(); XMM_COUNT],
            saves: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 写槽（槽号越界必拒）。
    pub fn set(&mut self, idx: usize, lo: u64, hi: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XM_SEALED);
        }
        if idx >= XMM_COUNT {
            return Err(XM_SLOT);
        }
        self.slots[idx] = XmmSlot::new(lo, hi);
        Ok(())
    }

    /// 读槽。
    pub fn get(&self, idx: usize) -> Result<XmmSlot, u32> {
        if idx >= XMM_COUNT {
            return Err(XM_SLOT);
        }
        Ok(self.slots[idx])
    }

    /// 批量保存（16 字节对齐地址；重复同址保存拒）。
    pub fn save(&mut self, addr: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(XM_SEALED);
        }
        if addr % 16 != 0 {
            return Err(XM_ALIGN);
        }
        if self.saves.iter().any(|&(a, _)| a == addr) {
            return Err(XM_TWICE);
        }
        self.saves.push((addr, XMM_COUNT));
        Ok(XMM_COUNT)
    }

    /// 非零槽计数。
    pub fn nonzero(&self) -> usize {
        self.slots.iter().filter(|s| !s.is_zero()).count()
    }

    /// 全槽渲染。
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for (i, s) in self.slots.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{}{}", i, s.render()));
        }
        out
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a700;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.lo as u32);
            h = h.wrapping_mul(211).wrapping_add(s.hi as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("slots={} nonzero={} saves={}", XMM_COUNT, self.nonzero(), self.saves.len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = XmmBank::new();
    // 1) 槽号 16 越界必拒。
    got[0] = b.set(16, 1, 1).err() == Some(XM_SLOT);
    // 2) 16 字节非对齐地址保存必拒。
    got[1] = b.save(0x2008).err() == Some(XM_ALIGN);
    // 3) 空槽同址重复保存必拒。
    let _ = b.save(0x2000);
    got[2] = b.save(0x2000).err() == Some(XM_TWICE);
    // 4) 槽号越界读必拒。
    got[3] = b.get(99).err() == Some(XM_SLOT);
    // 5) 印后写必拒。
    b.seal();
    got[4] = b.set(0, 1, 1).err() == Some(XM_SEALED);
    got
}

/// 判据入口：UNX-F12167
pub fn run_unx12167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12167");
    let mut b = XmmBank::new();

    // J1 十六槽逐槽写读。
    let mut ok1 = true;
    for i in 0..XMM_COUNT {
        let lo = 0x1000 + i as u64;
        let hi = 0x2000 + i as u64;
        if b.set(i, lo, hi).is_err() || b.get(i) != Ok(XmmSlot::new(lo, hi)) {
            ok1 = false;
        }
    }
    s.add("J1_set16", ok1, "十六槽写读全对");

    // J2 非零槽计数。
    s.add(
        "J2_nonzero",
        b.nonzero() == 16,
        "全槽非零计 16",
    );

    // J3 对齐地址保存 10/10。
    let mut ok3 = true;
    for i in 0..10u64 {
        if b.save(0x4000 + i * 16).is_err() {
            ok3 = false;
        }
    }
    s.add("J3_save_10x", ok3 && b.saves.len() == 10, "十次对齐保存全过");

    // J4 非对齐保存拒。
    s.add(
        "J4_misalign",
        b.save(0x5008).err() == Some(XM_ALIGN),
        "8 偏移地址拒 ALIGN",
    );

    // J5 同址重复保存拒。
    s.add(
        "J5_dup_save",
        b.save(0x4000).err() == Some(XM_TWICE),
        "同址重存拒 TWICE",
    );

    // J6 槽号越界拒。
    s.add(
        "J6_slot_oob",
        b.set(16, 0, 0).err() == Some(XM_SLOT) && b.get(16).err() == Some(XM_SLOT),
        "槽 16 双向拒 SLOT",
    );

    // J7 槽渲染。
    let r = XmmSlot::new(0xAB, 0xCD).render();
    s.add(
        "J7_render",
        r == "[0xcd:0xab]",
        "槽渲染 lo/hi 对平",
    );

    // J8 零槽判定。
    let z = XmmSlot::default();
    s.add("J8_zero", z.is_zero(), "默认槽全零");

    // J9 dump 非空含槽 0。
    let d = b.dump();
    s.add(
        "J9_dump",
        d.starts_with("0[") && d.contains(",15["),
        "dump 含首尾槽",
    );

    // J10 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J10_fingerprint", f1 == f2, "XMM 库折叠指纹一致");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 印账受控。
    b.seal();
    s.add(
        "J12_sealed",
        b.set(0, 9, 9).err() == Some(XM_SEALED) && b.save(0x6000).err() == Some(XM_SEALED),
        "印账后写/存皆拒",
    );

    // J13 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J13_summary",
        sm.contains("nonzero=16") && sm.contains("saves=10"),
        "摘要含非零/存数",
    );

    // J14 lo/hi 独立写。
    let mut b14 = XmmBank::new();
    let _ = b14.set(3, 0xA, 0);
    let _ = b14.set(3, 0xA, 0xB);
    let g14 = b14.get(3);
    s.add(
        "J14_lohi",
        g14 == Ok(XmmSlot::new(0xA, 0xB)),
        "lo/hi 双字独立",
    );

    // J15 空库非零计数 0。
    let b15 = XmmBank::new();
    s.add(
        "J15_fresh_zero",
        b15.nonzero() == 0,
        "空库非零计 0",
    );

    // J16 保存账地址全对齐回放。
    let mut ok16 = true;
    for (a, _) in &b.saves {
        if a % 16 != 0 {
            ok16 = false;
        }
    }
    s.add("J16_saves_aligned", ok16, "保存账地址全 16 对齐");

    // J17 空库指纹稳定。
    let b17 = XmmBank::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空 XMM 库指纹稳定");

    // J18 高位写不改低位。
    let mut b18 = XmmBank::new();
    let _ = b18.set(2, 0x11, 0);
    let f18 = b18.fold();
    let _ = b18.set(2, 0x11, 0x22);
    s.add(
        "J18_hi_moves",
        b18.fold() != f18 && b18.get(2).map(|x| x.lo) == Ok(0x11),
        "写 hi 动指纹、lo 保持",
    );

    // J19 大写读 16 槽两轮。
    let mut b19 = XmmBank::new();
    let mut ok19 = true;
    for round in 0..2u64 {
        for i in 0..XMM_COUNT {
            if b19.set(i, 0x900 + round, 0).is_err() {
                ok19 = false;
            }
        }
    }
    s.add(
        "J19_two_rounds",
        ok19 && b19.nonzero() == 16 && b19.get(0) == Ok(XmmSlot::new(0x901, 0)),
        "两轮覆写后值=第二轮",
    );

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
    fn unx12167_xmm() {
        let mut b = XmmBank::new();
        let _ = b.set(0, 1, 2);
        assert_eq!(b.get(0), Ok(XmmSlot::new(1, 2)));
        assert_eq!(b.set(16, 0, 0), Err(XM_SLOT));
    }

    #[test]
    fn unx12167_checks_green() {
        let s = run_unx12167_checks();
        assert!(s.all_passed(), "UNX-F12167 判据未全绿");
    }

    #[test]
    fn unx12167_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
