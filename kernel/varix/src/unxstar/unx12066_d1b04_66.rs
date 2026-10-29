//! UNX-F12066 WOW32Reserved WOW64 转换槽（0x19A60 段）
//!
//! 判据原文：WOW32Reserved 是 TEB 中的 WOW64 转换存根指针；
//! 语义规则：64 位进程该槽非空即 wow64 存根；32 位进程恒零；
//! 槽状态三面（Absent/Wow64Present/Native）；由加载器一次性
//! 定装（二次定装拒）；快照面记录定装史；封存后拒定装。
//!
//! 结构：槽三面（WowSlot）+ 定装核 + 快照账（WowSnap 留痕）+
//! 四码守卫 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12066_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19A60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19A60;
pub const WO_RESEAT: u32 = SEG_BASE + 0x01; // 二次定装
pub const WO_BAD_REC: u32 = SEG_BASE + 0x02; // 空留痕/伪账
pub const WO_FROZEN: u32 = SEG_BASE + 0x03; // 印账后仍定装
pub const WO_ARCH: u32 = SEG_BASE + 0x04; // 架构不匹配定装
pub const WO_BAD_STUB: u32 = SEG_BASE + 0x05; // 伪存根（未对齐/零）

/// 进程架构。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
    X86,
    X64,
}

/// 槽三面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WowFace {
    Absent,
    Wow64Present,
    Native,
}

/// 一笔定装账。
#[derive(Clone)]
pub struct WowSnap {
    pub seq: u32,
    pub stub_va: u32,
    pub face: WowFace,
    pub note: String,
}

impl WowSnap {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// WOW32 槽。
pub struct WowSlot {
    pub arch: Arch,
    pub stub_va: u32,
    pub face: WowFace,
    pub snaps: Vec<WowSnap>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WowSlot {
    pub fn new(arch: Arch) -> WowSlot {
        WowSlot {
            arch,
            stub_va: 0,
            face: WowFace::Absent,
            snaps: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 定装核：一次性；X64 架构可定装 WOW64 存根（4 对齐非零）；
    /// X86 架构槽恒零（Native 面记录但不存指针）。
    pub fn seat(&mut self, stub_va: u32, note: &str) -> Result<WowFace, u32> {
        if self.sealed {
            return Err(WO_FROZEN);
        }
        if note.is_empty() {
            return Err(WO_BAD_REC);
        }
        if self.face != WowFace::Absent {
            return Err(WO_RESEAT);
        }
        if self.arch == Arch::X86 {
            // X86 无 WOW64 层：定装任何指针都判架构错。
            if stub_va != 0 {
                return Err(WO_ARCH);
            }
            self.face = WowFace::Native;
        } else {
            // X64：零指针 → Native（64 位纯进程）；非零须 4 对齐。
            if stub_va == 0 {
                self.face = WowFace::Native;
            } else if stub_va % 4 == 0 {
                self.face = WowFace::Wow64Present;
            } else {
                return Err(WO_BAD_STUB);
            }
        }
        self.stub_va = stub_va;
        let snap = WowSnap { seq: self.seq_next, stub_va, face: self.face, note: note.to_string() };
        if !snap.ok() {
            return Err(WO_BAD_REC);
        }
        self.seq_next += 1;
        self.snaps.push(snap);
        Ok(self.face)
    }

    /// 定装史笔数。
    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.snaps.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.stub_va >> 4);
        h = h.wrapping_mul(211).wrapping_add(self.face as u32);
        for r in &self.snaps {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) X86 架构定装指针 → WO_ARCH。
    let mut a = WowSlot::new(Arch::X86);
    got[0] = a.seat(0x7700_0000, "x86").err() == Some(WO_ARCH);
    // 2) 未对齐存根 → WO_BAD_STUB。
    let mut b = WowSlot::new(Arch::X64);
    got[1] = b.seat(0x7700_0002, "aln").err() == Some(WO_BAD_STUB);
    // 3) 空留痕 → WO_BAD_REC。
    let mut c = WowSlot::new(Arch::X64);
    got[2] = c.seat(0x7700_0000, "").err() == Some(WO_BAD_REC);
    // 4) 二次定装 → WO_RESEAT。
    let _ = c.seat(0x7700_0000, "ok");
    got[3] = c.seat(0x7800_0000, "again").err() == Some(WO_RESEAT);
    // 5) 印账后定装 → WO_FROZEN。
    c.seal();
    got[4] = c.seat(0x7900_0000, "post").err() == Some(WO_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12066_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12066");

    // J1 初态 Absent。
    let j1 = WowSlot::new(Arch::X64).face == WowFace::Absent;
    s.add("J1-初态面", j1, "槽初始 Absent");

    // J2 X64 定装 WOW64 存根。
    let j2 = {
        let mut w = WowSlot::new(Arch::X64);
        w.seat(0x7700_0000, "wow") == Ok(WowFace::Wow64Present)
            && w.stub_va == 0x7700_0000
    };
    s.add("J2-WOW64定装", j2, "存根落槽");

    // J3 X64 零定装 → Native。
    let j3 = {
        let mut w = WowSlot::new(Arch::X64);
        w.seat(0, "native") == Ok(WowFace::Native)
    };
    s.add("J3-纯64面", j3, "零指针 Native");

    // J4 X86 槽恒 Native（零定装合法）。
    let j4 = {
        let mut w = WowSlot::new(Arch::X86);
        w.seat(0, "x86") == Ok(WowFace::Native)
    };
    s.add("J4-X86面", j4, "X86 零定装");

    // J5 X86 定装指针拒。
    let j5 = WowSlot::new(Arch::X86).seat(0x1000, "x").err() == Some(WO_ARCH);
    s.add("J5-X86指针拒", j5, "架构不匹配");

    // J6 未对齐存根拒（1/2 偏双样）。
    let j6 = {
        let mut a = WowSlot::new(Arch::X64);
        let mut b = WowSlot::new(Arch::X64);
        a.seat(1, "a").err() == Some(WO_BAD_STUB)
            && b.seat(2, "b").err() == Some(WO_BAD_STUB)
    };
    s.add("J6-未对齐拒", j6, "1/2 偏全拒");

    // J7 二次定装拒（Wow64Present 与 Native 两面皆拒）。
    let j7 = {
        let mut a = WowSlot::new(Arch::X64);
        let _ = a.seat(0x7700_0000, "1");
        let mut b = WowSlot::new(Arch::X64);
        let _ = b.seat(0, "1");
        a.seat(0x7800_0000, "2").err() == Some(WO_RESEAT)
            && b.seat(0, "2").err() == Some(WO_RESEAT)
    };
    s.add("J7-二次拒", j7, "双面重装拒");

    // J8 定装史留痕。
    let j8 = {
        let mut w = WowSlot::new(Arch::X64);
        let _ = w.seat(0x7700_0000, "s1");
        w.snap_count() == 1 && w.snaps[0].face == WowFace::Wow64Present && w.ledger_ok()
    };
    s.add("J8-定装史", j8, "一笔留痕");

    // J9 空留痕拒。
    let j9 = WowSlot::new(Arch::X64).seat(0x1000, "").err() == Some(WO_BAD_REC);
    s.add("J9-空留痕拒", j9, "留痕必填");

    // J10 指纹区分度与幂等。
    let j10 = {
        let mut a = WowSlot::new(Arch::X64);
        let _ = a.seat(0x7700_0000, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = WowSlot::new(Arch::X64);
        let _ = c.seat(0, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J10-指纹性质", j10, "幂等且异槽异纹");

    // J11 印账冻结。
    let j11 = {
        let mut w = WowSlot::new(Arch::X64);
        w.seal();
        w.seat(0x1000, "post").err() == Some(WO_FROZEN)
    };
    s.add("J11-印账冻结", j11, "印后定装拒");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例。
    let mut w = WowSlot::new(Arch::X64);
    let _ = w.seat(0x7700_1000, "sum");
    s.add("样例定装", w.face == WowFace::Wow64Present && w.ledger_ok(), "存根面齐");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12066_seat_once() {
        let mut w = WowSlot::new(Arch::X64);
        assert_eq!(w.seat(0x7700_0000, "a"), Ok(WowFace::Wow64Present));
        assert_eq!(w.seat(0x7800_0000, "b").err(), Some(WO_RESEAT));
    }

    #[test]
    fn unx12066_checks_green() {
        let s = run_unx12066_checks();
        assert!(s.all_passed(), "UNX-F12066 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12066_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
