//! UNX-F11192 上游消费核销账（C2 挂号 / C3 释放闸终核销）（0x1f5d0 段）
//!
//! 纯功能模型：上游两个消费面——C2 挂号面与 C3 释放闸面——各以键记账。
//! 每键须先挂号、后释放，且各恰一次；终核销要求两面逐键对平、双向闭合
//! （挂号数=释放数、无悬空键）。重复挂号、重复释放、释放未挂号键、带悬空
//! 键核销一律拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f5d0;
/// 上游面号越界
pub const UL_EUP: u32 = SEG_BASE + 0x01;
/// 重复挂号
pub const UL_EDUP: u32 = SEG_BASE + 0x02;
/// 释放未挂号键
pub const UL_EMISS: u32 = SEG_BASE + 0x03;
/// 重复释放
pub const UL_EDOUBLE: u32 = SEG_BASE + 0x04;
/// 悬空键未清
pub const UL_EOPEN: u32 = SEG_BASE + 0x05;
/// 已冻结
pub const UL_EFROZEN: u32 = SEG_BASE + 0x06;
/// 键号越界
pub const UL_EKEY: u32 = SEG_BASE + 0x07;

/// 上游面：C2 挂号 0 / C3 释放闸 1
pub const SURFACES: usize = 2;
/// 每面键数
pub const KEYS_PER_SURFACE: usize = 12;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一笔上游消费账。
#[derive(Clone, Copy, Debug)]
pub struct UpEntry {
    pub up: u8,
    pub key: u32,
    pub registered: bool,
    pub released: bool,
}

impl UpEntry {
    fn new(up: u8, key: u32) -> Self {
        UpEntry { up, key, registered: false, released: false }
    }

    /// 是否悬空（挂而未释）。
    pub fn dangling(&self) -> bool {
        self.registered && !self.released
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.up as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.key as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.registered { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.released { 2 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 面上对账读数。
#[derive(Clone, Copy, Debug)]
pub struct SurfaceReading {
    pub registered: u64,
    pub released: u64,
}

impl SurfaceReading {
    /// 面内双向闭合：挂=释且非零。
    pub fn closed(&self) -> bool {
        self.registered == self.released && self.registered > 0
    }
}

/// 上游消费核销账。
#[derive(Clone, Debug)]
pub struct UpstreamLedger {
    entries: Vec<UpEntry>,
    frozen: Option<u64>,
    pub registers: u64,
    pub releases: u64,
    pub rejects: u64,
}

impl UpstreamLedger {
    /// 空账。
    pub fn new() -> Self {
        UpstreamLedger { entries: Vec::new(), frozen: None, registers: 0, releases: 0, rejects: 0 }
    }

    /// 一次装满两面全键（未挂未释）。
    pub fn scaffold() -> Self {
        let mut l = UpstreamLedger::new();
        let mut up = 0u8;
        while (up as usize) < SURFACES {
            let mut k = 0u32;
            while (k as usize) < KEYS_PER_SURFACE {
                l.entries.push(UpEntry::new(up, k));
                k = k.wrapping_add(1);
            }
            up = up.wrapping_add(1);
        }
        l
    }

    /// 定位一笔账。
    fn locate(&self, up: u8, key: u32) -> Result<usize, u32> {
        if (up as usize) >= SURFACES {
            return Err(UL_EUP);
        }
        if (key as usize) >= KEYS_PER_SURFACE {
            return Err(UL_EKEY);
        }
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.up == up && e.key == key {
                return Ok(i);
            }
            i += 1;
        }
        Err(UL_EMISS)
    }

}

impl UpstreamLedger {
    /// 已冻结即拒改。
    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(UL_EFROZEN);
        }
        Ok(())
    }

    /// 挂号：同键重复挂号拒止。
    pub fn register(&mut self, up: u8, key: u32) -> Result<u64, u32> {
        self.guard()?;
        let ix = self.locate(up, key)?;
        match self.entries.get_mut(ix) {
            Some(e) => {
                if e.registered {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(UL_EDUP);
                }
                e.registered = true;
                self.registers = self.registers.wrapping_add(1);
                Ok(self.registers)
            }
            None => Err(UL_EMISS),
        }
    }

    /// 释放：未挂号键拒止，重复释放拒止。
    pub fn release(&mut self, up: u8, key: u32) -> Result<u64, u32> {
        self.guard()?;
        let ix = self.locate(up, key)?;
        match self.entries.get_mut(ix) {
            Some(e) => {
                if !e.registered {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(UL_EMISS);
                }
                if e.released {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(UL_EDOUBLE);
                }
                e.released = true;
                self.releases = self.releases.wrapping_add(1);
                Ok(self.releases)
            }
            None => Err(UL_EMISS),
        }
    }

    /// 面内读数。
    pub fn reading(&self, up: u8) -> Result<SurfaceReading, u32> {
        if (up as usize) >= SURFACES {
            return Err(UL_EUP);
        }
        let mut reg = 0u64;
        let mut rel = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.up == up {
                if e.registered {
                    reg = reg.wrapping_add(1);
                }
                if e.released {
                    rel = rel.wrapping_add(1);
                }
            }
            i += 1;
        }
        Ok(SurfaceReading { registered: reg, released: rel })
    }

    /// 悬空键数。
    pub fn dangling(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].dangling() {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 双向闭合：两面各自挂=释，且全局挂=释=键数。
    pub fn closed_bidirectionally(&self) -> bool {
        let mut up = 0u8;
        while (up as usize) < SURFACES {
            match self.reading(up) {
                Ok(r) if r.closed() => {}
                _ => return false,
            }
            up = up.wrapping_add(1);
        }
        let want = (SURFACES * KEYS_PER_SURFACE) as u64;
        self.dangling() == 0 && self.registers == want && self.releases == want
    }

    /// 逐面对平读数（双面各一项）。
    pub fn surface_table(&self) -> Vec<SurfaceReading> {
        let mut out = Vec::new();
        let mut up = 0u8;
        while (up as usize) < SURFACES {
            if let Ok(r) = self.reading(up) {
                out.push(r);
            }
            up = up.wrapping_add(1);
        }
        out
    }

    /// 终核销：双面闭合方可核销。
    pub fn write_off(&mut self) -> Result<u64, u32> {
        if self.frozen.is_some() {
            return Err(UL_EFROZEN);
        }
        if !self.closed_bidirectionally() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(UL_EOPEN);
        }
        let v = self.fold();
        self.frozen = Some(v);
        Ok(v)
    }

    /// 账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.entries.len() {
            h = (h ^ self.entries[i].fold()).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h = (h ^ self.registers).wrapping_mul(FNV_PRIME);
        h = (h ^ self.releases).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（核销即冻结；未核销也可冻结取当前指纹）。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen.is_none() {
            self.frozen = Some(self.fold());
        }
        self.frozen
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }
}

/// 标准装配：两面全键挂号 → 全键释放 → 终核销。
pub fn build_upstream() -> (UpstreamLedger, bool) {
    let mut l = UpstreamLedger::scaffold();
    let mut up = 0u8;
    while (up as usize) < SURFACES {
        let mut k = 0u32;
        while (k as usize) < KEYS_PER_SURFACE {
            if l.register(up, k).is_err() {
                return (l, false);
            }
            k = k.wrapping_add(1);
        }
        up = up.wrapping_add(1);
    }
    let mut up2 = 0u8;
    while (up2 as usize) < SURFACES {
        let mut k = 0u32;
        while (k as usize) < KEYS_PER_SURFACE {
            if l.release(up2, k).is_err() {
                return (l, false);
            }
            k = k.wrapping_add(1);
        }
        up2 = up2.wrapping_add(1);
    }
    let ok = l.write_off().is_ok();
    (l, ok)
}

/// 四类注入：面号越界 / 重复挂号 / 释放未挂号键 / 悬空键核销，全部须拒止。
pub fn inject_four() -> [bool; 4] {
    let mut l = UpstreamLedger::scaffold();

    // ① 面号越界
    let a = l.register(SURFACES as u8, 0).err() == Some(UL_EUP);

    // ② 重复挂号
    let reg = l.register(0, 1).is_ok();
    let b = reg && l.register(0, 1).err() == Some(UL_EDUP);

    // ③ 释放未挂号键
    let c = l.release(1, 0).err() == Some(UL_EMISS);

    // ④ 悬空键核销（挂而不释即核销）
    let mut d0 = UpstreamLedger::scaffold();
    let _ = d0.register(0, 0);
    let d = d0.write_off().err() == Some(UL_EOPEN) && d0.dangling() == 1;

    [a, b, c, d]
}

/// 判据台：上游消费核销账的十三条判据。
pub fn run_unx11192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11192");

    let (l, built) = build_upstream();
    let table = l.surface_table();
    let both_closed = table.len() == SURFACES && table.iter().all(|r| r.closed());

    // J1 上游账逐面终核销
    s.add(
        "J1-逐面终核销",
        built && l.is_frozen() && both_closed,
        "两面各自挂=释后终核销落定",
    );

    // J2 双向闭合
    s.add(
        "J2-双向闭合",
        l.closed_bidirectionally() && l.dangling() == 0,
        "挂=释=键数且无悬空键",
    );

    // J3 计数守恒
    let want = (SURFACES * KEYS_PER_SURFACE) as u64;
    s.add(
        "J3-计数守恒",
        l.registers == want && l.releases == want,
        "挂号与释放各满键数",
    );

    // J4 面内读数对平
    let ok4 = table
        .iter()
        .all(|r| r.registered == KEYS_PER_SURFACE as u64 && r.released == KEYS_PER_SURFACE as u64);
    s.add("J4-面内对平", ok4, "每面读数皆满键");

    // J5 冻结后拒改
    let mut f = l.clone();
    s.add(
        "J5-冻结拒改",
        f.register(0, 0).err() == Some(UL_EFROZEN)
            && f.release(0, 0).err() == Some(UL_EFROZEN)
            && f.write_off() == Err(UL_EFROZEN),
        "核销后挂/释/再核销三路皆拒",
    );

    // J6 重复登记拒止
    let mut d = UpstreamLedger::scaffold();
    let first = d.register(1, 3).is_ok();
    s.add(
        "J6-重复登记",
        first && d.register(1, 3).err() == Some(UL_EDUP) && d.registers == 1,
        "同键二次挂号拒止且计数不重复",
    );

    // J7 重复释放拒止
    let mut dd = UpstreamLedger::scaffold();
    let r1 = dd.register(0, 2).is_ok();
    let r2 = dd.release(0, 2).is_ok();
    s.add(
        "J7-重复释放",
        r1 && r2 && dd.release(0, 2).err() == Some(UL_EDOUBLE) && dd.releases == 1,
        "同键二次释放拒止",
    );

    // J8 释放未挂号拒止
    let mut m = UpstreamLedger::scaffold();
    s.add(
        "J8-释放未挂号",
        m.release(0, 5).err() == Some(UL_EMISS) && m.rejects == 1,
        "未挂号键不得释放且拒绝入账",
    );

    // J9 面号/键号越界
    let mut x = UpstreamLedger::scaffold();
    s.add(
        "J9-面号键号越界",
        x.register(SURFACES as u8, 0).err() == Some(UL_EUP)
            && x.register(0, KEYS_PER_SURFACE as u32).err() == Some(UL_EKEY),
        "面号与键号越界双路拒止",
    );

    // J10 半程核销拒止
    let mut h = UpstreamLedger::scaffold();
    let mut k = 0u32;
    while (k as usize) < KEYS_PER_SURFACE {
        let _ = h.register(0, k);
        let _ = h.release(0, k);
        k = k.wrapping_add(1);
    }
    s.add(
        "J10-半程拒核销",
        h.write_off() == Err(UL_EOPEN) && !h.closed_bidirectionally(),
        "仅一面闭合不得核销",
    );

    // J11 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i += 1;
    }
    s.add("J11-注入四拒", all, "越界/重复挂/未挂释放/悬空核销四路全拒");

    // J12 重算一致
    let (a1, ok1) = build_upstream();
    let (a2, ok2) = build_upstream();
    s.add(
        "J12-重算一致",
        ok1 && ok2 && a1.fold() == a2.fold(),
        "同装配两次指纹一致",
    );

    // J13 动态明细
    s.add_dyn(
        "J13-核销明细",
        l.rejects == 0 && l.dangling() == 0,
        &format!(
            "reg={} rel={} rejects={} root={:#x}",
            l.registers,
            l.releases,
            l.rejects,
            l.fold()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11192_upstream_flow() {
        let (l, ok) = build_upstream();
        assert!(ok);
        assert!(l.is_frozen());
        assert!(l.closed_bidirectionally());
        assert_eq!(l.dangling(), 0);
        assert_eq!(l.registers, (SURFACES * KEYS_PER_SURFACE) as u64);
        assert_eq!(l.surface_table().len(), SURFACES);
        assert!(inject_four().iter().all(|h| *h));

        let mut e = UpstreamLedger::new();
        assert_eq!(e.write_off(), Err(UL_EOPEN));
        assert_eq!(e.release(0, 0), Err(UL_EMISS));
    }

    #[test]
    fn unx11192_checks_green() {
        let s = run_unx11192_checks();
        assert!(s.all_passed(), "UNX-F11192 判据未全绿");
    }
}
